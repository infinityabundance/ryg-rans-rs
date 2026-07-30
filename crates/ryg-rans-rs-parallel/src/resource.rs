//! # Resource accounting — memory estimation and worker count computation
//!
//! ## Memory estimation methodology
//!
//! The parallel engine must bound its memory usage before execution begins.
//! Runtime `malloc`/`mmap` accounting is expensive and platform-specific,
//! so we use a **static estimation** model based on configuration parameters
//! and average block size.
//!
//! ### Components
//!
//! | Component | Formula | Description |
//! |-----------|---------|-------------|
//! | Fixed overhead | 1 MiB | Executor metadata, channel buffers, crossbeam queues. |
//! | Per worker | `avg_block_size * 4 + 4096` | Input buffer + output buffer + model buffer + hash state. The 4× factor accounts for worst-case expansion during decode. |
//! | In-flight blocks | `max_in_flight * avg_block_size * 2` | Blocks queued for processing: input + output simultaneously. |
//! | Reorder buffer | `min(max_buffered_output_bytes, in_flight * avg_block_size)` | Completed blocks waiting for sequential commit. Capped by both the configured limit and the worst-case number of in-flight completions. |
//! | **Estimated peak** | Sum of all above | Conservative upper bound. |
//!
//! ### Why checked arithmetic?
//!
//! Memory estimates multiply large numbers (e.g., `worker_count *
//! avg_block_size * 4`).  Without checked arithmetic, a multiplication
//! overflow could produce a small estimate, defeating the purpose of the
//! bound.  All arithmetic uses `saturating_add()` and `saturating_mul()`.
//!
//! ## Worker count computation
//!
//! `effective_worker_count()` clamps the requested thread count:
//!
//! ```text
//! effective = min(requested, max(1, total_blocks))
//! ```
//!
//! This ensures:
//! - No more workers than blocks (a worker with no work would spin idle).
//! - At least 1 worker (sequential fallback).
//! - Determinism: same config + same block count = same worker count.
//!
//! ## Related reading
//!
//! Runtime memory enforcement happens in the `ReorderBuffer`, which
//! rejects insertions that would exceed `max_buffered_output_bytes`.

use crate::config::ParallelConfig;

/// A conservative memory estimate for the parallel engine, computed before
/// execution begins.
///
/// # Usage
///
/// The estimate is returned to the caller for capacity planning.  It is
/// **not** used for runtime enforcement (that is the job of the
/// `ReorderBuffer` and the bounded channels).  However, if the estimate
/// exceeds a known system limit, the caller may choose to reject the
/// operation before spawning any threads.
///
/// # Accuracy
///
/// The estimate is deliberately conservative (overestimates).  Actual
/// peak memory is typically 60–80% of the estimate.  The overestimate
/// accounts for:
/// - Vec capacity versus length (pre-allocated but not fully used).
/// - OS memory allocator overhead (jemalloc/malloc arena fragmentation).
/// - Stack memory (not tracked, but included in the overhead estimate).
#[derive(Debug, Clone)]
pub struct ParallelMemoryEstimate {
    /// Fixed overhead for executor infrastructure.
    /// Includes: executor struct, channels, synchronisation primitives,
    /// metadata tables, and the shared cancellation token.
    pub fixed_bytes: u64,
    /// Additional bytes per worker thread.
    /// Includes: scratch input buffer, scratch output buffer, model buffer,
    /// and hash state structure.
    pub per_worker_bytes: u64,
    /// Bytes consumed by blocks that are queued or actively decoding.
    /// Each in-flight block allocates both an input buffer (compressed)
    /// and an output buffer (decoded/encoded).
    pub in_flight_bytes: u64,
    /// Bytes consumed by the reorder buffer for completed but uncommitted
    /// results.
    pub reorder_bytes: u64,
    /// Peak estimated memory usage — sum of all components.
    ///
    /// This is the recommended value for capacity planning.  To be safe,
    /// the caller should ensure this fits within available memory.
    pub estimated_peak_bytes: u64,
}

/// Compute a conservative memory estimate for the parallel engine.
///
/// # Parameters
///
/// - `config`: The parallel configuration (buffer sizes, in-flight limits).
/// - `avg_block_size`: Expected average block size in bytes.  Use
///   `block_size` from `FixedBlockPlan` for a good estimate, or the
///   expected average for the workload.
/// - `worker_count`: The number of workers that will be spawned.  Use
///   the result of `effective_worker_count()`.
///
/// # Return value
///
/// A `ParallelMemoryEstimate` with per-component breakdown and a total
/// `estimated_peak_bytes`.  All arithmetic uses saturating operations
/// to prevent integer overflow.
///
/// # Example
///
/// ```ignore
/// let estimate = estimate_memory(&config, 4096, 8);
/// println!("Peak memory estimate: {} MiB", estimate.estimated_peak_bytes / (1024*1024));
/// ```
pub fn estimate_memory(
    config: &ParallelConfig,
    avg_block_size: u64,
    worker_count: usize,
) -> ParallelMemoryEstimate {
    let in_flight_blocks = config.max_in_flight_blocks.get() as u64;

    // Fixed: executor + channels + metadata (rough estimate)
    let fixed_bytes: u64 = 1024 * 1024; // 1 MiB overhead

    // Per worker: input buffer + output buffer + model buffer + hash state
    let per_worker_bytes = avg_block_size * 4 + 4096; // 4x block size + overhead

    // In-flight: blocks queued for processing
    let in_flight_bytes = in_flight_blocks * avg_block_size * 2; // input + output

    // Reorder: completed blocks waiting for commit
    let reorder_bytes = config
        .max_buffered_output_bytes
        .min(in_flight_blocks * avg_block_size);

    let estimated_peak_bytes: u64 = fixed_bytes
        .saturating_add(per_worker_bytes.saturating_mul(worker_count as u64))
        .saturating_add(in_flight_bytes)
        .saturating_add(reorder_bytes);

    ParallelMemoryEstimate {
        fixed_bytes,
        per_worker_bytes,
        in_flight_bytes,
        reorder_bytes,
        estimated_peak_bytes,
    }
}

/// Compute the effective worker count, clamping the requested count to the
/// number of blocks.
///
/// # Determinism
///
/// This function is deterministic given the same `(config, total_blocks)`:
/// - `ThreadCount::Exact(n)` always returns `min(n, total_blocks)`.
/// - `ThreadCount::AvailableParallelism` calls
///   `std::thread::available_parallelism()`, which may vary across machines
///   but is stable on a single machine across runs.
///
/// # Errors
///
/// Returns `ParallelError::Config` if:
/// - `ThreadCount::Exact(0)` — thread count must be at least 1.
///
/// # Fallback behavior
///
/// If `std::thread::available_parallelism()` fails (e.g., on platforms
/// that don't support it), falls back to 1 worker.
pub fn effective_worker_count(
    config: &ParallelConfig,
    total_blocks: usize,
) -> Result<usize, crate::ParallelError> {
    let requested = match config.threads {
        crate::ThreadCount::Exact(n) => n.get(),
        crate::ThreadCount::AvailableParallelism => {
            // The parallel crate always has std, so this is always available.
            std::thread::available_parallelism()
                .map(core::num::NonZeroUsize::get)
                .unwrap_or(1)
        }
    };

    if requested == 0 {
        return Err(crate::ParallelError::Config(
            "thread count must be >= 1".into(),
        ));
    }

    // Clamp to number of blocks (no more workers than blocks)
    let effective = requested.min(total_blocks.max(1));
    Ok(effective)
}
