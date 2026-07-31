//! # Parallel per-block encoding with ordered commit
//!
//! This module implements the **encoding pipeline** for the RYGRANS parallel
//! compressor.  Encoding proceeds through the following stages:
//!
//! 1. **Job ingestion** — `EncodeBlockJob` values arrive from the coordinator,
//!    each carrying raw uncompressed data, a block index, and configuration
//!    (codec policy, model policy, scale bits).
//! 2. **Frequency model construction** — `build_frequency_model` counts byte
//!    frequencies in the raw data and normalises them so they sum exactly to
//!    `1 << scale_bits` (typically 4096).  The normaliser (see
//!    `normalize_frequencies`) is a **canonical normalizer** that guarantees
//!    every observed symbol gets at least 1, using a reserved-slot + remainder
//!    distribution algorithm.
//! 3. **rANS encoding** — `encode_word_interleaved16` (or the 8-way variant)
//!    processes symbols **in reverse order** using a backward writer, encoding
//!    into 16 interleaved lanes.  Final states are flushed in **reverse lane
//!    order** (lane 15 → 0) to maintain the correct decode order.
//! 4. **Payload hashing** — The encoded u16 payload is flattened to bytes and
//!    SHA-256 hashed for integrity verification.
//! 5. **Block record construction** — `build_block_record` (or
//!    `build_empty_block` for empty input) serialises the RYGRANS container
//!    header, the 1024-byte packed frequency model (256 × u32 LE), and the
//!    encoded payload into a single `Vec<u8>`.
//! 6. **Ordered commit** — The `ParallelEncoder::encode_blocks` method feeds
//!    tasks to the bounded executor, collects results through a
//!    `ReorderBuffer` that commits blocks in block-index order, and returns
//!    an `OrderedEncodedBlocks` ready for container serialization.
//!
//! ## Codec distinction
//!
//! - `CODEC_WORD_INTERLEAVED16` (value 8): 16-state, 16-bit renormalization,
//!   scale_bits = 12.  Uses 16 interleaved rANS lanes.  The default when
//!   `CodecPolicy::Auto` is selected.
//! - `CODEC_WORD_INTERLEAVED8` (value 7): 8-state, 16-bit renormalization,
//!   scale_bits = 12.  Uses 8 interleaved lanes.  A simpler fallback with
//!   slightly lower compression density.
//!
//! ## Empty-block path
//!
//! When the input data is empty, `encode_single_block` takes a fast path via
//! `build_empty_block`.  This produces a minimal 104-byte header-only block
//! with zero-length model and payload, but with the **real SHA-256 of the
//! empty input** stored in the `decoded_sha256` field.  This guarantees that
//! verification of a zero-length block will still confirm the integrity of
//! the original (empty) data.
//!
//! ## Parallelism model
//!
//! Each block encodes independently — there is no shared mutable state between
//! blocks.  Model construction and rANS encoding are fully parallelised across
//! worker threads via the bounded executor (`crate::executor::run_tasks`).
//! The only serial bottleneck is the final ordered commit through the reorder
//! buffer.
//!
//! ## Error model
//!
//! Encoding errors are captured per-block and propagated as `BlockError`
//! values.  The `CanonicalErrorTracker` selects a single canonical error
//! from the set of failures, favouring earlier block indices.  The overall
//! result is `ParallelError::EncodeFailed` if any block fails.

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

    fn block_index(&self) -> Option<u64> {
        Some(self.job.block_index)
    }
}

/// Codec ID for WORD_INTERLEAVED16 (16-state, 16-bit renormalization, scale_bits=12).
const CODEC_WORD_INTERLEAVED16: u16 = 8;
/// Codec ID for WORD_INTERLEAVED8 (8-state, 16-bit renormalization, scale_bits=12).
const CODEC_WORD_INTERLEAVED8: u16 = 7;

/// Encode a single block using the core rANS library.
/// Encode a single block using the core rANS library.
///
/// This is the primary encoding entry point, called by worker threads
/// (via `EncodeTask::run`) and usable directly for single-block encoding.
///
/// ## Pipeline
///
/// 1. **Empty fast path** — If the input data is empty, return a minimal
///    header-only block via `build_empty_block` with the real SHA-256 of
///    the empty data.  No frequency model or payload is produced.
/// 2. **Codec selection** — If `CodecPolicy::Auto`, defaults to
///    `CODEC_WORD_INTERLEAVED16`.  An explicit policy picks the requested
///    codec.
/// 3. **Frequency model** — `build_frequency_model` counts bytes and
///    normalises to `1 << scale_bits` total.
/// 4. **rANS encode** — The selected codec (`encode_word_interleaved16` or
///    the inline 8-way loop) produces a `Vec<u16>` payload.
/// 5. **Hashing** — The flattened byte payload is SHA-256 hashed
///    (`payload_hash`) and the original input data is hashed
///    (`decoded_hash`) for later verification.
/// 6. **Container assembly** — `build_block_record` serialises the
///    RYGRANS header, the 1024-byte packed model, and the payload bytes.
/// 7. **Backend attribution** — The `BackendId` is set based on the actual
///    codec used (`Scalar16` for INTERLEAVED16, `Scalar8` for INTERLEAVED8).
///
/// ## Returns
///
/// - `Ok(EncodedBlockResult)` with the complete encoded block, hashes, and
///   metadata.
/// - `Err(BlockError)` if the model is empty (`BlockErrorKind::Model`) or
///   the codec ID is unrecognised.
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

    // Build the RYGRANS container block bytes with real model and hashes
    let block = build_block_record(
        job.block_index,
        codec_id,
        scale_bits,
        data.len() as u32,
        &payload_bytes,
        &freqs,
        decoded_hash,
    );

    // Determine correct BackendId based on actual codec used
    let backend = match codec_id {
        CODEC_WORD_INTERLEAVED16 => BackendId::Scalar16,
        CODEC_WORD_INTERLEAVED8 => BackendId::Scalar8,
        _ => BackendId::Scalar8,
    };

    Ok(EncodedBlockResult {
        block_index: job.block_index,
        input_offset: job.input_offset,
        input_length: data.len() as u32,
        block,
        backend,
        payload_hash,
        decoded_hash,
        model_hash: Some(crate::encode::sha256(build_model_bytes(&freqs).as_slice())),
        elapsed_ns: None,
    })
}

/// Build frequency model from raw data.
///
/// Counts byte frequencies (0–255) in the input, then normalises them
/// so they sum exactly to `1 << scale_bits` (the target total).
///
/// ## Normalisation guarantee
///
/// The normaliser in `normalize_frequencies` is a **canonical normalizer**
/// that guarantees **every observed symbol gets at least 1** in the output
/// frequency table.  This is critical for rANS decoding: a zero-frequency
/// symbol would cause a division-by-zero during decoding.  The algorithm:
///
/// 1. Reserve `nonzero_count` slots (one per observed symbol).
/// 2. Scale each observed frequency proportionally into the remaining
///    `total - nonzero_count` space, clamped to [1, 4094].
/// 3. Distribute any remainder to the largest frequencies not yet at the
///    cap of 4095.
///
/// ## Returns
///
/// - `Ok((freqs, cum_freqs))` where `freqs` is `[u32; 256]` of normalised
///   frequencies summing to `1 << scale_bits`, and `cum_freqs` is
///   `[u32; 257]` of cumulative frequencies for the rANS encoder.
/// - `Err(BlockError { kind: Model })` if the input is empty (zero total
///   symbol count).
/// - `Err(BlockError { kind: ResourceLimit })` if a single byte appears
///   more than `u32::MAX` times (practically impossible, but checked).
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

/// Normalize frequencies to sum exactly to `total` using deterministic scaling.
///
/// This is the **canonical normalizer** for the RYGRANS format.  It produces
/// a frequency table that satisfies several invariants:
///
/// - Sum of all entries equals `total` (typically 4096, i.e. `1 << 12`).
/// - Every non-zero input frequency maps to at least 1 in the output.
/// - No output frequency exceeds 4095 (the maximum that fits in the 12-bit
///   packed table field used by the SIMD decoder).
/// - The mapping is fully deterministic: same input frequencies always
///   produce the same output frequencies.
///
/// ## Algorithm
///
/// 1. **Reserve** `nonzero_count` slots.  Because every observed symbol
///    must get at least 1, we subtract `nonzero_count` from `total` to get
///    the `available` pool for proportional scaling.
/// 2. **Scale** each observed frequency: `scaled = 1 + (raw * available / sum)`,
///    clamped to `min(scaled, 4094)`.  The floor of 1 ensures every symbol
///    is represented; the cap of 4094 leaves room for the final remainder
///    distribution.
/// 3. **Distribute remainder** — After proportional scaling, the sum is
///    typically slightly less than `total`.  The remainder is distributed
///    **one unit at a time** to the frequency bin with the largest current
///    value that is below 4095.  This greedy approach produces a smooth
///    distribution biased toward already-frequent symbols, which is the
///    standard behaviour for rANS frequency normalisation.
/// 4. **Fallback** — If for some reason all frequencies are at 4095 and
///    a remainder still exists (should not happen in practice), the first
///    two bins absorb the excess.
///
/// ## Why nonzero_count reservation?
///
/// Without reserving at least 1 per observed symbol, proportional scaling
/// could round a rare symbol's frequency down to 0, which would make that
/// symbol **undecodable** (the decoder would attempt a division by zero).
/// Reserving `nonzero_count` slots before computing proportions guarantees
/// every symbol's floor.
///
/// ## Rationale for remainder distribution
///
/// Proportional scaling with integer arithmetic almost never lands exactly
/// on `total`.  The remainder (typically 1–20 for 256 symbols scaled to
/// 4096) must be distributed in a deterministic way to preserve the sum
/// invariant.  The greedy "largest first" approach is simpler than
/// sophisticated methods like the Hare quota or d'Hondt method, and is
/// sufficient for rANS because the decoder only needs the exact table,
/// not an optimal statistical fit.
fn normalize_frequencies(freqs: &mut [u32], total: u32) {
    let sum: u64 = freqs.iter().map(|&f| f as u64).sum();
    if sum == 0 {
        return;
    }
    let nonzero_count = freqs.iter().filter(|&&f| f > 0).count() as u64;
    let reserved = nonzero_count;
    let available = (total as u64).saturating_sub(reserved);
    let mut allocated: u64 = 0;
    let raw = freqs.to_vec();
    for (i, f) in freqs.iter_mut().enumerate() {
        if raw[i] > 0 {
            let scaled = 1 + ((raw[i] as u64 * available) / sum).min(4094);
            *f = scaled as u32;
            allocated += *f as u64;
        } else {
            *f = 0;
        }
    }
    // Distribute remainder to largest frequencies that aren't at max.
    let mut remaining = (total as u64).saturating_sub(allocated);
    while remaining > 0 {
        let mut best_idx = 0usize;
        let mut best_val = 0u32;
        for i in 0..256 {
            if freqs[i] < 4095 && freqs[i] > best_val {
                best_val = freqs[i];
                best_idx = i;
            }
        }
        if best_val == 0 {
            if freqs[0] < 4095 {
                let add = remaining.min((4095 - freqs[0]) as u64);
                freqs[0] += add as u32;
                remaining -= add;
            } else {
                freqs[1] += 1;
                remaining -= 1;
            }
            continue;
        }
        freqs[best_idx] += 1;
        remaining -= 1;
    }
}

/// Encode data using the WORD_INTERLEAVED16 (16-state) rANS codec.
///
/// This is the primary encoding codec for RYGRANS.  It encodes symbols into
/// 16 interleaved rANS states using a **backward writer** (encoding proceeds
/// from the last symbol to the first).
///
/// ## Why reverse order + reverse lane flush?
///
/// rANS with interleaved lanes encodes each symbol into one of N lanes
/// (here N=16) in a round-robin fashion.  For the decoder to reconstruct
/// the original order, it must read lanes in the **same round-robin order**
/// but in reverse.  The encoder therefore:
///
/// 1. Processes symbols **in reverse order** (`data.len()-1` down to 0),
///    assigning each to `lane = i & 15`.
/// 2. After all symbols are encoded, **flushes states in reverse lane
///    order** (lane 15 down to 0), writing two u16 values per state (the
///    low and high 16 bits of the 32-bit state).
///
/// This arrangement causes the decoder's first read to be the last flush
/// value, which corresponds to the final encoded state at the end of the
/// backward pass — exactly matching the decode algorithm's expectations.
///
/// ## SIMD acceleration
///
/// When the `simd` feature is enabled, the function delegates to
/// `ryg_rans_rs_simd::packed_table::encode_interleaved16`, a vectorised
/// implementation.  Otherwise, it falls back to the manual scalar loop.
///
/// ## Buffer safety
///
/// The output buffer is pre-allocated with capacity `data.len() * 2 + 64`
/// (checked for overflow).  Each symbol may emit at most one u16 flush
/// value, and each final state flush emits exactly 2 u16 values per lane
/// (32 total for 16 lanes).  The `writer` index tracks backward position
/// and returns `ResourceLimit` if exhausted.
///
/// ## Error conditions
///
/// Returns `BlockError { kind: Model }` if a symbol's frequency is zero
/// (should never happen if `normalize_frequencies` ran correctly).
/// Returns `BlockError { kind: ResourceLimit }` if the output buffer
/// would overflow.
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
    #[cfg(feature = "simd")]
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

    #[cfg(not(feature = "simd"))]
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

/// Build 256 × u32 LE serialised frequency bytes for the model.
///
/// Converts the 256-element frequency array into a flat 1024-byte buffer
/// of little-endian u32 values.  This is the wire format stored in the
/// RYGRANS container block after the header.
///
/// ## Format
///
/// - 256 entries × 4 bytes each = 1024 bytes total.
/// - Each entry is stored as native-endian u32 (currently LE on all
///   supported platforms).
/// - If fewer than 256 frequencies are provided (should not happen in
///   normal operation), remaining slots are zero-filled.
///
/// ## Invariant
///
/// The output is always exactly 1024 bytes, enforced by `debug_assert`.
/// This fixed-size model simplifies decoder offset calculations.
pub fn build_model_bytes(freqs: &[u32]) -> Vec<u8> {
    let mut model_data = Vec::with_capacity(1024);
    for &f in freqs.iter().take(256) {
        model_data.extend_from_slice(&f.to_le_bytes());
    }
    for _ in freqs.len()..256 {
        model_data.extend_from_slice(&0u32.to_le_bytes());
    }
    debug_assert_eq!(model_data.len(), 1024);
    model_data
}

/// Build an empty block record for empty input data.
///
/// Produces a minimal RYGRANS container block consisting of only a 104-byte
/// header with **no model and no payload**.  This is the empty-block fast
/// path taken when `encode_single_block` receives zero-length input.
///
/// ## Contents
///
/// - Block tag `"RYGR"` and standard header fields with:
///   - `uncompressed_length = 0`
///   - `payload_length = 0`
///   - `model_length = 0`
///   - `codec_id = 0` (no codec used)
///   - `state_count = 16` (arbitrary, no states are written)
/// - `payload_sha256 = SHA-256(&[])` — hash of the empty payload.
/// - `decoded_sha256 = SHA-256(&[])` — **real hash of the original empty
///   input**.  This is critical for integrity: a verifier can confirm that
///   decoding the empty block produces the empty output, because the
///   stored `decoded_sha256` is the correct hash of that empty output.
///
/// ## Integrity guarantees
///
/// Even though the block carries no payload, the verifier can still detect
/// corruption: if the header is mangled such that a decoder attempts to
/// read a non-empty payload, the decode will either fail or produce wrong
/// output whose hash doesn't match `decoded_sha256`.  If the header stays
/// intact but the payload is spuriously non-empty, the decoder will read
/// zero payload bytes (as instructed by `payload_length = 0`) and produce
/// the correct empty output, whose hash matches.
///
/// ## Returns
///
/// A `Vec<u8>` of exactly 104 bytes (the minimum header size).
fn build_empty_block(block_index: u64) -> Vec<u8> {
    let empty_hash = sha256(&[]);
    let mut buf = Vec::with_capacity(104);
    buf.extend_from_slice(b"RYGR"); // BLOCK_TAG
    buf.extend_from_slice(&(104u16).to_le_bytes()); // header_size
    buf.push(1); // block_version
    buf.push(0); // block_kind = RANS
    buf.extend_from_slice(&block_index.to_le_bytes());
    // Use WORD_INTERLEAVED16 (codec 8) — the default for the parallel encoder.
    // The strict parser requires codec_id to be 7 or 8.
    buf.extend_from_slice(&8u16.to_le_bytes()); // codec_id = WORD_INTERLEAVED16
    buf.push(12); // scale_bits
    buf.push(16); // state_count (must match codec 8)
    buf.push(0); // model_encoding
    buf.extend_from_slice(&[0u8; 3]); // reserved
    buf.extend_from_slice(&0u32.to_le_bytes()); // uncompressed_length
    buf.extend_from_slice(&0u32.to_le_bytes()); // payload_length
    buf.extend_from_slice(&0u32.to_le_bytes()); // model_length
    buf.extend_from_slice(&[0u8; 4]); // reserved2
    buf.extend_from_slice(&empty_hash); // payload_sha256 = SHA-256 of empty payload
    buf.extend_from_slice(&empty_hash); // decoded_sha256 = SHA-256 of empty data
    buf
}

/// Build a RYGRANS container block record (header + model + payload).
///
/// Assembles the complete on-disk/on-wire representation of an encoded
/// block.  This is the primary container construction function for
/// non-empty blocks.
///
/// ## Container layout
///
/// ```text
/// Offset  Size  Field
/// ------  ----  -----
///  0      4     Block tag: "RYGR"
///  4      2     Header size (always 104)
///  6      1     Block version (1)
///  7      1     Block kind (0 = RANS)
///  8      8     Block index (little-endian u64)
/// 16      2     Codec ID (e.g. 8 for INTERLEAVED16)
/// 18      1     Scale bits (typically 12)
/// 19      1     State count (typically 16)
/// 20      1     Model encoding (0 = raw 256×u32 LE)
/// 21      3     Reserved
/// 24      4     Uncompressed length (u32 LE)
/// 28      4     Payload length (u32 LE)
/// 32      4     Model length (u32 LE, always 1024)
/// 36      4     Reserved
/// 40     32     Payload SHA-256
/// 72     32     Decoded SHA-256
///104   1024     Model data (256 × u32 LE frequencies)
///1128    ...    Encoded payload bytes
/// ```
///
/// Total header size is 104 bytes, model is 1024 bytes, followed by the
/// variable-length payload.  The header layout matches what `parse_block_header`
/// in the decoder expects.
///
/// ## Hashing
///
/// - `payload_sha256` is computed from the **flattened byte representation**
///   of the encoded u16 payload, **not** from the u16 words directly.  This
///   ensures that the verifier can hash the raw payload bytes without needing
///   to understand the u16 encoding.
/// - `decoded_sha256` is the SHA-256 of the **original uncompressed input**
///   data, passed in as a parameter from `encode_single_block`.
///
/// ## Returns
///
/// A `Vec<u8>` containing the complete block: header (104 bytes) + model
/// (1024 bytes) + payload (variable).
fn build_block_record(
    block_index: u64,
    codec_id: u16,
    scale_bits: u8,
    uncompressed_length: u32,
    payload: &[u8],
    freqs: &[u32],
    decoded_hash: [u8; 32],
) -> Vec<u8> {
    let state_count: u8 = 16;
    // Serialise 256 frequency values as u32 LE (1024 bytes total)
    let mut model_data = Vec::with_capacity(1024);
    for &f in freqs.iter().take(256) {
        model_data.extend_from_slice(&f.to_le_bytes());
    }
    // Pad if fewer than 256 frequencies (shouldn't happen, but be safe)
    for _ in freqs.len()..256 {
        model_data.extend_from_slice(&0u32.to_le_bytes());
    }
    debug_assert_eq!(model_data.len(), 1024);

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
    buf.push(0); // model_encoding=0 means raw 256×u32 LE frequencies
    buf.extend_from_slice(&[0u8; 3]); // reserved: 21-23
    buf.extend_from_slice(&uncompressed_length.to_le_bytes()); // 24-27
    buf.extend_from_slice(&(payload_bytes.len() as u32).to_le_bytes()); // 28-31
    buf.extend_from_slice(&(model_data.len() as u32).to_le_bytes()); // 32-35
    buf.extend_from_slice(&[0u8; 4]); // reserved2: 36-39
    buf.extend_from_slice(&payload_sha256); // 40-71
    buf.extend_from_slice(&decoded_hash); // 72-103 — real decoded SHA-256
    buf.extend_from_slice(&model_data);
    buf.extend_from_slice(&payload_bytes);
    buf
}

/// Compute SHA-256 of a byte slice, returning a fixed-size 32-byte array.
///
/// Uses the `sha2` crate (`Sha256` digest).  This is used throughout the
/// encoding and verification pipeline for:
///
/// - **Payload integrity** — `payload_sha256` is stored in each block's
///   header so the verifier can confirm the payload was not corrupted.
/// - **Decoded data integrity** — `decoded_sha256` is stored in each
///   block's header so the verifier can confirm that decoding produces
///   the expected output.
/// - **Model integrity** — `model_hash` is stored in `EncodedBlockResult`
///   metadata (but not in the container header) for debugging and testing.
///
/// ## Consistency note
///
/// The hashing is done over the flat byte representation.  For payload
/// hashing, `encode_single_block` flattens the u16 vector to bytes via
/// `iter().flat_map(|w| w.to_le_bytes())` before passing to this function,
/// ensuring that the stored `payload_sha256` matches what the verifier
/// computes from the raw payload bytes.
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
    ///
    /// This convenience API creates an internal cancellation token and delegates to
    /// [`Self::encode_blocks_with_cancel`].
    pub fn encode_blocks(
        blocks: impl IntoIterator<Item = EncodeBlockJob>,
        config: &ParallelConfig,
    ) -> Result<OrderedEncodedBlocks, ParallelError> {
        Self::encode_blocks_with_cancel(blocks, config, None)
    }

    /// Encode blocks in parallel with an optional external cancellation token.
    ///
    /// When `external_cancel` is provided, workers check it cooperatively and
    /// the operation returns [`ParallelError::Cancelled`] with completion counts
    /// if cancellation is observed before all blocks complete.  The operation
    /// never returns `Ok` with fewer results than declared blocks.
    pub fn encode_blocks_with_cancel(
        blocks: impl IntoIterator<Item = EncodeBlockJob>,
        config: &ParallelConfig,
        external_cancel: Option<std::sync::Arc<crate::cancellation::CancellationToken>>,
    ) -> Result<OrderedEncodedBlocks, ParallelError> {
        let jobs: Vec<EncodeBlockJob> = blocks.into_iter().collect();
        if jobs.is_empty() {
            return Ok(OrderedEncodedBlocks {
                blocks: Vec::new(),
                execution: crate::job::ExecutionMetadata {
                    requested_workers: 0,
                    effective_workers: 0,
                    queue_capacity: 0,
                    block_count: 0,
                    declared_blocks: 0,
                    completed_blocks: 0,
                    cancelled: false,
                },
            });
        }

        let block_count = jobs.len();
        let worker_count = crate::resource::effective_worker_count(config, block_count)?;
        let queue_capacity = config.max_in_flight_blocks.get().max(worker_count);

        // ---- max_buffered_input_bytes enforcement (encode) ----
        let input_bytes: u64 = jobs.iter().map(|j| j.data.len() as u64).sum();
        if input_bytes > config.max_buffered_input_bytes {
            return Err(ParallelError::ResourceLimit(format!(
                "max_buffered_input_bytes exceeded: {} > {}",
                input_bytes, config.max_buffered_input_bytes
            )));
        }

        // Convert jobs to tasks
        let tasks: Vec<EncodeTask> = jobs.into_iter().map(|job| EncodeTask { job }).collect();

        // Run tasks in parallel with the external cancellation token.
        let report: ExecutorReport<Result<EncodedBlockResult, BlockError>> = run_tasks(
            tasks,
            worker_count,
            queue_capacity,
            config.worker_stack_size,
            external_cancel,
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
        let completed_blocks = ordered_blocks.len();
        let cancelled = report.cancelled;

        Ok(OrderedEncodedBlocks {
            blocks: ordered_blocks,
            execution: crate::job::ExecutionMetadata {
                requested_workers: worker_count,
                effective_workers: report.effective_workers,
                queue_capacity,
                block_count,
                declared_blocks: block_count,
                completed_blocks,
                cancelled,
            },
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
