//! # AVX2 Word rANS decode kernels
//!
//! This module implements AVX2-accelerated decode surfaces for existing
//! eight-way and sixteen-way Word rANS stream formats.  All kernels are
//! bitstream-compatible with the scalar reference implementations.
//!
//! ## Backend surfaces
//!
//! | Function | Backend | Stream | Table access |
//! |----------|---------|--------|-------------|
//! | `decode_interleaved8_avx2_manual_gather_into` | Manual gather 8-way | 8-way | Scalar loads into vector |
//! | `decode_interleaved8_avx2_hardware_gather_into` | Hardware gather 8-way | 8-way | `VPGATHERDD` |
//! | `decode_interleaved16_avx2_2x8_into` | 2×8 on 16-way | 16-way | `VPGATHERDD` 2× |
//! | `decode_interleaved16_uniform256_avx2_into` | Uniform256 table-free | 16-way | None (arithmetic only) |
//!
//! ## Safety
//!
//! All `_into` functions are `unsafe` and require the caller to verify AVX2
//! runtime support via `std::is_x86_feature_detected!("avx2")` before calling.
//! The safe `_checked` wrappers in `backends.rs` perform runtime detection.
//!
//! ## AVX2 vs AVX-512 differences
//!
//! AVX2 lacks several facilities that AVX-512 provides:
//!
//! 1. **No `_mm256_cmplt_epu32_mask`**: Use signed compare trick
//!    (XOR with 0x80000000, signed compare, extract using movemask).
//! 2. **No `_mm256_maskz_expand_epi32`**: Use permutation table
//!    (`vpermd` with precomputed indices per mask).
//! 3. **No `_mm256_mask_blend_epi32`**: Use `_mm256_blendv_epi8`
//!    with a broadcast blend mask.
//! 4. **No `_mm256_cvtepi32_epi8` (VPMOVDB)**: Use two-step pack:
//!    `_mm256_packus_epi32` → `_mm_packus_epi16` → store.

use crate::RANS_WORD_L;
use crate::RANS_WORD_M;
use crate::avx2_renorm::{Avx2RenormPermutations, renorm8_avx2};
use crate::packed_table::{DecodeReport, PackedWordTable};
use alloc::vec;
use alloc::vec::Vec;
use core::arch::x86_64::*;

/// The static AVX2 renormalization permutation table.
/// Built once and shared across all AVX2 decode kernels.
#[derive(Debug)]
pub struct Avx2Context {
    pub perm_table: Avx2RenormPermutations,
}

impl Avx2Context {
    /// Create a new AVX2 decode context with pre-built tables.
    pub fn new() -> Self {
        Self {
            perm_table: crate::avx2_renorm::build_avx2_renorm_table(),
        }
    }
}

impl Default for Avx2Context {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helper: 8-way unsigned compare mask for AVX2
// ---------------------------------------------------------------------------

/// Compute an 8-bit mask where bit N is 1 if state[N] < RANS_WORD_L.
///
/// Uses the signed compare trick: XOR each state with 0x80000000, then
/// compare signed against biased RANS_WORD_L.  The movemask extracts
/// the sign bit of each 32-bit lane, which we compact to 8 bits.
///
/// `_mm256_movemask_epi8` returns an i32 whose bit N is the high bit of byte N.
/// For 32-bit lanes, bytes 0-3 correspond to lane 0, bytes 4-7 to lane 1, etc.
/// The high bit of lane N is at bit position N*4+3 = 4*N+3.
///
/// # Safety
///
/// Requires AVX2 at runtime.
#[target_feature(enable = "avx2")]
unsafe fn avx2_renorm_mask_8way(states: __m256i) -> u8 {
    unsafe {
        let flip = _mm256_set1_epi32(i32::MIN);
        let biased = _mm256_xor_si256(states, flip);
        let biased_l = _mm256_set1_epi32((RANS_WORD_L as i32) ^ i32::MIN);
        // cmplt: biased_l > biased_states
        let cmp = _mm256_cmpgt_epi32(biased_l, biased);
        let mm: i32 = _mm256_movemask_epi8(cmp);
        // The 32-bit movemask has 32 bits (one per byte).
        // For 32-bit lane i, the byte at position 4*i+3 (bit index 4*i*8+3*8 = 32*i+24)
        // Actually: _mm256_movemask_epi8 extracts bit 7 of each byte.
        // Lane 0: bytes 0-3 -> bits 0, 8, 16, 24. The significant one is bit 24 (byte 3).
        // Lane 1: bytes 4-7 -> bits 32, 40, 48, 56. The significant one is bit 56 (byte 7).
        // But mm is i32 with only 32 bits! The high bits are lost.
        // We need a different approach.
        //
        // Actually _mm256_movemask_epi8 returns 32 bits in a 32-bit integer.
        // For 256-bit vector: bytes 0-31.
        // Lane 0: bytes 0-3 -> movemask bits 0,1,2,3 (bit 3 is byte 3's high bit)
        // Lane i: byte 4*i+3 -> movemask bit 4*i+3
        //
        // So the lane mask is: for lane i, ((mm >> (4*i + 3)) & 1) << i
        let mut result: u8 = 0;
        for lane in 0..8 {
            let bit_pos = 4 * lane + 3;
            if ((mm >> bit_pos) & 1) != 0 {
                result |= 1 << lane;
            }
        }
        result
    }
}

/// Compact 8-bit lane mask to a blend vector.
///
/// Converts an 8-bit mask (bit N = active lane N) into a `__m256i` where
/// active lanes are all-ones (0xFFFFFFFF) and inactive lanes are all-zeros.
///
/// # Safety
///
/// Requires AVX2 at runtime.
#[target_feature(enable = "avx2")]
unsafe fn mask_to_blend_epi32(mask_u8: u8) -> __m256i {
    unsafe {
        // Broadcast the 8-bit mask to all 32-bit lanes.
        let b = _mm256_set1_epi32(mask_u8 as i32);
        // Shift right by lane index to get the bit for each lane.
        let lane_indices = _mm256_set_epi32(7, 6, 5, 4, 3, 2, 1, 0);
        let shifted = _mm256_srlv_epi32(b, lane_indices);
        let bit = _mm256_and_si256(shifted, _mm256_set1_epi32(1));
        // All-ones for active lanes, all-zeros for inactive: 0 - bit
        _mm256_sub_epi32(_mm256_setzero_si256(), bit)
    }
}

// ---------------------------------------------------------------------------
// Helper: direct symbol store (two-step pack)
// ---------------------------------------------------------------------------

/// Pack 8 × u32 symbols into 8 bytes and store at the output pointer.
///
/// AVX2 lacks `_mm256_cvtepi32_epi8` (VPMOVDB, which is AVX-512VL).
/// We use `_mm256_packus_epi32` (vpackusdw) to narrow u32→u16, then
/// `_mm_packus_epi16` (vpackuswb) to narrow u16→u8, then store 8 bytes.
///
/// Lane order after two-step pack:
///   input:  [s0, s1, s2, s3, s4, s5, s6, s7]
///   after packus_epi32: [s0, s1, s2, s3] (low), [s4, s5, s6, s7] (high)
///   vpacks sdwords to words: lo=[s0,s1,s2,s3], hi=[s4,s5,s6,s7]
///   vpackuswb: [s0,s1,s2,s3,s4,s5,s6,s7]
///
/// # Safety
///
/// Requires AVX2 at runtime.  `output` must be writable for 8 bytes.
#[target_feature(enable = "avx2")]
unsafe fn store_symbols_8way(symbols32: __m256i, output: *mut u8) {
    unsafe {
        // Step 1: Pack u32 → u16 using unsigned saturation pack.
        let lo = _mm256_castsi256_si128(symbols32);
        let hi = _mm256_extracti128_si256(symbols32, 1);
        let words16 = _mm_packus_epi32(lo, hi);
        // Step 2: Pack u16 → u8 using unsigned saturation pack.
        let bytes8 = _mm_packus_epi16(words16, _mm_setzero_si128());
        // Store 8 bytes.
        _mm_storel_epi64(output as *mut __m128i, bytes8);
    }
}

// ---------------------------------------------------------------------------
// 1. AVX2 manual-gather eight-way decoder
// ---------------------------------------------------------------------------
//
// This decoder uses scalar `PackedWordTable::get()` calls to load individual
// table entries into a YMM register.  Each gather iteration:
//   1. Extract 8 slot indices from state vector.
//   2. Store indices to a [u32; 8] buffer.
//   3. Load 8 packed entries via scalar loads into the buffer.
//   4. Load buffer into a YMM register.
//
// While this is slower than hardware gather, it avoids gather-related
// penalties on CPUs where VPGATHERDD is microcoded (pre-Ice Lake Intel).

/// Decode 8 symbols using AVX2 with manual (scalar) table loads.
///
/// # Safety
///
/// - Requires AVX2 at runtime.
/// - `compressed` must have at least 16 u16 words.
/// - `output.len()` must equal the number of symbols to decode.
/// - `perm_table` must be a valid Avx2RenormPermutations.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved8_avx2_manual_gather_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
    perm_table: &Avx2RenormPermutations,
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let n = output.len();
        if n == 0 {
            return Ok(DecodeReport {
                words_consumed: 0,
                final_states: [0u32; 16],
            });
        }
        if compressed.len() < 16 {
            return Err("compressed too short for 8 init states (AVX2)");
        }

        // Load initial states
        let mut init = [0u32; 8];
        for i in 0..8 {
            init[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state = _mm256_loadu_si256(init.as_ptr() as *const __m256i);
        let mut reader_pos = 16usize;
        let even8 = n & !7;
        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
        const SCALE8: i32 = 12;

        for i in (0..even8).step_by(8) {
            // ---- Manual gather: extract indices, scalar loads, insert ----
            let indices = _mm256_and_si256(state, mask_v);
            let mut idx_buf: [u32; 8] = core::mem::zeroed();
            _mm256_storeu_si256(idx_buf.as_mut_ptr() as *mut __m256i, indices);

            let mut entry_buf = [0u32; 8];
            for lane in 0..8 {
                let slot = idx_buf[lane] as usize;
                entry_buf[lane] = table.get(slot).0; // PackedWordEntry.0 = u32
            }
            let gathered = _mm256_loadu_si256(entry_buf.as_ptr() as *const __m256i);

            // Unpack fields
            let freq_mask = _mm256_set1_epi32(0x0fff);
            let freq_v = _mm256_and_si256(gathered, freq_mask);
            let bias_v = _mm256_and_si256(_mm256_srli_epi32(gathered, 12), freq_mask);
            let symbols_v = _mm256_srli_epi32(gathered, 24);

            // Store symbols directly
            store_symbols_8way(symbols_v, output.as_mut_ptr().add(i));

            // State update
            let xscaled = _mm256_srli_epi32(state, SCALE8);
            let new_state = _mm256_add_epi32(_mm256_mullo_epi32(xscaled, freq_v), bias_v);

            // Renormalization using AVX2 renorm primitive
            let renorm_result = match renorm8_avx2(new_state, &compressed[reader_pos..], perm_table)
            {
                Ok(r) => r,
                Err(_) => return Err("unexpected EOF in AVX2 manual-gather renorm"),
            };

            let mut final_state_arr = [0u32; 8];
            final_state_arr.copy_from_slice(&renorm_result.states);
            state = _mm256_loadu_si256(final_state_arr.as_ptr() as *const __m256i);
            reader_pos += renorm_result.words_consumed;
        }

        // Tail: scalar fallback
        for i in even8..n {
            let lane = i & 7;
            let mut lanes: [u32; 8] = core::mem::zeroed();
            _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, state);
            let x = lanes[lane];
            let slot = x as usize & (RANS_WORD_M - 1);
            let entry = table.get(slot).0;
            output[i] = (entry >> 24) as u8;
            let freq_entry = entry & 0x0fff;
            let bias_entry = (entry >> 12) & 0x0fff;
            let new_x = freq_entry * (x >> 12) + bias_entry;
            lanes[lane] = new_x;
            if new_x < RANS_WORD_L {
                if reader_pos >= compressed.len() {
                    return Err("unexpected EOF in AVX2 manual-gather tail");
                }
                lanes[lane] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            state = _mm256_loadu_si256(lanes.as_ptr() as *const __m256i);
        }

        // Collect final states
        let mut final_states = [0u32; 16];
        let mut lo_buf = [0u32; 8];
        _mm256_storeu_si256(lo_buf.as_mut_ptr() as *mut __m256i, state);
        for j in 0..8 {
            final_states[j] = lo_buf[j];
        }

        Ok(DecodeReport {
            words_consumed: reader_pos,
            final_states,
        })
    }
}

/// Allocating wrapper for AVX2 manual-gather 8-way decode.
///
/// # Safety
///
/// Requires AVX2 CPU feature at runtime.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved8_avx2_manual_gather(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
    perm_table: &Avx2RenormPermutations,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        let mut output = vec![0u8; expected_len];
        let report = decode_interleaved8_avx2_manual_gather_into(
            compressed,
            table,
            &mut output,
            perm_table,
        )?;
        Ok((output, report))
    }
}

// ---------------------------------------------------------------------------
// 2. AVX2 hardware-gather eight-way decoder
// ---------------------------------------------------------------------------
//
// Uses `_mm256_i32gather_epi32` to load 8 packed table entries in one
// instruction.  On CPUs with efficient gather (Skylake-X, Ice Lake+,
// Zen 4+), this is faster than manual scalar loads.

/// Decode 8 symbols using AVX2 with hardware gather (`VPGATHERDD`).
///
/// # Safety
///
/// - Requires AVX2 at runtime.
/// - `compressed` must have at least 16 u16 words.
/// - `output.len()` must equal the number of symbols to decode.
/// - `perm_table` must be a valid Avx2RenormPermutations.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved8_avx2_hardware_gather_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
    perm_table: &Avx2RenormPermutations,
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let n = output.len();
        if n == 0 {
            return Ok(DecodeReport {
                words_consumed: 0,
                final_states: [0u32; 16],
            });
        }
        if compressed.len() < 16 {
            return Err("compressed too short for 8 init states (AVX2 HW gather)");
        }

        let mut init = [0u32; 8];
        for i in 0..8 {
            init[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state = _mm256_loadu_si256(init.as_ptr() as *const __m256i);
        let mut reader_pos = 16usize;
        let even8 = n & !7;
        let table_ptr = table.as_ptr() as *const i32;
        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
        const SCALE8: i32 = 12;

        for i in (0..even8).step_by(8) {
            // Hardware gather: one instruction loads 8 × u32 from 8 different slots
            let indices = _mm256_and_si256(state, mask_v);
            let gathered = _mm256_i32gather_epi32(table_ptr, indices, 4);

            // Unpack fields
            let freq_mask = _mm256_set1_epi32(0x0fff);
            let freq_v = _mm256_and_si256(gathered, freq_mask);
            let bias_v = _mm256_and_si256(_mm256_srli_epi32(gathered, 12), freq_mask);
            let symbols_v = _mm256_srli_epi32(gathered, 24);

            // Direct symbol store
            store_symbols_8way(symbols_v, output.as_mut_ptr().add(i));

            // State update
            let xscaled = _mm256_srli_epi32(state, SCALE8);
            let new_state = _mm256_add_epi32(_mm256_mullo_epi32(xscaled, freq_v), bias_v);

            // Renormalization
            let renorm_result = match renorm8_avx2(new_state, &compressed[reader_pos..], perm_table)
            {
                Ok(r) => r,
                Err(_) => return Err("unexpected EOF in AVX2 HW-gather renorm"),
            };

            let mut buf = [0u32; 8];
            buf.copy_from_slice(&renorm_result.states);
            state = _mm256_loadu_si256(buf.as_ptr() as *const __m256i);
            reader_pos += renorm_result.words_consumed;
        }

        // Tail
        for i in even8..n {
            let lane = i & 7;
            let mut lanes: [u32; 8] = core::mem::zeroed();
            _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, state);
            let x = lanes[lane];
            let slot = x as usize & (RANS_WORD_M - 1);
            let entry = table.get(slot).0;
            output[i] = (entry >> 24) as u8;
            let freq_entry = entry & 0x0fff;
            let bias_entry = (entry >> 12) & 0x0fff;
            let new_x = freq_entry * (x >> 12) + bias_entry;
            lanes[lane] = new_x;
            if new_x < RANS_WORD_L {
                if reader_pos >= compressed.len() {
                    return Err("unexpected EOF in AVX2 HW-gather tail");
                }
                lanes[lane] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            state = _mm256_loadu_si256(lanes.as_ptr() as *const __m256i);
        }

        let mut final_states = [0u32; 16];
        let mut lo_buf = [0u32; 8];
        _mm256_storeu_si256(lo_buf.as_mut_ptr() as *mut __m256i, state);
        for j in 0..8 {
            final_states[j] = lo_buf[j];
        }

        Ok(DecodeReport {
            words_consumed: reader_pos,
            final_states,
        })
    }
}

/// Allocating wrapper for AVX2 hardware-gather 8-way decode.
///
/// # Safety
///
/// Requires AVX2 CPU feature at runtime.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved8_avx2_hardware_gather(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
    perm_table: &Avx2RenormPermutations,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        let mut output = vec![0u8; expected_len];
        let report = decode_interleaved8_avx2_hardware_gather_into(
            compressed,
            table,
            &mut output,
            perm_table,
        )?;
        Ok((output, report))
    }
}

// ---------------------------------------------------------------------------
// 3. AVX2 two-by-eight on sixteen-way format
// ---------------------------------------------------------------------------
//
// Represents the 16 states as two 8-state vectors:
//   state_lo = lanes 0–7
//   state_hi = lanes 8–15
//
// Renormalization order: low lanes (0-7) first, then high lanes (8-15).
// This must match the scalar sixteen-way decoder exactly.

/// Decode 16-way interleaved Word rANS using AVX2 2×8.
///
/// # Safety
///
/// - Requires AVX2 at runtime.
/// - `compressed` must have at least 32 u16 words (16 init states × 2).
/// - `output.len()` must equal the number of symbols to decode.
/// - `perm_table` must be a valid Avx2RenormPermutations.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved16_avx2_2x8_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
    perm_table: &Avx2RenormPermutations,
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let n = output.len();
        if n == 0 {
            return Ok(DecodeReport {
                words_consumed: 0,
                final_states: [0u32; 16],
            });
        }
        if compressed.len() < 32 {
            return Err("compressed too short for 16 init states (AVX2 2x8)");
        }

        let mut init = [0u32; 16];
        for i in 0..16 {
            init[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state_lo = _mm256_loadu_si256(init[..8].as_ptr() as *const __m256i);
        let mut state_hi = _mm256_loadu_si256(init[8..].as_ptr() as *const __m256i);
        let mut reader_pos = 32usize;
        let even16 = n & !15;
        let table_ptr = table.as_ptr() as *const i32;
        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
        const SCALE8: i32 = 12;

        for i in (0..even16).step_by(16) {
            // ---- Low group: lanes 0-7 ----
            let idx_lo = _mm256_and_si256(state_lo, mask_v);
            let gath_lo = _mm256_i32gather_epi32(table_ptr, idx_lo, 4);
            let freq_lo = _mm256_and_si256(gath_lo, _mm256_set1_epi32(0x0fff));
            let bias_lo =
                _mm256_and_si256(_mm256_srli_epi32(gath_lo, 12), _mm256_set1_epi32(0x0fff));
            let syms_lo = _mm256_srli_epi32(gath_lo, 24);
            store_symbols_8way(syms_lo, output.as_mut_ptr().add(i));

            let xsc_lo = _mm256_srli_epi32(state_lo, SCALE8);
            let new_lo = _mm256_add_epi32(_mm256_mullo_epi32(xsc_lo, freq_lo), bias_lo);

            let rl = match renorm8_avx2(new_lo, &compressed[reader_pos..], perm_table) {
                Ok(r) => r,
                Err(_) => return Err("unexpected EOF in AVX2 2x8 lo renorm"),
            };
            let mut lo_buf = [0u32; 8];
            lo_buf.copy_from_slice(&rl.states);
            state_lo = _mm256_loadu_si256(lo_buf.as_ptr() as *const __m256i);
            reader_pos += rl.words_consumed;

            // ---- High group: lanes 8-15 ----
            let idx_hi = _mm256_and_si256(state_hi, mask_v);
            let gath_hi = _mm256_i32gather_epi32(table_ptr, idx_hi, 4);
            let freq_hi = _mm256_and_si256(gath_hi, _mm256_set1_epi32(0x0fff));
            let bias_hi =
                _mm256_and_si256(_mm256_srli_epi32(gath_hi, 12), _mm256_set1_epi32(0x0fff));
            let syms_hi = _mm256_srli_epi32(gath_hi, 24);
            store_symbols_8way(syms_hi, output.as_mut_ptr().add(i + 8));

            let xsc_hi = _mm256_srli_epi32(state_hi, SCALE8);
            let new_hi = _mm256_add_epi32(_mm256_mullo_epi32(xsc_hi, freq_hi), bias_hi);

            let rh = match renorm8_avx2(new_hi, &compressed[reader_pos..], perm_table) {
                Ok(r) => r,
                Err(_) => return Err("unexpected EOF in AVX2 2x8 hi renorm"),
            };
            let mut hi_buf = [0u32; 8];
            hi_buf.copy_from_slice(&rh.states);
            state_hi = _mm256_loadu_si256(hi_buf.as_ptr() as *const __m256i);
            reader_pos += rh.words_consumed;
        }

        // Tail: scalar fallback for remaining symbols
        for i in even16..n {
            let lane = i & 15;
            let (sv, lv) = if lane < 8 {
                (&state_lo, lane)
            } else {
                (&state_hi, lane - 8)
            };
            let mut lns: [u32; 8] = core::mem::zeroed();
            _mm256_storeu_si256(lns.as_mut_ptr() as *mut __m256i, *sv);
            let x = lns[lv];
            let slot = x as usize & (RANS_WORD_M - 1);
            let entry = table.get(slot).0;
            output[i] = (entry >> 24) as u8;
            let freq_entry = entry & 0x0fff;
            let bias_entry = (entry >> 12) & 0x0fff;
            let new_x = freq_entry * (x >> 12) + bias_entry;
            lns[lv] = new_x;
            if new_x < RANS_WORD_L {
                if reader_pos >= compressed.len() {
                    return Err("unexpected EOF in AVX2 2x8 tail");
                }
                lns[lv] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            let reload = _mm256_loadu_si256(lns.as_ptr() as *const __m256i);
            if lane < 8 {
                state_lo = reload;
            } else {
                state_hi = reload;
            }
        }

        // Merge final states
        let mut lo_buf = [0u32; 8];
        _mm256_storeu_si256(lo_buf.as_mut_ptr() as *mut __m256i, state_lo);
        let mut hi_buf = [0u32; 8];
        _mm256_storeu_si256(hi_buf.as_mut_ptr() as *mut __m256i, state_hi);
        let mut final_states = [0u32; 16];
        for j in 0..8 {
            final_states[j] = lo_buf[j];
            final_states[j + 8] = hi_buf[j];
        }

        Ok(DecodeReport {
            words_consumed: reader_pos,
            final_states,
        })
    }
}

/// Allocating wrapper for AVX2 2×8 sixteen-way decode.
///
/// # Safety
///
/// Requires AVX2 CPU feature at runtime.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved16_avx2_2x8(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
    perm_table: &Avx2RenormPermutations,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        let mut output = vec![0u8; expected_len];
        let report =
            decode_interleaved16_avx2_2x8_into(compressed, table, &mut output, perm_table)?;
        Ok((output, report))
    }
}

// ---------------------------------------------------------------------------
// 4. Uniform256 table-free AVX2 decoder
// ---------------------------------------------------------------------------
//
// Valid only when:
// - scale_bits == 12
// - 256 symbols, each with frequency == 16
// - cumulative[symbol] = symbol × 16
//
// State transition:
//   slot      = state & 4095
//   symbol    = slot >> 4   (slot / 16)
//   bias      = slot & 15   (slot % 16)
//   new_state = ((state >> 12) << 4) + bias
//
// Renormalization when new_state < 65536:
//   new_state = (new_state << 16) | word

/// Decode 16-way Uniform256 stream using AVX2 (table-free).
///
/// # Safety
///
/// - Requires AVX2 at runtime.
/// - Caller MUST validate that the model is Uniform256 (all 256 frequencies == 16,
///   scale_bits == 12) before calling this function.
/// - `compressed` must have at least 32 u16 words.
/// - `output.len()` must equal the number of symbols to decode.
/// - `perm_table` must be a valid Avx2RenormPermutations.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved16_uniform256_avx2_into(
    compressed: &[u16],
    output: &mut [u8],
    perm_table: &Avx2RenormPermutations,
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let n = output.len();
        if n == 0 {
            return Ok(DecodeReport {
                words_consumed: 0,
                final_states: [0u32; 16],
            });
        }
        if compressed.len() < 32 {
            return Err("compressed too short for 16 init states (AVX2 uniform256)");
        }

        let mut init = [0u32; 16];
        for i in 0..16 {
            init[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state_lo = _mm256_loadu_si256(init[..8].as_ptr() as *const __m256i);
        let mut state_hi = _mm256_loadu_si256(init[8..].as_ptr() as *const __m256i);
        let mut reader_pos = 32usize;
        let even16 = n & !15;

        // Uniform256 constants
        // slot       = state & 4095
        // symbol_u8  = slot >> 4
        // bias       = slot & 15
        // new_state  = ((state >> 12) << 4) + bias
        let slot_mask = _mm256_set1_epi32(0x0FFF);
        let shift4 = _mm256_set1_epi32(4); // for left shift after >>12 and for right shift
        const SHIFT12: i32 = 12;

        for i in (0..even16).step_by(16) {
            // ---- Low group ----
            let slot_lo = _mm256_and_si256(state_lo, slot_mask);
            // symbol = slot >> 4
            let sym_lo = _mm256_srli_epi32(slot_lo, 4);
            store_symbols_8way(sym_lo, output.as_mut_ptr().add(i));
            // bias = slot & 15
            let bias_lo = _mm256_and_si256(slot_lo, _mm256_set1_epi32(15));
            // new = ((state >> 12) << 4) + bias
            let xsc_lo = _mm256_srli_epi32(state_lo, SHIFT12);
            let xsh_lo = _mm256_sllv_epi32(xsc_lo, shift4);
            let new_lo = _mm256_add_epi32(xsh_lo, bias_lo);

            let rl = match renorm8_avx2(new_lo, &compressed[reader_pos..], perm_table) {
                Ok(r) => r,
                Err(_) => return Err("unexpected EOF in AVX2 uniform256 lo renorm"),
            };
            let mut lo_buf = [0u32; 8];
            lo_buf.copy_from_slice(&rl.states);
            state_lo = _mm256_loadu_si256(lo_buf.as_ptr() as *const __m256i);
            reader_pos += rl.words_consumed;

            // ---- High group ----
            let slot_hi = _mm256_and_si256(state_hi, slot_mask);
            let sym_hi = _mm256_srli_epi32(slot_hi, 4);
            store_symbols_8way(sym_hi, output.as_mut_ptr().add(i + 8));
            let bias_hi = _mm256_and_si256(slot_hi, _mm256_set1_epi32(15));
            let xsc_hi = _mm256_srli_epi32(state_hi, SHIFT12);
            let xsh_hi = _mm256_sllv_epi32(xsc_hi, shift4);
            let new_hi = _mm256_add_epi32(xsh_hi, bias_hi);

            let rh = match renorm8_avx2(new_hi, &compressed[reader_pos..], perm_table) {
                Ok(r) => r,
                Err(_) => return Err("unexpected EOF in AVX2 uniform256 hi renorm"),
            };
            let mut hi_buf = [0u32; 8];
            hi_buf.copy_from_slice(&rh.states);
            state_hi = _mm256_loadu_si256(hi_buf.as_ptr() as *const __m256i);
            reader_pos += rh.words_consumed;
        }

        // Tail
        for i in even16..n {
            let lane = i & 15;
            let (sv_ptr, _lv) = if lane < 8 {
                (&state_lo as *const __m256i, lane)
            } else {
                (&state_hi as *const __m256i, lane - 8)
            };
            let mut lns: [u32; 8] = core::mem::zeroed();
            _mm256_storeu_si256(lns.as_mut_ptr() as *mut __m256i, *sv_ptr);
            let lv = if lane < 8 { lane } else { lane - 8 };
            let x = lns[lv];
            let slot = x & 0xFFF;
            output[i] = (slot >> 4) as u8;
            let bias = slot & 15;
            let new_x = ((x >> 12) << 4) + bias;
            lns[lv] = new_x;
            if new_x < RANS_WORD_L {
                if reader_pos >= compressed.len() {
                    return Err("unexpected EOF in AVX2 uniform256 tail");
                }
                lns[lv] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            let reload = _mm256_loadu_si256(lns.as_ptr() as *const __m256i);
            if lane < 8 {
                state_lo = reload;
            } else {
                state_hi = reload;
            }
        }

        let mut lo_buf = [0u32; 8];
        _mm256_storeu_si256(lo_buf.as_mut_ptr() as *mut __m256i, state_lo);
        let mut hi_buf = [0u32; 8];
        _mm256_storeu_si256(hi_buf.as_mut_ptr() as *mut __m256i, state_hi);
        let mut final_states = [0u32; 16];
        for j in 0..8 {
            final_states[j] = lo_buf[j];
            final_states[j + 8] = hi_buf[j];
        }

        Ok(DecodeReport {
            words_consumed: reader_pos,
            final_states,
        })
    }
}

/// Allocating wrapper for AVX2 Uniform256 table-free decode.
///
/// # Safety
///
/// Requires AVX2 CPU feature at runtime.  Caller must validate Uniform256 model.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_interleaved16_uniform256_avx2(
    compressed: &[u16],
    expected_len: usize,
    perm_table: &Avx2RenormPermutations,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        let mut output = vec![0u8; expected_len];
        let report =
            decode_interleaved16_uniform256_avx2_into(compressed, &mut output, perm_table)?;
        Ok((output, report))
    }
}

// ---------------------------------------------------------------------------
// 5. AVX2 batch-four decoder
// ---------------------------------------------------------------------------
//
// Decodes up to 4 independent 16-way streams in round-robin fashion.
// Each iteration processes one 16-symbol group from each job, then moves
// to the next.  This allows the CPU to overlap gather latency of one
// stream with the arithmetic of another.
//
// Each job uses the 2x8 representation (two YMM registers per job).
// Renormalization uses the AVX2 permutation table.

/// A single decode job for batched AVX2 multi-stream decoding.
pub struct Avx2DecodeJob<'a> {
    pub compressed: &'a [u16],
    pub table: &'a PackedWordTable,
    pub output: &'a mut [u8],
    pub block_index: u64,
}

/// Decode up to 4 independent 16-way streams in a batch, interleaving
/// one 16-symbol group per job to hide gather latency.
///
/// Processes all jobs in groups of 4 (chunks of up to 4 per batch).
/// Each job's output must be pre-sized to its expected decoded length.
/// Returns one `DecodeReport` per job in input order.
///
/// # Safety
///
/// - Requires AVX2 CPU feature at runtime.
/// - Each job's `compressed` must have at least 32 u16 words.
/// - Each job's `output.len()` must match its expected symbol count.
/// - `perm_table` must be a valid Avx2RenormPermutations.
#[target_feature(enable = "avx2")]
pub unsafe fn decode_batch4_interleaved16_avx2(
    jobs: &mut [Avx2DecodeJob<'_>],
    perm_table: &Avx2RenormPermutations,
) -> Result<Vec<DecodeReport>, &'static str> {
    unsafe {
        if jobs.is_empty() {
            return Ok(Vec::new());
        }

        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
        const SCALE8: i32 = 12;
        let mut reports = Vec::with_capacity(jobs.len());

        for batch in jobs.chunks_mut(4) {
            let batch_size = batch.len();

            let mut state_lo = [core::mem::zeroed::<__m256i>(); 4];
            let mut state_hi = [core::mem::zeroed::<__m256i>(); 4];
            let mut readers = [0usize; 4];
            let mut cursors = [0usize; 4];
            let mut job_has_more = [true; 4];

            for j in 0..batch_size {
                let job = &batch[j];
                if job.output.is_empty() {
                    job_has_more[j] = false;
                    continue;
                }
                if job.compressed.len() < 32 {
                    return Err("batch4 job: compressed too short");
                }
                let mut init = [0u32; 16];
                for i in 0..16 {
                    init[i] =
                        job.compressed[i * 2] as u32 | (job.compressed[i * 2 + 1] as u32) << 16;
                }
                state_lo[j] = _mm256_loadu_si256(init[..8].as_ptr() as *const __m256i);
                state_hi[j] = _mm256_loadu_si256(init[8..].as_ptr() as *const __m256i);
                readers[j] = 32;
                cursors[j] = 0;
            }

            // Round-robin: one 16-symbol group per job per iteration
            let mut any_active = true;
            while any_active {
                any_active = false;
                for j in 0..batch_size {
                    if !job_has_more[j] {
                        continue;
                    }
                    let job = &mut batch[j];
                    let output_len = job.output.len();
                    let cursor = cursors[j];

                    if cursor + 16 > output_len {
                        job_has_more[j] = false;
                        continue;
                    }

                    let table_ptr = job.table.as_ptr() as *const i32;

                    // Low group: lanes 0-7
                    let idx_lo = _mm256_and_si256(state_lo[j], mask_v);
                    let gath_lo = _mm256_i32gather_epi32(table_ptr, idx_lo, 4);
                    let freq_lo = _mm256_and_si256(gath_lo, _mm256_set1_epi32(0x0fff));
                    let bias_lo =
                        _mm256_and_si256(_mm256_srli_epi32(gath_lo, 12), _mm256_set1_epi32(0x0fff));
                    let syms_lo = _mm256_srli_epi32(gath_lo, 24);
                    store_symbols_8way(syms_lo, job.output.as_mut_ptr().add(cursor));

                    let xsc_lo = _mm256_srli_epi32(state_lo[j], SCALE8);
                    let new_lo = _mm256_add_epi32(_mm256_mullo_epi32(xsc_lo, freq_lo), bias_lo);

                    let rl = match renorm8_avx2(new_lo, &job.compressed[readers[j]..], perm_table) {
                        Ok(r) => r,
                        Err(_) => return Err("batch4 job: renorm lo failed"),
                    };
                    let mut buf_lo = [0u32; 8];
                    buf_lo.copy_from_slice(&rl.states);
                    state_lo[j] = _mm256_loadu_si256(buf_lo.as_ptr() as *const __m256i);
                    readers[j] += rl.words_consumed;

                    // High group: lanes 8-15
                    let idx_hi = _mm256_and_si256(state_hi[j], mask_v);
                    let gath_hi = _mm256_i32gather_epi32(table_ptr, idx_hi, 4);
                    let freq_hi = _mm256_and_si256(gath_hi, _mm256_set1_epi32(0x0fff));
                    let bias_hi =
                        _mm256_and_si256(_mm256_srli_epi32(gath_hi, 12), _mm256_set1_epi32(0x0fff));
                    let syms_hi = _mm256_srli_epi32(gath_hi, 24);
                    store_symbols_8way(syms_hi, job.output.as_mut_ptr().add(cursor + 8));

                    let xsc_hi = _mm256_srli_epi32(state_hi[j], SCALE8);
                    let new_hi = _mm256_add_epi32(_mm256_mullo_epi32(xsc_hi, freq_hi), bias_hi);

                    let rh = match renorm8_avx2(new_hi, &job.compressed[readers[j]..], perm_table) {
                        Ok(r) => r,
                        Err(_) => return Err("batch4 job: renorm hi failed"),
                    };
                    let mut buf_hi = [0u32; 8];
                    buf_hi.copy_from_slice(&rh.states);
                    state_hi[j] = _mm256_loadu_si256(buf_hi.as_ptr() as *const __m256i);
                    readers[j] += rh.words_consumed;

                    cursors[j] = cursor + 16;
                    any_active = true;
                }
            }

            // Process tails and build reports
            for j in 0..batch_size {
                if !job_has_more[j] {
                    let job = &mut batch[j];
                    let output_len = job.output.len();
                    let cursor = cursors[j];

                    if cursor < output_len {
                        let mut ls_lo = [0u32; 8];
                        let mut ls_hi = [0u32; 8];
                        _mm256_storeu_si256(ls_lo.as_mut_ptr() as *mut __m256i, state_lo[j]);
                        _mm256_storeu_si256(ls_hi.as_mut_ptr() as *mut __m256i, state_hi[j]);
                        for i in cursor..output_len {
                            let lane = i & 15;
                            let (ls, lv) = if lane < 8 {
                                (&mut ls_lo, lane)
                            } else {
                                (&mut ls_hi, lane - 8)
                            };
                            let x = ls[lv];
                            let slot = x as usize & (RANS_WORD_M - 1);
                            let entry = (*job.table.get(slot)).0;
                            job.output[i] = (entry >> 24) as u8;
                            let freq_entry = entry & 0x0fff;
                            let bias_entry = (entry >> 12) & 0x0fff;
                            let new_x = freq_entry * (x >> 12) + bias_entry;
                            ls[lv] = new_x;
                            if new_x < RANS_WORD_L {
                                if readers[j] >= job.compressed.len() {
                                    return Err("batch4 job: tail renorm EOF");
                                }
                                ls[lv] = (new_x << 16) | job.compressed[readers[j]] as u32;
                                readers[j] += 1;
                            }
                        }
                        // CRITICAL: reload the updated states into the vector registers
                        // so the report construction below reads the correct final states.
                        state_lo[j] = _mm256_loadu_si256(ls_lo.as_ptr() as *const __m256i);
                        state_hi[j] = _mm256_loadu_si256(ls_hi.as_ptr() as *const __m256i);
                    }

                    let mut lo_buf = [0u32; 8];
                    _mm256_storeu_si256(lo_buf.as_mut_ptr() as *mut __m256i, state_lo[j]);
                    let mut hi_buf = [0u32; 8];
                    _mm256_storeu_si256(hi_buf.as_mut_ptr() as *mut __m256i, state_hi[j]);
                    let mut final_states = [0u32; 16];
                    for idx in 0..8 {
                        final_states[idx] = lo_buf[idx];
                        final_states[idx + 8] = hi_buf[idx];
                    }

                    reports.push(DecodeReport {
                        words_consumed: readers[j],
                        final_states,
                    });
                }
            }
        }

        Ok(reports)
    }
}
