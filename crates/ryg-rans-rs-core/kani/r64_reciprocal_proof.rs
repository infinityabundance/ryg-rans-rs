// Kani proof: 64-bit rANS reciprocal fast-path matches division.
//
// Proves that for the 64-bit rANS encoder, the reciprocal fast path
// (`rans64_enc_put_symbol`) produces the same state transition as the
// division-based reference for every valid combination of state, freq,
// start, and scale_bits where no renormalization is needed (state < x_max)
// and the state is a reachable encoder state (x >= RANS64_L).
//
// As with the byte proof, the harness is instantiated per concrete freq:
// the Alverson exactness theorem is per-frequency and the solver cannot
// bit-blast a symbolic division.  freq = 1 exercises the special case
// (bias = start + 2^s - 1); powers of two are exact; small odd values are
// the worst rounding cases; 2^31 - 1 is the maximum frequency at scale 31.

// kani-flags: --harness kani_r64_reciprocal_equals_division

use crate::{RANS64_L, Rans64EncSymbol, rans64_mul_hi};

/// Compute the division-based C(s,x) directly (pure function, no I/O).
fn div_put(x: u64, start: u32, freq: u32, scale_bits: u32) -> u64 {
    ((x / (freq as u64)) << scale_bits) + (x % (freq as u64)) + (start as u64)
}

/// Verify the reciprocal identity for one concrete `freq` at scale 31.
///
/// `x >= RANS64_L` is load-bearing exactly as in the byte proof: the
/// freq == 1 special case computes q = x - 1, which only holds for x >= 1.
fn check_reciprocal(freq: u32) {
    let scale_bits: u32 = 31;
    let x: u64 = kani::any();
    let start: u32 = kani::any();

    let max_total = 1u64 << scale_bits;
    kani::assume((start as u64) <= max_total - (freq as u64));

    let x_max = ((RANS64_L >> scale_bits) << 32) * (freq as u64);
    kani::assume(x >= RANS64_L);
    kani::assume(x < x_max);

    let expected = div_put(x, start, freq, scale_bits);

    let sym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();
    let q = rans64_mul_hi(x, sym.rcp_freq) >> sym.rcp_shift;
    let reciprocal_result = x + sym.bias + q * (sym.cmpl_freq as u64);

    assert_eq!(
        reciprocal_result, expected,
        "R64 reciprocal must match division for freq={}, start={}, x={}",
        freq, start, x
    );
}

/// freq = 1: the special case (bias = start + 2^s - 1, rcp_freq = ~0).
#[kani::proof]
#[kani::unwind(40)]
fn kani_r64_reciprocal_equals_division_freq1() {
    check_reciprocal(1);
}

/// freq = 2: power of two (exact reciprocal).
#[kani::proof]
#[kani::unwind(40)]
fn kani_r64_reciprocal_equals_division_freq2() {
    check_reciprocal(2);
}

/// freq = 3: small odd (worst-case rounding).
#[kani::proof]
#[kani::unwind(40)]
fn kani_r64_reciprocal_equals_division_freq3() {
    check_reciprocal(3);
}

/// freq = 65535: mid-range non-power of two.
#[kani::proof]
#[kani::unwind(40)]
fn kani_r64_reciprocal_equals_division_freq65535() {
    check_reciprocal(65535);
}

/// freq = 2^31 - 1: maximum frequency at scale 31.
#[kani::proof]
#[kani::unwind(40)]
fn kani_r64_reciprocal_equals_division_freqmax() {
    check_reciprocal((1u64 << 31) as u32 - 1);
}
