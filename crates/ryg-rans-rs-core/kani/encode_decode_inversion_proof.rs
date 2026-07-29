//! Kani proof: encode-decode inversion.
//!
//! Proves that for the byte rANS:
//!   decode(encode(s, x)) == x
//!
//! This is the fundamental property of ANS: the encoding and decoding
//! operations are inverses. We prove it for the core formula (without
//! renormalization) for all valid symbol parameters.

// kani-flags: --unwind 4 --harness kani_byte_encode_decode_inversion

use ryg_rans_rs_core::RANS_BYTE_L;

/// Division-based encode: C(s, x) = ((x/freq) << scale_bits) + (x%freq) + start
fn encode_step(x: u32, start: u32, freq: u32, scale_bits: u32) -> u32 {
    ((x / freq) << scale_bits) + (x % freq) + start
}

/// Division-based decode: D(s, C(s,x)) = freq * (C >> scale_bits) + (C & mask) - start
fn decode_step(c: u32, start: u32, freq: u32, scale_bits: u32) -> u32 {
    let mask = (1u32 << scale_bits) - 1;
    freq * (c >> scale_bits) + (c & mask) - start
}

#[kani::proof]
fn kani_byte_encode_decode_inversion() {
    let x: u32 = kani::any();
    let start: u32 = kani::any();
    let freq: u32 = kani::any();
    let scale_bits: u32 = kani::any();

    // Valid parameters
    kani::assume(scale_bits >= 1 && scale_bits <= 16);
    kani::assume(freq > 0);
    let max_total = 1u64 << scale_bits;
    kani::assume((start as u64) <= max_total);
    kani::assume((freq as u64) <= max_total - (start as u64));
    kani::assume(x >= RANS_BYTE_L); // valid encoder state

    // No renormalization needed
    let x_max = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
    kani::assume(x < x_max);

    // Encode then decode must recover x
    let encoded = encode_step(x, start, freq, scale_bits);
    let decoded = decode_step(encoded, start, freq, scale_bits);

    assert_eq!(decoded, x, "decode(encode(x)) must equal x");
}
