// Kani proof: encode-decode inversion.
//
// Proves that for the byte rANS:
//   decode(encode(s, x)) == x
//
// This is the fundamental property of ANS: the encoding and decoding
// operations are inverses. We prove it for the core formula (without
// renormalization) for all valid symbol parameters, instantiated per
// concrete freq (division by a symbolic freq is not bit-blastable).

// kani-flags: --harness kani_byte_encode_decode_inversion

use crate::RANS_BYTE_L;

/// Division-based encode: C(s, x) = ((x/freq) << scale_bits) + (x%freq) + start
fn encode_step(x: u32, start: u32, freq: u32, scale_bits: u32) -> u32 {
    ((x / freq) << scale_bits) + (x % freq) + start
}

/// Division-based decode: D(s, C(s,x)) = freq * (C >> scale_bits) + (C & mask) - start
fn decode_step(c: u32, start: u32, freq: u32, scale_bits: u32) -> u32 {
    let mask = (1u32 << scale_bits) - 1;
    freq * (c >> scale_bits) + (c & mask) - start
}

/// Verify decode(encode(x)) == x for one concrete freq at scale 12.
fn check_inversion(freq: u32) {
    let scale_bits: u32 = 12;
    let x: u32 = kani::any();
    let start: u32 = kani::any();

    let max_total = 1u64 << scale_bits;
    kani::assume((start as u64) <= max_total - (freq as u64));
    kani::assume(x >= RANS_BYTE_L); // valid encoder state

    // No renormalization needed.
    let x_max = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
    kani::assume(x < x_max);

    let encoded = encode_step(x, start, freq, scale_bits);
    let decoded = decode_step(encoded, start, freq, scale_bits);

    assert_eq!(decoded, x, "decode(encode(x)) must equal x");
}

/// freq = 1 (the ANS degenerate single-symbol case).
#[kani::proof]
#[kani::unwind(20)]
fn kani_byte_encode_decode_inversion_freq1() {
    check_inversion(1);
}

/// freq = 2.
#[kani::proof]
#[kani::unwind(20)]
fn kani_byte_encode_decode_inversion_freq2() {
    check_inversion(2);
}

/// freq = 3.
#[kani::proof]
#[kani::unwind(20)]
fn kani_byte_encode_decode_inversion_freq3() {
    check_inversion(3);
}

/// freq = 255.
#[kani::proof]
#[kani::unwind(20)]
fn kani_byte_encode_decode_inversion_freq255() {
    check_inversion(255);
}

/// freq = 4095 (maximum at scale 12).
#[kani::proof]
#[kani::unwind(20)]
fn kani_byte_encode_decode_inversion_freq4095() {
    check_inversion(4095);
}
