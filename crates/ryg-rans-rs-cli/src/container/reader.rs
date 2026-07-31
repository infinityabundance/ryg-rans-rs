//! # Container reader — parse and validate RYGRANS containers

use crate::container::block::{Block, BlockHeaderInfo};
use crate::container::footer::FileFooter;
use crate::container::header::FileHeader;
use crate::container::{BLOCK_HEADER_SIZE, FOOTER_SIZE, HEADER_SIZE};
use crate::error::{AppError, FormatError};
use crate::limits::Limits;
use sha2::{Digest, Sha256};
use std::io::Read;

/// Container reader that validates every field, bound, and hash.
pub struct ContainerReader<R: Read> {
    reader: R,
    limits: Limits,
    header: Option<FileHeader>,
    bytes_read: u64,
    blocks_seen: u64,
    total_uncompressed: u64,
    total_payload: u64,
    hasher: Sha256,
    decoded_hasher: Sha256,
    /// Final container hash, captured at `read_footer` time.
    container_hash_final: Option<[u8; 32]>,
    /// Final decoded-stream hash, captured at `read_footer` time.
    decoded_stream_hash_final: Option<[u8; 32]>,
    /// The 4-byte END1 tag consumed by `read_block`; `read_footer` prepends
    /// it to the remaining 100 footer bytes.
    pending_footer_tag: Option<[u8; 4]>,
}

impl<R: Read> ContainerReader<R> {
    /// Create a new container reader.
    pub fn new(reader: R, limits: Limits) -> Self {
        Self {
            reader,
            limits,
            header: None,
            bytes_read: 0,
            blocks_seen: 0,
            total_uncompressed: 0,
            total_payload: 0,
            hasher: Sha256::new(),
            decoded_hasher: Sha256::new(),
            container_hash_final: None,
            decoded_stream_hash_final: None,
            pending_footer_tag: None,
        }
    }

    /// Read and validate the file header.
    pub fn read_header(&mut self) -> Result<FileHeader, AppError> {
        let mut header_buf = [0u8; HEADER_SIZE];
        self.read_exact(&mut header_buf)?;
        let header = FileHeader::from_bytes(&header_buf)?;

        // Check declared block size against limits
        self.limits.check_block_size(header.declared_block_size)?;

        // Feed header to container hasher
        self.hasher.update(&header_buf);

        self.header = Some(header.clone());
        Ok(header)
    }

    /// Read and process the next block.  Returns `None` when no more blocks
    /// (footer will be read separately).
    ///
    /// The returned tuple is the parsed `Block` plus the decoded bytes, so
    /// callers never need to decode twice (decode/inspect/verify/compare all
    /// consume the same codec truth through `decode_fn`).
    pub fn read_block<F>(&mut self, mut decode_fn: F) -> Result<Option<(Block, Vec<u8>)>, AppError>
    where
        F: FnMut(&BlockHeaderInfo, &[u8], &[u8]) -> Result<Vec<u8>, AppError>,
    {
        // Read the 4-byte tag.  A clean EOF here means the container is
        // missing its footer (truncated).  The tag is read explicitly so it
        // is consumed exactly once; the block header that follows is the
        // full 104-byte record (which starts with the same tag bytes).
        let mut tag = [0u8; 4];
        match self.reader.read_exact(&mut tag) {
            Ok(()) => {}
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Err(AppError::Format(FormatError {
                    detail: "truncated container: missing footer".into(),
                    block_index: Some(self.blocks_seen),
                    offset: Some(self.bytes_read),
                }));
            }
            Err(e) => {
                return Err(AppError::Io(crate::error::IoError {
                    path: None,
                    detail: format!("read tag: {}", e),
                }));
            }
        }
        self.bytes_read += 4;

        if &tag == b"END1" {
            self.pending_footer_tag = Some(tag);
            return Ok(None); // footer follows
        }
        if &tag != b"BLK1" {
            return Err(AppError::Format(FormatError {
                detail: format!("expected block or footer, got {:02x?}", tag),
                block_index: Some(self.blocks_seen),
                offset: Some(self.bytes_read - 4),
            }));
        }

        // Read the remaining 100 bytes of the 104-byte block header.
        let mut header_buf = [0u8; BLOCK_HEADER_SIZE];
        header_buf[0..4].copy_from_slice(&tag);
        self.read_exact(&mut header_buf[4..])?;
        let (info, _) = Block::parse_header(&header_buf, self.blocks_seen)?;

        // Check limits
        self.limits.check_block_size(info.uncompressed_length)?;
        self.limits.check_payload_size(info.payload_length)?;
        self.limits.check_model_size(info.model_length)?;
        self.limits.check_block_count(self.blocks_seen + 1)?;
        self.limits
            .check_output_total(self.total_uncompressed, info.uncompressed_length as u64)?;

        // Feed header to container hasher
        self.hasher.update(&header_buf);

        // Read model data
        let model_data = if info.model_length > 0 {
            let mut buf = vec![0u8; info.model_length as usize];
            self.read_exact(&mut buf)?;
            self.hasher.update(&buf);
            buf
        } else {
            Vec::new()
        };

        // Read payload
        let mut payload = vec![0u8; info.payload_length as usize];
        self.read_exact(&mut payload)?;
        self.hasher.update(&payload);

        // Verify payload SHA-256
        let mut pay_hasher = Sha256::new();
        pay_hasher.update(&payload);
        let pay_hash: [u8; 32] = pay_hasher.finalize().into();
        if pay_hash != info.payload_sha256 {
            return Err(AppError::Integrity(crate::error::IntegrityError {
                detail: format!("payload hash mismatch at block {}", self.blocks_seen),
                block_index: Some(self.blocks_seen),
            }));
        }

        // Decode
        let decoded = decode_fn(&info, &model_data, &payload)?;

        // Verify decoded SHA-256.  Strict integrity: a block passes only
        // when the stored decoded hash is non-zero and matches.  A zero
        // stored hash therefore fails here (computed != zero), which is the
        // CLI-level enforcement of the strict integrity policy.
        if info.block_kind != crate::container::BLOCK_KIND_RAW {
            let mut dec_hasher = Sha256::new();
            dec_hasher.update(&decoded);
            let dec_hash: [u8; 32] = dec_hasher.finalize().into();
            if dec_hash != info.decoded_sha256 {
                return Err(AppError::Integrity(crate::error::IntegrityError {
                    detail: format!("decoded hash mismatch at block {}", self.blocks_seen),
                    block_index: Some(self.blocks_seen),
                }));
            }
        }

        // Accumulate
        self.blocks_seen += 1;
        self.total_uncompressed = self
            .total_uncompressed
            .checked_add(info.uncompressed_length as u64)
            .ok_or_else(|| {
                AppError::Format(FormatError {
                    detail: "total uncompressed overflow".into(),
                    block_index: Some(self.blocks_seen - 1),
                    offset: None,
                })
            })?;
        self.total_payload = self
            .total_payload
            .checked_add(info.payload_length as u64)
            .ok_or_else(|| {
                AppError::Format(FormatError {
                    detail: "total payload overflow".into(),
                    block_index: Some(self.blocks_seen - 1),
                    offset: None,
                })
            })?;

        self.decoded_hasher.update(&decoded);

        Ok(Some((
            Block {
                block_index: info.block_index,
                block_kind: info.block_kind,
                codec_id: info.codec_id,
                scale_bits: info.scale_bits,
                state_count: info.state_count,
                uncompressed_length: info.uncompressed_length,
                payload,
                model_data,
                payload_sha256: info.payload_sha256,
                decoded_sha256: info.decoded_sha256,
            },
            decoded,
        )))
    }

    /// Read and verify the footer.
    pub fn read_footer(&mut self) -> Result<FileFooter, AppError> {
        let mut footer_buf = [0u8; FOOTER_SIZE];
        // The END1 tag was already consumed by `read_block`; prepend it to
        // the remaining 100 bytes instead of re-reading 104.
        if let Some(tag) = self.pending_footer_tag.take() {
            footer_buf[0..4].copy_from_slice(&tag);
            self.read_exact(&mut footer_buf[4..])?;
        } else {
            self.read_exact(&mut footer_buf)?;
        }
        let footer = FileFooter::from_bytes(&footer_buf)?;

        // Verify footer totals
        footer.verify_totals(
            self.blocks_seen,
            self.total_uncompressed,
            self.total_payload,
        )?;

        // Verify container hash
        let hasher_clone = std::mem::replace(&mut self.hasher, Sha256::new());
        let container_hash: [u8; 32] = hasher_clone.finalize().into();
        self.container_hash_final = Some(container_hash);
        if container_hash != footer.container_sha256 {
            return Err(AppError::Integrity(crate::error::IntegrityError {
                detail: "container hash mismatch".into(),
                block_index: None,
            }));
        }

        // Verify decoded stream hash
        let dec_hasher_clone = std::mem::replace(&mut self.decoded_hasher, Sha256::new());
        let decoded_hash: [u8; 32] = dec_hasher_clone.finalize().into();
        self.decoded_stream_hash_final = Some(decoded_hash);
        if decoded_hash != footer.decoded_stream_sha256 {
            return Err(AppError::Integrity(crate::error::IntegrityError {
                detail: "decoded stream hash mismatch".into(),
                block_index: None,
            }));
        }

        Ok(footer)
    }

    /// Check that there is no trailing data after the footer.
    pub fn check_trailing_data(&mut self) -> Result<(), AppError> {
        let mut buf = [0u8; 1];
        match self.reader.read_exact(&mut buf) {
            Ok(()) => Err(AppError::Format(FormatError {
                detail: "trailing data after footer".into(),
                block_index: Some(self.blocks_seen),
                offset: Some(self.bytes_read),
            })),
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(()),
            Err(e) => Err(AppError::Io(crate::error::IoError {
                path: None,
                detail: e.to_string(),
            })),
        }
    }

    /// Blocks seen so far (valid after `read_footer`).
    pub fn block_count_seen(&self) -> u64 {
        self.blocks_seen
    }

    /// Total uncompressed bytes seen (valid after `read_footer`).
    pub fn total_uncompressed_seen(&self) -> u64 {
        self.total_uncompressed
    }

    /// Total payload bytes seen (valid after `read_footer`).
    pub fn total_payload_seen(&self) -> u64 {
        self.total_payload
    }

    /// Final verified decoded-stream hash (valid after `read_footer`).
    pub fn decoded_stream_hash(&self) -> [u8; 32] {
        self.decoded_stream_hash_final.unwrap_or([0u8; 32])
    }

    // ---- Private helpers ----

    fn read_exact(&mut self, buf: &mut [u8]) -> Result<(), AppError> {
        self.reader.read_exact(buf).map_err(|e| {
            AppError::Format(FormatError {
                detail: format!("read error: {}", e),
                block_index: Some(self.blocks_seen),
                offset: Some(self.bytes_read),
            })
        })?;
        self.bytes_read += buf.len() as u64;
        Ok(())
    }
}
