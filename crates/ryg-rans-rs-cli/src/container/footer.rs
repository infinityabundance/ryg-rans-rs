//! # File footer — terminal totals and stream hashes

use crate::container::FOOTER_SIZE;
use crate::error::{AppError, FormatError, IntegrityError};

/// Container footer — 104 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFooter {
    pub flags: u8,
    pub block_count: u64,
    pub total_uncompressed_length: u64,
    pub total_payload_length: u64,
    pub container_sha256: [u8; 32],
    pub decoded_stream_sha256: [u8; 32],
}

impl FileFooter {
    /// Compute the footer from accumulated totals and stream hashes.
    pub fn compute(
        block_count: u64,
        total_uncompressed: u64,
        total_payload: u64,
        container_hash: [u8; 32],
        decoded_hash: [u8; 32],
    ) -> Self {
        Self {
            flags: 0,
            block_count,
            total_uncompressed_length: total_uncompressed,
            total_payload_length: total_payload,
            container_sha256: container_hash,
            decoded_stream_sha256: decoded_hash,
        }
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> [u8; FOOTER_SIZE] {
        let mut buf = [0u8; FOOTER_SIZE];
        buf[0..4].copy_from_slice(b"END1");
        buf[4..6].copy_from_slice(&(FOOTER_SIZE as u16).to_le_bytes());
        buf[6] = 1; // footer_version
        buf[7] = self.flags;
        buf[8..16].copy_from_slice(&self.block_count.to_le_bytes());
        buf[16..24].copy_from_slice(&self.total_uncompressed_length.to_le_bytes());
        buf[24..32].copy_from_slice(&self.total_payload_length.to_le_bytes());
        buf[32..64].copy_from_slice(&self.container_sha256);
        buf[64..96].copy_from_slice(&self.decoded_stream_sha256);
        // reserved bytes 96..104 remain zero
        buf
    }

    /// Parse footer from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AppError> {
        if bytes.len() < FOOTER_SIZE {
            return Err(AppError::Format(FormatError {
                detail: "footer too short".into(),
                block_index: None,
                offset: None,
            }));
        }

        let tag = &bytes[0..4];
        if tag != b"END1" {
            return Err(AppError::Format(FormatError {
                detail: format!("bad footer tag: {:02x?}", tag),
                block_index: None,
                offset: None,
            }));
        }

        let _footer_length = u16::from_le_bytes([bytes[4], bytes[5]]);
        let footer_version = bytes[6];
        if footer_version != 1 {
            return Err(AppError::Format(FormatError {
                detail: format!("unsupported footer version: {}", footer_version),
                block_index: None,
                offset: None,
            }));
        }

        let flags = bytes[7];
        if flags != 0 {
            return Err(AppError::Format(FormatError {
                detail: format!("non-zero footer flags: {}", flags),
                block_index: None,
                offset: None,
            }));
        }

        let block_count = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);

        let total_uncompressed_length = u64::from_le_bytes([
            bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
        ]);

        let total_payload_length = u64::from_le_bytes([
            bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
        ]);

        let mut container_sha256 = [0u8; 32];
        container_sha256.copy_from_slice(&bytes[32..64]);
        let mut decoded_stream_sha256 = [0u8; 32];
        decoded_stream_sha256.copy_from_slice(&bytes[64..96]);

        // reserved bytes 96..104
        if bytes[96..104].iter().any(|&b| b != 0) {
            return Err(AppError::Format(FormatError {
                detail: "non-zero reserved bytes in footer".into(),
                block_index: None,
                offset: None,
            }));
        }

        Ok(Self {
            flags,
            block_count,
            total_uncompressed_length,
            total_payload_length,
            container_sha256,
            decoded_stream_sha256,
        })
    }

    /// Verify footer totals against accumulated values.
    pub fn verify_totals(
        &self,
        block_count: u64,
        total_uncompressed: u64,
        total_payload: u64,
    ) -> Result<(), AppError> {
        if self.block_count != block_count {
            return Err(AppError::Integrity(IntegrityError {
                detail: format!(
                    "footer block count mismatch: declared {}, actual {}",
                    self.block_count, block_count
                ),
                block_index: None,
            }));
        }
        if self.total_uncompressed_length != total_uncompressed {
            return Err(AppError::Integrity(IntegrityError {
                detail: format!(
                    "footer uncompressed total mismatch: declared {}, actual {}",
                    self.total_uncompressed_length, total_uncompressed
                ),
                block_index: None,
            }));
        }
        if self.total_payload_length != total_payload {
            return Err(AppError::Integrity(IntegrityError {
                detail: format!(
                    "footer payload total mismatch: declared {}, actual {}",
                    self.total_payload_length, total_payload
                ),
                block_index: None,
            }));
        }
        Ok(())
    }
}
