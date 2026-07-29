//! # Cooperative cancellation token
//!
//! Workers must check cancellation at defined points.  Once cancelled,
//! no new expensive work begins, in-progress work may abort at checkpoints,
//! and all workers are still joined.

use core::sync::atomic::{AtomicBool, Ordering};

/// A thread-safe cancellation token.
///
/// Workers check `is_cancelled()` at defined yield points:
/// - Before beginning a block
/// - After expensive model construction
/// - Before encoding or decoding
/// - Before hashing
/// - Before returning a large result
#[derive(Debug)]
pub struct CancellationToken {
    cancelled: AtomicBool,
}

impl CancellationToken {
    /// Create a new cancellation token (not cancelled).
    pub fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
        }
    }

    /// Signal cancellation.  May be called from any thread.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Check whether cancellation has been signalled.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Check cancellation and return `Err(Cancelled)` if signalled.
    pub fn check(&self) -> Result<(), crate::ParallelError> {
        if self.is_cancelled() {
            Err(crate::ParallelError::Cancelled)
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
        let ct = std::sync::Arc::new(CancellationToken::new());
        let ct2 = ct.clone();
        let handle = std::thread::spawn(move || {
            ct2.cancel();
        });
        handle.join().unwrap();
        assert!(ct.is_cancelled());
    }
}
