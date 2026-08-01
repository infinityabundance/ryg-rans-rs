//! # CLI cancellation: SIGINT / SIGTERM / timeout
//!
//! ## Purpose
//!
//! Long-running operations (decode, encode, verify over many blocks) must be
//! cancellable from outside: Ctrl-C (SIGINT), `kill`/service managers
//! (SIGTERM), or an explicit `--timeout`.  Without this, a stuck or huge
//! container forces a hard kill and leaves no typed error, and automation
//! cannot bound wall time.
//!
//! ## Design
//!
//! A single process-wide [`CancellationGuard`] owns the shared state:
//!
//! - Two `static AtomicBool`s (one for "a termination signal arrived", one for
//!   "timeout expired").  Signal handlers must be async-signal-safe; writing
//!   an `AtomicBool` with `Ordering::SeqCst` is async-signal-safe on the
//!   platforms we target (no locks, no allocation, no syscalls).
//! - Signal handlers are installed with `libc::signal` (SIGINT, SIGTERM).  The
//!   handler only stores the fact that a signal arrived; the operation loop
//!   polls [`CancellationGuard::check`] between blocks and returns a typed
//!   [`crate::error::AppError::Cancelled`] with the stable exit code 11.
//! - A watchdog thread sleeps for `--timeout` seconds and sets the timeout
//!   flag.  The guard drops and joins the watchdog when the operation
//!   completes early.
//!
//! ## Why polling, not longjmp or immediate exit
//!
//! Terminating the process inside a signal handler would bypass all typed
//! error paths, stream flushing, and the documented exit-code contract.  The
//! polling design keeps cancellation on the same typed error path as every
//! other failure, so exit-code semantics stay stable and observability (the
//! `error: cancelled: ...` line on stderr) is preserved.
//!
//! ## Threading and safety
//!
//! - The flag writes happen in the signal handler (async context) and in the
//!   watchdog thread; reads happen in the operation thread.  `AtomicBool` with
//!   `SeqCst` gives the required cross-thread visibility (the handler is never
//!   racing a torn read).
//! - Handlers are installed once per process (a static `Once`).  Restoring the
//!   default disposition on drop is best-effort: a process exiting anyway does
//!   not need it, and the operation returns a typed error promptly.
//! - `libc::signal` is used rather than `sigaction` because the handler only
//!   needs the classic two-state behavior; `SA_RESTART` semantics are
//!   irrelevant since we never interrupt a syscall from the handler.
//!
//! ## Platform support
//!
//! Signal installation is Unix-only and gated behind the `signals` feature
//! (`dep:libc`).  Without it, the guard is a no-op that never reports
//! signal cancellation, so the CLI still compiles and runs; `--timeout`
//! (pure `std::thread::sleep`) works everywhere.
//!
//! ## Tests
//!
//! The timeout path is exercised by the CLI integration tests (a tiny timeout
//! on a multi-block input must exit 11).  Signal delivery is exercised by the
//! integration test that spawns the CLI and sends SIGINT.

use crate::error::{AppError, CancelledError};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

#[cfg(all(target_family = "unix", feature = "signals"))]
use std::sync::Once;

/// Set by the SIGINT/SIGTERM handler.  Written in async-signal context;
/// read by the operation loop.
#[cfg(all(target_family = "unix", feature = "signals"))]
static SIGNALLED: AtomicBool = AtomicBool::new(false);
/// Set by the timeout watchdog thread.
static TIMED_OUT: AtomicBool = AtomicBool::new(false);
/// Guards single installation of the signal handlers.
#[cfg(all(target_family = "unix", feature = "signals"))]
static INSTALL: Once = Once::new();

/// RAII guard that installs signal handlers and (optionally) a timeout
/// watchdog, and tears them down when the operation completes.
pub struct CancellationGuard {
    /// Join handle of the timeout watchdog (None when no timeout requested).
    watchdog: Option<std::thread::JoinHandle<()>>,
}

impl CancellationGuard {
    /// Install handlers and start the timeout watchdog if `timeout_secs > 0`.
    ///
    /// `timeout_secs` is fractional seconds (e.g. `0.5` for half a second),
    /// so automation can bound wall time tightly; `0` disables the watchdog.
    pub fn install(timeout_secs: f64) -> Self {
        TIMED_OUT.store(false, Ordering::SeqCst);
        #[cfg(all(target_family = "unix", feature = "signals"))]
        {
            SIGNALLED.store(false, Ordering::SeqCst);
            INSTALL.call_once(|| unsafe {
                // SAFETY: the handlers are `extern "C"` functions that only
                // store to a static AtomicBool (async-signal-safe).
                // `libc::signal` is the documented portable installer for this
                // two-state pattern.
                libc::signal(
                    libc::SIGINT,
                    handle_sigint as *const () as libc::sighandler_t,
                );
                libc::signal(
                    libc::SIGTERM,
                    handle_sigterm as *const () as libc::sighandler_t,
                );
            });
        }
        let watchdog = if timeout_secs > 0.0 {
            Some(std::thread::spawn(move || {
                std::thread::sleep(Duration::from_secs_f64(timeout_secs));
                TIMED_OUT.store(true, Ordering::SeqCst);
            }))
        } else {
            None
        };
        CancellationGuard { watchdog }
    }

    /// True when a termination signal arrived or the timeout expired.
    pub fn is_cancelled() -> bool {
        let timed = TIMED_OUT.load(Ordering::SeqCst);
        #[cfg(all(target_family = "unix", feature = "signals"))]
        {
            timed || SIGNALLED.load(Ordering::SeqCst)
        }
        #[cfg(not(all(target_family = "unix", feature = "signals")))]
        {
            timed
        }
    }

    /// Return a typed `Cancelled` error when cancelled; `Ok(())` otherwise.
    ///
    /// Call this between blocks in long-running loops.  The error names the
    /// trigger (signal vs timeout) so operators can distinguish Ctrl-C from a
    /// wall-clock bound.
    pub fn check() -> Result<(), AppError> {
        #[cfg(all(target_family = "unix", feature = "signals"))]
        if SIGNALLED.load(Ordering::SeqCst) {
            return Err(AppError::Cancelled(CancelledError {
                detail: "interrupted by SIGINT/SIGTERM".into(),
            }));
        }
        if TIMED_OUT.load(Ordering::SeqCst) {
            return Err(AppError::Cancelled(CancelledError {
                detail: "timeout expired".into(),
            }));
        }
        Ok(())
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
    }
}

/// SIGINT handler: record the signal, nothing else (async-signal-safe).
#[cfg(all(target_family = "unix", feature = "signals"))]
extern "C" fn handle_sigint(_sig: libc::c_int) {
    SIGNALLED.store(true, Ordering::SeqCst);
}

/// SIGTERM handler: record the signal, nothing else (async-signal-safe).
#[cfg(all(target_family = "unix", feature = "signals"))]
extern "C" fn handle_sigterm(_sig: libc::c_int) {
    SIGNALLED.store(true, Ordering::SeqCst);
}
