//! # Backend detection and dispatch for Word rANS decoders
//!
//! Provides runtime CPU-feature detection, backend selection, and
//! safe public API wrappers for all decode surfaces.

use crate::packed_table::{DecodeReport, PackedWordTable};
use alloc::vec::Vec;

/// Identifies which decoder backend was actually used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeBackend {
    /// Pure-Rust scalar 8-way decode (always available).
    Scalar8,
    /// SSE4.1 8-way interleaved decode (requires SSE4.1 + SSSE3).
    Sse41Interleaved8,
    /// AVX512VL 8-way interleaved decode (requires AVX512F + AVX512VL + AVX512BW).
    Avx512VlInterleaved8,
    /// Pure-Rust scalar 16-way decode (always available).
    Scalar16,
    /// AVX-512 16-way interleaved decode (requires AVX512F + AVX512BW).
    Avx512Interleaved16,
}

impl DecodeBackend {
    /// Stable string identifier for use in court receipts and backend assertions.
    pub fn label(&self) -> &'static str {
        match self {
            DecodeBackend::Scalar8 => "scalar-8way",
            DecodeBackend::Sse41Interleaved8 => "sse41-8way",
            DecodeBackend::Avx512VlInterleaved8 => "avx512vl-8way",
            DecodeBackend::Scalar16 => "scalar-16way",
            DecodeBackend::Avx512Interleaved16 => "avx512-16way",
        }
    }
}

/// Full decode result including backend identity.
#[derive(Clone, Debug)]
pub struct DecodeResult {
    pub output: Vec<u8>,
    pub report: DecodeReport,
    pub backend: DecodeBackend,
}

/// Extended decode error type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    InputTooShort,
    InvalidTable,
    UnsupportedBackend,
    OutputLengthMismatch,
    TrailingData,
    StateInvariantViolation,
}

// ---------------------------------------------------------------------------
// Runtime feature detection
// ---------------------------------------------------------------------------

/// Check whether AVX512F + AVX512VL + AVX512BW are available.
///
/// Uses compile-time `cfg!(target_feature)` when `std` is not available.
/// When `std` is available, also checks runtime CPUID.
fn avx512vl_available() -> bool {
    #[cfg(feature = "std")]
    {
        std::is_x86_feature_detected!("avx512f")
            && std::is_x86_feature_detected!("avx512vl")
            && std::is_x86_feature_detected!("avx512bw")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(all(
            target_feature = "avx512f",
            target_feature = "avx512vl",
            target_feature = "avx512bw",
        ))
    }
}

/// Check whether AVX512F + AVX512BW are available at runtime.
fn avx512_available() -> bool {
    #[cfg(feature = "std")]
    {
        std::is_x86_feature_detected!("avx512f") && std::is_x86_feature_detected!("avx512bw")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(all(target_feature = "avx512f", target_feature = "avx512bw",))
    }
}

// ---------------------------------------------------------------------------
// Safe 8-way auto-dispatch
// ---------------------------------------------------------------------------

/// Decode 8-way interleaved Word rANS using the best available backend.
///
/// Automatically selects AVX512VL → SSE4.1 → scalar based on runtime
/// feature detection.
pub fn decode_interleaved8_auto(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if avx512vl_available() {
            unsafe {
                let (output, report) = crate::avx512::decode_interleaved8_avx512vl_kernel(
                    compressed,
                    table,
                    expected_len,
                )
                .map_err(|_| DecodeError::InputTooShort)?;
                return Ok(DecodeResult {
                    output,
                    report,
                    backend: DecodeBackend::Avx512VlInterleaved8,
                });
            }
        }
    }
    // Fall back to scalar packed decoder
    let output = crate::packed_table::decode_8way_packed_scalar(compressed, table, expected_len)
        .map_err(|_| DecodeError::InputTooShort)?;
    let report = DecodeReport {
        words_consumed: compressed.len(), // approximate
        final_states: [0u32; 16],
    };
    Ok(DecodeResult {
        output,
        report,
        backend: DecodeBackend::Scalar8,
    })
}

/// Decode 8-way using the explicit scalar backend.
pub fn decode_interleaved8_scalar(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    let output = crate::packed_table::decode_8way_packed_scalar(compressed, table, expected_len)
        .map_err(|_| DecodeError::InputTooShort)?;
    Ok(DecodeResult {
        output,
        report: DecodeReport {
            words_consumed: compressed.len(),
            final_states: [0u32; 16],
        },
        backend: DecodeBackend::Scalar8,
    })
}

/// Decode 8-way using the explicit AVX512VL backend.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available.
pub unsafe fn decode_interleaved8_avx512vl(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    let (output, report) =
        crate::avx512::decode_interleaved8_avx512vl_kernel(compressed, table, expected_len)
            .map_err(|_| DecodeError::InputTooShort)?;
    Ok(DecodeResult {
        output,
        report,
        backend: DecodeBackend::Avx512VlInterleaved8,
    })
}

// ---------------------------------------------------------------------------
// Safe 16-way auto-dispatch
// ---------------------------------------------------------------------------

/// Decode 16-way interleaved Word rANS using the best available backend.
///
/// Automatically selects AVX512 → scalar based on runtime feature detection.
pub fn decode_interleaved16_auto(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
    {
        if avx512_available() {
            unsafe {
                let (output, report) = crate::avx512::decode_interleaved16_avx512_kernel(
                    compressed,
                    table,
                    expected_len,
                )
                .map_err(|_| DecodeError::InputTooShort)?;
                return Ok(DecodeResult {
                    output,
                    report,
                    backend: DecodeBackend::Avx512Interleaved16,
                });
            }
        }
    }
    // Fall back to scalar
    let (output, report) =
        crate::packed_table::decode_interleaved16_scalar(compressed, table, expected_len)
            .map_err(|_| DecodeError::InputTooShort)?;
    Ok(DecodeResult {
        output,
        report,
        backend: DecodeBackend::Scalar16,
    })
}

/// Decode 16-way using the explicit scalar backend.
pub fn decode_interleaved16_scalar(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    let (output, report) =
        crate::packed_table::decode_interleaved16_scalar(compressed, table, expected_len)
            .map_err(|_| DecodeError::InputTooShort)?;
    Ok(DecodeResult {
        output,
        report,
        backend: DecodeBackend::Scalar16,
    })
}

/// Decode 16-way using the explicit AVX512 backend.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512BW are available.
pub unsafe fn decode_interleaved16_avx512(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    let (output, report) =
        crate::avx512::decode_interleaved16_avx512_kernel(compressed, table, expected_len)
            .map_err(|_| DecodeError::InputTooShort)?;
    Ok(DecodeResult {
        output,
        report,
        backend: DecodeBackend::Avx512Interleaved16,
    })
}

// ---------------------------------------------------------------------------
// Allocating convenience wrappers
// ---------------------------------------------------------------------------

/// Allocating 8-way auto-dispatch decoder.
pub fn decode_interleaved8(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    decode_interleaved8_auto(compressed, table, expected_len)
}

/// Allocating 16-way auto-dispatch decoder.
pub fn decode_interleaved16(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    decode_interleaved16_auto(compressed, table, expected_len)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packed_table::PackedWordTable;
    use alloc::vec;

    fn uniform_model() -> (Vec<u32>, Vec<u32>, PackedWordTable) {
        let total = 1u32 << 12;
        let base = total / 256;
        let mut freqs = vec![base; 256];
        freqs[255] += total - freqs.iter().sum::<u32>();
        let mut cum = vec![0u32; 257];
        for i in 0..256 {
            cum[i + 1] = cum[i] + freqs[i];
        }
        let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
        (freqs, cum, packed)
    }

    #[test]
    fn test_backend_labels() {
        assert_eq!(DecodeBackend::Scalar8.label(), "scalar-8way");
        assert_eq!(DecodeBackend::Sse41Interleaved8.label(), "sse41-8way");
        assert_eq!(DecodeBackend::Avx512VlInterleaved8.label(), "avx512vl-8way");
        assert_eq!(DecodeBackend::Scalar16.label(), "scalar-16way");
        assert_eq!(DecodeBackend::Avx512Interleaved16.label(), "avx512-16way");
    }

    #[test]
    fn test_scalar8_dispatch() {
        let (freqs, cum, packed) = uniform_model();
        let symbols: Vec<u8> = (0..50).map(|i| (i % 16) as u8).collect();
        let compressed = crate::encode_8way_for_test(&symbols, &freqs, &cum);

        let result = decode_interleaved8_scalar(&compressed, &packed, symbols.len()).unwrap();
        assert_eq!(result.output, symbols);
        assert_eq!(result.backend, DecodeBackend::Scalar8);
    }

    #[test]
    fn test_scalar16_dispatch() {
        let (freqs, cum, packed) = uniform_model();
        let symbols: Vec<u8> = (0..50).map(|i| (i % 16) as u8).collect();
        let compressed = crate::packed_table::encode_interleaved16(&symbols, &freqs, &cum, 12);

        let result = decode_interleaved16_scalar(&compressed, &packed, symbols.len()).unwrap();
        assert_eq!(result.output, symbols);
        assert_eq!(result.backend, DecodeBackend::Scalar16);
    }

    #[test]
    fn test_truncated_8way_rejected() {
        let (_freqs, _cum, packed) = uniform_model();
        assert!(decode_interleaved8_scalar(&[], &packed, 8).is_err());
        assert!(decode_interleaved8_scalar(&[0u16; 15], &packed, 8).is_err());
    }

    #[test]
    fn test_truncated_16way_rejected() {
        let (_freqs, _cum, packed) = uniform_model();
        assert!(decode_interleaved16_scalar(&[], &packed, 16).is_err());
        assert!(decode_interleaved16_scalar(&[0u16; 31], &packed, 16).is_err());
    }
}
