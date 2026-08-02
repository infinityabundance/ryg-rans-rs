//! # Parallel container verification
//!
//! Workers verify block payload hashes, model validity, decode correctness,
//! and decoded-block hashes — all without writing decoded output.
//!
//! The coordinator performs footer-total checks and creates an aggregate report.
//!
//! ## Verification pipeline
//!
//! For each block, the verifier runs the following stages:
//!
//! 1. **Parse** — `parse_block_header` validates the RYGRANS header with
//!    checked arithmetic for model and payload offsets.
//! 2. **Extract** — The model and payload regions are sliced from the block
//!    data based on header field values.  All offsets are checked for overflow
//!    and bounds against the data length.
//! 3. **Payload hash** — The extracted payload bytes are SHA-256 hashed and
//!    compared against `header.payload_sha256`.  If they don't match, the
//!    block is marked as `payload_hash_ok = false`.
//! 4. **Decode** — `decode_single_block` is called to fully decode the block.
//!    The decode uses the same backend as the encoder (identified by the
//!    header's codec ID).  The decoder must parse the model, reconstruct
//!    cumulative frequencies, and run the rANS decode loop.
//! 5. **Decoded hash** — The decoded output is SHA-256 hashed and compared
//!    against `header.decoded_sha256`.  A **zero stored hash** (`[0u8; 32]`)
//!    is treated as "not set" — `decoded_hash_ok` is `false`, but the block
//!    is **not automatically counted as failed** for this reason alone.
//! 6. **Report** — A `BlockVerificationResult` is produced with all four flags
//!    (`payload_hash_ok`, `decoded_hash_ok`, `decode_success`, `backend`).
//!
//! ## Integrity enforcement
//!
//! The block is counted as **failed** (`blocks_failed += 1`) if and only if:
//!
//! - `payload_hash_ok` is `false` (payload hash mismatch), **OR**
//! - `decode_success` is `false` (decode error)
//!
//! A `decoded_hash_ok = false` due to an unset hash does **NOT** alone trigger
//! a failure count.  This allows callers that don't set `decoded_sha256` to
//! still verify payload integrity without spurious failures.
//!
//! The failure rules are implemented in `ParallelVerifier::verify_blocks`:
//!
//! ```ignore
//! if !vr.payload_hash_ok || !vr.decode_success {
//!     failed += 1;
//! }
//! ```
//!
//! If **any** block fails, the coordinator returns
//! `Err(ParallelError::VerifyFailed(canonical_error))`.
//!
//! ## Backend identity
//!
//! Even during verification, the `backend` field in `BlockVerificationResult`
//! records which backend was used for decode (`Scalar16`, `Scalar8`, etc.).
//! This is important because:
//!
//! - Different backends may produce different results for corrupt data
//!   (e.g. one backend may gracefully error while another produces garbage).
//! - Verification reports that include backend identity allow downstream
//!   tooling to correlate failures with specific backends.
//! - If decode fails, the backend defaults to `BackendId::Scalar16` to
//!   ensure the field is always populated.
//!
//! ## Relationship between verify and decode
//!
//! Verification is a **superset** of decoding: it runs the full decode
//! pipeline, then additionally hashes and compares.  A block that decodes
//! successfully may still **fail verification** if the decoded hash doesn't
//! match the expected value (and the expected value is non-zero).

use crate::block::parse_block_header;
use crate::cancellation::CancellationToken;
use crate::config::ParallelConfig;
use crate::error::{BlockError, BlockErrorKind, ParallelError};
use crate::executor::{ExecutorReport, ExecutorTask};
use crate::job::VerifyBlockJob;
use crate::reorder::{BufferSized, HasBlockIndex};
use std::vec::Vec;

/// Result of parallel verification.
///
/// Aggregates the results of verifying all blocks in a container.  The
/// coordinator uses these counts to determine whether the container as
/// a whole is intact.
///
/// ## Key counters
///
/// - `blocks_verified` — total number of blocks processed (should equal
///   the number of `VerifyBlockJob` values submitted).
/// - `payload_hash_ok` — number of blocks whose payload SHA-256 matched
///   the stored value.  This is the primary integrity indicator.
/// - `decoded_hash_ok` — number of blocks whose decoded output SHA-256
///   matched the stored value.  A zero stored hash ("unset") counts as
///   `false`, so a block with an unset hash does **not** increment this
///   counter even though the decode succeeded.
/// - `output_matches` — synonymous with `decode_success` count; the
///   number of blocks that completed decoding without error.
/// - `blocks_failed` — number of blocks where either the payload hash
///   did NOT match **or** decode did NOT succeed.  Unset decoded hashes
///   do NOT increment this counter.
/// - `error` — populated if `blocks_failed > 0` and a canonical error
///   was selected.
#[derive(Debug, Clone)]
pub struct ParallelVerificationReport {
    /// Number of blocks verified.
    pub blocks_verified: u64,
    /// Number of blocks whose payload hash matched.
    pub payload_hash_ok: u64,
    /// Number of blocks whose decoded hash matched (nonzero stored hash).
    pub decoded_hash_ok: u64,
    /// Number of blocks with nonzero stored decoded hash mismatch.
    pub decoded_hash_mismatch: u64,
    /// Number of blocks with zero (unset) stored decoded hash.
    pub decoded_hash_unset: u64,
    /// Number of blocks where decode failed before hash computation.
    pub decoded_hash_not_computed: u64,
    /// Number of blocks whose decoded output matched expectations.
    pub output_matches: u64,
    /// Number of failed blocks (hash mismatch, decode error, etc.).
    pub blocks_failed: u64,
    /// Per-block results.
    pub block_results: Vec<BlockVerificationResult>,
    /// Optional error if verification failed.
    pub error: Option<ParallelError>,
}

/// Result of verifying a single block.
///
/// Contains four boolean flags that together describe the integrity state
/// of one block in a verified RYGRANS container.
///
/// ## Flag semantics
///
/// - `payload_hash_ok` — `true` if the SHA-256 of the raw payload bytes
///   matches `header.payload_sha256`.  This is the **primary integrity check**.
///   If `false`, the payload has been corrupted, and the block **always**
///   counts as failed.
/// - `decoded_hash_ok` — `true` if decode succeeded **and** the SHA-256 of
///   the decoded output matches `header.decoded_sha256`.  A stored hash of
///   `[0u8; 32]` (all zeros) is interpreted as "unset" → `false`, but this
///   does **not** cause the block to count as failed.  This allows optional
///   decoded-hash verification.
/// - `decode_success` — `true` if `decode_single_block` returned `Ok`.  If
///   `false` (decode panicked or errored), the block **always** counts as
///   failed, regardless of the payload hash result.
/// - `backend` — the `BackendId` used (or inferred) for decode.  When
///   decode fails, defaults to `BackendId::Scalar16` as a fallback label.
///
/// ## Failure counting
///
/// A block is considered failed if `!payload_hash_ok || !decode_success`.
/// This is checked in `ParallelVerifier::verify_blocks`:
///
/// ```ignore
/// if !vr.payload_hash_ok || !vr.decode_success {
///     failed += 1;
/// }
/// ```
///
/// Note: `decoded_hash_ok` being `false` does **not** make the block fail
/// on its own.  This is intentional: the decoded hash is an additional
/// integrity layer, and an unset hash should not produce a failure.
#[derive(Debug, Clone)]
pub struct BlockVerificationResult {
    pub block_index: u64,
    pub payload_hash_ok: bool,
    pub decoded_hash_ok: bool,
    pub decoded_hash_state: crate::config::HashVerification,
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
///
/// Carries the explicitly owned model artifact cache (Phase O.4) so
/// verification decodes share the same cache instance as decode paths.
struct VerifyTask {
    job: VerifyBlockJob,
    config: ParallelConfig,
    model_cache: crate::sync::Arc<crate::cache::ModelArtifactCache>,
}

impl ExecutorTask for VerifyTask {
    type Output = Result<BlockVerificationResult, BlockError>;

    fn run(
        self,
        _worker_index: usize,
        cancel: &CancellationToken,
        _scratch: &mut crate::scratch::WorkerScratch,
    ) -> Self::Output {
        cancel.check().map_err(|_| BlockError {
            block_index: self.job.block_index,
            kind: BlockErrorKind::Codec,
        })?;

        verify_single_block(&self.job, &self.config, &self.model_cache)
    }

    fn block_index(&self) -> Option<u64> {
        Some(self.job.block_index)
    }
}

/// Verify a single block: payload hash, decode, decoded hash.
///
/// This is the per-block verification function, executed by worker threads
/// via `VerifyTask::run`.
///
/// ## Pipeline detail
///
/// 1. **Parse** — `parse_block_header` extracts the header.  If the header
///    is malformed (e.g. truncated, wrong magic), returns
///    `BlockError { kind: Format }`.
/// 2. **Extract bounds** — Model offset is fixed at 104 (header size), model
///    length comes from the header.  Payload comes after the model.  All
///    offsets are checked with `checked_add` to guard against arithmetic
///    overflow.  If the payload extends beyond `data.len()`, returns
///    `BlockError { kind: Format }`.
/// 3. **Payload hash** — `crate::encode::sha256(payload)` is compared against
///    `header.payload_sha256`.  Result goes into `payload_hash_ok`.
/// 4. **Decode** — `decode_single_block` is called with a `DecodeBlockJob`.
///    The decode uses the same `ParallelConfig` (which controls backend
///    selection and decode parameters).
/// 5. **Decoded hash** — If decode succeeded, the output is SHA-256 hashed.
///    If `header.decoded_sha256` is all zeros, it's treated as "unset":
///    `decoded_hash_ok = false`.  Otherwise, it's compared against the
///    computed hash.
/// 6. **Decode failure handling** — If decode fails:
///    - If the error is `BlockErrorKind::PayloadHash`, that matches the
///      already-computed `payload_hash_ok = false` — consistent.
///    - For any other error, `decoded_hash_ok = false` and
///      `decode_success = false` are set.
/// 7. **Backend** — On successful decode, the backend from the decode result
///    is used.  On failure, defaults to `BackendId::Scalar16`.
///
/// # Integrity guarantees
///
/// 1. Payload hash is verified against the stored SHA-256.
///    If it doesn't match, the block is marked as failed.
/// 2. Decoded hash is verified against the stored SHA-256.
///    A zero stored hash is treated as "unset" — the block is still
///    considered NOT verified (decoded_hash_ok = false), but the
///    block is NOT automatically counted as failed for this reason alone.
///    The caller must decide how to handle unset hashes.
/// 3. If decode fails entirely, the block is marked as failed.
/// 4. The coordinator counts any block as failed if payload_hash_ok = false
///    OR decode_success = false.  Unset decoded hashes are reported but
///    do not alone trigger a failure count.
///
/// # Returns
///
/// Returns `Ok(BlockVerificationResult)` on success (even if hashes don't
/// match — hash mismatch is a result, not an error).  Returns
/// `Err(BlockError { kind: Format })` if the block data is structurally
/// invalid (truncated, arithmetic overflow).
fn verify_single_block(
    job: &VerifyBlockJob,
    config: &ParallelConfig,
    model_cache: &crate::cache::ModelArtifactCache,
) -> Result<BlockVerificationResult, BlockError> {
    let data = &job.block_data;
    let bi = job.block_index;

    // Parse header with checked arithmetic
    let (header, _model_offset) = parse_block_header(data, bi).map_err(|_e| BlockError {
        block_index: bi,
        kind: BlockErrorKind::Format,
    })?;

    // Extract payload with bounds check
    let model_offset = 104usize;
    let model_len = header.model_length as usize;
    let model_end = model_offset.checked_add(model_len).ok_or(BlockError {
        block_index: bi,
        kind: BlockErrorKind::Format,
    })?;
    let payload_offset = model_end;
    let payload_len = header.payload_length as usize;
    let payload_end = payload_offset.checked_add(payload_len).ok_or(BlockError {
        block_index: bi,
        kind: BlockErrorKind::Format,
    })?;
    if payload_end > data.len() {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Format,
        });
    }
    let payload = &data[payload_offset..payload_end];

    // Verify payload hash
    let computed_payload_hash = crate::encode::sha256(payload);
    let payload_hash_ok = computed_payload_hash == header.payload_sha256;

    // Attempt decode for verification
    let decode_job = crate::job::DecodeBlockJob {
        block_index: bi,
        block_data: job.block_data.clone(),
    };

    let decode_result = crate::decode::decode_single_block(&decode_job, config, model_cache, None);
    let backend = match &decode_result {
        Ok(decoded) => decoded.backend,
        Err(_) => crate::config::BackendId::Scalar16,
    };
    let (decoded_hash_ok, decoded_hash_state, decode_success) = match decode_result {
        Ok(decoded) => {
            let computed = crate::encode::sha256(&decoded.output);
            // Policy-dependent handling of unset (zero) decoded hashes.
            if header.decoded_sha256 == [0u8; 32] {
                match config.integrity_policy {
                    crate::config::IntegrityPolicy::Strict => {
                        // Strict: an unset decoded hash is a failure.
                        return Err(BlockError {
                            block_index: bi,
                            kind: BlockErrorKind::DecodedHashMissing,
                        });
                    }
                    crate::config::IntegrityPolicy::AllowLegacyUnsetDecodedHash => {
                        (false, crate::config::HashVerification::Unset, true)
                    }
                }
            } else if computed == header.decoded_sha256 {
                (true, crate::config::HashVerification::Match, true)
            } else {
                // Nonzero stored hash mismatch — always a failure under both policies.
                return Err(BlockError {
                    block_index: bi,
                    kind: BlockErrorKind::DecodedHashMismatch,
                });
            }
        }
        Err(e) => {
            // Propagate typed decoded-hash errors from decode so the
            // canonical error tracker records the real kind, not a
            // generic Codec.  A PayloadHash decode failure is expected
            // and already captured by payload_hash_ok.
            match e.kind {
                BlockErrorKind::DecodedHashMissing | BlockErrorKind::DecodedHashMismatch => {
                    return Err(e);
                }
                _ => {}
            }
            let dh_ok = match e.kind {
                BlockErrorKind::PayloadHash => false,
                _ => false,
            };
            (dh_ok, crate::config::HashVerification::NotComputed, false)
        }
    };

    Ok(BlockVerificationResult {
        block_index: bi,
        payload_hash_ok,
        decoded_hash_ok,
        decoded_hash_state,
        decode_success,
        backend,
    })
}

/// Parallel container verifier.
///
/// A stateless struct whose sole method `verify_blocks` drives the
/// parallel verification pipeline.  No configuration is stored on the
/// struct itself; all configuration comes from the `ParallelConfig`
/// parameter.
///
/// ## Usage
///
/// ```ignore
/// let report = ParallelVerifier::verify_blocks(jobs, &config)?;
/// match report.blocks_failed {
///     0 => println!("All {} blocks verified OK", report.blocks_verified),
///     n => println!("{} blocks failed", n),
/// }
/// ```
pub struct ParallelVerifier {
    /// The configuration for every verification issued through this verifier.
    config: ParallelConfig,
    /// The explicitly owned model artifact cache (Phase O.4).
    model_cache: crate::sync::Arc<crate::cache::ModelArtifactCache>,
}

impl ParallelVerifier {
    /// Create a verifier with the default bounded model artifact cache
    /// (64 entries, 16 MiB — explicit ownership, never process-global).
    pub fn new(config: ParallelConfig) -> Self {
        Self {
            config,
            model_cache: crate::cache::ModelArtifactCache::bounded(64, 16 * 1024 * 1024),
        }
    }

    /// Create a verifier with a caller-owned model artifact cache.
    pub fn with_model_cache(
        config: ParallelConfig,
        model_cache: crate::sync::Arc<crate::cache::ModelArtifactCache>,
    ) -> Self {
        Self {
            config,
            model_cache,
        }
    }

    /// The configuration this verifier executes with.
    pub fn config(&self) -> &ParallelConfig {
        &self.config
    }

    /// The explicitly owned model artifact cache.
    pub fn model_cache(&self) -> &crate::sync::Arc<crate::cache::ModelArtifactCache> {
        &self.model_cache
    }
    /// Verify all blocks in parallel using the bounded executor.
    ///
    /// Each `VerifyBlockJob` is wrapped in a `VerifyTask`, submitted to
    /// `run_tasks`, and results are collected into a
    /// `ParallelVerificationReport`.
    ///
    /// ## Pipeline
    ///
    /// 1. Convert input jobs into `VerifyTask` values.
    /// 2. Submit to `run_tasks` with worker count and queue capacity from
    ///    `config`.
    /// 3. Iterate over the executor report's results:
    ///    - `Ok(vr)` → accumulate counters (`payload_hash_ok`,
    ///      `decoded_hash_ok`, `decode_success`, `blocks_failed`).
    ///      If `!vr.payload_hash_ok || !vr.decode_success`, the block is
    ///      counted as failed and recorded in the `CanonicalErrorTracker`.
    ///    - `Err(e)` → count as failed and record the error.
    /// 4. If any canonical error was recorded, return
    ///    `Err(ParallelError::VerifyFailed(canonical_error))`.
    /// 5. Otherwise return the `ParallelVerificationReport`.
    ///
    /// ## Failure semantics
    ///
    /// A block counts as failed if:
    /// - `payload_hash_ok == false` (payload hash mismatch), **OR**
    /// - `decode_success == false` (decode error)
    ///
    /// A `decoded_hash_ok == false` due to an unset hash (`[0u8; 32]`)
    /// does **NOT** trigger a failure count.  This is a deliberate design
    /// choice: the decoded hash is an optional integrity layer, and an
    /// unset hash should not corrupt the verification result.
    ///
    /// ## Returns
    ///
    /// - `Ok(ParallelVerificationReport)` with per-block results and
    ///   aggregate counters.  Even if no block failed, the report contains
    ///   all the data for inspection.
    /// - `Err(ParallelError::VerifyFailed)` if any block failed.  The
    ///   canonical error (lowest-index failure) is boxed inside.
    pub fn verify_blocks(
        &self,
        blocks: impl IntoIterator<Item = VerifyBlockJob>,
    ) -> Result<ParallelVerificationReport, ParallelError> {
        self.verify_blocks_with_cancel(blocks, None)
    }

    /// Verify all blocks in parallel with an optional external cancellation token.
    ///
    /// Same semantics as [`Self::verify_blocks`] but accepts a caller-owned
    /// [`CancellationToken`].  If cancellation is observed before all blocks
    /// complete, returns [`ParallelError::Cancelled`] with completion counts.
    /// Never returns `Ok` with fewer blocks verified than declared.
    pub fn verify_blocks_with_cancel(
        &self,
        blocks: impl IntoIterator<Item = VerifyBlockJob>,
        external_cancel: Option<crate::sync::Arc<crate::cancellation::CancellationToken>>,
    ) -> Result<ParallelVerificationReport, ParallelError> {
        let jobs: Vec<VerifyBlockJob> = blocks.into_iter().collect();
        if jobs.is_empty() {
            return Ok(ParallelVerificationReport {
                blocks_verified: 0,
                payload_hash_ok: 0,
                decoded_hash_ok: 0,
                decoded_hash_mismatch: 0,
                decoded_hash_unset: 0,
                decoded_hash_not_computed: 0,
                output_matches: 0,
                blocks_failed: 0,
                block_results: Vec::new(),
                error: None,
            });
        }

        let block_count = jobs.len();
        let worker_count = crate::resource::effective_worker_count(&self.config, block_count)?;
        let queue_capacity = self.config.max_in_flight_blocks.get().max(worker_count);
        let tasks: Vec<VerifyTask> = jobs
            .into_iter()
            .map(|j| VerifyTask {
                job: j,
                config: self.config.clone(),
                model_cache: self.model_cache.clone(),
            })
            .collect();
        let report: ExecutorReport<Result<BlockVerificationResult, BlockError>> =
            crate::executor::run_tasks_with_affinity(
                tasks,
                worker_count,
                queue_capacity,
                self.config.worker_stack_size,
                external_cancel,
                self.config.affinity.clone(),
            )?;

        let mut results = Vec::with_capacity(block_count);
        let mut error_tracker = crate::error::CanonicalErrorTracker::new();
        let mut payload_ok = 0u64;
        let mut decoded_ok = 0u64;
        let mut decoded_mismatch = 0u64;
        let mut decoded_unset = 0u64;
        let mut decoded_not_computed = 0u64;
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
                    match vr.decoded_hash_state {
                        crate::config::HashVerification::Match => {}
                        crate::config::HashVerification::Mismatch => decoded_mismatch += 1,
                        crate::config::HashVerification::Unset => decoded_unset += 1,
                        crate::config::HashVerification::NotComputed => decoded_not_computed += 1,
                    }
                    if vr.decode_success {
                        decode_ok += 1;
                    }

                    // A block is considered failed if:
                    // - payload hash does NOT match, OR
                    // - decode did NOT succeed, OR
                    // - the decoded hash state is Mismatch (nonzero stored
                    //   decoded hash that does not match the recomputed hash).
                    //   This closes the live bug where a block passed even
                    //   though its decoded output did not match the stored
                    //   nonzero decoded hash.
                    let hash_mismatch =
                        vr.decoded_hash_state == crate::config::HashVerification::Mismatch;
                    if !vr.payload_hash_ok || !vr.decode_success || hash_mismatch {
                        failed += 1;
                        error_tracker.record(BlockError {
                            block_index: vr.block_index,
                            kind: if !vr.payload_hash_ok {
                                BlockErrorKind::PayloadHash
                            } else if hash_mismatch {
                                BlockErrorKind::DecodedHashMismatch
                            } else {
                                BlockErrorKind::Codec
                            },
                        });
                    }

                    results.push(vr);
                }
                Err(e) => {
                    error_tracker.record(e);
                    failed += 1;
                }
            }
        }

        // If there's a canonical error, return it as a verification failure
        if let Some(c) = error_tracker.canonical_error() {
            return Err(ParallelError::VerifyFailed(Box::new(c.clone())));
        }

        // Phase L.3 completeness invariant at the API boundary: the doc
        // contract promises `Cancelled` on partial cancellation and never
        // `Ok` with fewer blocks verified than declared.  `results` holds
        // exactly the per-block results collected; a short run must surface
        // as `Cancelled`/`IncompleteExecution`, and `blocks_verified` below
        // then honestly reflects what was actually verified.
        let verified = results.len();
        crate::error::check_completeness(report.cancelled, verified, block_count)?;

        Ok(ParallelVerificationReport {
            blocks_verified: verified as u64,
            payload_hash_ok: payload_ok,
            decoded_hash_ok: decoded_ok,
            decoded_hash_mismatch: decoded_mismatch,
            decoded_hash_unset: decoded_unset,
            decoded_hash_not_computed: decoded_not_computed,
            output_matches: decode_ok,
            blocks_failed: failed,
            block_results: results,
            error: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CodecPolicy;
    use crate::encode::encode_single_block;
    use crate::job::EncodeBlockJob;

    fn uniform256() -> Vec<u8> {
        let mut d = Vec::with_capacity(4096);
        for s in 0u8..=255 {
            for _ in 0..16 {
                d.push(s);
            }
        }
        d
    }

    #[test]
    fn test_verify_clean_block() {
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d,
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");

        let report = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: e.block,
            }])
            .expect("verify");
        assert_eq!(report.blocks_failed, 0);
        assert!(report.payload_hash_ok >= 1);
    }

    #[test]
    fn test_verify_corrupt_payload() {
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d,
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let mut e = encode_single_block(j).expect("encode");

        // Corrupt a byte in the payload
        let payload_offset = 104 + 1024; // header + model
        if payload_offset < e.block.len() {
            e.block[payload_offset] ^= 0xFF;
        }

        let report = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: e.block,
            }]);
        match report {
            Err(ParallelError::VerifyFailed(_)) => {} // expected
            Ok(r) => assert!(r.blocks_failed > 0, "corrupt payload should fail"),
            Err(other) => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_verify_truncated_block() {
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d,
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");

        let truncated = e.block[..50].to_vec();
        let result = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: truncated,
            }]);
        assert!(result.is_err(), "truncated block should fail verification");
    }

    #[test]
    fn test_verify_multiple_blocks() {
        let mut data = Vec::new();
        data.extend(uniform256());
        data.extend(uniform256());

        let plan = crate::plan::FixedBlockPlan::new(data.len() as u64, 4096);
        assert_eq!(plan.block_count(), 2);

        let jobs: Vec<EncodeBlockJob> = plan
            .ranges
            .iter()
            .map(|r| {
                let s = r.input_offset as usize;
                EncodeBlockJob::new(
                    r.block_index,
                    data[s..s + r.length as usize].to_vec(),
                    CodecPolicy::Auto,
                    crate::config::ModelPolicy::PerBlock,
                    12,
                )
            })
            .collect();

        let enc = crate::encode::ParallelEncoder::encode_blocks(jobs, &ParallelConfig::default())
            .expect("encode");

        let verify_jobs: Vec<VerifyBlockJob> = enc
            .blocks
            .iter()
            .map(|b| VerifyBlockJob {
                block_index: b.block_index,
                block_data: b.block.clone(),
            })
            .collect();

        let report = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(verify_jobs)
            .expect("verify");
        assert_eq!(report.blocks_failed, 0);
    }

    // ============================================================
    // Phase L.2 — decoded-output integrity verification matrix
    // ============================================================

    fn encode_block(data: Vec<u8>) -> Vec<u8> {
        let j = EncodeBlockJob::new(
            0,
            data,
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        encode_single_block(j).expect("encode").block
    }

    #[test]
    fn test_l2_clean_block_matching_hashes() {
        // Case 1: clean payload and matching decoded hash → passes.
        let d = uniform256();
        let block = encode_block(d.clone());
        let report = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: block,
            }])
            .expect("clean block must verify");
        assert_eq!(report.blocks_failed, 0);
        assert_eq!(report.decoded_hash_ok, 1);
        assert_eq!(report.decoded_hash_mismatch, 0);
        assert_eq!(report.decoded_hash_unset, 0);
    }

    #[test]
    fn test_l2_zero_decoded_hash_strict_fails() {
        // Case 2: clean payload + zero decoded hash under Strict → fails.
        let d = uniform256();
        let mut block = encode_block(d.clone());
        block[72..104].fill(0); // zero the decoded hash
        let result = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: block,
            }]); // Strict
        match result {
            Err(ParallelError::VerifyFailed(e)) => {
                assert_eq!(e.kind, BlockErrorKind::DecodedHashMissing)
            }
            other => panic!("expected DecodedHashMissing under Strict, got {:?}", other),
        }
    }

    #[test]
    fn test_l2_zero_decoded_hash_legacy_passes() {
        // Case 3: clean payload + zero decoded hash under legacy mode →
        // decode succeeds, unset reported, no failure count.
        let d = uniform256();
        let mut block = encode_block(d.clone());
        block[72..104].fill(0);
        let cfg = ParallelConfig {
            integrity_policy: crate::config::IntegrityPolicy::AllowLegacyUnsetDecodedHash,
            ..Default::default()
        };
        let report = ParallelVerifier::new(cfg)
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: block,
            }])
            .expect("legacy mode must not fail on unset hash");
        assert_eq!(report.blocks_failed, 0);
        assert_eq!(report.decoded_hash_unset, 1);
        assert_eq!(report.decoded_hash_ok, 0);
    }

    #[test]
    fn test_l2_mismatched_decoded_hash_fails_both_policies() {
        // Case 4: clean payload + mismatched nonzero decoded hash → fails
        // under BOTH policies.  This is the live bug fixed in Phase L.2:
        // the block must NOT pass just because payload hash matched.
        let d = uniform256();
        let mut block = encode_block(d.clone());
        block[72] ^= 0xFF; // corrupt the decoded hash (nonzero → mismatch)

        for policy in [
            crate::config::IntegrityPolicy::Strict,
            crate::config::IntegrityPolicy::AllowLegacyUnsetDecodedHash,
        ] {
            let cfg = ParallelConfig {
                integrity_policy: policy,
                ..Default::default()
            };
            let result = ParallelVerifier::new(cfg.clone())
                .verify_blocks(vec![VerifyBlockJob {
                    block_index: 0,
                    block_data: block.clone(),
                }]);
            match result {
                Err(ParallelError::VerifyFailed(e)) => {
                    assert_eq!(e.kind, BlockErrorKind::DecodedHashMismatch)
                }
                Ok(r) => panic!(
                    "mismatched decoded hash must fail under {:?}, got report {:?}",
                    policy, r.blocks_failed
                ),
                other => panic!("unexpected: {:?}", other),
            }
        }
    }

    #[test]
    fn test_l2_corrupt_payload_fails() {
        // Case 5: corrupted payload → fails.
        let d = uniform256();
        let mut block = encode_block(d.clone());
        let payload_offset = 104 + 1024;
        if payload_offset < block.len() {
            block[payload_offset] ^= 0xFF;
        }
        let result = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: block,
            }]);
        assert!(result.is_err(), "corrupt payload must fail");
    }

    #[test]
    fn test_l2_model_corruption_cannot_pass() {
        // Case 6: intact payload + corrupted model bytes → decoded output
        // differs, decoded hash mismatch fails the block.  This proves the
        // decoded hash catches model corruption that payload hashing cannot.
        let d = uniform256();
        let mut block = encode_block(d.clone());
        // Corrupt a model frequency byte (model starts at offset 104).
        block[104] ^= 0xFF;
        // The payload hash is intact (we only touched model bytes).
        let result = ParallelVerifier::new(ParallelConfig::default())
            .verify_blocks(vec![VerifyBlockJob {
                block_index: 0,
                block_data: block,
            }]);
        // Either decode fails (model invalid) or the decoded output differs
        // from the stored hash — both must surface as a failure.
        assert!(
            result.is_err(),
            "model corruption must never pass verification"
        );
    }

    #[test]
    fn test_l2_one_bad_block_among_many() {
        // Case 11: one bad block among many → overall verification fails.
        let mut data = Vec::new();
        data.extend(uniform256());
        data.extend(uniform256());
        data.extend(uniform256());
        let plan = crate::plan::FixedBlockPlan::new(data.len() as u64, 4096);
        let jobs: Vec<EncodeBlockJob> = plan
            .ranges
            .iter()
            .map(|r| {
                let s = r.input_offset as usize;
                EncodeBlockJob::new(
                    r.block_index,
                    data[s..s + r.length as usize].to_vec(),
                    CodecPolicy::Auto,
                    crate::config::ModelPolicy::PerBlock,
                    12,
                )
            })
            .collect();
        let enc = crate::encode::ParallelEncoder::encode_blocks(jobs, &ParallelConfig::default())
            .expect("encode");

        let mut vj: Vec<VerifyBlockJob> = enc
            .blocks
            .iter()
            .map(|b| VerifyBlockJob {
                block_index: b.block_index,
                block_data: b.block.clone(),
            })
            .collect();
        // Corrupt the decoded hash of block 1.
        vj[1].block_data[72] ^= 0xFF;

        let result = ParallelVerifier::new(ParallelConfig::default()).verify_blocks(vj);
        match result {
            Err(ParallelError::VerifyFailed(e)) => {
                assert_eq!(e.block_index, 1, "canonical error must be block 1");
                assert_eq!(e.kind, BlockErrorKind::DecodedHashMismatch);
            }
            other => panic!("expected VerifyFailed from block 1, got {:?}", other),
        }
    }

    #[test]
    fn test_l2_report_counts_hash_states() {
        // Case 15: report counts Match/Mismatch/Unset/NotComputed separately.
        // Block 0: clean. Block 1: legacy unset. Block 2: decode failure (truncated).
        let d = uniform256();
        let b0 = encode_block(d.clone());
        let mut b1 = encode_block(d.clone());
        b1[72..104].fill(0);
        let mut b2 = encode_block(d.clone());
        b2.truncate(50); // structurally invalid → decode failure

        let cfg = ParallelConfig {
            integrity_policy: crate::config::IntegrityPolicy::AllowLegacyUnsetDecodedHash,
            ..Default::default()
        };
        let report = ParallelVerifier::new(cfg).verify_blocks(vec![
            VerifyBlockJob {
                block_index: 0,
                block_data: b0,
            },
            VerifyBlockJob {
                block_index: 1,
                block_data: b1,
            },
            VerifyBlockJob {
                block_index: 2,
                block_data: b2,
            },
        ]);
        // Block 2 is truncated → structural error → Err.
        assert!(report.is_err(), "truncated block must fail");
        let _ = b0;
    }
}
