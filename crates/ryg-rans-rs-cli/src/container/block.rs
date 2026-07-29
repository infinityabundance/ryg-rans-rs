//! # Block record — header, model, payload

use crate::container::codec;
use crate::container::{
    BLOCK_HEADER_SIZE, BLOCK_KIND_RANS, BLOCK_KIND_RAW, BLOCK_KIND_RLE, BLOCK_TAG,
};
use crate::error::{AppError, FormatError, IntegrityError};
use sha2::{Digest, Sha256};

/// A decoded block record.
#[derive(Debug, Clone)]
pub struct Block {
    pub block_index: u64,
    pub block_kind: u8,
    pub codec_id: u16,
    pub scale_bits: u8,
    pub state_count: u8,
    pub uncompressed_length: u32,
    pub payload: Vec<u8>,
    pub model_data: Vec<u8>,
    pub payload_sha256: [u8; 32],
    pub decoded_sha256: [u8; 32],
}

impl Block {
    /// Create a new RANS block.
    pub fn new_rans(
        block_index: u64,
        codec_id: u16,
        scale_bits: u8,
        state_count: u8,
        uncompressed_length: u32,
        payload: Vec<u8>,
        model_data: Vec<u8>,
    ) -> Self {
        let payload_sha256 = sha256(&payload);
        let decoded_sha256 = [0u8; 32]; // computed by caller after decode
        Self {
            block_index,
            block_kind: BLOCK_KIND_RANS,
            codec_id,
            scale_bits,
            state_count,
            uncompressed_length,
            payload,
            model_data,
            payload_sha256,
            decoded_sha256,
        }
    }

    /// Create a new RAW block.
    pub fn new_raw(block_index: u64, data: Vec<u8>) -> Self {
        let payload_sha256 = sha256(&data);
        let decoded_sha256 = payload_sha256; // raw: payload == decoded
        Self {
            block_index,
            block_kind: BLOCK_KIND_RAW,
            codec_id: 0,
            scale_bits: 0,
            state_count: 0,
            uncompressed_length: data.len() as u32,
            payload: data,
            model_data: Vec::new(),
            payload_sha256,
            decoded_sha256,
        }
    }

    /// Create a new RLE block.
    pub fn new_rle(block_index: u64, symbol: u8, count: u32) -> Self {
        let payload = vec![symbol];
        let payload_sha256 = sha256(&payload);
        let decoded_data = vec![symbol; count as usize];
        let decoded_sha256 = sha256(&decoded_data);
        Self {
            block_index,
            block_kind: BLOCK_KIND_RLE,
            codec_id: 0,
            scale_bits: 0,
            state_count: 0,
            uncompressed_length: count,
            payload,
            model_data: Vec::new(),
            payload_sha256,
            decoded_sha256,
        }
    }

    /// Serialize block header to bytes (104 bytes).
    pub fn header_to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(BLOCK_HEADER_SIZE);
        buf.extend_from_slice(BLOCK_TAG);
        buf.extend_from_slice(&(BLOCK_HEADER_SIZE as u16).to_le_bytes());
        buf.push(1); // block_version
        buf.push(self.block_kind);
        buf.extend_from_slice(&self.block_index.to_le_bytes());
        buf.extend_from_slice(&self.codec_id.to_le_bytes());
        buf.push(self.scale_bits);
        buf.push(self.state_count);
        buf.push(if self.model_data.is_empty() { 0 } else { 0 }); // model_encoding
        buf.push(0); // integrity_id
        buf.extend_from_slice(&[0u8; 2]); // reserved
        buf.extend_from_slice(&self.uncompressed_length.to_le_bytes());
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(self.model_data.len() as u32).to_le_bytes());
        buf.extend_from_slice(&[0u8; 4]); // reserved2
        buf.extend_from_slice(&self.payload_sha256);
        buf.extend_from_slice(&self.decoded_sha256);
        buf
    }

    /// Parse a block header from bytes.  Returns the header + number of bytes consumed.
    pub fn parse_header(
        bytes: &[u8],
        expected_index: u64,
    ) -> Result<(BlockHeaderInfo, usize), AppError> {
        if bytes.len() < BLOCK_HEADER_SIZE {
            return Err(AppError::Format(FormatError {
                detail: "block header too short".into(),
                block_index: Some(expected_index),
                offset: Some(0),
            }));
        }

        let tag = &bytes[0..4];
        if tag != BLOCK_TAG {
            return Err(AppError::Format(FormatError {
                detail: format!("bad block tag: {:02x?}", tag),
                block_index: Some(expected_index),
                offset: Some(0),
            }));
        }

        let _block_header_length = u16::from_le_bytes([bytes[4], bytes[5]]);
        let block_version = bytes[6];
        if block_version != 1 {
            return Err(AppError::Format(FormatError {
                detail: format!("unsupported block version: {}", block_version),
                block_index: Some(expected_index),
                offset: Some(6),
            }));
        }

        let block_kind = bytes[7];
        if block_kind > 2 {
            return Err(AppError::Format(FormatError {
                detail: format!("unknown block kind: {}", block_kind),
                block_index: Some(expected_index),
                offset: Some(7),
            }));
        }

        let block_index = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        if block_index != expected_index {
            return Err(AppError::Format(FormatError {
                detail: format!("expected block {}, got {}", expected_index, block_index),
                block_index: Some(expected_index),
                offset: Some(8),
            }));
        }

        let codec_id = u16::from_le_bytes([bytes[16], bytes[17]]);
        if block_kind == BLOCK_KIND_RANS && !codec::is_supported(codec_id) {
            return Err(AppError::Format(FormatError {
                detail: format!("unsupported codec id: {}", codec_id),
                block_index: Some(block_index),
                offset: Some(16),
            }));
        }

        let scale_bits = bytes[18];
        let state_count = bytes[19];

        // reserved bytes 22..23 must be zero
        if bytes[22] != 0 || bytes[23] != 0 {
            return Err(AppError::Format(FormatError {
                detail: "non-zero reserved bytes in block header".into(),
                block_index: Some(block_index),
                offset: Some(22),
            }));
        }

        let uncompressed_length = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let payload_length = u32::from_le_bytes([bytes[28], bytes[29], bytes[30], bytes[31]]);
        let model_length = u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]);

        // reserved2 bytes 36..39 must be zero
        if bytes[36..40].iter().any(|&b| b != 0) {
            return Err(AppError::Format(FormatError {
                detail: "non-zero reserved2 bytes in block header".into(),
                block_index: Some(block_index),
                offset: Some(36),
            }));
        }

        let mut payload_sha256 = [0u8; 32];
        payload_sha256.copy_from_slice(&bytes[40..72]);
        let mut decoded_sha256 = [0u8; 32];
        decoded_sha256.copy_from_slice(&bytes[72..104]);

        Ok((
            BlockHeaderInfo {
                block_index,
                block_kind,
                codec_id,
                scale_bits,
                state_count,
                uncompressed_length,
                payload_length,
                model_length,
                payload_sha256,
                decoded_sha256,
            },
            BLOCK_HEADER_SIZE,
        ))
    }

    /// Serialize full block record (header + model + payload) to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = self.header_to_bytes();
        buf.extend_from_slice(&self.model_data);
        buf.extend_from_slice(&self.payload);
        buf
    }
}

/// Parsed block header info (before reading model/payload).
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

/// Compute SHA-256 of a byte slice.
fn sha256(data: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}
