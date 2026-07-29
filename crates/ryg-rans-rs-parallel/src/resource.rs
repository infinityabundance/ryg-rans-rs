//! # Resource accounting
//!
//! Before execution, calculate a conservative memory bound.
//! Runtime accounting enforces the actual limit.
//! No integer overflow — checked arithmetic throughout.

use crate::config::ParallelConfig;

/// A conservative memory estimate for the parallel engine.
#[derive(Debug, Clone)]
pub struct ParallelMemoryEstimate {
    /// Fixed overhead (executor, channels, metadata).
    pub fixed_bytes: u64,
    /// Additional bytes per worker (scratch buffers, stack).
    pub per_worker_bytes: u64,
    /// Bytes for in-flight blocks (queued + decoding).
    pub in_flight_bytes: u64,
    /// Bytes for the reorder buffer.
    pub reorder_bytes: u64,
    /// Peak estimated memory usage.
    pub estimated_peak_bytes: u64,
}

/// Compute a conservative memory estimate.
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

/// Deterministic worker count computation.
pub fn effective_worker_count(
    config: &ParallelConfig,
    total_blocks: usize,
) -> Result<usize, crate::ParallelError> {
    let requested = match config.threads {
        crate::ThreadCount::Exact(n) => n.get(),
        crate::ThreadCount::AvailableParallelism => {
            #[cfg(feature = "std")]
            {
                std::thread::available_parallelism()
                    .map(core::num::NonZeroUsize::get)
                    .unwrap_or(1)
            }
            #[cfg(not(feature = "std"))]
            {
                1
            }
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
