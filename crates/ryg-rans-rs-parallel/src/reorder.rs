//! # Bounded ordered result buffer
//!
//! Workers may finish in any order.  The ordered commit requires results in
//! ascending block-index order.  This module provides a bounded reorder buffer
//! that collects out-of-order results and emits them only when the next
//! sequential block is available.
//!
//! ## Bounds
//!
//! The buffer enforces:
//! - Maximum number of buffered blocks (count-based backpressure)
//! - Maximum total buffered decoded bytes (memory-based backpressure)
//!
//! A slow early block causes producer backpressure rather than unbounded growth.
//! Once the buffer is full, insertions block until a slot frees up (via commit).

use crate::error::{BlockError, BlockErrorKind};
use std::collections::BTreeMap;

/// A bounded reorder buffer keyed by block index.
///
/// `T` is the type of result to reorder (e.g., `EncodedBlockResult` or `DecodedBlockResult`).
/// The buffer expects results to have a `block_index: u64` field accessible via the
/// `HasBlockIndex` trait.
#[derive(Debug)]
pub struct ReorderBuffer<T> {
    /// Storage for out-of-order results, keyed by block_index.
    pending: BTreeMap<u64, T>,
    /// The next expected block index (0-based).
    next_expected: u64,
    /// Maximum number of buffered blocks.
    max_blocks: usize,
    /// Maximum total bytes buffered.
    max_bytes: u64,
    /// Current total bytes buffered (sum of sizes using the `BufferSized` trait).
    current_bytes: u64,
}

/// Trait for types that have a block index.
pub trait HasBlockIndex {
    fn block_index(&self) -> u64;
}

/// Trait for types that report their buffered size in bytes.
pub trait BufferSized {
    fn buffer_size(&self) -> u64;
}

impl<T> ReorderBuffer<T>
where
    T: HasBlockIndex + BufferSized,
{
    /// Create a new reorder buffer.
    pub fn new(max_blocks: usize, max_bytes: u64) -> Self {
        Self {
            pending: BTreeMap::new(),
            next_expected: 0,
            max_blocks,
            max_bytes,
            current_bytes: 0,
        }
    }

    /// Insert a result into the buffer.
    ///
    /// Returns `Ok(Some(result))` if the result is the next expected block and
    /// should be committed immediately.
    /// Returns `Ok(None)` if the result is buffered for later commit.
    /// Returns `Err` if the result is for an already-committed block (duplicate).
    pub fn insert(&mut self, result: T) -> Result<Option<T>, BlockError> {
        let idx = result.block_index();

        // Check for already-committed block (index < next_expected)
        if idx < self.next_expected {
            return Err(BlockError {
                block_index: idx,
                kind: BlockErrorKind::OutputCommit,
            });
        }

        // Check for duplicate in-flight
        if self.pending.contains_key(&idx) {
            return Err(BlockError {
                block_index: idx,
                kind: BlockErrorKind::OutputCommit,
            });
        }

        // If this is the next expected block, return it immediately
        if idx == self.next_expected {
            self.next_expected += 1;
            return Ok(Some(result));
        }

        // Otherwise, buffer it.  Check bounds first.
        let size = result.buffer_size();
        if self.pending.len() >= self.max_blocks {
            return Err(BlockError {
                block_index: idx,
                kind: BlockErrorKind::ResourceLimit,
            });
        }
        if self.current_bytes + size > self.max_bytes {
            return Err(BlockError {
                block_index: idx,
                kind: BlockErrorKind::ResourceLimit,
            });
        }

        self.current_bytes += size;
        self.pending.insert(idx, result);
        Ok(None)
    }

    /// Drain all consecutively available results from the buffer.
    /// Returns results in ascending block-index order.
    pub fn drain_ready(&mut self) -> Vec<T> {
        let mut ready = Vec::new();
        while let Some(result) = self.pending.remove(&self.next_expected) {
            let size = result.buffer_size();
            self.current_bytes = self.current_bytes.saturating_sub(size);
            self.next_expected += 1;
            ready.push(result);
        }
        ready
    }

    /// Number of buffered results.
    pub fn buffered_count(&self) -> usize {
        self.pending.len()
    }

    /// Current total bytes buffered.
    pub fn buffered_bytes(&self) -> u64 {
        self.current_bytes
    }

    /// The next expected block index.
    pub fn next_expected(&self) -> u64 {
        self.next_expected
    }

    /// Whether all results have been committed (empty and no pending).
    pub fn is_complete(&self) -> bool {
        self.pending.is_empty()
    }
}

// Blanket implementations for common types

impl HasBlockIndex for u64 {
    fn block_index(&self) -> u64 {
        *self
    }
}

impl BufferSized for u64 {
    fn buffer_size(&self) -> u64 {
        8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestResult {
        index: u64,
        size: u64,
    }

    impl HasBlockIndex for TestResult {
        fn block_index(&self) -> u64 {
            self.index
        }
    }

    impl BufferSized for TestResult {
        fn buffer_size(&self) -> u64 {
            self.size
        }
    }

    #[test]
    fn test_sequential_insert() {
        let mut buf = ReorderBuffer::new(16, 65536);
        for i in 0..5 {
            let r = buf.insert(TestResult { index: i, size: 10 }).unwrap();
            assert!(r.is_some(), "sequential insert {} should return ready", i);
        }
        assert!(buf.is_complete());
    }

    #[test]
    fn test_out_of_order_insert() {
        let mut buf = ReorderBuffer::new(16, 65536);
        // Insert 1 before 0 (order 1, 0, 2)
        assert!(
            buf.insert(TestResult { index: 1, size: 10 })
                .unwrap()
                .is_none()
        );
        assert_eq!(buf.buffered_count(), 1);
        // Insert 0: should return ready, and also drain 1
        let r0 = buf.insert(TestResult { index: 0, size: 10 }).unwrap();
        assert!(r0.is_some());
        // After draining 0, 1 should also be ready
        let drained = buf.drain_ready();
        assert_eq!(drained.len(), 1); // block 1
        assert!(buf.is_complete());
    }

    #[test]
    fn test_reverse_order() {
        let mut buf = ReorderBuffer::new(16, 65536);
        // Insert 4,3,2,1,0 (reverse order)
        for i in (1..5).rev() {
            let r = buf.insert(TestResult { index: i, size: 10 }).unwrap();
            assert!(r.is_none(), "block {} should buffer (not next)", i);
        }
        assert_eq!(buf.buffered_count(), 4);
        // Now insert 0 — it IS next_expected (0), so it should return Some
        let r0 = buf.insert(TestResult { index: 0, size: 10 }).unwrap();
        assert!(r0.is_some(), "block 0 should be ready");
        // Drain should return the 4 buffered blocks (1,2,3,4)
        let drained = buf.drain_ready();
        assert_eq!(drained.len(), 4);
        assert!(buf.is_complete());
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut buf = ReorderBuffer::new(16, 65536);
        buf.insert(TestResult { index: 0, size: 10 }).unwrap();
        let dup = buf.insert(TestResult { index: 0, size: 10 });
        assert!(dup.is_err());
    }

    #[test]
    fn test_buffer_full() {
        let mut buf = ReorderBuffer::new(2, 65536);
        // Insert block 2 and 3 (buffer them, since 0 is expected)
        buf.insert(TestResult { index: 2, size: 10 }).unwrap();
        buf.insert(TestResult { index: 3, size: 10 }).unwrap();
        // Buffer is full (max_blocks = 2)
        let r = buf.insert(TestResult { index: 4, size: 10 });
        assert!(r.is_err());
    }

    #[test]
    fn test_empty_buffer() {
        let mut buf: ReorderBuffer<TestResult> = ReorderBuffer::new(16, 65536);
        assert!(buf.is_complete());
        assert_eq!(buf.buffered_count(), 0);
    }
}
