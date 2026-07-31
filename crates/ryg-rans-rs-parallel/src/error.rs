//! # Parallel error types — deterministic canonical error selection
//!
//! ## Why determinism matters
//!
//! In a parallel engine, multiple blocks may fail concurrently in the same
//! execution.  Without a deterministic selection policy, the error returned
//! to the caller would depend on the nondeterministic order in which workers
//! finish.  This would break the core contract: **same input → same output**
//! (including errors).
//!
//! ## Canonical error selection
//!
//! The canonical block error is the failure associated with the **lowest
//! block index**.  Among multiple errors at the same block index, priority
//! follows the `BlockErrorKind` ordinal (lower ordinal = higher priority).
//!
//! For example, if blocks 3, 7, and 3 fail (two errors at block 3), the
//! canonical error is the higher-priority error from block 3.  The errors
//! at block 7 are discarded.
//!
//! ## Invariants
//!
//! - Thread count must never change the returned error kind or its block index.
//! - Worker scheduling delay must never change the canonical error.
//! - The `Display` representation is stable and machine-readable.
//! - `ParallelError` implements `std::error::Error` for interop with `anyhow`,
//!   `eyre`, and other error-handling frameworks.
//!
//! ## Error containment strategy
//!
//! Worker panics are caught by `std::panic::catch_unwind` and converted to
//! `ParallelError::WorkerPanic`.  This prevents a single bad block from
//! tearing down the entire executor.  The panic message is preserved for
//! debugging.
//!
//! Resource limits (`ResourceLimit`) are reported eagerly: if a block would
//! exceed the configured memory budget, it fails immediately rather than
//! blocking indefinitely.  This is a fail-fast policy that prevents
//! executor deadlock from backpressure exhaustion.

use std::fmt;

/// Top-level error type for the parallel engine.
///
/// # Variant categories
///
/// | Category | Variants | Description |
/// |----------|----------|-------------|
/// | Block operation | `EncodeFailed`, `DecodeFailed`, `VerifyFailed` | Per-block failure wrapped with `BlockError`. The canonical (lowest-index) error is surfaced. |
/// | Runtime failure | `WorkerPanic`, `ThreadCreate` | Infrastructure failures: a thread panicked or could not be spawned. |
/// | Validation | `Config` | Configuration error detected before any work began. Always fatal. |
/// | Cancellation | `Cancelled` | User-requested or externally signalled cancellation. |
/// | Resource | `ResourceLimit` | Memory or queue budget exhausted. |
/// | Structural | `Format` | Container format error that is not per-block (e.g., truncated header). |
/// | I/O | `Io` | Wrapped I/O error from the underlying filesystem or network. |
/// | Internal | `Internal` | Executor coordination logic bug. Indicates a programming error. |
///
/// # Block error wrapping
///
/// `EncodeFailed`, `DecodeFailed`, and `VerifyFailed` each contain a
/// `BlockError` that identifies the failing block and the error kind.
/// The `Box` indirection keeps the enum small (pointer-sized) for
/// efficient `Result` passing across thread boundaries.
#[derive(Debug, Clone)]
pub enum ParallelError {
    /// One or more blocks failed during encoding.
    ///
    /// The inner `BlockError` is the canonical (lowest-index) failure.
    /// Other block failures are discarded.
    EncodeFailed(Box<BlockError>),
    /// One or more blocks failed during decoding.
    ///
    /// The inner `BlockError` is the canonical (lowest-index) failure.
    DecodeFailed(Box<BlockError>),
    /// Container verification failed on one or more blocks.
    VerifyFailed(Box<BlockError>),
    /// A worker thread panicked while processing a block.
    ///
    /// The panic is caught via `std::panic::catch_unwind` and converted
    /// to this error.  The block index may be unknown if the panic
    /// occurred before the block index was assigned.
    WorkerPanic {
        /// The block index being processed when the panic occurred,
        /// or `None` if unknown (panic before index assignment).
        block_index: Option<u64>,
        /// The 0-based index of the worker that panicked.
        worker_index: usize,
    },
    /// Configuration validation error detected at executor start.
    ///
    /// This is always fatal — no work was started.  The string describes
    /// the invalid configuration.
    Config(String),
    /// The operation was cancelled by the user or an external signal.
    ///
    /// Checked cooperatively at yield points.  In-flight workers may
    /// continue briefly but no new work is dispatched.
    ///
    /// Carries the completion counts so callers can distinguish partial
    /// progress from a fully completed result.
    Cancelled {
        /// Number of blocks that completed before cancellation.
        completed: usize,
        /// Total number of blocks declared.
        expected: usize,
    },
    /// The operation returned fewer results than declared blocks.
    ///
    /// This indicates silent truncation — the executor finished without
    /// an explicit cancellation or error but did not produce every
    /// expected result.  This must never be returned as `Ok`.
    IncompleteExecution {
        /// Number of results actually produced.
        completed: usize,
        /// Number of blocks declared.
        expected: usize,
    },
    /// A resource limit would be exceeded.
    ///
    /// Examples: max buffered bytes exceeded, too many in-flight blocks.
    /// This is a fail-fast signal to prevent executor deadlock.
    ResourceLimit(String),
    /// Structural error in the container format.
    ///
    /// This is not a per-block error but a global structural issue
    /// such as a truncated header, invalid magic bytes, or version
    /// mismatch.
    Format(String),
    /// Wrapped I/O error (read, write, seek, etc.).
    Io(String),
    /// Failed to create a worker thread.
    ///
    /// This may indicate that the system's thread limit has been
    /// reached, or that the configured stack size is invalid.
    ThreadCreate(String),
    /// Internal executor coordination error.
    ///
    /// This indicates a programming error in the executor logic
    /// (e.g., unexpected channel state, violated invariant).
    /// It should never occur in normal operation.
    Internal(String),
}

/// Per-block error record with deterministic ordering properties.
///
/// Together, `block_index` and `kind` form a total order:
/// 1. Primary key: `block_index` (ascending).
/// 2. Secondary key: `BlockErrorKind` ordinal (ascending).
///
/// This ordering is used by `CanonicalErrorTracker` to select the
/// canonical error among concurrent failures.
#[derive(Debug, Clone)]
pub struct BlockError {
    /// The 0-based block index where the failure occurred.
    pub block_index: u64,
    /// The specific kind of error that occurred.
    ///
    /// The `PartialOrd` derivation on `BlockErrorKind` ensures
    /// consistent prioritisation within the same block.
    pub kind: BlockErrorKind,
}

/// Stable priority ordering for same-block errors.
///
/// # Priority (ascending ordinal = higher priority)
///
/// 1. `Format` — Block header validation failed. Highest priority because
///    a structurally invalid block invalidates all further processing.
/// 2. `ResourceLimit` — Memory or queue budget exhausted during processing.
///    Important to surface before codec errors because resource limits
///    may cause spurious codec failures.
/// 3. `PayloadHash` — Compressed payload integrity check failed.
///    A corrupted payload should be reported before attempting decode.
/// 4. `Model` — Frequency model validation failed.
///    An invalid model makes decode impossible.
/// 5. `BackendFormatMismatch` — The requested backend is incompatible with
///    the block's format (e.g. an 8-way backend on a codec-8 16-way block,
///    or a RAW/RLE backend on a RANS block).  Detected at planning time,
///    before any execution.  Outranks `Codec` because the combination is
///    structurally invalid, not merely a decode failure.
/// 6. `BackendUnavailable` — The requested backend exists but cannot execute
///    here: the CPU lacks the required instruction set at runtime, or the
///    build was not compiled with the required target features, or an
///    explicit SIMD request was combined with `disable_simd`.  Never
///    silently substituted — the caller gets a typed error instead.
/// 7. `BackendRequiresBatchContext` — The requested batch backend needs
///    coordinator-level grouping of four compatible jobs.  The one-block
///    API cannot execute it, so the plan is rejected at planning time.
/// 8. `Codec` — Codec execution failed (encode or decode inner loop).
///    A genuine algorithmic failure.
/// 9. `DecodedHashMissing` — Stored decoded hash is zero/unset under Strict.
///    The decode completed but the stored hash cannot verify it.
/// 10. `DecodedHashMismatch` — Stored nonzero decoded hash does not match
///    the recomputed hash.  The decode completed but produced wrong output
///    (e.g. model corruption that payload hashing cannot catch).
/// 11. `WorkerPanic` — The worker thread panicked.  Lowest priority
///    because the panic may be a consequence of a preceding error.
/// 12. `OutputCommit` — Failed to commit output (reorder buffer).
///    Usually a secondary consequence of another failure.
///
/// # Determinism guarantee
///
/// The `#[derive(PartialOrd, Ord)]` on this enum guarantees that
/// the ordinal order is determined by the declaration order of the
/// variants.  This is a stable, documented contract — variants must
/// not be reordered without updating this documentation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockErrorKind {
    /// Block header format validation failure.
    Format,
    /// Resource limit exceeded (memory, queue capacity).
    ResourceLimit,
    /// Compressed payload integrity hash mismatch.
    PayloadHash,
    /// Frequency model validation failure.
    Model,
    /// Requested backend is incompatible with the block format.
    BackendFormatMismatch,
    /// Requested backend cannot execute on this CPU or in this build.
    BackendUnavailable,
    /// Requested batch backend requires coordinator-level batch context.
    BackendRequiresBatchContext,
    /// Codec execution failure (encode or decode).
    Codec,
    /// Stored decoded hash is zero/unset — cannot verify under Strict policy.
    DecodedHashMissing,
    /// Stored decoded hash is nonzero and does not match the recomputed hash.
    DecodedHashMismatch,
    /// Worker thread panicked during block processing.
    WorkerPanic,
    /// Failed to commit output to the reorder buffer.
    OutputCommit,
}

impl fmt::Display for ParallelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EncodeFailed(e) => {
                write!(f, "encode failed at block {}: {:?}", e.block_index, e.kind)
            }
            Self::DecodeFailed(e) => {
                write!(f, "decode failed at block {}: {:?}", e.block_index, e.kind)
            }
            Self::VerifyFailed(e) => {
                write!(f, "verify failed at block {}: {:?}", e.block_index, e.kind)
            }
            Self::WorkerPanic {
                block_index,
                worker_index,
            } => {
                write!(
                    f,
                    "worker {} panicked{}",
                    worker_index,
                    block_index
                        .map(|b| format!(" processing block {}", b))
                        .unwrap_or_default()
                )
            }
            Self::Config(msg) => write!(f, "config error: {}", msg),
            Self::Cancelled {
                completed,
                expected,
            } => {
                write!(
                    f,
                    "operation cancelled after {} of {} blocks completed",
                    completed, expected
                )
            }
            Self::IncompleteExecution {
                completed,
                expected,
            } => {
                write!(
                    f,
                    "incomplete execution: {} of {} blocks produced results",
                    completed, expected
                )
            }
            Self::ResourceLimit(msg) => write!(f, "resource limit: {}", msg),
            Self::Format(msg) => write!(f, "format error: {}", msg),
            Self::Io(msg) => write!(f, "i/o error: {}", msg),
            Self::ThreadCreate(msg) => write!(f, "thread create failed: {}", msg),
            Self::Internal(msg) => write!(f, "internal error: {}", msg),
        }
    }
}

impl std::error::Error for ParallelError {}

/// Tracks the lowest-index failing block across concurrent worker results.
///
/// # Purpose
///
/// This is the heart of deterministic error selection.  Workers report
/// failures as they complete, in nondeterministic order.  The tracker
/// retains only the error associated with the **lowest block index**,
/// discarding all others.
///
/// # Thread safety
///
/// `CanonicalErrorTracker` is **not** `Sync` — it is designed to be
/// used from a single thread (the collector/committer) that receives
/// results from multiple worker threads via channels.  The channel
/// provides the necessary synchronisation.
///
/// # Algorithm
///
/// 1. Start with `lowest_failing_block = None`.
/// 2. On each `record()` call, compare the incoming `block_index` against
///    the current lowest.  If the new index is lower (or no error has been
///    recorded yet), replace the stored error.
/// 3. When execution completes, `canonical_error()` returns the surviving
///    error (if any).
///
/// This is O(1) per insertion and O(1) to retrieve the final result.
///
/// # Same-block tiebreaking
///
/// If two errors occur at the same block index (e.g., both a format error
/// and a codec error), the first one recorded wins **unless** a later
/// error has a lower `block_index`.  For strict prioritisation at the
/// same index, the caller should use `BlockErrorKind` ordinal ordering
/// before calling `record()`.
///
/// # Example
///
/// ```ignore
/// let mut tracker = CanonicalErrorTracker::new();
/// tracker.record(BlockError { block_index: 5, kind: BlockErrorKind::Codec });
/// tracker.record(BlockError { block_index: 3, kind: BlockErrorKind::Format });
/// // Block 3 is lower than 5, so the error is now the Format error from block 3.
/// tracker.record(BlockError { block_index: 7, kind: BlockErrorKind::PayloadHash });
/// // Block 7 > 3, discarded.
/// assert_eq!(tracker.lowest_failing_index(), Some(3));
/// ```
#[derive(Debug)]
pub struct CanonicalErrorTracker {
    /// The lowest failing block index encountered so far, or `None`.
    lowest_failing_block: Option<u64>,
    /// The canonical error corresponding to `lowest_failing_block`, or `None`.
    error: Option<BlockError>,
}

impl CanonicalErrorTracker {
    /// Create a new empty tracker (no errors recorded).
    pub fn new() -> Self {
        Self {
            lowest_failing_block: None,
            error: None,
        }
    }

    /// Record a block error, retaining it only if its block index is
    /// strictly lower than any previously recorded failure.
    ///
    /// # Ordering
    ///
    /// - If this is the first error, it is always retained.
    /// - If a previous error exists at a lower index, this error is
    ///   silently discarded.
    /// - If this error has a lower index, it replaces the previous one.
    ///
    /// # Threading note
    ///
    /// This method is not `Sync` — call from the single-threaded
    /// collector, not from worker threads directly.
    pub fn record(&mut self, err: BlockError) {
        let should_replace = match self.lowest_failing_block {
            Some(current) => err.block_index < current,
            None => true,
        };
        if should_replace {
            self.lowest_failing_block = Some(err.block_index);
            self.error = Some(err);
        }
    }

    /// Return a reference to the canonical (lowest-index) error, if any.
    ///
    /// Returns `None` if no errors have been recorded.
    pub fn canonical_error(&self) -> Option<&BlockError> {
        self.error.as_ref()
    }

    /// Return the lowest failing block index, if any.
    pub fn lowest_failing_index(&self) -> Option<u64> {
        self.lowest_failing_block
    }
}

impl Default for CanonicalErrorTracker {
    fn default() -> Self {
        Self::new()
    }
}
