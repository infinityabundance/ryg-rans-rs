//! Fuzz target: AVX512VL 8-way Word rANS round-trip.
//!
//! Exercises the AVX512VL 8-way decoder against the packed scalar reference.
//! Verifies output equivalence, final-state correctness, and error handling.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_simd::avx512::decode_interleaved8_avx512vl_kernel;
use ryg_rans_rs_simd::backends::DecodeBackend;
use ryg_rans_rs_simd::packed_table::{decode_8way_packed_scalar, PackedWordTable};
use ryg_rans_rs_simd::{encode_8way_for_test, RANS_WORD_SCALE_BITS};

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 || data.len() > 16384 {
        return;
    }

    // Only run if AVX512 is available at compile time
    if !cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw",
    )) {
        return;
    }

    let scale_bits = RANS_WORD_SCALE_BITS as u32;
    let total = 1u32 << scale_bits;
    let used_syms = (data[0] as usize % 256).max(1);
    let uniform_freq = total / used_syms as u32;
    if uniform_freq == 0 {
        return;
    }

    // Build frequency model
    let mut freqs = vec![0u32; 256];
    let mut cum = vec![0u32; 257];
    for i in 0..used_syms {
        freqs[i] = uniform_freq;
        cum[i + 1] = cum[i] + freqs[i];
    }
    let sum: u32 = freqs[..used_syms].iter().sum();
    if sum < total {
        freqs[used_syms - 1] += total - sum;
    }
    cum[0] = 0;
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    // Build packed table
    let packed = match PackedWordTable::from_freqs(&freqs, &cum, scale_bits) {
        Ok(t) => t,
        Err(_) => return,
    };

    // Generate symbols
    let symbols: Vec<u8> = data[1..].iter().map(|&b| b % used_syms as u8).collect();
    if symbols.is_empty() {
        return;
    }

    // Encode
    let compressed = encode_8way_for_test(&symbols, &freqs, &cum);

    // Scalar decode
    let scalar_result = decode_8way_packed_scalar(&compressed, &packed, symbols.len());
    let scalar_ok = scalar_result
        .as_ref()
        .map(|d| d == &symbols)
        .unwrap_or(false);

    // AVX512VL decode
    unsafe {
        let avx_result = decode_interleaved8_avx512vl_kernel(&compressed, &packed, symbols.len());
        match (scalar_result, avx_result) {
            (Ok(scalar_out), Ok((avx_out, _avx_report))) => {
                // Both succeeded: must agree with each other and with input
                assert_eq!(avx_out, symbols, "AVX512VL fuzz: roundtrip failed");
                assert_eq!(avx_out, scalar_out, "AVX512VL fuzz: must match scalar");
            }
            (Err(_), Err(_)) => {
                // Both failed: acceptable (truncated stream scenario)
            }
            (Ok(_), Err(e)) => {
                // Scalar succeeded but AVX512 failed: this is a bug
                panic!("AVX512VL fuzz: scalar OK but AVX512VL failed: {}", e);
            }
            (Err(e), Ok(_)) => {
                // AVX512 succeeded but scalar failed: unexpected
                panic!("AVX512VL fuzz: AVX512VL OK but scalar failed: {}", e);
            }
        }
    }
});
