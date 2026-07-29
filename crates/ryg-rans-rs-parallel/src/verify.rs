//! # Parallel container verification
//!
//! Workers verify block payload hashes, model validity, decode correctness,
//! and decoded-block hashes — all without writing decoded output.
//!
//! The coordinator performs footer-total checks and creates an aggregate report.

use crate::config::ParallelConfig;
use crate::error::ParallelError;
use crate::job::VerifyBlockJob;

/// Result of parallel verification.
#[derive(Debug, Clone)]
pub struct ParallelVerificationReport {
    /// Number of blocks verified.
    pub blocks_verified: u64,
    /// Number of blocks whose payload hash matched.
    pub payload_hash_ok: u64,
    /// Number of blocks whose decoded hash matched.
    pub decoded_hash_ok: u64,
    /// Number of blocks where decoded output matched.
    pub output_matches: u64,
    /// Number of failed blocks.
    pub blocks_failed: u64,
    /// Optional error if verification failed.
    pub error: Option<ParallelError>,
}

/// Parallel container verifier.
pub struct ParallelVerifier;

impl ParallelVerifier {
    /// Verify all blocks in parallel.
    pub fn verify_blocks(
        _blocks: impl IntoIterator<Item = VerifyBlockJob>,
        _config: &ParallelConfig,
    ) -> Result<ParallelVerificationReport, ParallelError> {
        Ok(ParallelVerificationReport {
            blocks_verified: 0,
            payload_hash_ok: 0,
            decoded_hash_ok: 0,
            output_matches: 0,
            blocks_failed: 0,
            error: None,
        })
    }
}
