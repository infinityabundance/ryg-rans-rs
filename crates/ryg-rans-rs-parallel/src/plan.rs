//! # Fixed block planning — thread-count-independent block boundaries
//!
//! ## Determinism invariant
//!
//! Block boundaries are a pure function of `(input_length, block_size)`.
//! Thread count, worker count, and runtime scheduling **never** affect
//! where block boundaries fall.  This ensures that two runs with different
//! thread counts produce identical block decompositions and therefore
//! identical per-block work.
//!
//! ## How boundaries are computed
//!
//! ```text
//! num_full_blocks = input_length / block_size
//! remainder       = input_length % block_size
//! total_blocks    = num_full_blocks + (1 if remainder > 0 else 0)
//!
//! Block 0:       offset = 0,                  length = block_size
//! Block 1:       offset = block_size,          length = block_size
//! ...
//! Block N-1:     offset = N * block_size,      length = block_size
//! Block N:       offset = N * block_size,      length = remainder  (if > 0)
//! ```
//!
//! The last block may be shorter than `block_size` (the remainder block).
//! This is by design — it avoids padding the input and ensures that the
//! total output length equals the input length.
//!
//! ## Why thread count doesn't affect boundaries
//!
//! A common mistake in parallel processing is to partition the input by
//! worker count (e.g., each worker gets `input_length / num_workers`
//! bytes).  This makes boundaries non-deterministic when thread count
//! varies.  The `FixedBlockPlan` uses a **fixed block size** instead,
//! so boundaries are independent of the number of workers.
//!
//! Workers consume blocks from a shared queue.  A worker with 4 threads
//! processes blocks 4 at a time; a worker with 8 threads processes 8 at
//! a time — but the blocks themselves are the same size and start at the
//! same offsets.
//!
//! ## Empty input
//!
//! If `input_length == 0`, the plan contains zero blocks.  The executor
//! handles this as a no-op: no workers are dispatched, and an empty
//! output is produced immediately.

use std::vec::Vec;

/// A single planned block range — the byte span of one block in the input.
///
/// # Fields
///
/// - `block_index`: 0-based index (primary key).  This is the only
///   stable identifier for a block across runs.
/// - `input_offset`: Starting byte offset in the original input stream.
///   Consecutive ranges have no gaps: `range[i].input_offset + range[i].length
///   == range[i+1].input_offset`.
/// - `length`: Number of bytes in this block.  Equals `block_size` for
///   all blocks except possibly the last (remainder block).
///
/// # Invariant
///
/// The sum of all `length` values equals `total_input_length`.  There
/// are no gaps and no overlaps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRange {
    /// 0-based block index.
    pub block_index: u64,
    /// Starting byte offset in the original input stream.
    pub input_offset: u64,
    /// Length of this block in bytes.
    pub length: u64,
}

/// A complete fixed block plan — the immutable result of planning.
///
/// # Fields
///
/// - `ranges`: The ordered list of block ranges.  Guaranteed to cover
///   the entire input without gaps or overlaps.
/// - `total_input_length`: The original input length.  Used for sanity
///   checks in the executor.
/// - `block_size`: The configured block size.  All blocks (except possibly
///   the last) have this length.
/// - `planner_version`: A monotonically increasing version number.
///   Incremented when the planning algorithm changes.  Used to detect
///   plans created with different planner logic in tests.
///
/// # Invariant
///
/// The plan is **immutable** after construction.  No field changes after
/// creation.  The executor reads the plan but never modifies it.
///
/// # Empty plan
///
/// If `total_input_length == 0`, `ranges` is empty and `block_count()`
/// returns 0.  The executor handles this as a no-op.
#[derive(Debug, Clone)]
pub struct FixedBlockPlan {
    /// Ordered list of block ranges covering the entire input.
    pub ranges: Vec<BlockRange>,
    /// Total input length in bytes.
    pub total_input_length: u64,
    /// Configured block size (all blocks except last are this size).
    pub block_size: u64,
    /// Planner algorithm version (currently 1).
    pub planner_version: u8,
}

impl FixedBlockPlan {
    /// Create a new fixed block plan from the input length and block size.
    ///
    /// # Panics
    ///
    /// Panics if `block_size == 0`.  Block size must be positive.
    ///
    /// # Determinism
    ///
    /// This function is deterministic: same `(input_length, block_size)`
    /// always produces the same plan, regardless of thread count,
    /// scheduling, or any other runtime state.
    ///
    /// # Complexity
    ///
    /// O(N) where N = total_blocks.  The plan allocates a `Vec<BlockRange>`
    /// with the exact capacity needed (`total_blocks`).  For typical block
    /// sizes (4–64 KiB) and input sizes (MiB–GiB), this is trivially fast.
    pub fn new(input_length: u64, block_size: u64) -> Self {
        assert!(block_size > 0, "block_size must be > 0");
        if input_length == 0 {
            return Self {
                ranges: Vec::new(),
                total_input_length: 0,
                block_size,
                planner_version: 1,
            };
        }
        let num_full = input_length / block_size;
        let remainder = input_length % block_size;
        let total_blocks = num_full + if remainder > 0 { 1 } else { 0 };
        let mut ranges = Vec::with_capacity(total_blocks as usize);
        for i in 0..num_full {
            ranges.push(BlockRange {
                block_index: i,
                input_offset: i * block_size,
                length: block_size,
            });
        }
        if remainder > 0 {
            ranges.push(BlockRange {
                block_index: num_full,
                input_offset: num_full * block_size,
                length: remainder,
            });
        }
        Self {
            ranges,
            total_input_length: input_length,
            block_size,
            planner_version: 1,
        }
    }

    /// Return the number of blocks in this plan.
    ///
    /// This is the count of block ranges.  Returns 0 for empty input.
    pub fn block_count(&self) -> usize {
        self.ranges.len()
    }

    /// Whether the plan contains zero blocks (empty input).
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

/// Decode plan types for model-aware backend selection.
pub use super::decode_plan::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        let p = FixedBlockPlan::new(0, 4096);
        assert!(p.is_empty());
        assert_eq!(p.block_count(), 0);
    }
    #[test]
    fn test_single() {
        let p = FixedBlockPlan::new(4096, 4096);
        assert_eq!(p.block_count(), 1);
        assert_eq!(p.ranges[0].length, 4096);
    }
    #[test]
    fn test_coverage() {
        let p = FixedBlockPlan::new(10000, 4096);
        let t: u64 = p.ranges.iter().map(|r| r.length).sum();
        assert_eq!(t, 10000);
    }
    #[test]
    fn test_no_gaps() {
        let p = FixedBlockPlan::new(10000, 4096);
        for i in 1..p.ranges.len() {
            let prev = p.ranges[i - 1].input_offset + p.ranges[i - 1].length;
            assert_eq!(prev, p.ranges[i].input_offset);
        }
    }
    #[test]
    fn test_version() {
        assert_eq!(FixedBlockPlan::new(100, 64).planner_version, 1);
    }
}
