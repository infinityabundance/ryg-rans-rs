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
//! ## Auto-dispatch architecture
//!
//! The auto-dispatch functions (`decode_interleaved8_auto` and
//! `decode_interleaved16_auto`) perform runtime CPU feature detection to select the
//! fastest available backend.  The dispatch is intentionally conservative:
//!
//! ```text
//! 8-way:  scalar (default, fastest on Zen 5) → SSE4.1 → AVX512VL
//! 16-way: scalar (default, fastest on Zen 5) → AVX2 → AVX512
//! ```
//!
//! ### Why is scalar the default?
//!
//! On the AMD Ryzen 7 9800X3D (Zen 5), the 16 KB packed decode table is
//! **L1-resident** (32 KB L1D cache).  Sequential scalar loads from L1
//! complete in ~4 cycles, while SIMD gather instructions (`VPGATHERDD`)
//! take ~10–15 cycles on this microarchitecture.  This makes the scalar
//! decoder ~2–3× faster than any SIMD backend for the 8-way format.
//!
//! Explicit SIMD backends remain available for:
//! - **Cross-verification**: comparing backend outputs for correctness.
//! - **Future CPUs**: microarchitectures with faster gather execution.
//! - **Users who prefer SIMD**: choosing a specific backend via `_checked`.
//! - **Court evidence**: asserting a specific backend in behavioral receipts.
//!
//! ## Runtime feature detection
//!
//! The detection functions use a **two-tier strategy**:
//!
//! 1. **`std` feature enabled**: Calls `std::is_x86_feature_detected!()` which
//!    executes the CPUID instruction at runtime.  This is the most reliable
//!    method because it detects features available on the actual CPU.
//! 2. **`no_std` context**: Falls back to `cfg!(target_feature = "...")` which
//!    checks compile-time target features from `-C target-feature=...` flags.
//!    This is sufficient for embedded or cross-compiled environments where
//!    the binary targets a known CPU.
//!
//! ### Detection functions
//!
//! | Function | Checks | Requires (std) | Requires (no_std) |
//! |----------|--------|----------------|-------------------|
//! | `avx2_available_checked()` | AVX2 | `avx2` | `target_feature = "avx2"` |
//! | `avx512vl_available_checked()` | AVX512F+VL+BW | `avx512f+avx512vl+avx512bw` | same (all three) |
//! | `sse41_available_checked()` | SSE4.1 | `sse4.1` | `target_feature = "sse4.1"` |
//! | `avx512_available_checked()` | AVX512F+BW | `avx512f+avx512bw` | same (both) |
//!
//! The `_checked` wrappers are `#[doc(hidden)]` and exposed publicly for
//! benchmark tools to query feature support.
//!
//! ## Checked wrapper architecture
//!
//! Each AVX2 backend has a corresponding **safe `_checked` wrapper**:
//!
//! 1. Call `avx2_available()` (runtime detection).
//! 2. If AVX2 is available, build the renormalization permutation table
//!    (`build_avx2_renorm_table()`).
//! 3. Call the `unsafe` SIMD kernel inside an `unsafe { }` block (safe because
//!    we verified AVX2 support in step 1).
//! 4. Wrap the result in a `DecodeResult` with the correct `DecodeBackend` label.
//! 5. If AVX2 is not available, return `Err(DecodeError::UnsupportedBackend)`.
//!
//! The SSE4.1 and AVX-512 `_checked` wrappers follow the identical pattern:
//! runtime feature detection first, then the `unsafe` kernel, then
//! `Err(UnsupportedBackend)` when the CPU cannot execute the instructions.
//! These wrappers make the exact-backend semantics expressible from safe
//! code: a caller that receives `Err(UnsupportedBackend)` knows the kernel
//! was **not** executed — there is no silent substitution.
//!
//! | Wrapper | Inner kernel | Backend variant |
//! |---------|-------------|-----------------|
//! | `decode_interleaved8_avx2_manual_gather_checked` | `decode_interleaved8_avx2_manual_gather` | `Avx2ManualGather8` |
//! | `decode_interleaved8_avx2_hardware_gather_checked` | `decode_interleaved8_avx2_hardware_gather` | `Avx2HardwareGather8` |
//! | `decode_interleaved16_avx2_2x8_checked` | `decode_interleaved16_avx2_2x8` | `Avx2TwoBy8On16` |
//! | `decode_interleaved16_uniform256_avx2_checked` | `decode_interleaved16_uniform256_avx2` | `Avx2Uniform256TableFree16` |
//! | `decode_batch4_interleaved16_avx2_checked` | `decode_batch4_interleaved16_avx2` | `Avx2Batch4On16` |
//! | `decode_interleaved8_sse41_checked` | `decode_simd_8way_unchecked` | `Sse41Interleaved8` |
//! | `decode_interleaved8_avx512vl_checked` | `decode_interleaved8_avx512vl` | `Avx512VlInterleaved8` |
//! | `decode_interleaved16_avx512_checked` | `decode_interleaved16_avx512` | `Avx512Interleaved16` |
//! | `decode_interleaved8_avx512vl_manual_gather_checked` | `decode_interleaved8_manual_gather` | `Avx512VlManualGather8` |
//! | `decode_interleaved16_avx512_manual_gather_checked` | `decode_interleaved16_manual_gather` | `Avx512ManualGather16` |
//! | `decode_interleaved16_avx512vl_2x8_checked` | `decode_interleaved16_2x8` | `Avx512Vl2x8On16` |
//!
//! ## DecodeResult and DecodeReport
//!
//! - `DecodeResult` bundles the decoded output (`Vec<u8>`), a `DecodeReport`
//!   (words consumed + 16 final states), and the `DecodeBackend` identity.
//!   The backend field is critical for court evidence — it proves that a
//!   specific SIMD backend was actually used.
//! - `DecodeReport` is returned by the `_into` variants and contains only
//!   the metadata (no output vector).  This allows callers who manage their
//!   own buffers to avoid the allocation overhead of `DecodeResult`.
//!
//! ## Error handling
//!
//! The `DecodeError` enum provides backend-aware error types:
//! - `InputTooShort`: The compressed stream is truncated.
//! - `InvalidTable`: The decode table is malformed.
//! - `UnsupportedBackend`: The requested SIMD ISA is not available.
//! - `OutputLengthMismatch`: The output buffer size doesn't match the
//!   expected decode count.
//! - `TrailingData`: Extra data remains after complete decode.
//! - `StateInvariantViolation`: A rANS state invariant was violated.
//!
//! ## check_uniform256 helper
//!
//! The `check_uniform256` function tests whether a frequency model represents
//! a **Uniform256** distribution: all 256 symbols have frequency 16, and
//! `scale_bits == 12`.  This is a fast-path check that iterates over the
//! first 256 frequency entries (1024 bytes) and verifies each u32 equals 16.
//! When true, the Uniform256 table-free decoder can be used, which avoids
//! all table lookups by computing `symbol = slot >> 4` and `bias = slot & 15`.
//!
//! ## Safety
//!
//! The safe `_auto` and `_checked` functions never execute unsupported
//! instructions.  They perform runtime CPU feature detection before calling
//! any `#[target_feature]`-gated kernel.  The explicit `_avx512vl` and
//! `_avx512` functions are `unsafe` and require the caller to ensure CPU
//! support.

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
    /// AVX512VL 8-way with scalar table loads (manual gather).
    Avx512VlManualGather8,
    /// AVX-512 16-way with scalar table loads (manual gather).
    Avx512ManualGather16,
    /// AVX512VL two-vector interleaved16 (2 × 256-bit on 16-way format).
    Avx512Vl2x8On16,
    /// AVX2 8-way manual gather (scalar table loads into vector).
    Avx2ManualGather8,
    /// AVX2 8-way hardware gather (`_mm256_i32gather_epi32`).
    Avx2HardwareGather8,
    /// AVX2 two-vector interleaved16 (2 × 256-bit on 16-way format).
    Avx2TwoBy8On16,
    /// AVX2 Uniform256 table-free 16-way decode.
    Avx2Uniform256TableFree16,
    /// AVX2 batched decode of four independent 16-way streams.
    Avx2Batch4On16,
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
            DecodeBackend::Avx512VlManualGather8 => "avx512vl-manual-gather-8way",
            DecodeBackend::Avx512ManualGather16 => "avx512-manual-gather-16way",
            DecodeBackend::Avx512Vl2x8On16 => "avx512vl-2x8-on16",
            DecodeBackend::Avx2ManualGather8 => "avx2-manual-gather-8way",
            DecodeBackend::Avx2HardwareGather8 => "avx2-hardware-gather-8way",
            DecodeBackend::Avx2TwoBy8On16 => "avx2-2x8-on16",
            DecodeBackend::Avx2Uniform256TableFree16 => "avx2-uniform256-tablefree-16way",
            DecodeBackend::Avx2Batch4On16 => "avx2-batch4-on16",
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

/// Public runtime detection wrappers for benchmark use.
#[doc(hidden)]
pub fn avx2_available_checked() -> bool {
    avx2_available()
}

/// Public runtime detection wrappers for benchmark use.
#[doc(hidden)]
pub fn avx512vl_available_checked() -> bool {
    avx512vl_available()
}

/// Check whether SSE4.1 is available at runtime.
fn sse41_available() -> bool {
    #[cfg(feature = "std")]
    {
        std::is_x86_feature_detected!("sse4.1")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(target_feature = "sse4.1")
    }
}

/// Public runtime detection wrapper for SSE4.1.
#[doc(hidden)]
pub fn sse41_available_checked() -> bool {
    sse41_available()
}

/// Public runtime detection wrapper for AVX-512.
#[doc(hidden)]
pub fn avx512_available_checked() -> bool {
    avx512_available()
}

/// Check whether a frequency model is Uniform256.
///
/// Returns true if `scale_bits == 12` and every 4-byte chunk in `model_data`
/// equals 16 (u32 LE).
pub fn check_uniform256(model_data: &[u8], scale_bits: u8) -> bool {
    if scale_bits != 12 || model_data.len() < 1024 {
        return false;
    }
    for chunk in model_data.chunks_exact(4).take(256) {
        let f = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if f != 16 {
            return false;
        }
    }
    true
}

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

/// Check whether AVX2 is available at runtime.
///
/// When `std` is enabled, uses `is_x86_feature_detected!("avx2")` to query
/// CPUID.  When `std` is not available, falls back to compile-time
/// `cfg!(target_feature = "avx2")`.
fn avx2_available() -> bool {
    #[cfg(feature = "std")]
    {
        std::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(target_feature = "avx2")
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

/// Decode 8-way using the explicit AVX512VL backend into a preallocated buffer.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.
/// `output.len()` must equal the number of symbols to decode.
///
/// The kernel body is compile-time gated on `target_feature = "avx512bw"`:
/// in builds without it, this returns `UnsupportedBackend` and the inputs
/// are acknowledged (not silently ignored).
pub unsafe fn decode_interleaved8_avx512vl_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: this is an `unsafe fn`; the caller guarantees AVX512F +
        // AVX512VL + AVX512BW at runtime (see fn docs).
        unsafe {
            let report =
                crate::avx512::decode_interleaved8_avx512vl_into(compressed, table, output)
                    .map_err(|_| DecodeError::InputTooShort)?;
            Ok(report)
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, output);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 8-way using manual gather (scalar loads + vector arithmetic).
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.
pub unsafe fn decode_interleaved8_manual_gather(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + VL + BW at runtime.
        unsafe {
            let (output, report) = crate::avx512::decode_interleaved8_manual_gather_kernel(
                compressed,
                table,
                expected_len,
            )
            .map_err(|_| DecodeError::InputTooShort)?;
            Ok(DecodeResult {
                output,
                report,
                backend: DecodeBackend::Avx512VlManualGather8,
            })
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, expected_len);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 8-way using manual gather into a preallocated buffer.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.
pub unsafe fn decode_interleaved8_manual_gather_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + VL + BW at runtime.
        unsafe {
            crate::avx512::decode_interleaved8_manual_gather_into(compressed, table, output)
                .map_err(|_| DecodeError::InputTooShort)
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, output);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 16-way using manual gather (scalar loads + vector arithmetic).
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512BW are available at runtime.
pub unsafe fn decode_interleaved16_manual_gather(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + BW at runtime.
        unsafe {
            let (output, report) = crate::avx512::decode_interleaved16_manual_gather_kernel(
                compressed,
                table,
                expected_len,
            )
            .map_err(|_| DecodeError::InputTooShort)?;
            Ok(DecodeResult {
                output,
                report,
                backend: DecodeBackend::Avx512ManualGather16,
            })
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, expected_len);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 16-way using manual gather into a preallocated buffer.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512BW are available at runtime.
pub unsafe fn decode_interleaved16_manual_gather_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + BW at runtime.
        unsafe {
            crate::avx512::decode_interleaved16_manual_gather_into(compressed, table, output)
                .map_err(|_| DecodeError::InputTooShort)
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, output);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 16-way format using two independent 256-bit gather chains (2x8).
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.
pub unsafe fn decode_interleaved16_2x8(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + VL + BW at runtime.
        unsafe {
            let (output, report) =
                crate::avx512::decode_interleaved16_2x8_kernel(compressed, table, expected_len)
                    .map_err(|_| DecodeError::InputTooShort)?;
            Ok(DecodeResult {
                output,
                report,
                backend: DecodeBackend::Avx512Vl2x8On16,
            })
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, expected_len);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 16-way 2x8 into a preallocated buffer.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.
pub unsafe fn decode_interleaved16_2x8_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + VL + BW at runtime.
        unsafe {
            crate::avx512::decode_interleaved16_2x8_into(compressed, table, output)
                .map_err(|_| DecodeError::InputTooShort)
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, output);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 8-way using the explicit AVX512VL backend.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.
/// No CPU feature detection is performed — the kernel is called directly.
///
/// The kernel body is compile-time gated on `target_feature = "avx512bw"`;
/// in builds without it this returns `UnsupportedBackend`.
pub unsafe fn decode_interleaved8_avx512vl(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + VL + BW at runtime.
        unsafe {
            let (output, report) =
                crate::avx512::decode_interleaved8_avx512vl_kernel(compressed, table, expected_len)
                    .map_err(|_| DecodeError::InputTooShort)?;
            Ok(DecodeResult {
                output,
                report,
                backend: DecodeBackend::Avx512VlInterleaved8,
            })
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, expected_len);
        Err(DecodeError::UnsupportedBackend)
    }
}

// ---------------------------------------------------------------------------
// 16-way auto-dispatch
// ---------------------------------------------------------------------------

/// Decode 16-way interleaved Word rANS using the best available backend.
///
/// Selection priority: scalar (fastest on measured Zen 5) → AVX2 2×8 → AVX512.
/// Available explicit backends: AVX2 manual/hardware gather 8-way, AVX2 2×8,
/// AVX2 Uniform256 table-free, AVX2 batch4, AVX512VL 8-way, AVX512 16-way.
///
/// On the Ryzen 7 9800X3D, scalar 16-way achieves 1.44-1.83 GiB/s vs
/// AVX512 16-way at 0.64-1.32 GiB/s (0.43-0.72× scalar).  AVX2 2×8 has
/// not yet been benchmarked on this platform.
///
/// **Auto-dispatch is intentionally conservative.**  The default is scalar
/// until architecture-specific performance data supports changing it.
/// Use explicit AVX2 backends (`decode_interleaved16_avx2_2x8_checked`,
/// `decode_interleaved16_uniform256_avx2_checked`) for benchmarking and
/// courts where you want to assert exact backend identity.
///
/// Explicit backends remain available for cross-verification, future CPUs
/// with faster gathers, and for users who explicitly request them.
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

/// Decode 16-way using the explicit AVX512 backend into a preallocated buffer.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512BW are available at runtime.
/// `output.len()` must equal the number of symbols to decode.
///
/// The kernel body is compile-time gated on `target_feature = "avx512bw"`;
/// in builds without it this returns `UnsupportedBackend`.
pub unsafe fn decode_interleaved16_avx512_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: this is an `unsafe fn`; the caller guarantees AVX512F +
        // AVX512BW at runtime (see fn docs).
        unsafe {
            let report = crate::avx512::decode_interleaved16_avx512_into(compressed, table, output)
                .map_err(|_| DecodeError::InputTooShort)?;
            Ok(report)
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, output);
        Err(DecodeError::UnsupportedBackend)
    }
}

/// Decode 16-way using the explicit AVX512 backend.
///
/// # Safety
///
/// Caller must ensure AVX512F + AVX512BW are available at runtime.
///
/// The kernel body is compile-time gated on `target_feature = "avx512bw"`;
/// in builds without it this returns `UnsupportedBackend`.
pub unsafe fn decode_interleaved16_avx512(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    #[cfg(target_feature = "avx512bw")]
    {
        // SAFETY: caller guarantees AVX512F + BW at runtime.
        unsafe {
            let (output, report) =
                crate::avx512::decode_interleaved16_avx512_kernel(compressed, table, expected_len)
                    .map_err(|_| DecodeError::InputTooShort)?;
            Ok(DecodeResult {
                output,
                report,
                backend: DecodeBackend::Avx512Interleaved16,
            })
        }
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        let _ = (compressed, table, expected_len);
        Err(DecodeError::UnsupportedBackend)
    }
}

// ---------------------------------------------------------------------------
// AVX2 safe wrappers
// ---------------------------------------------------------------------------

/// Safe wrapper for AVX2 manual-gather 8-way decode.
///
/// Checks AVX2 runtime support before executing.  If AVX2 is not available
/// at runtime, returns `UnsupportedBackend`.
pub fn decode_interleaved8_avx2_manual_gather_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx2_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    let perm_table = crate::avx2_renorm::build_avx2_renorm_table();
    unsafe {
        let (output, report) = crate::avx2::decode_interleaved8_avx2_manual_gather(
            compressed,
            table,
            expected_len,
            &perm_table,
        )
        .map_err(|_| DecodeError::InputTooShort)?;
        Ok(DecodeResult {
            output,
            report,
            backend: DecodeBackend::Avx2ManualGather8,
        })
    }
}

/// Safe wrapper for AVX2 hardware-gather 8-way decode.
pub fn decode_interleaved8_avx2_hardware_gather_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx2_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    let perm_table = crate::avx2_renorm::build_avx2_renorm_table();
    unsafe {
        let (output, report) = crate::avx2::decode_interleaved8_avx2_hardware_gather(
            compressed,
            table,
            expected_len,
            &perm_table,
        )
        .map_err(|_| DecodeError::InputTooShort)?;
        Ok(DecodeResult {
            output,
            report,
            backend: DecodeBackend::Avx2HardwareGather8,
        })
    }
}

/// Safe wrapper for AVX2 2×8 sixteen-way decode.
pub fn decode_interleaved16_avx2_2x8_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx2_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    let perm_table = crate::avx2_renorm::build_avx2_renorm_table();
    unsafe {
        let (output, report) = crate::avx2::decode_interleaved16_avx2_2x8(
            compressed,
            table,
            expected_len,
            &perm_table,
        )
        .map_err(|_| DecodeError::InputTooShort)?;
        Ok(DecodeResult {
            output,
            report,
            backend: DecodeBackend::Avx2TwoBy8On16,
        })
    }
}

/// Safe wrapper for AVX2 Uniform256 table-free sixteen-way decode.
///
/// Caller must validate that the model is Uniform256 before calling this.
pub fn decode_interleaved16_uniform256_avx2_checked(
    compressed: &[u16],
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx2_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    let perm_table = crate::avx2_renorm::build_avx2_renorm_table();
    unsafe {
        let (output, report) = crate::avx2::decode_interleaved16_uniform256_avx2(
            compressed,
            expected_len,
            &perm_table,
        )
        .map_err(|_| DecodeError::InputTooShort)?;
        Ok(DecodeResult {
            output,
            report,
            backend: DecodeBackend::Avx2Uniform256TableFree16,
        })
    }
}

/// Safe wrapper for AVX2 batch-four sixteen-way decode.
///
/// Checks AVX2 runtime support before executing.  Each job's output must
/// be pre-sized to its expected decoded length.
pub fn decode_batch4_interleaved16_avx2_checked(
    jobs: &mut [crate::avx2::Avx2DecodeJob<'_>],
) -> Result<Vec<DecodeResult>, DecodeError> {
    if !avx2_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    let perm_table = crate::avx2_renorm::build_avx2_renorm_table();
    unsafe {
        let reports = crate::avx2::decode_batch4_interleaved16_avx2(jobs, &perm_table)
            .map_err(|_| DecodeError::InputTooShort)?;

        let results: Vec<DecodeResult> = reports
            .into_iter()
            .enumerate()
            .map(|(idx, report)| {
                let job = &jobs[idx];
                DecodeResult {
                    output: job.output.to_vec(),
                    report,
                    backend: DecodeBackend::Avx2Batch4On16,
                }
            })
            .collect();
        Ok(results)
    }
}

/// Safe wrapper for SSE4.1 8-way decode (SSSE3 + SSE4.1).
///
/// Checks runtime support before executing.  The kernel is the classic
/// `decode_simd_8way_unchecked` (4-lane × 2-state SSE4.1 pipeline) operating
/// on `RansWordTables`.  If the CPU lacks SSSE3 or SSE4.1 at runtime,
/// returns `UnsupportedBackend` — the kernel is never executed.
///
/// This wrapper exists so the exact-backend contract can be expressed from
/// safe code: `Ok` implies `Sse41Interleaved8` actually executed;
/// `Err(UnsupportedBackend)` means it did not.
pub fn decode_interleaved8_sse41_checked(
    compressed: &[u16],
    tables: &crate::RansWordTables<'_>,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !sse41_with_ssse3_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    // SAFETY: runtime detection above guarantees SSSE3 + SSE4.1 are
    // available; `decode_simd_8way_unchecked` carries its own
    // `#[target_feature(enable = "ssse3,sse4.1")]` attributes.
    let output = unsafe { crate::decode_simd_8way_unchecked(compressed, tables, expected_len) }
        .map_err(|_| DecodeError::InputTooShort)?;
    Ok(DecodeResult {
        output,
        // The SSE4.1 kernel does not surface a report; zeros mean "not
        // available", mirroring the convention used elsewhere in this crate.
        report: DecodeReport {
            words_consumed: 0,
            final_states: [0u32; 16],
        },
        backend: DecodeBackend::Sse41Interleaved8,
    })
}

/// Check whether SSSE3 + SSE4.1 are available at runtime (or at compile
/// time in no_std builds).
fn sse41_with_ssse3_available() -> bool {
    #[cfg(feature = "std")]
    {
        std::is_x86_feature_detected!("ssse3") && std::is_x86_feature_detected!("sse4.1")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(all(target_feature = "ssse3", target_feature = "sse4.1"))
    }
}

/// Safe wrapper for AVX-512VL interleaved 8-way decode.
///
/// Runtime-checks AVX512F + AVX512VL + AVX512BW before executing.  If the
/// CPU lacks the features, or the build was not compiled with
/// `-C target-feature=+avx512f,+avx512vl,+avx512bw` (the kernel body is
/// compile-time gated), returns `Err(UnsupportedBackend)`.  No fallback:
/// `Ok` means `Avx512VlInterleaved8` actually executed.
pub fn decode_interleaved8_avx512vl_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx512vl_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    // SAFETY: runtime detection above guarantees AVX512F + VL + BW.
    unsafe { decode_interleaved8_avx512vl(compressed, table, expected_len) }
}

/// Safe wrapper for AVX-512 interleaved 16-way decode.
///
/// Runtime-checks AVX512F + AVX512BW.  See
/// [`decode_interleaved8_avx512vl_checked`] for the exact-backend contract.
pub fn decode_interleaved16_avx512_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx512_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    // SAFETY: runtime detection above guarantees AVX512F + BW.
    unsafe { decode_interleaved16_avx512(compressed, table, expected_len) }
}

/// Safe wrapper for AVX-512VL manual-gather 8-way decode.
///
/// Runtime-checks AVX512F + AVX512VL + AVX512BW.  See
/// [`decode_interleaved8_avx512vl_checked`] for the exact-backend contract.
pub fn decode_interleaved8_avx512vl_manual_gather_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx512vl_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    // SAFETY: runtime detection above guarantees AVX512F + VL + BW.
    unsafe { decode_interleaved8_manual_gather(compressed, table, expected_len) }
}

/// Safe wrapper for AVX-512 manual-gather 16-way decode.
///
/// Runtime-checks AVX512F + AVX512BW.  See
/// [`decode_interleaved8_avx512vl_checked`] for the exact-backend contract.
pub fn decode_interleaved16_avx512_manual_gather_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx512_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    // SAFETY: runtime detection above guarantees AVX512F + BW.
    unsafe { decode_interleaved16_manual_gather(compressed, table, expected_len) }
}

/// Safe wrapper for AVX-512VL 2×8-on-16-way decode.
///
/// Runtime-checks AVX512F + AVX512VL + AVX512BW.  See
/// [`decode_interleaved8_avx512vl_checked`] for the exact-backend contract.
pub fn decode_interleaved16_avx512vl_2x8_checked(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<DecodeResult, DecodeError> {
    if !avx512vl_available() {
        return Err(DecodeError::UnsupportedBackend);
    }
    // SAFETY: runtime detection above guarantees AVX512F + VL + BW.
    unsafe { decode_interleaved16_2x8(compressed, table, expected_len) }
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
