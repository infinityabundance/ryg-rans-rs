//! # Fixed block planning — thread-count-independent block boundaries
//!
//! Block planning depends only on input length and configured block size.

use std::vec::Vec;

/// A single planned block range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockRange {
    pub block_index: u64,
    pub input_offset: u64,
    pub length: u64,
}

/// A complete fixed block plan.
#[derive(Debug, Clone)]
pub struct FixedBlockPlan {
    pub ranges: Vec<BlockRange>,
    pub total_input_length: u64,
    pub block_size: u64,
    pub planner_version: u8,
}

impl FixedBlockPlan {
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

    pub fn block_count(&self) -> usize {
        self.ranges.len()
    }
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
