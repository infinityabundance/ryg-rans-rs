//! # Parallel execution report
//!
//! Structured JSON-serializable report returned after parallel execution.
//! Exposes thread counts, backend selection, resource usage, and diagnostics.
//! Does NOT expose unstable scheduler internals.

use crate::config::BackendId;
use std::collections::BTreeMap;

/// Top-level parallel execution report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParallelExecutionReport {
    /// Number of threads requested by the caller.
    pub requested_threads: usize,
    /// Actual number of worker threads used.
    pub actual_threads: usize,
    /// Total blocks submitted for processing.
    pub blocks_submitted: u64,
    /// Blocks that completed processing.
    pub blocks_completed: u64,
    /// Blocks committed in order to the output.
    pub blocks_committed: u64,
    /// Peak number of jobs simultaneously in flight.
    pub peak_jobs_in_flight: usize,
    /// Peak buffered input bytes.
    pub peak_buffered_input_bytes: u64,
    /// Peak buffered output bytes.
    pub peak_buffered_output_bytes: u64,
    /// Count of blocks per backend used.
    pub backend_counts: BTreeMap<BackendId, u64>,
    /// Number of worker panics that occurred.
    pub worker_panics: u64,
    /// Whether the operation was cancelled.
    pub cancelled: bool,
    /// Block index of the canonical error (if any).
    pub canonical_error_block: Option<u64>,
}

/// Per-block execution report.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParallelBlockReport {
    /// 0-based block index.
    pub block_index: u64,
    /// Which backend decoded this block.
    pub backend: BackendId,
    /// Input bytes (compressed payload).
    pub input_bytes: u32,
    /// Output bytes (decoded data).
    pub output_bytes: u32,
    /// Number of u16 words consumed from the compressed stream (word rANS).
    pub words_consumed: Option<u64>,
    /// SHA-256 of final states (where applicable).
    pub final_states_hash: Option<[u8; 32]>,
    /// Whether the payload hash was verified.
    pub payload_verified: bool,
    /// Whether the output hash was verified.
    pub output_verified: bool,
}

impl ParallelExecutionReport {
    pub fn new(requested: usize, actual: usize) -> Self {
        Self {
            requested_threads: requested,
            actual_threads: actual,
            blocks_submitted: 0,
            blocks_completed: 0,
            blocks_committed: 0,
            peak_jobs_in_flight: 0,
            peak_buffered_input_bytes: 0,
            peak_buffered_output_bytes: 0,
            backend_counts: BTreeMap::new(),
            worker_panics: 0,
            cancelled: false,
            canonical_error_block: None,
        }
    }
}
