//! # Malformed input tests for AVX512 decode surfaces
//!
//! Tests that truncated, corrupted, and edge-case inputs are rejected
//! safely — without panics, overreads, or silent partial output.
//!
//! Every test makes a concrete assertion about the expected behavior.
//! No test silently accepts both success and failure.

use crate::avx512::{decode_interleaved8_avx512vl_kernel, decode_interleaved16_avx512_kernel};
use crate::packed_table::{
    PackedWordTable, decode_8way_packed_scalar_with_report, decode_interleaved16_scalar,
    encode_interleaved16,
};
use alloc::vec::Vec;

fn uniform_model() -> (Vec<u32>, Vec<u32>, PackedWordTable) {
    let total = 1u32 << 12;
    let base = total / 256;
    let mut freqs = alloc::vec![base; 256];
    freqs[255] += total - freqs.iter().sum::<u32>();
    let mut cum = alloc::vec![0u32; 257];
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }
    let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
    (freqs, cum, packed)
}

// ----- Truncated 8-way -----

#[test]
fn test_avx512vl8_truncated_empty() {
    // Empty input must be rejected — not enough data for 8 init states
    let (_, _, packed) = uniform_model();
    unsafe {
        let result = decode_interleaved8_avx512vl_kernel(&[], &packed, 8);
        assert!(result.is_err(), "8-way AVX512VL: empty input must fail");
    }
}

#[test]
fn test_avx512vl8_truncated_partial_init() {
    // 15 u16 words instead of 16: not enough for 8 initial states
    let (_, _, packed) = uniform_model();
    let short = alloc::vec![0u16; 15];
    unsafe {
        let result = decode_interleaved8_avx512vl_kernel(&short, &packed, 8);
        assert!(result.is_err(), "8-way AVX512VL: 15 init words must fail");
    }
}

#[test]
fn test_avx512vl8_truncated_during_decode() {
    // Create a valid stream, then truncate renorm words.
    // The decoder must fail (not silently succeed with partial output).
    let (freqs, cum, packed) = uniform_model();
    let symbols: Vec<u8> = (0..100).map(|i| (i % 16) as u8).collect();
    let compressed = crate::encode_8way_for_test(&symbols, &freqs, &cum);

    // Remove the last few renorm words to force truncation
    if compressed.len() > 20 {
        let truncated = &compressed[..compressed.len() - 10];
        unsafe {
            let result = decode_interleaved8_avx512vl_kernel(truncated, &packed, symbols.len());
            // Must fail because renorm data is missing
            assert!(
                result.is_err(),
                "8-way AVX512VL: truncated renorm must fail (original len {}, truncated len {})",
                compressed.len(),
                truncated.len()
            );
        }
    }
}

// ----- Truncated 16-way -----

#[test]
fn test_16way_truncated_empty() {
    let (_, _, packed) = uniform_model();
    unsafe {
        let result = decode_interleaved16_avx512_kernel(&[], &packed, 16);
        assert!(result.is_err(), "16-way AVX512: empty input must fail");
    }
}

#[test]
fn test_16way_truncated_partial_init() {
    let (_, _, packed) = uniform_model();
    let short = alloc::vec![0u16; 31];
    unsafe {
        let result = decode_interleaved16_avx512_kernel(&short, &packed, 16);
        assert!(result.is_err(), "16-way AVX512: 31 init words must fail");
    }
}

#[test]
fn test_16way_truncated_during_decode() {
    let (freqs, cum, packed) = uniform_model();
    let symbols: Vec<u8> = (0..100).map(|i| (i % 16) as u8).collect();
    let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);

    if compressed.len() > 40 {
        let truncated = &compressed[..compressed.len() - 20];
        unsafe {
            let result = decode_interleaved16_avx512_kernel(truncated, &packed, symbols.len());
            assert!(result.is_err(), "16-way AVX512: truncated renorm must fail");
        }
    }
}

// ----- Final state parity -----

#[test]
fn test_16way_final_state_parity() {
    // Scalar and AVX512 decoders must produce identical final states
    // and consume exactly the same number of words.
    let (freqs, cum, packed) = uniform_model();
    let symbols: Vec<u8> = (0..64).map(|i| (i % 16) as u8).collect();
    let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);

    let (_scalar_out, scalar_report) =
        decode_interleaved16_scalar(&compressed, &packed, symbols.len()).unwrap();

    unsafe {
        let (_avx_out, avx_report) =
            decode_interleaved16_avx512_kernel(&compressed, &packed, symbols.len()).unwrap();

        // Final states must match exactly, lane by lane
        for lane in 0..16 {
            assert_eq!(
                scalar_report.final_states[lane], avx_report.final_states[lane],
                "16-way final state mismatch at lane {}: scalar={}, avx={}",
                lane, scalar_report.final_states[lane], avx_report.final_states[lane]
            );
        }

        // Word consumption must match exactly
        assert_eq!(
            avx_report.words_consumed, scalar_report.words_consumed,
            "16-way word consumption mismatch: scalar={}, avx={}",
            scalar_report.words_consumed, avx_report.words_consumed
        );
    }
}
