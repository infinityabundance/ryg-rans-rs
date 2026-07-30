//! # Exhaustive AVX2 tests
//!
//! These tests verify AVX2 decode backends against scalar references.
//! They are conditionally compiled — only present when `target_feature = "avx2"`.

use crate::avx2;
use crate::avx2_renorm::{self, Avx2RenormPermutations, renorm8_avx2};
use crate::backends::DecodeError;
use crate::packed_table::PackedWordTable;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn perm_table() -> Avx2RenormPermutations {
    avx2_renorm::build_avx2_renorm_table()
}

/// Build a known frequency model and its packed table.
fn uniform256_table() -> PackedWordTable {
    let mut freqs = [0u32; 256];
    let mut cum = [0u32; 257];
    for i in 0..256 {
        freqs[i] = 16;
        cum[i + 1] = cum[i] + 16;
    }
    PackedWordTable::from_freqs(&freqs, &cum, 12).expect("uniform256 table")
}

/// Build a skewed frequency model for testing general 16-way decode.
fn skewed_table() -> PackedWordTable {
    let mut freqs = [0u32; 256];
    let mut remaining = 4096u32;
    for i in 0usize..10 {
        let f = remaining / (10 - i as u32);
        freqs[i] = f;
        remaining -= f;
    }
    for i in 10usize..256 {
        if remaining > 0 {
            freqs[i] = 1;
            remaining -= 1;
        }
    }
    if remaining > 0 {
        freqs[255] += remaining;
    }
    let mut cum = [0u32; 257];
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }
    PackedWordTable::from_freqs(&freqs, &cum, 12).expect("skewed table")
}

/// Encode data with the canonical 16-way encoder.
fn encode_16way(data: &[u8], freqs: &[u32], cum: &[u32]) -> Vec<u16> {
    use crate::packed_table::encode_interleaved16;
    encode_interleaved16(data, freqs, cum, 12).expect("encode 16-way")
}

/// Encode data with the canonical 8-way encoder.
fn encode_8way(data: &[u8], freqs: &[u32], cum: &[u32]) -> Vec<u16> {
    crate::encode_8way_for_test(data, freqs, cum)
}

fn is_avx2_supported() -> bool {
    #[cfg(feature = "std")]
    {
        std::is_x86_feature_detected!("avx2")
    }
    #[cfg(not(feature = "std"))]
    {
        cfg!(target_feature = "avx2")
    }
}

// ======================================================================
// 256-mask renormalization exhaustion
// ======================================================================

#[test]
fn test_renorm_exhaustive_256_masks() {
    use core::arch::x86_64::*;
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();

    unsafe {
        for mask in 0..=255u8 {
            let mut state_arr = [0u32; 8];
            for lane in 0..8 {
                if (mask >> lane) & 1 == 1 {
                    state_arr[lane] = lane as u32;
                } else {
                    state_arr[lane] = crate::RANS_WORD_L + lane as u32;
                }
            }
            let states = _mm256_loadu_si256(state_arr.as_ptr() as *const __m256i);
            let input: Vec<u16> = (0..8).map(|i| (100 + i) as u16).collect();
            let result = renorm8_avx2(states, &input, &pt).expect("renorm should succeed");

            assert_eq!(
                result.observed_mask, mask,
                "mask {:08b}: expected mask {:08b}, got {:08b}",
                mask, mask, result.observed_mask
            );

            let expected_words = mask.count_ones() as usize;
            assert_eq!(
                result.words_consumed, expected_words,
                "mask {:08b}: expected {} words, got {}",
                mask, expected_words, result.words_consumed
            );

            let mut expected_states = state_arr;
            let mut compact_idx = 0usize;
            for lane in 0..8 {
                if (mask >> lane) & 1 == 1 {
                    expected_states[lane] = (state_arr[lane] << 16) | input[compact_idx] as u32;
                    compact_idx += 1;
                }
            }

            for lane in 0..8 {
                assert_eq!(
                    result.states[lane], expected_states[lane],
                    "mask {:08b} lane {}: expected 0x{:08X}, got 0x{:08X}",
                    mask, lane, expected_states[lane], result.states[lane]
                );
            }
        }
    }
}

#[test]
fn test_renorm_exact_input_succeeds() {
    use core::arch::x86_64::*;
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    unsafe {
        let state_arr = [0u32, 1, 2, 3, 4, 5, 6, 7];
        let states = _mm256_loadu_si256(state_arr.as_ptr() as *const __m256i);
        let input = vec![100u16, 101, 102, 103, 104, 105, 106, 107];
        let result = renorm8_avx2(states, &input, &pt);
        assert!(result.is_ok(), "exact input should succeed");
    }
}

#[test]
fn test_renorm_one_word_short_fails() {
    use core::arch::x86_64::*;
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    unsafe {
        let state_arr = [0u32, 1, 2, 3, 4, 5, 6, 7];
        let states = _mm256_loadu_si256(state_arr.as_ptr() as *const __m256i);
        let input = vec![100u16, 101, 102, 103, 104, 105, 106];
        let result = renorm8_avx2(states, &input, &pt);
        assert!(matches!(result, Err(DecodeError::InputTooShort)));
    }
}

// ======================================================================
// Poison buffer tests
// ======================================================================

fn test_data() -> (Vec<u8>, Vec<u32>, Vec<u32>) {
    let data: Vec<u8> = (0..128).map(|i| (i % 256) as u8).collect();
    let total = 4096u32;
    let mut freqs = vec![0u32; 256];
    for &b in &data {
        freqs[b as usize] += 1;
    }
    let sum: u64 = freqs.iter().map(|&f| f as u64).sum();
    for f in &mut freqs {
        *f = ((*f as u64 * total as u64) / sum) as u32;
    }
    let current: u32 = freqs.iter().sum();
    freqs[255] += total - current;
    let mut cum = vec![0u32; 257];
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }
    (data, freqs, cum)
}

#[test]
fn test_avx2_manual_gather_poison() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");
    // Use 8-way encoded data for the 8-way decoder
    let encoded = encode_8way(&data, &freqs, &cum);

    // Scalar 8-way reference
    let (slots, slot2sym) = crate::build_word_tables(&freqs, &cum, 12);
    let tables = crate::RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };
    let scalar_out =
        crate::decode_8way_scalar(&encoded, &tables, data.len()).expect("scalar 8-way decode");

    unsafe {
        let mut poisoned = vec![0xAu8; data.len()];
        avx2::decode_interleaved8_avx2_manual_gather_into(&encoded, &table, &mut poisoned, &pt)
            .expect("AVX2 manual gather decode");
        assert_eq!(
            poisoned, scalar_out,
            "AVX2 manual gather must match scalar 8-way"
        );
    }
}

#[test]
fn test_avx2_hardware_gather_poison() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");
    let encoded = encode_8way(&data, &freqs, &cum);

    // Scalar 8-way reference
    let (slots, slot2sym) = crate::build_word_tables(&freqs, &cum, 12);
    let tables = crate::RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };
    let scalar_out =
        crate::decode_8way_scalar(&encoded, &tables, data.len()).expect("scalar 8-way decode");

    unsafe {
        let mut poisoned = vec![0xAu8; data.len()];
        avx2::decode_interleaved8_avx2_hardware_gather_into(&encoded, &table, &mut poisoned, &pt)
            .expect("AVX2 HW gather decode");
        assert_eq!(
            poisoned, scalar_out,
            "AVX2 HW gather must match scalar 8-way"
        );
    }
}

#[test]
fn test_avx2_2x8_poison() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");
    let encoded =
        crate::packed_table::encode_interleaved16(&data, &freqs, &cum, 12).expect("encode");

    let (scalar_out, _) =
        crate::packed_table::decode_interleaved16_scalar(&encoded, &table, data.len())
            .expect("scalar decode");

    unsafe {
        let mut poisoned = vec![0xAu8; data.len()];
        let report = avx2::decode_interleaved16_avx2_2x8_into(&encoded, &table, &mut poisoned, &pt)
            .expect("AVX2 2x8 decode");
        assert_eq!(poisoned, scalar_out, "AVX2 2x8 must match scalar");
        assert_eq!(report.words_consumed, encoded.len());
    }
}

#[test]
fn test_avx2_uniform256_poison() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let data: Vec<u8> = (0..256)
        .flat_map(|s| core::iter::repeat(s as u8).take(16))
        .collect();
    let table = uniform256_table();
    let uniform_cum = {
        let mut cum = [0u32; 257];
        for i in 0..256 {
            cum[i + 1] = cum[i] + 16;
        }
        cum
    };
    let encoded = crate::packed_table::encode_interleaved16(&data, &[16u32; 256], &uniform_cum, 12)
        .expect("encode uniform256");

    let (scalar_out, _) =
        crate::packed_table::decode_interleaved16_scalar(&encoded, &table, data.len())
            .expect("scalar decode");

    unsafe {
        let mut poisoned = vec![0xAu8; data.len()];
        let report = avx2::decode_interleaved16_uniform256_avx2_into(&encoded, &mut poisoned, &pt)
            .expect("AVX2 uniform256 decode");
        assert_eq!(poisoned, scalar_out, "AVX2 uniform256 must match scalar");
        assert_eq!(report.words_consumed, encoded.len());
    }
}

// ======================================================================
// Tail length tests
// ======================================================================

#[test]
fn test_avx2_2x8_all_tails() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");

    for len in 1..=33usize {
        let test_data: Vec<u8> = data.iter().copied().take(len).collect();
        let encoded = encode_16way(&test_data, &freqs, &cum);
        let (scalar_out, scalar_states, scalar_words) =
            unsafe { scalar_decode_unchecked(&encoded, &table, test_data.len()) };

        unsafe {
            let mut avx2_out = vec![0xAu8; test_data.len()];
            let report =
                avx2::decode_interleaved16_avx2_2x8_into(&encoded, &table, &mut avx2_out, &pt)
                    .expect("AVX2 2x8 decode");

            assert_eq!(
                avx2_out, scalar_out,
                "AVX2 2x8 tail test length {}: output mismatch",
                len
            );
            assert_eq!(
                report.words_consumed, scalar_words,
                "AVX2 2x8 tail test length {}: words consumed mismatch",
                len
            );
            for lane in 0..16 {
                assert_eq!(
                    report.final_states[lane], scalar_states[lane],
                    "AVX2 2x8 tail test length {}: final state lane {} mismatch",
                    len, lane
                );
            }
        }
    }
}

/// Scalar decode helper that can be used inside unsafe blocks.
unsafe fn scalar_decode_unchecked(
    words: &[u16],
    table: &PackedWordTable,
    len: usize,
) -> (Vec<u8>, [u32; 16], usize) {
    let (output, report) =
        crate::packed_table::decode_interleaved16_scalar(words, table, len).expect("scalar decode");
    (output, report.final_states, report.words_consumed)
}

// ======================================================================
// Truncation tests
// ======================================================================

#[test]
fn test_avx2_2x8_truncation() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");
    let encoded = encode_16way(&data, &freqs, &cum);

    for truncate_at in (33..encoded.len()).step_by(7) {
        let truncated: Vec<u16> = encoded.iter().copied().take(truncate_at).collect();
        unsafe {
            let mut output = vec![0xAu8; data.len()];
            let result =
                avx2::decode_interleaved16_avx2_2x8_into(&truncated, &table, &mut output, &pt);
            if let Ok(report) = result {
                if let Ok((scalar_out, _)) =
                    crate::packed_table::decode_interleaved16_scalar(&truncated, &table, data.len())
                {
                    assert_eq!(
                        output, scalar_out,
                        "truncated at {}: output mismatch",
                        truncate_at
                    );
                }
                let _ = report;
            }
        }
    }
}

// ======================================================================
// Hardware vs manual gather comparison
// ======================================================================

#[test]
fn test_avx2_manual_vs_hardware_gather_parity() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");
    let encoded =
        crate::packed_table::encode_interleaved16(&data, &freqs, &cum, 12).expect("encode");

    unsafe {
        let mut manual_out = vec![0xAu8; data.len()];
        let manual_report = avx2::decode_interleaved8_avx2_manual_gather_into(
            &encoded,
            &table,
            &mut manual_out,
            &pt,
        )
        .expect("manual gather decode");

        let mut hw_out = vec![0xAu8; data.len()];
        let hw_report =
            avx2::decode_interleaved8_avx2_hardware_gather_into(&encoded, &table, &mut hw_out, &pt)
                .expect("HW gather decode");

        assert_eq!(manual_out, hw_out, "manual and HW gather output must match");
        assert_eq!(manual_report.words_consumed, hw_report.words_consumed);
        assert_eq!(manual_report.final_states, hw_report.final_states);
    }
}

// ======================================================================
// Batch4 tests
// ======================================================================

#[test]
fn test_avx2_batch4_parity() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");
    let encoded = encode_16way(&data, &freqs, &cum);

    // Scalar reference
    let (scalar_out, scalar_states, scalar_words) =
        unsafe { scalar_decode_unchecked(&encoded, &table, data.len()) };

    // Batch4 decode (single job batch)
    let mut output = vec![0xAu8; data.len()];
    let mut jobs = [avx2::Avx2DecodeJob {
        compressed: &encoded,
        table: &table,
        output: &mut output,
        block_index: 0,
    }];

    unsafe {
        let reports =
            avx2::decode_batch4_interleaved16_avx2(&mut jobs, &pt).expect("batch4 decode");
        assert_eq!(reports.len(), 1);
        assert_eq!(output, scalar_out, "batch4 output must match scalar");
        assert_eq!(
            reports[0].words_consumed, scalar_words,
            "batch4 words consumed must match"
        );
        for lane in 0..16 {
            assert_eq!(
                reports[0].final_states[lane], scalar_states[lane],
                "batch4 final state lane {} mismatch",
                lane
            );
        }
    }
}

#[test]
fn test_avx2_batch4_multi_job() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let (data, freqs, cum) = test_data();
    let table = PackedWordTable::from_freqs(&freqs, &cum, 12).expect("table");

    // Create 3 jobs of varying lengths
    let data2: Vec<u8> = data.iter().copied().take(64).collect();
    let encoded1 = encode_16way(&data, &freqs, &cum);
    let encoded2 = encode_16way(&data2, &freqs, &cum);

    let (scalar1, _, _) = unsafe { scalar_decode_unchecked(&encoded1, &table, data.len()) };
    let (scalar2, _, _) = unsafe { scalar_decode_unchecked(&encoded2, &table, data2.len()) };

    let mut out1 = vec![0xAu8; data.len()];
    let mut out2 = vec![0xAu8; data2.len()];

    let mut jobs = [
        avx2::Avx2DecodeJob {
            compressed: &encoded1,
            table: &table,
            output: &mut out1,
            block_index: 0,
        },
        avx2::Avx2DecodeJob {
            compressed: &encoded2,
            table: &table,
            output: &mut out2,
            block_index: 1,
        },
    ];

    unsafe {
        let reports =
            avx2::decode_batch4_interleaved16_avx2(&mut jobs, &pt).expect("batch4 multi decode");
        assert_eq!(reports.len(), 2);
        assert_eq!(out1, scalar1, "batch4 job 0 must match scalar");
        assert_eq!(out2, scalar2, "batch4 job 1 must match scalar");
    }
}

#[test]
fn test_avx2_batch4_empty_jobs() {
    if !is_avx2_supported() {
        return;
    }
    let pt = perm_table();
    let mut jobs: [avx2::Avx2DecodeJob<'_>; 0] = [];
    unsafe {
        let reports = avx2::decode_batch4_interleaved16_avx2(&mut jobs, &pt).expect("batch4 empty");
        assert!(reports.is_empty());
    }
}
