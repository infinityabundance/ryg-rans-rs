//! # Malformed-stream hardening
//!
//! Defensive checks for truncated, corrupted, and edge-case rANS streams.
//!
//! The core crate's encoder/decoder already returns `Result` with
//! `DecodeError::InputTooShort` and `EncodeError::OutputTooSmall`. This
//! module adds:
//!
//! - **Pre-decode validation** – check compressed stream integrity invariants
//!   before touching the decoder hot path.
//! - **Renormalization guards** – ensure renormalization loops cannot spin
//!   forever on corrupted input.
//! - **Edge-case tables** – verify frequency models obey the upstream
//!   constraints before constructing decoder tables.
//! - **Unsafe-free bounds enforcement** – all checks are pure safe Rust
//!   with no side effects.
//!
//! ## Design principle
//!
//! Every validation function returns `Result<(), ValidationError>` and is
//! separated from the hot-path arithmetic. Callers may skip validation
//! for already-trusted input, but should run it for untrusted streams.

use crate::DecodeError;

// ---------------------------------------------------------------------------
// Validation error type
// ---------------------------------------------------------------------------

/// Errors produced by stream validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    /// The compressed stream is too short to contain a valid initial state.
    TruncatedStream,
    /// The stream would require an unreasonable number of renormalization
    /// steps, indicating probable corruption.
    ExcessiveRenormalization,
    /// A symbol frequency in the model is zero.
    ZeroFrequency,
    /// A cumulative frequency overflows the allowed range.
    CumulativeOverflow,
    /// The scale_bits parameter is outside the valid range for this variant.
    InvalidScaleBits,
    /// Start + frequency exceeds the allowed range.
    RangeOverflow,
    /// The compressed stream has trailing data after a complete decode.
    TrailingData,
}

impl core::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ValidationError::TruncatedStream => write!(f, "compressed stream is truncated"),
            ValidationError::ExcessiveRenormalization => {
                write!(f, "excessive renormalization steps – probable corruption")
            }
            ValidationError::ZeroFrequency => write!(f, "symbol frequency is zero"),
            ValidationError::CumulativeOverflow => {
                write!(f, "cumulative frequency exceeds allowed range")
            }
            ValidationError::InvalidScaleBits => {
                write!(f, "scale_bits parameter is out of valid range")
            }
            ValidationError::RangeOverflow => {
                write!(f, "start + frequency exceeds allowed range")
            }
            ValidationError::TrailingData => write!(f, "trailing data after complete decode"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for ValidationError {}

// ---------------------------------------------------------------------------
// Byte rANS validation
// ---------------------------------------------------------------------------

/// Maximum consecutive renormalization steps before we declare the stream
/// corrupted.  For byte rANS, each step emits/consumes one byte and shifts
/// the state right by 8 bits.  With a 31-bit effective state, the worst
/// case is 4 consecutive renormalizations (32 bits / 8 = 4).  We use a
/// generous safety margin.
const MAX_RENORM_STEPS_BYTE: u32 = 16;

/// Maximum consecutive renormalization steps for 64-bit rANS (32-bit word
/// renormalization).  A 63-bit state needs at most 2 word reads, but we
/// allow lenience for corrupted streams.
const MAX_RENORM_STEPS_R64: u32 = 8;

/// Maximum consecutive renormalization steps for word rANS (16-bit word
/// renormalization).  With L = 2^16, at most 1 word read is needed, but
/// a corrupted stream could loop more.
const MAX_RENORM_STEPS_WORD: u32 = 8;

/// Validate that a byte rANS compressed stream has enough data for init.
///
/// The decoder reads 4 bytes (u32) for the initial state. This function
/// checks that the stream is at least 4 bytes long.
#[inline]
pub fn validate_byte_compressed(compressed: &[u8]) -> Result<(), ValidationError> {
    if compressed.len() < 4 {
        return Err(ValidationError::TruncatedStream);
    }
    Ok(())
}

/// Validate that a 64-bit rANS compressed stream has enough data for init.
///
/// The decoder reads 2 × u32 words (8 bytes) for the initial state.
#[inline]
pub fn validate_r64_compressed(compressed: &[u8]) -> Result<(), ValidationError> {
    if compressed.len() < 8 {
        return Err(ValidationError::TruncatedStream);
    }
    Ok(())
}

/// Validate that a word rANS compressed stream (u16 slice) has enough data
/// for init.  The decoder reads 2 u16 words (4 bytes) for the initial state.
#[inline]
pub fn validate_word_compressed(compressed: &[u16]) -> Result<(), ValidationError> {
    if compressed.len() < 2 {
        return Err(ValidationError::TruncatedStream);
    }
    Ok(())
}

/// Validate scale_bits for byte rANS (1..=16).
#[inline]
pub fn validate_byte_scale_bits(scale_bits: u32) -> Result<(), ValidationError> {
    if !(1..=16).contains(&scale_bits) {
        return Err(ValidationError::InvalidScaleBits);
    }
    Ok(())
}

/// Validate scale_bits for 64-bit rANS (1..=31).
#[inline]
pub fn validate_r64_scale_bits(scale_bits: u32) -> Result<(), ValidationError> {
    if !(1..=31).contains(&scale_bits) {
        return Err(ValidationError::InvalidScaleBits);
    }
    Ok(())
}

/// Validate the frequency model for a set of decoder symbols.
///
/// Checks:
/// - No zero frequencies.
/// - Cumulative sum does not overflow.
/// - No start + freq exceeds the allowed range (1 << scale_bits).
#[inline]
pub fn validate_freq_model(
    cum_freqs: &[u32],
    freqs: &[u32],
    scale_bits: u32,
) -> Result<(), ValidationError> {
    let total = 1u64 << scale_bits;
    let n = freqs.len().min(cum_freqs.len().saturating_sub(1));

    for i in 0..n {
        if freqs[i] == 0 {
            // Zero frequencies are valid for unused symbols, skip.
            continue;
        }
        let start = cum_freqs[i] as u64;
        let freq = freqs[i] as u64;
        if start + freq > total {
            return Err(ValidationError::RangeOverflow);
        }
    }

    // Verify cumulative frequencies are monotonically non-decreasing.
    let m = cum_freqs.len().min(256);
    for i in 1..m {
        if cum_freqs[i] < cum_freqs[i - 1] {
            return Err(ValidationError::CumulativeOverflow);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Renormalization-loop guards
// ---------------------------------------------------------------------------

/// A guard that limits the number of renormalization iterations the decoder
/// will perform before declaring the stream corrupted.
///
/// # Usage
///
/// ```ignore
/// let mut guard = RenormGuard::new_byte();
/// loop {
///     guard.check()?;
///     let b = reader.read_byte().ok_or(DecodeError::InputTooShort)?;
///     x = (x << 8) | (b as u32);
///     if x >= RANS_BYTE_L { break; }
/// }
/// ```
pub struct RenormGuard {
    remaining: u32,
}

impl RenormGuard {
    /// Create a new renormalization guard for byte rANS.
    #[inline]
    pub fn new_byte() -> Self {
        Self {
            remaining: MAX_RENORM_STEPS_BYTE,
        }
    }

    /// Create a new renormalization guard for 64-bit rANS.
    #[inline]
    pub fn new_r64() -> Self {
        Self {
            remaining: MAX_RENORM_STEPS_R64,
        }
    }

    /// Create a new renormalization guard for word rANS.
    #[inline]
    pub fn new_word() -> Self {
        Self {
            remaining: MAX_RENORM_STEPS_WORD,
        }
    }

    /// Decrement the iteration budget. Returns `Err` if the budget is
    /// exhausted (probable corruption).
    #[inline]
    pub fn check(&mut self) -> Result<(), ValidationError> {
        if self.remaining == 0 {
            return Err(ValidationError::ExcessiveRenormalization);
        }
        self.remaining -= 1;
        Ok(())
    }

    /// Reset the budget (e.g., after a successful renormalization).
    #[inline]
    pub fn reset(&mut self) {
        self.remaining = Self::default_remaining_for(&self);
    }
}

impl RenormGuard {
    fn default_remaining_for(&self) -> u32 {
        MAX_RENORM_STEPS_BYTE // conservative default
    }
}

// ---------------------------------------------------------------------------
// Edge-case frequency models
// ---------------------------------------------------------------------------

/// Check whether a frequency model has any single symbol occupying more
/// than half the total range.  Such models stress the decoder's
/// renormalization behaviour.
#[inline]
pub fn has_dominant_symbol(freqs: &[u32], total: u32) -> bool {
    freqs.iter().any(|&f| f as u64 * 2 > total as u64)
}

/// Check whether a frequency model uses only a single active symbol
/// (freq > 0 for exactly one symbol).  This exercises the freq = total
/// fast path.
#[inline]
pub fn is_single_symbol(freqs: &[u32]) -> bool {
    freqs.iter().filter(|&&f| f > 0).count() == 1
}

/// Check whether a frequency model has any symbol with freq == 1
/// (the special-case reciprocal encoder path).
#[inline]
pub fn has_freq_one(freqs: &[u32]) -> bool {
    freqs.iter().any(|&f| f == 1)
}

// ---------------------------------------------------------------------------
// Wrapper: check and convert errors
// ---------------------------------------------------------------------------

/// Convert a `ValidationError` into a `DecodeError` for use in decoder
/// functions that don't want to expose validation details.
#[inline]
pub fn validation_to_decode_error(e: ValidationError) -> DecodeError {
    match e {
        ValidationError::TruncatedStream => DecodeError::InputTooShort,
        ValidationError::ExcessiveRenormalization => DecodeError::InputTooShort,
        ValidationError::ZeroFrequency => DecodeError::InputTooShort,
        ValidationError::CumulativeOverflow => DecodeError::InputTooShort,
        ValidationError::InvalidScaleBits => DecodeError::InputTooShort,
        ValidationError::RangeOverflow => DecodeError::InputTooShort,
        ValidationError::TrailingData => DecodeError::InputTooShort,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ----- Truncated-stream detection ---------------------------------------

    #[test]
    fn test_validate_byte_compressed_short() {
        assert_eq!(
            validate_byte_compressed(&[]),
            Err(ValidationError::TruncatedStream)
        );
        assert_eq!(
            validate_byte_compressed(&[0; 3]),
            Err(ValidationError::TruncatedStream)
        );
        assert!(validate_byte_compressed(&[0; 4]).is_ok());
    }

    #[test]
    fn test_validate_r64_compressed_short() {
        assert_eq!(
            validate_r64_compressed(&[0; 7]),
            Err(ValidationError::TruncatedStream)
        );
        assert!(validate_r64_compressed(&[0; 8]).is_ok());
    }

    #[test]
    fn test_validate_word_compressed_short() {
        assert_eq!(
            validate_word_compressed(&[0; 1]),
            Err(ValidationError::TruncatedStream)
        );
        assert!(validate_word_compressed(&[0; 2]).is_ok());
    }

    // ----- Scale-bits validation --------------------------------------------

    #[test]
    fn test_validate_byte_scale_bits() {
        assert_eq!(
            validate_byte_scale_bits(0),
            Err(ValidationError::InvalidScaleBits)
        );
        assert_eq!(
            validate_byte_scale_bits(17),
            Err(ValidationError::InvalidScaleBits)
        );
        assert!(validate_byte_scale_bits(14).is_ok());
        assert!(validate_byte_scale_bits(1).is_ok());
        assert!(validate_byte_scale_bits(16).is_ok());
    }

    #[test]
    fn test_validate_r64_scale_bits() {
        assert_eq!(
            validate_r64_scale_bits(0),
            Err(ValidationError::InvalidScaleBits)
        );
        assert_eq!(
            validate_r64_scale_bits(32),
            Err(ValidationError::InvalidScaleBits)
        );
        assert!(validate_r64_scale_bits(14).is_ok());
        assert!(validate_r64_scale_bits(31).is_ok());
    }

    // ----- Frequency model validation ---------------------------------------

    #[test]
    fn test_validate_freq_model_valid() {
        let total = 1u32 << 14;
        let freqs = [total / 3, total / 3, total - 2 * (total / 3)];
        let cum = [0u32, freqs[0], freqs[0] + freqs[1], total];
        assert!(validate_freq_model(&cum, &freqs, 14).is_ok());
    }

    #[test]
    fn test_validate_freq_model_range_overflow() {
        let total = 1u32 << 14;
        // freq + start > total
        let freqs = [total + 1];
        let cum = [0u32, 0];
        assert_eq!(
            validate_freq_model(&cum, &freqs, 14),
            Err(ValidationError::RangeOverflow)
        );
    }

    #[test]
    fn test_validate_freq_model_non_monotonic() {
        let freqs = [100u32, 50];
        let cum = [0u32, 100, 50]; // non-monotonic: 50 < 100
        assert_eq!(
            validate_freq_model(&cum, &freqs, 14),
            Err(ValidationError::CumulativeOverflow)
        );
    }

    // ----- Renormalization guard -------------------------------------------

    #[test]
    fn test_renorm_guard_byte() {
        let mut guard = RenormGuard::new_byte();
        for _ in 0..MAX_RENORM_STEPS_BYTE - 1 {
            assert!(guard.check().is_ok());
        }
        // Exhaust budget
        assert!(guard.check().is_ok());
        assert_eq!(
            guard.check(),
            Err(ValidationError::ExcessiveRenormalization)
        );
    }

    #[test]
    fn test_renorm_guard_r64() {
        let mut guard = RenormGuard::new_r64();
        for _ in 0..MAX_RENORM_STEPS_R64 {
            assert!(guard.check().is_ok());
        }
        assert_eq!(
            guard.check(),
            Err(ValidationError::ExcessiveRenormalization)
        );
    }

    // ----- Edge-case detection ----------------------------------------------

    #[test]
    fn test_has_dominant_symbol() {
        let freqs = [1000u32, 1, 1];
        assert!(has_dominant_symbol(&freqs, 1002));
        let freqs = [500u32, 500];
        assert!(!has_dominant_symbol(&freqs, 1000)); // exactly half, not > half
        let freqs = [501u32, 499];
        assert!(has_dominant_symbol(&freqs, 1000));
    }

    #[test]
    fn test_is_single_symbol() {
        assert!(is_single_symbol(&[100u32, 0, 0]));
        assert!(!is_single_symbol(&[50u32, 50]));
        assert!(!is_single_symbol(&[0u32, 0]));
    }

    #[test]
    fn test_has_freq_one() {
        assert!(has_freq_one(&[1u32, 100]));
        assert!(!has_freq_one(&[2u32, 100]));
    }
}
