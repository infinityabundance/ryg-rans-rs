//! # Parallel execution report — structured, JSON-serializable diagnostics
//!
//! ## Purpose
//!
//! After a parallel encode or decode operation, the caller receives a
//! `ParallelExecutionReport` that describes what happened: how many threads
//! were used, which backends were selected, how much memory was consumed,
//! and whether any errors occurred.
//!
//! ## Stability
//!
//! The report is **stable** across releases: the same inputs produce the
//! same report fields (modulo timing and thread-count-dependent stats like
//! `peak_jobs_in_flight`).
//!
//! ## What is NOT exposed
//!
//! - Scheduler internals (work-stealing decisions, wait times).
//! - Per-worker timing breakdowns (may be added later as optional).
//! - Memory allocator statistics (platform-specific).
//!
//! ## Serialization
//!
//! Both `ParallelExecutionReport` and `ParallelBlockReport` derive
//! `serde::Serialize` and `serde::Deserialize`.  They can be serialised
//! to JSON, CBOR, or any other serde-compatible format for downstream
//! processing or logging.
//!
//! ## Backend counts
//!
//! `backend_counts: BTreeMap<BackendId, u64>` records how many blocks
//! were processed by each backend.  This is useful for:
//! - Detecting degenerate scheduling (all blocks on one backend).
//! - Verifying that model-aware dispatch selected the expected backends.
//! - Performance analysis (did the SIMD backend actually get used?).
//!
//! The `BTreeMap` ensures deterministic key ordering in serialised output.

use crate::config::BackendId;
use std::collections::BTreeMap;

/// Top-level report summarising a parallel execution (encode or decode).
///
/// # Field categories
///
/// **Threading**: `requested_threads`, `actual_threads` — may differ due
/// to clamping to block count.
///
/// **Progress**: `blocks_submitted`, `blocks_completed`, `blocks_committed`.
/// These may differ if blocks were cancelled or failed.
///
/// **Resource pressure**: `peak_jobs_in_flight`, `peak_buffered_input_bytes`,
/// `peak_buffered_output_bytes`.  Useful for tuning buffer sizes.
///
/// **Backend distribution**: `backend_counts` — shows which decode kernels
/// were used and how often.
///
/// **Errors**: `worker_panics`, `cancelled`, `canonical_error_block`.
///
/// # Construction
///
/// Created by the executor at the end of an operation.  The report is
/// initially empty and fields are populated as the operation progresses.
/// The `new()` constructor initialises the report with request/actual
/// thread counts; all other fields start at zero/false/None.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParallelExecutionReport {
    /// Number of worker threads requested by the caller (from config).
    pub requested_threads: usize,
    /// Actual number of worker threads used after clamping.
    ///
    /// May be less than `requested_threads` if the block count is lower.
    pub actual_threads: usize,
    /// Total number of blocks submitted to the executor.
    pub blocks_submitted: u64,
    /// Number of blocks that completed processing (success or failure).
    pub blocks_completed: u64,
    /// Number of blocks committed in order to the output container.
    ///
    /// This is always ≤ `blocks_completed`.  Blocks that fail are not
    /// committed.
    pub blocks_committed: u64,
    /// Peak number of jobs simultaneously in flight at any point.
    ///
    /// A measure of pipeline depth.  If this is consistently below
    /// `max_in_flight_blocks`, the bottleneck is elsewhere (I/O,
    /// per-block processing time).
    pub peak_jobs_in_flight: usize,
    /// Peak total input bytes buffered across all in-flight blocks.
    pub peak_buffered_input_bytes: u64,
    /// Peak total decoded/encoded output bytes buffered in the reorder buffer.
    pub peak_buffered_output_bytes: u64,
    /// Count of blocks processed by each backend.
    ///
    /// Key is `BackendId`, value is the number of blocks that used that
    /// backend.  Uses `BTreeMap` for deterministic key ordering in
    /// serialised output.
    pub backend_counts: BTreeMap<BackendId, u64>,
    /// Number of worker threads that panicked during processing.
    pub worker_panics: u64,
    /// Whether the operation was cancelled (via `CancellationToken`).
    pub cancelled: bool,
    /// Block index of the canonical error, if any operation failed.
    ///
    /// `None` if all blocks completed successfully.
    pub canonical_error_block: Option<u64>,
}

/// Per-block report with detailed forensic information.
///
/// Each block in a parallel execution produces a `ParallelBlockReport`
/// with metadata useful for verification, debugging, and performance
/// analysis.  The fields `words_consumed` and `final_states_hash` are
/// specific to word rANS and are `None` for other codec types.
///
/// # Verification use
///
/// The verification stage compares per-block reports against expected
/// values: `words_consumed` must match the header-declared count,
/// `final_states_hash` must match the expected final states, and
/// both verification booleans must be `true`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParallelBlockReport {
    /// 0-based block index.
    pub block_index: u64,
    /// Which backend decoded this block.
    pub backend: BackendId,
    /// Size of the compressed payload in bytes.
    pub input_bytes: u32,
    /// Size of the decoded output in bytes.
    pub output_bytes: u32,
    /// Number of u16 words consumed from the compressed stream.
    ///
    /// `None` for raw-copy or RLE-fill passes where stream consumption
    /// is not tracked.
    pub words_consumed: Option<u64>,
    /// SHA-256 of the concatenated final rANS states.
    ///
    /// Used for cryptographic chaining between blocks in streaming
    /// applications.  `None` for backends that do not expose states.
    pub final_states_hash: Option<[u8; 32]>,
    /// Whether the compressed payload integrity hash was verified.
    pub payload_verified: bool,
    /// Whether the decoded output hash was verified against the
    /// expected value.
    pub output_verified: bool,
}

impl ParallelExecutionReport {
    /// Create a new execution report with initial thread counts.
    ///
    /// All progress counters start at zero.  Backend counts start empty.
    /// The report is populated by the executor as processing progresses.
    ///
    /// # Parameters
    ///
    /// - `requested`: The thread count requested by the caller (before clamping).
    /// - `actual`: The actual thread count used (after clamping to block count).
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
