//! # Decode plan — model-aware backend selection
//!
//! A worker receives a prevalidated immutable plan.  It must not repeat
//! expensive model classification inside the hot loop.
//!
//! Supported inner execution plans: RawCopy, RleFill, Scalar8, Scalar16,
//! Avx512Vl2x8, Avx512Batch4, Uniform256TableFree16.
//!
//! ## Backend selection logic
//!
//! 1. **Uniform256 model**: all 256 normalised frequencies equal 16 when
//!    scale_bits = 12 and total = 4096.  This enables the table-free
//!    sixteen-way kernel where `output = slot / 16` and
//!    `next_state = 16 * (x >> 12) + (slot & 15)`.
//!
//! 2. **Skewed model (AVX-512)**: if `codec_id == 8` and AVX-512VL is
//!    available, the 2×8 kernel may be selected for general skewed models.
//!
//! 3. **General model**: use scalar 16-way as the portable fallback.
//!    This is the safest, most tested path.
//!
//! 4. **8-way**: for `codec_id == 7`, use scalar 8-way.
//!
//! The planner must be created from validated block metadata.  No expensive
//! model classification happens inside the hot decode loop.

use crate::cache::ModelCacheKey;
use crate::config::BackendId;

/// A validated, immutable decode plan for one block.
#[derive(Clone)]
pub enum DecodePlan {
    /// Raw copy — no decoding needed.
    RawCopy,
    /// RLE fill — repeat a single symbol.
    RleFill { symbol: u8, count: usize },
    /// Scalar 8-way Word rANS (codec_id = 7).
    Scalar8 { scale_bits: u8 },
    /// Scalar 16-way Word rANS (codec_id = 8).
    Scalar16 { scale_bits: u8, is_uniform256: bool },
    /// Table-free 16-way uniform decode (slot/16 arithmetic).
    Uniform256TableFree16 { scale_bits: u8 },
    /// AVX-512VL 2×8-on-16-way decode.
    Avx512Vl2x8 { scale_bits: u8 },
    /// AVX-512 batch-4 decode.
    Avx512Batch4 { scale_bits: u8 },
    /// AVX2 manual-gather 8-way decode.
    Avx2ManualGather8 { scale_bits: u8 },
    /// AVX2 hardware-gather 8-way decode.
    Avx2HardwareGather8 { scale_bits: u8 },
    /// AVX2 two-vector 2×8 on 16-way format.
    Avx2TwoBy8On16 { scale_bits: u8 },
    /// AVX2 Uniform256 table-free 16-way decode.
    Avx2Uniform256TableFree16 { scale_bits: u8 },
    /// AVX2 batch-four 16-way decode.
    Avx2Batch4On16 { scale_bits: u8 },
}

impl DecodePlan {
    pub fn backend_id(&self) -> BackendId {
        match self {
            Self::RawCopy => BackendId::RawCopy,
            Self::RleFill { .. } => BackendId::RleFill,
            Self::Scalar8 { .. } => BackendId::Scalar8,
            Self::Scalar16 { .. } => BackendId::Scalar16,
            Self::Uniform256TableFree16 { .. } => BackendId::Uniform256TableFree16,
            Self::Avx512Vl2x8 { .. } => BackendId::Avx512Vl2x8,
            Self::Avx512Batch4 { .. } => BackendId::Avx512Batch4,
            Self::Avx2ManualGather8 { .. } => BackendId::Avx2ManualGather8,
            Self::Avx2HardwareGather8 { .. } => BackendId::Avx2HardwareGather8,
            Self::Avx2TwoBy8On16 { .. } => BackendId::Avx2TwoBy8On16,
            Self::Avx2Uniform256TableFree16 { .. } => BackendId::Avx2Uniform256TableFree16,
            Self::Avx2Batch4On16 { .. } => BackendId::Avx2Batch4On16,
        }
    }
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

/// Create a decode plan from block metadata and runtime capabilities.
///
/// This function inspects the actual model frequencies and CPU feature flags
/// to select the best available backend.  The plan is deterministic given
/// fixed inputs — it depends only on the model data, codec ID, and policy.
pub fn create_decode_plan(
    codec_id: u16,
    scale_bits: u8,
    model_data: &[u8],
    backend_policy: crate::config::BackendPolicy,
    cpu_has_avx512: bool,
    cpu_has_avx512vl: bool,
    cpu_has_avx2: bool,
    disable_simd: bool,
) -> DecodePlan {
    // Determine if the model is uniform256
    let uniform256 = is_uniform256_model(model_data, scale_bits);

    // disable_simd: a diagnostic safety control.  It forces scalar
    // selection for every auto/manual policy and makes an explicit SIMD
    // request a config conflict (returned by the caller as a typed error
    // before any execution).  Scalar kernels are always safe.
    if disable_simd {
        match backend_policy {
            // Explicit SIMD + disable_simd is a config conflict; signal it
            // by returning a scalar plan.  The caller validates and rejects
            // the combination before execution (see execute_decode_plan).
            crate::config::BackendPolicy::Explicit(b) => match b {
                crate::config::BackendId::Scalar8 => DecodePlan::Scalar8 { scale_bits },
                crate::config::BackendId::Scalar16
                | crate::config::BackendId::Sse41Interleaved8 => DecodePlan::Scalar16 {
                    scale_bits,
                    is_uniform256: uniform256,
                },
                _ => {
                    // Any other explicit backend is SIMD — the caller must
                    // reject; we return Scalar16 as a defensive fallback.
                    DecodePlan::Scalar16 {
                        scale_bits,
                        is_uniform256: uniform256,
                    }
                }
            },
            _ => match codec_id {
                7 => DecodePlan::Scalar8 { scale_bits },
                _ => DecodePlan::Scalar16 {
                    scale_bits,
                    is_uniform256: uniform256,
                },
            },
        }
    } else {
        create_decode_plan_inner(
            codec_id,
            scale_bits,
            model_data,
            backend_policy,
            cpu_has_avx512,
            cpu_has_avx512vl,
            cpu_has_avx2,
            uniform256,
        )
    }
}

fn create_decode_plan_inner(
    codec_id: u16,
    scale_bits: u8,
    model_data: &[u8],
    backend_policy: crate::config::BackendPolicy,
    cpu_has_avx512: bool,
    cpu_has_avx512vl: bool,
    cpu_has_avx2: bool,
    uniform256: bool,
) -> DecodePlan {
    match backend_policy {
        crate::config::BackendPolicy::Portable => {
            // Only portable scalar kernels
            match codec_id {
                7 => DecodePlan::Scalar8 { scale_bits },
                8 if uniform256 => DecodePlan::Scalar16 {
                    scale_bits,
                    is_uniform256: uniform256,
                },
                _ => DecodePlan::Scalar16 {
                    scale_bits,
                    is_uniform256: uniform256,
                },
            }
        }
        crate::config::BackendPolicy::ScalarPreferred => match codec_id {
            7 => DecodePlan::Scalar8 { scale_bits },
            _ => DecodePlan::Scalar16 {
                scale_bits,
                is_uniform256: uniform256,
            },
        },
        crate::config::BackendPolicy::Auto | crate::config::BackendPolicy::ModelAware => {
            // Conservative dispatch: scalar-first until multi-machine benchmarking
            // establishes architecture-specific crossover points.
            // Explicit AVX2 selection is available via `Explicit(Avx2TwoBy8On16)` etc.
            match codec_id {
                7 => DecodePlan::Scalar8 { scale_bits },
                8 if uniform256 => DecodePlan::Scalar16 {
                    scale_bits,
                    is_uniform256: true,
                },
                _ => DecodePlan::Scalar16 {
                    scale_bits,
                    is_uniform256: false,
                },
            }
        }
        crate::config::BackendPolicy::Explicit(backend) => match backend {
            BackendId::RawCopy => DecodePlan::RawCopy,
            BackendId::RleFill => DecodePlan::RleFill {
                symbol: 0,
                count: 0,
            },
            BackendId::Scalar8 => DecodePlan::Scalar8 { scale_bits },
            BackendId::Scalar16 => DecodePlan::Scalar16 {
                scale_bits,
                is_uniform256: uniform256,
            },
            BackendId::Sse41Interleaved8 => DecodePlan::Scalar8 { scale_bits },
            BackendId::Avx512VlInterleaved8 => DecodePlan::Scalar8 { scale_bits },
            BackendId::Avx512Interleaved16 => DecodePlan::Scalar16 {
                scale_bits,
                is_uniform256: uniform256,
            },
            BackendId::Avx512VlManualGather8 => DecodePlan::Scalar8 { scale_bits },
            BackendId::Avx512ManualGather16 => DecodePlan::Scalar16 {
                scale_bits,
                is_uniform256: uniform256,
            },
            BackendId::Uniform256TableFree16 => DecodePlan::Uniform256TableFree16 { scale_bits },
            BackendId::Avx512Vl2x8 => DecodePlan::Avx512Vl2x8 { scale_bits },
            BackendId::Avx512Batch4 => DecodePlan::Avx512Batch4 { scale_bits },
            BackendId::Avx2ManualGather8 => DecodePlan::Avx2ManualGather8 { scale_bits },
            BackendId::Avx2HardwareGather8 => DecodePlan::Avx2HardwareGather8 { scale_bits },
            BackendId::Avx2TwoBy8On16 => DecodePlan::Avx2TwoBy8On16 { scale_bits },
            BackendId::Avx2Uniform256TableFree16 => {
                DecodePlan::Avx2Uniform256TableFree16 { scale_bits }
            }
            BackendId::Avx2Batch4On16 => DecodePlan::Avx2Batch4On16 { scale_bits },
        },
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
        let plan = create_decode_plan(
            8,
            12,
            &uniform,
            crate::config::BackendPolicy::Portable,
            false,
            false,
            false,
            false,
        );
        assert!(matches!(plan, DecodePlan::Scalar16 { .. }));
    }

    #[test]
    fn test_model_aware_uniform256() {
        let uniform = uniform256_model_data();
        // Auto dispatch is conservative: always scalar until benchmarks exist.
        let plan = create_decode_plan(
            8,
            12,
            &uniform,
            crate::config::BackendPolicy::Auto,
            false,
            false,
            true,
            false,
        );
        assert!(matches!(
            plan,
            DecodePlan::Scalar16 {
                is_uniform256: true,
                ..
            }
        ));

        // Explicit policy still selects the requested backend.
        let plan2 = create_decode_plan(
            8,
            12,
            &uniform,
            crate::config::BackendPolicy::Explicit(BackendId::Avx2Uniform256TableFree16),
            false,
            false,
            true,
            false,
        );
        assert!(matches!(
            plan2,
            DecodePlan::Avx2Uniform256TableFree16 { .. }
        ));
    }

    #[test]
    fn test_scalar_auto() {
        // Auto dispatch with AVX2 available still returns scalar (conservative).
        let skewed = skewed_model_data();
        let plan = create_decode_plan(
            8,
            12,
            &skewed,
            crate::config::BackendPolicy::Auto,
            false,
            false,
            true,
            false,
        );
        assert!(matches!(
            plan,
            DecodePlan::Scalar16 {
                is_uniform256: false,
                ..
            }
        ));

        let plan2 = create_decode_plan(
            7,
            12,
            &skewed,
            crate::config::BackendPolicy::Auto,
            false,
            false,
            true,
            false,
        );
        assert!(matches!(plan2, DecodePlan::Scalar8 { .. }));
    }

    #[test]
    fn test_explicit_avx2_plan() {
        // Explicit policy must select the requested backend.
        let skewed = skewed_model_data();
        let plan = create_decode_plan(
            8,
            12,
            &skewed,
            crate::config::BackendPolicy::Explicit(BackendId::Avx2TwoBy8On16),
            false,
            false,
            true,
            false,
        );
        assert!(matches!(plan, DecodePlan::Avx2TwoBy8On16 { .. }));
    }

    #[test]
    fn test_scalar8_plan() {
        let uniform = uniform256_model_data();
        let plan = create_decode_plan(
            7,
            12,
            &uniform,
            crate::config::BackendPolicy::Portable,
            false,
            false,
            false,
            false,
        );
        assert!(matches!(plan, DecodePlan::Scalar8 { .. }));
    }

    #[test]
    fn test_explicit_backend() {
        let uniform = uniform256_model_data();
        let plan = create_decode_plan(
            8,
            12,
            &uniform,
            crate::config::BackendPolicy::Explicit(BackendId::Scalar16),
            false,
            false,
            false,
            false,
        );
        assert!(matches!(plan, DecodePlan::Scalar16 { .. }));
    }

    #[test]
    fn test_disable_simd_forces_scalar() {
        let uniform = uniform256_model_data();
        // Explicit SIMD request + disable_simd must fall back to scalar.
        let plan = create_decode_plan(
            8,
            12,
            &uniform,
            crate::config::BackendPolicy::Explicit(BackendId::Avx2TwoBy8On16),
            false,
            false,
            true,
            true,
        );
        assert!(matches!(plan, DecodePlan::Scalar16 { .. }));
    }
}
