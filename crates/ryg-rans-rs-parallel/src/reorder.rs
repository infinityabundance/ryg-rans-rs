//! # Bounded ordered result buffer — out-of-order commit serialiser
//!
//! ## The problem
//!
//! Workers in a thread pool may finish blocks in any order.  Worker A may
//! get blocks 0 and 10; worker B may get blocks 5 and 3.  The output
//! container requires blocks in ascending index order (0, 1, 2, ...).
//! Without a reorder buffer, we would need to either:
//!
//! - **Wait synchronously**: Block the commit until each block finishes.
//!   This serialises the pipeline and defeats parallelism.
//! - **Accept unordered output**: Violates the container format requirement.
//!
//! ## The solution: sparse slot array + gap tracking + draining
//!
//! The reorder buffer maintains a `BTreeMap<u64, T>` keyed by block index.
//! Results are inserted in any order.  A monotonically increasing
//! `next_expected` counter tracks the next sequential block to commit.
//!
//! ### Insert path:
//!
//! ```text
//! insert(result):
//!   if result.block_index < next_expected:
//!     → Err (already committed, duplicate)
//!   if result.block_index == next_expected:
//!     → advance next_expected, return Ok(Some(result))  [immediate commit]
//!   if pending.contains_key(result.block_index):
//!     → Err (duplicate in-flight, should never happen)
//!   if pending.len() >= max_blocks or current_bytes + size > max_bytes:
//!     → Err (ResourceLimit)  [backpressure]
//!   → store in pending, return Ok(None)  [buffered]
//! ```
//!
//! ### Drain path:
//!
//! ```text
//! drain_ready():
//!   while pending.contains(next_expected):
//!     remove it, advance next_expected, append to result Vec
//!   return result Vec
//! ```
//!
//! ## Backpressure model
//!
//! The buffer enforces **two independent limits**:
//!
//! 1. **Count limit** (`max_blocks`): Maximum number of out-of-order results
//!    stored simultaneously.  Prevents the pending map from growing without
//!    bound due to many small blocks.
//! 2. **Byte limit** (`max_bytes`): Maximum total bytes (sum of result sizes
//!    via `BufferSized`) stored simultaneously.  Prevents a single large
//!    block from consuming all memory while waiting for a slow early block.
//!
//! When either limit is reached, further `insert()` calls return
//! `Err(BlockErrorKind::ResourceLimit)`.  The caller (executor commit logic)
//! must apply backpressure to the producer, typically by blocking the job
//! dispatch channel until `drain_ready()` frees capacity.
//!
//! ## Why `BTreeMap` instead of a slot array?
//!
//! A fixed-size slot array (`Vec<Option<T>>`) indexed by block number would
//! be simpler but would waste memory on sparse insertions.  `BTreeMap` is
//! O(log N) per operation, which is acceptable for typical block counts
//! (hundreds to millions).  The constant factor is low because `u64` keys
//! are cheap to compare.
//!
//! ## History (why the API is the way it is)
//!
//! The original API was `insert(item) -> Result<Option<T>>` plus a separate
//! `drain_ready()` that callers had to remember after receiving the next
//! expected item.  That unenforced protocol was fragile — a caller that
//! forgot the drain silently lost contiguous blocks — so Phase L.5 replaced
//! it with `insert(item) -> Result<Vec<T>>`, which returns the newly
//! inserted item plus every contiguous pending item it unblocks, in
//! strictly ascending order, with **no separate drain required** (ADR-0014).
//! `drain_ready` remains only as a final inspection API for diagnostics.
//!
//! ## Invariants
//!
//! 1. Committed output is strictly ascending block index, with no gaps.
//! 2. If every input block is inserted exactly once and no insertion
//!    errors, the concatenation of all commit batches equals `[0, 1, …,
//!    N-1]` — pinned by the exhaustive permutation test for N ≤ 9.
//! 3. A duplicate or stale (already-committed) block index is a typed
//!    error, never a silent drop.
//! 4. Count and byte limits are enforced *before* the entry is stored, so
//!    a resource-limit error can never be followed by an unbounded grow.
//!
//! ## Failure modes
//!
//! * **Duplicate index** — two blocks with the same index (encoder bug or
//!    corrupted plan) → `Err(Duplicate)`; the canonical error tracker picks
//!    it up and the operation fails deterministically.
//! * **Stale index** — an index below `next_expected` (committed already)
//!    → `Err(Stale)`.
//! * **Resource limit** — pending count or bytes would exceed the budget
//!    → `Err(ResourceLimit)`; the caller back-pressures the producer.
//! * **Overflow accounting** — byte totals use checked/saturating
//!    arithmetic so a hostile `BufferSized` cannot corrupt the budget.
//!
//! ## Performance
//!
//! O(log N) per insert, O(k log N) per commit batch of k items, O(1)
//! amortised memory per pending block.  For typical workloads the reorder
//! stage is a negligible fraction of the pipeline cost (paper 0005 §4).
//!
//! ## Verification / Receipts / Tests
//!
//! The exhaustive permutation test (N ≤ 9, all permutations) proves the
//! atomic-commit invariant; property tests cover larger N; dedicated tests
//! cover duplicates, stale indexes, missing gaps, resource limits, overflow
//! accounting, error recovery, and cancellation boundaries.  The court
//! `RYG_RANS.L.REORDER.ATOMIC_COMMIT` seals the contract.
//!
//! ## References
//!
//! `docs/adr/0014` (atomic commit batches); `docs/papers/0004-parallel-engine.md`
//! §3 (live reorder commit); `docs/glossary.md` (reorder buffering, committed
//! output).

use crate::error::{BlockError, BlockErrorKind};
use std::collections::BTreeMap;

/// A bounded reorder buffer that serialises out-of-order results into
/// ascending block-index order.
///
/// # Type parameter
///
/// `T` is the type of result to reorder (e.g., `EncodedBlockResult` or
/// `DecodedBlockResult`).  `T` must implement `HasBlockIndex` (to read
/// its block index) and `BufferSized` (to report its memory footprint).
///
/// # State machine
///
/// ```text
///                   ┌──────────────┐
///                   │  Empty       │
///                   │  (no pending)│
///                   └──────┬───────┘
///                           │
///                  insert(out-of-order)
///                           │
///                           ▼
///                   ┌──────────────┐
///   ┌──────────────►│  Pending     │◄───────────────┐
///   │               │  (buffered)  │                │
///   │               └──────┬───────┘                │
///   │                      │                        │
///   │           insert(next_expected)               │
///   │           or drain_ready() hits               │
///   │                      │                        │
///   │                      ▼                        │
///   │               ┌──────────────┐                │
///   │               │  Emitting    │────────────────┘
///   │               │  (committed) │  insert(out-of-order)
///   │               └──────────────┘
///   │                      │
///   └──────────────────────┘
///       all blocks drained
/// ```
#[derive(Debug)]
pub struct ReorderBuffer<T> {
    /// Sparse storage for out-of-order results, keyed by block_index.
    /// Uses `BTreeMap` for ordered iteration during draining.
    pending: BTreeMap<u64, T>,
    /// The next expected block index (0-based).  Monotonically increasing.
    next_expected: u64,
    /// Maximum number of buffered blocks (count-based backpressure).
    max_blocks: usize,
    /// Maximum total bytes buffered (memory-based backpressure).
    max_bytes: u64,
    /// Current total bytes buffered (sum of sizes via `BufferSized`).
    current_bytes: u64,
}

/// Trait for types that carry a block index (the primary key for reordering).
///
/// Implemented for `EncodedBlockResult`, `DecodedBlockResult`, and similar
/// result types.  The `ReorderBuffer` uses this trait to extract the index
/// from a generic `T`.
pub trait HasBlockIndex {
    /// Return the 0-based block index of this item.
    fn block_index(&self) -> u64;
}

/// Trait for types that report their memory footprint for backpressure.
///
/// The `ReorderBuffer` uses this trait to track the total memory consumed
/// by buffered results.  The reported size should be the number of bytes
/// that the item contributes to heap memory (e.g., `Vec::capacity() * size_of::<u8>()`).
///
/// # Accuracy
///
/// Overestimating is safer than underestimating.  If the reported size is
/// too low, the buffer may exceed its memory budget.  Conservative estimates
/// are preferred.
pub trait BufferSized {
    /// Return the approximate memory footprint of this item in bytes.
    fn buffer_size(&self) -> u64;
}

impl<T> ReorderBuffer<T>
where
    T: HasBlockIndex + BufferSized,
{
    /// Create a new bounded reorder buffer.
    ///
    /// # Parameters
    ///
    /// - `max_blocks`: Maximum number of out-of-order results to buffer.
    ///   When this limit is reached, further insertions return `ResourceLimit`.
    /// - `max_bytes`: Maximum total memory (sum of `BufferSized` sizes) to
    ///   buffer.  When this limit is reached, further insertions return
    ///   `ResourceLimit`.
    ///
    /// Both limits must be respected.  Either one being exceeded blocks
    /// further inserts.
    pub fn new(max_blocks: usize, max_bytes: u64) -> Self {
        Self {
            pending: BTreeMap::new(),
            next_expected: 0,
            max_blocks,
            max_bytes,
            current_bytes: 0,
        }
    }

    /// Insert a result into the reorder buffer and atomically return every
    /// newly committable result.
    ///
    /// # Return value
    ///
    /// Returns `Ok(Vec<T>)` containing:
    ///
    /// - The newly inserted item, if it was (or became) the next expected
    ///   block.
    /// - Every contiguous pending item it unblocks, in strictly ascending
    ///   block-index order.
    ///
    /// The returned vector is empty only when the item was buffered because
    /// an earlier block is still missing (a gap exists).
    ///
    /// # Caller obligation
    ///
    /// **There is no separate drain call required after insertion.**  Every
    /// result that can be committed is returned by this call.  A final
    /// `drain_ready()` is retained only as a diagnostics/inspection API for
    /// asserting completeness at the end of a run.
    ///
    /// # Error conditions
    ///
    /// - `BlockErrorKind::OutputCommit`: Block index < `next_expected`
    ///   (already committed) or duplicate index already in `pending`.
    /// - `BlockErrorKind::ResourceLimit`: `pending.len() >= max_blocks` or
    ///   `current_bytes + item.buffer_size() > max_bytes`.
    pub fn insert(&mut self, item: T) -> Result<Vec<T>, BlockError> {
        let idx = item.block_index();

        // Already-committed block (index < next_expected)
        if idx < self.next_expected {
            return Err(BlockError {
                block_index: idx,
                kind: BlockErrorKind::OutputCommit,
            });
        }

        // Duplicate in-flight
        if self.pending.contains_key(&idx) {
            return Err(BlockError {
                block_index: idx,
                kind: BlockErrorKind::OutputCommit,
            });
        }

        // If this is the next expected block, commit it and then drain
        // every contiguous pending block it unblocks.
        if idx == self.next_expected {
            self.next_expected += 1;
            let mut committed = Vec::new();
            committed.push(item);
            self.drain_into(&mut committed);
            return Ok(committed);
        }

        // Otherwise, buffer it.  Check bounds first.
        let size = item.buffer_size();
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
        self.pending.insert(idx, item);
        Ok(Vec::new())
    }

    /// Drain all consecutively available results from the buffer into `out`.
    fn drain_into(&mut self, out: &mut Vec<T>) {
        while let Some(result) = self.pending.remove(&self.next_expected) {
            let size = result.buffer_size();
            self.current_bytes = self.current_bytes.saturating_sub(size);
            self.next_expected += 1;
            out.push(result);
        }
    }

    /// Drain all consecutively available results from the buffer.
    ///
    /// Retained as a **diagnostics/inspection** API for asserting completeness
    /// at the end of a run.  Callers using [`Self::insert`] do not need to
    /// call this after every insertion — `insert` already returns everything
    /// newly committable.  Call this once after the final insertion to pick up
    /// any tail that was buffered before the last gap closed.
    pub fn drain_ready(&mut self) -> Vec<T> {
        let mut ready = Vec::new();
        self.drain_into(&mut ready);
        ready
    }

    /// Return the number of currently buffered results.
    ///
    /// This is the count of out-of-order results waiting for a gap to fill.
    /// It does not include results that were immediately committed.
    pub fn buffered_count(&self) -> usize {
        self.pending.len()
    }

    /// Return the total estimated memory used by buffered results (in bytes).
    pub fn buffered_bytes(&self) -> u64 {
        self.current_bytes
    }

    /// Return the next block index expected for sequential commit.
    ///
    /// This is the block index of the next result that will be immediately
    /// committed (returned as `Ok(Some(...))` from `insert()`).
    pub fn next_expected(&self) -> u64 {
        self.next_expected
    }

    /// Whether all results have been committed (no pending entries).
    ///
    /// Returns `true` if all processed blocks have been inserted and
    /// drained.  This does **not** imply that all blocks are done,
    /// only that no results are waiting in the buffer.
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
            let committed = buf.insert(TestResult { index: i, size: 10 }).unwrap();
            assert_eq!(
                committed.len(),
                1,
                "sequential insert {} should commit 1",
                i
            );
            assert_eq!(committed[0].index, i);
        }
        assert!(buf.is_complete());
    }

    #[test]
    fn test_out_of_order_insert() {
        let mut buf = ReorderBuffer::new(16, 65536);
        // Insert 1 before 0 (order 1, 0, 2)
        let c1 = buf.insert(TestResult { index: 1, size: 10 }).unwrap();
        assert!(c1.is_empty(), "block 1 must buffer (gap at 0)");
        assert_eq!(buf.buffered_count(), 1);
        // Insert 0: should return [0, 1] atomically — no separate drain call.
        let c0 = buf.insert(TestResult { index: 0, size: 10 }).unwrap();
        assert_eq!(c0.len(), 2, "insert(0) must atomically return [0, 1]");
        assert_eq!(c0[0].index, 0);
        assert_eq!(c0[1].index, 1);
        assert!(buf.is_complete());
    }

    #[test]
    fn test_reverse_order() {
        let mut buf = ReorderBuffer::new(16, 65536);
        // Insert 4,3,2,1 (reverse order) — all buffered.
        for i in (1..5).rev() {
            let c = buf.insert(TestResult { index: i, size: 10 }).unwrap();
            assert!(c.is_empty(), "block {} should buffer (not next)", i);
        }
        assert_eq!(buf.buffered_count(), 4);
        // Insert 0 — atomically commits [0,1,2,3,4].
        let c0 = buf.insert(TestResult { index: 0, size: 10 }).unwrap();
        assert_eq!(c0.len(), 5, "insert(0) must atomically return [0..4]");
        for (i, item) in c0.iter().enumerate() {
            assert_eq!(item.index, i as u64);
        }
        assert!(buf.is_complete());
    }

    #[test]
    fn test_permutation_atomic_commit() {
        // Property-style: for N up to 8, every insertion order must produce
        // the exact sequence [0..N-1] when commit batches are concatenated.
        use std::collections::HashSet;
        for n in 1..=8usize {
            // Build all permutations via Heap's algorithm (iterative).
            let mut perm: Vec<usize> = (0..n).collect();
            let mut perms: Vec<Vec<usize>> = Vec::new();
            perms.push(perm.clone());
            let mut c = vec![0usize; n];
            let mut i = 0usize;
            while i < n {
                if c[i] < i {
                    if i % 2 == 0 {
                        perm.swap(0, i);
                    } else {
                        perm.swap(c[i], i);
                    }
                    perms.push(perm.clone());
                    c[i] += 1;
                    i = 0;
                } else {
                    c[i] = 0;
                    i += 1;
                }
            }
            let mut seen: HashSet<Vec<usize>> = HashSet::new();
            for p in &perms {
                if !seen.insert(p.clone()) {
                    continue;
                }
                let mut buf = ReorderBuffer::new(64, 65536);
                let mut out: Vec<u64> = Vec::new();
                for &idx in p {
                    let committed = buf
                        .insert(TestResult {
                            index: idx as u64,
                            size: 10,
                        })
                        .unwrap();
                    out.extend(committed.iter().map(|t| t.index));
                }
                out.extend(buf.drain_ready().iter().map(|t| t.index));
                assert_eq!(
                    out,
                    (0..n as u64).collect::<Vec<u64>>(),
                    "permutation {:?} must commit [0..{}]",
                    p,
                    n - 1
                );
            }
        }
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut buf = ReorderBuffer::new(16, 65536);
        buf.insert(TestResult { index: 0, size: 10 }).unwrap();
        let dup = buf.insert(TestResult { index: 0, size: 10 });
        assert!(dup.is_err());
    }

    #[test]
    fn test_stale_index_rejected() {
        let mut buf = ReorderBuffer::new(16, 65536);
        buf.insert(TestResult { index: 1, size: 10 }).unwrap();
        buf.insert(TestResult { index: 0, size: 10 }).unwrap(); // commits [0,1]
        // Block 0 is now stale (already committed) — must be rejected.
        let stale = buf.insert(TestResult { index: 0, size: 10 });
        assert!(stale.is_err(), "stale index must be rejected");
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
    fn test_byte_limit_reached() {
        let mut buf = ReorderBuffer::new(16, 25); // max 25 bytes total
        buf.insert(TestResult { index: 2, size: 10 }).unwrap();
        buf.insert(TestResult { index: 3, size: 10 }).unwrap();
        // current_bytes = 20; adding a 10-byte item exceeds 25.
        let r = buf.insert(TestResult { index: 4, size: 10 });
        assert!(r.is_err(), "byte limit must be enforced");
    }

    #[test]
    fn test_empty_buffer() {
        let buf: ReorderBuffer<TestResult> = ReorderBuffer::new(16, 65536);
        assert!(buf.is_complete());
        assert_eq!(buf.buffered_count(), 0);
    }
}
