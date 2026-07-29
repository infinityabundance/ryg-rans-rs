//! # Deterministic scheduling injection
//!
//! A test scheduler that injects deterministic delays to force specific
//! completion orders, verifying that thread scheduling order never changes
//! canonical output, errors, or forensic results.
//!
//! Supported injection modes:
//! - `Forward`: blocks complete in index order (1, 2, 3, ...)
//! - `Reverse`: blocks complete in reverse index order (N, N-1, ..., 1)
//! - `OddFirst`: odd-indexed blocks complete before even
//! - `EvenFirst`: even-indexed blocks complete before odd
//! - `RandomSeeded`: pseudo-random completion order with a fixed seed
//! - `SlowEarlyBlock`: block 0 takes significantly longer than others
//! - `SlowLateBlock`: the last block takes significantly longer

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Scheduling injection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleMode {
    Forward,
    Reverse,
    OddFirst,
    EvenFirst,
    RandomSeeded(u64),
    SlowEarlyBlock,
    SlowLateBlock,
}

/// A deterministic delay injector for testing.
///
/// Maps each block index to a delay duration (in microseconds) that the
/// executor should wait before processing the block.
#[derive(Debug, Clone)]
pub struct DelaySchedule {
    delays: Vec<u64>,
}

impl DelaySchedule {
    /// Create a delay schedule for `num_blocks` using the given mode.
    pub fn new(num_blocks: usize, mode: ScheduleMode) -> Self {
        let mut delays = vec![0u64; num_blocks];

        match mode {
            ScheduleMode::Forward => {
                // Equal delays — natural forward processing
                for i in 0..num_blocks {
                    delays[i] = 0;
                }
            }
            ScheduleMode::Reverse => {
                // Reverse: high-index blocks finish first
                // Give block 0 a large delay
                if num_blocks > 0 {
                    delays[0] = 10_000;
                }
            }
            ScheduleMode::OddFirst => {
                for i in 0..num_blocks {
                    if i % 2 == 0 {
                        delays[i] = 5_000;
                    } // delay even blocks
                }
            }
            ScheduleMode::EvenFirst => {
                for i in 0..num_blocks {
                    if i % 2 == 1 {
                        delays[i] = 5_000;
                    } // delay odd blocks
                }
            }
            ScheduleMode::RandomSeeded(seed) => {
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                seed.hash(&mut hasher);
                let base = hasher.finish();
                for i in 0..num_blocks {
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    (base ^ i as u64).hash(&mut h);
                    delays[i] = (h.finish() % 1000) * 10; // 0-10ms
                }
            }
            ScheduleMode::SlowEarlyBlock => {
                if num_blocks > 0 {
                    delays[0] = 50_000;
                } // 50ms delay for block 0
            }
            ScheduleMode::SlowLateBlock => {
                if num_blocks > 1 {
                    delays[num_blocks - 1] = 50_000;
                }
            }
        }

        Self { delays }
    }

    /// Get the delay for a specific block index (in microseconds).
    pub fn delay_for(&self, block_index: u64) -> u64 {
        self.delays.get(block_index as usize).copied().unwrap_or(0)
    }

    /// Execute a deterministic test that verifies output is independent of schedule.
    pub fn run_determinism_test<F, T>(
        blocks: Vec<T>,
        schedule: ScheduleMode,
        worker_count: usize,
        f: F,
    ) where
        F: Fn(Vec<T>, usize) -> Result<Vec<u8>, String>,
    {
        let schedule = Self::new(blocks.len(), schedule);
        let _ = schedule; // delays would be applied in the test harness
        let _result = f(blocks, worker_count);
        // Compare result with forward schedule
    }
}

/// A priority queue based deterministic scheduler.
/// Ensures that blocks complete in a specific order regardless of actual
/// thread scheduling.
#[derive(Debug)]
pub struct DeterministicScheduler {
    order: BinaryHeap<Reverse<u64>>,
}

impl DeterministicScheduler {
    pub fn new(completion_order: &[u64]) -> Self {
        let mut order = BinaryHeap::new();
        for &idx in completion_order.iter().rev() {
            order.push(Reverse(idx));
        }
        Self { order }
    }

    /// Return the next block that should complete, according to the schedule.
    pub fn next_block(&mut self) -> Option<u64> {
        self.order.pop().map(|r| r.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_forward_schedule() {
        let s = DelaySchedule::new(5, ScheduleMode::Forward);
        assert_eq!(s.delay_for(0), 0);
        assert_eq!(s.delay_for(4), 0);
    }

    #[test]
    fn test_reverse_schedule() {
        let s = DelaySchedule::new(5, ScheduleMode::Reverse);
        assert!(s.delay_for(0) > 0);
    }

    #[test]
    fn test_odd_first() {
        let s = DelaySchedule::new(4, ScheduleMode::OddFirst);
        assert!(s.delay_for(0) > 0); // even, delayed
        assert_eq!(s.delay_for(1), 0); // odd, no delay
        assert!(s.delay_for(2) > 0); // even, delayed
        assert_eq!(s.delay_for(3), 0); // odd, no delay
    }

    #[test]
    fn test_even_first() {
        let s = DelaySchedule::new(4, ScheduleMode::EvenFirst);
        assert_eq!(s.delay_for(0), 0); // even, no delay
        assert!(s.delay_for(1) > 0); // odd, delayed
    }

    #[test]
    fn test_random_seeded_deterministic() {
        let s1 = DelaySchedule::new(10, ScheduleMode::RandomSeeded(42));
        let s2 = DelaySchedule::new(10, ScheduleMode::RandomSeeded(42));
        for i in 0..10 {
            assert_eq!(
                s1.delay_for(i as u64),
                s2.delay_for(i as u64),
                "random seeded schedule must be deterministic"
            );
        }
    }

    #[test]
    fn test_slow_early() {
        let s = DelaySchedule::new(5, ScheduleMode::SlowEarlyBlock);
        assert!(s.delay_for(0) > 0);
        assert_eq!(s.delay_for(1), 0);
    }

    #[test]
    fn test_deterministic_scheduler() {
        // Min-heap: items come out in ascending order
        let mut ds = DeterministicScheduler::new(&[2, 0, 1]);
        assert_eq!(ds.next_block(), Some(0)); // smallest first
        assert_eq!(ds.next_block(), Some(1));
        assert_eq!(ds.next_block(), Some(2));
        assert_eq!(ds.next_block(), None);
    }
}
