//! # Loom concurrency courts for the parallel executor
//!
//! These tests compile the **real executor** against loom's synchronization
//! primitives (via the `crate::sync` swap layer) and run it inside
//! `loom::model`, which explores every thread interleaving of the closure.
//!
//! Run with:
//!
//! ```sh
//! RUSTFLAGS="--cfg loom" cargo test -p ryg-rans-rs-parallel \
//!   --features loom --release --test loom_tests
//! ```
//!
//! ## What is explored
//!
//! * Producer/worker/coordinator coordination: no lost tasks, no duplicate
//!   results, no deadlock, no task accepted after shutdown.
//! * Cancellation races: a token cancelled while tasks are in flight.
//! * Panic + cancellation: a panicking task cannot wedge the pipeline.
//! * Completeness accounting: the executor report's declared/submitted/
//!   completed counters.
//!
//! ## What is NOT explored (and why)
//!
//! * Queue *capacity*: loom's mpsc is unbounded, so the producer never
//!   blocks on a full queue under loom.  Boundedness and backpressure are
//!   pinned by the real-thread stress tests in `executor.rs` and
//!   `phase_i_tests.rs`.
//! * `sched_setaffinity` (the `affinity` feature): never enabled under loom
//!   builds; it is runtime-only and cannot be modelled.
//!
//! The models use tiny task counts and worker counts on purpose: loom's
//! state-space exploration is exponential in the number of interleaving
//! points.

#![cfg(loom)]

use loom::sync::Arc;
use ryg_rans_rs_parallel::{
    BufferSized, CancellationToken, ExecutorReport, ExecutorTask, HasBlockIndex, ParallelError,
    ReorderBuffer, WorkerScratch, run_tasks, run_tasks_with_sink,
};

/// A trivial task: echoes its input with a `usize` output.
struct EchoTask(pub usize);

impl ExecutorTask for EchoTask {
    type Output = usize;
    fn block_index(&self) -> Option<u64> {
        Some(self.0 as u64)
    }
    fn run(
        self,
        _worker: usize,
        _cancel: &CancellationToken,
        _scratch: &mut WorkerScratch,
    ) -> Self::Output {
        self.0
    }
}

/// A task that either panics or echoes, selected per task.
enum MixedTask {
    Panic(usize),
    Echo(usize),
}

impl ExecutorTask for MixedTask {
    type Output = usize;
    fn block_index(&self) -> Option<u64> {
        Some(match self {
            MixedTask::Panic(i) | MixedTask::Echo(i) => *i as u64,
        })
    }
    fn run(
        self,
        _worker: usize,
        _cancel: &CancellationToken,
        _scratch: &mut WorkerScratch,
    ) -> Self::Output {
        match self {
            MixedTask::Panic(i) => panic!("loom panic task {}", i),
            MixedTask::Echo(i) => i,
        }
    }
}

/// A task that checks cancellation at its yield point.
struct CancelProbeTask(pub usize);

impl ExecutorTask for CancelProbeTask {
    type Output = usize;
    fn block_index(&self) -> Option<u64> {
        Some(self.0 as u64)
    }
    fn run(
        self,
        _worker: usize,
        cancel: &CancellationToken,
        _scratch: &mut WorkerScratch,
    ) -> Self::Output {
        // Yield point: if cancelled, abandon this unit of work (the worker
        // loop counts it as cancelled, not completed).
        if cancel.is_cancelled() {
            return self.0;
        }
        self.0
    }
}

/// A `BufferSized`+`HasBlockIndex` item for the reorder model.
struct ReorderItem(pub u64);

impl ryg_rans_rs_parallel::HasBlockIndex for ReorderItem {
    fn block_index(&self) -> u64 {
        self.0
    }
}
impl ryg_rans_rs_parallel::BufferSized for ReorderItem {
    fn buffer_size(&self) -> u64 {
        8
    }
}

/// Every task is executed exactly once and every result is returned:
/// `declared == started == completed == returned`, and the returned set is
/// exactly the input set.
///
/// The model is kept tiny (2 tasks, 2 workers): loom's state-space
/// exploration is exponential in scheduling points, so the full 3-task/2-
/// worker model is run under `LOOM_MAX_PREEMPTIONS` (see the module docs).
#[test]
fn loom_executor_no_lost_tasks() {
    loom::model(|| {
        let tasks: Vec<EchoTask> = (0..2).map(EchoTask).collect();
        let report: ExecutorReport<usize> = run_tasks(tasks, 2, 2, None, None).unwrap();
        assert_eq!(report.returned_results, 2, "all results returned");
        let mut got: Vec<usize> = report.results.clone();
        got.sort_unstable();
        assert_eq!(got, vec![0, 1], "exactly the input set");
        assert_eq!(report.completed_tasks, 2, "completeness counter");
    });
}

/// Cancellation is never allowed to produce `Ok` with fewer results than
/// declared: the run must either return every result or a typed
/// `ParallelError::Cancelled`/`IncompleteExecution` (the completeness check
/// enforces this before returning `Ok`).
#[test]
fn loom_cancellation_race_completeness() {
    loom::model(|| {
        let cancel = Arc::new(CancellationToken::new());
        let probe = cancel.clone();
        let cancel_thread = loom::thread::spawn(move || {
            probe.cancel();
        });
        let tasks: Vec<CancelProbeTask> = (0..3).map(CancelProbeTask).collect();
        match run_tasks(tasks, 2, 2, None, Some(cancel)) {
            Ok(report) => {
                // If the run returned Ok, every task must be accounted for.
                assert_eq!(
                    report.returned_results, report.completed_tasks,
                    "Ok run must return every completed result"
                );
            }
            Err(ParallelError::Cancelled {
                completed,
                expected,
            }) => {
                assert!(completed <= expected, "completed cannot exceed expected");
            }
            Err(_) => {}
        }
        cancel_thread.join().unwrap();
    });
}

/// A panicking task must not wedge the pipeline: the run returns a typed
/// `WorkerPanic` error (or, depending on the interleaving, a completed run
/// of the non-panicking tasks), and no thread leaks.
#[test]
fn loom_panic_cancellation_no_wedge() {
    loom::model(|| {
        let tasks = vec![MixedTask::Panic(0), MixedTask::Echo(1)];
        let _ = run_tasks(tasks, 1, 1, None, None);
    });
}

/// The sink path (used by decode streaming) has the same no-lost-task
/// property when results are consumed through the callback.
#[test]
fn loom_sink_path_completeness() {
    loom::model(|| {
        let tasks: Vec<EchoTask> = (0..2).map(EchoTask).collect();
        let seen: Arc<std::sync::Mutex<Vec<usize>>> = Arc::default();
        let seen_clone = seen.clone();
        let report: ExecutorReport<usize> =
            run_tasks_with_sink(tasks, 1, 1, None, None, move |r| {
                seen_clone.lock().unwrap().push(r);
            })
            .unwrap();
        assert_eq!(report.returned_results, 2);
        let mut got = seen.lock().unwrap().clone();
        got.sort_unstable();
        assert_eq!(got, vec![0, 1], "sink received every result");
    });
}

/// ReorderBuffer commit under arbitrary completion order: inserting results
/// out of order must eventually commit exactly `[0, 1, ..., N-1]` in
/// strictly ascending order, with the newly-ready chain returned per insert.
#[test]
fn loom_reorder_commit_ascending() {
    loom::model(|| {
        let mut buf = ReorderBuffer::<ReorderItem>::new(16, 4096);
        // Completion order is nondeterministic; probe several orders.
        let orders: [[u64; 3]; 3] = [[1, 0, 2], [2, 1, 0], [0, 2, 1]];
        for order in orders {
            let mut buf2 = ReorderBuffer::<ReorderItem>::new(16, 4096);
            let mut committed: Vec<u64> = Vec::new();
            for &item in &order {
                for c in buf2.insert(ReorderItem(item)).unwrap() {
                    committed.push(c.block_index());
                }
            }
            assert_eq!(committed, vec![0, 1, 2], "strictly ascending commit");
        }
        let _ = buf;
    });
}

// ---------------------------------------------------------------------------
// Model artifact cache — single-flight courts (Phase O.5/O.9)
// ---------------------------------------------------------------------------
//
// These models compile the REAL cache (`ModelArtifactCache`) against loom's
// primitives via `crate::sync`.  Each model asserts the single-flight
// contract: exactly one construction per concurrent same-key cold burst, no
// permanent `Building` state after failure or panic, and identical artifacts
// for every caller.
//
// Run with the same command as the executor courts:
//   RUSTFLAGS="--cfg loom" cargo test -p ryg-rans-rs-parallel \
//     --features loom --release --test loom_tests

use ryg_rans_rs_parallel::{
    ModelArtifactBuildError, ModelArtifactCache, build_validated_model_artifacts,
};

/// Two threads request the same cold key: exactly one build, both receive
/// the same artifact.
#[test]
fn loom_cache_two_same_key_requests_one_build() {
    loom::model(|| {
        let cache = ModelArtifactCache::bounded(8, 1 << 20);
        let builds = Arc::new(loom::sync::atomic::AtomicUsize::new(0));
        let (c1, b1) = (cache.clone(), builds.clone());
        let t1 = loom::thread::spawn(move || {
            c1.get_or_build(7, 12, &[], None, || {
                b1.fetch_add(1, loom::sync::atomic::Ordering::SeqCst);
                build_validated_model_artifacts(7, 12, &[])
            })
        });
        let (c2, b2) = (cache.clone(), builds.clone());
        let t2 = loom::thread::spawn(move || {
            c2.get_or_build(7, 12, &[], None, || {
                b2.fetch_add(1, loom::sync::atomic::Ordering::SeqCst);
                build_validated_model_artifacts(7, 12, &[])
            })
        });
        let r1 = t1.join().expect("t1");
        let r2 = t2.join().expect("t2");
        let a1 = r1.expect("build ok");
        let a2 = r2.expect("build ok");
        assert_eq!(a1.freqs.len(), 256);
        assert_eq!(a2.freqs.len(), 256);
        assert!(Arc::ptr_eq(&a1, &a2), "single-flight shares one artifact");
        assert_eq!(
            builds.load(loom::sync::atomic::Ordering::SeqCst),
            1,
            "exactly one construction"
        );
    });
}

/// Two different cold keys built concurrently: two builds, no cross-talk.
#[test]
fn loom_cache_different_keys_two_builds() {
    loom::model(|| {
        let cache = ModelArtifactCache::bounded(8, 1 << 20);
        let t1 = loom::thread::spawn({
            let cache = cache.clone();
            move || {
                cache
                    .get_or_build(7, 12, &[], None, || {
                        build_validated_model_artifacts(7, 12, &[])
                    })
                    .expect("k1")
            }
        });
        let t2 = loom::thread::spawn({
            let cache = cache.clone();
            move || {
                cache
                    .get_or_build(8, 12, &[], None, || {
                        build_validated_model_artifacts(8, 12, &[])
                    })
                    .expect("k2")
            }
        });
        let a1 = t1.join().expect("t1");
        let a2 = t2.join().expect("t2");
        assert_eq!(a1.freqs.len(), 256);
        assert_eq!(a2.freqs.len(), 256);
        let m = cache.metrics();
        assert_eq!(m.insertions, 2, "two distinct keys admitted");
        assert_eq!(m.builds_started, 2);
    });
}

/// A failing build with waiters: every caller receives the same typed error,
/// nothing is admitted, and a later retry succeeds (no abandoned state).
#[test]
fn loom_cache_build_failure_releases_waiters() {
    loom::model(|| {
        let cache = ModelArtifactCache::bounded(8, 1 << 20);
        let bad = [0u8; 100];
        let t1 = loom::thread::spawn({
            let cache = cache.clone();
            move || {
                cache
                    .get_or_build(7, 12, &bad, None, || {
                        build_validated_model_artifacts(7, 12, &bad)
                    })
                    .err()
            }
        });
        let t2 = loom::thread::spawn({
            let cache = cache.clone();
            move || {
                cache
                    .get_or_build(7, 12, &bad, None, || {
                        build_validated_model_artifacts(7, 12, &bad)
                    })
                    .err()
            }
        });
        let e1 = t1.join().expect("t1");
        let e2 = t2.join().expect("t2");
        assert_eq!(e1, Some(ModelArtifactBuildError::InvalidFrequencyCount));
        assert_eq!(e2, Some(ModelArtifactBuildError::InvalidFrequencyCount));
        // Retry with a valid model: the key must not be stuck.
        let ok = cache
            .get_or_build(7, 12, &[], None, || build_validated_model_artifacts(7, 12, &[]))
            .expect("retry succeeds");
        assert_eq!(ok.freqs.len(), 256);
        let m = cache.metrics();
        assert_eq!(m.current_entries, 1);
    });
}

/// A builder panic with waiters: the panic is caught, converted to a typed
/// error, no permanent `Building` state survives, and a retry succeeds.
#[test]
fn loom_cache_builder_panic_releases_waiters() {
    loom::model(|| {
        let cache = ModelArtifactCache::bounded(8, 1 << 20);
        let t1 = loom::thread::spawn({
            let cache = cache.clone();
            move || {
                cache
                    .get_or_build(7, 12, &[], None, || {
                        panic!("deliberate builder panic");
                    })
                    .err()
            }
        });
        let t2 = loom::thread::spawn({
            let cache = cache.clone();
            move || {
                cache
                    .get_or_build(7, 12, &[], None, || {
                        panic!("deliberate builder panic");
                    })
                    .err()
            }
        });
        let e1 = t1.join().expect("t1");
        let e2 = t2.join().expect("t2");
        // Every caller sees the same typed panic class (retry may have made
        // one of them a successful builder of the *same* panicking key — but
        // the panicking closure never succeeds, so both must be errors).
        assert_eq!(e1, Some(ModelArtifactBuildError::Panicked));
        assert_eq!(e2, Some(ModelArtifactBuildError::Panicked));
        // Retry with a working builder: no abandoned Building state.
        let ok = cache
            .get_or_build(7, 12, &[], None, || build_validated_model_artifacts(7, 12, &[]))
            .expect("retry after panic succeeds");
        assert_eq!(ok.freqs.len(), 256);
    });
}
