//! # Parallel per-block encoding with ordered commit
//!
//! Uses the bounded executor to encode blocks in parallel, then commits
//! them in block-index order through the reorder buffer.
//!
//! Each block independently builds its frequency model and encodes using
//! the core rANS library.  Model construction and encoding are fully
//! parallel — no shared mutable state between blocks.

use crate::cancellation::CancellationToken;
use crate::config::{BackendId, CodecPolicy, ModelPolicy, ParallelConfig};
use crate::error::{BlockError, BlockErrorKind, ParallelError};
use crate::executor::{ExecutorReport, ExecutorTask, run_tasks};
use crate::job::{EncodeBlockJob, EncodedBlockResult, OrderedEncodedBlocks};
use crate::plan::FixedBlockPlan;
use crate::reorder::{BufferSized, HasBlockIndex, ReorderBuffer};
use std::vec::Vec;

impl HasBlockIndex for EncodedBlockResult {
    fn block_index(&self) -> u64 {
        self.block_index
    }
}

impl BufferSized for EncodedBlockResult {
    fn buffer_size(&self) -> u64 {
        self.block.len() as u64 + 128 // 128 bytes overhead
    }
}

/// An encode task submitted to the executor.
struct EncodeTask {
    job: EncodeBlockJob,
}

impl ExecutorTask for EncodeTask {
    type Output = Result<EncodedBlockResult, BlockError>;

    fn run(self, _worker_index: usize, cancel: &CancellationToken) -> Self::Output {
        cancel.check().map_err(|_| BlockError {
            block_index: self.job.block_index,
            kind: BlockErrorKind::Codec,
        })?;

        encode_single_block(self.job)
    }
}

/// Codec ID for WORD_INTERLEAVED16 (16-state, 16-bit renormalization, scale_bits=12).
const CODEC_WORD_INTERLEAVED16: u16 = 8;
/// Codec ID for WORD_INTERLEAVED8 (8-state, 16-bit renormalization, scale_bits=12).
const CODEC_WORD_INTERLEAVED8: u16 = 7;

/// Encode a single block using the core rANS library.
pub fn encode_single_block(job: EncodeBlockJob) -> Result<EncodedBlockResult, BlockError> {
    let data = &job.data;
    if data.is_empty() {
        // Empty block — produce an empty RANS payload with zero-length model
        let block = build_empty_block(job.block_index);
        return Ok(EncodedBlockResult {
            block_index: job.block_index,
            input_offset: job.input_offset,
            input_length: 0,
            block,
            backend: BackendId::Scalar8,
            payload_hash: [0u8; 32],
            decoded_hash: sha256(data),
            model_hash: None,
            elapsed_ns: None,
        });
    }

    // Choose codec and determine parameters
    let codec_id = match job.codec_policy {
        CodecPolicy::Explicit(id) => id,
        CodecPolicy::Auto => CODEC_WORD_INTERLEAVED16,
    };

    // Build frequency model from the input data
    let scale_bits = job.scale_bits;
    let (freqs, cum_freqs) = build_frequency_model(data, scale_bits)?;

    // Encode using the selected codec
    let encoded = match codec_id {
        CODEC_WORD_INTERLEAVED16 => {
            encode_word_interleaved16(data, &freqs, &cum_freqs, scale_bits)?
        }
        CODEC_WORD_INTERLEAVED8 => {
            // Fall back to word interleaved 8-way via core encoding
            let mut buf = Vec::with_capacity(data.len() * 2 + 32);
            let mut states = [ryg_rans_rs_core::RANS_WORD_L as u32; 8];
            // Simplified 8-way encode — one symbol per state round-robin
            for (i, &sym) in data.iter().enumerate() {
                let lane = i & 7;
                let s = sym as usize;
                let f = freqs[s];
                let st = cum_freqs[s];
                let x_max = ((ryg_rans_rs_core::RANS_WORD_L >> scale_bits) << 16) * f;
                if states[lane] >= x_max {
                    buf.push((states[lane] & 0xffff) as u16);
                    states[lane] >>= 16;
                }
                states[lane] = ((states[lane] / f) << scale_bits) + (states[lane] % f) + st;
            }
            // Flush states
            for idx in (0..8).rev() {
                buf.push((states[idx] & 0xffff) as u16);
                buf.push(((states[idx] >> 16) & 0xffff) as u16);
            }
            buf.reverse();
            buf
        }
        _ => {
            return Err(BlockError {
                block_index: job.block_index,
                kind: BlockErrorKind::Model,
            });
        }
    };

    // Convert u16 payload to bytes for hashing
    let payload_bytes: Vec<u8> = encoded.iter().flat_map(|w| w.to_le_bytes()).collect();
    let payload_hash = sha256(&payload_bytes);
    let decoded_hash = sha256(data);

    // Build the RYGRANS container block bytes
    let block = build_block_record(
        job.block_index,
        codec_id,
        scale_bits,
        data.len() as u32,
        &payload_bytes,
        &freqs,
        &cum_freqs,
    );

    Ok(EncodedBlockResult {
        block_index: job.block_index,
        input_offset: job.input_offset,
        input_length: data.len() as u32,
        block,
        backend: BackendId::Scalar8,
        payload_hash,
        decoded_hash,
        model_hash: None,
        elapsed_ns: None,
    })
}

/// Build frequency model from raw data.
fn build_frequency_model(data: &[u8], scale_bits: u8) -> Result<(Vec<u32>, Vec<u32>), BlockError> {
    let total = 1u32 << scale_bits;
    let mut freqs = vec![0u32; 256];
    for &b in data {
        freqs[b as usize] = freqs[b as usize].checked_add(1).ok_or(BlockError {
            block_index: 0,
            kind: BlockErrorKind::ResourceLimit,
        })?;
    }

    let freqs_total: u32 = freqs.iter().sum();
    if freqs_total == 0 {
        return Err(BlockError {
            block_index: 0,
            kind: BlockErrorKind::Model,
        });
    }

    // Normalize frequencies to the target total
    normalize_frequencies(&mut freqs, total);
    let mut cum = vec![0u32; 257];
    cum[0] = 0;
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    Ok((freqs, cum))
}

/// Normalize frequencies to sum to `total` using deterministic scaling.
fn normalize_frequencies(freqs: &mut [u32], total: u32) {
    let sum: u64 = freqs.iter().map(|&f| f as u64).sum();
    if sum == 0 {
        return;
    }
    let mut allocated: u64 = 0;
    for f in freqs.iter_mut() {
        if sum > 0 {
            let scaled = ((*f as u64) * (total as u64)) / sum;
            *f = scaled as u32;
            allocated += scaled;
        }
    }
    // Distribute remainder to the largest frequency
    let remainder = (total as u64).saturating_sub(allocated);
    if remainder > 0 {
        if let Some(max_idx) = freqs
            .iter()
            .position(|f| *f == *freqs.iter().max().unwrap_or(&0))
        {
            freqs[max_idx] = freqs[max_idx].saturating_add(remainder as u32);
        }
    }
}

/// Encode data using the WORD_INTERLEAVED16 codec.
fn encode_word_interleaved16(
    data: &[u8],
    freqs: &[u32],
    _cum_freqs: &[u32],
    scale_bits: u8,
) -> Result<Vec<u16>, BlockError> {
    // Build cumulative frequencies
    let mut cum = vec![0u32; 257];
    cum[0] = 0;
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    // Use the simd crate's encoder if available, otherwise manual encode
    #[cfg(feature = "ryg-rans-rs-simd")]
    {
        let result = ryg_rans_rs_simd::packed_table::encode_interleaved16(
            data,
            freqs,
            &cum,
            scale_bits as u32,
        )
        .map_err(|_| BlockError {
            block_index: 0,
            kind: BlockErrorKind::Codec,
        })?;
        return Ok(result);
    }

    #[cfg(not(feature = "ryg-rans-rs-simd"))]
    {
        // Manual 16-way encode using backward writer (matching SIMD encoder format).
        let capacity = data
            .len()
            .checked_mul(2)
            .and_then(|c| c.checked_add(64))
            .unwrap_or(usize::MAX);
        let mut buf = vec![0u16; capacity];
        let mut writer = capacity; // backward writer
        let mut states = [ryg_rans_rs_core::RANS_WORD_L as u32; 16];

        // Encode in reverse order
        for i in (0..data.len()).rev() {
            let lane = i & 15;
            let s = data[i] as usize;
            let f = freqs[s];
            let st = cum[s];
            if f == 0 {
                return Err(BlockError {
                    block_index: 0,
                    kind: BlockErrorKind::Model,
                });
            }
            let threshold = ((ryg_rans_rs_core::RANS_WORD_L >> scale_bits) << 16) * f;
            if states[lane] >= threshold {
                if writer == 0 {
                    return Err(BlockError {
                        block_index: 0,
                        kind: BlockErrorKind::ResourceLimit,
                    });
                }
                writer -= 1;
                buf[writer] = (states[lane] & 0xffff) as u16;
                states[lane] >>= 16;
            }
            states[lane] = ((states[lane] / f) << scale_bits) + (states[lane] % f) + st;
        }

        // Flush states in REVERSE lane order (15 down to 0)
        for idx in (0..16).rev() {
            if writer < 2 {
                return Err(BlockError {
                    block_index: 0,
                    kind: BlockErrorKind::ResourceLimit,
                });
            }
            writer -= 2;
            buf[writer] = (states[idx] & 0xffff) as u16;
            buf[writer + 1] = ((states[idx] >> 16) & 0xffff) as u16;
        }

        return Ok(buf[writer..].to_vec());
    }
}

/// Build an empty block record (for empty input).
fn build_empty_block(block_index: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(104);
    buf.extend_from_slice(b"RYGR"); // BLOCK_TAG
    buf.extend_from_slice(&(104u16).to_le_bytes()); // header_size
    buf.push(1); // block_version
    buf.push(0); // block_kind = RANS
    buf.extend_from_slice(&block_index.to_le_bytes());
    buf.extend_from_slice(&0u16.to_le_bytes()); // codec_id
    buf.push(12); // scale_bits
    buf.push(16); // state_count
    buf.push(0); // model_encoding
    buf.extend_from_slice(&[0u8; 3]); // reserved
    buf.extend_from_slice(&0u32.to_le_bytes()); // uncompressed_length
    buf.extend_from_slice(&0u32.to_le_bytes()); // payload_length
    buf.extend_from_slice(&0u32.to_le_bytes()); // model_length
    buf.extend_from_slice(&[0u8; 4]); // reserved2
    buf.extend_from_slice(&[0u8; 32]); // payload_sha256
    buf.extend_from_slice(&sha256(&[])); // decoded_sha256
    buf
}

/// Build a RYGRANS container block record (header + model + payload).
fn build_block_record(
    block_index: u64,
    codec_id: u16,
    scale_bits: u8,
    uncompressed_length: u32,
    payload: &[u8],
    _freqs: &[u32],
    _cum_freqs: &[u32],
) -> Vec<u8> {
    let state_count: u8 = 16;
    let model_data: Vec<u8> = Vec::new();
    let payload_bytes = payload.to_vec();
    let payload_sha256 = sha256(payload);
    let header_size: u16 = 104;

    let mut buf = Vec::with_capacity(header_size as usize + model_data.len() + payload_bytes.len());
    buf.extend_from_slice(b"RYGR");
    buf.extend_from_slice(&header_size.to_le_bytes());
    buf.push(1); // block_version
    buf.push(0); // block_kind = RANS
    buf.extend_from_slice(&block_index.to_le_bytes());
    buf.extend_from_slice(&codec_id.to_le_bytes());
    buf.push(scale_bits);
    buf.push(state_count);
    buf.push(0); // model_encoding: 20
    buf.extend_from_slice(&[0u8; 3]); // reserved: 21-23
    buf.extend_from_slice(&uncompressed_length.to_le_bytes()); // 24-27
    buf.extend_from_slice(&(payload_bytes.len() as u32).to_le_bytes()); // 28-31
    buf.extend_from_slice(&(model_data.len() as u32).to_le_bytes()); // 32-35
    buf.extend_from_slice(&[0u8; 4]); // reserved2: 36-39
    buf.extend_from_slice(&payload_sha256); // 40-71
    buf.extend_from_slice(&[0u8; 32]); // decoded_sha256: 72-103
    buf.extend_from_slice(&model_data);
    buf.extend_from_slice(&payload_bytes);
    buf
}

/// Compute SHA-256 of bytes.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Parallel block encoder.
pub struct ParallelEncoder;

impl ParallelEncoder {
    /// Encode blocks in parallel using the given configuration.
    ///
    /// Returns blocks in ascending block-index order, ready for container serialisation.
    pub fn encode_blocks(
        blocks: impl IntoIterator<Item = EncodeBlockJob>,
        config: &ParallelConfig,
    ) -> Result<OrderedEncodedBlocks, ParallelError> {
        let jobs: Vec<EncodeBlockJob> = blocks.into_iter().collect();
        if jobs.is_empty() {
            return Ok(OrderedEncodedBlocks { blocks: Vec::new() });
        }

        let block_count = jobs.len();
        let worker_count = crate::resource::effective_worker_count(config, block_count)?;
        let queue_capacity = config.max_in_flight_blocks.get().max(worker_count);

        // Convert jobs to tasks
        let tasks: Vec<EncodeTask> = jobs.into_iter().map(|job| EncodeTask { job }).collect();

        // Run tasks in parallel
        let report: ExecutorReport<Result<EncodedBlockResult, BlockError>> = run_tasks(
            tasks,
            worker_count,
            queue_capacity,
            config.worker_stack_size,
        )?;

        // Collect results through reorder buffer
        let mut reorder = ReorderBuffer::new(
            config.max_in_flight_blocks.get(),
            config.max_buffered_output_bytes,
        );
        let mut ordered_blocks = Vec::with_capacity(block_count);
        let mut error_tracker = crate::error::CanonicalErrorTracker::new();

        for result in report.results {
            match result {
                Ok(block) => {
                    match reorder.insert(block) {
                        Ok(Some(ready)) => {
                            ordered_blocks.push(ready);
                            // Drain any additional ready blocks
                            ordered_blocks.extend(reorder.drain_ready());
                        }
                        Ok(None) => { /* buffered */ }
                        Err(e) => {
                            error_tracker.record(e);
                        }
                    }
                }
                Err(e) => {
                    error_tracker.record(e);
                }
            }
        }

        // Drain any remaining ready blocks
        ordered_blocks.extend(reorder.drain_ready());

        // Check for canonical error
        if let Some(canonical) = error_tracker.canonical_error() {
            return Err(ParallelError::EncodeFailed(Box::new(canonical.clone())));
        }

        // Sort by block index to ensure ordering
        ordered_blocks.sort_by_key(|b| b.block_index);

        Ok(OrderedEncodedBlocks {
            blocks: ordered_blocks,
        })
    }

    /// Encode blocks from raw input data with a pre-computed fixed block plan.
    pub fn encode_planned(
        plan: &FixedBlockPlan,
        data: &[u8],
        config: &ParallelConfig,
    ) -> Result<OrderedEncodedBlocks, ParallelError> {
        let jobs: Vec<EncodeBlockJob> = plan
            .ranges
            .iter()
            .map(|range| {
                let start = range.input_offset as usize;
                let end = start + range.length as usize;
                let block_data = data[start..end].to_vec();
                EncodeBlockJob::new(
                    range.block_index,
                    block_data,
                    CodecPolicy::Auto,
                    ModelPolicy::PerBlock,
                    config.default_scale_bits(),
                )
            })
            .collect();

        Self::encode_blocks(jobs, config)
    }
}

impl ParallelConfig {
    /// Default scale_bits for block encoding.
    pub fn default_scale_bits(&self) -> u8 {
        12
    }
}
