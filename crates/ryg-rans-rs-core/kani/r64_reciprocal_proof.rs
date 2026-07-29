//! Kani proof: 64-bit rANS reciprocal fast-path matches division.
//!
//! Proves that `Rans64EncSymbol::new` + reciprocal put produces the same
//! new state as the division-based reference for all valid parameter
//! combinations where no renormalization is needed.

// kani-flags: --unwind 6 --harness kani_r64_reciprocal_equals_division

use ryg_rans_rs_core::{RANS64_L, Rans64EncSymbol, rans64_mul_hi};

fn div_put(x: u64, start: u32, freq: u32, scale_bits: u32) -> u64 {
    ((x / (freq as u64)) << scale_bits) + (x % (freq as u64)) + (start as u64)
}

#[kani::proof]
#[kani::unwind(4)]
fn kani_r64_reciprocal_equals_division() {
    let x: u64 = kani::any();
    let start: u32 = kani::any();
    let freq: u32 = kani::any();
    let scale_bits: u32 = kani::any();

    kani::assume(scale_bits >= 1 && scale_bits <= 31);
    kani::assume(freq > 0);
    let max_total = 1u64 << scale_bits;
    kani::assume((start as u64) <= max_total);
    kani::assume((freq as u64) <= max_total - (start as u64));

    // No renormalization needed
    let x_max = ((RANS64_L >> scale_bits) << 32) * (freq as u64);
    kani::assume(x < x_max);

    let expected = div_put(x, start, freq, scale_bits);

    let sym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();
    let q = rans64_mul_hi(x, sym.rcp_freq) >> sym.rcp_shift;
    let reciprocal_result = x + sym.bias + q * (sym.cmpl_freq as u64);

    assert_eq!(
        reciprocal_result, expected,
        "R64 reciprocal must match division"
    );
}
