//! # Exhaustive SIMD renormalization mask tests
//!
//! Tests every possible renormalization mask for both AVX512VL 8-way (256 masks)
//! and AVX512 16-way (65536 masks) by invoking the standalone SIMD renorm
//! kernels directly — NOT by reconstructing masks via scalar comparison.
//!
//! For each mask we verify:
//! - Observed mask == requested mask (via `_mm{256,512}_cmplt_epu32_mask`)
//! - Words consumed == popcount(mask)
//! - Active lane N receives its ascending-order word (0x0100 + lane)
//! - Inactive lanes remain bit-identical
//! - Exactly popcount(mask)-sized input succeeds
//! - popcount(mask) - 1 sized input fails (truncation detection)
//!
//! Run the 8-way test in any mode (256 iterations):
//! ```sh
//! cargo test -p ryg-rans-rs-simd test_8way_exhaustive_simd_renorm
//! ```
//!
//! Run the 16-way test with `--release` for acceptable speed (65536 iterations):
//! ```sh
//! RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
//!     cargo test --release -p ryg-rans-rs-simd -- --ignored test_16way_exhaustive_simd_renorm
//! ```

use crate::RANS_WORD_L;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Popcount for 8-bit value.
fn popcount8(x: u8) -> usize {
    x.count_ones() as usize
}

/// Popcount for 16-bit value.
fn popcount16(x: u16) -> usize {
    x.count_ones() as usize
}

// ---------------------------------------------------------------------------
// 8-way exhaustive SIMD renorm test
// ---------------------------------------------------------------------------

#[test]
fn test_8way_exhaustive_simd_renorm() {
    // Verify the AVX512VL renormalization machinery for every possible
    // 8-lane mask (0..256) by calling the standalone renorm kernel.
    //
    // This tests:
    //   - _mm256_cmplt_epu32_mask returns the correct mask
    //   - u16 words are distributed to the correct lanes in ascending order
    //   - Inactive lanes are unchanged
    //   - Reader advancement equals popcount(mask)
    //   - Truncation is detected (popcount-1 sized input fails)

    // Only run if AVX512VL is compiled in.
    if !cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw",
    )) {
        return;
    }

    // Track which masks have been tested for diagnostic reporting.
    let mut tested: u64 = 0;

    for mask in 0u8..=255u8 {
        let words_needed = popcount8(mask);

        // Build initial states:
        // - Lanes in the mask get state = RANS_WORD_L - 1 (needs renorm)
        // - Other lanes get state = RANS_WORD_L (no renorm needed)
        // - Each state has a unique upper portion so we can verify that
        //   inactive lanes remain unchanged.
        let mut init_states = [0u32; 8];
        for lane in 0..8 {
            if (mask >> lane) & 1 != 0 {
                // Below threshold — needs renorm.
                init_states[lane] = RANS_WORD_L - 1;
            } else {
                // Stable state — will not trigger renorm.
                // Use a unique value per lane to detect any spurious writes.
                init_states[lane] = RANS_WORD_L + (lane as u32) * 1000 + 0x1000;
            }
        }

        // Prepare renorm words: each active lane gets a word that identifies it.
        let mut renorm_words: Vec<u16> = Vec::with_capacity(words_needed);
        for lane in 0..8 {
            if (mask >> lane) & 1 != 0 {
                // Use 0x0100 + lane so the final state's low 16 bits
                // equal this value after renorm.
                renorm_words.push(0x0100 + lane as u16);
            }
        }

        // Load states into __m256i.
        // SAFETY: The SIMD kernel requires target features.
        unsafe {
            use core::arch::x86_64::*;

            let state_v = _mm256_loadu_si256(init_states.as_ptr() as *const __m256i);

            // ---- Test with exactly the right number of renorm words ----
            {
                let result = crate::avx512::renorm8_avx512vl(state_v, &renorm_words);
                assert!(
                    result.is_ok(),
                    "8-way mask {:08b}: renorm with {} words should succeed (popcount={})",
                    mask,
                    renorm_words.len(),
                    words_needed
                );
                let report = result.unwrap();

                // 1. Observed mask must match requested mask.
                assert_eq!(
                    report.mask, mask,
                    "8-way mask mismatch: requested {:08b}, observed {:08b}",
                    mask, report.mask
                );

                // 2. Words consumed must equal popcount(mask).
                assert_eq!(
                    report.words_consumed, words_needed,
                    "8-way mask {:08b}: words consumed {} != popcount {}",
                    mask, report.words_consumed, words_needed
                );

                // 3. Active lanes must receive their ascending-order renorm words.
                //    And inactive lanes must remain unchanged.
                let mut rp = 0usize;
                for lane in 0..8 {
                    if (mask >> lane) & 1 != 0 {
                        // Active lane: state should be (old_state << 16) | renorm_word.
                        let expected_state = ((RANS_WORD_L - 1) << 16) | (0x0100 + lane as u32);
                        assert_eq!(
                            report.states[lane], expected_state,
                            "8-way mask {:08b}: lane {} active: expected 0x{:08x}, got 0x{:08x}",
                            mask, lane, expected_state, report.states[lane]
                        );
                        rp += 1;
                    } else {
                        // Inactive lane: state must be unchanged.
                        assert_eq!(
                            report.states[lane], init_states[lane],
                            "8-way mask {:08b}: lane {} inactive: state changed from 0x{:08x} to 0x{:08x}",
                            mask, lane, init_states[lane], report.states[lane]
                        );
                    }
                }
            }

            // ---- Test truncation: popcount-1 renorm words must fail ----
            if words_needed > 0 {
                let short_words = &renorm_words[..renorm_words.len().saturating_sub(1)];
                let result = crate::avx512::renorm8_avx512vl(state_v, short_words);
                assert!(
                    result.is_err(),
                    "8-way mask {:08b}: renorm with {} words (need {}) should FAIL",
                    mask,
                    short_words.len(),
                    words_needed
                );
            }
        }

        tested += 1;
    }

    assert_eq!(tested, 256, "All 256 8-way masks must be tested");
}

// ---------------------------------------------------------------------------
// 16-way exhaustive SIMD renorm test
// ---------------------------------------------------------------------------

#[test]
#[ignore = "Run with --release: 65536 iterations (approx 1-2 seconds)"]
fn test_16way_exhaustive_simd_renorm() {
    // Verify the AVX512 16-way renormalization machinery for every possible
    // 16-lane mask (0..65536) by calling the standalone renorm kernel.
    //
    // The same assertions as the 8-way test apply, extended to 16 lanes.

    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }

    for mask in 0u16..=65535u16 {
        let words_needed = popcount16(mask);

        let mut init_states = [0u32; 16];
        for lane in 0..16 {
            if (mask >> lane) & 1 != 0 {
                init_states[lane] = RANS_WORD_L - 1;
            } else {
                init_states[lane] = RANS_WORD_L + (lane as u32) * 1000 + 0x1000;
            }
        }

        let mut renorm_words: Vec<u16> = Vec::with_capacity(words_needed);
        for lane in 0..16 {
            if (mask >> lane) & 1 != 0 {
                renorm_words.push(0x0200 + lane as u16);
            }
        }

        unsafe {
            use core::arch::x86_64::*;

            let state_v = _mm512_loadu_si512(init_states.as_ptr() as *const __m512i);

            // ---- Test with exactly the right number of renorm words ----
            {
                let result = crate::avx512::renorm16_avx512(state_v, &renorm_words);
                assert!(
                    result.is_ok(),
                    "16-way mask {:016b}: renorm with {} words should succeed",
                    mask,
                    renorm_words.len()
                );
                let report = result.unwrap();

                // 1. Observed mask must match requested mask.
                assert_eq!(
                    report.mask, mask,
                    "16-way mask mismatch: requested {:016b}, observed {:016b}",
                    mask, report.mask
                );

                // 2. Words consumed must equal popcount(mask).
                assert_eq!(
                    report.words_consumed, words_needed,
                    "16-way mask {:016b}: words consumed {} != popcount {}",
                    mask, report.words_consumed, words_needed
                );

                // 3. Active lanes receive words; inactive lanes remain unchanged.
                for lane in 0..16 {
                    if (mask >> lane) & 1 != 0 {
                        let expected_state = ((RANS_WORD_L - 1) << 16) | (0x0200 + lane as u32);
                        assert_eq!(
                            report.states[lane], expected_state,
                            "16-way mask {:016b}: lane {} active: expected 0x{:08x}, got 0x{:08x}",
                            mask, lane, expected_state, report.states[lane]
                        );
                    } else {
                        assert_eq!(
                            report.states[lane], init_states[lane],
                            "16-way mask {:016b}: lane {} inactive: state changed",
                            mask, lane
                        );
                    }
                }
            }

            // ---- Test truncation: popcount-1 renorm words must fail ----
            if words_needed > 0 {
                let short_words = &renorm_words[..renorm_words.len().saturating_sub(1)];
                let result = crate::avx512::renorm16_avx512(state_v, short_words);
                assert!(
                    result.is_err(),
                    "16-way mask {:016b}: renorm with {} words (need {}) should FAIL",
                    mask,
                    short_words.len(),
                    words_needed
                );
            }
        }
    }
}
