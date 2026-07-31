//! # Job types — encode, decode, verify block jobs and their results
//!
//! ## Lifecycle overview
//!
//! Each block goes through a three-stage lifecycle:
//!
//! ```text
//! Job creation (planner)
//!     │
//!     ▼
//! Worker dispatch (executor assigns job to worker)
//!     │
//!     ├─ EncodeBlockJob  ──►  EncodedBlockResult
//!     ├─ DecodeBlockJob  ──►  DecodedBlockResult
//!     └─ VerifyBlockJob  ──►  VerifiedBlockResult
//!     │
//!     ▼
//! Reorder buffer (sequential commit by block_index)
//!     │
//!     ▼
//! Ordered collection (OrderedEncodedBlocks / OrderedDecodedBlocks)
//! ```
//!
//! ## Why block_index is the primary key
//!
//! Every job and result carries a `block_index: u64` field.  This is the
//! **primary key** that ties together planning, execution, reordering, and
//! error selection:
//!
//! - **Planning**: the `FixedBlockPlan` assigns indices sequentially.
//! - **Dispatch**: workers are assigned jobs by index, but may complete
//!   in any order.
//! - **Reordering**: the `ReorderBuffer` uses `block_index` to reconstruct
//!   sequential order from out-of-order completions.
//! - **Error selection**: `CanonicalErrorTracker` uses `block_index` to
//!   select the canonical error.
//! - **Reporting**: `ParallelBlockReport` and `ParallelExecutionReport`
//!   reference blocks by index.
//!
//! ## Data flow: EncodedBlockResult fields
//!
//! | Field | Source | Consumer |
//! |-------|--------|----------|
//! | `block` | Encoder output | Container writer, decoder |
//! | `payload_hash` | SHA-256 of compressed payload | Verify stage, decoder integrity check |
//! | `decoded_hash` | SHA-256 of original input | Verify stage |
//! | `model_hash` | SHA-256 of frequency model | Cache lookup, verify stage |
//! | `backend` | Backend selector | Reporting, diagnostics |
//! | `elapsed_ns` | Wall clock | Reporting, performance analysis |
//!
//! ## Data flow: DecodedBlockResult fields
//!
//! | Field | Source | Consumer |
//! |-------|--------|----------|
//! | `output` | Decoder output | Container writer, caller |
//! | `words_consumed` | Decoder inner loop | Verification (stream position) |
//! | `final_states` | Decoder inner loop | Verification (cryptographic continuity) |
//! | `payload_verified` | Integrity check | Reporting |
//! | `output_verified` | Hash comparison | Reporting |
//! | `output_hash` | SHA-256 of `output` | Verification stage |
//!
//! `words_consumed` and `final_states` propagate from the inner decode
//! kernel to the verification layer.  They are used to prove that the
//! compressed stream was fully consumed and that the final rANS states
//! match expected values, which is important for streaming applications
//! where the decoder must be in a known state after each block.

use crate::config::{BackendId, CodecPolicy, ModelPolicy};
use std::vec::Vec;

/// An encode job for a single block.
///
/// Created by the planner and dispatched to a worker by the executor.
/// The worker constructs a frequency model (according to `model_policy`),
/// selects a codec (according to `codec_policy`), and produces an
/// `EncodedBlockResult`.
///
/// # Invariant
///
/// `data.len()` must equal the block length from the `FixedBlockPlan`.
/// This is guaranteed by the planner — the executor should not re-validate.
#[derive(Debug, Clone)]
pub struct EncodeBlockJob {
    /// 0-based block index, assigned by the planner.
    pub block_index: u64,
    /// Byte offset of this block in the original input stream.
    ///
    /// Used for reconstructing the original stream order during
    /// sequential output commit.
    pub input_offset: u64,
    /// Raw input data for this block (owned Vec for thread safety).
    pub data: Vec<u8>,
    /// Codec selection policy for this block.
    pub codec_policy: CodecPolicy,
    /// Model construction policy for this block.
    pub model_policy: ModelPolicy,
    /// Precision parameter (scale_bits), e.g. 12 for standard word rANS.
    ///
    /// Determines the normalisation factor: total = 1 << scale_bits.
    /// Higher values give better compression but require larger tables.
    pub scale_bits: u8,
}

/// Result produced by encoding a single block.
///
/// Contains the encoded bytes, integrity hashes, and metadata needed
/// by the container writer and verification stage.  Results are ordered
/// by `block_index` via the `ReorderBuffer` before being committed to
/// the output.
///
/// # Hash fields
///
/// - `payload_hash`: SHA-256 of `block` (the compressed payload).  Used
///   for integrity verification during decode.  The container stores this
///   alongside the block data.
/// - `decoded_hash`: SHA-256 of the original input data.  Used for
///   end-to-end verification: after decoding, the output hash is compared
///   against this value.
/// - `model_hash`: SHA-256 of the serialised frequency model.  Used as a
///   cache key for the `ModelCache` to avoid rebuild duplicate decode plans.
///
/// # Memory ownership
///
/// `block: Vec<u8>` is an owned allocation that is passed from the worker
/// to the reorder buffer and then to the container writer.  No copying
/// occurs between these stages — the `Vec` is moved via ownership transfer.
#[derive(Debug, Clone)]
pub struct EncodedBlockResult {
    /// 0-based block index (primary key for ordering).
    pub block_index: u64,
    /// Byte offset in the original input stream.
    pub input_offset: u64,
    /// Original input length in bytes.
    pub input_length: u32,
    /// The encoded block bytes in container format (header + model + payload).
    pub block: Vec<u8>,
    /// Which backend performed the encode.
    pub backend: BackendId,
    /// SHA-256 of the encoded payload for integrity verification.
    pub payload_hash: [u8; 32],
    /// SHA-256 of the decoded (original) data for end-to-end verification.
    pub decoded_hash: [u8; 32],
    /// SHA-256 of the frequency model, if one was built.
    ///
    /// `None` for raw-copy or RLE-fill passes where no model exists.
    pub model_hash: Option<[u8; 32]>,
    /// Elapsed wall time for this block's processing, in nanoseconds.
    ///
    /// `None` if timing was not enabled for this run.
    pub elapsed_ns: Option<u64>,
}

impl EncodeBlockJob {
    /// Construct a new encode job for the given block.
    ///
    /// Note: `input_offset` is set to 0 by default.  The planner
    /// should set it explicitly from the `BlockRange`.
    pub fn new(
        block_index: u64,
        data: Vec<u8>,
        codec_policy: CodecPolicy,
        model_policy: ModelPolicy,
        scale_bits: u8,
    ) -> Self {
        Self {
            block_index,
            input_offset: 0,
            data,
            codec_policy,
            model_policy,
            scale_bits,
        }
    }
}

/// A decode job for a single block.
///
/// Contains the complete encoded block data (header + model + compressed
/// payload).  The worker parses the header, retrieves or rebuilds the
/// decode plan, and runs the inner decode kernel.
///
/// # Thread safety
///
/// `block_data: Vec<u8>` is an owned allocation, making `DecodeBlockJob`
/// `Send`.  Jobs are distributed to workers via a channel — no borrowing
/// or shared ownership is required.
#[derive(Debug, Clone)]
pub struct DecodeBlockJob {
    /// 0-based block index.
    pub block_index: u64,
    /// The complete encoded block bytes (header + model + compressed payload).
    ///
    /// Ownership is transferred from the container reader to the worker.
    pub block_data: Vec<u8>,
}

/// Result produced by decoding a single block.
///
/// Contains the decoded output bytes, verification status, and forensic
/// metadata.  The `words_consumed` and `final_states` fields propagate
/// from the inner decode kernel and are used by the verification layer
/// to prove complete and correct decoding.
///
/// # Field propagation
///
/// - `words_consumed`: The number of u16 words consumed from the
///   compressed stream.  This is verified against the expected count
///   from the block header.  A mismatch indicates truncation or corruption.
/// - `final_states`: The final rANS decoder states after all symbols
///   have been decoded.  Streaming applications chain blocks by using
///   the final states of block N as the initial states of block N+1.
///   These states are hashed for compact verification.
/// - `output_hash`: SHA-256 of `output`.  Compared against the
///   `decoded_hash` from the corresponding `EncodedBlockResult` during
///   verification.
///
/// # Memory ownership
///
/// `output: Vec<u8>` is owned.  It moves from the worker to the reorder
/// buffer to the caller without copying.
#[derive(Debug, Clone)]
pub struct DecodedBlockResult {
    /// 0-based block index (primary key for ordering).
    pub block_index: u64,
    /// Decoded output bytes (owned Vec).
    pub output: Vec<u8>,
    /// Which backend performed the decode.
    pub backend: BackendId,
    /// Whether the compressed payload hash was verified against the container.
    ///
    /// `true` if the payload integrity check passed or was skipped.
    pub payload_verified: bool,
    /// Whether the decoded output hash matches the expected value.
    ///
    /// `true` if `SHA-256(output) == decoded_hash` or if verification
    /// was not requested.
    pub output_verified: bool,
    /// SHA-256 of the decoded output, computed during processing.
    pub output_hash: [u8; 32],
    /// Number of u16 words consumed from the compressed stream.
    ///
    /// 0 if the decode kernel does not report this value (e.g., raw-copy).
    pub words_consumed: usize,
    /// Final rANS decoder states after all symbols have been decoded.
    ///
    /// Empty if the decode kernel does not report states (e.g., raw-copy).
    /// For codec 7 (8-way), length is 8.  For codec 8 (16-way), length is 16.
    pub final_states: Vec<u32>,
    /// Elapsed wall time for this block's processing, in nanoseconds.
    ///
    /// `None` if timing was not enabled.
    pub elapsed_ns: Option<u64>,
}

/// A verify job for a single block.
///
/// Contains the complete encoded block data plus the expected decoded
/// hash.  The worker decodes the block (typically using a lightweight
/// verify-only decode path) and checks:
/// 1. Payload integrity: `SHA-256(compressed_payload) == stored_hash`
/// 2. Decoded integrity: `SHA-256(decoded_output) == stored_decoded_hash`
/// 3. Output match: decoded output exactly matches expected (if available)
///
/// # Lightweight verification
///
/// The verify path may skip expensive operations that are not needed for
/// verification alone, such as full result serialisation.  It always runs
/// the full decode kernel to ensure the data is semantically valid.
#[derive(Debug, Clone)]
pub struct VerifyBlockJob {
    /// 0-based block index.
    pub block_index: u64,
    /// The complete encoded block bytes (header + model + payload).
    pub block_data: Vec<u8>,
}

/// Result produced by verifying a single block.
///
/// Contains three independent verification results:
/// - `payload_hash_ok`: The compressed payload has not been tampered with.
/// - `decoded_hash_ok`: The decoded output matches the hash recorded at
///    encode time.  This is an end-to-end integrity check.
/// - `output_matches`: The decoded output exactly matches a reference
///    (either the original input or an externally supplied expected value).
///
/// All three must be `true` for the block to pass verification.
#[derive(Debug, Clone)]
pub struct VerifiedBlockResult {
    /// 0-based block index.
    pub block_index: u64,
    /// Whether the compressed payload hash matches the stored value.
    pub payload_hash_ok: bool,
    /// Whether SHA-256(decoded_output) matches the stored decoded hash.
    pub decoded_hash_ok: bool,
    /// Whether the decoded output exactly matches the expected output.
    pub output_matches: bool,
    /// Which backend performed the decode for verification.
    pub backend: BackendId,
}

/// Execution metadata for a parallel operation run.
///
/// Carries the actual worker count, queue capacity, block count, and
/// completeness counters so benchmark evidence can prove exactly how
/// many workers executed and that every declared block was accounted for.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionMetadata {
    /// Number of worker threads requested by the caller.
    pub requested_workers: usize,
    /// Number of worker threads actually created (clamped to block count).
    pub effective_workers: usize,
    /// Capacity of the bounded job queue.
    pub queue_capacity: usize,
    /// Total number of blocks in this operation.
    pub block_count: usize,
    /// Number of blocks declared by the caller.
    pub declared_blocks: usize,
    /// Number of blocks that completed successfully.
    pub completed_blocks: usize,
    /// Whether the run was cancelled.
    pub cancelled: bool,
}

/// Ordered collection of encoded block results.
///
/// # Guarantee
///
/// Blocks are **always** in ascending `block_index` order.  This is
/// enforced by the `ReorderBuffer` which only emits results in sequence.
/// Callers may rely on this ordering for sequential output construction.
///
/// # Construction
///
/// Created by the encode executor after all blocks have been committed.
/// The outer container writer iterates `blocks` in order and writes
/// each block's header + payload to the output stream.
#[derive(Debug, Clone)]
pub struct OrderedEncodedBlocks {
    /// Blocks in ascending `block_index` order.
    pub blocks: Vec<EncodedBlockResult>,
    /// Execution metadata for this operation.
    pub execution: ExecutionMetadata,
}

/// Ordered collection of decoded block results.
///
/// # Guarantee
///
/// Blocks are **always** in ascending `block_index` order, enforced by
/// the `ReorderBuffer`.  Callers may splice the `output` fields in order
/// to reconstruct the original stream.
///
/// # Construction
///
/// Created by the decode executor after all blocks have been committed.
/// The caller iterates `blocks` in order and concatenates each block's
/// `output` to produce the final decoded stream.
#[derive(Debug, Clone)]
pub struct OrderedDecodedBlocks {
    /// Blocks in ascending `block_index` order.
    pub blocks: Vec<DecodedBlockResult>,
    /// Execution metadata for this operation.
    pub execution: ExecutionMetadata,
}
