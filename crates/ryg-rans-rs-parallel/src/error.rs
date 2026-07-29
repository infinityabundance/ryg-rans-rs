//! # Parallel error types — deterministic canonical error selection
//!
//! ## Canonical error selection
//!
//! Parallel discovery order is nondeterministic.  Returned error selection must
//! not be.  The canonical block error is the failure associated with the lowest
//! block index.  Same-block ordering follows the stable priority below.
//!
//! Thread count and worker delay must never change the returned error kind,
//! its block index, or its structured representation.

use std::fmt;

/// Top-level parallel engine error.
#[derive(Debug, Clone)]
pub enum ParallelError {
    /// One or more blocks failed.  The error is the canonical (lowest-index) failure.
    EncodeFailed(Box<BlockError>),
    /// One or more blocks failed during decode.
    DecodeFailed(Box<BlockError>),
    /// Container verification failed.
    VerifyFailed(Box<BlockError>),
    /// A worker thread panicked.
    WorkerPanic {
        /// The block index being processed when the panic occurred (if known).
        block_index: Option<u64>,
        /// The 0-based worker index.
        worker_index: usize,
    },
    /// Configuration validation error.
    Config(String),
    /// The operation was cancelled.
    Cancelled,
    /// Resource limit would be exceeded.
    ResourceLimit(String),
    /// Container format error (structural, not per-block).
    Format(String),
    /// I/O error (wrapped).
    Io(String),
    /// Thread creation failed.
    ThreadCreate(String),
    /// Internal error in the executor or coordination logic.
    Internal(String),
}

/// Per-block error with deterministic ordering.
#[derive(Debug, Clone)]
pub struct BlockError {
    /// The block index (0-based).
    pub block_index: u64,
    /// The kind of error.
    pub kind: BlockErrorKind,
}

/// Stable priority order for same-block errors (lowest ordinal = highest priority).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlockErrorKind {
    /// Format validation failure in the block header.
    Format,
    /// Resource limit reached during processing.
    ResourceLimit,
    /// Payload integrity hash mismatch.
    PayloadHash,
    /// Model validation failure.
    Model,
    /// Codec execution failed (encode or decode).
    Codec,
    /// Decoded output hash mismatch.
    DecodedHash,
    /// Internal worker panic.
    WorkerPanic,
    /// Output commit failed.
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
            Self::Cancelled => write!(f, "operation cancelled"),
            Self::ResourceLimit(msg) => write!(f, "resource limit: {}", msg),
            Self::Format(msg) => write!(f, "format error: {}", msg),
            Self::Io(msg) => write!(f, "i/o error: {}", msg),
            Self::ThreadCreate(msg) => write!(f, "thread create failed: {}", msg),
            Self::Internal(msg) => write!(f, "internal error: {}", msg),
        }
    }
}

impl std::error::Error for ParallelError {}

/// Track the lowest failing block index across concurrent results.
///
/// This is the heart of deterministic error selection — only the error
/// associated with the lowest block index is retained.
#[derive(Debug)]
pub struct CanonicalErrorTracker {
    lowest_failing_block: Option<u64>,
    error: Option<BlockError>,
}

impl CanonicalErrorTracker {
    pub fn new() -> Self {
        Self {
            lowest_failing_block: None,
            error: None,
        }
    }

    /// Record a block error.  Retained only if its block index is lower than
    /// any previously recorded failure.
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

    /// Returns the canonical (lowest-index) error, if any.
    pub fn canonical_error(&self) -> Option<&BlockError> {
        self.error.as_ref()
    }

    /// Returns the lowest failing block index, if any.
    pub fn lowest_failing_index(&self) -> Option<u64> {
        self.lowest_failing_block
    }
}

impl Default for CanonicalErrorTracker {
    fn default() -> Self {
        Self::new()
    }
}
