//! # Resource limits for the ryg-rans CLI
//!
//! Central `Limits` type that controls all resource bounds.
//! Every command must respect these limits.  No command may bypass limits
//! because it uses stdin rather than a file.
//!
//! Limits are enforced during reading, not after.  All accumulation uses
//! checked arithmetic.

use crate::error::{AppError, ResourceLimitError};
use std::time::Duration;

/// Default maximum input size (16 GiB).
pub const DEFAULT_MAX_INPUT: u64 = 16 * 1024 * 1024 * 1024;

/// Default maximum output size (16 GiB).
pub const DEFAULT_MAX_OUTPUT: u64 = 16 * 1024 * 1024 * 1024;

/// Default maximum block size (1 MiB).
pub const DEFAULT_BLOCK_SIZE: u32 = 1024 * 1024;

/// Hard maximum block size (64 MiB).
pub const HARD_MAX_BLOCK_SIZE: u32 = 64 * 1024 * 1024;

/// Default maximum payload per block (same as block size).
pub const DEFAULT_MAX_PAYLOAD: u32 = 1024 * 1024;

/// Default maximum model size (2 KB — 256 symbols × 5 bytes + 2).
pub const DEFAULT_MAX_MODEL: u32 = 2048;

/// Default maximum blocks (1 million).
pub const DEFAULT_MAX_BLOCKS: u64 = 1_000_000;

/// Default maximum trace symbols.
pub const DEFAULT_MAX_TRACE_SYMBOLS: u64 = 256;

/// Default maximum oracle output bytes (64 MB).
pub const DEFAULT_MAX_ORACLE_OUTPUT: u64 = 64 * 1024 * 1024;

/// Default oracle timeout.
pub const DEFAULT_ORACLE_TIMEOUT: Duration = Duration::from_secs(60);

/// Central resource limits.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Maximum input bytes to read.
    pub max_input_bytes: u64,
    /// Maximum output bytes to produce.
    pub max_output_bytes: u64,
    /// Maximum bytes per uncompressed block.
    pub max_block_bytes: u32,
    /// Maximum bytes per compressed payload.
    pub max_payload_bytes: u32,
    /// Maximum bytes per model encoding.
    pub max_model_bytes: u32,
    /// Maximum number of blocks.
    pub max_blocks: u64,
    /// Maximum trace symbols to emit.
    pub max_trace_symbols: u64,
    /// Maximum bytes to capture from oracle output.
    pub max_oracle_output_bytes: u64,
    /// Maximum wall-clock time for oracle execution.
    pub oracle_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_input_bytes: DEFAULT_MAX_INPUT,
            max_output_bytes: DEFAULT_MAX_OUTPUT,
            max_block_bytes: DEFAULT_BLOCK_SIZE,
            max_payload_bytes: DEFAULT_MAX_PAYLOAD,
            max_model_bytes: DEFAULT_MAX_MODEL,
            max_blocks: DEFAULT_MAX_BLOCKS,
            max_trace_symbols: DEFAULT_MAX_TRACE_SYMBOLS,
            max_oracle_output_bytes: DEFAULT_MAX_ORACLE_OUTPUT,
            oracle_timeout: DEFAULT_ORACLE_TIMEOUT,
        }
    }
}

impl Limits {
    /// Check that a block's declared uncompressed length is within limits.
    pub fn check_block_size(&self, size: u32) -> Result<(), AppError> {
        if size > self.max_block_bytes {
            return Err(AppError::ResourceLimit(ResourceLimitError {
                detail: "block size exceeds limit".into(),
                limit: self.max_block_bytes as u64,
                requested: size as u64,
            }));
        }
        Ok(())
    }

    /// Check that a block's declared payload length is within limits.
    pub fn check_payload_size(&self, size: u32) -> Result<(), AppError> {
        if size > self.max_payload_bytes {
            return Err(AppError::ResourceLimit(ResourceLimitError {
                detail: "payload size exceeds limit".into(),
                limit: self.max_payload_bytes as u64,
                requested: size as u64,
            }));
        }
        Ok(())
    }

    /// Check that a model length is within limits.
    pub fn check_model_size(&self, size: u32) -> Result<(), AppError> {
        if size > self.max_model_bytes {
            return Err(AppError::ResourceLimit(ResourceLimitError {
                detail: "model size exceeds limit".into(),
                limit: self.max_model_bytes as u64,
                requested: size as u64,
            }));
        }
        Ok(())
    }

    /// Check that the block count does not exceed the limit.
    pub fn check_block_count(&self, count: u64) -> Result<(), AppError> {
        if count > self.max_blocks {
            return Err(AppError::ResourceLimit(ResourceLimitError {
                detail: "block count exceeds limit".into(),
                limit: self.max_blocks,
                requested: count,
            }));
        }
        Ok(())
    }

    /// Check that adding `additional` to `current` output does not exceed the limit.
    /// Uses checked arithmetic.
    pub fn check_output_total(&self, current: u64, additional: u64) -> Result<u64, AppError> {
        let new_total = current.checked_add(additional).ok_or_else(|| {
            AppError::ResourceLimit(ResourceLimitError {
                detail: "output total overflow".into(),
                limit: self.max_output_bytes,
                requested: current.saturating_add(additional),
            })
        })?;
        if new_total > self.max_output_bytes {
            return Err(AppError::ResourceLimit(ResourceLimitError {
                detail: "output total exceeds limit".into(),
                limit: self.max_output_bytes,
                requested: new_total,
            }));
        }
        Ok(new_total)
    }

    /// Parse a human-readable size string (e.g., "1MiB", "64KiB", "1024").
    /// Supports: KiB, MiB, GiB (1024-based).  No suffix = bytes.
    pub fn parse_size(s: &str) -> Result<u64, String> {
        let s = s.trim();
        if s.is_empty() {
            return Err("empty size string".into());
        }

        // Find the numeric prefix and optional suffix
        let num_end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        let num_part = &s[..num_end];
        let suffix = s[num_end..].trim();

        let value: u64 = num_part
            .parse()
            .map_err(|_| format!("invalid number: '{}'", num_part))?;

        match suffix.to_lowercase().as_str() {
            "" | "b" => Ok(value),
            "kib" | "ki" => value.checked_mul(1024).ok_or("overflow".into()),
            "mib" | "mi" => value.checked_mul(1024 * 1024).ok_or("overflow".into()),
            "gib" | "gi" => value
                .checked_mul(1024 * 1024 * 1024)
                .ok_or("overflow".into()),
            _ => Err(format!("unknown size suffix: '{}'", suffix)),
        }
    }
}
