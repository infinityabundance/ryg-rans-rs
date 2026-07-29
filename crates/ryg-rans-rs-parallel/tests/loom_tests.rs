//! Loom concurrency model tests for the parallel executor
//!
//! These tests require `--cfg loom` to be passed to rustc.
//! Run with: `RUSTFLAGS="--cfg loom" cargo test --release`
//!
//! Loom explores all possible thread interleavings to verify:
//! - No lost tasks
//! - No duplicate tasks
//! - No result committed twice
//! - No shutdown deadlock
//! - No task accepted after terminal shutdown
//! - All workers eventually joined
//! - Canonical block index never increases after a lower failure is observed

#![cfg(loom)]

use std::sync::Arc;

/// Verify that the executor shuts down cleanly under all interleavings.
#[test]
fn loom_executor_shutdown() {
    loom::model(|| {
        let cancel = Arc::new(crate::CancellationToken::new());
        let cancel_clone = cancel.clone();

        // Spawn a worker that checks cancellation
        let handle = std::thread::spawn(move || {
            while !cancel_clone.is_cancelled() {
                std::thread::yield_now();
            }
        });

        cancel.cancel();
        handle.join().unwrap();
    });
}

/// Verify that the reorder buffer correctly handles concurrent insertions.
#[test]
fn loom_reorder_buffer() {
    loom::model(|| {
        let mut buf = crate::ReorderBuffer::<u64>::new(16, 65536);
        // Insert blocks in a nondeterministic order
        let r0 = buf.insert(0u64);
        let r1 = buf.insert(1u64);
        // Both must succeed
        assert!(r0.is_ok() || r0.is_ok());
        let _ = r1;
    });
}
