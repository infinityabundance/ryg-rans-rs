//! # Fixed block planning — thread-count-independent block boundaries
//!
//! Block planning depends only on:
//! - Input length
//! - Configured block size
//! - Explicit format options
//! - Deterministic model mode
//!
//! It must NOT depend on:
//! - Thread count
//! - Queue state
//! - Task completion order
//! - CPU architecture
//! - Worker timing
//! - SIMD availability

/// A single planned block range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRange {
    /// 0-based block index.
    pub block_index: u64,
    /// Start offset in the input (inclusive).
    pub input_offset: u64,
    /// Length of this block in bytes.
    pub length: u64,
}

/// A complete fixed block plan.
///
/// Guarantees:
/// - Every input byte is covered exactly once.
/// - No gaps or overlaps.
/// - No dependency on thread count or timing.
#[derive(Debug, Clone)]
pub struct FixedBlockPlan {
    /// The planned block ranges in ascending index order.
    pub ranges: Vec<BlockRange>,
    /// Total input length that was planned.
    pub total_input_length: u64,
    /// Block size used for planning.
    pub block_size: u64,
    /// Version of the planning algorithm (for forensic reproducibility).
    pub planner_version: u8,
}

impl FixedBlockPlan {
    /// Plan blocks for the given input length and block size.
    ///
    /// The last block may be shorter than `block_size`; it is never empty
    /// unless the input itself is empty.
    ///
    /// # Panics
    ///
    /// Panics if `block_size == 0`.
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

    /// Number of planned blocks.
    pub fn block_count(&self) -> usize {
        self.ranges.len()
    }

    /// Returns true if no blocks were planned (empty input).
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input() {
        let plan = FixedBlockPlan::new(0, 4096);
        assert!(plan.is_empty());
        assert_eq!(plan.block_count(), 0);
    }

    #[test]
    fn test_single_block_exact() {
        let plan = FixedBlockPlan::new(4096, 4096);
        assert_eq!(plan.block_count(), 1);
        assert_eq!(plan.ranges[0].block_index, 0);
        assert_eq!(plan.ranges[0].input_offset, 0);
        assert_eq!(plan.ranges[0].length, 4096);
    }

    #[test]
    fn test_single_block_partial() {
        let plan = FixedBlockPlan::new(100, 4096);
        assert_eq!(plan.block_count(), 1);
        assert_eq!(plan.ranges[0].length, 100);
    }

    #[test]
    fn test_multiple_blocks() {
        let plan = FixedBlockPlan::new(10000, 4096);
        assert_eq!(plan.block_count(), 3);
        assert_eq!(plan.ranges[0].length, 4096);
        assert_eq!(plan.ranges[1].length, 4096);
        assert_eq!(plan.ranges[2].length, 10000 - 8192);
    }

    #[test]
    fn test_exact_coverage() {
        let plan = FixedBlockPlan::new(10000, 4096);
        let total: u64 = plan.ranges.iter().map(|r| r.length).sum();
        assert_eq!(total, 10000);
        // No gaps
        for i in 1..plan.ranges.len() {
            let prev_end = plan.ranges[i - 1].input_offset + plan.ranges[i - 1].length;
            assert_eq!(prev_end, plan.ranges[i].input_offset);
        }
    }

    #[test]
    fn test_large_input() {
        let plan = FixedBlockPlan::new(1024 * 1024 * 1024, 65536);
        assert_eq!(plan.block_count(), 16384);
        let total: u64 = plan.ranges.iter().map(|r| r.length).sum();
        assert_eq!(total, 1024 * 1024 * 1024);
    }

    #[test]
    fn test_version_is_stable() {
        let plan = FixedBlockPlan::new(100, 64);
        assert_eq!(plan.planner_version, 1);
    }
}
