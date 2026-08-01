//! Fuzz target: AVX512 16-way Word rANS round-trip.
//!
//! Exercises the AVX512 16-way decoder against the scalar 16-way reference.
//! Verifies output equivalence, word consumption, and error handling.

#![no_main]

use libfuzzer_sys::fuzz_target;
#[cfg(all(target_feature = "avx512f", target_feature = "avx512bw"))]
use ryg_rans_rs_simd::avx512::decode_interleaved16_avx512_kernel;
#[cfg(all(target_feature = "avx512f", target_feature = "avx512bw"))]
use ryg_rans_rs_simd::packed_table::{
    decode_interleaved16_scalar, encode_interleaved16, PackedWordTable,
};
#[cfg(all(target_feature = "avx512f", target_feature = "avx512bw"))]
use ryg_rans_rs_simd::RANS_WORD_SCALE_BITS;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 || data.len() > 16384 {
        return;
    }

    // The kernel is compile-time gated on avx512bw (the module-level cfg in
    // avx512.rs); without the feature this target is a no-op, mirroring the
    // crate's portable-build behavior.
    #[cfg(all(target_feature = "avx512f", target_feature = "avx512bw"))]
    {
        fuzz_avx512_16way(data);
    }
});

#[cfg(all(target_feature = "avx512f", target_feature = "avx512bw"))]
fn fuzz_avx512_16way(data: &[u8]) {
    let scale_bits = RANS_WORD_SCALE_BITS as u32;
    let total = 1u32 << scale_bits;
    let used_syms = (data[0] as usize % 256).max(1);
    let uniform_freq = total / used_syms as u32;
    if uniform_freq == 0 {
        return;
    }

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

    let packed = match PackedWordTable::from_freqs(&freqs, &cum, scale_bits) {
        Ok(t) => t,
        Err(_) => return,
    };

    let symbols: Vec<u8> = data[1..].iter().map(|&b| b % used_syms as u8).collect();
    if symbols.is_empty() {
        return;
    }

    let compressed = match encode_interleaved16(&symbols, &freqs, &cum, scale_bits) {
        Ok(c) => c,
        Err(_) => return,
    };

    // Scalar 16-way decode
    let scalar_result = decode_interleaved16_scalar(&compressed, &packed, symbols.len());

    // AVX512 16-way decode
    unsafe {
        let avx_result = decode_interleaved16_avx512_kernel(&compressed, &packed, symbols.len());
        match (scalar_result, avx_result) {
            (Ok((scalar_out, scalar_report)), Ok((avx_out, avx_report))) => {
                assert_eq!(avx_out, symbols, "AVX512 16-way fuzz: roundtrip failed");
                assert_eq!(avx_out, scalar_out, "AVX512 16-way fuzz: must match scalar");
                assert_eq!(
                    avx_report.words_consumed, scalar_report.words_consumed,
                    "AVX512 16-way fuzz: word consumption must match"
                );
            }
            (Err(_), Err(_)) => {}
            (Ok(_), Err(e)) => {
                panic!("AVX512 16-way fuzz: scalar OK but AVX512 failed: {}", e);
            }
            (Err(e), Ok(_)) => {
                panic!("AVX512 16-way fuzz: AVX512 OK but scalar failed: {}", e);
            }
        }
    }
}
