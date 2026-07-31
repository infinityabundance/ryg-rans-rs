//! # Decode plan — model-aware backend selection
//!
//! A worker receives a prevalidated immutable plan.  It must not repeat
//! expensive model classification inside the hot loop.
//!
//! ## Exact-backend doctrine (Phase L.9)
//!
//! An **explicit** backend request is never rewritten to a different backend
//! during planning.  Every explicit request either produces a real plan for
//! the requested backend, or a typed [`BlockError`] explaining why it cannot
//! (`BackendFormatMismatch`, `BackendUnavailable`,
//! `BackendRequiresBatchContext`).  There is no silent scalar substitution.
//!
//! Format compatibility is validated **before** execution:
//!
//! ```text
//! 8-way backend   ↔ codec 7  (canonical 8-way stream)
//! 16-way backend  ↔ codec 8  (canonical 16-way stream)
//! Uniform256      ↔ validated Uniform256 model (all freqs == 16, scale 12)
//! Batch backend   ↔ coordinator batch context (NOT available via one-block API)
//! RAW backend     ↔ RAW block kind
//! RLE backend     ↔ RLE block kind
//! ```
//!
//! Invalid combinations return a typed error at planning time.  The planner
//! is the single source of truth for these checks — the executor never
//! re-derives them.
//!
//! ## Backend selection logic (non-explicit policies)
//!
//! 1. **Portable / ScalarPreferred / Auto**: conservative scalar dispatch.
//!    `Scalar8` for codec 7, `Scalar16` for codec 8.  No runtime SIMD
//!    selection until multi-machine benchmarks establish crossover points.
//!
//! 2. **ModelAware**: as `Auto`, but a validated Uniform256 model (all 256
//!    normalised frequencies equal 16 at scale_bits = 12) selects the real
//!    table-free scalar kernel `Uniform256TableFree16`, where
//!    `symbol = slot >> 4` and `next_state = 16 * (x >> 12) + (slot & 15)`.
//!    This is a distinct, executable plan — not a rename of `Scalar16`.
//!
//! 3. **Explicit(backend)**: exact request, validated for format
//!    compatibility, never rewritten.  Execution capability (CPU features,
//!    compiled target features) is checked at execution time and reported as
//!    `BackendUnavailable` if absent.
//!
//! ## `disable_simd` semantics
//!
//! `disable_simd` forces scalar plans for every non-explicit policy and makes
//! an explicit SIMD request a typed config conflict (`BackendUnavailable`).
//! `Uniform256TableFree16` is scalar arithmetic (no vector instructions) and
//! is therefore not treated as SIMD by this control.

use crate::block::{
    BLOCK_KIND_RAW, BLOCK_KIND_RLE, CODEC_WORD_INTERLEAVED8, CODEC_WORD_INTERLEAVED16,
};
use crate::cache::ModelCacheKey;
use crate::config::BackendId;
use crate::error::{BlockError, BlockErrorKind};

/// A validated, immutable decode plan for one block.
///
/// The plan records **intent** — which backend was selected.  The executor
/// reports what actually ran separately (see `ExecutedDecode`).
#[derive(Clone, Debug)]
pub enum DecodePlan {
    /// Scalar 8-way Word rANS (codec_id = 7).
    Scalar8 { scale_bits: u8 },
    /// Scalar 16-way Word rANS (codec_id = 8).
    Scalar16 { scale_bits: u8, is_uniform256: bool },
    /// Table-free 16-way uniform decode (slot/16 arithmetic, scalar).
    Uniform256TableFree16 { scale_bits: u8 },
    /// SSE4.1 interleaved 8-way decode.
    Sse41Interleaved8 { scale_bits: u8 },
    /// AVX-512VL (256-bit) interleaved 8-way decode.
    Avx512VlInterleaved8 { scale_bits: u8 },
    /// AVX-512 (512-bit) interleaved 16-way decode.
    Avx512Interleaved16 { scale_bits: u8 },
    /// AVX-512VL manual-gather 8-way decode.
    Avx512VlManualGather8 { scale_bits: u8 },
    /// AVX-512 manual-gather 16-way decode.
    Avx512ManualGather16 { scale_bits: u8 },
    /// AVX-512VL 2×8-on-16-way decode.
    Avx512Vl2x8 { scale_bits: u8 },
    /// AVX2 manual-gather 8-way decode.
    Avx2ManualGather8 { scale_bits: u8 },
    /// AVX2 hardware-gather 8-way decode.
    Avx2HardwareGather8 { scale_bits: u8 },
    /// AVX2 two-vector 2×8 on 16-way format.
    Avx2TwoBy8On16 { scale_bits: u8 },
    /// AVX2 Uniform256 table-free 16-way decode.
    Avx2Uniform256TableFree16 { scale_bits: u8 },
}

impl DecodePlan {
    /// The `BackendId` this plan *intends* to execute.
    ///
    /// This is the selected-plan identity.  It may differ from the executed
    /// identity only when execution failed; on success they are equal
    /// (Phase L.9 forbids silent backend substitution).
    pub fn backend_id(&self) -> BackendId {
        match self {
            Self::Scalar8 { .. } => BackendId::Scalar8,
            Self::Scalar16 { .. } => BackendId::Scalar16,
            Self::Uniform256TableFree16 { .. } => BackendId::Uniform256TableFree16,
            Self::Sse41Interleaved8 { .. } => BackendId::Sse41Interleaved8,
            Self::Avx512VlInterleaved8 { .. } => BackendId::Avx512VlInterleaved8,
            Self::Avx512Interleaved16 { .. } => BackendId::Avx512Interleaved16,
            Self::Avx512VlManualGather8 { .. } => BackendId::Avx512VlManualGather8,
            Self::Avx512ManualGather16 { .. } => BackendId::Avx512ManualGather16,
            Self::Avx512Vl2x8 { .. } => BackendId::Avx512Vl2x8,
            Self::Avx2ManualGather8 { .. } => BackendId::Avx2ManualGather8,
            Self::Avx2HardwareGather8 { .. } => BackendId::Avx2HardwareGather8,
            Self::Avx2TwoBy8On16 { .. } => BackendId::Avx2TwoBy8On16,
            Self::Avx2Uniform256TableFree16 { .. } => BackendId::Avx2Uniform256TableFree16,
        }
    }
}

/// Whether a backend operates on the 8-way (codec 7) or 16-way (codec 8)
/// stream format.  `None` for backends that are not RANS-width bound
/// (RAW, RLE, batch).
fn backend_width(backend: BackendId) -> Option<BackendWidth> {
    Some(match backend {
        BackendId::Scalar8
        | BackendId::Sse41Interleaved8
        | BackendId::Avx512VlInterleaved8
        | BackendId::Avx512VlManualGather8
        | BackendId::Avx2ManualGather8
        | BackendId::Avx2HardwareGather8 => BackendWidth::Eight,
        BackendId::Scalar16
        | BackendId::Avx512Interleaved16
        | BackendId::Avx512ManualGather16
        | BackendId::Avx512Vl2x8
        | BackendId::Uniform256TableFree16
        | BackendId::Avx2TwoBy8On16
        | BackendId::Avx2Uniform256TableFree16 => BackendWidth::Sixteen,
        BackendId::RawCopy
        | BackendId::RleFill
        | BackendId::Avx512Batch4
        | BackendId::Avx2Batch4On16 => return None,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendWidth {
    Eight,
    Sixteen,
}

/// True if the backend executes SIMD (vector) instructions.
///
/// `Uniform256TableFree16` is scalar arithmetic — `symbol = slot >> 4` — and
/// therefore is not SIMD for the purposes of the `disable_simd` control.
fn is_simd_backend(backend: BackendId) -> bool {
    !matches!(
        backend,
        BackendId::Scalar8
            | BackendId::Scalar16
            | BackendId::RawCopy
            | BackendId::RleFill
            | BackendId::Uniform256TableFree16
    )
}

/// Check whether a frequency model represents the uniform-256 distribution.
///
/// Uniform256 means: scale_bits = 12, total = 4096, and every symbol's
/// normalised frequency is exactly 16 (4096 / 256).
fn is_uniform256_model(model_data: &[u8], scale_bits: u8) -> bool {
    if scale_bits != 12 {
        return false;
    }
    if model_data.len() < 1024 {
        return false;
    }
    // Check that every frequency is 16 (u32 LE)
    for chunk in model_data.chunks_exact(4).take(256) {
        let f = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if f != 16 {
            return false;
        }
    }
    true
}

/// Validate format compatibility between a requested backend and a block.
///
/// Phase L.9 compatibility matrix:
///
/// ```text
/// 8-way backend   ↔ codec 7
/// 16-way backend  ↔ codec 8
/// Uniform256      ↔ validated Uniform256 model
/// Batch backend   ↔ coordinator batch context (rejected for one-block API)
/// RAW backend     ↔ RAW block kind
/// RLE backend     ↔ RLE block kind
/// ```
///
/// Any violation returns a typed `BlockError`; no plan is produced.
fn validate_backend_format(
    backend: BackendId,
    codec_id: u16,
    block_kind: u8,
    uniform256: bool,
    bi: u64,
) -> Result<(), BlockError> {
    let mismatch = || BlockError {
        block_index: bi,
        kind: BlockErrorKind::BackendFormatMismatch,
    };
    match backend {
        BackendId::RawCopy => {
            return if block_kind == BLOCK_KIND_RAW {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        BackendId::RleFill => {
            return if block_kind == BLOCK_KIND_RLE {
                Ok(())
            } else {
                Err(mismatch())
            };
        }
        // Batch backends need coordinator-level grouping of four compatible
        // jobs.  The one-block plan API cannot execute them; reject at
        // planning time so no unexecutable plan is ever selected.
        BackendId::Avx512Batch4 | BackendId::Avx2Batch4On16 => {
            return Err(BlockError {
                block_index: bi,
                kind: BlockErrorKind::BackendRequiresBatchContext,
            });
        }
        _ => {}
    }
    let expected = match backend_width(backend) {
        Some(BackendWidth::Eight) => CODEC_WORD_INTERLEAVED8,
        Some(BackendWidth::Sixteen) => CODEC_WORD_INTERLEAVED16,
        None => return Err(mismatch()),
    };
    if codec_id != expected {
        return Err(mismatch());
    }
    // Uniform256 backends additionally require a validated Uniform256 model.
    if matches!(
        backend,
        BackendId::Uniform256TableFree16 | BackendId::Avx2Uniform256TableFree16
    ) && !uniform256
    {
        return Err(mismatch());
    }
    Ok(())
}

/// Create a decode plan from block metadata.
///
/// The planner is **capability-agnostic**: it does not inspect runtime CPU
/// features or compile-time target features.  Non-explicit policies select
/// scalar plans only (portable on every host); explicit policies produce the
/// requested plan or a typed error.  Whether a requested SIMD backend can
/// actually execute is checked at execution time (`execute_decode_plan`),
/// which reports `BackendUnavailable` when the CPU or build cannot run it.
///
/// # Errors
///
/// Returns a typed [`BlockError`] at planning time for:
///
/// * `BackendFormatMismatch` — explicit backend incompatible with the block
///   format (width vs codec, Uniform256 model requirement, RAW/RLE kind).
/// * `BackendUnavailable` — explicit SIMD request combined with
///   `disable_simd`.
/// * `BackendRequiresBatchContext` — explicit batch backend via the one-block
///   plan API.
///
/// The plan is deterministic given fixed inputs — it depends only on the
/// model data, codec ID, block kind, and policy.
pub fn create_decode_plan(
    codec_id: u16,
    scale_bits: u8,
    model_data: &[u8],
    backend_policy: crate::config::BackendPolicy,
    disable_simd: bool,
    block_kind: u8,
    bi: u64,
) -> Result<DecodePlan, BlockError> {
    // Determine if the model is uniform256
    let uniform256 = is_uniform256_model(model_data, scale_bits);

    match backend_policy {
        crate::config::BackendPolicy::Explicit(backend) => {
            // `disable_simd` with an explicit SIMD backend is a config
            // conflict — a typed error, never a silent scalar substitution.
            if disable_simd && is_simd_backend(backend) {
                return Err(BlockError {
                    block_index: bi,
                    kind: BlockErrorKind::BackendUnavailable,
                });
            }
            // Format compatibility is validated here, before any execution.
            // `validate_backend_format` rejects RawCopy/RleFill (block-kind
            // mismatch) and batch backends (no coordinator context), so the
            // match below is total over the remaining RANS backends.
            validate_backend_format(backend, codec_id, block_kind, uniform256, bi)?;
            Ok(match backend {
                BackendId::Scalar8 => DecodePlan::Scalar8 { scale_bits },
                BackendId::Scalar16 => DecodePlan::Scalar16 {
                    scale_bits,
                    is_uniform256: uniform256,
                },
                BackendId::Sse41Interleaved8 => DecodePlan::Sse41Interleaved8 { scale_bits },
                BackendId::Avx512VlInterleaved8 => DecodePlan::Avx512VlInterleaved8 { scale_bits },
                BackendId::Avx512Interleaved16 => DecodePlan::Avx512Interleaved16 { scale_bits },
                BackendId::Avx512VlManualGather8 => {
                    DecodePlan::Avx512VlManualGather8 { scale_bits }
                }
                BackendId::Avx512ManualGather16 => DecodePlan::Avx512ManualGather16 { scale_bits },
                BackendId::Uniform256TableFree16 => {
                    DecodePlan::Uniform256TableFree16 { scale_bits }
                }
                BackendId::Avx512Vl2x8 => DecodePlan::Avx512Vl2x8 { scale_bits },
                BackendId::Avx2ManualGather8 => DecodePlan::Avx2ManualGather8 { scale_bits },
                BackendId::Avx2HardwareGather8 => DecodePlan::Avx2HardwareGather8 { scale_bits },
                BackendId::Avx2TwoBy8On16 => DecodePlan::Avx2TwoBy8On16 { scale_bits },
                BackendId::Avx2Uniform256TableFree16 => {
                    DecodePlan::Avx2Uniform256TableFree16 { scale_bits }
                }
                // Rejected by validate_backend_format above; defensive arms
                // that return the same typed errors instead of any plan.
                BackendId::RawCopy | BackendId::RleFill => {
                    return Err(BlockError {
                        block_index: bi,
                        kind: BlockErrorKind::BackendFormatMismatch,
                    });
                }
                BackendId::Avx512Batch4 | BackendId::Avx2Batch4On16 => {
                    return Err(BlockError {
                        block_index: bi,
                        kind: BlockErrorKind::BackendRequiresBatchContext,
                    });
                }
            })
        }
        _ => {
            // Non-explicit policies never select SIMD.  `disable_simd` forces
            // plain scalar (never the table-free kernel, which is scalar but
            // belongs to the "uniform" family this control excludes for
            // maximal conservatism).
            if disable_simd {
                return Ok(match codec_id {
                    CODEC_WORD_INTERLEAVED8 => DecodePlan::Scalar8 { scale_bits },
                    _ => DecodePlan::Scalar16 {
                        scale_bits,
                        is_uniform256: uniform256,
                    },
                });
            }
            if matches!(backend_policy, crate::config::BackendPolicy::ModelAware) {
                // ModelAware is distinct from Auto: a validated Uniform256
                // model selects the real table-free scalar kernel.
                Ok(match codec_id {
                    CODEC_WORD_INTERLEAVED8 => DecodePlan::Scalar8 { scale_bits },
                    CODEC_WORD_INTERLEAVED16 if uniform256 => {
                        DecodePlan::Uniform256TableFree16 { scale_bits }
                    }
                    _ => DecodePlan::Scalar16 {
                        scale_bits,
                        is_uniform256: uniform256,
                    },
                })
            } else {
                // Portable / ScalarPreferred / Auto: conservative scalar.
                Ok(match codec_id {
                    CODEC_WORD_INTERLEAVED8 => DecodePlan::Scalar8 { scale_bits },
                    _ => DecodePlan::Scalar16 {
                        scale_bits,
                        is_uniform256: uniform256,
                    },
                })
            }
        }
    }
}

/// Compute a cache key for a decode plan.
pub fn plan_cache_key(codec_id: u16, scale_bits: u8, model_data: &[u8]) -> ModelCacheKey {
    let model_sha256 = crate::encode::sha256(model_data);
    ModelCacheKey {
        model_sha256,
        scale_bits,
        codec_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::BackendPolicy;

    fn uniform256_model_data() -> Vec<u8> {
        let mut data = Vec::with_capacity(1024);
        for _ in 0..256 {
            data.extend_from_slice(&16u32.to_le_bytes());
        }
        data
    }

    fn skewed_model_data() -> Vec<u8> {
        let mut data = Vec::with_capacity(1024);
        // Most freq concentrated in first few symbols
        let mut remaining = 4096u32;
        for i in 0..256 {
            let f = if i < 5 {
                remaining / 5
            } else {
                1u32.max(remaining / (256 - i))
            };
            let f = f.min(remaining);
            data.extend_from_slice(&f.to_le_bytes());
            remaining -= f;
        }
        // Ensure exact total
        if remaining > 0 {
            let last = u32::from_le_bytes([data[1020], data[1021], data[1022], data[1023]]);
            let corrected = last + remaining;
            data[1020..1024].copy_from_slice(&corrected.to_le_bytes());
        }
        data
    }

    fn plan(
        codec: u16,
        scale: u8,
        model: &[u8],
        policy: BackendPolicy,
        disable_simd: bool,
    ) -> Result<DecodePlan, BlockError> {
        create_decode_plan(codec, scale, model, policy, disable_simd, 0, 7)
    }

    #[test]
    fn test_uniform256_detection() {
        let data = uniform256_model_data();
        assert!(is_uniform256_model(&data, 12));
        assert!(!is_uniform256_model(&data, 11)); // wrong scale_bits
        assert!(!is_uniform256_model(&[], 12)); // empty data
    }

    #[test]
    fn test_skewed_not_uniform() {
        let data = skewed_model_data();
        assert!(!is_uniform256_model(&data, 12));
    }

    #[test]
    fn test_portable_plan() {
        let uniform = uniform256_model_data();
        let p = plan(8, 12, &uniform, BackendPolicy::Portable, false).unwrap();
        assert!(matches!(p, DecodePlan::Scalar16 { .. }));
    }

    #[test]
    fn test_model_aware_selects_table_free_for_uniform256() {
        let uniform = uniform256_model_data();
        // ModelAware must select the real table-free kernel for a validated
        // Uniform256 model — this is the documented distinction from Auto.
        let p = plan(8, 12, &uniform, BackendPolicy::ModelAware, false).unwrap();
        assert!(matches!(p, DecodePlan::Uniform256TableFree16 { .. }));

        // Auto stays conservative: plain Scalar16.
        let p2 = plan(8, 12, &uniform, BackendPolicy::Auto, false).unwrap();
        assert!(matches!(
            p2,
            DecodePlan::Scalar16 {
                is_uniform256: true,
                ..
            }
        ));

        // ModelAware with a skewed model falls back to Scalar16.
        let skewed = skewed_model_data();
        let p3 = plan(8, 12, &skewed, BackendPolicy::ModelAware, false).unwrap();
        assert!(matches!(p3, DecodePlan::Scalar16 { .. }));
    }

    #[test]
    fn test_explicit_avx2_plan_never_rewritten() {
        let skewed = skewed_model_data();
        // Explicit must produce the requested plan, not a scalar rewrite.
        let p = plan(
            8,
            12,
            &skewed,
            BackendPolicy::Explicit(BackendId::Avx2TwoBy8On16),
            false,
        )
        .unwrap();
        assert!(matches!(p, DecodePlan::Avx2TwoBy8On16 { .. }));
    }

    #[test]
    fn test_explicit_simd_backends_map_to_real_plans() {
        // Every explicit SIMD backend must produce a plan of its own identity
        // (Phase L.9: never rewritten to scalar during planning).
        let skewed = skewed_model_data();
        let cases = [
            (BackendId::Sse41Interleaved8, 7u16),
            (BackendId::Avx512VlInterleaved8, 7),
            (BackendId::Avx512Interleaved16, 8),
            (BackendId::Avx512VlManualGather8, 7),
            (BackendId::Avx512ManualGather16, 8),
            (BackendId::Avx512Vl2x8, 8),
            (BackendId::Avx2ManualGather8, 7),
            (BackendId::Avx2HardwareGather8, 7),
            (BackendId::Avx2TwoBy8On16, 8),
        ];
        for (backend, codec) in cases {
            let p = plan(codec, 12, &skewed, BackendPolicy::Explicit(backend), false).unwrap();
            assert_eq!(
                p.backend_id(),
                backend,
                "explicit {backend:?} must plan to itself"
            );
        }
    }

    #[test]
    fn test_format_mismatch_rejected() {
        let skewed = skewed_model_data();
        // 8-way backend on a codec-8 (16-way) block → typed mismatch.
        let e = plan(
            8,
            12,
            &skewed,
            BackendPolicy::Explicit(BackendId::Scalar8),
            false,
        )
        .unwrap_err();
        assert_eq!(e.kind, BlockErrorKind::BackendFormatMismatch);
        // 16-way backend on a codec-7 (8-way) block → typed mismatch.
        let e = plan(
            7,
            12,
            &skewed,
            BackendPolicy::Explicit(BackendId::Scalar16),
            false,
        )
        .unwrap_err();
        assert_eq!(e.kind, BlockErrorKind::BackendFormatMismatch);
    }

    #[test]
    fn test_uniform256_backend_requires_uniform_model() {
        let skewed = skewed_model_data();
        let e = plan(
            8,
            12,
            &skewed,
            BackendPolicy::Explicit(BackendId::Uniform256TableFree16),
            false,
        )
        .unwrap_err();
        assert_eq!(e.kind, BlockErrorKind::BackendFormatMismatch);

        let uniform = uniform256_model_data();
        let p = plan(
            8,
            12,
            &uniform,
            BackendPolicy::Explicit(BackendId::Uniform256TableFree16),
            false,
        )
        .unwrap();
        assert!(matches!(p, DecodePlan::Uniform256TableFree16 { .. }));
    }

    #[test]
    fn test_batch_backend_rejected_at_plan_time() {
        // Batch backends need coordinator context; the one-block planner must
        // reject them with a typed error rather than selecting an
        // unexecutable plan.
        let skewed = skewed_model_data();
        for backend in [BackendId::Avx512Batch4, BackendId::Avx2Batch4On16] {
            let e = plan(8, 12, &skewed, BackendPolicy::Explicit(backend), false).unwrap_err();
            assert_eq!(e.kind, BlockErrorKind::BackendRequiresBatchContext);
        }
    }

    #[test]
    fn test_raw_rle_backends_require_matching_block_kind() {
        // RAW ↔ RAW block, RLE ↔ RLE block.  The parallel crate only parses
        // RANS blocks (kind 0), so both requests are format mismatches.
        let skewed = skewed_model_data();
        let e = plan(
            7,
            12,
            &skewed,
            BackendPolicy::Explicit(BackendId::RawCopy),
            false,
        )
        .unwrap_err();
        assert_eq!(e.kind, BlockErrorKind::BackendFormatMismatch);
        let e = plan(
            8,
            12,
            &skewed,
            BackendPolicy::Explicit(BackendId::RleFill),
            false,
        )
        .unwrap_err();
        assert_eq!(e.kind, BlockErrorKind::BackendFormatMismatch);
    }

    #[test]
    fn test_disable_simd_conflict_is_typed() {
        let uniform = uniform256_model_data();
        // Explicit SIMD + disable_simd → typed conflict, never scalar.
        let e = plan(
            8,
            12,
            &uniform,
            BackendPolicy::Explicit(BackendId::Avx2TwoBy8On16),
            true,
        )
        .unwrap_err();
        assert_eq!(e.kind, BlockErrorKind::BackendUnavailable);

        // disable_simd with non-explicit policy → plain scalar.
        let p = plan(8, 12, &uniform, BackendPolicy::ModelAware, true).unwrap();
        assert!(matches!(p, DecodePlan::Scalar16 { .. }));

        // Explicit scalar + disable_simd → allowed.
        let p = plan(
            8,
            12,
            &uniform,
            BackendPolicy::Explicit(BackendId::Scalar16),
            true,
        )
        .unwrap();
        assert!(matches!(p, DecodePlan::Scalar16 { .. }));

        // Explicit Uniform256TableFree16 is scalar arithmetic — allowed
        // under disable_simd when the model validates.
        let p = plan(
            8,
            12,
            &uniform,
            BackendPolicy::Explicit(BackendId::Uniform256TableFree16),
            true,
        )
        .unwrap();
        assert!(matches!(p, DecodePlan::Uniform256TableFree16 { .. }));
    }

    #[test]
    fn test_scalar8_plan() {
        let uniform = uniform256_model_data();
        let p = plan(7, 12, &uniform, BackendPolicy::Portable, false).unwrap();
        assert!(matches!(p, DecodePlan::Scalar8 { .. }));
    }
}
