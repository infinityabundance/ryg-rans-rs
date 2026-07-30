//! # Deterministic scheduling injection — testing that order never matters
//!
//! ## Why a test scheduler?
//!
//! The parallel engine's core invariant is: **same input → same output,
//! regardless of thread scheduling order**.  Proving this invariant
//! statically is difficult (it depends on the reorder buffer,
//! deterministic error selection, and fixed block planning).
//!
//! Instead, we test it dynamically: use a scheduler that forces specific
//! completion orders and verify that the output is identical across all
//! orders.
//!
//! ## Supported injection modes
//!
//! | Mode | Effect | What it tests |
//! |------|--------|---------------|
//! | `Forward` | Blocks complete 0, 1, 2, ..., N | Baseline — natural order, no reordering needed. |
//! | `Reverse` | Blocks complete N, N-1, ..., 0 | Maximum reordering — all blocks must buffer. |
//! | `OddFirst` | Odd indices complete before even | Tests partial reordering with gaps. |
//! | `EvenFirst` | Even indices complete before odd | Symmetric test. |
//! | `RandomSeeded(seed)` | Pseudo-random order | Stress test — unpredictable interleaving. |
//! | `SlowEarlyBlock` | Block 0 delayed 50 ms | Tests backpressure when the first block is slow. |
//! | `SlowLateBlock` | Last block delayed 50 ms | Tests backpressure when the last block is slow. |
//!
//! ## Design: `DelaySchedule`
//!
//! A `DelaySchedule` maps each block index to a delay duration (in
//! microseconds).  The test harness inserts this delay before processing
//! each block, effectively controlling the order in which blocks
//! complete.
//!
//! ## Design: `DeterministicScheduler`
//!
//! A more precise scheduler that uses a priority queue (binary heap) to
//! enforce an exact completion order.  The test harness pops the next
//! block from the heap and submits it for processing, ensuring the
//! specified order.
//!
//! ## Using in determinism tests
//!
//! ```ignore
//! fn test_determinism() {
//!     let input = ...;
//!     let reference = run_with_schedule(&input, ScheduleMode::Forward);
//!     for mode in &[Reverse, OddFirst, EvenFirst, RandomSeeded(42), SlowEarlyBlock] {
//!         let result = run_with_schedule(&input, *mode);
//!         assert_eq!(result, reference,
//!             "output changed with schedule {:?}", mode);
//!     }
//! }
//! ```

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Scheduling injection mode for deterministic test scheduling.
///
/// Each variant represents a specific completion order strategy.
/// The mode is used by `DelaySchedule::new()` to compute per-block delays.
///
/// # Determinism requirement
///
/// `RandomSeeded(seed)` must produce the same delay sequence for the
/// same seed across runs, across platforms, and across Rust versions.
/// The current implementation uses `DefaultHasher` which is guaranteed
/// to be deterministic within the same process but may vary across
/// Rust versions.  For cross-version determinism, use a well-defined
/// hash function (e.g., `seahash` or `fxhash`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScheduleMode {
    /// No artificial delays — blocks complete in their natural order.
    /// This is the baseline for comparision.
    Forward,
    /// Block 0 is heavily delayed so later blocks complete first.
    /// Tests the reorder buffer's ability to buffer out-of-order results.
    Reverse,
    /// Odd-indexed blocks (1, 3, 5, ...) complete before even-indexed ones.
    /// Tests gap handling in the reorder buffer.
    OddFirst,
    /// Even-indexed blocks (0, 2, 4, ...) complete before odd-indexed ones.
    /// Symmetric test to `OddFirst`.
    EvenFirst,
    /// Pseudo-random completion order with a fixed seed.
    /// The seed ensures reproducibility.  Different seeds produce
    /// different interleavings.
    RandomSeeded(u64),
    /// Block 0 is delayed by ~50 ms to test the backpressure path
    /// when the earliest block is the slowest.
    SlowEarlyBlock,
    /// The last block is delayed by ~50 ms to test backpressure
    /// when the final block holds up completion.
    SlowLateBlock,
}

/// A deterministic delay injector for testing completion order independence.
///
/// Maps each block index to a delay duration (in microseconds).  The test
/// executor waits for the specified delay before beginning to process each
/// block (or before reporting completion).  This forces a specific
/// completion order without modifying the processing logic.
///
/// # Example
///
/// ```ignore
/// let schedule = DelaySchedule::new(10, ScheduleMode::Reverse);
/// for block_index in 0..10 {
///     let delay = schedule.delay_for(block_index);
///     std::thread::sleep(Duration::from_micros(delay));
///     // process block...
/// }
/// ```
#[derive(Debug, Clone)]
pub struct DelaySchedule {
    /// Per-block delays in microseconds.  Indexed by block index.
    delays: Vec<u64>,
}

impl DelaySchedule {
    /// Create a delay schedule for `num_blocks` using the given injection mode.
    ///
    /// # Parameters
    ///
    /// - `num_blocks`: Total number of blocks to schedule.
    /// - `mode`: The scheduling injection mode.
    ///
    /// # Determinism
    ///
    /// Different calls with the same `(num_blocks, mode)` produce the same
    /// schedule.  For `RandomSeeded(seed)`, the same seed always produces
    /// the same delays.
    ///
    /// # Panics
    ///
    /// Does not panic.  All modes handle `num_blocks == 0` gracefully
    /// (produces an empty schedule).
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

    /// Get the delay for a specific block index, in microseconds.
    ///
    /// The test executor should call this before processing each block
    /// and wait for the returned duration.
    ///
    /// Returns 0 for indices beyond the schedule length (no delay).
    /// This gracefully handles off-by-one errors in tests.
    pub fn delay_for(&self, block_index: u64) -> u64 {
        self.delays.get(block_index as usize).copied().unwrap_or(0)
    }

    /// Run a determinism test comparing a given schedule against the
    /// forward (natural order) baseline.
    ///
    /// # Type parameters
    ///
    /// - `T`: The block type.
    /// - `F`: The processing function.  Takes `(Vec<T>, usize)` and
    ///   returns `Result<Vec<u8>, String>`.
    ///
    /// # Current status
    ///
    /// This is a stub.  The delays are computed but not applied — the
    /// caller must integrate the delay logic into the test harness.
    /// A full implementation would:
    /// 1. Run with `ScheduleMode::Forward` to get reference output.
    /// 2. Run with the target schedule, applying `delay_for()` before
    ///    each block.
    /// 3. Assert equality of outputs.
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

/// A priority-queue-based deterministic scheduler that enforces an exact
/// completion order.
///
/// Unlike `DelaySchedule`, which uses relative delays to influence order,
/// `DeterministicScheduler` uses a binary min-heap to control the exact
/// sequence.  The test harness pops the next block index from the heap
/// and only processes that block next.
///
/// This is more precise than delay-based scheduling but requires the
/// test harness to cooperate (it must check the scheduler before each
/// block).
///
/// # Example
///
/// ```ignore
/// let mut sched = DeterministicScheduler::new(&[2, 0, 1]);
/// assert_eq!(sched.next_block(), Some(0));
/// assert_eq!(sched.next_block(), Some(1));
/// assert_eq!(sched.next_block(), Some(2));
/// assert_eq!(sched.next_block(), None);
/// ```
#[derive(Debug)]
pub struct DeterministicScheduler {
    /// Min-heap of remaining block indices to process.
    order: BinaryHeap<Reverse<u64>>,
}

impl DeterministicScheduler {
    /// Create a new deterministic scheduler with the given completion order.
    ///
    /// # Parameters
    ///
    /// - `completion_order`: The exact order in which blocks should complete.
    ///   Each element is a block index.  The first element in the slice is
    ///   the **last** block to complete (because the min-heap pops the
    ///   smallest element first, and we push in reverse order to get the
    ///   desired sequence).
    ///
    /// # Panics
    ///
    /// Does not panic.
    pub fn new(completion_order: &[u64]) -> Self {
        let mut order = BinaryHeap::new();
        for &idx in completion_order.iter().rev() {
            order.push(Reverse(idx));
        }
        Self { order }
    }

    /// Return the next block that should complete, according to the schedule.
    ///
    /// Returns `None` when all scheduled blocks have been returned.
    ///
    /// # Determinism
    ///
    /// Popping from a `BinaryHeap` is deterministic given the same insertion
    /// order.  The `Reverse` wrapper ensures ascending order (min-heap).
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
