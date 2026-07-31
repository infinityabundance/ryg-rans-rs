//! # CPU affinity for worker threads
//!
//! Applies the configured [`AffinityPolicy`] to individual worker threads.
//!
//! ## Platform support
//!
//! - **Linux**: implemented via `libc::sched_setaffinity`.  The affinity is
//!   applied at the top of each worker thread, before any task executes.
//! - **Other platforms**: `Compact`, `Spread`, and `Explicit` return a typed
//!   [`ParallelError::Config`] — they are never silently ignored.  `None`
//!   is a no-op everywhere.
//!
//! ## Policy semantics
//!
//! - `None`: no affinity is set; the scheduler manages placement.
//! - `Compact`: worker `i` is pinned to CPU `i % online_cpus`.
//! - `Spread`: worker `i` is pinned to CPU `(i * stride) % online_cpus`
//!   where `stride = online_cpus / total_workers`, spreading workers across
//!   distinct cores when possible.
//! - `Explicit(list)`: worker `i` is pinned to `list[i % list.len()]`.
//!   A non-empty list is required; an empty list is a typed config error.
//!
//! Affinity never affects canonical output — it only changes scheduling.

use crate::config::AffinityPolicy;
use crate::error::ParallelError;

/// Online logical CPU count (Linux).  Falls back to 1 on other platforms.
fn online_cpus() -> usize {
    #[cfg(target_os = "linux")]
    {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    }
    #[cfg(not(target_os = "linux"))]
    {
        1
    }
}

/// Compute the target CPU index for worker `worker_index` under `policy`.
fn target_cpu(
    policy: &AffinityPolicy,
    worker_index: usize,
    total_workers: usize,
) -> Result<Option<usize>, ParallelError> {
    match policy {
        AffinityPolicy::None => Ok(None),
        AffinityPolicy::Compact => {
            let n = online_cpus();
            Ok(Some(worker_index % n))
        }
        AffinityPolicy::Spread => {
            let n = online_cpus();
            let stride = (n / total_workers.max(1)).max(1);
            Ok(Some((worker_index * stride) % n))
        }
        AffinityPolicy::Explicit(cpus) => {
            if cpus.is_empty() {
                return Err(ParallelError::Config(
                    "AffinityPolicy::Explicit requires a non-empty CPU list".into(),
                ));
            }
            Ok(Some(cpus[worker_index % cpus.len()]))
        }
    }
}

/// Validate the affinity policy before any workers are spawned.
///
/// Returns a typed [`ParallelError::Config`] for unsupported platforms or
/// invalid explicit CPU lists.  Never silently ignored.
pub fn validate_affinity_policy(policy: &AffinityPolicy) -> Result<(), ParallelError> {
    match policy {
        AffinityPolicy::None => Ok(()),
        #[cfg(all(target_os = "linux", feature = "affinity"))]
        AffinityPolicy::Compact | AffinityPolicy::Spread => Ok(()),
        #[cfg(not(all(target_os = "linux", feature = "affinity")))]
        AffinityPolicy::Compact | AffinityPolicy::Spread => Err(ParallelError::Config(
            "affinity policies other than None require the 'affinity' feature on Linux".into(),
        )),
        AffinityPolicy::Explicit(cpus) => {
            if cpus.is_empty() {
                return Err(ParallelError::Config(
                    "AffinityPolicy::Explicit requires a non-empty CPU list".into(),
                ));
            }
            #[cfg(not(all(target_os = "linux", feature = "affinity")))]
            {
                return Err(ParallelError::Config(
                    "affinity policies other than None require the 'affinity' feature on Linux"
                        .into(),
                ));
            }
            #[cfg(all(target_os = "linux", feature = "affinity"))]
            {
                Ok(())
            }
        }
    }
}

/// Apply the affinity policy to the calling thread for `worker_index`.
///
/// Call at the very top of each worker thread.  Returns a typed error for
/// unsupported platforms or invalid explicit CPU lists — never silently
/// ignored.
///
/// Requires the `affinity` cargo feature (Linux).  Without the feature,
/// non-`None` policies return a typed config error.
pub fn apply_worker_affinity(
    policy: &AffinityPolicy,
    worker_index: usize,
    total_workers: usize,
) -> Result<(), ParallelError> {
    #[cfg(all(target_os = "linux", feature = "affinity"))]
    {
        let Some(cpu) = target_cpu(policy, worker_index, total_workers)? else {
            return Ok(());
        };
        // SAFETY: sched_setaffinity takes a pid of 0 to mean "calling
        // thread".  We build a cpu_set_t with exactly one CPU set.  The
        // pointer arithmetic on the byte array is standard for the libc
        // CPU_SET macros which this mirrors.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_SET(cpu, &mut set);
            let ret = libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set);
            if ret != 0 {
                let err = std::io::Error::last_os_error();
                return Err(ParallelError::Config(format!(
                    "sched_setaffinity(cpu={}) failed: {}",
                    cpu, err
                )));
            }
        }
        Ok(())
    }
    #[cfg(not(all(target_os = "linux", feature = "affinity")))]
    {
        match policy {
            AffinityPolicy::None => Ok(()),
            _ => Err(ParallelError::Config(
                "affinity policies other than None require the 'affinity' feature on Linux".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_target_cpu_none() {
        assert_eq!(target_cpu(&AffinityPolicy::None, 0, 4).unwrap(), None);
    }

    #[test]
    fn test_target_cpu_compact() {
        let c = target_cpu(&AffinityPolicy::Compact, 0, 4).unwrap().unwrap();
        assert!(c < online_cpus());
    }

    #[test]
    fn test_target_cpu_explicit_empty_rejected() {
        let r = target_cpu(&AffinityPolicy::Explicit(vec![]), 0, 4);
        assert!(r.is_err(), "empty explicit CPU list must be a typed error");
    }

    #[test]
    fn test_target_cpu_explicit_round_robin() {
        let c = target_cpu(&AffinityPolicy::Explicit(vec![3, 7]), 3, 4)
            .unwrap()
            .unwrap();
        assert_eq!(c, 7, "worker 3 % 2 = 1 → cpus[1] = 7");
    }
}
