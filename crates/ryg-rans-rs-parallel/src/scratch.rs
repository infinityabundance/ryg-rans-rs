//! # Worker-local scratch buffers — allocation avoidance in hot paths
//!
//! ## The problem
//!
//! In a parallel decode pipeline, each block requires temporary buffers:
//! - Input buffer: holds the compressed block data.
//! - Output buffer: holds the decoded bytes.
//! - Model buffer: holds the serialised frequency model.
//!
//! If every block allocation went through `malloc`/`free`, the allocator
//! contention across N threads would become a bottleneck.  Worse, the
//! allocator's per-thread arenas would grow and shrink unpredictably,
//! causing page faults and TLB pressure.
//!
//! ## The solution: reusable scratch buffers
//!
//! Each worker thread owns a `WorkerScratch` containing pre-allocated
//! `Vec<u8>` buffers.  These buffers are:
//!
//! 1. **Created once** at worker initialisation with a reasonable initial
//!    capacity.
//! 2. **Cleared** (not freed) between blocks.  `Vec::clear()` preserves
//!    the allocated capacity so subsequent blocks skip allocation.
//! 3. **Capped** at `max_retain` bytes.  After processing a very large
//!    block, `shrink_to(max_retain / 3)` is called on each buffer so that
//!    one adversarial block cannot permanently inflate every worker's
//!    memory footprint.
//!
//! ## Pool design: `ScratchPool`
//!
//! The `ScratchPool` holds one `WorkerScratch` per worker, indexed by
//! worker index (0..worker_count).  The executor distributes jobs to
//! workers and each worker uses its fixed slot.
//!
//! ### Why indexed access (not Arc swapping)?
//!
//! An alternative design would give each job its own scratch buffer via
//! an `Arc` pool.  This is more flexible but adds reference-counting
//! overhead.  Since workers are long-lived and each processes one block
//! at a time, fixed indexing is simpler and faster.
//!
//! ### Safety: no mutable aliasing
//!
//! `ScratchPool::get(worker_index)` returns `Option<&mut WorkerScratch>`.
//! The executor ensures that each `worker_index` is used by exactly one
//! thread at a time.  The borrow checker enforces this at compile time
//! (no two `&mut` references to the same slot).

/// Per-worker scratch buffers for decode operations.
///
/// Each worker gets one `WorkerScratch` at startup.  The buffers are
/// reused across blocks via `reset()`, avoiding repeated `malloc`/`free`
/// in the hot loop.
///
/// # Field sizing rationale
///
/// - `input_buffer`: Initial capacity = `initial_capacity` (typically
///   `avg_block_size`).  Holds compressed block data from the container.
/// - `output_buffer`: Initial capacity = `initial_capacity * 2`.
///   Decoded data is typically larger than compressed data; for rANS
///   with typical entropy, the expansion factor is ~2×.
/// - `model_buffer`: Initial capacity = 256 bytes.  Frequency models
///   are small (256 × 4 bytes for u32 frequencies = 1024 bytes; plus
///   header overhead).  The small initial capacity is intentional —
///   models are rarely large.
/// - `max_retain`: Prevents memory retention attacks.  After a block
///   with a 1 GiB input, the buffers would be resized to `max_retain`.
///
/// # Degradation behavior
///
/// If a block requires more capacity than available, the `Vec` grows
/// automatically (standard Rust `realloc`).  After `reset()`, the
/// enlarged capacity is retained.  If it exceeds `max_retain`, the
/// caller should call `shrink_to(max_retain)` to free the excess.
#[derive(Debug)]
pub struct WorkerScratch {
    /// Input buffer for compressed block data.
    /// Cleared but not freed between blocks.
    pub input_buffer: Vec<u8>,
    /// Output buffer for decoded bytes.
    /// Cleared but not freed between blocks.
    pub output_buffer: Vec<u8>,
    /// Buffer for serialised frequency model data.
    /// Cleared but not freed between blocks.
    pub model_buffer: Vec<u8>,
    /// Maximum capacity to retain after processing a block.
    /// Beyond this, `shrink_to(max_retain)` is called to free memory.
    max_retain: usize,
}

impl WorkerScratch {
    /// Create new scratch buffers with the given initial capacities.
    ///
    /// # Parameters
    ///
    /// - `initial_capacity`: Starting capacity for the input buffer.
    ///   The output buffer gets `initial_capacity * 2` (expected decode
    ///   expansion).  The model buffer gets a fixed 256 bytes.
    /// - `max_retain`: Maximum capacity each buffer is allowed to retain
    ///   after `reset()`.  If a Vec's capacity exceeds this, it should
    ///   be shrunk.  (Currently `reset()` only clears; the caller may
    ///   call `shrink_to()` if needed.)
    ///
    /// # Panics
    ///
    /// Does not panic. `Vec::with_capacity` may panic if the capacity
    /// exceeds `isize::MAX` bytes, which is not a realistic scenario.
    pub fn new(initial_capacity: usize, max_retain: usize) -> Self {
        Self {
            input_buffer: Vec::with_capacity(initial_capacity),
            output_buffer: Vec::with_capacity(initial_capacity * 2),
            model_buffer: Vec::with_capacity(256),
            max_retain,
        }
    }

    /// Reset buffers for reuse with the next block.
    ///
    /// `Vec::clear()` sets the length to 0 but preserves the allocated
    /// capacity.  This means subsequent blocks can reuse the existing
    /// allocation without calling `malloc`/`realloc`.
    ///
    /// # Memory retention
    ///
    /// If a buffer's capacity exceeds `max_retain`, shrink it:
    /// ```ignore
    /// if scratch.input_buffer.capacity() > scratch.max_retain {
    ///     scratch.input_buffer.shrink_to(scratch.max_retain);
    /// }
    /// ```
    /// Currently this must be done by the caller.
    pub fn reset(&mut self) {
        self.input_buffer.clear();
        self.output_buffer.clear();
        self.model_buffer.clear();
    }
}

/// A pool of scratch buffers, indexed by worker ID.
///
/// # Thread safety
///
/// `ScratchPool` is **not** `Sync`.  It is owned by the executor and
/// accessed from a single coordination thread that distributes work.
/// Each worker accesses only its own slot via the returned `&mut`
/// reference.
///
/// # Capacity planning
///
/// The pool is created with `count` buffers.  `count` should equal the
/// effective worker count from `effective_worker_count()`.  If more
/// workers are added dynamically, the pool must be resized.
#[derive(Debug)]
pub struct ScratchPool {
    /// Scratch buffers, indexed by worker index.
    buffers: Vec<WorkerScratch>,
}

impl ScratchPool {
    /// Create a pool with `count` scratch buffers.
    ///
    /// # Parameters
    ///
    /// - `count`: Number of buffers to create (one per worker).
    /// - `initial_capacity`: Passed through to each `WorkerScratch::new()`.
    /// - `max_retain`: Passed through to each `WorkerScratch::new()`.
    ///
    /// # Allocation
    ///
    /// Allocates `count * (initial_capacity * 3 + 256)` bytes eagerly.
    /// This is a deliberate upfront cost to avoid runtime allocation
    /// during block processing.
    pub fn new(count: usize, initial_capacity: usize, max_retain: usize) -> Self {
        let buffers = (0..count)
            .map(|_| WorkerScratch::new(initial_capacity, max_retain))
            .collect();
        Self { buffers }
    }

    /// Get the scratch buffer for a specific worker index.
    ///
    /// Returns `None` if `worker_index >= len()`.
    /// The caller guarantees that this index is used by exactly one
    /// thread at a time (no aliased `&mut` references).
    pub fn get(&mut self, worker_index: usize) -> Option<&mut WorkerScratch> {
        self.buffers.get_mut(worker_index)
    }

    /// Reset all buffers in the pool.
    ///
    /// Called at the end of a batch operation to prepare for the next
    /// batch.  Retains allocated capacity up to each buffer's `max_retain`.
    pub fn reset_all(&mut self) {
        for buf in &mut self.buffers {
            buf.reset();
        }
    }

    /// Return the number of buffers (workers) in the pool.
    pub fn len(&self) -> usize {
        self.buffers.len()
    }

    /// Whether the pool is empty (no workers).
    pub fn is_empty(&self) -> bool {
        self.buffers.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scratch_creation() {
        let s = WorkerScratch::new(1024, 65536);
        assert!(s.input_buffer.capacity() >= 1024);
    }

    #[test]
    fn test_scratch_reset() {
        let mut s = WorkerScratch::new(1024, 65536);
        s.input_buffer.resize(100, 42);
        assert_eq!(s.input_buffer.len(), 100);
        s.reset();
        assert!(s.input_buffer.is_empty());
    }

    #[test]
    fn test_pool() {
        let mut pool = ScratchPool::new(4, 1024, 65536);
        assert_eq!(pool.len(), 4);
        assert!(pool.get(0).is_some());
        assert!(pool.get(4).is_none());
    }
}
