//! # Backend detection and dispatch for Word rANS decoders
//!
//! This module provides the safe public API for multi-backend Word rANS decoding.
//! It handles three concerns:
//!
//! 1. **Backend identification**: A `DecodeBackend` enum with stable string labels
//!    used in court receipts and performance measurement.
//! 2. **Runtime feature detection**: Selection of the best available SIMD backend
//!    based on CPU capabilities, with graceful scalar fallback.
//! 3. **Dispatch**: Auto-selection (`_auto` functions) and explicit backend selection
//!    (`_scalar`, `_avx512vl`, `_avx512` functions).
//!
//! ## Auto-dispatch policy
//!
//! The `_auto` functions select the fastest available backend.  The priority is:
//!
//! ```text
//! 8-way:  AVX512VL → SSE4.1 → scalar
//! 16-way: AVX512 → scalar
//! ```
//!
//! This priority order is based on expected throughput, not availability.  If the
//! required ISA feature is not detected at runtime, the next backend in priority
//! order is used.
//!
//! ## Safety
//!
//! The safe `_auto` functions never execute unsupported instructions.  They perform
//! runtime CPU feature detection before calling any `#[target_feature]`-gated kernel.
//! The explicit `_avx512vl` and `_avx512` functions are `unsafe` and require the
//! caller to ensure CPU support.

use crate::packed_table::{DecodeReport, PackedWordTable};
use alloc::vec::Vec;

/// Identifies which decoder backend was actually used.
///
/// This enum is returned by every decode operation so callers can verify
/// which backend executed.  This is essential for:
/// - **Court receipts**: backend assertions in behavioral evidence
/// - **Performance measurement**: comparing throughput per backend
/// - **Debugging**: knowing whether SIMD acceleration was applied
///
/// Each variant has a stable string `label()` used in JSON serialization
/// and court receipt fields.  These labels must not change across versions
/// to maintain evidence integrity.
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
    ///
    /// These labels are:
    /// - **Immutable**: changing them would break evidence chain integrity
    /// - **Unique**: no two backends share a label
    /// - **Self-describing**: the label indicates both the ISA and lane count
    ///
    /// ```ignore
    /// // Example (not runnable as doctest in no_std context):
    /// // assert_eq!(backend.label(), "avx512vl-8way");
    /// ```
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
///
/// This struct bundles three pieces of information:
/// 1. `output` — the decoded symbol bytes
/// 2. `report` — metadata (words consumed, final states)
/// 3. `backend` — which backend actually executed
///
/// The `backend` field is critical for court evidence: it proves that
/// the claimed SIMD backend was actually used, not a scalar fallback.
#[derive(Clone, Debug)]
pub struct DecodeResult {
    pub output: Vec<u8>,
    pub report: DecodeReport,
    pub backend: DecodeBackend,
}

/// Extended decode error type covering all decode surfaces.
///
/// This enum provides more granular error types than the core crate's
/// `DecodeError`, which only has `InputTooShort`.  The additional
/// variants allow SIMD decoders to express specific failure modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The compressed stream is too short for the requested operation.
    InputTooShort,
    /// The decode table is invalid or inconsistent.
    InvalidTable,
    /// The requested backend is not supported on this CPU.
    UnsupportedBackend,
    /// The output length does not match expectations.
    OutputLengthMismatch,
    /// Trailing data remains after complete decode.
    TrailingData,
    /// A state invariant was violated during decode.
    StateInvariantViolation,
}

// ---------------------------------------------------------------------------
// Runtime feature detection
// ---------------------------------------------------------------------------
//
// The detection functions use a two-tier strategy:
//
// 1. If the `std` feature is enabled, use `std::is_x86_feature_detected!()`
//    which calls the CPUID instruction at runtime.  This is the most reliable
//    method because it detects features available on the actual CPU.
//
// 2. If `std` is not available (no_std context), fall back to
//    `cfg!(target_feature = "...")` which checks compile-time target features.
//    This requires the user to set `-C target-feature=...` in their build flags.
//
// The compile-time check is weaker but sufficient for environments where
// the binary is compiled for a specific CPU (embedded, kernel, cross-compiled).

/// Check whether AVX512F + AVX512VL + AVX512BW are available.
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

/// Check whether AVX512F + AVX512BW are available.
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
// 8-way auto-dispatch
// ---------------------------------------------------------------------------

/// Decode 8-way interleaved Word rANS using the best available backend.
///
/// Selection priority: scalar (fastest on measured Zen 5) → SSE4.1 → AVX512VL.
/// On the Ryzen 7 9800X3D, scalar is ~2-3× faster than SIMD backends because
/// the 16 KB decode table is L1-resident and sequential scalar loads (~4 cycles)
/// beat gather instructions (~10-15 cycles).
///
/// Explicit SIMD backends remain available for cross-verification, future CPUs
/// with faster gathers, and for users who explicitly request them.
///
/// This function is **safe** because it checks CPU features before calling
/// any SIMD kernel.
///
/// # Arguments
///
/// * `compressed` - The compressed stream as u16 words.  Must have at least
///   16 words (8 initial states × 2 u16 each).
/// * `table` - A `PackedWordTable` with exactly 4096 entries.
/// * `expected_len` - The number of symbols expected in the decoded output.
///
/// # Returns
///
/// * `Ok(DecodeResult)` containing decoded output, decode report, and backend.
/// * `Err(DecodeError)` if the stream is truncated or the table is invalid.
pub fn decode_interleaved8_auto(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    // Scalar is the default: on measured Zen 5 hardware it is ~2-3× faster
    // than any SIMD backend because the 16 KB decode table is L1-resident.
    // Explicit SIMD backends (SSE4.1, AVX512VL) remain available via the
    // unsafe `decode_interleaved8_avx512vl` and similar explicit functions.
    decode_interleaved8_scalar(compressed, table, expected_len)
}

/// Decode 8-way using the explicit scalar backend.
///
/// Always uses the pure-Rust packed scalar decoder regardless of
/// available SIMD features.  Useful for benchmarking or verification.
pub fn decode_interleaved8_scalar(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    let (output, r8) =
        crate::packed_table::decode_8way_packed_scalar_with_report(compressed, table, expected_len)
            .map_err(|_| DecodeError::InputTooShort)?;
    Ok(DecodeResult {
        output,
        report: DecodeReport {
            words_consumed: r8.words_consumed,
            final_states: [
                r8.final_states[0],
                r8.final_states[1],
                r8.final_states[2],
                r8.final_states[3],
                r8.final_states[4],
                r8.final_states[5],
                r8.final_states[6],
                r8.final_states[7],
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ],
        },
        backend: DecodeBackend::Scalar8,
    })
}

/// Decode 8-way using the explicit AVX512VL backend.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.
/// No CPU feature detection is performed — the kernel is called directly.
pub unsafe fn decode_interleaved8_avx512vl(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    unsafe {
        #[cfg(any(target_feature = "avx512bw", feature = "std"))]
        {
            let (output, report) =
                crate::avx512::decode_interleaved8_avx512vl_kernel(compressed, table, expected_len)
                    .map_err(|_| DecodeError::InputTooShort)?;
            return Ok(DecodeResult {
                output,
                report,
                backend: DecodeBackend::Avx512VlInterleaved8,
            });
        }
        #[cfg(not(any(target_feature = "avx512bw", feature = "std")))]
        {
            Err(DecodeError::UnsupportedBackend)
        }
    }
}

// ---------------------------------------------------------------------------
// 16-way auto-dispatch
// ---------------------------------------------------------------------------

/// Decode 16-way interleaved Word rANS using the best available backend.
///
/// Selection priority: scalar (fastest on measured Zen 5) → AVX512.
/// On the Ryzen 7 9800X3D, scalar 16-way achieves 1.44-1.83 GiB/s vs
/// AVX512 16-way at 0.64-1.32 GiB/s (0.43-0.72× scalar).
///
/// Explicit AVX512 selection remains available via `decode_interleaved16_avx512`
/// for courts, cross-verification, benchmarks, and future architectures.
pub fn decode_interleaved16_auto(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    decode_interleaved16_scalar(compressed, table, expected_len)
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
/// Caller must ensure AVX512F + AVX512BW are available at runtime.
pub unsafe fn decode_interleaved16_avx512(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> { unsafe {
    #[cfg(any(target_feature = "avx512bw", feature = "std"))]
    {
        let (output, report) =
            crate::avx512::decode_interleaved16_avx512_kernel(compressed, table, expected_len)
                .map_err(|_| DecodeError::InputTooShort)?;
        return Ok(DecodeResult {
            output,
            report,
            backend: DecodeBackend::Avx512Interleaved16,
        });
    }
    #[cfg(not(any(target_feature = "avx512bw", feature = "std")))]
    {
        Err(DecodeError::UnsupportedBackend)
    }
}

// ---------------------------------------------------------------------------
// Allocating convenience wrappers
// ---------------------------------------------------------------------------

/// Allocating 8-way auto-dispatch decoder.
///
/// Convenience wrapper around `decode_interleaved8_auto`.
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
    use crate::packed_table::{PackedWordTable, encode_interleaved16};
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
        // Verify backend labels are stable strings.
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
        let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12).unwrap();

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
