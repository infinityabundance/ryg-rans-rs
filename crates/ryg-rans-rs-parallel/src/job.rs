//! # Job types — encode, decode, verify block jobs and their results

use crate::config::{BackendId, CodecPolicy, ModelPolicy};
use std::vec::Vec;

/// An encode job for one block.
#[derive(Debug, Clone)]
pub struct EncodeBlockJob {
    /// 0-based block index.
    pub block_index: u64,
    /// Byte offset in the original input.
    pub input_offset: u64,
    /// Raw input data for this block.
    pub data: Vec<u8>,
    /// Codec selection policy.
    pub codec_policy: CodecPolicy,
    /// Model construction policy.
    pub model_policy: ModelPolicy,
    /// Precision (scale_bits), e.g. 12 for word rANS.
    pub scale_bits: u8,
}

/// Result of encoding one block.
#[derive(Debug, Clone)]
pub struct EncodedBlockResult {
    /// 0-based block index.
    pub block_index: u64,
    /// Byte offset in the original input.
    pub input_offset: u64,
    /// Original input length in bytes.
    pub input_length: u32,
    /// The encoded block (container format).
    pub block: Vec<u8>,
    /// Which backend performed the encode.
    pub backend: BackendId,
    /// SHA-256 of the encoded payload (not the full container block).
    pub payload_hash: [u8; 32],
    /// SHA-256 of the decoded data.
    pub decoded_hash: [u8; 32],
    /// Model hash, if a model was built.
    pub model_hash: Option<[u8; 32]>,
    /// Elapsed wall time for this block's processing, if measured.
    pub elapsed_ns: Option<u64>,
}

impl EncodeBlockJob {
    /// Construct a new encode job.
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

/// A decode job for one block.
#[derive(Debug, Clone)]
pub struct DecodeBlockJob {
    /// 0-based block index.
    pub block_index: u64,
    /// The complete encoded block bytes (header + model + payload).
    pub block_data: Vec<u8>,
}

/// Result of decoding one block.
#[derive(Debug, Clone)]
pub struct DecodedBlockResult {
    /// 0-based block index.
    pub block_index: u64,
    /// Decoded output bytes.
    pub output: Vec<u8>,
    /// Which backend performed the decode.
    pub backend: BackendId,
    /// Whether the payload hash was verified.
    pub payload_verified: bool,
    /// Whether the decoded data hash was verified.
    pub output_verified: bool,
    /// SHA-256 of the decoded output (computed during processing).
    pub output_hash: [u8; 32],
    /// Words consumed from the compressed stream (0 if unknown).
    pub words_consumed: usize,
    /// Final rANS states after decode (empty if unknown).
    pub final_states: Vec<u32>,
    /// Elapsed wall time for this block's processing, if measured.
    pub elapsed_ns: Option<u64>,
}

/// A verify job for one block.
#[derive(Debug, Clone)]
pub struct VerifyBlockJob {
    /// 0-based block index.
    pub block_index: u64,
    /// The complete encoded block bytes.
    pub block_data: Vec<u8>,
}

/// Result of verifying one block.
#[derive(Debug, Clone)]
pub struct VerifiedBlockResult {
    /// 0-based block index.
    pub block_index: u64,
    /// Whether the payload hash matches.
    pub payload_hash_ok: bool,
    /// Whether the decoded data hash matches.
    pub decoded_hash_ok: bool,
    /// Whether the decoded output exactly matches expectations.
    pub output_matches: bool,
    /// Which backend performed the decode for verification.
    pub backend: BackendId,
}

/// Ordered collection of encoded blocks (block-index order guaranteed).
#[derive(Debug, Clone)]
pub struct OrderedEncodedBlocks {
    /// Blocks in ascending block_index order.
    pub blocks: Vec<EncodedBlockResult>,
}

/// Ordered collection of decoded blocks.
#[derive(Debug, Clone)]
pub struct OrderedDecodedBlocks {
    /// Blocks in ascending block_index order.
    pub blocks: Vec<DecodedBlockResult>,
}
