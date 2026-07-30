//! # Block format constants and strict parser for RYGRANS container blocks.
//!
//! Shared between the encoder and decoder to ensure consistent layout.
//! Format matches the CLI crate's `Block::header_to_bytes()` layout:
//!
//! ```text
//! Offset  Size  Field
//! 0       4     BLOCK_TAG ("RYGR")
//! 4       2     header_size (u16 LE, must be 104)
//! 6       1     block_version (must be 1)
//! 7       1     block_kind (0=RANS, 1=RAW, 2=RLE)
//! 8       8     block_index (u64 LE)
//! 16      2     codec_id (u16 LE, must be 7 or 8)
//! 18      1     scale_bits (must be 1..=15)
//! 19      1     state_count (must match codec_id: 8 for codec 7, 16 for codec 8)
//! 20      1     model_encoding (0=raw freqs, 1=uniform, 2=RLE)
//! 21      3     reserved (must be zero)
//! 24      4     uncompressed_length (u32 LE)
//! 28      4     payload_length (u32 LE)
//! 32      4     model_length (u32 LE)
//! 36      4     reserved2 (must be zero)
//! 40      32    payload_sha256
//! 72      32    decoded_sha256
//! 104     var   model_data
//! 104+ml  var   payload
//! ```
//!
//! ## Strict Parsing
//!
//! `parse_block_header()` enforces every field constraint:
//!
//! - **header_size** must equal 104 — rejects unexpected header lengths
//! - **block_version** must equal 1 — future-proofing for format evolution
//! - **reserved bytes** must be zero — prevents ambiguity in future field assignments
//! - **scale_bits** validated BEFORE any shift operation — prevents panic on invalid shift
//! - **codec_id** must be 7 (8-way) or 8 (16-way) — only supported formats
//! - **state_count** must match codec_id — 8 for codec 7, 16 for codec 8
//! - **model_encoding** must be 0, 1, or 2
//! - **No trailing bytes** — data must be exactly header + model + payload
//! - **Minimum payload size** for RANS blocks: at least `state_count * 2` bytes for initial states

use std::vec::Vec;

pub const BLOCK_HEADER_SIZE: usize = 104;
pub const BLOCK_TAG: &[u8; 4] = b"RYGR";
pub const BLOCK_KIND_RANS: u8 = 0;
pub const BLOCK_KIND_RAW: u8 = 1;
pub const BLOCK_KIND_RLE: u8 = 2;
pub const CODEC_WORD_INTERLEAVED16: u16 = 8;
pub const CODEC_WORD_INTERLEAVED8: u16 = 7;
pub const MODEL_ENCODING_RAW_FREQS: u8 = 0;

/// Validated block header info.
#[derive(Debug, Clone)]
pub struct BlockHeaderInfo {
    pub block_index: u64,
    pub block_kind: u8,
    pub codec_id: u16,
    pub scale_bits: u8,
    pub state_count: u8,
    pub uncompressed_length: u32,
    pub payload_length: u32,
    pub model_length: u32,
    pub payload_sha256: [u8; 32],
    pub decoded_sha256: [u8; 32],
}

/// Parse and validate a RYGRANS block header from raw bytes with strict validation.
///
/// # Strict Validation Rules
///
/// 1. Header must be at least 104 bytes
/// 2. Block tag must be "RYGR"
/// 3. header_size field must equal 104 — rejects unexpected header layouts
/// 4. block_version must equal 1 — rejects unknown format versions
/// 5. block_kind must be 0 (RANS), 1 (RAW), or 2 (RLE)
/// 6. Reserved bytes (21-23, 36-39) must be zero
/// 7. scale_bits validated BEFORE any shift — range 1..=15
/// 8. codec_id must be 7 (WORD_INTERLEAVED8) or 8 (WORD_INTERLEAVED16)
/// 9. state_count must match codec_id (8 for codec 7, 16 for codec 8)
/// 10. model_encoding must be 0, 1, or 2
/// 11. No trailing bytes — exact record length enforced
/// 12. Minimum payload size for non-empty RANS blocks
/// 13. Uncompressed length capped at 256 MiB
///
/// # Returns
///
/// `(BlockHeaderInfo, model_offset)` on success, or `Err(String)` with a descriptive message.
pub fn parse_block_header(
    data: &[u8],
    expected_index: u64,
) -> Result<(BlockHeaderInfo, usize), String> {
    if data.len() < BLOCK_HEADER_SIZE {
        return Err(format!(
            "block header too short: {} < {}",
            data.len(),
            BLOCK_HEADER_SIZE
        ));
    }
    if &data[0..4] != BLOCK_TAG {
        return Err(format!("bad block tag: {:02x?}", &data[0..4]));
    }

    // ---- Strict field validation ----

    // header_size must equal 104 (u16 LE)
    let hdr_size = u16::from_le_bytes([data[4], data[5]]);
    if hdr_size as usize != BLOCK_HEADER_SIZE {
        return Err(format!(
            "header_size mismatch: expected {}, got {}",
            BLOCK_HEADER_SIZE, hdr_size
        ));
    }

    // block_version must be 1
    if data[6] != 1 {
        return Err(format!("unsupported block version: {}", data[6]));
    }

    // block_kind: 0=RANS, 1=RAW, 2=RLE
    let bk = data[7];
    if bk > 2 {
        return Err(format!("unknown block kind: {}", bk));
    }

    // block_index (u64 LE)
    let bi = u64::from_le_bytes(data[8..16].try_into().unwrap());
    if bi != expected_index {
        return Err(format!("expected block {}, got {}", expected_index, bi));
    }

    // codec_id (u16 LE) — must be 7 or 8
    let ci = u16::from_le_bytes([data[16], data[17]]);
    if ci != 7 && ci != 8 {
        return Err(format!("unsupported codec_id: {} (must be 7 or 8)", ci));
    }

    // scale_bits — validate BEFORE any shift operation.
    // This is critical: an attacker can set scale_bits to any value.
    // We must check the range BEFORE computing 1u32 << scale_bits.
    let sb = data[18];
    if !(1..=15).contains(&sb) {
        return Err(format!(
            "invalid scale_bits: {} (must be 1..=15 for Word rANS)",
            sb
        ));
    }

    // state_count — must match codec_id
    let sc = data[19];
    match ci {
        CODEC_WORD_INTERLEAVED8 => {
            if sc != 8 {
                return Err(format!("codec 7 requires 8 states, got {}", sc));
            }
        }
        CODEC_WORD_INTERLEAVED16 => {
            if sc != 16 {
                return Err(format!("codec 8 requires 16 states, got {}", sc));
            }
        }
        _ => unreachable!(), // validated above
    }

    // model_encoding: 0=raw freqs, 1=uniform, 2=RLE
    let me = data[20];
    if me > 2 {
        return Err(format!("unsupported model_encoding: {}", me));
    }

    // Reserved bytes 21-23 must be zero
    if data[21] != 0 || data[22] != 0 || data[23] != 0 {
        return Err(format!(
            "reserved bytes [21..24] must be zero, got {:02x?}",
            &data[21..24]
        ));
    }

    // Length fields (u32 LE)
    let ul = u32::from_le_bytes(data[24..28].try_into().unwrap());
    let pl = u32::from_le_bytes(data[28..32].try_into().unwrap());
    let ml = u32::from_le_bytes(data[32..36].try_into().unwrap());

    // Reserved2 bytes 36-39 must be zero
    if data[36] != 0 || data[37] != 0 || data[38] != 0 || data[39] != 0 {
        return Err(format!(
            "reserved2 bytes [36..40] must be zero, got {:02x?}",
            &data[36..40]
        ));
    }

    // SHA-256 hashes
    let mut psh = [0u8; 32];
    psh.copy_from_slice(&data[40..72]);
    let mut dsh = [0u8; 32];
    dsh.copy_from_slice(&data[72..104]);

    // ---- Length validation ----

    // Check for overflow in model and payload offsets
    let model_end = BLOCK_HEADER_SIZE
        .checked_add(ml as usize)
        .ok_or_else(|| format!("model length overflow: {}", ml))?;
    let payload_end = model_end
        .checked_add(pl as usize)
        .ok_or_else(|| format!("payload length overflow: {}", pl))?;

    // Data must be large enough to contain header + model + payload
    if payload_end > data.len() {
        return Err(format!(
            "block data truncated: header+model+payload={} > data.len={}",
            payload_end,
            data.len()
        ));
    }

    // Strict: NO trailing bytes — data must be exactly header+model+payload.
    // This prevents silent acceptance of malformed or malicious containers
    // that append extra data after the expected block boundary.
    if payload_end != data.len() {
        return Err(format!(
            "trailing bytes: expected {} header+model+payload bytes, got {} total ({} extra)",
            payload_end,
            data.len(),
            data.len() - payload_end
        ));
    }

    // Output size sanity bound: 256 MiB per block maximum
    if ul > 256 * 1024 * 1024 {
        return Err(format!(
            "uncompressed length too large: {} (max 256 MiB)",
            ul
        ));
    }

    // For non-empty RANS blocks, the payload must be at least large enough
    // for the initial states: each state needs 2 bytes (one u16 word).
    // Total minimum = state_count * 2 bytes.
    if bk == BLOCK_KIND_RANS && ul > 0 {
        let min_init = (sc as u32) * 2;
        if pl < min_init {
            return Err(format!(
                "payload too short for {} initial states: {} < {} (need at least {} bytes)",
                sc, pl, min_init, min_init
            ));
        }
    }

    Ok((
        BlockHeaderInfo {
            block_index: bi,
            block_kind: bk,
            codec_id: ci,
            scale_bits: sb,
            state_count: sc,
            uncompressed_length: ul,
            payload_length: pl,
            model_length: ml,
            payload_sha256: psh,
            decoded_sha256: dsh,
        },
        BLOCK_HEADER_SIZE,
    ))
}

/// Build a RYGRANS block header bytes.
pub fn build_header(
    block_index: u64,
    block_kind: u8,
    codec_id: u16,
    scale_bits: u8,
    state_count: u8,
    model_encoding: u8,
    uncompressed_length: u32,
    payload_length: u32,
    model_length: u32,
    payload_sha256: [u8; 32],
    decoded_sha256: [u8; 32],
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(BLOCK_HEADER_SIZE);
    buf.extend_from_slice(BLOCK_TAG);
    buf.extend_from_slice(&(BLOCK_HEADER_SIZE as u16).to_le_bytes());
    buf.push(1); // block_version
    buf.push(block_kind);
    buf.extend_from_slice(&block_index.to_le_bytes());
    buf.extend_from_slice(&codec_id.to_le_bytes());
    buf.push(scale_bits);
    buf.push(state_count);
    buf.push(model_encoding);
    buf.extend_from_slice(&[0u8; 3]); // reserved
    buf.extend_from_slice(&uncompressed_length.to_le_bytes());
    buf.extend_from_slice(&payload_length.to_le_bytes());
    buf.extend_from_slice(&model_length.to_le_bytes());
    buf.extend_from_slice(&[0u8; 4]); // reserved2
    buf.extend_from_slice(&payload_sha256);
    buf.extend_from_slice(&decoded_sha256);
    debug_assert_eq!(buf.len(), BLOCK_HEADER_SIZE);
    buf
}
