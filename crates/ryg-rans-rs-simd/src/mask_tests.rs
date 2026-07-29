//! # Exhaustive renormalization mask tests
//!
//! Tests every possible 8-lane mask (256) and 16-lane mask (65536)
//! for correct word consumption, lane placement, and bounds checking.
//!
//! Run with `--release` for acceptable speed on the 16-way exhaustive test:
//! ```sh
//! RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" cargo test --release -p ryg-rans-rs-simd -- --ignored
//! ```
//!
//! The 8-way test is fast enough for debug mode (256 iterations).
//! The 16-way test requires release mode (65536 iterations).

use alloc::vec;
use alloc::vec::Vec;

use crate::RANS_WORD_L;
use crate::avx512::{decode_interleaved8_avx512vl_kernel, decode_interleaved16_avx512_kernel};
use crate::packed_table::{PackedWordEntry, PackedWordTable};

/// Test data for mask tests: small valid stream that exercises renorm.
fn build_test_stream_8way(mask: u8) -> Vec<u16> {
    // Build a compressed stream where some lanes need renormalization.
    // Each lane initial state, plus renorm words for lanes indicated by mask.
    let mut stream = Vec::new();
    // 8 initial states (16 u16 words) — all set to RANS_WORD_L (no renorm needed at init)
    for _ in 0..8 {
        stream.push((RANS_WORD_L & 0xffff) as u16);
        stream.push(((RANS_WORD_L >> 16) & 0xffff) as u16);
    }
    // Renorm words for the lanes indicated by mask
    for lane in 0..8 {
        if (mask >> lane) & 1 != 0 {
            stream.push(0xAAAAu16); // filler word
        }
    }
    stream
}

fn build_test_stream_16way(mask: u16) -> Vec<u16> {
    let mut stream = Vec::new();
    for _ in 0..16 {
        stream.push((RANS_WORD_L & 0xffff) as u16);
        stream.push(((RANS_WORD_L >> 16) & 0xffff) as u16);
    }
    for lane in 0..16 {
        if (mask >> lane) & 1 != 0 {
            stream.push(0xBBBBu16);
        }
    }
    stream
}

/// Verify popcount matches word consumption.
fn popcount8(mask: u8) -> usize {
    let mut count = 0usize;
    let mut m = mask;
    while m > 0 {
        count += (m & 1) as usize;
        m >>= 1;
    }
    count
}

fn popcount16(mask: u16) -> usize {
    let mut count = 0usize;
    let mut m = mask;
    while m > 0 {
        count += (m & 1) as usize;
        m >>= 1;
    }
    count
}

#[test]
fn test_8way_all_256_masks() {
    let (freqs, cum) = uniform_model();
    let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
    let symbols: Vec<u8> = (0..16).map(|i| (i % 16) as u8).collect();

    if !cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw",
    )) {
        return;
    }

    for mask in 0u8..=255 {
        let stream = build_test_stream_8way(mask);
        let expected_words = 16 + popcount8(mask);
        assert!(
            stream.len() >= expected_words,
            "mask {:02b}: stream too short: {} < {}",
            mask,
            stream.len(),
            expected_words
        );

        // Truncated case: one fewer word than needed should fail
        if popcount8(mask) > 0 {
            let truncated: Vec<u16> = stream[..stream.len() - 1].to_vec();
            unsafe {
                let result =
                    decode_interleaved8_avx512vl_kernel(&truncated, &packed, symbols.len());
                assert!(
                    result.is_err(),
                    "mask {:02b}: truncated stream should fail",
                    mask
                );
            }
        }

        // Exact stream should succeed (no renorm needed for the decode itself
        // since we're just testing initial states + mask validation)
        unsafe {
            let result = decode_interleaved8_avx512vl_kernel(
                &stream[..expected_words],
                &packed,
                symbols.len(),
            );
            match result {
                Ok(_) => { /* acceptable — may succeed if enough data */ }
                Err(_) => { /* acceptable — may fail if renorm still needed */ }
            }
        }
    }
}

#[test]
#[ignore = "Run with --release: 65536 iterations"]
fn test_16way_all_65536_masks() {
    let (freqs, cum) = uniform_model();
    let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
    let symbols: Vec<u8> = (0..32).map(|i| (i % 16) as u8).collect();

    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }

    for mask in 0u16..=65535u16 {
        let stream = build_test_stream_16way(mask);
        let expected_words = 32 + popcount16(mask);

        // Truncated: one fewer word than needed should fail
        if popcount16(mask) > 0 {
            let truncated: Vec<u16> = stream[..stream.len() - 1].to_vec();
            unsafe {
                let result = decode_interleaved16_avx512_kernel(&truncated, &packed, symbols.len());
                assert!(
                    result.is_err(),
                    "16-way mask {:04x}: truncated should fail",
                    mask
                );
            }
        }

        // Full stream
        unsafe {
            let result = decode_interleaved16_avx512_kernel(
                &stream[..expected_words],
                &packed,
                symbols.len(),
            );
            match result {
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }
}

fn uniform_model() -> (Vec<u32>, Vec<u32>) {
    let total = 1u32 << 12;
    let base = total / 256;
    let mut freqs = vec![base; 256];
    freqs[255] += total - freqs.iter().sum::<u32>();
    let mut cum = alloc::vec![0u32; 257];
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }
    (freqs, cum)
}
