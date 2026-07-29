//! # Decode plan — model-aware backend selection
//!
//! A worker receives a prevalidated immutable plan.  It must not repeat
//! expensive model classification inside the hot loop.
//!
//! Supported inner execution plans: RawCopy, RleFill, Scalar8, Scalar16,
//! Avx512Vl2x8, Avx512Batch4, Uniform256TableFree16.

use crate::cache::ModelCacheKey;
use crate::config::BackendId;

/// A validated, immutable decode plan for one block.
#[derive(Clone)]
pub enum DecodePlan {
    RawCopy,
    RleFill { symbol: u8, count: usize },
    Scalar16 { scale_bits: u8, is_uniform256: bool },
    Uniform256TableFree16 { scale_bits: u8 },
    Avx512Vl2x8 { scale_bits: u8 },
    Avx512Batch4 { scale_bits: u8 },
}

impl DecodePlan {
    pub fn backend_id(&self) -> BackendId {
        match self {
            Self::RawCopy => BackendId::RawCopy,
            Self::RleFill { .. } => BackendId::RleFill,
            Self::Scalar16 { .. } => BackendId::Scalar16,
            Self::Uniform256TableFree16 { .. } => BackendId::Uniform256TableFree16,
            Self::Avx512Vl2x8 { .. } => BackendId::Avx512Vl2x8,
            Self::Avx512Batch4 { .. } => BackendId::Avx512Batch4,
        }
    }
}

/// Create a decode plan from block metadata and runtime capabilities.
pub fn create_decode_plan(
    codec_id: u16,
    scale_bits: u8,
    model_data: &[u8],
    backend_policy: crate::config::BackendPolicy,
    _cpu_has_avx512: bool,
    _cpu_has_avx512vl: bool,
) -> DecodePlan {
    if codec_id == 8 && scale_bits == 12 {
        let is_uniform = false; // TODO: check actual model
        match backend_policy {
            crate::config::BackendPolicy::Portable
            | crate::config::BackendPolicy::ScalarPreferred => DecodePlan::Scalar16 {
                scale_bits,
                is_uniform256: is_uniform,
            },
            _ => DecodePlan::Scalar16 {
                scale_bits,
                is_uniform256: is_uniform,
            },
        }
    } else {
        DecodePlan::Scalar16 {
            scale_bits,
            is_uniform256: false,
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

    #[test]
    fn test_default_plan() {
        let p = create_decode_plan(
            8,
            12,
            &[],
            crate::config::BackendPolicy::Portable,
            false,
            false,
        );
        assert!(matches!(p, DecodePlan::Scalar16 { .. }));
    }
}
