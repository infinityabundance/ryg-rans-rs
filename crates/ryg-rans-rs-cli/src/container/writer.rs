//! # Container writer — serialize RYGRANS containers

use crate::container::block::Block;
use crate::container::footer::FileFooter;
use crate::container::header::FileHeader;
use crate::error::AppError;
use sha2::{Digest, Sha256};
use std::io::Write;

/// Container writer that produces canonical RYGRANS containers.
pub struct ContainerWriter<W: Write> {
    writer: W,
    header_written: bool,
    footer_written: bool,
    block_count: u64,
    total_uncompressed: u64,
    total_payload: u64,
    hasher: Sha256,
    decoded_hasher: Sha256,
}

impl<W: Write> ContainerWriter<W> {
    /// Create a new container writer.
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            header_written: false,
            footer_written: false,
            block_count: 0,
            total_uncompressed: 0,
            total_payload: 0,
            hasher: Sha256::new(),
            decoded_hasher: Sha256::new(),
        }
    }

    /// Write the file header.
    pub fn write_header(&mut self, header: &FileHeader) -> Result<(), AppError> {
        let bytes = header.to_bytes();
        self.writer.write_all(&bytes).map_err(|e| {
            AppError::Io(crate::error::IoError {
                path: None,
                detail: format!("write header: {}", e),
            })
        })?;
        self.hasher.update(&bytes);
        self.header_written = true;
        Ok(())
    }

    /// Write a block record.
    pub fn write_block(&mut self, block: &Block, decoded_data: &[u8]) -> Result<(), AppError> {
        let bytes = block.to_bytes();
        self.writer.write_all(&bytes).map_err(|e| {
            AppError::Io(crate::error::IoError {
                path: None,
                detail: format!("write block {}: {}", block.block_index, e),
            })
        })?;

        self.hasher.update(&bytes);
        self.decoded_hasher.update(decoded_data);

        self.block_count += 1;
        self.total_uncompressed = self
            .total_uncompressed
            .checked_add(block.uncompressed_length as u64)
            .ok_or_else(|| {
                AppError::Format(crate::error::FormatError {
                    detail: "total uncompressed overflow".into(),
                    block_index: Some(block.block_index),
                    offset: None,
                })
            })?;
        self.total_payload = self
            .total_payload
            .checked_add(block.payload.len() as u64)
            .ok_or_else(|| {
                AppError::Format(crate::error::FormatError {
                    detail: "total payload overflow".into(),
                    block_index: Some(block.block_index),
                    offset: None,
                })
            })?;
        Ok(())
    }

    /// Write the file footer.  Must be called after all blocks.
    pub fn write_footer(&mut self) -> Result<FileFooter, AppError> {
        use sha2::Digest;
        let hasher_clone = std::mem::replace(&mut self.hasher, Sha256::new());
        let container_hash: [u8; 32] = hasher_clone.finalize().into();
        let dec_hasher_clone = std::mem::replace(&mut self.decoded_hasher, Sha256::new());
        let decoded_hash: [u8; 32] = dec_hasher_clone.finalize().into();

        let footer = FileFooter::compute(
            self.block_count,
            self.total_uncompressed,
            self.total_payload,
            container_hash,
            decoded_hash,
        );

        let bytes = footer.to_bytes();
        self.writer.write_all(&bytes).map_err(|e| {
            AppError::Io(crate::error::IoError {
                path: None,
                detail: format!("write footer: {}", e),
            })
        })?;

        self.footer_written = true;
        Ok(footer)
    }

    /// Flush the underlying writer.
    pub fn flush(&mut self) -> Result<(), AppError> {
        self.writer.flush().map_err(|e| {
            AppError::Io(crate::error::IoError {
                path: None,
                detail: format!("flush: {}", e),
            })
        })
    }

    /// Return the inner writer.
    pub fn into_inner(self) -> W {
        self.writer
    }
}
