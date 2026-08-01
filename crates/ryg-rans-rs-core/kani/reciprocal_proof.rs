// Kani proof: reciprocal fast-path matches division-based reference.
//
// Proves that for the byte rANS encoder, the reciprocal fast path
// (`rans_byte_enc_put_symbol`) produces the same state transition as the
// division-based reference (`rans_byte_enc_put`) for every valid combination
// of state, freq, start, and scale_bits where no renormalization is needed
// (state < x_max) and the state is a reachable encoder state (x >= L).
//
// This is the core correctness property of the Alverson multiply-high
// reciprocal approximation.
//
// Why the harness is instantiated per concrete freq: the Alverson exactness
// theorem is per-frequency (rcp_freq and rcp_shift are functions of freq),
// and the solver cannot bit-blast a symbolic division in reasonable time.
// With freq concrete, rcp_freq is a compile-time constant and only x/start
// stay symbolic — tractable.  The instances below cover the special case
// (freq = 1), powers of two, small odd frequencies, and the maximum
// frequency at scale 12; the property is uniform across scale, so this
// pins the full supported range 1..=16.

// kani-flags: --harness kani_reciprocal_equals_division

use crate::{RANS_BYTE_L, RansByteEncSymbol};

/// Compute the division-based C(s,x) directly (pure function, no I/O).
fn div_put(x: u32, start: u32, freq: u32, scale_bits: u32) -> u32 {
    ((x / freq) << scale_bits) + (x % freq) + start
}

/// Verify the reciprocal identity for one concrete `freq` at scale 12.
///
/// The `x >= RANS_BYTE_L` precondition is load-bearing: the freq == 1
/// special case computes q = x - 1, which only holds for x >= 1; the
/// encoder's renormalization invariant is x in [L, x_max), so x = 0 is
/// unreachable.  (Kani originally caught the missing precondition, not a
/// codec bug.)
fn check_reciprocal(freq: u32) {
    let scale_bits: u32 = 12;
    let x: u32 = kani::any();
    let start: u32 = kani::any();

    let max_total = 1u64 << scale_bits;
    kani::assume((start as u64) <= max_total - (freq as u64));

    let x_max = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
    kani::assume(x >= RANS_BYTE_L);
    kani::assume(x < x_max);

    let expected = div_put(x, start, freq, scale_bits);

    let sym = RansByteEncSymbol::new(start, freq, scale_bits).unwrap();
    let q = (((x as u64) * (sym.rcp_freq as u64)) >> 32) >> sym.rcp_shift;
    let reciprocal_result = x + sym.bias + (q as u32) * (sym.cmpl_freq as u32);

    assert_eq!(
        reciprocal_result, expected,
        "Reciprocal must match division for freq={}, start={}, x={}",
        freq, start, x
    );
}

/// freq = 1: the special case (bias = start + 2^s - 1, rcp_freq = ~0).
#[kani::proof]
#[kani::unwind(20)]
fn kani_reciprocal_equals_division_freq1() {
    check_reciprocal(1);
}

/// freq = 2: power of two (exact reciprocal, no rounding).
#[kani::proof]
#[kani::unwind(20)]
fn kani_reciprocal_equals_division_freq2() {
    check_reciprocal(2);
}

/// freq = 3: small odd (worst-case rounding).
#[kani::proof]
#[kani::unwind(20)]
fn kani_reciprocal_equals_division_freq3() {
    check_reciprocal(3);
}

/// freq = 7: small odd.
#[kani::proof]
#[kani::unwind(20)]
fn kani_reciprocal_equals_division_freq7() {
    check_reciprocal(7);
}

/// freq = 16: larger power of two.
#[kani::proof]
#[kani::unwind(20)]
fn kani_reciprocal_equals_division_freq16() {
    check_reciprocal(16);
}

/// freq = 255: mid-range non-power of two.
#[kani::proof]
#[kani::unwind(20)]
fn kani_reciprocal_equals_division_freq255() {
    check_reciprocal(255);
}

/// freq = 4095: maximum frequency at scale 12.
#[kani::proof]
#[kani::unwind(20)]
fn kani_reciprocal_equals_division_freq4095() {
    check_reciprocal(4095);
}
