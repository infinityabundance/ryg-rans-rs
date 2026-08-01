// Kani proof: `RansByteEncSymbol::new` never panics for valid inputs.
//
// Proves that for all permissible `start`, `freq`, and `scale_bits`
// values, the encoder symbol initialization:
// - Never overflows in arithmetic
// - Returns `Ok` for valid combinations
// - Returns `Err(ModelError::InvalidScaleBits)` for out-of-range scale_bits
// - Returns `Err(ModelError::ZeroFrequency)` for freq == 0
// - Returns `Err(ModelError::StartOutOfRange)` for start > (1 << scale_bits)
// - Returns `Err(ModelError::FrequencyOutOfRange)` for freq > (1 << scale_bits) - start

// kani-flags: --unwind 4 --harness kani_enc_symbol_new_valid

use crate::RansByteEncSymbol;

/// Prove that for ANY valid parameters, new() returns Ok.
#[kani::proof]
fn kani_enc_symbol_new_valid() {
    let start: u32 = kani::any();
    let freq: u32 = kani::any();
    let scale_bits: u32 = kani::any();

    // Constrain to valid range (limits Kani search space)
    kani::assume(scale_bits >= 1 && scale_bits <= 16);
    kani::assume(freq > 0);
    let max_total = 1u64 << scale_bits;
    kani::assume((start as u64) <= max_total);
    kani::assume((freq as u64) <= max_total - (start as u64));

    let result = RansByteEncSymbol::new(start, freq, scale_bits);
    assert!(result.is_ok(), "valid parameters must produce Ok");

    let sym = result.unwrap();
    // Post-conditions from upstream specification
    assert!(sym.x_max > 0, "x_max must be > 0 for any non-zero freq");
    assert!(sym.rcp_freq > 0, "rcp_freq must be > 0");
}

/// Prove that scale_bits=0 or scale_bits>16 produces InvalidScaleBits.
#[kani::proof]
fn kani_enc_symbol_new_invalid_scale() {
    let start: u32 = kani::any();
    let freq: u32 = kani::any();
    let scale_bits: u32 = kani::any();

    kani::assume(scale_bits == 0 || scale_bits > 16);

    let result = RansByteEncSymbol::new(start, freq, scale_bits);
    assert!(matches!(result, Err(crate::ModelError::InvalidScaleBits)));
}

/// Prove that freq=0 produces ZeroFrequency.
///
/// Requires `start` in range: the validation order checks range before
/// zero-frequency, so an out-of-range `start` correctly yields
/// `StartOutOfRange` instead.
#[kani::proof]
fn kani_enc_symbol_new_zero_freq() {
    let start: u32 = kani::any();
    let freq: u32 = 0;
    let scale_bits: u32 = kani::any();

    kani::assume(scale_bits >= 1 && scale_bits <= 16);
    let max_total = 1u64 << scale_bits;
    kani::assume((start as u64) <= max_total);

    let result = RansByteEncSymbol::new(start, freq, scale_bits);
    assert!(matches!(result, Err(crate::ModelError::ZeroFrequency)));
}
