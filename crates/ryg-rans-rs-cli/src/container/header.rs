//! # File header — fixed-size, exactly 32 bytes.

use crate::container::{HEADER_SIZE, MAGIC, MAJOR_VERSION, MINOR_VERSION};
use crate::error::{AppError, FormatError};

/// Canonical file header flags bitfield.
pub mod flags {
    pub const GLOBAL_MODEL: u16 = 0x0001;
    pub const ALWAYS_COMPRESS: u16 = 0x0002;
    pub const HAS_EXTRA_HEADER: u16 = 0x0004;
    /// Mask of all defined flags.
    pub const KNOWN_FLAGS: u16 = GLOBAL_MODEL | ALWAYS_COMPRESS | HAS_EXTRA_HEADER;
}

/// Codec-independent file header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    pub major_version: u16,
    pub minor_version: u16,
    pub flags: u16,
    pub default_codec_id: u16,
    pub default_scale_bits: u8,
    pub default_model_mode: u8,
    pub declared_block_size: u32,
}

impl Default for FileHeader {
    fn default() -> Self {
        Self {
            major_version: MAJOR_VERSION,
            minor_version: MINOR_VERSION,
            flags: 0,
            default_codec_id: 2, // BYTE_INTERLEAVED2
            default_scale_bits: 12,
            default_model_mode: 0, // per-block
            declared_block_size: crate::limits::DEFAULT_BLOCK_SIZE,
        }
    }
}

impl FileHeader {
    /// Serialize to bytes.
    pub fn to_bytes(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..8].copy_from_slice(MAGIC);
        buf[8..10].copy_from_slice(&self.major_version.to_le_bytes());
        buf[10..12].copy_from_slice(&self.minor_version.to_le_bytes());
        buf[12..14].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        buf[14..16].copy_from_slice(&self.flags.to_le_bytes());
        buf[16..18].copy_from_slice(&self.default_codec_id.to_le_bytes());
        buf[18] = self.default_scale_bits;
        buf[19] = self.default_model_mode;
        buf[20..24].copy_from_slice(&self.declared_block_size.to_le_bytes());
        // reserved bytes 24..32 remain zero
        buf
    }

    /// Parse from bytes.  Returns `Err` for any invariant violation.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, AppError> {
        if bytes.len() < HEADER_SIZE {
            return Err(AppError::Format(FormatError {
                detail: "header too short".into(),
                block_index: None,
                offset: Some(0),
            }));
        }

        let magic = &bytes[0..8];
        if magic != MAGIC {
            return Err(AppError::Format(FormatError {
                detail: format!("bad magic: expected RYGRANS\\0, got {:02x?}", magic),
                block_index: None,
                offset: Some(0),
            }));
        }

        let major_version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if major_version != MAJOR_VERSION {
            return Err(AppError::Format(FormatError {
                detail: format!("unsupported major version: {}", major_version),
                block_index: None,
                offset: Some(8),
            }));
        }

        let minor_version = u16::from_le_bytes([bytes[10], bytes[11]]);
        let header_length = u16::from_le_bytes([bytes[12], bytes[13]]);
        if header_length != HEADER_SIZE as u16 {
            return Err(AppError::Format(FormatError {
                detail: format!("unexpected header length: {}", header_length),
                block_index: None,
                offset: Some(12),
            }));
        }

        let flags = u16::from_le_bytes([bytes[14], bytes[15]]);
        // Reject unknown required flags
        let unknown_flags = flags & !flags::KNOWN_FLAGS;
        if unknown_flags != 0 {
            return Err(AppError::Format(FormatError {
                detail: format!("unknown flags: {:04x}", unknown_flags),
                block_index: None,
                offset: Some(14),
            }));
        }

        // Check reserved bytes (24..32) are zero
        if bytes[24..32].iter().any(|&b| b != 0) {
            return Err(AppError::Format(FormatError {
                detail: "non-zero reserved bytes in header".into(),
                block_index: None,
                offset: Some(24),
            }));
        }

        let default_codec_id = u16::from_le_bytes([bytes[16], bytes[17]]);
        let default_scale_bits = bytes[18];
        let default_model_mode = bytes[19];
        let declared_block_size = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);

        if declared_block_size == 0 {
            return Err(AppError::Format(FormatError {
                detail: "declared block size is zero".into(),
                block_index: None,
                offset: Some(20),
            }));
        }

        Ok(Self {
            major_version,
            minor_version,
            flags,
            default_codec_id,
            default_scale_bits,
            default_model_mode,
            declared_block_size,
        })
    }

    /// Whether the GLOBAL_MODEL flag is set.
    pub fn has_global_model(&self) -> bool {
        self.flags & flags::GLOBAL_MODEL != 0
    }
}
