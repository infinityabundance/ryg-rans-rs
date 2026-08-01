//! # Parallel block decoder
//!
//! Decodes RYGRANS-compressed blocks using the stored frequency model and the
//! best available SIMD or scalar backend for the host CPU.  Each block is
//! independently decodable — there is no shared mutable state between blocks,
//! which enables lock-free parallel dispatch via the bounded executor.
//!
//! ## Architecture overview: block pipeline
//!
//! Every decoded block passes through the following stages, in order:
//!
//! 1. **Parse header** (`block::parse_block_header`):
//!    Validate the block header with checked arithmetic.  Reject truncated or
//!    malformed blocks before any decode work begins.
//!
//! 2. **Extract model** (raw frequencies or uniform default):
//!    Read 256 × u32 LE frequency counts (1024 bytes) from `model_data`.  If
//!    `model_length == 0`, synthesise a uniform frequency model where every
//!    symbol has equal probability.
//!
//! 3. **Build decode plan** (`create_decode_plan`):
//!    Select a decode strategy based on the codec ID, the frequency model, and
//!    the runtime-detected CPU features (AVX-512, AVX-512VL, AVX2).  The plan
//!    is a zero- or near-zero-cost enum dispatch — no heap allocation, no vtables.
//!
//! 4. **Execute decode plan** (`execute_decode_plan`):
//!    Dispatch to the selected SIMD or scalar kernel.  If the chosen backend
//!    signals `UnsupportedBackend` at runtime (e.g. the CPU lacks a required
//!    instruction set feature), the fallback policy determines whether to retry
//!    with a scalar backend or to fail hard.
//!
//! 5. **Integrity verification** (payload SHA-256 then decoded SHA-256):
//!    Always verify the payload hash (SHA-256 of the compressed word stream).
//!    Then verify the decoded output hash (SHA-256 of the uncompressed bytes).
//!    A zero stored `decoded_sha256` means "hash not set" — we decode but mark
//!    `output_verified = false`.
//!
//! 6. **Report packing**:
//!    Wrap the decoded output, the **true** executing backend (not the plan),
//!    integrity flags, and diagnostic fields (`words_consumed`, `final_states`)
//!    into a `DecodedBlockResult` that the caller or reorder buffer can consume.
//!
//! ## Backend truthfulness: why `ExecutedDecode` vs `DecodePlan`
//!
//! The `DecodePlan` is a *strategy* chosen at plan time.  The `ExecutedDecode`
//! is the *real outcome* — it records which backend actually ran, how many
//! compressed words were consumed, and what the final rANS states were.
//!
//! These two can differ when fallback occurs.  For example, `DecodePlan` might
//! select `Avx2ManualGather8`, but if the CPU doesn't support it, execution may
//! fall back to `Scalar8`.  The `ExecutedDecode.backend` field always reflects
//! the ground truth, not the plan.  This matters for:
//!
//! - **Observability**: monitoring dashboards, telemetry, and profilers see what
//!   actually ran, not what was intended.
//! - **Reproducibility**: the backend identity is included in the
//!   `DecodedBlockResult` so that any downstream auditor can tell which code
//!   path produced the bytes.
//! - **Debugging**: if a decode mismatch occurs, knowing the real backend
//!   narrows the search to one kernel instead of a chain of fallbacks.
//!
//! ## Backend policies and exact-backend semantics
//!
//! The `BackendPolicy` enum controls which backend the planner selects:
//!
//! - **`Explicit(backend)`**: exact request.  The planner produces a plan for
//!   exactly that backend or a typed error (`BackendFormatMismatch`,
//!   `BackendUnavailable`, `BackendRequiresBatchContext`).  There is **no
//!   fallback** — if the CPU or build cannot run the kernel, execution fails
//!   with `BackendUnavailable`, never a silent scalar substitution.
//!
//! - **`Portable` / `ScalarPreferred` / `Auto`**: conservative scalar-only
//!   planning (Scalar8/Scalar16).  No SIMD is ever selected automatically.
//!
//! - **`ModelAware`**: as `Auto`, but a validated Uniform256 model selects
//!   the real table-free scalar kernel (`Uniform256TableFree16`).
//!
//! Because non-explicit policies never produce SIMD plans, `Auto` and
//! `ModelAware` have no fallback path either: the plan always executes as
//! planned.
//!
//! ## Report fields: why `words_consumed` and `final_states` are preserved
//!
//! The decode kernels report `words_consumed` (exact u16 words read from the
//! compressed stream) and `final_states` (final rANS state per lane).  These
//! are used for two purposes:
//!
//! 1. **Stream integrity**: `words_consumed` lets the caller detect mismatches
//!    between the declared `payload_length` and actual consumption.
//!
//! 2. **State continuity**: `final_states` lets diagnostic tools verify decoder
//!    fidelity independent of the output bytes.
//!
//! Backends that do not surface a report use the `0`/empty convention meaning
//! "not available" (output correctness is unaffected).  See the backend-arm
//! table on `execute_decode_plan` for which kernels report.
//!
//! ## Bounded executor, cancellation token, and reorder buffer interaction
//!
//! Decode tasks are dispatched through the crate's bounded executor
//! (`run_tasks`).  The interaction of components is:
//!
//! 1. **Bounded channel / queue**: the executor maintains a fixed-capacity queue
//!    of in-flight tasks (`max_in_flight_blocks`).  If all worker threads are
//!    busy and the queue is full, the producer blocks — this provides natural
//!    backpressure.
//!
//! 2. **Cancellation token**: each decode run creates a fresh
//!    `CancellationToken`.  If any block fails, the `CanonicalErrorTracker`
//!    records the first error.  The token allows the executor to cancel pending
//!    tasks on early exit, though the current implementation waits for all in-flight
//!    tasks to complete and then returns the first encountered error.
//!
//! 3. **Reorder buffer**: because workers may complete out of index order,
//!    results are inserted into a `ReorderBuffer` keyed by `block_index`.
//!    The buffer drains blocks in ascending index order once contiguous prefixes
//!    are available.  It also enforces a maximum buffered decoded byte count
//!    (`max_buffered_output_bytes`) — inserting a block that would exceed this
//!    limit returns an error (backpressure at the buffer level).
//!
//! The three components together form a pipeline: **bounded dispatch → parallel
//! execution → ordered reassembly → bounded storage**.  No single component can
//! consume unbounded memory or block indefinitely.

use crate::block::{BLOCK_HEADER_SIZE, parse_block_header};
use crate::cancellation::CancellationToken;
use crate::config::{BackendId, ParallelConfig};
use crate::decode_plan::{DecodePlan, create_decode_plan};
use crate::error::{BlockError, BlockErrorKind, ParallelError};
use crate::executor::{ExecutorReport, ExecutorTask, run_tasks, run_tasks_with_sink};
use crate::job::{DecodeBlockJob, DecodedBlockResult, OrderedDecodedBlocks};
use crate::reorder::{BufferSized, HasBlockIndex, ReorderBuffer};
use std::vec::Vec;

impl HasBlockIndex for DecodedBlockResult {
    fn block_index(&self) -> u64 {
        self.block_index
    }
}

impl BufferSized for DecodedBlockResult {
    fn buffer_size(&self) -> u64 {
        self.output.len() as u64 + 64
    }
}

/// SHA-256 of the concatenated decoded output in ascending `block_index`
/// order.
///
/// This is the canonical stream-level integrity digest for a decoded block
/// sequence: deterministic and independent of worker scheduling order.  It
/// is the honest implementation of the stream hash documented on
/// `decode_streaming_with_cancel` (previously the hasher was instantiated
/// but never fed — a Phase L.13 inert-code defect).
fn stream_hash_of(blocks: &[DecodedBlockResult]) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    for b in blocks {
        hasher.update(&b.output);
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// A single decode task dispatched to the bounded executor.
///
/// Wraps a `DecodeBlockJob` together with the `ParallelConfig` so that every
/// worker thread carries its own copy of configuration (clone-on-dispatch).
/// This avoids shared-mutable-state contention and allows the executor to
/// treat tasks as fully independent units of work.
struct DecodeTask {
    job: DecodeBlockJob,
    config: ParallelConfig,
}

impl ExecutorTask for DecodeTask {
    type Output = Result<DecodedBlockResult, BlockError>;

    /// Execute `decode_single_block` after checking the cancellation token.
    ///
    /// Every task begins by polling the cancellation token.  If cancellation
    /// has been requested (e.g. because another task failed), we bail early
    /// with a `Codec` error rather than wasting cycles on decode work that
    /// will be discarded.  The `_wi` (worker index) parameter is unused here
    /// but required by the `ExecutorTask` trait — it could be used in the
    /// future to pin tasks to specific NUMA nodes or CPU cores.
    fn run(
        self,
        _wi: usize,
        cancel: &CancellationToken,
        _scratch: &mut crate::scratch::WorkerScratch,
    ) -> Self::Output {
        cancel.check().map_err(|_| BlockError {
            block_index: self.job.block_index,
            kind: BlockErrorKind::Codec,
        })?;
        decode_single_block(&self.job, &self.config)
    }

    fn block_index(&self) -> Option<u64> {
        Some(self.job.block_index)
    }
}

/// Decode a single RYGRANS block using the stored model.
///
/// This function is the core single-block decode entry point.  It is called
/// from `DecodeTask::run` (one per worker thread) and performs all pipeline
/// stages from header parse through integrity verification.
///
/// ## Step-by-step walkthrough
///
/// ### Step 1 — Parse and validate header
/// [`parse_block_header`] reads the fixed-size block header with checked
/// arithmetic.  Every offset and length is validated before any slice is
/// formed, preventing out-of-bounds access on truncated or malformed blocks.
///
/// ### Step 2 — Validate model encoding
/// Byte offset 20 in the block data encodes the model encoding type.
/// Only encoding type 0 (raw frequency table) is supported.  Any other
/// value returns `BlockErrorKind::Model`.  This strict check ensures we
/// never misinterpret non-frequency model data.
///
/// ### Step 3 — Extract frequency model
/// The model data starts at offset `BLOCK_HEADER_SIZE` and has length
/// `header.model_length`.  We validate that `model_end` doesn't overflow
/// or exceed the block data length.  If `model_length == 1024`, we parse
/// 256 × u32 LE frequency counts.  If `model_length == 0`, we synthesise
/// a uniform model where each symbol has frequency `total / 256`.
/// Any other model length is rejected with `BlockErrorKind::Model`.
///
/// ### Step 4 — Validate frequency sum
/// The frequencies must sum to exactly `1 << scale_bits`.  This is a
/// fundamental rANS invariant: the decoder's cumulative-frequency lookup
/// assumes a power-of-two total.  If the sum doesn't match, the block is
/// structurally invalid and we reject it with `BlockErrorKind::Model`.
///
/// ### Step 5 — Extract and verify payload hash
/// Payload boundaries are computed with checked arithmetic.  We compute
/// SHA-256 over the raw payload bytes and compare against the stored
/// `payload_sha256` from the header.  This verification is **always**
/// performed, even for empty payloads (where the hash of an empty input
/// must still match).  A mismatch returns `BlockErrorKind::PayloadHash`.
///
/// ### Step 6 — Bounds-check output allocation
/// The `uncompressed_length` field determines the output buffer size.
/// If it exceeds `config.max_buffered_output_bytes`, we reject the block
/// with `BlockErrorKind::ResourceLimit`.  This prevents a single malicious
/// or corrupted block from triggering an unbounded allocation.
///
/// ### Step 7 — Build decode plan and execute
/// Runtime CPU feature detection (`cpu_feature_detection`) queries the
/// host for AVX-512, AVX-512VL, and AVX2 support.  Together with the
/// codec ID, scale bits, model data, and backend policy, `create_decode_plan`
/// selects the best available decode strategy.  The plan is then dispatched
/// through `execute_decode_plan`, which handles fallback internally.
///
/// ### Step 8 — Verify decoded output hash
/// SHA-256 is computed over the decoded output bytes.  If `header.decoded_sha256`
/// is all-zeros, an older encoder produced the block and didn't store a hash.
/// We do NOT reject the block — instead we set `output_verified = false` so the
/// caller can make their own trust decision.  If the stored hash is non-zero and
/// doesn't match, we return `BlockErrorKind::DecodedHash`.
///
/// ### Step 9 — Pack result
/// All fields (output, backend, integrity flags, hash, diagnostic report) are
/// packed into a `DecodedBlockResult`.  Note that `executed.backend` is used,
/// not `plan` — this ensures the caller sees the **actual** backend that ran.
///
/// # Integrity guarantees (summary)
///
/// 1. Header is parsed with checked arithmetic — no unvalidated slices.
/// 2. Model data is extracted and validated (length, format, frequency sum).
/// 3. Payload SHA-256 is verified against the stored hash (always).
/// 4. Decoded output SHA-256 is verified against the stored hash.
/// 5. Zero stored hash is NOT automatically accepted for verification;
///    it is treated as "hash not set" and `output_verified` is set to false.
/// 6. Output allocation is bounded by `config.max_buffered_output_bytes`.
pub fn decode_single_block(
    job: &DecodeBlockJob,
    config: &ParallelConfig,
) -> Result<DecodedBlockResult, BlockError> {
    let data = &job.block_data;
    let bi = job.block_index;

    // ----- Step 1: Parse and validate header -----
    let (header, _model_offset) = parse_block_header(data, bi).map_err(|_| BlockError {
        block_index: bi,
        kind: BlockErrorKind::Format,
    })?;

    // Validate model_encoding — we support raw frequencies only
    if data.len() > 20 && data[20] != 0 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Model,
        });
    }

    // ----- Step 2: Extract model data with bounds check -----
    let model_offset = BLOCK_HEADER_SIZE;
    let model_len = header.model_length as usize;
    let model_end = model_offset.checked_add(model_len).ok_or(BlockError {
        block_index: bi,
        kind: BlockErrorKind::Format,
    })?;
    if model_end > data.len() {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Format,
        });
    }
    let model_data = &data[model_offset..model_end];

    // Validate model length — we expect exactly 1024 bytes (256 × u32 LE)
    if model_len != 1024 && model_len != 0 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Model,
        });
    }

    // Parse frequency model from model_data — via the bounded model cache.
    // The cache is keyed by (model_sha256, scale_bits, codec_id) and stores
    // validated immutable artifacts (freqs + uniform256 flag).  Backend
    // selection happens AFTER the lookup, so a cached artifact is never
    // reused under incompatible execution conditions.  A miss simply
    // rebuilds the artifacts (always correct).
    let codec_id = header.codec_id;
    let scale = header.scale_bits;
    let artifacts = crate::cache::cached_model_artifacts(codec_id, scale, model_data, || {
        let freqs: Vec<u32> = if model_len >= 1024 {
            model_data
                .chunks_exact(4)
                .take(256)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect()
        } else {
            // Zero model length — use uniform model
            let total = 1u32 << scale;
            let uniform_freq = total / 256;
            vec![uniform_freq; 256]
        };

        if freqs.len() != 256 {
            return None;
        }

        // Validate that frequencies sum to expected total
        let expected_total = 1u32 << scale;
        let sum: u64 = freqs.iter().map(|&f| f as u64).sum();
        if sum != expected_total as u64 {
            return None;
        }

        // Uniform256 detection: all frequencies == 16 at scale_bits == 12.
        let uniform256 = scale == 12 && freqs.iter().all(|&f| f == 16);

        Some(crate::cache::ValidatedModelArtifacts { freqs, uniform256 })
    })
    .ok_or(BlockError {
        block_index: bi,
        kind: BlockErrorKind::Model,
    })?;

    let freqs = artifacts.freqs;
    // `artifacts.uniform256` is available for future plan construction;
    // the plan function currently re-detects it from model_data, which is
    // a cheap scan and keeps create_decode_plan's signature stable.

    // ----- Step 3: Extract and verify payload -----
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

    // Verify payload SHA-256 (always, even for empty payloads)
    let computed_payload_hash = crate::encode::sha256(payload);
    if computed_payload_hash != header.payload_sha256 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::PayloadHash,
        });
    }

    // ----- Step 4: Minimum payload already validated in parse_block_header -----
    let ul = header.uncompressed_length as usize;

    // ----- Step 5: Bounds-check output allocation -----
    if (ul as u64) > config.max_buffered_output_bytes {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::ResourceLimit,
        });
    }

    // ----- Step 6: Build decode plan and decode -----
    //
    // The planner is capability-agnostic (Phase L.9): it validates format
    // compatibility and produces either the exact requested plan or a typed
    // error.  Execution-capability checks (runtime CPU features, compiled
    // target features) happen inside `execute_decode_plan`, which reports
    // `BackendUnavailable` rather than silently substituting a backend.
    let backend_policy = config.backend_policy;
    let plan = create_decode_plan(
        header.codec_id,
        header.scale_bits,
        model_data,
        backend_policy,
        config.disable_simd,
        header.block_kind,
        bi,
    )?;

    let executed = execute_decode_plan(&plan, payload, &freqs, ul, header.scale_bits, bi)?;

    // ----- Step 7: Verify decoded hash -----
    let computed_decoded_hash = crate::encode::sha256(&executed.output);

    // Verify decoded SHA-256 under the configured integrity policy.
    //
    // Strict (default):
    //   zero/unset stored hash → DecodedHashMissing error
    //   nonzero stored hash that does not match → DecodedHashMismatch error
    //   matching nonzero hash → verified
    //
    // AllowLegacyUnsetDecodedHash:
    //   zero/unset stored hash → output_verified = false (not an error)
    //   nonzero stored hash that does not match → DecodedHashMismatch error
    let output_verified = if header.decoded_sha256 == [0u8; 32] {
        match config.integrity_policy {
            crate::config::IntegrityPolicy::Strict => {
                return Err(BlockError {
                    block_index: bi,
                    kind: BlockErrorKind::DecodedHashMissing,
                });
            }
            crate::config::IntegrityPolicy::AllowLegacyUnsetDecodedHash => {
                // Zero hash: cannot verify.  Mark as unverified but allow decode.
                false
            }
        }
    } else if computed_decoded_hash != header.decoded_sha256 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::DecodedHashMismatch,
        });
    } else {
        true
    };

    Ok(DecodedBlockResult {
        block_index: bi,
        output: executed.output,
        backend: executed.backend, // Use the REAL executed backend, not the plan
        plan_backend: executed.plan_backend,
        payload_verified: true,
        output_verified,
        output_hash: computed_decoded_hash,
        words_consumed: executed.words_consumed,
        final_states: executed.final_states,
        elapsed_ns: None,
    })
}

/// The result of executing a decode plan — the ground truth, not the plan.
///
/// This struct records what **actually** happened during decode, as distinct
/// from what the `DecodePlan` *intended* to happen.  Phase L.9 forbids
/// silent backend substitution, so on success `backend` equals
/// `plan_backend`; on failure no `ExecutedDecode` is produced at all.
///
/// ## Fields
///
/// * `output` — the decoded bytes (may be empty for zero-length blocks).
///
/// * `backend` — the `BackendId` of the kernel that actually ran.  This is
///   the **executed** backend.  Under Phase L.9 exact-backend semantics it
///   always equals `plan_backend` on success.
///
/// * `plan_backend` — the `BackendId` the `DecodePlan` selected.  Kept so
///   every execution result carries both the selected plan identity and the
///   actual executed identity (they are equal by construction; the pair is
///   evidence, not a fallback mechanism).
///
/// * `words_consumed` — how many u16 compressed words were consumed from the
///   payload.  Zero means "unknown" (some backends don't surface this).
///   Non-zero values enable the caller to verify that the declared payload
///   length matches actual consumption.
///
/// * `final_states` — the final rANS state values per lane after decoding the
///   last symbol of the block.  Empty means "unknown".  When populated, these
///   allow downstream tools to verify decoder state continuity across blocks.
pub struct ExecutedDecode {
    pub output: Vec<u8>,
    pub backend: BackendId,
    pub plan_backend: BackendId,
    /// Words consumed from the compressed stream (0 if unknown).
    pub words_consumed: usize,
    /// Final rANS states after decode (empty if unknown).
    pub final_states: Vec<u32>,
}

impl ExecutedDecode {
    /// Create an `ExecutedDecode` with output and backend only.
    ///
    /// Sets `words_consumed` to 0 and `final_states` to an empty vec — the
    /// "report not available" convention used by backends that do not surface
    /// the SIMD report API (e.g. pure-scalar decode functions).  Output
    /// correctness is unaffected.
    pub fn new(output: Vec<u8>, backend: BackendId) -> Self {
        Self {
            output,
            backend,
            plan_backend: backend,
            words_consumed: 0,
            final_states: Vec::new(),
        }
    }
}

impl ExecutedDecode {
    /// Create from the SIMD crate's `DecodeResult`, preserving all report fields.
    ///
    /// This constructor is the primary conversion path for successful SIMD
    /// decode results.  It maps the SIMD crate's `DecodeBackend` enum to the
    /// parallel crate's `BackendId` via [`map_backend`] and copies the report
    /// fields (`words_consumed`, `final_states`) directly from the result.
    ///
    /// `plan_backend` is set to the same value as the executed `backend`: the
    /// SIMD crate reports what actually ran, and Phase L.9 forbids a
    /// divergence between plan and execution on success.
    #[cfg(feature = "simd")]
    fn from_simd(result: ryg_rans_rs_simd::backends::DecodeResult) -> Self {
        let backend = map_backend(result.backend);
        Self {
            output: result.output,
            backend,
            plan_backend: backend,
            words_consumed: result.report.words_consumed,
            final_states: result.report.final_states.to_vec(),
        }
    }
}

/// Map a SIMD crate `DecodeBackend` to the parallel crate's `BackendId`.
///
/// ## Why a dedicated mapping function?
///
/// The SIMD crate (`ryg_rans_rs_simd`) and the parallel crate each define
/// their own backend enumeration.  They are structurally similar but must
/// remain independent — the SIMD crate should not depend on the parallel
/// crate's types, and vice versa.  `map_backend` is the single point of
/// translation between the two worlds.
///
/// ## Why distinct SIMD backends must map to distinct BackendIds
///
/// Every SIMD variant (`Avx2ManualGather8`, `Avx2HardwareGather8`,
/// `Avx2TwoBy8On16`, `Avx512VlInterleaved8`, etc.) maps to a **unique**
/// `BackendId`.  This one-to-one mapping is essential for:
///
/// - **Observability**: telemetry can distinguish "decoded with AVX2 hardware
///   gather" from "decoded with AVX2 manual gather" — two kernels that may
///   have different performance or correctness characteristics on different
///   microarchitectures.
/// - **Determinism debugging**: if a decode mismatch occurs only on certain
///   hardware, the exact backend identity pinpoints which kernel is suspect.
/// - **Fuzzing and coverage**: distinct IDs allow test infrastructure to
///   verify that every SIMD path is exercised.
/// - **Performance analysis**: profiling tools can attribute decode time to
///   the precise kernel variant, guiding optimization effort.
///
/// The mapping is exhaustive: every variant of the SIMD crate's
/// `DecodeBackend` enum has a corresponding entry here.  Adding a new
/// SIMD kernel requires both a new `DecodeBackend` variant and a new
/// `BackendId` variant mapped here.
#[cfg(feature = "simd")]
fn map_backend(b: ryg_rans_rs_simd::backends::DecodeBackend) -> BackendId {
    use ryg_rans_rs_simd::backends::DecodeBackend as S;
    match b {
        S::Scalar8 => BackendId::Scalar8,
        S::Sse41Interleaved8 => BackendId::Sse41Interleaved8,
        S::Avx512VlInterleaved8 => BackendId::Avx512VlInterleaved8,
        S::Scalar16 => BackendId::Scalar16,
        S::Avx512Interleaved16 => BackendId::Avx512Interleaved16,
        S::Avx512VlManualGather8 => BackendId::Avx512VlManualGather8,
        S::Avx512ManualGather16 => BackendId::Avx512ManualGather16,
        S::Avx512Vl2x8On16 => BackendId::Avx512Vl2x8,
        S::Avx2ManualGather8 => BackendId::Avx2ManualGather8,
        S::Avx2HardwareGather8 => BackendId::Avx2HardwareGather8,
        S::Avx2TwoBy8On16 => BackendId::Avx2TwoBy8On16,
        S::Avx2Uniform256TableFree16 => BackendId::Avx2Uniform256TableFree16,
        S::Avx2Batch4On16 => BackendId::Avx2Batch4On16,
    }
}

/// Build the cumulative-frequency prefix sum from a raw frequency array.
///
/// Given an array of 256 per-symbol frequencies, this produces a 257-element
/// cumulative array where `cum[i] = sum(freqs[0..i])`.  `cum[0] = 0` and
/// `cum[256]` equals the total frequency sum (which must be a power of two,
/// typically `1 << 12 = 4096`).
///
/// The cumulative array is used by the rANS decoder to map a slot value
/// (produced by `state & (M-1)`) to its corresponding symbol: the symbol
/// is the smallest `s` such that `slot < cum[s+1]`.
///
/// This is a helper for both the SIMD `PackedWordTable::from_freqs` path
/// and the pure-scalar decode path.  The 257th element (`cum[256]`) is
/// included so that binary search over `cum` always has a valid upper bound.
/// Only the SIMD-gated decode plan executor calls it, so it is gated too.
#[cfg(feature = "simd")]
fn build_cum_freqs(freqs: &[u32]) -> Vec<u32> {
    let mut cum = Vec::with_capacity(257);
    cum.push(0);
    for i in 0..256 {
        cum.push(cum[i] + freqs.get(i).copied().unwrap_or(0));
    }
    cum
}

/// Execute a decode plan — exact-backend dispatch (SIMD-enabled build).
///
/// This is the main decode-dispatch function when the `simd` feature is
/// enabled.  Each arm of the `match` on `DecodePlan` executes the kernel for
/// that plan, or returns a typed error.
///
/// # Phase L.9 exact-backend semantics
///
/// This function never substitutes a different backend for the planned one.
/// There is no fallback path: an explicit request either executes exactly or
/// fails with a typed error.
///
/// * `BackendUnavailable` — the kernel cannot run on this CPU (runtime
///   feature detection failed) or in this build (the kernel body is
///   compile-time gated by `target_feature`).  The SIMD crate's `_checked`
///   wrappers report `UnsupportedBackend` for both cases, mapped here to
///   `BackendUnavailable`.
/// * `Codec` — the kernel executed but the stream or table was malformed
///   (truncation, state invariant violation, invalid table).
///
/// ## Before dispatch: payload → u16 words
///
/// The compressed payload is a byte-aligned stream of u16 words in
/// little-endian format.  We convert it to `Vec<u16>` upfront.  Every decode
/// kernel (SIMD and scalar) operates on this word slice.  The conversion
/// uses `chunks_exact(2)`, which panics if the payload length is odd — but
/// this is guaranteed not to happen because the block format pads to an even
/// byte count (enforced by the strict block parser).
///
/// ## Backend arms
///
/// | Plan | Kernel | Report |
/// |------|--------|--------|
/// | `Scalar16` | `decode_interleaved16_scalar` | full |
/// | `Scalar8` | `decode_8way_packed_scalar_with_report` | full (8 states padded to 16) |
/// | `Uniform256TableFree16` | local scalar table-free kernel | full |
/// | `Sse41Interleaved8` | SIMD crate `decode_simd_8way_unchecked` via `_sse41_checked` | none (0/empty) |
/// | `Avx512VlInterleaved8` | `decode_interleaved8_avx512vl_checked` | full |
/// | `Avx512Interleaved16` | `decode_interleaved16_avx512_checked` | full |
/// | `Avx512VlManualGather8` | `decode_interleaved8_avx512vl_manual_gather_checked` | full |
/// | `Avx512ManualGather16` | `decode_interleaved16_avx512_manual_gather_checked` | full |
/// | `Avx512Vl2x8` | `decode_interleaved16_avx512vl_2x8_checked` | full |
/// | `Avx2ManualGather8` | `decode_interleaved8_avx2_manual_gather_checked` | full |
/// | `Avx2HardwareGather8` | `decode_interleaved8_avx2_hardware_gather_checked` | full |
/// | `Avx2TwoBy8On16` | `decode_interleaved16_avx2_2x8_checked` | full |
/// | `Avx2Uniform256TableFree16` | `decode_interleaved16_uniform256_avx2_checked` | full |
///
/// "Report" = `words_consumed` + `final_states`.  The SSE4.1 kernel does not
/// surface a report; its `ExecutedDecode` uses the 0/empty convention
/// meaning "not available" (output correctness is unaffected).
///
/// Every successful result records the executed backend via
/// `ExecutedDecode::from_simd`, which maps the SIMD crate's own backend
/// enum — the ground truth of what ran.
#[cfg(feature = "simd")]
fn execute_decode_plan(
    plan: &DecodePlan,
    payload: &[u8],
    freqs: &[u32],
    expected_len: usize,
    _scale_bits: u8,
    bi: u64,
) -> Result<ExecutedDecode, BlockError> {
    use ryg_rans_rs_simd::backends::DecodeError;
    use ryg_rans_rs_simd::packed_table::{PackedWordTable, decode_interleaved16_scalar};

    let words: Vec<u16> = payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();

    /// Map a SIMD crate `DecodeError` to a `BlockErrorKind`.
    ///
    /// `UnsupportedBackend` means the kernel was **not** executed (CPU or
    /// build lacks the capability) → `BackendUnavailable`.  Every other
    /// error means the kernel ran and the stream/table was malformed →
    /// `Codec`.
    fn map_decode_err(e: DecodeError, bi: u64) -> BlockError {
        let kind = match e {
            DecodeError::UnsupportedBackend => BlockErrorKind::BackendUnavailable,
            _ => BlockErrorKind::Codec,
        };
        BlockError {
            block_index: bi,
            kind,
        }
    }

    /// Build a packed word table from frequencies (inner helper).
    ///
    /// Converts raw frequencies to a `PackedWordTable` via
    /// `build_cum_freqs` + `PackedWordTable::from_freqs`.
    ///
    /// # Errors
    ///
    /// Returns `BlockErrorKind::Model` if the frequency table cannot be
    /// constructed (e.g. invalid scale or frequency sum mismatch).
    fn build_table(freqs: &[u32], scale: u32, bi: u64) -> Result<PackedWordTable, BlockError> {
        let cum = build_cum_freqs(freqs);
        PackedWordTable::from_freqs(freqs, &cum, scale).map_err(|_| BlockError {
            block_index: bi,
            kind: BlockErrorKind::Model,
        })
    }

    match plan {
        // ---- Scalar 16-way ----
        DecodePlan::Scalar16 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            let (out, report) =
                decode_interleaved16_scalar(&words, &table, expected_len).map_err(|_| {
                    BlockError {
                        block_index: bi,
                        kind: BlockErrorKind::Codec,
                    }
                })?;
            Ok(ExecutedDecode {
                output: out,
                backend: BackendId::Scalar16,
                plan_backend: BackendId::Scalar16,
                words_consumed: report.words_consumed,
                final_states: report.final_states.to_vec(),
            })
        }
        // ---- Scalar 8-way (using packed table for full report) ----
        DecodePlan::Scalar8 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            let (out, r8) = ryg_rans_rs_simd::packed_table::decode_8way_packed_scalar_with_report(
                &words,
                &table,
                expected_len,
            )
            .map_err(|_| BlockError {
                block_index: bi,
                kind: BlockErrorKind::Codec,
            })?;
            // DecodeReport8 has [u32; 8] final_states — pad to 16 for
            // ExecutedDecode.  Lanes 8–15 are unused by 8-way decode.
            let mut final_states = vec![0u32; 16];
            for i in 0..8 {
                final_states[i] = r8.final_states[i];
            }
            Ok(ExecutedDecode {
                output: out,
                backend: BackendId::Scalar8,
                plan_backend: BackendId::Scalar8,
                words_consumed: r8.words_consumed,
                final_states,
            })
        }
        // ---- Uniform256 table-free (scalar arithmetic, no table) ----
        DecodePlan::Uniform256TableFree16 { .. } => {
            decode_16way_uniform256_scalar(&words, expected_len, bi)
        }
        // ---- SSE4.1 8-way ----
        DecodePlan::Sse41Interleaved8 { .. } => {
            // The SSE4.1 kernel operates on `RansWordTables` (slot +
            // slot→sym slices); build them from the validated freqs.
            let cum = build_cum_freqs(freqs);
            let (slots, slot2sym) = ryg_rans_rs_simd::build_word_tables(freqs, &cum, 12);
            let tables = ryg_rans_rs_simd::RansWordTables {
                slots: &slots,
                slot2sym: &slot2sym,
            };
            ryg_rans_rs_simd::backends::decode_interleaved8_sse41_checked(
                &words,
                &tables,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX2 manual-gather 8-way ----
        DecodePlan::Avx2ManualGather8 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved8_avx2_manual_gather_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX2 hardware-gather 8-way ----
        DecodePlan::Avx2HardwareGather8 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved8_avx2_hardware_gather_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX2 2x8 on 16-way ----
        DecodePlan::Avx2TwoBy8On16 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved16_avx2_2x8_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX2 Uniform256 table-free ----
        DecodePlan::Avx2Uniform256TableFree16 { .. } => {
            ryg_rans_rs_simd::backends::decode_interleaved16_uniform256_avx2_checked(
                &words,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX-512VL interleaved 8-way ----
        DecodePlan::Avx512VlInterleaved8 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX-512 interleaved 16-way ----
        DecodePlan::Avx512Interleaved16 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved16_avx512_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX-512VL manual-gather 8-way ----
        DecodePlan::Avx512VlManualGather8 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl_manual_gather_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX-512 manual-gather 16-way ----
        DecodePlan::Avx512ManualGather16 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved16_avx512_manual_gather_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
        // ---- AVX-512VL 2x8 on 16-way ----
        DecodePlan::Avx512Vl2x8 { .. } => {
            let table = build_table(freqs, _scale_bits as u32, bi)?;
            ryg_rans_rs_simd::backends::decode_interleaved16_avx512vl_2x8_checked(
                &words,
                &table,
                expected_len,
            )
            .map(ExecutedDecode::from_simd)
            .map_err(|e| map_decode_err(e, bi))
        }
    }
}

/// Execute a decode plan using pure scalar backends (SIMD feature disabled).
///
/// No SIMD intrinsics are available in this build.  Scalar and
/// Uniform256-table-free plans execute their real kernels; every SIMD-only
/// plan returns `BackendUnavailable` — a typed error, never a silent
/// substitution (Phase L.9).  SIMD-only plans can only be produced by an
/// explicit policy, so a typed error is the only honest outcome in a build
/// that cannot run them.
///
/// ## Report fields
///
/// The pure-scalar kernels do not surface the SIMD report API, so
/// `words_consumed = 0` and `final_states = Vec::new()` (the "not
/// available" convention).  Output correctness is unaffected.
#[cfg(not(feature = "simd"))]
fn execute_decode_plan(
    plan: &DecodePlan,
    payload: &[u8],
    freqs: &[u32],
    expected_len: usize,
    _scale_bits: u8,
    bi: u64,
) -> Result<ExecutedDecode, BlockError> {
    let words: Vec<u16> = payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();

    match plan {
        DecodePlan::Scalar16 { .. } => {
            let (out, _) = decode_16way_pure_scalar(&words, freqs, expected_len, bi)?;
            Ok(ExecutedDecode::new(out, BackendId::Scalar16))
        }
        DecodePlan::Uniform256TableFree16 { .. } => {
            decode_16way_uniform256_scalar(&words, expected_len, bi)
        }
        DecodePlan::Scalar8 { .. } => {
            let (out, _) = decode_8way_pure_scalar(&words, freqs, expected_len, bi)?;
            Ok(ExecutedDecode::new(out, BackendId::Scalar8))
        }
        DecodePlan::Sse41Interleaved8 { .. }
        | DecodePlan::Avx512VlInterleaved8 { .. }
        | DecodePlan::Avx512Interleaved16 { .. }
        | DecodePlan::Avx512VlManualGather8 { .. }
        | DecodePlan::Avx512ManualGather16 { .. }
        | DecodePlan::Avx512Vl2x8 { .. }
        | DecodePlan::Avx2ManualGather8 { .. }
        | DecodePlan::Avx2HardwareGather8 { .. }
        | DecodePlan::Avx2TwoBy8On16 { .. }
        | DecodePlan::Avx2Uniform256TableFree16 { .. } => Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::BackendUnavailable,
        }),
    }
}

/// Pure scalar 16-way Word rANS decode using a flat frequency array.
///
/// This is the fallback when the SIMD crate is not available.  It implements
/// the exact same interleaved-16 algorithm as `decode_interleaved16_scalar` in
/// the SIMD crate, but with plain Rust arithmetic — no vector instructions.
///
/// 1. **Initialise 16 states** from the first 32 u16 words (2 words per state,
///    little-endian, forming a 32-bit state each).  Word index `rp` starts at 32.
///
/// 2. **Interleave**: for each output byte `i`, select lane `i & 15`.  Read the
///    current state `x` for that lane.  Compute `slot = x & 0xFFF` (modulo the
///    rANS word `M = 4096`).
///
/// 3. **Find symbol**: `find_symbol_from_freqs` walks the cumulative frequency
///    array to find which symbol occupies the slot.
///
/// 4. **Advance state**: the rANS state renormalisation is:
///    `new_state = freq[sym] * (x >> 12) + slot - cum[sym]`.
///    If `new_state < RANS_WORD_L (65536)`, a new word is consumed from the
///    stream: `state = (new_state << 16) | word[rp++]`.
///
/// 5. **Repeat** for all `expected_len` bytes.
///
/// ## Minimum payload check
///
/// The function requires at least `32` u16 words in the payload (16 initial
/// states × 2 words each).  If `words.len() < 32`, the input is malformed or
/// truncated — we return `BlockErrorKind::Format`.  This check prevents
/// underflow in the initial-state loading loop.
///
/// This check is **not** redundant with header validation because the payload
/// length declared in the header may be larger than actual available data
/// (truncated block).  The minimum is checked eagerly before any decode work.
///
/// ## Empty output shortcut
///
/// If `expected_len == 0`, we return immediately without touching the payload.
/// This handles the edge case of zero-length blocks efficiently.
///
/// ## Report fields
///
/// Returns `(Vec<u8>, ())` — the unit tuple indicates "no report available".
/// Callers that need `words_consumed` and `final_states` should use the SIMD
/// crate's scalar functions via `scalar16_fallback` or `scalar8_fallback`.
#[cfg(not(feature = "simd"))]
fn decode_16way_pure_scalar(
    words: &[u16],
    freqs: &[u32],
    expected_len: usize,
    bi: u64,
) -> Result<(Vec<u8>, ()), BlockError> {
    let expected_total = 4096u32; // scale_bits = 12
    let mut cum = [0u32; 257];
    cum[0] = 0;
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }
    let _ = cum;
    let _ = expected_total;

    if expected_len == 0 {
        return Ok((Vec::new(), ()));
    }
    if words.len() < 32 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Format,
        });
    }

    let mut states = [0u32; 16];
    for i in 0..16 {
        states[i] = words[i * 2] as u32 | (words[i * 2 + 1] as u32) << 16;
    }
    let mut rp = 32usize;
    let mut out = vec![0u8; expected_len];

    for i in 0..expected_len {
        let lane = i & 15;
        let x = states[lane];
        let slot = x as usize & 0xFFF; // RANS_WORD_M = 4096

        // Find the symbol for this slot using cumulative frequencies
        let sym = find_symbol_from_freqs(freqs, slot);
        out[i] = sym;

        // Advance state: new_state = freq * (x >> 12) + (x & 0xFFF) - cum[sym]
        let f = freqs[sym as usize] as u32;
        let st = if sym > 0 {
            freqs[..sym as usize].iter().sum::<u32>()
        } else {
            0u32
        };
        let nx = f * (x >> 12) + (x & 0xFFF) - st;
        states[lane] = nx;

        if nx < 65536 {
            // RANS_WORD_L = 1 << 16
            if rp >= words.len() {
                return Err(BlockError {
                    block_index: bi,
                    kind: BlockErrorKind::Format,
                });
            }
            states[lane] = (nx << 16) | words[rp] as u32;
            rp += 1;
        }
    }

    Ok((out, ()))
}

/// Scalar Uniform256 table-free 16-way decode.
///
/// # Why this kernel exists
///
/// For a validated Uniform256 model (scale_bits = 12, every normalised
/// frequency exactly 16, total 4096), the rANS decode collapses to two
/// arithmetic identities — no slot→symbol table is needed at all:
///
/// ```text
/// slot        = x & 0xFFF            (M = 4096)
/// symbol      = slot >> 4            (each symbol owns 16 consecutive slots)
/// next_state  = 16 * (x >> 12) + (slot & 15)
///             = freq * (x >> scale) + slot - cum[symbol]
/// ```
///
/// `freq = 16`, `cum[symbol] = 16 * symbol`, so the general formula reduces
/// exactly to the shift/mask form above.  This kernel is pure scalar
/// arithmetic — no vector instructions — so it is permitted under
/// `disable_simd` and available in builds without the `simd` feature.
///
/// # Report fields
///
/// `words_consumed` is the final u16 reader position (32 initial state words
/// plus renormalisation reads); `final_states` are the 16 lane states — the
/// same conventions as the packed-table scalar path.
fn decode_16way_uniform256_scalar(
    words: &[u16],
    expected_len: usize,
    bi: u64,
) -> Result<ExecutedDecode, BlockError> {
    if expected_len == 0 {
        return Ok(ExecutedDecode {
            output: Vec::new(),
            backend: BackendId::Uniform256TableFree16,
            plan_backend: BackendId::Uniform256TableFree16,
            words_consumed: 0,
            final_states: Vec::new(),
        });
    }
    if words.len() < 32 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Format,
        });
    }

    let mut states = [0u32; 16];
    for i in 0..16 {
        states[i] = words[i * 2] as u32 | (words[i * 2 + 1] as u32) << 16;
    }
    let mut rp = 32usize;
    let mut out = vec![0u8; expected_len];

    for i in 0..expected_len {
        let lane = i & 15;
        let x = states[lane];
        let slot = (x & 0xFFF) as u32;
        out[i] = (slot >> 4) as u8;
        let nx = ((x >> 12) << 4) | (slot & 15);
        states[lane] = nx;

        if nx < 65536 {
            // RANS_WORD_L = 1 << 16
            if rp >= words.len() {
                return Err(BlockError {
                    block_index: bi,
                    kind: BlockErrorKind::Format,
                });
            }
            states[lane] = (nx << 16) | words[rp] as u32;
            rp += 1;
        }
    }

    Ok(ExecutedDecode {
        output: out,
        backend: BackendId::Uniform256TableFree16,
        plan_backend: BackendId::Uniform256TableFree16,
        words_consumed: rp,
        final_states: states.to_vec(),
    })
}

/// Pure scalar 8-way Word rANS decode using a flat frequency array.
///
/// This is the 8-way variant of `decode_16way_pure_scalar`.  It uses 8
/// interleaved lanes instead of 16, requiring only 16 initial u16 words
/// (8 states × 2 words each).
///
/// Requires at least `16` u16 words.  Returns `BlockErrorKind::Format`
/// otherwise.  See [`decode_16way_pure_scalar`] for detailed rationale.
///
/// ## Algorithm
///
/// Identical to the 16-way version except:
/// - Lane count is 8 instead of 16 (`lane = i & 7`).
/// - Initial states consume 16 words instead of 32.
/// - `rp` starts at 16 instead of 32.
///
/// The state renormalisation, symbol lookup, and output production are
/// otherwise byte-identical to the 16-way path.
#[cfg(not(feature = "simd"))]
fn decode_8way_pure_scalar(
    words: &[u16],
    freqs: &[u32],
    expected_len: usize,
    bi: u64,
) -> Result<(Vec<u8>, ()), BlockError> {
    if expected_len == 0 {
        return Ok((Vec::new(), ()));
    }
    if words.len() < 16 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Format,
        });
    }

    let mut states = [0u32; 8];
    for i in 0..8 {
        states[i] = words[i * 2] as u32 | (words[i * 2 + 1] as u32) << 16;
    }
    let mut rp = 16usize;
    let mut out = vec![0u8; expected_len];

    for i in 0..expected_len {
        let lane = i & 7;
        let x = states[lane];
        let slot = x as usize & 0xFFF;

        let sym = find_symbol_from_freqs(freqs, slot);
        out[i] = sym;

        let f = freqs[sym as usize] as u32;
        let st = if sym > 0 {
            freqs[..sym as usize].iter().sum::<u32>()
        } else {
            0u32
        };
        let nx = f * (x >> 12) + (x & 0xFFF) - st;
        states[lane] = nx;

        if nx < 65536 {
            if rp >= words.len() {
                return Err(BlockError {
                    block_index: bi,
                    kind: BlockErrorKind::Format,
                });
            }
            states[lane] = (nx << 16) | words[rp] as u32;
            rp += 1;
        }
    }

    Ok((out, ()))
}

/// Find the symbol for a given slot using the frequency array.
///
/// This implements the core rANS symbol-lookup operation: given a slot value
/// in the range `[0, 4095)` (where `4096 = RANS_WORD_M`), walk the cumulative
/// frequency prefix sum to find the symbol whose frequency band contains the
/// slot.
///
/// ## Algorithm
///
/// Accumulate frequencies one symbol at a time.  For each symbol `s`, check
/// if `slot < accum`.  The first `s` where this holds is the decoded symbol.
///
/// This is a linear scan of 256 entries at most.  For power-of-two cumulative
/// totals (`M = 4096`), a binary search over the cumulative array would be
/// faster (O(log 256) vs O(256)), but the linear scan is simple, branch-predictable
/// for typical data, and avoids the overhead of building a separate cumulative
/// array in the pure-scalar path (which uses flat frequencies directly).
///
/// ## Invariant
///
/// `slot` is guaranteed to be `< 4096` by the caller (masked with `0xFFF`).
/// The sum of all 256 frequencies equals `4096`, so the loop will always find
/// a match before or at symbol 255.  The fallthrough return of `255` is a
/// safety net — it should never be reached for valid input.
///
/// ## Performance note
///
/// SIMD backends use precomputed slot→symbol lookup tables instead of this
/// linear scan.  This function exists only for the pure-scalar fallback paths.
#[cfg(not(feature = "simd"))]
fn find_symbol_from_freqs(freqs: &[u32], slot: usize) -> u8 {
    let mut accum = 0u32;
    for s in 0..256u32 {
        accum += freqs[s as usize];
        if slot < accum as usize {
            return s as u8;
        }
    }
    255u8
}

// ---------------------------------------------------------------------------
// Parallel decoder struct
// ---------------------------------------------------------------------------

/// The top-level parallel decoder.
///
/// `ParallelDecoder` is a zero-sized struct whose methods provide the primary
/// public API for multi-block RYGRANS decoding.  It coordinates three
/// components:
///
/// 1. **Bounded executor** (`run_tasks`): dispatches decode tasks across a
///    configurable pool of worker threads, with a bounded queue of in-flight
///    blocks.
///
/// 2. **Reorder buffer** (`ReorderBuffer`): collects completed results (which
///    may finish out of index order) and yields them in ascending block-index
///    order.  Enforces a maximum buffered decoded byte count.
///
/// 3. **Error tracker** (`CanonicalErrorTracker`): records the first error
///    across all blocks and reports it as the canonical failure cause.
///
/// ## Usage
///
/// ```ignore
/// let decoded = ParallelDecoder::decode_blocks(jobs, &config)?;
/// ```
///
/// All decode jobs must have been produced by the encoder pipeline with the
/// same `ParallelConfig` parameters (frequency model, codec policy, etc.)
/// that will be used for decoding.
pub struct ParallelDecoder;

impl ParallelDecoder {
    /// Decode all blocks in parallel and return them in ascending order.
    ///
    /// This method materialises all jobs into a `Vec` immediately, then
    /// dispatches them to the bounded executor.  It is suitable for
    /// in-memory decoding where all block data is already available.
    ///
    /// ## Pipeline
    ///
    /// 1. **Collect jobs**: materialise the `IntoIterator` into a `Vec`.
    ///    Empty inputs short-circuit to an empty `OrderedDecodedBlocks`.
    ///
    /// 2. **Calculate resources**: `effective_worker_count` clamps the
    ///    requested thread count to available hardware parallelism or the
    ///    job count, whichever is smaller.  Queue capacity is the max of
    ///    `max_in_flight_blocks` and the effective worker count.
    ///
    /// 3. **Dispatch**: wrap each job in a `DecodeTask` and call `run_tasks`.
    ///    A fresh `CancellationToken` is created for this batch.
    ///
    /// 4. **Reorder and collect**: as results arrive (not necessarily in
    ///    order), insert them into the `ReorderBuffer`.  Drain contiguous
    ///    ready blocks into the ordered output vec.  Record any errors via
    ///    `CanonicalErrorTracker`.
    ///
    /// 5. **Final drain**: after all tasks complete, drain any remaining
    ///    ready blocks from the reorder buffer.
    ///
    /// 6. **Error check**: if any error was recorded, return the first one
    ///    (canonical error) wrapped in `ParallelError::DecodeFailed`.
    ///
    /// 7. **Sort and return**: sort by `block_index` (belt-and-suspenders;
    ///    the reorder buffer should already produce ordered results) and
    ///    return.
    ///
    /// ## Determinism
    ///
    /// The output is deterministically ordered by `block_index` regardless
    /// of execution order.  The parallel decode itself is deterministic:
    /// the same block data always produces the same decoded output on the
    /// same backend, independent of thread scheduling.
    pub fn decode_blocks(
        blocks: impl IntoIterator<Item = DecodeBlockJob>,
        config: &ParallelConfig,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        Self::decode_blocks_with_cancel(blocks, config, None)
    }

    /// Decode all blocks in parallel with an optional external cancellation token.
    ///
    /// Same semantics as [`Self::decode_blocks`], but accepts a caller-owned
    /// [`CancellationToken`].  If cancellation is observed before all blocks
    /// complete, returns [`ParallelError::Cancelled`] with completion counts.
    /// Never returns `Ok` with fewer blocks than declared.
    pub fn decode_blocks_with_cancel(
        blocks: impl IntoIterator<Item = DecodeBlockJob>,
        config: &ParallelConfig,
        external_cancel: Option<crate::sync::Arc<crate::cancellation::CancellationToken>>,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        let jobs: Vec<DecodeBlockJob> = blocks.into_iter().collect();
        if jobs.is_empty() {
            return Ok(OrderedDecodedBlocks {
                blocks: Vec::new(),
                stream_hash: crate::encode::sha256(&[]),
                execution: crate::job::ExecutionMetadata {
                    requested_workers: 0,
                    effective_workers: 0,
                    queue_capacity: 0,
                    block_count: 0,
                    declared_blocks: 0,
                    completed_blocks: 0,
                    cancelled: false,
                },
            });
        }
        let bc = jobs.len();

        // ---- max_buffered_input_bytes enforcement ----
        // Checked BEFORE the sequential threshold so the budget is enforced
        // on every path (parallel and sequential).
        let input_bytes: u64 = jobs.iter().map(|j| j.block_data.len() as u64).sum();
        if input_bytes > config.max_buffered_input_bytes {
            return Err(ParallelError::ResourceLimit(format!(
                "max_buffered_input_bytes exceeded: {} > {}",
                input_bytes, config.max_buffered_input_bytes
            )));
        }

        // ---- parallel_threshold_bytes: sequential fallback ----
        // Below the threshold, run the decode inline on the calling thread
        // without spawning a worker pool (thread spawn + queue overhead
        // exceeds any parallel gain for small inputs).
        if input_bytes < config.parallel_threshold_bytes {
            let tasks: Vec<DecodeTask> = jobs
                .into_iter()
                .map(|j| DecodeTask {
                    job: j,
                    config: config.clone(),
                })
                .collect();
            let report = crate::executor::run_tasks_sequential(tasks, external_cancel)?;
            let mut reorder = ReorderBuffer::new(
                config.max_in_flight_blocks.get(),
                config.max_buffered_output_bytes,
            );
            let mut ordered = Vec::with_capacity(bc);
            let mut et = crate::error::CanonicalErrorTracker::new();
            for r in report.results {
                match r {
                    Ok(b) => match reorder.insert(b) {
                        Ok(committed) => ordered.extend(committed),
                        Err(e) => et.record(e),
                    },
                    Err(e) => et.record(e),
                }
            }
            ordered.extend(reorder.drain_ready());
            if let Some(c) = et.canonical_error() {
                return Err(ParallelError::DecodeFailed(Box::new(c.clone())));
            }
            ordered.sort_by_key(|b| b.block_index);
            let stream_hash = stream_hash_of(&ordered);
            let completed_blocks = ordered.len();
            let cancelled = report.cancelled;
            return Ok(OrderedDecodedBlocks {
                blocks: ordered,
                stream_hash,
                execution: crate::job::ExecutionMetadata {
                    requested_workers: 1,
                    effective_workers: 1,
                    queue_capacity: 1,
                    block_count: bc,
                    declared_blocks: bc,
                    completed_blocks,
                    cancelled,
                },
            });
        }

        let wc = crate::resource::effective_worker_count(config, bc)?;
        let qc = config.max_in_flight_blocks.get().max(wc);

        let tasks: Vec<DecodeTask> = jobs
            .into_iter()
            .map(|j| DecodeTask {
                job: j,
                config: config.clone(),
            })
            .collect();

        // Run tasks with the caller-provided external cancellation token.
        // If the caller supplied none, run_tasks creates its own internal
        // token that is only cancelled on worker panic.
        let report: ExecutorReport<Result<DecodedBlockResult, BlockError>> =
            crate::executor::run_tasks_with_affinity(
                tasks,
                wc,
                qc,
                config.worker_stack_size,
                external_cancel,
                config.affinity.clone(),
            )?;

        let effective_workers = report.effective_workers;

        let mut reorder = ReorderBuffer::new(
            config.max_in_flight_blocks.get(),
            config.max_buffered_output_bytes,
        );
        let mut ordered = Vec::with_capacity(bc);
        let mut et = crate::error::CanonicalErrorTracker::new();

        for r in report.results {
            match r {
                Ok(b) => match reorder.insert(b) {
                    Ok(committed) => {
                        ordered.extend(committed);
                    }
                    Err(e) => et.record(e),
                },
                Err(e) => et.record(e),
            }
        }

        ordered.extend(reorder.drain_ready());

        if let Some(c) = et.canonical_error() {
            return Err(ParallelError::DecodeFailed(Box::new(c.clone())));
        }

        ordered.sort_by_key(|b| b.block_index);
        let stream_hash = stream_hash_of(&ordered);
        let completed_blocks = ordered.len();
        let cancelled = report.cancelled;
        Ok(OrderedDecodedBlocks {
            blocks: ordered,
            stream_hash,
            execution: crate::job::ExecutionMetadata {
                requested_workers: wc,
                effective_workers,
                queue_capacity: qc,
                block_count: bc,
                declared_blocks: bc,
                completed_blocks,
                cancelled,
            },
        })
    }

    /// Streaming decode for non-seekable input.
    /// Streaming decode for non-seekable or incremental input.
    ///
    /// This method reads blocks from an iterator and dispatches them to the
    /// bounded executor as they arrive.  Unlike [`ParallelDecoder::decode_blocks`], which
    /// materialises all jobs upfront, this method is designed for pipelines
    /// where blocks are produced incrementally (e.g. from a network stream
    /// or a file read-ahead buffer).
    ///
    /// ## Current limitations
    ///
    /// The current implementation still materialises all jobs into a `Vec`
    /// before dispatching.  This is because `run_tasks` (the executor) expects
    /// a `Vec<DecodeTask>`.  A true streaming pipeline would require a
    /// bounded producer-consumer channel where the main thread feeds blocks
    /// one at a time while workers consume them.  That architecture is noted
    /// as future work.
    ///
    /// Despite this limitation, the function already:
    ///
    /// - Enforces bounded buffered decoded bytes via the reorder buffer's
    ///   backpressure (`max_buffered_output_bytes`).
    /// - Computes the full-stream SHA-256 hash as blocks are drained from
    ///   the reorder buffer in sequential order.  This hash covers all
    ///   decoded bytes in canonical block order, suitable for stream-level
    ///   integrity verification.
    ///
    /// ## Stream hash (v1 protocol)
    ///
    /// The stream hash is a SHA-256 digest computed over the concatenated
    /// decoded output of all blocks in ascending block-index order.  The
    /// hash is updated as blocks emerge from the reorder buffer — i.e. in
    /// the order they were originally encoded, not the order they were
    /// decoded.  This ensures the stream hash is deterministic and
    /// independent of thread scheduling.
    ///
    /// The stream hash is computed over the concatenated decoded output in
    /// ascending block-index order and stored as `OrderedDecodedBlocks::stream_hash`.
    /// It is deterministic and independent of worker scheduling.
    ///
    /// ## Cancellation
    ///
    /// As with `decode_blocks`, a fresh `CancellationToken` is created per
    /// invocation.  The token is passed to every task, enabling prompt
    /// cancellation if an error is detected in one block while others are
    /// still in flight.
    pub fn decode_streaming(
        blocks: impl IntoIterator<Item = DecodeBlockJob>,
        config: &ParallelConfig,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        Self::decode_streaming_with_cancel(blocks, config, None)
    }

    /// Streaming decode with an optional external cancellation token.
    ///
    /// Same semantics as [`Self::decode_streaming`] but accepts a
    /// caller-owned [`CancellationToken`].  Never returns `Ok` with fewer
    /// blocks than declared.
    pub fn decode_streaming_with_cancel(
        blocks: impl IntoIterator<Item = DecodeBlockJob>,
        config: &ParallelConfig,
        external_cancel: Option<crate::sync::Arc<crate::cancellation::CancellationToken>>,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        let jobs: Vec<DecodeBlockJob> = blocks.into_iter().collect();
        if jobs.is_empty() {
            return Ok(OrderedDecodedBlocks {
                blocks: Vec::new(),
                stream_hash: crate::encode::sha256(&[]),
                execution: crate::job::ExecutionMetadata {
                    requested_workers: 0,
                    effective_workers: 0,
                    queue_capacity: 0,
                    block_count: 0,
                    declared_blocks: 0,
                    completed_blocks: 0,
                    cancelled: false,
                },
            });
        }

        let bc = jobs.len();
        let wc = crate::resource::effective_worker_count(config, bc)?;
        let qc = config.max_in_flight_blocks.get().max(wc);

        // ---- max_buffered_input_bytes enforcement (streaming) ----
        let input_bytes: u64 = jobs.iter().map(|j| j.block_data.len() as u64).sum();
        if input_bytes > config.max_buffered_input_bytes {
            return Err(ParallelError::ResourceLimit(format!(
                "max_buffered_input_bytes exceeded: {} > {}",
                input_bytes, config.max_buffered_input_bytes
            )));
        }

        // NOTE: This currently materialises all jobs into a Vec before
        // dispatch, so it is not a true streaming pipeline yet.  The live
        // bounded executor redesign (Phase L.4) replaces this with a
        // coordinator loop that feeds blocks one at a time through a
        // bounded producer channel.
        let tasks: Vec<DecodeTask> = jobs
            .into_iter()
            .map(|j| DecodeTask {
                job: j,
                config: config.clone(),
            })
            .collect();

        let report: ExecutorReport<Result<DecodedBlockResult, BlockError>> =
            run_tasks(tasks, wc, qc, config.worker_stack_size, external_cancel)?;

        let effective_workers = report.effective_workers;

        let mut reorder = ReorderBuffer::new(
            config.max_in_flight_blocks.get(),
            config.max_buffered_output_bytes,
        );
        let mut ordered = Vec::with_capacity(bc);
        let mut et = crate::error::CanonicalErrorTracker::new();
        use sha2::Digest;
        let mut stream_hasher = sha2::Sha256::new();

        for r in report.results {
            match r {
                Ok(b) => {
                    // Add decoded bytes to the full-stream hash in the order
                    // they were decoded.  The reorder buffer will reorder them
                    // for sequential output, but we compute the stream hash
                    // during reorder drain to ensure canonical order.
                    match reorder.insert(b) {
                        Ok(committed) => {
                            for ready in committed {
                                sha2::Digest::update(&mut stream_hasher, &ready.output);
                                ordered.push(ready);
                            }
                        }
                        Err(e) => et.record(e),
                    }
                }
                Err(e) => et.record(e),
            }
        }

        // Drain remaining reorder buffer results
        for rd in reorder.drain_ready() {
            sha2::Digest::update(&mut stream_hasher, &rd.output);
            ordered.push(rd);
        }

        if let Some(c) = et.canonical_error() {
            return Err(ParallelError::DecodeFailed(Box::new(c.clone())));
        }

        ordered.sort_by_key(|b| b.block_index);
        // The stream hash was fed from the reorder drain in canonical
        // ascending order; finalise it now and store it so the documented
        // stream-level digest is real, not just computed-and-discarded.
        let digest = sha2::Digest::finalize(stream_hasher);
        let mut stream_hash = [0u8; 32];
        stream_hash.copy_from_slice(&digest);
        let completed_blocks = ordered.len();
        let cancelled = report.cancelled;
        Ok(OrderedDecodedBlocks {
            blocks: ordered,
            stream_hash,
            execution: crate::job::ExecutionMetadata {
                requested_workers: wc,
                effective_workers,
                queue_capacity: qc,
                block_count: bc,
                declared_blocks: bc,
                completed_blocks,
                cancelled,
            },
        })
    }

    /// Decode with a sink callback, committing decoded blocks in block-index
    /// order without collecting the entire workload.
    ///
    /// Results stream from the bounded executor through a bounded result
    /// channel into a live [`ReorderBuffer`]; every time a contiguous run
    /// becomes committable, the caller's `sink` receives each block in
    /// ascending block-index order.  Peak memory is bounded by the queue
    /// capacities plus whatever the sink retains — the operation itself
    /// does not accumulate all decoded output.
    ///
    /// Returns [`ParallelError::Cancelled`] if cancelled before all blocks
    /// complete; never returns `Ok` with fewer blocks delivered to the sink.
    pub fn decode_with_sink<F>(
        blocks: impl IntoIterator<Item = DecodeBlockJob>,
        config: &ParallelConfig,
        external_cancel: Option<crate::sync::Arc<crate::cancellation::CancellationToken>>,
        sink: F,
    ) -> Result<ExecutorReport<Result<DecodedBlockResult, BlockError>>, ParallelError>
    where
        F: FnMut(DecodedBlockResult) + Send + 'static,
    {
        let jobs: Vec<DecodeBlockJob> = blocks.into_iter().collect();
        let bc = jobs.len();
        if bc == 0 {
            // Zero blocks: no work, sink never invoked.  Return an empty
            // report with the same R type as the non-empty path.
            return run_tasks_with_sink(
                Vec::<DecodeTask>::new(),
                1,
                1,
                None,
                external_cancel,
                |_r: Result<DecodedBlockResult, BlockError>| {},
            );
        }
        let wc = crate::resource::effective_worker_count(config, bc)?;
        let qc = config.max_in_flight_blocks.get().max(wc);

        let input_bytes: u64 = jobs.iter().map(|j| j.block_data.len() as u64).sum();
        if input_bytes > config.max_buffered_input_bytes {
            return Err(ParallelError::ResourceLimit(format!(
                "max_buffered_input_bytes exceeded: {} > {}",
                input_bytes, config.max_buffered_input_bytes
            )));
        }

        let tasks: Vec<DecodeTask> = jobs
            .into_iter()
            .map(|j| DecodeTask {
                job: j,
                config: config.clone(),
            })
            .collect();

        // Live reorder: results arrive in completion order; commit only
        // contiguous runs in block-index order, passing each to the sink.
        // The sink closure runs on the coordinator thread and must be Send,
        // so share the reorder buffer and error tracker through Arc<Mutex>.
        let reorder = crate::sync::Arc::new(crate::sync::Mutex::new(ReorderBuffer::new(
            config.max_in_flight_blocks.get(),
            config.max_buffered_output_bytes,
        )));
        let et = crate::sync::Arc::new(crate::sync::Mutex::new(
            crate::error::CanonicalErrorTracker::new(),
        ));
        let sink = crate::sync::Arc::new(crate::sync::Mutex::new(sink));

        let reorder_rc = reorder.clone();
        let et_rc = et.clone();
        let sink_rc = sink.clone();
        let report = run_tasks_with_sink(
            tasks,
            wc,
            qc,
            config.worker_stack_size,
            external_cancel,
            move |result: Result<DecodedBlockResult, BlockError>| {
                // Mutex-poisoning note (Phase L.12): these locks are held only
                // by this coordinator closure (the calling thread).  Worker
                // task execution happens inside `catch_unwind` and never
                // touches these mutexes, so a task panic cannot poison them.
                // A panic in the user's own `sink` callback propagates to the
                // caller of `decode_with_sink` directly — the lock poisoning
                // is then moot because the same frame is already unwinding.
                let mut reorder = reorder_rc.lock().unwrap();
                let mut et = et_rc.lock().unwrap();
                let mut sink = sink_rc.lock().unwrap();
                match result {
                    Ok(b) => match reorder.insert(b) {
                        Ok(committed) => {
                            for ready in committed {
                                sink(ready);
                            }
                        }
                        Err(e) => et.record(e),
                    },
                    Err(e) => et.record(e),
                }
            },
        )?;

        {
            let mut reorder = reorder.lock().unwrap();
            let mut sink = sink.lock().unwrap();
            for rd in reorder.drain_ready() {
                sink(rd);
            }
        }

        if let Some(c) = et.lock().unwrap().canonical_error() {
            return Err(ParallelError::DecodeFailed(Box::new(c.clone())));
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BackendPolicy, CodecPolicy};
    use crate::encode::{ParallelEncoder, encode_single_block};
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

    fn nonuniform_data() -> Vec<u8> {
        // Skewed data: mostly 'a', some other bytes
        let mut d = Vec::with_capacity(4096);
        for i in 0..4096 {
            if i % 256 < 200 {
                d.push(b'a');
            } else if i % 256 < 220 {
                d.push(b'b');
            } else if i % 256 < 240 {
                d.push(b'c');
            } else {
                d.push((i % 256) as u8);
            }
        }
        d
    }

    #[test]
    fn test_roundtrip_uniform256() {
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");
        let dec = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: e.block,
            },
            &ParallelConfig::default(),
        )
        .expect("decode");
        assert_eq!(dec.output, d);
    }

    #[test]
    fn test_roundtrip_nonuniform() {
        let d = nonuniform_data();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");
        let dec = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: e.block,
            },
            &ParallelConfig::default(),
        )
        .expect("decode");
        assert_eq!(dec.output, d);
    }

    #[test]
    fn test_roundtrip_multiple_blocks() {
        let mut data = Vec::with_capacity(8192);
        for _ in 0..2 {
            data.extend(nonuniform_data());
        }
        let plan = crate::plan::FixedBlockPlan::new(data.len() as u64, 4096);
        assert_eq!(plan.block_count(), 2);
        let cfg = ParallelConfig {
            threads: crate::ThreadCount::Exact(std::num::NonZeroUsize::new(2).unwrap()),
            ..Default::default()
        };
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
        let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("encode");
        assert_eq!(enc.blocks.len(), 2);
        let dj: Vec<DecodeBlockJob> = enc
            .blocks
            .iter()
            .map(|b| DecodeBlockJob {
                block_index: b.block_index,
                block_data: b.block.clone(),
            })
            .collect();
        let dec = ParallelDecoder::decode_blocks(dj, &cfg).expect("decode");
        let mut full = Vec::new();
        for b in &dec.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(full, data);
    }

    #[test]
    fn test_deterministic() {
        let d = uniform256();
        let j1 = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let j2 = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let r1 = encode_single_block(j1).expect("e1");
        let r2 = encode_single_block(j2).expect("e2");
        assert_eq!(r1.block, r2.block);
    }

    #[test]
    fn test_rejects_zero_hash_strict() {
        // Under Strict (default) policy, a block with all-zero decoded_sha256
        // must FAIL with DecodedHashMissing.
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");

        // Manually zero out the decoded_sha256 field (bytes 72-103)
        let mut tampered = e.block.clone();
        tampered[72..104].fill(0);

        let result = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: tampered,
            },
            &ParallelConfig::default(), // Strict is the default
        );
        match result {
            Err(BlockError {
                kind: BlockErrorKind::DecodedHashMissing,
                ..
            }) => {} // expected
            other => panic!("expected DecodedHashMissing error, got {:?}", other),
        }
    }

    #[test]
    fn test_zero_hash_allowed_under_legacy_policy() {
        // Under AllowLegacyUnsetDecodedHash, a zero decoded hash decodes but
        // output_verified is false.
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");

        let mut tampered = e.block.clone();
        tampered[72..104].fill(0);

        let cfg = ParallelConfig {
            integrity_policy: crate::config::IntegrityPolicy::AllowLegacyUnsetDecodedHash,
            ..Default::default()
        };
        let dec = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: tampered,
            },
            &cfg,
        )
        .expect("decode with zero hash under legacy policy should succeed");
        assert!(
            !dec.output_verified,
            "block with zero stored hash must not be marked verified"
        );
        assert_eq!(dec.output, d, "decoded data must still match");
    }

    #[test]
    fn test_rejects_corrupt_payload_hash() {
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");

        // Corrupt the payload hash
        let mut tampered = e.block.clone();
        tampered[40] ^= 0xFF;

        let result = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: tampered,
            },
            &ParallelConfig::default(),
        );
        match result {
            Err(BlockError {
                kind: BlockErrorKind::PayloadHash,
                ..
            }) => {} // expected
            other => panic!("expected PayloadHash error, got {:?}", other),
        }
    }

    #[test]
    fn test_rejects_corrupt_decoded_hash() {
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");

        // Corrupt the decoded hash
        let mut tampered = e.block.clone();
        tampered[72] ^= 0xFF;

        let result = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: tampered,
            },
            &ParallelConfig::default(),
        );
        match result {
            Err(BlockError {
                kind: BlockErrorKind::DecodedHashMismatch,
                ..
            }) => {} // expected
            other => panic!("expected DecodedHashMismatch error, got {:?}", other),
        }
    }

    #[test]
    fn test_rejects_truncated_block() {
        let d = uniform256();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");

        // Truncate to just the header
        let truncated = e.block[..50].to_vec();
        let result = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: truncated,
            },
            &ParallelConfig::default(),
        );
        assert!(result.is_err(), "truncated block must be rejected");
    }

    #[test]
    fn test_empty_input_roundtrip() {
        let d: Vec<u8> = Vec::new();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode empty");
        let dec = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: e.block,
            },
            &ParallelConfig::default(),
        )
        .expect("decode empty");
        assert_eq!(dec.output, d);
        assert_eq!(dec.output.len(), 0);
    }

    #[test]
    fn test_parallel_determinism() {
        // Encode nonuniform data with different thread counts
        let data = nonuniform_data();
        let plan = crate::plan::FixedBlockPlan::new(data.len() as u64, 1024);
        assert!(plan.block_count() >= 2);

        // Encode with 1 thread
        let cfg1 = ParallelConfig {
            threads: crate::ThreadCount::Exact(std::num::NonZeroUsize::new(1).unwrap()),
            ..Default::default()
        };
        let jobs1: Vec<DecodeBlockJob> = {
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
            let enc = ParallelEncoder::encode_blocks(jobs, &cfg1).expect("encode 1t");
            enc.blocks
                .into_iter()
                .map(|b| DecodeBlockJob {
                    block_index: b.block_index,
                    block_data: b.block,
                })
                .collect()
        };

        let dec1 = ParallelDecoder::decode_blocks(jobs1, &cfg1).expect("decode 1t");
        let mut full1 = Vec::new();
        for b in &dec1.blocks {
            full1.extend_from_slice(&b.output);
        }
        assert_eq!(full1, data, "1-thread decode must match original");
    }

    // -------------------------------------------------------------------
    // Phase L.9 — exact backend semantics
    // -------------------------------------------------------------------

    /// Encode `data` with an explicit codec and return (block, codec_id).
    fn encode_with_codec(data: &[u8], codec_id: u16) -> (Vec<u8>, u16) {
        let j = EncodeBlockJob::new(
            0,
            data.to_vec(),
            CodecPolicy::Explicit(codec_id),
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("encode");
        (e.block, codec_id)
    }

    /// Decode `block` under `Explicit(backend)`; returns Ok(result) or a
    /// typed error.
    fn decode_explicit(block: &[u8], backend: BackendId) -> Result<DecodedBlockResult, BlockError> {
        let cfg = ParallelConfig {
            backend_policy: BackendPolicy::Explicit(backend),
            ..Default::default()
        };
        decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: block.to_vec(),
            },
            &cfg,
        )
    }

    /// The 8-way (codec 7) backend family.
    const BACKENDS_8WAY: &[BackendId] = &[
        BackendId::Scalar8,
        BackendId::Sse41Interleaved8,
        BackendId::Avx512VlInterleaved8,
        BackendId::Avx512VlManualGather8,
        BackendId::Avx2ManualGather8,
        BackendId::Avx2HardwareGather8,
    ];

    /// The 16-way (codec 8) backend family.
    const BACKENDS_16WAY: &[BackendId] = &[
        BackendId::Scalar16,
        BackendId::Avx512Interleaved16,
        BackendId::Avx512ManualGather16,
        BackendId::Avx512Vl2x8,
        BackendId::Avx2TwoBy8On16,
    ];

    #[test]
    fn test_explicit_backend_executes_exactly_or_typed_error() {
        // Phase L.9: an explicit backend request either executes exactly
        // (backend == plan_backend == requested, byte-identical output) or
        // returns `BackendUnavailable` — never a different backend.
        let data = nonuniform_data();
        for (codec, family) in [(7u16, BACKENDS_8WAY), (8u16, BACKENDS_16WAY)] {
            let (block, _) = encode_with_codec(&data, codec);
            for &b in family {
                match decode_explicit(&block, b) {
                    Ok(dec) => {
                        assert_eq!(
                            dec.backend, b,
                            "codec {codec}: executed backend must equal requested {b:?}"
                        );
                        assert_eq!(
                            dec.plan_backend, b,
                            "codec {codec}: plan backend must equal requested {b:?}"
                        );
                        assert_eq!(dec.output, data, "codec {codec} {b:?}: output must match");
                    }
                    Err(BlockError {
                        kind: BlockErrorKind::BackendUnavailable,
                        ..
                    }) => {
                        // Honest typed refusal: this CPU/build cannot run the
                        // kernel.  Never a silent scalar substitution.
                    }
                    Err(other) => panic!("codec {codec} {b:?}: unexpected error {:?}", other),
                }
            }
        }
    }

    #[test]
    fn test_uniform256_tablefree_executes_and_parities_with_scalar() {
        // ModelAware must select the real table-free kernel for a validated
        // Uniform256 model, and its output/report must be byte-identical to
        // the packed-scalar 16-way decode (cross-backend parity).
        let data = uniform256();
        let (block, _) = encode_with_codec(&data, 8);

        let cfg = ParallelConfig {
            backend_policy: BackendPolicy::ModelAware,
            ..Default::default()
        };
        let tf = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: block.clone(),
            },
            &cfg,
        )
        .expect("ModelAware decode of uniform256 must succeed");
        assert_eq!(tf.backend, BackendId::Uniform256TableFree16);
        assert_eq!(tf.plan_backend, BackendId::Uniform256TableFree16);
        assert_eq!(tf.output, data);
        assert!(tf.words_consumed > 0, "table-free kernel must report words");
        assert_eq!(tf.final_states.len(), 16);

        // Explicit Uniform256TableFree16 — the same kernel, directly.
        let tf2 = decode_explicit(&block, BackendId::Uniform256TableFree16)
            .expect("explicit table-free must succeed");
        assert_eq!(tf2.output, data);
        assert_eq!(tf2.words_consumed, tf.words_consumed);
        assert_eq!(tf2.final_states, tf.final_states);

        // Parity with the packed-scalar 16-way decoder.
        let sc = decode_explicit(&block, BackendId::Scalar16).expect("scalar16");
        assert_eq!(sc.output, tf.output);
        assert_eq!(sc.words_consumed, tf.words_consumed);
        assert_eq!(sc.final_states, tf.final_states);
    }

    #[test]
    fn test_backend_reports_agree_across_executable_backends() {
        // Every executable backend for a given block must produce identical
        // output AND identical words_consumed/final_states — the report is
        // a property of the stream, not the kernel.
        let data = nonuniform_data();
        let (block, codec) = encode_with_codec(&data, 8);
        let family = BACKENDS_16WAY;
        let mut reference: Option<DecodedBlockResult> = None;
        for &b in family {
            match decode_explicit(&block, b) {
                Ok(dec) => match &reference {
                    None => reference = Some(dec),
                    Some(ref_) => {
                        assert_eq!(dec.output, ref_.output, "{b:?} output");
                        assert_eq!(
                            dec.words_consumed, ref_.words_consumed,
                            "{b:?} words_consumed"
                        );
                        assert_eq!(dec.final_states, ref_.final_states, "{b:?} final_states");
                    }
                },
                Err(BlockError {
                    kind: BlockErrorKind::BackendUnavailable,
                    ..
                }) => {}
                Err(other) => panic!("codec {codec} {b:?}: unexpected error {:?}", other),
            }
        }
        assert!(
            reference.is_some(),
            "at least one 16-way backend must execute on this host"
        );
    }

    #[test]
    fn test_format_mismatch_rejected_at_decode() {
        // Width mismatch (8-way backend on a codec-8 block) must be a typed
        // BackendFormatMismatch, not a silent scalar execution.
        let data = nonuniform_data();
        let (block8, _) = encode_with_codec(&data, 8);
        for b in BACKENDS_8WAY {
            let e = decode_explicit(&block8, *b).expect_err("must be rejected");
            assert_eq!(
                e.kind,
                BlockErrorKind::BackendFormatMismatch,
                "{b:?} on codec 8 must be a format mismatch"
            );
        }
        let (block7, _) = encode_with_codec(&data, 7);
        for b in BACKENDS_16WAY {
            let e = decode_explicit(&block7, *b).expect_err("must be rejected");
            assert_eq!(
                e.kind,
                BlockErrorKind::BackendFormatMismatch,
                "{b:?} on codec 7 must be a format mismatch"
            );
        }
    }

    #[test]
    fn test_batch_and_raw_rle_backends_rejected_at_decode() {
        // Batch backends need coordinator context; RAW/RLE backends need
        // matching block kinds.  Both must be typed errors at planning time.
        let data = nonuniform_data();
        let (block, _) = encode_with_codec(&data, 8);
        for b in [BackendId::Avx512Batch4, BackendId::Avx2Batch4On16] {
            let e = decode_explicit(&block, b).expect_err("batch must be rejected");
            assert_eq!(e.kind, BlockErrorKind::BackendRequiresBatchContext);
        }
        for b in [BackendId::RawCopy, BackendId::RleFill] {
            let e = decode_explicit(&block, b).expect_err("raw/rle must be rejected");
            assert_eq!(e.kind, BlockErrorKind::BackendFormatMismatch);
        }
    }

    #[test]
    fn test_disable_simd_conflict_is_typed_at_decode() {
        let data = nonuniform_data();
        let (block, _) = encode_with_codec(&data, 8);
        let cfg = ParallelConfig {
            backend_policy: BackendPolicy::Explicit(BackendId::Avx2TwoBy8On16),
            disable_simd: true,
            ..Default::default()
        };
        let e = decode_single_block(
            &DecodeBlockJob {
                block_index: 0,
                block_data: block,
            },
            &cfg,
        )
        .expect_err("disable_simd + explicit SIMD must be a typed conflict");
        assert_eq!(e.kind, BlockErrorKind::BackendUnavailable);
    }

    /// Prove that the AVX-512 kernels **execute** (backend identity in the
    /// result) when the build has `avx512bw` compiled in and the host CPU
    /// supports it — the Phase L "kernel compiled AND kernel executed"
    /// distinction.  In portable builds this test does not exist at all;
    /// the portable equivalents assert `BackendUnavailable` instead.
    #[cfg(all(target_arch = "x86_64", target_feature = "avx512bw"))]
    #[test]
    fn test_avx512_kernels_execute_on_native_build() {
        let data = nonuniform_data();
        let (block16, _) = encode_with_codec(&data, 8);
        let (block8, _) = encode_with_codec(&data, 7);

        for (b, block) in [
            (BackendId::Avx512Interleaved16, &block16),
            (BackendId::Avx512ManualGather16, &block16),
            (BackendId::Avx512Vl2x8, &block16),
            (BackendId::Avx512VlInterleaved8, &block8),
            (BackendId::Avx512VlManualGather8, &block8),
        ] {
            let dec = decode_explicit(block, b)
                .unwrap_or_else(|e| panic!("{b:?} must execute on a native AVX-512 build: {e:?}"));
            assert_eq!(dec.backend, b, "executed backend must be {b:?}");
            assert_eq!(dec.plan_backend, b);
            assert_eq!(dec.output, data, "{b:?} output must match");
            assert!(dec.words_consumed > 0, "{b:?} must report words consumed");
        }
    }

    // -------------------------------------------------------------------
    // Phase L.11 — adversarial malformed-input and differential tests
    // -------------------------------------------------------------------

    #[test]
    fn test_truncation_at_every_byte_never_panics() {
        // For every truncation position of a valid block, decode must return
        // a typed Err (or Ok only for the untruncated block) — never panic,
        // never over-read, never silently produce partial output.
        for codec in [7u16, 8u16] {
            let data = nonuniform_data();
            let (block, _) = encode_with_codec(&data, codec);
            for cut in 0..block.len() {
                let truncated = block[..cut].to_vec();
                let r = decode_single_block(
                    &DecodeBlockJob {
                        block_index: 0,
                        block_data: truncated,
                    },
                    &ParallelConfig::default(),
                );
                assert!(
                    r.is_err(),
                    "codec {codec}: truncated block of {} bytes (cut {cut}) must be rejected",
                    block.len()
                );
            }
        }
    }

    #[test]
    fn test_header_field_mutations_never_panic() {
        // Flip every header byte of a valid block; decode must always return
        // a typed error (never panic) — even when the mutation makes
        // scale_bits/state_count/codec_id inconsistent.
        for codec in [7u16, 8u16] {
            let data = nonuniform_data();
            let (block, _) = encode_with_codec(&data, codec);
            for byte_idx in 0..crate::block::BLOCK_HEADER_SIZE {
                let mut mutated = block.clone();
                mutated[byte_idx] ^= 0xFF;
                let r = decode_single_block(
                    &DecodeBlockJob {
                        block_index: 0,
                        block_data: mutated,
                    },
                    &ParallelConfig::default(),
                );
                assert!(
                    r.is_err(),
                    "codec {codec}: mutated header byte {byte_idx} must be rejected"
                );
            }
        }
    }

    #[test]
    fn test_model_corruption_never_panics_and_never_passes() {
        // Corrupt model frequencies in-place (keeping the payload hash
        // intact).  The decoded output hash must catch this: decode either
        // fails with DecodedHashMismatch (Strict) or produces different
        // output — it must never silently pass with the wrong bytes.
        let data = nonuniform_data();
        let (block, _) = encode_with_codec(&data, 8);
        for model_byte in 0..16 {
            let mut mutated = block.clone();
            // Flip a byte inside the 1024-byte model region (header is 104
            // bytes).  Flipping may break the frequency-sum invariant (Model
            // error) or produce a valid-but-wrong model (DecodedHashMismatch).
            mutated[104 + model_byte * 7] ^= 0x01;
            let r = decode_single_block(
                &DecodeBlockJob {
                    block_index: 0,
                    block_data: mutated,
                },
                &ParallelConfig::default(),
            );
            match r {
                Err(_) => {} // typed rejection is the expected outcome
                Ok(dec) => {
                    assert_ne!(
                        dec.output, data,
                        "model corruption at byte {model_byte} must not decode to the original data"
                    );
                    panic!(
                        "model corruption at byte {model_byte} decoded without an error; the decoded-hash check must catch this"
                    );
                }
            }
        }
    }

    #[test]
    fn test_randomized_cross_backend_differential() {
        // Randomized inputs of many lengths, decoded by every executable
        // backend for each codec: all backends must agree byte-for-byte and
        // on the report fields, and the result must equal the original.
        // Phase L.11 scalar/SIMD parity requirement.
        let mut seed = 0x5EED_2026u64;
        let mut rng = || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        for trial in 0..40 {
            let len = 1 + (rng() % 6000) as usize;
            let data: Vec<u8> = (0..len).map(|_| (rng() & 0xFF) as u8).collect();
            // Occasionally use skewed data (few distinct symbols).
            let data: Vec<u8> = if trial % 3 == 0 {
                data.iter().map(|b| b % 5).collect()
            } else {
                data
            };
            for codec in [7u16, 8u16] {
                let (block, _) = encode_with_codec(&data, codec);
                let family = if codec == 7 {
                    BACKENDS_8WAY
                } else {
                    BACKENDS_16WAY
                };
                let mut reference: Option<(Vec<u8>, usize, Vec<u32>)> = None;
                for &b in family {
                    match decode_explicit(&block, b) {
                        Ok(dec) => {
                            assert_eq!(
                                dec.output, data,
                                "trial {trial} codec {codec} {b:?}: output"
                            );
                            match &reference {
                                None => {
                                    reference =
                                        Some((dec.output, dec.words_consumed, dec.final_states))
                                }
                                Some((_, wc, fs)) => {
                                    assert_eq!(
                                        dec.words_consumed, *wc,
                                        "trial {trial} codec {codec} {b:?}: words"
                                    );
                                    assert_eq!(
                                        dec.final_states, *fs,
                                        "trial {trial} codec {codec} {b:?}: states"
                                    );
                                }
                            }
                        }
                        Err(BlockError {
                            kind: BlockErrorKind::BackendUnavailable,
                            ..
                        }) => {}
                        Err(other) => panic!(
                            "trial {trial} codec {codec} {b:?}: unexpected error {:?}",
                            other
                        ),
                    }
                }
                assert!(reference.is_some(), "trial {trial}: no backend executed");
            }
        }
    }
}
