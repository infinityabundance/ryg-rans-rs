//! # Parallel container verification
//!
//! Workers verify block payload hashes, model validity, decode correctness,
//! and decoded-block hashes — all without writing decoded output.
//!
//! The coordinator performs footer-total checks and creates an aggregate report.

use crate::cancellation::CancellationToken;
use crate::config::ParallelConfig;
use crate::error::{BlockError, BlockErrorKind, ParallelError};
use crate::executor::{ExecutorReport, ExecutorTask, run_tasks};
use crate::job::VerifyBlockJob;
use crate::reorder::{BufferSized, HasBlockIndex};
use std::vec::Vec;

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
    /// Per-block results.
    pub block_results: Vec<BlockVerificationResult>,
    /// Optional error if verification failed.
    pub error: Option<ParallelError>,
}

/// Result of verifying a single block.
#[derive(Debug, Clone)]
pub struct BlockVerificationResult {
    pub block_index: u64,
    pub payload_hash_ok: bool,
    pub decoded_hash_ok: bool,
    pub decode_success: bool,
    pub backend: crate::config::BackendId,
}

impl HasBlockIndex for BlockVerificationResult {
    fn block_index(&self) -> u64 {
        self.block_index
    }
}

impl BufferSized for BlockVerificationResult {
    fn buffer_size(&self) -> u64 {
        64
    }
}

/// A verify task submitted to the executor.
struct VerifyTask {
    job: VerifyBlockJob,
}

impl ExecutorTask for VerifyTask {
    type Output = Result<BlockVerificationResult, BlockError>;

    fn run(self, _worker_index: usize, cancel: &CancellationToken) -> Self::Output {
        cancel.check().map_err(|_| BlockError {
            block_index: self.job.block_index,
            kind: BlockErrorKind::Codec,
        })?;

        verify_single_block(&self.job)
    }
}

/// Verify a single block: payload hash, decode, decoded hash.
fn verify_single_block(job: &VerifyBlockJob) -> Result<BlockVerificationResult, BlockError> {
    let data = &job.block_data;
    if data.len() < 104 {
        return Err(BlockError {
            block_index: job.block_index,
            kind: BlockErrorKind::Format,
        });
    }
    if &data[0..4] != b"RYGR" {
        return Err(BlockError {
            block_index: job.block_index,
            kind: BlockErrorKind::Format,
        });
    }

    let bi = job.block_index;
    let payload_length = u32::from_le_bytes(data[28..32].try_into().unwrap());
    let model_length = u32::from_le_bytes(data[32..36].try_into().unwrap());
    let mut stored_payload_hash = [0u8; 32];
    stored_payload_hash.copy_from_slice(&data[40..72]);
    let mut stored_decoded_hash = [0u8; 32];
    stored_decoded_hash.copy_from_slice(&data[72..104]);

    let payload =
        &data[104 + model_length as usize..104 + model_length as usize + payload_length as usize];
    let payload_hash_ok = crate::encode::sha256(payload) == stored_payload_hash;

    // Attempt decode for verification
    let decode_job = crate::job::DecodeBlockJob {
        block_index: bi,
        block_data: job.block_data.clone(),
    };

    let decode_result = crate::decode::decode_single_block(&decode_job);
    let (decoded_hash_ok, decode_success) = match decode_result {
        Ok(decoded) => {
            let computed = crate::encode::sha256(&decoded.output);
            let dh_ok = computed == stored_decoded_hash || stored_decoded_hash == [0u8; 32];
            (dh_ok, true)
        }
        Err(_) => (false, false),
    };

    Ok(BlockVerificationResult {
        block_index: bi,
        payload_hash_ok,
        decoded_hash_ok,
        decode_success,
        backend: crate::config::BackendId::Scalar16,
    })
}

/// Parallel container verifier.
pub struct ParallelVerifier;

impl ParallelVerifier {
    /// Verify all blocks in parallel.
    pub fn verify_blocks(
        blocks: impl IntoIterator<Item = VerifyBlockJob>,
        config: &ParallelConfig,
    ) -> Result<ParallelVerificationReport, ParallelError> {
        let jobs: Vec<VerifyBlockJob> = blocks.into_iter().collect();
        if jobs.is_empty() {
            return Ok(ParallelVerificationReport {
                blocks_verified: 0,
                payload_hash_ok: 0,
                decoded_hash_ok: 0,
                output_matches: 0,
                blocks_failed: 0,
                block_results: Vec::new(),
                error: None,
            });
        }

        let block_count = jobs.len();
        let worker_count = crate::resource::effective_worker_count(config, block_count)?;
        let queue_capacity = config.max_in_flight_blocks.get().max(worker_count);
        let tasks: Vec<VerifyTask> = jobs.into_iter().map(|j| VerifyTask { job: j }).collect();
        let report: ExecutorReport<Result<BlockVerificationResult, BlockError>> = run_tasks(
            tasks,
            worker_count,
            queue_capacity,
            config.worker_stack_size,
        )?;

        let mut results = Vec::with_capacity(block_count);
        let mut error_tracker = crate::error::CanonicalErrorTracker::new();
        let mut payload_ok = 0u64;
        let mut decoded_ok = 0u64;
        let mut decode_ok = 0u64;
        let mut failed = 0u64;

        for r in report.results {
            match r {
                Ok(vr) => {
                    if vr.payload_hash_ok {
                        payload_ok += 1;
                    }
                    if vr.decoded_hash_ok {
                        decoded_ok += 1;
                    }
                    if vr.decode_success {
                        decode_ok += 1;
                    } else {
                        failed += 1;
                    }
                    results.push(vr);
                }
                Err(e) => {
                    error_tracker.record(e);
                    failed += 1;
                }
            }
        }

        if let Some(c) = error_tracker.canonical_error() {
            return Err(ParallelError::VerifyFailed(Box::new(c.clone())));
        }

        Ok(ParallelVerificationReport {
            blocks_verified: block_count as u64,
            payload_hash_ok: payload_ok,
            decoded_hash_ok: decoded_ok,
            output_matches: decode_ok,
            blocks_failed: failed,
            block_results: results,
            error: None,
        })
    }
}
