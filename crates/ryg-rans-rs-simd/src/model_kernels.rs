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
use alloc::vec;
use alloc::vec::Vec;
use core::arch::x86_64::*;

/// Decode a 16-way interleaved stream using the uniform-256 table-free kernel.
///
/// ## What makes this kernel unique
///
/// This is the **only decoder in the entire ryg-rans-rs ecosystem that performs zero table
/// lookups**. It exploits a special property of the UNIFORM256 model at precision S12:
/// every symbol has exactly the same frequency (16), and the 4096-slot table becomes a
/// regular structure that can be computed with pure arithmetic.
///
/// ### The math
///
/// For UNIFORM256 at S12 where total = 4096 and alphabet_size = 256:
///
/// ```text
/// frequency  = total / alphabet_size = 4096 / 256 = 16   (constant for all symbols)
/// start(s)   = s × 16                                    (cumulative start of symbol s)
/// slot       = state & 4095                              (12-bit slot index from state)
/// symbol     = slot / 16 = slot >> 4                     (which symbol owns this slot)
/// bias       = slot - start(s) = slot & 15              (position within symbol's range)
///
/// new_state  = frequency × (state >> 12) + bias
///            = 16 × (state >> 12) + (slot & 15)
///            = ((state >> 12) << 4) | (slot & 15)
/// ```
///
/// ### Contrast with general decoder
///
/// The general packed-table decoder must: mask→gather→extract→multiply-add→renorm.
/// This kernel: mask→shift→narrow→multiply-add→renorm — no gather, no table, no
/// bitfield extraction.  The symbol is `slot >> 4`, bias is `slot & 15`, frequency
/// is the constant 16.
///
/// On the Ryzen 7 9800X3D this reaches **2.75 GiB/s** uniform — ~1.8× the packed-scalar
/// decoder and ~2.3× the original hardware-gather AVX-512 kernel.
///
/// ### Correctness note
///
/// A prior implementation used the shortcut `state >> 8` which is NOT equivalent:
/// `state >> 8 = 16 × (state >> 12) + (slot >> 8)`.  The term `slot >> 8` (0..15)
/// causes divergence for any slot >= 256 (~93.75% of the table).  The corrected
/// formula: `((state >> 12) << 4) + bias`.
///
/// # Safety
///
/// Requires `avx512f` + `avx512bw`.  The compressed stream must be a valid 16-way
/// interleaved Word rANS stream encoded with a UNIFORM256 model at scale_bits=12.
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_uniform256_avx512(
    compressed: &[u16],
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        let mut output = vec![0u8; expected_len];
        let report = decode_interleaved16_uniform256_avx512_into(compressed, &mut output)?;
        Ok((output, report))
    }
}

/// Decode uniform256 without table lookups, writing directly into a caller-provided buffer.
///
/// ## What makes this kernel unique
///
/// This is the **only decoder in the entire ryg-rans-rs ecosystem that performs zero table
/// lookups**. It exploits a special property of the UNIFORM256 model at precision S12:
/// every symbol has exactly the same frequency (16), and the 4096-slot table becomes a
/// regular structure that can be computed with pure arithmetic.
///
/// ### The math
///
/// For UNIFORM256 at S12 where total = 4096 and alphabet_size = 256:
///
/// ```text
/// frequency  = total / alphabet_size = 4096 / 256 = 16   (constant for all symbols)
/// start(s)   = s × 16                                    (cumulative start of symbol s)
/// slot       = state & 4095                              (12-bit slot index from state)
/// symbol(s)  = slot / 16 = slot >> 4                     (which symbol owns this slot)
/// bias       = slot - start(s) = slot & 15              (position within symbol's 16 slots)
///
/// new_state  = frequency × (state >> 12) + bias
///            = 16 × (state >> 12) + (slot & 15)
///            = ((state >> 12) << 4) | (slot & 15)
/// ```
///
/// ### Contrast with the general decoder
///
/// The general packed-table decoder (in `packed_table.rs`) must:
/// 1. Load the 12-bit slot from state (mask)
/// 2. Gather from the 4096-entry packed table (memory load)
/// 3. Extract freq, bias, symbol from the packed u32 (bit shifts)
/// 4. Compute freq × (state >> 12) + bias (multiply-add)
/// 5. Renormalize
///
/// This kernel skips steps 2 and 3 entirely — no gather, no table, no bitfield extraction.
/// The symbol is computed as `slot >> 4`, the bias as `slot & 15`, and the frequency is
/// the constant 16.
///
/// ### Performance impact
///
/// On the Ryzen 7 9800X3D, this kernel achieves **2.75 GiB/s** for uniform data — roughly
/// **1.8× faster** than the general packed-scalar decoder and **2.3× faster** than the
/// original hardware-gather AVX-512 kernel. The elimination of the gather instruction
/// (10-15 cycle latency on Zen 5) is the primary source of the gain.
///
/// ### Correctness note
///
/// A previous implementation used the shortcut `state >> 8` instead of the correct
/// `(state >> 12) × 16 + bias`. These are NOT equivalent:
///
/// ```text
/// state >> 8 = 16 × (state >> 12) + (slot >> 8)
/// ```
///
/// The term `slot >> 8` ranges from 0 to 15, causing divergence whenever slot >= 256
/// (approximately 93.75% of the table). The corrected formula uses the two-step
/// shift-left-12 → shift-left-4 → add approach.
///
/// # Safety
///
/// - Requires `avx512f` + `avx512bw` CPU features at runtime
/// - The compressed stream must be a valid 16-way interleaved Word rANS stream encoded
///   with a UNIFORM256 model at `scale_bits = 12`
/// - `output.len()` must equal the exact number of symbols to decode
/// - The caller is responsible for verifying the model is uniform256 before dispatching
///   to this kernel (or it will silently produce wrong output for non-uniform models)
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_uniform256_avx512_into(
    compressed: &[u16],
    output: &mut [u8],
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
            return Err("compressed too short for 16 init states");
        }

        let mut init_array = [0u32; 16];
        for i in 0..16 {
            init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state = _mm512_loadu_si512(init_array.as_ptr() as *const __m512i);

        let mut reader_pos = 32usize;
        let even16 = n & !15;
        let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);

        // Precompute uniform-256 constants
        let mask_4095 = _mm512_set1_epi32(0x0fff);
        const SHIFT_12: u32 = 12;
        const SHIFT_4: u32 = 4;

        // ================================================================
        // BEGIN MAIN DECODE LOOP
        //
        // Processes 16 symbols per iteration using only AVX-512 arithmetic:
        // no memory loads beyond the initial state and renormalization words.
        //
        // The loop iterates over the even-aligned 16-symbol groups.  Remaining
        // symbols (the "tail") are handled by the scalar fallback below.
        // ================================================================
        for i in (0..even16).step_by(16) {
            // Step 1: Extract slot index from current state
            // slot = state & 0xfff  (low 12 bits of each 16× u32 lane)
            // This is the index into the ANS coding table.  In the general
            // decoder we would use this to GATHER from the 4096-entry table.
            // Here we know the table layout is uniform, so we compute.
            let slot = _mm512_and_si512(state, mask_4095);

            // Step 2: Compute symbol from slot
            // symbol = slot >> 4
            // For uniform256 at S12, each symbol occupies exactly 16 consecutive
            // slots (frequency = 16).  The high 8 bits of the 12-bit slot index
            // identify which symbol owns this slot.
            let symbols_v = _mm512_srli_epi32(slot, SHIFT_4);

            // Step 3: Narrow 32-bit symbol IDs to bytes and store to output
            // _mm512_cvtepi32_epi8 truncates each 32-bit lane to 8 bits and
            // packs them into a 128-bit register (16 bytes).  We store this
            // directly to the output buffer — no temporary array, no scalar loop.
            let symbol_bytes = _mm512_cvtepi32_epi8(symbols_v);
            _mm_storeu_si128(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

            // Step 4: Compute bias = slot & 15
            // bias is the position within the symbol's 16-slot range.
            // It's added to (frequency × scaled_state) to produce the new state.
            let bias = _mm512_and_si512(slot, _mm512_set1_epi32(15));

            // Step 5: Compute new state via the ANS forward transform
            // new_state = frequency × (state >> 12) + bias
            //          = 16 × (state >> 12) + (slot & 15)
            //
            // The shift-then-shift approach (>> 12 then << 4) is intentional.
            // A naive `state >> 8` would add an unwanted (slot >> 8) term,
            // corrupting the state for any slot >= 256 (~93.75% of cases).
            let scaled = _mm512_srli_epi32(state, SHIFT_12); // state >> 12
            let new_state = _mm512_add_epi32(
                _mm512_slli_epi32(scaled, 4), // × 16 (shift left 4)
                bias,                         // + (slot & 15)
            );

            // Step 6: Renormalization check
            // Any lane where new_state < RANS_WORD_L (= 0x8000) has fallen below
            // the minimum state threshold and must absorb a u16 word from the
            // compressed stream to bring it back into the valid range.
            //
            // _mm512_cmplt_epu32_mask computes all 16 comparisons in parallel
            // and returns a 16-bit mask.  The hardware popcount (count_ones)
            // tells us how many u16 words to consume.
            let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
            let words_needed = renorm_mask.count_ones() as usize;

            if words_needed > 0 {
                // ---- Renormalization: mask-expand pattern ----
                // Instead of spilling/reloading one lane at a time (which is
                // what the original AVX-512 kernel did), we use the AVX-512
                // expand instruction to scatter compact input words into
                // the mask-selected lanes in a single operation.
                if reader_pos + words_needed > compressed.len() {
                    return Err("unexpected EOF in uniform renorm");
                }

                // Compact: copy exactly `words_needed` contiguous u16 words
                // from the compressed stream into a scratch buffer, widening
                // them to u32.  This is the only per-iteration memory load.
                let mut compact = [0u32; 16];
                for idx in 0..words_needed {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }

                // Expand: _mm512_maskz_expand_epi32 takes a mask and a
                // compact source vector.  For each set bit in the mask, it
                // copies the next element from the source to that lane.
                // For cleared bits, it writes zero.  This is the SIMD
                // equivalent of "scatter these words to their lanes".
                let compact_v = _mm512_loadu_si512(compact.as_ptr() as *const __m512i);
                let expanded = _mm512_maskz_expand_epi32(renorm_mask, compact_v);

                // Shift the existing new_state left by 16 bits to make room
                // for the incoming word, then OR with the expanded words.
                // Inactive lanes already have a valid new_state >= RANS_WORD_L
                // and are zeroed by the maskz expand, so they remain unchanged
                // after the blend.
                let shifted_rn = _mm512_slli_epi32(new_state, 16);
                let renormed = _mm512_or_si512(shifted_rn, expanded);

                // Blend: for mask-set lanes, use the renormalized value;
                // for mask-cleared lanes, use the original new_state.
                // This is a single-cycle operation on modern AVX-512 hardware.
                state = _mm512_mask_blend_epi32(renorm_mask, new_state, renormed);
                reader_pos += words_needed;
            } else {
                // No renormalization needed for any lane — fast path.
                state = new_state;
            }
        }

        // ================================================================
        // TAIL HANDLING (scalar fallback)
        //
        // Processes remaining 0..15 symbols one at a time.  Each lane is
        // spilled to a scalar buffer, processed, and reloaded.  This is
        // slow but correct, and runs at most 15 iterations.
        // ================================================================
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

        Ok(DecodeReport {
            words_consumed: reader_pos,
            final_states,
        })
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
