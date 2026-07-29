//! # AVX-512 Word rANS decode kernels
//!
//! Two decode surfaces:
//!
//! 1. **AVX512VL.INTERLEAVED8** — 8-way interleaved decode using 256-bit vectors
//!    (AVX512VL + AVX512F + AVX512BW).  Consumes the *existing* canonical eight-way
//!    Word rANS stream format.
//!
//! 2. **AVX512.INTERLEAVED16** — 16-way interleaved decode using 512-bit vectors
//!    (AVX512F + AVX512BW).  Consumes the *new* sixteen-way Word rANS stream format.
//!
//! ## ISA requirements
//!
//! | Kernel | Required features |
//! |--------|------------------|
//! | `decode_interleaved8_avx512vl_kernel` | `avx512f, avx512vl, avx512bw` |
//! | `decode_interleaved16_avx512_kernel`  | `avx512f, avx512bw` |
//!
//! ## Safety
//!
//! All kernels are `unsafe` and require the caller to verify CPU feature support
//! via runtime detection before calling.  See `docs/unsafe-ledger.md`.

use crate::RANS_WORD_L;
use crate::RANS_WORD_M;
use crate::packed_table::{DecodeReport, PackedWordTable};
use alloc::vec::Vec;
use core::arch::x86_64::*;

// ---------------------------------------------------------------------------
// Renormalization mask tables
// ---------------------------------------------------------------------------

/// Number of u16 words consumed for each 8-bit lane mask (AVX512VL 8-way).
static NUM_WORDS_8: [u8; 256] = {
    let mut table = [0u8; 256];
    let mut mask: u8 = 0;
    while mask < 255 {
        let mut count: u8 = 0;
        let mut m = mask;
        while m > 0 {
            count += m & 1u8;
            m >>= 1;
        }
        table[mask as usize] = count;
        mask += 1;
    }
    table[255] = 8;
    table
};

/// Number of u16 words consumed for each 16-bit lane mask (AVX512 16-way).
static NUM_WORDS_16: [u8; 65536] = {
    let mut table = [0u8; 65536];
    let mut mask: u16 = 0;
    while mask < 65535 {
        let mut count: u16 = 0;
        let mut m = mask;
        while m > 0 {
            count += (m & 1) as u16;
            m >>= 1;
        }
        table[mask as usize] = count as u8;
        mask += 1;
    }
    table[65535] = 16;
    table
};

// ---------------------------------------------------------------------------
// AVX512VL.INTERLEAVED8: 8-way decode using 256-bit vectors
// ---------------------------------------------------------------------------

/// Decode 8 symbols using AVX512VL + AVX512BW (256-bit vectors).
///
/// # Safety
///
/// - Requires `avx512f, avx512vl, avx512bw` CPU features.
/// - `compressed` must have at least 16 u16 words.
/// - `table` must have exactly 4096 entries aligned for gather.
/// - `expected_len` must match the actual symbol count.
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn decode_interleaved8_avx512vl_kernel(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    if compressed.len() < 16 {
        return Err("compressed too short for 8 init states (AVX512VL)");
    }

    // Load initial 8 states from the first 16 u16 words.
    // Each state is stored as [low, high] pair, so state[i] = low | (high << 16).
    // We use a small scalar loop for correct deinterleaving, then load into SIMD.
    let mut init_array = [0u32; 8];
    for i in 0..8 {
        init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
    }
    let mut state = _mm256_loadu_si256(init_array.as_ptr() as *const __m256i);

    let mut reader_pos = 16usize;
    let n = expected_len;
    let even8 = n & !7;
    let mut output = Vec::with_capacity(n);
    output.resize(n, 0u8);

    let table_ptr = table.as_ptr() as *const i32;
    let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
    const SCALE8: i32 = 12;

    for i in (0..even8).step_by(8) {
        // ---- Gather phase ----
        let indices = _mm256_and_si256(state, mask_v);
        let gathered = _mm256_i32gather_epi32(table_ptr, indices, 4);

        // Unpack: freq = gathered & 0xfff, bias = (gathered >> 12) & 0xfff, symbol = gathered >> 24
        let freq_mask = _mm256_set1_epi32(0x0fff);
        let freq_v = _mm256_and_si256(gathered, freq_mask);
        let bias_v = _mm256_and_si256(_mm256_srli_epi32(gathered, 12), freq_mask);
        let symbols_v = _mm256_srli_epi32(gathered, 24);

        // Store symbols in lane order via temp buffer.
        let mut sym_buf: [u32; 8] = core::mem::zeroed();
        _mm256_storeu_si256(sym_buf.as_mut_ptr() as *mut __m256i, symbols_v);
        for lane in 0..8 {
            output[i + lane] = sym_buf[lane] as u8;
        }

        // ---- State update ----
        let xscaled = _mm256_srli_epi32(state, SCALE8);
        let new_state = _mm256_add_epi32(_mm256_mullo_epi32(xscaled, freq_v), bias_v);

        // ---- Renormalization ----
        let renorm_mask = _mm256_cmplt_epu32_mask(new_state, _mm256_set1_epi32(RANS_WORD_L as i32));
        let words_needed = NUM_WORDS_8[renorm_mask as usize] as usize;

        if words_needed > 0 {
            if reader_pos + words_needed > compressed.len() {
                return Err("unexpected EOF in AVX512VL renorm");
            }
            let mut temp_state = new_state;
            let mut rp = reader_pos;
            for lane in 0..8 {
                if (renorm_mask >> lane) & 1 != 0 {
                    let w = compressed[rp] as u32;
                    rp += 1;
                    let mut lanes: [u32; 8] = core::mem::zeroed();
                    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, temp_state);
                    lanes[lane] = (lanes[lane] << 16) | w;
                    temp_state = _mm256_loadu_si256(lanes.as_ptr() as *const __m256i);
                }
            }
            state = temp_state;
            reader_pos = rp;
        } else {
            state = new_state;
        }
    }

    // ---- Tail: fall back to scalar per-lane ----
    for i in even8..n {
        let lane = i & 7;
        let mut lanes: [u32; 8] = core::mem::zeroed();
        _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, state);
        let x = lanes[lane];
        let slot = x as usize & (RANS_WORD_M - 1);
        let entry = (*table.get(slot)).0;
        output[i] = (entry >> 24) as u8;
        let freq_entry = entry & 0x0fff;
        let bias_entry = (entry >> 12) & 0x0fff;
        let new_x = freq_entry * (x >> 12) + bias_entry;
        lanes[lane] = new_x;
        if new_x < RANS_WORD_L {
            if reader_pos >= compressed.len() {
                return Err("unexpected EOF in AVX512VL tail renorm");
            }
            lanes[lane] = (new_x << 16) | compressed[reader_pos] as u32;
            reader_pos += 1;
        }
        state = _mm256_loadu_si256(lanes.as_ptr() as *const __m256i);
    }

    // Collect final states
    let mut final_states = [0u32; 8];
    _mm256_storeu_si256(final_states.as_mut_ptr() as *mut __m256i, state);

    let report = DecodeReport {
        words_consumed: reader_pos,
        final_states: [
            final_states[0],
            final_states[1],
            final_states[2],
            final_states[3],
            final_states[4],
            final_states[5],
            final_states[6],
            final_states[7],
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ],
    };

    Ok((output, report))
}

// ---------------------------------------------------------------------------
// AVX512.INTERLEAVED16: 16-way decode using 512-bit vectors
// ---------------------------------------------------------------------------

/// Decode 16 symbols using AVX512 (512-bit vectors, masked operations).
///
/// # Safety
///
/// - Requires `avx512f, avx512bw` CPU features.
/// - `compressed` must have at least 32 u16 words.
/// - `table` must have exactly 4096 entries aligned for gather.
/// - `expected_len` must match the actual symbol count.
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_avx512_kernel(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    if compressed.len() < 32 {
        return Err("compressed too short for 16 init states (AVX512)");
    }

    // Load 16 initial states from the first 32 u16 words.
    // Each state is stored as [low, high] pair, so state[i] = low | (high << 16).
    let mut init_array = [0u32; 16];
    for i in 0..16 {
        init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
    }
    let mut state = _mm512_loadu_si512(init_array.as_ptr() as *const __m512i);

    let mut reader_pos = 32usize;
    let n = expected_len;
    let even16 = n & !15;
    let mut output = Vec::with_capacity(n);
    output.resize(n, 0u8);

    let table_ptr = table.as_ptr() as *const i32;
    let mask_v = _mm512_set1_epi32((RANS_WORD_M - 1) as i32);
    let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);
    const SCALE16: u32 = 12;

    for i in (0..even16).step_by(16) {
        // ---- Gather phase ----
        let indices = _mm512_and_si512(state, mask_v);
        let gathered = _mm512_i32gather_epi32(indices, table_ptr, 4);

        // Unpack fields
        let freq_v = _mm512_and_si512(gathered, _mm512_set1_epi32(0x0fff));
        let bias_v = _mm512_and_si512(_mm512_srli_epi32(gathered, 12), _mm512_set1_epi32(0x0fff));
        let symbols_v = _mm512_srli_epi32(gathered, 24);

        // Store symbols in lane order using a temporary buffer.
        // The packus approach has complex interleaving that doesn't preserve lane order.
        // Instead, store to temp and copy low bytes.
        let mut sym_buf: [u32; 16] = core::mem::zeroed();
        _mm512_storeu_si512(sym_buf.as_mut_ptr() as *mut __m512i, symbols_v);
        for lane in 0..16 {
            output[i + lane] = sym_buf[lane] as u8;
        }

        // ---- State update ----
        let xscaled = _mm512_srli_epi32(state, SCALE16);
        let new_state = _mm512_add_epi32(_mm512_mullo_epi32(xscaled, freq_v), bias_v);

        // ---- Masked renormalization ----
        let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
        let words_needed = NUM_WORDS_16[renorm_mask as usize] as usize;

        if words_needed > 0 {
            if reader_pos + words_needed > compressed.len() {
                return Err("unexpected EOF in AVX512 renorm");
            }
            let mut temp_state = new_state;
            let mut rp = reader_pos;
            for lane in 0..16 {
                if (renorm_mask >> lane) & 1 != 0 {
                    let w = compressed[rp] as u32;
                    rp += 1;
                    let mut lanes: [u32; 16] = core::mem::zeroed();
                    _mm512_storeu_si512(lanes.as_mut_ptr() as *mut __m512i, temp_state);
                    lanes[lane] = (lanes[lane] << 16) | w;
                    temp_state = _mm512_loadu_si512(lanes.as_ptr() as *const __m512i);
                }
            }
            state = temp_state;
            reader_pos = rp;
        } else {
            state = new_state;
        }
    }

    // ---- Tail: scalar fallback ----
    for i in even16..n {
        let lane = i & 15;
        let mut lanes: [u32; 16] = core::mem::zeroed();
        _mm512_storeu_si512(lanes.as_mut_ptr() as *mut __m512i, state);
        let x = lanes[lane];
        let slot = x as usize & (RANS_WORD_M - 1);
        let entry = (*table.get(slot)).0;
        output[i] = (entry >> 24) as u8;
        let freq_entry = entry & 0x0fff;
        let bias_entry = (entry >> 12) & 0x0fff;
        let new_x = freq_entry * (x >> 12) + bias_entry;
        lanes[lane] = new_x;
        if new_x < RANS_WORD_L {
            if reader_pos >= compressed.len() {
                return Err("unexpected EOF in AVX512 tail renorm");
            }
            lanes[lane] = (new_x << 16) | compressed[reader_pos] as u32;
            reader_pos += 1;
        }
        state = _mm512_loadu_si512(lanes.as_ptr() as *const __m512i);
    }

    // Collect final states
    let mut final_states = [0u32; 16];
    _mm512_storeu_si512(final_states.as_mut_ptr() as *mut __m512i, state);

    let report = DecodeReport {
        words_consumed: reader_pos,
        final_states,
    };

    Ok((output, report))
}

// ---------------------------------------------------------------------------
// Mask exhaustion tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packed_table::{decode_8way_packed_scalar, encode_interleaved16};
    use alloc::vec;

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

    #[test]
    fn test_avx512vl8_equivalence_scalar() {
        let (freqs, cum, packed) = uniform_model();
        if !is_avx512vl_available() {
            // Skipping AVX512VL test: not compiled with avx512f+avx512vl+avx512bw
            return;
        }

        let symbols: Vec<u8> = (0..128).map(|i| (i % 16) as u8).collect();
        let compressed = crate::encode_8way_for_test(&symbols, &freqs, &cum);

        // Scalar decode
        let scalar_out = decode_8way_packed_scalar(&compressed, &packed, symbols.len()).unwrap();

        // AVX512VL decode
        unsafe {
            let (avx_out, _avx_report) =
                decode_interleaved8_avx512vl_kernel(&compressed, &packed, symbols.len()).unwrap();
            assert_eq!(avx_out, symbols, "AVX512VL roundtrip must match input");
            assert_eq!(avx_out, scalar_out, "AVX512VL must match scalar");
        }
    }

    #[test]
    fn test_avx512vl8_various_lengths() {
        let (freqs, cum, packed) = uniform_model();
        if !is_avx512vl_available() {
            // Skipping AVX512VL test: not compiled with avx512f+avx512vl+avx512bw
            return;
        }
        let lengths: &[usize] = &[
            1, 2, 3, 4, 5, 6, 7, 8, 9, 16, 17, 63, 64, 65, 127, 128, 129, 255, 256, 257, 511, 512,
            513, 1023, 1024, 1025,
        ];
        for &len in lengths {
            let symbols: Vec<u8> = (0..len).map(|i| (i % 16) as u8).collect();
            let compressed = crate::encode_8way_for_test(&symbols, &freqs, &cum);
            unsafe {
                let (decoded, _) =
                    decode_interleaved8_avx512vl_kernel(&compressed, &packed, symbols.len())
                        .unwrap();
                assert_eq!(decoded, symbols, "AVX512VL len={} failed", len);
            }
        }
    }

    #[test]
    fn test_avx512_16way_equivalence() {
        let (freqs, cum, packed) = uniform_model();
        if !is_avx512_available() {
            // Skipping AVX512 16-way test: not compiled with avx512f+avx512bw
            return;
        }

        let symbols: Vec<u8> = (0..128).map(|i| (i % 16) as u8).collect();
        let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);

        // Scalar decode
        let (scalar_out, scalar_report) =
            crate::packed_table::decode_interleaved16_scalar(&compressed, &packed, symbols.len())
                .unwrap();

        // AVX512 decode
        unsafe {
            let (avx_out, avx_report) =
                decode_interleaved16_avx512_kernel(&compressed, &packed, symbols.len()).unwrap();
            assert_eq!(avx_out, symbols, "AVX512 16-way roundtrip");
            assert_eq!(avx_out, scalar_out, "AVX512 16-way must match scalar");
            assert_eq!(
                avx_report.words_consumed, scalar_report.words_consumed,
                "AVX512 16-way words consumed must match scalar"
            );
        }
    }

    #[test]
    fn test_avx512_16way_all_tails() {
        let (freqs, cum, packed) = uniform_model();
        if !is_avx512_available() {
            // Skipping AVX512 16-way test: not compiled with avx512f+avx512bw
            return;
        }
        for tail in 0..16 {
            let len = 32 + tail;
            let symbols: Vec<u8> = (0..len).map(|i| (i % 16) as u8).collect();
            let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);
            unsafe {
                let (decoded, _) =
                    decode_interleaved16_avx512_kernel(&compressed, &packed, symbols.len())
                        .unwrap();
                assert_eq!(decoded, symbols, "AVX512 16-way tail={} failed", tail);
            }
        }
    }

    #[test]
    fn test_avx512_16way_truncated_rejected() {
        let (freqs, cum, packed) = uniform_model();
        if !is_avx512_available() {
            // Skipping AVX512 16-way truncated test
            return;
        }
        unsafe {
            assert!(decode_interleaved16_avx512_kernel(&[], &packed, 16).is_err());
            let short = vec![0u16; 31];
            assert!(decode_interleaved16_avx512_kernel(&short, &packed, 16).is_err());
        }
    }

    /// Compile-time check for AVX512F + AVX512VL + AVX512BW availability.
    fn is_avx512vl_available() -> bool {
        cfg!(all(
            target_feature = "avx512f",
            target_feature = "avx512vl",
            target_feature = "avx512bw",
        ))
    }

    /// Compile-time check for AVX512F + AVX512BW availability.
    fn is_avx512_available() -> bool {
        cfg!(all(target_feature = "avx512f", target_feature = "avx512bw",))
    }
}
