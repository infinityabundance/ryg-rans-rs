//! # Model-specialized Word rANS decode kernels
//!
//! These kernels exploit properties of specific frequency models to avoid
//! the 4096-entry packed table lookup (gather).  For the common UNIFORM256
//! model at S12:
//!
//! ```text
//! frequency  = 16
//! symbol     = slot >> 4
//! bias       = slot & 15
//! new_state  = 16 * (state >> 12) + (slot & 15)
//!            = (state >> 8) + (slot & 15)
//! ```
//!
//! This decoder needs NO table and NO gather — just a shift, mask, and add.
//! The savings compound because:
//! 1. No gather instruction (eliminates ~10-15 cycle gather latency)
//! 2. No table load (saves ~4 cycles for table pointer dereference)
//! 3. No port pressure from gather (5-8 µops vs 1 µop for shift+mask+add)
//!
//! ## Usage
//!
//! These kernels are NOT called automatically.  The caller must verify that
//! the model is uniform or dominant-symbol before dispatching.  Currently
//! they are exposed for benchmark comparison; automatic dispatch will be
//! added when the `WordDecodePlan` decision engine is implemented.

use crate::RANS_WORD_L;
use crate::RANS_WORD_M;
use crate::packed_table::DecodeReport;
use alloc::vec::Vec;
use core::arch::x86_64::*;

/// Decode a 16-way interleaved stream using the uniform-256 table-free kernel.
///
/// This kernel assumes a UNIFORM256 model at S12 where every symbol has
/// frequency 16.  It computes slot → symbol/bias/freq directly without
/// any table lookup.
///
/// # Safety
///
/// Requires avx512f + avx512bw CPU features.  The compressed stream must
/// be a valid 16-way interleaved Word rANS stream encoded with a uniform256
/// model at scale_bits=12.
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_uniform256_avx512(
    compressed: &[u16],
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        let n = expected_len;
        if n == 0 {
            return Ok((
                Vec::new(),
                DecodeReport {
                    words_consumed: 0,
                    final_states: [0u32; 16],
                },
            ));
        }
        if compressed.len() < 32 {
            return Err("compressed too short for 16 init states");
        }

        let mut init_array = [0u32; 16];
        for i in 0..16 {
            init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state = _mm512_loadu_si512(init_array.as_ptr() as *const __m512i);

        let mut reader_pos = 32usize;
        let even16 = n & !15;
        let mut output = Vec::with_capacity(n);
        output.resize(n, 0u8);
        let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);

        // Precompute uniform-256 constants
        let mask_4095 = _mm512_set1_epi32(0x0fff); // slot mask: state & 4095
        const SHIFT_12: u32 = 12; // state >> 12 (scale)
        const SHIFT_4: u32 = 4; // symbol = slot >> 4

        // NOTE on the Uniform256 transition:
        //   frequency = 16, start = symbol * 16
        //   slot = state & 4095, bias = slot & 15, symbol = slot >> 4
        //   new_state = 16 * (state >> 12) + (slot & 15)
        //
        // This is NOT equivalent to (state >> 8) + (slot & 15) because
        //   state >> 8 = 16 * (state >> 12) + (slot >> 8)
        // The term (slot >> 8) is 0..15, causing divergence for slots >= 256.
        //
        // So we compute: new_state = ((state >> 12) << 4) + bias
        // which is: shift right 12, left 4 (multiply by 16), add bias.

        for i in (0..even16).step_by(16) {
            // Table-free decode: slot = state & 4095
            let slot = _mm512_and_si512(state, mask_4095);

            // symbol = slot >> 4
            let symbols_v = _mm512_srli_epi32(slot, SHIFT_4);
            let symbol_bytes = _mm512_cvtepi32_epi8(symbols_v);
            _mm_storeu_si128(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

            // bias = slot & 15
            let bias = _mm512_and_si512(slot, _mm512_set1_epi32(15));

            // new_state = 16 * (state >> 12) + (slot & 15)
            // This is the correct ANS transition for Uniform256 at S12
            // where frequency = 16 and bias = slot & 15.
            //
            // NOTE: (state >> 8) is NOT equivalent because
            // state >> 8 = 16 * (state >> 12) + (slot >> 8)
            // The extra term (slot >> 8) is 0..15 and causes divergence
            // for any slot >= 256.
            let scaled = _mm512_srli_epi32(state, SHIFT_12); // state >> 12
            let new_state = _mm512_add_epi32(
                _mm512_slli_epi32(scaled, 4), // * 16
                bias,                         // + (slot & 15)
            );

            // Renormalization (standard)
            let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
            let words_needed = renorm_mask.count_ones() as usize;

            if words_needed > 0 {
                if reader_pos + words_needed > compressed.len() {
                    return Err("unexpected EOF in uniform renorm");
                }
                let mut compact = [0u32; 16];
                for idx in 0..words_needed {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let compact_v = _mm512_loadu_si512(compact.as_ptr() as *const __m512i);
                let expanded = _mm512_maskz_expand_epi32(renorm_mask, compact_v);
                let shifted_rn = _mm512_slli_epi32(new_state, 16);
                let renormed = _mm512_or_si512(shifted_rn, expanded);
                state = _mm512_mask_blend_epi32(renorm_mask, new_state, renormed);
                reader_pos += words_needed;
            } else {
                state = new_state;
            }
        }

        // Tail handling
        for i in even16..n {
            let lane = i & 15;
            let mut ls: [u32; 16] = core::mem::zeroed();
            _mm512_storeu_si512(ls.as_mut_ptr() as *mut __m512i, state);
            let x = ls[lane];
            let slot_val = x as usize & (RANS_WORD_M - 1);
            output[i] = (slot_val >> 4) as u8;
            let new_x = ((x >> 12) * 16) + (slot_val as u32 & 15);
            ls[lane] = new_x;
            if new_x < RANS_WORD_L {
                if reader_pos >= compressed.len() {
                    return Err("unexpected EOF in uniform tail");
                }
                ls[lane] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            state = _mm512_loadu_si512(ls.as_ptr() as *const __m512i);
        }

        let mut final_states = [0u32; 16];
        _mm512_storeu_si512(final_states.as_mut_ptr() as *mut __m512i, state);

        Ok((
            output,
            DecodeReport {
                words_consumed: reader_pos,
                final_states,
            },
        ))
    }
}

/// Decode a 16-way interleaved stream using a dominant-symbol fast path.
///
/// For models where one symbol dominates (e.g., SKEWED.255_1 where one symbol
/// has 255/256 of the total probability), most lanes will map to the same
/// symbol.  This kernel handles the common case with arithmetic and falls
/// back to table lookup only for the cold lanes.
///
/// NOTE: This function is intentionally NOT implemented.  It exists as a
/// placeholder for future development of a masked dominant-symbol fast path.
/// Calling it will return an error.  Do not use in production.
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_dominant_sketch(
    _compressed: &[u16],
    _table: &crate::packed_table::PackedWordTable,
    _expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    Err("dominant-symbol kernel not implemented")
}
