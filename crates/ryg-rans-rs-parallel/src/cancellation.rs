//! # Cooperative cancellation token — thread-safe, lock-free
//!
//! ## Design
//!
//! The cancellation token is a single `AtomicBool` wrapped in a struct.
//! It uses `SeqCst` ordering (sequentially consistent) which is the
//! strongest memory ordering in the C++/Rust memory model.  This ensures
//! that every thread sees the cancellation signal immediately, even on
//! weakly-ordered architectures like ARM.
//!
//! ## Why cooperative?
//!
//! True thread cancellation (e.g., `pthread_cancel`, `std::thread::kill`)
//! is unsafe in Rust because it can interrupt a thread while holding a
//! mutex or allocating memory, leading to deadlocks or corrupted state.
//! Instead, the token uses **cooperative cancellation**:
//!
//! 1. An external caller (or another thread) calls `cancel()`.
//! 2. Workers check `is_cancelled()` at defined yield points.
//! 3. When a worker detects cancellation, it stops processing and returns
//!    early (usually with a `ParallelError::Cancelled`).
//! 4. All workers are still joined normally — no forceful termination.
//!
//! ## Yield points
//!
//! Workers check cancellation at these points:
//!
//! - Before beginning a new block (no work started → cheap abort).
//! - After expensive model construction (don't proceed with a plan
//!   that won't be used).
//! - Before encoding or decoding inner loop (don't enter hot path).
//! - Before hashing (SHA-256 is CPU-intensive).
//! - Before returning a large result (allocation is pure overhead if
//!   cancelled).
//!
//! ## `check()` vs `is_cancelled()`
//!
//! - `is_cancelled()`: Returns a plain `bool`.  Use this inside hot loops
//!   where you want to check cheaply without error-handling overhead.
//!
//! - `check()`: Returns `Result<(), ParallelError>`.  Use this at yield
//!   points where you want to use the `?` operator for early return.
//!   If cancelled, returns `Err(ParallelError::Cancelled)`.
//!
//! ## External vs internal tokens
//!
//! - **External**: Created by the caller and passed to the parallel engine.
//!   The caller can cancel from another thread (e.g., on SIGINT or timeout).
//! - **Internal**: Used by the executor to signal cancellation to workers
//!   when a fatal error is detected (e.g., a worker panicked).
//!
//! Both types use the same `CancellationToken` type.
//!
//! ## Thread safety
//!
//! `CancellationToken` is `Sync + Send` because `AtomicBool` provides
//! atomic operations without mutable state.  `cancel()` takes `&self`,
//! not `&mut self`, so no mutable reference is needed.  This allows
//! cancellation from any thread holding a shared reference (e.g., via
//! `Arc<CancellationToken>`).

use crate::sync::{AtomicBool, Ordering};

/// A thread-safe, lock-free cooperative cancellation token.
///
/// # Usage
///
/// ```ignore
/// let token = Arc::new(CancellationToken::new());
/// let t = token.clone();
/// thread::spawn(move || {
///     while !t.is_cancelled() {
///         // do work
///     }
/// });
/// // Later, from another thread:
/// token.cancel();
/// ```
///
/// # Memory ordering
///
/// Uses `SeqCst` (sequentially consistent) ordering, which provides a
/// single total order of all atomic operations across all threads.
/// This is the safest (though slightly slower) choice.  On x86/x86_64,
/// `SeqCst` maps to `MFENCE` + `MOV` or `LOCK XCHG`; on ARM64, it maps
/// to `LDAR`/`STLR`.  For a cancellation flag that is checked at most
/// a few times per block, the performance difference vs `Acquire`/`Release`
/// is negligible.
///
/// # Idempotency
///
/// Calling `cancel()` multiple times is safe and idempotent.  Once the
/// flag is set to `true`, subsequent stores are no-ops.
#[derive(Debug)]
pub struct CancellationToken {
    /// Atomic flag: `true` = cancellation requested.
    cancelled: AtomicBool,
}

impl CancellationToken {
    /// Create a new cancellation token in the uncancelled state.
    ///
    /// The initial state is `false` — no cancellation has been requested.
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
        }
    }

    /// Signal cancellation.
    ///
    /// May be called from any thread, including from signal handlers
    /// (as long as the signal handler does not call non-async-signal-safe
    /// functions).  The store is `SeqCst` to ensure immediate visibility.
    ///
    /// # Idempotency
    ///
    /// Safe to call multiple times.  After the first call, subsequent
    /// calls are no-ops.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Check whether cancellation has been signalled.
    ///
    /// Returns `true` if `cancel()` was called on this or any clone.
    /// This is a **non-blocking** read — no memory allocation, no locks.
    ///
    /// Use this in hot loops or before expensive operations to avoid
    /// error-handling overhead.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Check cancellation and return `Err(ParallelError::Cancelled)` if
    /// cancellation was signalled.
    ///
    /// This is a convenience wrapper around `is_cancelled()` that returns
    /// a `Result` for use with the `?` operator.  Equivalent to:
    ///
    /// ```ignore
    /// if token.is_cancelled() {
    ///     return Err(ParallelError::Cancelled);
    /// }
    /// ```
    pub fn check(&self) -> Result<(), crate::ParallelError> {
        if self.is_cancelled() {
            // The check() API is used at cooperative yield points where the
            // completion counts are not yet known.  Callers that need exact
            // counts use the high-level `_with_cancel` APIs, which report
            // Cancelled { completed, expected } from the executor report.
            Err(crate::ParallelError::Cancelled {
                completed: 0,
                expected: 0,
            })
        } else {
            Ok(())
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_cancelled_by_default() {
        let ct = CancellationToken::new();
        assert!(!ct.is_cancelled());
        assert!(ct.check().is_ok());
    }

    #[test]
    fn test_cancel() {
        let ct = CancellationToken::new();
        ct.cancel();
        assert!(ct.is_cancelled());
        assert!(ct.check().is_err());
    }

    #[test]
    fn test_cancel_from_another_thread() {
        let ct = crate::sync::Arc::new(CancellationToken::new());
        let ct2 = ct.clone();
        let handle = std::thread::spawn(move || {
            ct2.cancel();
        });
        handle.join().unwrap();
        assert!(ct.is_cancelled());
    }
}
