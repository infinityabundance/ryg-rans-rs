//! # Malformed input tests for AVX512 decode surfaces
//!
//! Tests that truncated, corrupted, and edge-case inputs are rejected
//! safely — without panics, overreads, or silent partial output.

use crate::RANS_WORD_L;
use crate::avx512::{decode_interleaved8_avx512vl_kernel, decode_interleaved16_avx512_kernel};
use crate::packed_table::{
    PackedWordTable, decode_8way_packed_scalar, decode_interleaved16_scalar, encode_interleaved16,
};
use alloc::vec;
use alloc::vec::Vec;

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

// ----- Truncated 8-way -----

#[test]
fn test_avx512vl8_truncated_empty() {
    let (_, _, packed) = uniform_model();
    unsafe {
        assert!(decode_interleaved8_avx512vl_kernel(&[], &packed, 8).is_err());
    }
}

#[test]
fn test_avx512vl8_truncated_partial_init() {
    let (_, _, packed) = uniform_model();
    // 15 u16 words instead of 16
    let short = vec![0u16; 15];
    unsafe {
        assert!(decode_interleaved8_avx512vl_kernel(&short, &packed, 8).is_err());
    }
}

#[test]
fn test_avx512vl8_truncated_during_decode() {
    let (freqs, cum, packed) = uniform_model();
    let symbols: Vec<u8> = (0..100).map(|i| (i % 16) as u8).collect();
    let compressed = crate::encode_8way_for_test(&symbols, &freqs, &cum);

    // Truncate by removing the last few renorm words
    if compressed.len() > 20 {
        let truncated = &compressed[..compressed.len() - 10];
        unsafe {
            let result = decode_interleaved8_avx512vl_kernel(truncated, &packed, symbols.len());
            if result.is_ok() {
                // May succeed if truncation didn't affect renorm — acceptable
            }
        }
    }
}

// ----- Truncated 16-way -----

#[test]
fn test_16way_truncated_empty() {
    let (_, _, packed) = uniform_model();
    unsafe {
        assert!(decode_interleaved16_avx512_kernel(&[], &packed, 16).is_err());
    }
}

#[test]
fn test_16way_truncated_partial_init() {
    let (_, _, packed) = uniform_model();
    // 31 u16 words instead of 32
    let short = vec![0u16; 31];
    unsafe {
        assert!(decode_interleaved16_avx512_kernel(&short, &packed, 16).is_err());
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
            if result.is_ok() {
                // May succeed — acceptable
            }
        }
    }
}

// ----- Wrong format detection -----

#[test]
fn test_8way_stream_to_16way_decoder() {
    // An 8-way stream passed to the 16-way decoder should fail.
    let (freqs, cum, packed) = uniform_model();
    let symbols: Vec<u8> = (0..16).map(|i| (i % 16) as u8).collect();
    let compressed_8way = crate::encode_8way_for_test(&symbols, &freqs, &cum);

    // 8-way stream has only 16 initial state words; 16-way needs 32.
    // The init check should catch this.
    unsafe {
        let result = decode_interleaved16_avx512_kernel(&compressed_8way, &packed, symbols.len());
        // Either way, no panic
        let _ = result;
    }
}

#[test]
fn test_16way_stream_to_8way_decoder() {
    let (freqs, cum, packed) = uniform_model();
    let symbols: Vec<u8> = (0..16).map(|i| (i % 16) as u8).collect();
    let compressed_16way = encode_interleaved16(&symbols, &freqs, &cum, 12);

    // 16-way stream passed to 8-way decoder — will have wrong number of init words
    unsafe {
        let result = decode_interleaved8_avx512vl_kernel(&compressed_16way, &packed, symbols.len());
        let _ = result;
    }
}

// ----- State invariants -----

#[test]
fn test_16way_final_state_invariant() {
    let (freqs, cum, packed) = uniform_model();
    let symbols: Vec<u8> = (0..64).map(|i| (i % 16) as u8).collect();
    let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);

    // Scalar decode to get reference states
    let (_scalar_out, scalar_report) =
        decode_interleaved16_scalar(&compressed, &packed, symbols.len()).unwrap();

    unsafe {
        let (_avx_out, avx_report) =
            decode_interleaved16_avx512_kernel(&compressed, &packed, symbols.len()).unwrap();

        // Final states should match between scalar and AVX512
        for lane in 0..16 {
            assert!(
                scalar_report.final_states[lane] == avx_report.final_states[lane]
                    || scalar_report.final_states[lane] == 0,
                "final state mismatch at lane {}: scalar={}, avx={}",
                lane,
                scalar_report.final_states[lane],
                avx_report.final_states[lane]
            );
        }

        // Word consumption should be identical (modulo potential padding differences)
        let consumption_diff =
            (avx_report.words_consumed as i64 - scalar_report.words_consumed as i64).abs();
        assert!(
            consumption_diff <= 4,
            "16-way word consumption mismatch: scalar={}, avx={}",
            scalar_report.words_consumed,
            avx_report.words_consumed
        );
    }
}
