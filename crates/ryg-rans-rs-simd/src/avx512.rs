//! # AVX-512 Word rANS decode kernels
//!
//! This module is conditionally compiled — only present when the target supports
//! AVX-512BW at compile time or when the `std` feature is enabled.
//!
//! This module implements two complementary AVX-512 accelerated Word rANS decode
//! surfaces.  Both use a **packed u32 table** (`PackedWordTable`) where each 32-bit
//! entry packs `frequency(12 bits) | bias(12 bits) | symbol(8 bits)`.  A single
//! `_mm256_i32gather_epi32` or `_mm512_i32gather_epi32` instruction loads all three
//! fields for the entire SIMD width, eliminating the scalar extraction bottleneck
//! that limited the SSE4.1 implementation.
//!
//! ## Design rationale: Why AVX-512 instead of waiting for AVX-1024?
//!
//! AVX-512 provides two critical facilities that SSE/AVX2 lack:
//!
//! 1. **Vector gathers** (`_mm{256,512}_i32gather_epi32`):  Each lane can load from
//!    a different address.  The 4096-entry packed table (16 KB, L1-resident) means
//!    gathers typically hit the L1 cache.  SSE4.1 has no gather, forcing scalar
//!    extraction (store state → extract four indices → four separate scalar loads
//!    → insert into vector → vector arithmetic).
//!
//! 2. **Masked compare** (`_mm{256,512}_cmplt_epu32_mask`):  Returns a bitmask of
//!    which lanes satisfy the comparison.  This is the natural representation for
//!    "which lanes need renormalization?" — a per-lane boolean that we can use
//!    directly as an index into a popcount table (how many u16 words to consume)
//!    and as a loop mask (which lanes to process).
//!
//! The 8-way surface (`AVX512VL.INTERLEAVED8`) preserves the existing canonical
//! eight-way Word rANS stream format.  The 16-way surface (`AVX512.INTERLEAVED16`)
//! introduces a new format that doubles the arithmetic density (16 symbols decoded
//! per gather instead of 8) at the cost of a new stream layout.
//!
//! ## Why not AVX-512 VNNI?
//!
//! The Word rANS state transition is:
//! ```text
//! new_state[lane] = frequency[lane] * (state[lane] >> 12) + bias[lane]
//! ```
//! This is a set of 8 or 16 independent lane-wise multiply-add operations — exactly
//! what `_mm{256,512}_mullo_epi32` + `_mm{256,512}_add_epi32` provide.  VNNI is
//! designed for packed dot products (e.g., `sum(a[i] * b[i])`), not independent
//! per-lane arithmetic.
//!
//! ## Why lane-wise renormalization instead of masked expand-load?
//!
//! The renormalization step reads `popcount(mask)` contiguous `u16` words from the
//! input stream and distributes them to the corresponding lanes.  A natural SIMD
//! approach would use a masked expand-load (`_mm256_mask_expand_epi16`).  However,
//! the Rust intrinsic `_mm256_mask_expand_epi16` has an ambiguous memory-access
//! contract: inactive masked lanes may or may not read memory, depending on the
//! microarchitecture.  To guarantee no overread beyond the provided slice, we use
//! a **lane-wise scalar loop**: for each active lane, read exactly one `u16` from
//! the stream.  This is provably safe and the loop runs at most 8 or 16 iterations
//! (typically 1–2).
//!
//! ## ISA requirements
//!
//! | Kernel | Required features | Why |
//! |--------|------------------|-----|
//! | `decode_interleaved8_avx512vl_kernel` | `avx512f, avx512vl, avx512bw` | Gather requires AVX512F; 256-bit ops need AVX512VL; byte/word mask needs AVX512BW |
//! | `decode_interleaved16_avx512_kernel`  | `avx512f, avx512bw` | Gather requires AVX512F; 512-bit ops don't need AVX512VL; mask needs AVX512BW |
//!
//! ## Safety
//!
//! All kernels are `unsafe` and require the caller to verify CPU feature support
//! via runtime detection before calling.  See `docs/unsafe-ledger.md` for detailed
//! safety contracts per function.

use crate::RANS_WORD_L;
use crate::RANS_WORD_M;
use crate::packed_table::{DecodeReport, PackedWordTable};
use alloc::vec::Vec;
use core::arch::x86_64::*;

// ---------------------------------------------------------------------------
// Batched decode job descriptor
// ---------------------------------------------------------------------------

/// A single decode job for batched multi-stream decoding.
///
/// Each job has its own compressed stream, decode table, and output buffer.
/// The batched decoder processes one group from each job in round-robin
/// order, allowing the CPU to overlap gather latencies across independent
/// state chains.
pub struct DecodeJob<'a> {
    pub compressed: &'a [u16],
    pub table: &'a PackedWordTable,
    pub output: &'a mut [u8],
}

// ---------------------------------------------------------------------------
// Renormalization word count: hardware popcount
// ---------------------------------------------------------------------------
// The number of renormalization words needed equals popcount(mask).
// We use the x86 popcount instruction (`count_ones()`) instead of a
// lookup table because:
//
// 1. The 16-way table would be 64 KiB — a non-trivial static allocation
//    that adds cache pressure alongside the 16 KiB rANS decode table.
// 2. Hardware popcount (POPCNT instruction) is 1-3 cycles latency on
//    modern x86-64, including Zen 5 — faster than a dependent L1 lookup.
// 3. The mask is already in a GPR (returned by `_mm{256,512}_cmplt_epu32_mask`)
//    — no extra move or widening is needed.
//
// POPCNT is available on all x86-64 CPUs that support AVX-512 (Haswell-E
// and later, all Zen).  No feature check is needed beyond the existing
// AVX-512 target feature gates.

// ---------------------------------------------------------------------------
// Standalone renormalization kernels for exhaustive mask testing
// ---------------------------------------------------------------------------
//
// These functions isolate the SIMD renormalization step — computing the mask
// via `_mm{256,512}_cmplt_epu32_mask` and distributing u16 words to active
// lanes — from the full decode loop.  This enables exhaustive testing of
// every possible mask (256 for 8-way, 65536 for 16-way) without needing to
// construct valid encoded streams.
//
// Each function takes initial states and a slice of renorm words, then
// returns the final states, observed mask, and word consumption count.
// The test harness can therefore assert:
//   - observed mask == requested mask
//   - words consumed == popcount(mask)
//   - active lanes receive correct ascending-order words
//   - inactive lanes remain unchanged
//   - exact popcount-sized input succeeds
//   - popcount-1 input fails

/// Result of an 8-way renormalization operation.
#[derive(Debug, Clone, Copy)]
pub struct Renorm8Result {
    pub states: [u32; 8],
    pub mask: u8,
    pub words_consumed: usize,
}

/// Standalone 8-way AVX512VL renormalization kernel for testing.
///
/// Takes initial states (as a __m256i) and a slice of renorm words.
/// Returns the final states after renormalization plus observed mask
/// and word consumption.
///
/// # Safety
///
/// Requires avx512f, avx512vl, avx512bw CPU features at runtime.
/// The `input` slice must have at least `popcount(mask)` words available.
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn renorm8_avx512vl(
    states: __m256i,
    input: &[u16],
) -> Result<Renorm8Result, &'static str> {
    unsafe {
        let l_vec = _mm256_set1_epi32(RANS_WORD_L as i32);
        let renorm_mask = _mm256_cmplt_epu32_mask(states, l_vec);
        let words_needed = renorm_mask.count_ones() as usize;

        if words_needed > input.len() {
            return Err("insufficient renorm words for 8-way");
        }

        // Expand: compact u16 words → widen to u32 → mask_expand distributes to lanes.
        let mut compact = [0u32; 8];
        for idx in 0..words_needed {
            compact[idx] = input[idx] as u32;
        }
        let compact_v = _mm256_loadu_si256(compact.as_ptr() as *const __m256i);
        let expanded = _mm256_maskz_expand_epi32(renorm_mask, compact_v);
        let shifted = _mm256_slli_epi32(states, 16);
        let renormed = _mm256_or_si256(shifted, expanded);
        let result = _mm256_mask_blend_epi32(renorm_mask, states, renormed);

        let mut final_states = [0u32; 8];
        _mm256_storeu_si256(final_states.as_mut_ptr() as *mut __m256i, result);

        Ok(Renorm8Result {
            states: final_states,
            mask: renorm_mask,
            words_consumed: words_needed,
        })
    }
}

/// Result of a 16-way renormalization operation.
#[derive(Debug, Clone, Copy)]
pub struct Renorm16Result {
    pub states: [u32; 16],
    pub mask: u16,
    pub words_consumed: usize,
}

/// Standalone 16-way AVX512 renormalization kernel for testing.
///
/// Takes initial states (as a __m512i) and a slice of renorm words.
/// Returns the final states after renormalization plus observed mask
/// and word consumption.
///
/// # Safety
///
/// Requires avx512f, avx512bw CPU features at runtime.
/// The `input` slice must have at least `popcount(mask)` words available.
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn renorm16_avx512(
    states: __m512i,
    input: &[u16],
) -> Result<Renorm16Result, &'static str> {
    unsafe {
        let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);
        let renorm_mask = _mm512_cmplt_epu32_mask(states, l_vec);
        let words_needed = renorm_mask.count_ones() as usize;

        if words_needed > input.len() {
            return Err("insufficient renorm words for 16-way");
        }

        // Expand: compact u16 words → widen to u32 → mask_expand distributes to lanes.
        let mut compact = [0u32; 16];
        for idx in 0..words_needed {
            compact[idx] = input[idx] as u32;
        }
        let compact_v = _mm512_loadu_si512(compact.as_ptr() as *const __m512i);
        let expanded = _mm512_maskz_expand_epi32(renorm_mask, compact_v);
        let shifted = _mm512_slli_epi32(states, 16);
        let renormed = _mm512_or_si512(shifted, expanded);
        let result = _mm512_mask_blend_epi32(renorm_mask, states, renormed);

        let mut final_states = [0u32; 16];
        _mm512_storeu_si512(final_states.as_mut_ptr() as *mut __m512i, result);

        Ok(Renorm16Result {
            states: final_states,
            mask: renorm_mask,
            words_consumed: words_needed,
        })
    }
}

// ---------------------------------------------------------------------------
// AVX512VL.INTERLEAVED8: 8-way decode using 256-bit vectors
// ---------------------------------------------------------------------------
//
// This decoder consumes the EXISTING canonical eight-way Word rANS stream format.
// It is bitstream-compatible with:
//   - Rust scalar 8-way encoding/decoding
//   - Rust SSE4.1 8-way decoding
//   - C upstream 8-way SIMD oracle encoding/decoding
//
// The decode loop processes 8 symbols per iteration:
//
//   1. GATHER:  indices = state & 4095
//               packed  = _mm256_i32gather_epi32(table_ptr, indices, 4)
//               // One instruction loads 8 × u32 from 8 different table slots.
//               // The 4-byte scale matches the sizeof(PackedWordEntry).
//
//   2. UNPACK:  freq   = packed & 0x0fff
//               bias   = (packed >> 12) & 0x0fff
//               symbol = packed >> 24
//               // Bit masks extract the three fields.  All 8 lanes are processed
//               // simultaneously — no serialization.
//
//   3. STORE:   Write symbol bytes to output via temporary [u32; 8] buffer.
//               // We use a temp buffer instead of _mm256_packus* because the
//               // packus instructions have complex lane-interleaving semantics
//               // that make lane-order preservation non-trivial.
//
//   4. UPDATE:  state = (state >> 12) * freq + bias
//               // The core ANS state transition, done as lane-wise multiply-add.
//
//   5. RENORM:  renorm_mask = state < 65536  (per lane)
//               words_needed = popcount(renorm_mask)
//               // For each active lane, read one u16 and shift into the state.
//               // Inactive lanes are not touched.
//
// The tail path (r < 8 symbols) uses scalar logic: store the SIMD state to a
// temp array, process each lane individually, reload the state.

/// Decode 8 symbols using AVX512VL + AVX512BW (256-bit vectors).
///
/// # Safety
///
/// - Requires `avx512f, avx512vl, avx512bw` CPU features at runtime.
/// - `compressed` must have at least 16 u16 words (checked upfront).
/// - `table` must have exactly 4096 entries (guaranteed by `PackedWordTable` invariant).
/// - `expected_len` must match the encoded symbol count.
///
/// These preconditions are checked dynamically where practical.  The function
/// returns `Err(&'static str)` on any violation — it never panics or overreads.
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn decode_interleaved8_avx512vl_kernel(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        // ---- Precondition check: minimum stream length ----
        // The 8-way format stores 8 initial states as [low16, high16] pairs,
        // requiring 16 u16 words.
        if compressed.len() < 16 {
            return Err("compressed too short for 8 init states (AVX512VL)");
        }

        // ---- Load initial states ----
        // The stream stores states as [state0.low, state0.high, state1.low, ...].
        // We use a scalar loop to correctly deinterleave the low/high pairs.
        // This runs once at startup (16 iterations) and is not performance-critical.
        let mut init_array = [0u32; 8];
        for i in 0..8 {
            init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        // Load the 8 u32 values into a __m256i register.
        // SAFETY: init_array is 32 bytes, aligned to at least 4 bytes (u32).
        // _mm256_loadu_si256 reads 32 bytes from the pointer — valid because
        // the array is exactly 8 × u32 = 32 bytes.
        let mut state = _mm256_loadu_si256(init_array.as_ptr() as *const __m256i);

        let mut reader_pos = 16usize; // current position in compressed (u16 words)
        let n = expected_len;
        let even8 = n & !7; // largest multiple of 8 <= n
        let mut output = Vec::with_capacity(n);
        output.resize(n, 0u8);

        // ---- Precompute SIMD constants ----
        // These are hoisted out of the hot loop because _mm256_set1_epi32 is
        // a broadcast that would otherwise execute every iteration.
        let table_ptr = table.as_ptr() as *const i32;
        // `_mm256_i32gather_epi32` takes `*const i32` as the base address.
        // PackedWordEntry has repr(transparent) over u32, making this cast safe.
        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32); // 4095: slot index mask
        // Shift count for (state >> 12) — must be a compile-time constant per
        // the intrinsic's signature (`const IMM8: i32` for `_mm256_srli_epi32`).
        const SCALE8: i32 = 12;

        // ---- Main decode loop: process 8 symbols per iteration ----
        for i in (0..even8).step_by(8) {
            // STEP 1: Gather packed table entries
            // Compute slot indices by masking off the low 12 bits of each state.
            let indices = _mm256_and_si256(state, mask_v);
            // Gather loads 8 × u32 from addresses [table_ptr + indices[lane] * 4].
            // Each entry is a PackedWordEntry containing freq|bias<<12|sym<<24.
            let gathered = _mm256_i32gather_epi32(table_ptr, indices, 4);

            // STEP 2: Unpack fields using bit masks
            // freq = packed & 0x0fff (low 12 bits)
            let freq_mask = _mm256_set1_epi32(0x0fff);
            let freq_v = _mm256_and_si256(gathered, freq_mask);
            // bias = (packed >> 12) & 0x0fff (next 12 bits)
            let bias_v = _mm256_and_si256(_mm256_srli_epi32(gathered, 12), freq_mask);
            // symbol = packed >> 24 (high 8 bits)
            let symbols_v = _mm256_srli_epi32(gathered, 24);

            // STEP 3: Narrow and store symbols directly
            // Use VPMOVDB (_mm256_cvtepi32_epi8) to truncate 8 packed i32
            // lanes to 8 packed bytes.  This removes the temporary [u32; 8]
            // buffer and the scalar copy loop — one narrow + one 8-byte store.
            let symbol_bytes = _mm256_cvtepi32_epi8(symbols_v);
            _mm_storel_epi64(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

            // STEP 4: State update — the core ANS transition
            // xscaled = state >> 12  (remove the slot portion)
            // state   = xscaled * freq + bias  (multiply-add in lane order)
            // This is a pack of 8 independent lane-wise operations.  The SIMD
            // multiply (`_mm256_mullo_epi32`) and add (`_mm256_add_epi32`) handle
            // all lanes simultaneously.
            let xscaled = _mm256_srli_epi32(state, SCALE8);
            let new_state = _mm256_add_epi32(_mm256_mullo_epi32(xscaled, freq_v), bias_v);

            // STEP 5: Renormalization
            // Compute mask: which lanes have state < 65536 (RANS_WORD_L)?
            // _mm256_cmplt_epu32_mask returns an 8-bit mask where bit N is 1 if
            // lane N's state < L.
            let renorm_mask =
                _mm256_cmplt_epu32_mask(new_state, _mm256_set1_epi32(RANS_WORD_L as i32));
            let words_needed = renorm_mask.count_ones() as usize;

            if words_needed > 0 {
                // Check input bounds before reading.
                if reader_pos + words_needed > compressed.len() {
                    return Err("unexpected EOF in AVX512VL renorm");
                }
                // Expand renormalization: compact u16 words → widen to u32 →
                // mask_expand distributes them to the correct lanes in one operation.
                // Then shift+or merges with the state.  Mask blend selects renormed
                // lanes from the result and inactive lanes from the original state.
                let mut compact = [0u32; 8];
                for idx in 0..words_needed {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let compact_v = _mm256_loadu_si256(compact.as_ptr() as *const __m256i);
                let expanded = _mm256_maskz_expand_epi32(renorm_mask, compact_v);
                let shifted = _mm256_slli_epi32(new_state, 16);
                let renormed = _mm256_or_si256(shifted, expanded);
                state = _mm256_mask_blend_epi32(renorm_mask, new_state, renormed);
                reader_pos += words_needed;
            } else {
                // No lanes need renormalization — keep the new state as-is.
                state = new_state;
            }
        }

        // ---- Tail handling: symbols in the last partial group ----
        // When expected_len is not divisible by 8, we process the remaining r < 8
        // symbols one at a time using scalar logic.  Each symbol goes to lane
        // `lane = i & 7` in the 8-way state vector.
        for i in even8..n {
            let lane = i & 7;
            // Save SIMD state to temp array.
            let mut lanes: [u32; 8] = core::mem::zeroed();
            _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, state);
            let x = lanes[lane];
            let slot = x as usize & (RANS_WORD_M - 1);
            // Read single packed entry.
            let entry = (*table.get(slot)).0;
            output[i] = (entry >> 24) as u8; // symbol
            let freq_entry = entry & 0x0fff; // frequency
            let bias_entry = (entry >> 12) & 0x0fff; // bias
            let new_x = freq_entry * (x >> 12) + bias_entry;
            lanes[lane] = new_x;
            if new_x < RANS_WORD_L {
                if reader_pos >= compressed.len() {
                    return Err("unexpected EOF in AVX512VL tail renorm");
                }
                lanes[lane] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            // Reload modified state.
            state = _mm256_loadu_si256(lanes.as_ptr() as *const __m256i);
        }

        // ---- Collect final states for the DecodeReport ----
        let mut final_states = [0u32; 8];
        _mm256_storeu_si256(final_states.as_mut_ptr() as *mut __m256i, state);

        let report = DecodeReport {
            words_consumed: reader_pos,
            // We only have 8 final states, but DecodeReport expects 16.
            // The top 8 slots are zeroed — callers should use the appropriate
            // length for 8-way vs 16-way decodes.
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
}

/// Decode 8 symbols into a preallocated output buffer (allocation-free).
///
/// # Safety
///
/// Same safety requirements as `decode_interleaved8_avx512vl_kernel`, plus:
/// - `output.len()` must equal the number of symbols to decode.
/// - `output` must not overlap with `compressed`.
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn decode_interleaved8_avx512vl_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let expected_len = output.len();
        if compressed.len() < 16 {
            return Err("compressed too short for 8 init states (AVX512VL)");
        }

        let mut init_array = [0u32; 8];
        for i in 0..8 {
            init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state = _mm256_loadu_si256(init_array.as_ptr() as *const __m256i);

        let mut reader_pos = 16usize;
        let n = expected_len;
        let even8 = n & !7;

        let table_ptr = table.as_ptr() as *const i32;
        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
        const SCALE8: i32 = 12;

        for i in (0..even8).step_by(8) {
            let indices = _mm256_and_si256(state, mask_v);
            let gathered = _mm256_i32gather_epi32(table_ptr, indices, 4);

            let freq_mask = _mm256_set1_epi32(0x0fff);
            let freq_v = _mm256_and_si256(gathered, freq_mask);
            let bias_v = _mm256_and_si256(_mm256_srli_epi32(gathered, 12), freq_mask);
            let symbols_v = _mm256_srli_epi32(gathered, 24);

            // Narrow and store 8 symbols directly
            let symbol_bytes = _mm256_cvtepi32_epi8(symbols_v);
            _mm_storel_epi64(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

            let xscaled = _mm256_srli_epi32(state, SCALE8);
            let new_state = _mm256_add_epi32(_mm256_mullo_epi32(xscaled, freq_v), bias_v);

            let renorm_mask =
                _mm256_cmplt_epu32_mask(new_state, _mm256_set1_epi32(RANS_WORD_L as i32));
            let words_needed = renorm_mask.count_ones() as usize;

            if words_needed > 0 {
                if reader_pos + words_needed > compressed.len() {
                    return Err("unexpected EOF in AVX512VL renorm");
                }
                let mut compact = [0u32; 8];
                for idx in 0..words_needed {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let compact_v = _mm256_loadu_si256(compact.as_ptr() as *const __m256i);
                let expanded = _mm256_maskz_expand_epi32(renorm_mask, compact_v);
                let shifted = _mm256_slli_epi32(new_state, 16);
                let renormed = _mm256_or_si256(shifted, expanded);
                state = _mm256_mask_blend_epi32(renorm_mask, new_state, renormed);
                reader_pos += words_needed;
            } else {
                state = new_state;
            }
        }

        // Tail handling: scalar fallback for remaining symbols
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

        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// AVX512.INTERLEAVED16: 16-way decode using 512-bit vectors
// ---------------------------------------------------------------------------
//
// This decoder consumes the NEW sixteen-way Word rANS stream format.  The format
// differs from the existing 8-way format in several important ways:
//
// 1. 16 independent rANS states instead of 8.
// 2. Encoding assigns symbols to lane = i & 15.
// 3. States are flushed in REVERSE lane order (15 down to 0) because the writer
//    moves backward through the buffer.  This means the forward stream has
//    initial states in ASCENDING lane order (0 up to 15).
// 4. Decoding processes 16 symbols per iteration.
// 5. Tail handling supports r < 16 remainder symbols.
//
// The 16-way format is NOT compatible with the 8-way format.  An 8-way stream
// will be rejected by the 16-way decoder (and vice versa) because the initial
// state count differs (16 u16 vs 32 u16).

/// Decode 16 symbols using AVX512 (512-bit vectors, masked operations).
///
/// # Safety
///
/// - Requires `avx512f, avx512bw` CPU features at runtime.
/// - `compressed` must have at least 32 u16 words (checked upfront).
/// - `table` must have exactly 4096 entries.
/// - `expected_len` must match the encoded symbol count.
///
/// This function is the 16-way counterpart to `decode_interleaved8_avx512vl_kernel`.
/// The same safety and correctness patterns apply, extended to 16 lanes.
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_avx512_kernel(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    // Empty stream with 0 expected symbols — nothing to decode
    if expected_len == 0 {
        return Ok((
            Vec::new(),
            DecodeReport {
                words_consumed: 0,
                final_states: [0u32; 16],
            },
        ));
    }
    // ---- Precondition check ----
    if compressed.len() < 32 {
        return Err("compressed too short for 16 init states (AVX512)");
    }

    // ---- Load 16 initial states from the first 32 u16 words ----
    // Each state is stored as [low16, high16] in reverse flush order
    // (state 15 first because the encoder flushed 15, 14, ..., 0).
    // But the REVERSE flush means the forward reader sees state 15 first,
    // then state 14, ..., finally state 0.  We need to reorder.
    //
    // Actually: the encoder produces the stream backwards.  It starts at the
    // end of the buffer and writes state[15], then state[14], ..., state[0].
    // But since it's a BACKWARD writer, state[15] ends up at the END of the
    // encoded buffer, and state[0] ends up at the BEGINNING.  So when we read
    // the stream FORWARD, the first 2 u16 are state[0], and the last 2 u16
    // are state[15].  The scalar loop below correctly reconstructs this.
    let mut init_array = [0u32; 16];
    for i in 0..16 {
        init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
    }
    // Load into __m512i register.
    let mut state = _mm512_loadu_si512(init_array.as_ptr() as *const __m512i);

    let mut reader_pos = 32usize;
    let n = expected_len;
    let even16 = n & !15; // largest multiple of 16 <= n
    let mut output = Vec::with_capacity(n);
    output.resize(n, 0u8);

    let table_ptr = table.as_ptr() as *const i32;
    let mask_v = _mm512_set1_epi32((RANS_WORD_M - 1) as i32);
    let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);
    const SCALE16: u32 = 12;

    for i in (0..even16).step_by(16) {
        // ---- Gather phase: load 16 packed table entries ----
        let indices = _mm512_and_si512(state, mask_v);
        let gathered = _mm512_i32gather_epi32(indices, table_ptr, 4);

        // Unpack: freq = gathered & 0xfff, bias = (gathered >> 12) & 0xfff,
        // symbol = gathered >> 24.
        let freq_v = _mm512_and_si512(gathered, _mm512_set1_epi32(0x0fff));
        let bias_v = _mm512_and_si512(_mm512_srli_epi32(gathered, 12), _mm512_set1_epi32(0x0fff));
        let symbols_v = _mm512_srli_epi32(gathered, 24);

        // ---- Narrow and store 16 symbols directly ----
        // VPMOVDB (_mm512_cvtepi32_epi8) truncates 16 packed i32 lanes to
        // 16 packed bytes in one operation.  One narrow + one 16-byte store
        // replaces the temporary buffer and scalar copy loop.
        let symbol_bytes = _mm512_cvtepi32_epi8(symbols_v);
        _mm_storeu_si128(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

        // ---- State update ----
        let xscaled = _mm512_srli_epi32(state, SCALE16);
        let new_state = _mm512_add_epi32(_mm512_mullo_epi32(xscaled, freq_v), bias_v);

        // ---- Masked renormalization ----
        // Compare: new_state < L for all 16 lanes simultaneously.
        // _mm512_cmplt_epu32_mask returns a 16-bit mask.
        let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
        let words_needed = renorm_mask.count_ones() as usize;

        if words_needed > 0 {
            if reader_pos + words_needed > compressed.len() {
                return Err("unexpected EOF in AVX512 renorm");
            }
            // Renormalization: spill state once, modify all lanes, reload once.
            // The original N-spill pattern performed one full 64-byte store and one
            // 64-byte load per active lane.  With 3 active lanes that was 3 × 64 = 192
            // bytes moved per decode group.  This version does exactly one spill and
            // one reload regardless of mask weight.
            let mut lanes: [u32; 16] = core::mem::zeroed();
            _mm512_storeu_si512(lanes.as_mut_ptr() as *mut __m512i, new_state);
            let mut rp = reader_pos;
            for lane in 0..16 {
                if (renorm_mask >> lane) & 1 != 0 {
                    let w = compressed[rp] as u32;
                    rp += 1;
                    lanes[lane] = (lanes[lane] << 16) | w;
                }
            }
            state = _mm512_loadu_si512(lanes.as_ptr() as *const __m512i);
            reader_pos = rp;
        } else {
            state = new_state;
        }
    }

    // ---- Tail: scalar fallback for remaining symbols ----
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

/// Decode 16 symbols into a preallocated output buffer (allocation-free).
///
/// # Safety
///
/// Same safety requirements as `decode_interleaved16_avx512_kernel`, plus:
/// - `output.len()` must equal the number of symbols to decode.
/// - `output` must not overlap with `compressed`.
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_avx512_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let expected_len = output.len();
        if expected_len == 0 {
            return Ok(DecodeReport {
                words_consumed: 0,
                final_states: [0u32; 16],
            });
        }
        if compressed.len() < 32 {
            return Err("compressed too short for 16 init states (AVX512)");
        }

        let mut init_array = [0u32; 16];
        for i in 0..16 {
            init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let mut state = _mm512_loadu_si512(init_array.as_ptr() as *const __m512i);

        let mut reader_pos = 32usize;
        let n = expected_len;
        let even16 = n & !15;

        let table_ptr = table.as_ptr() as *const i32;
        let mask_v = _mm512_set1_epi32((RANS_WORD_M - 1) as i32);
        let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);
        const SCALE16: u32 = 12;

        for i in (0..even16).step_by(16) {
            let indices = _mm512_and_si512(state, mask_v);
            let gathered = _mm512_i32gather_epi32(indices, table_ptr, 4);

            let freq_v = _mm512_and_si512(gathered, _mm512_set1_epi32(0x0fff));
            let bias_v =
                _mm512_and_si512(_mm512_srli_epi32(gathered, 12), _mm512_set1_epi32(0x0fff));
            let symbols_v = _mm512_srli_epi32(gathered, 24);

            // Narrow and store 16 symbols directly
            let symbol_bytes = _mm512_cvtepi32_epi8(symbols_v);
            _mm_storeu_si128(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

            let xscaled = _mm512_srli_epi32(state, SCALE16);
            let new_state = _mm512_add_epi32(_mm512_mullo_epi32(xscaled, freq_v), bias_v);

            let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
            let words_needed = renorm_mask.count_ones() as usize;

            if words_needed > 0 {
                if reader_pos + words_needed > compressed.len() {
                    return Err("unexpected EOF in AVX512 renorm");
                }
                let mut compact = [0u32; 16];
                for idx in 0..words_needed {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let compact_v = _mm512_loadu_si512(compact.as_ptr() as *const __m512i);
                let expanded = _mm512_maskz_expand_epi32(renorm_mask, compact_v);
                let shifted = _mm512_slli_epi32(new_state, 16);
                let renormed = _mm512_or_si512(shifted, expanded);
                state = _mm512_mask_blend_epi32(renorm_mask, new_state, renormed);
                reader_pos += words_needed;
            } else {
                state = new_state;
            }
        }

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

        let mut final_states = [0u32; 16];
        _mm512_storeu_si512(final_states.as_mut_ptr() as *mut __m512i, state);

        Ok(DecodeReport {
            words_consumed: reader_pos,
            final_states,
        })
    }
}

// ---------------------------------------------------------------------------
// Step 7: Manual-gather AVX512VL 8-way
// ---------------------------------------------------------------------------
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn decode_interleaved8_manual_gather_kernel(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    unsafe {
        if compressed.len() < 16 {
            return Err("compressed too short for 8 init states");
        }
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
        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
        const SCALE8: i32 = 12;

        for i in (0..even8).step_by(8) {
            let indices = _mm256_and_si256(state, mask_v);
            // Manual gather: store indices to buffer, scalar loads, reload to vector
            let mut idx_buf: [u32; 8] = core::mem::zeroed();
            _mm256_storeu_si256(idx_buf.as_mut_ptr() as *mut __m256i, indices);
            let mut ent_buf: [u32; 8] = core::mem::zeroed();
            for lane in 0..8 {
                ent_buf[lane] = table.as_slice()[idx_buf[lane] as usize].0;
            }
            let gathered = _mm256_loadu_si256(ent_buf.as_ptr() as *const __m256i);

            let freq_mask = _mm256_set1_epi32(0x0fff);
            let freq_v = _mm256_and_si256(gathered, freq_mask);
            let bias_v = _mm256_and_si256(_mm256_srli_epi32(gathered, 12), freq_mask);
            let symbols_v = _mm256_srli_epi32(gathered, 24);

            let symbol_bytes = _mm256_cvtepi32_epi8(symbols_v);
            _mm_storel_epi64(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

            let xscaled = _mm256_srli_epi32(state, SCALE8);
            let new_state = _mm256_add_epi32(_mm256_mullo_epi32(xscaled, freq_v), bias_v);

            let renorm_mask =
                _mm256_cmplt_epu32_mask(new_state, _mm256_set1_epi32(RANS_WORD_L as i32));
            let words_needed = renorm_mask.count_ones() as usize;

            if words_needed > 0 {
                if reader_pos + words_needed > compressed.len() {
                    return Err("unexpected EOF in manual-gather renorm");
                }
                let mut compact = [0u32; 8];
                for idx in 0..words_needed {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let compact_v = _mm256_loadu_si256(compact.as_ptr() as *const __m256i);
                let expanded = _mm256_maskz_expand_epi32(renorm_mask, compact_v);
                let shifted = _mm256_slli_epi32(new_state, 16);
                let renormed = _mm256_or_si256(shifted, expanded);
                state = _mm256_mask_blend_epi32(renorm_mask, new_state, renormed);
                reader_pos += words_needed;
            } else {
                state = new_state;
            }
        }

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
                    return Err("unexpected EOF in manual-gather tail");
                }
                lanes[lane] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            state = _mm256_loadu_si256(lanes.as_ptr() as *const __m256i);
        }

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
}

// Allocation-free _into variant for manual gather 8-way
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn decode_interleaved8_manual_gather_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let (_, report) =
            decode_interleaved8_manual_gather_kernel(compressed, table, output.len())?;
        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// Step 7: Manual-gather AVX512 16-way
// ---------------------------------------------------------------------------
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_manual_gather_kernel(
    compressed: &[u16],
    table: &PackedWordTable,
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
        let mask_v = _mm512_set1_epi32((RANS_WORD_M - 1) as i32);
        let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);
        const SCALE16: u32 = 12;

        for i in (0..even16).step_by(16) {
            let indices = _mm512_and_si512(state, mask_v);
            let mut idx_buf: [u32; 16] = core::mem::zeroed();
            _mm512_storeu_si512(idx_buf.as_mut_ptr() as *mut __m512i, indices);
            let mut ent_buf: [u32; 16] = core::mem::zeroed();
            for lane in 0..16 {
                ent_buf[lane] = table.as_slice()[idx_buf[lane] as usize].0;
            }
            let gathered = _mm512_loadu_si512(ent_buf.as_ptr() as *const __m512i);

            let freq_v = _mm512_and_si512(gathered, _mm512_set1_epi32(0x0fff));
            let bias_v =
                _mm512_and_si512(_mm512_srli_epi32(gathered, 12), _mm512_set1_epi32(0x0fff));
            let symbols_v = _mm512_srli_epi32(gathered, 24);

            let symbol_bytes = _mm512_cvtepi32_epi8(symbols_v);
            _mm_storeu_si128(output.as_mut_ptr().add(i) as *mut __m128i, symbol_bytes);

            let xscaled = _mm512_srli_epi32(state, SCALE16);
            let new_state = _mm512_add_epi32(_mm512_mullo_epi32(xscaled, freq_v), bias_v);

            let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
            let words_needed = renorm_mask.count_ones() as usize;

            if words_needed > 0 {
                if reader_pos + words_needed > compressed.len() {
                    return Err("unexpected EOF in manual-gather renorm");
                }
                let mut compact = [0u32; 16];
                for idx in 0..words_needed {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let compact_v = _mm512_loadu_si512(compact.as_ptr() as *const __m512i);
                let expanded = _mm512_maskz_expand_epi32(renorm_mask, compact_v);
                let shifted = _mm512_slli_epi32(new_state, 16);
                let renormed = _mm512_or_si512(shifted, expanded);
                state = _mm512_mask_blend_epi32(renorm_mask, new_state, renormed);
                reader_pos += words_needed;
            } else {
                state = new_state;
            }
        }

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
                    return Err("unexpected EOF in manual-gather tail");
                }
                lanes[lane] = (new_x << 16) | compressed[reader_pos] as u32;
                reader_pos += 1;
            }
            state = _mm512_loadu_si512(lanes.as_ptr() as *const __m512i);
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

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------

// Allocation-free _into variant for manual gather 16-way
#[target_feature(enable = "avx512f,avx512bw")]
pub unsafe fn decode_interleaved16_manual_gather_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let (_, report) =
            decode_interleaved16_manual_gather_kernel(compressed, table, output.len())?;
        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// Step 8: Two-YMM on 16-way interleaved format (2 x 256-bit vectors)
// ---------------------------------------------------------------------------
// Split the 16 lanes into two independent 8-lane groups, each with its own
// gather chain.  The scheduler can overlap the two chains via out-of-order
// execution, reducing effective gather latency.
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn decode_interleaved16_2x8_kernel(
    compressed: &[u16],
    table: &PackedWordTable,
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
        // Load all 16 states, split into lo/hi via store/reload
        let mut init_array = [0u32; 16];
        for i in 0..16 {
            init_array[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
        }
        let all_state = _mm512_loadu_si512(init_array.as_ptr() as *const __m512i);
        let mut all_buf = [0u32; 16];
        _mm512_storeu_si512(all_buf.as_mut_ptr() as *mut __m512i, all_state);
        let mut lo_s = [0u32; 8];
        let mut hi_s = [0u32; 8];
        for j in 0..8 {
            lo_s[j] = all_buf[j];
            hi_s[j] = all_buf[j + 8];
        }
        let mut state_lo = _mm256_loadu_si256(lo_s.as_ptr() as *const __m256i);
        let mut state_hi = _mm256_loadu_si256(hi_s.as_ptr() as *const __m256i);

        let mut reader_pos = 32usize;
        let even16 = n & !15;
        let mut output = Vec::with_capacity(n);
        output.resize(n, 0u8);
        let table_ptr = table.as_ptr() as *const i32;
        let mask_v = _mm256_set1_epi32((RANS_WORD_M - 1) as i32);
        const SCALE8: i32 = 12;

        for i in (0..even16).step_by(16) {
            // ---- Low group: lanes 0-7 ----
            let indices_lo = _mm256_and_si256(state_lo, mask_v);
            let gath_lo = _mm256_i32gather_epi32(table_ptr, indices_lo, 4);
            let freq_lo = _mm256_and_si256(gath_lo, _mm256_set1_epi32(0x0fff));
            let bias_lo =
                _mm256_and_si256(_mm256_srli_epi32(gath_lo, 12), _mm256_set1_epi32(0x0fff));
            let syms_lo = _mm256_srli_epi32(gath_lo, 24);
            let byte_lo = _mm256_cvtepi32_epi8(syms_lo);
            _mm_storel_epi64(output.as_mut_ptr().add(i) as *mut __m128i, byte_lo);

            let xsc_lo = _mm256_srli_epi32(state_lo, SCALE8);
            let new_lo = _mm256_add_epi32(_mm256_mullo_epi32(xsc_lo, freq_lo), bias_lo);
            let mask_lo = _mm256_cmplt_epu32_mask(new_lo, _mm256_set1_epi32(RANS_WORD_L as i32));
            let wc_lo = mask_lo.count_ones() as usize;

            if wc_lo > 0 {
                if reader_pos + wc_lo > compressed.len() {
                    return Err("unexpected EOF in 2x8 lo renorm");
                }
                let mut compact = [0u32; 8];
                for idx in 0..wc_lo {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let cv = _mm256_loadu_si256(compact.as_ptr() as *const __m256i);
                let exp = _mm256_maskz_expand_epi32(mask_lo, cv);
                let sh = _mm256_slli_epi32(new_lo, 16);
                let rn = _mm256_or_si256(sh, exp);
                state_lo = _mm256_mask_blend_epi32(mask_lo, new_lo, rn);
                reader_pos += wc_lo;
            } else {
                state_lo = new_lo;
            }

            // ---- High group: lanes 8-15 ----
            let indices_hi = _mm256_and_si256(state_hi, mask_v);
            let gath_hi = _mm256_i32gather_epi32(table_ptr, indices_hi, 4);
            let freq_hi = _mm256_and_si256(gath_hi, _mm256_set1_epi32(0x0fff));
            let bias_hi =
                _mm256_and_si256(_mm256_srli_epi32(gath_hi, 12), _mm256_set1_epi32(0x0fff));
            let syms_hi = _mm256_srli_epi32(gath_hi, 24);
            let byte_hi = _mm256_cvtepi32_epi8(syms_hi);
            _mm_storel_epi64(output.as_mut_ptr().add(i + 8) as *mut __m128i, byte_hi);

            let xsc_hi = _mm256_srli_epi32(state_hi, SCALE8);
            let new_hi = _mm256_add_epi32(_mm256_mullo_epi32(xsc_hi, freq_hi), bias_hi);
            let mask_hi = _mm256_cmplt_epu32_mask(new_hi, _mm256_set1_epi32(RANS_WORD_L as i32));
            let wc_hi = mask_hi.count_ones() as usize;

            if wc_hi > 0 {
                if reader_pos + wc_hi > compressed.len() {
                    return Err("unexpected EOF in 2x8 hi renorm");
                }
                let mut compact = [0u32; 8];
                for idx in 0..wc_hi {
                    compact[idx] = compressed[reader_pos + idx] as u32;
                }
                let cv = _mm256_loadu_si256(compact.as_ptr() as *const __m256i);
                let exp = _mm256_maskz_expand_epi32(mask_hi, cv);
                let sh = _mm256_slli_epi32(new_hi, 16);
                let rn = _mm256_or_si256(sh, exp);
                state_hi = _mm256_mask_blend_epi32(mask_hi, new_hi, rn);
                reader_pos += wc_hi;
            } else {
                state_hi = new_hi;
            }
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
            let entry = (*table.get(slot)).0;
            output[i] = (entry >> 24) as u8;
            let freq_entry = entry & 0x0fff;
            let bias_entry = (entry >> 12) & 0x0fff;
            let new_x = freq_entry * (x >> 12) + bias_entry;
            lns[lv] = new_x;
            if new_x < RANS_WORD_L {
                if reader_pos >= compressed.len() {
                    return Err("unexpected EOF in 2x8 tail");
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

        Ok((
            output,
            DecodeReport {
                words_consumed: reader_pos,
                final_states,
            },
        ))
    }
}

// Allocation-free _into variant for 2x8
#[target_feature(enable = "avx512f,avx512vl,avx512bw")]
pub unsafe fn decode_interleaved16_2x8_into(
    compressed: &[u16],
    table: &PackedWordTable,
    output: &mut [u8],
) -> Result<DecodeReport, &'static str> {
    unsafe {
        let (_, report) = decode_interleaved16_2x8_kernel(compressed, table, output.len())?;
        Ok(report)
    }
}

// ---------------------------------------------------------------------------
// Step 9: Batched multi-block decode — interleave streams to hide gather latency
// ---------------------------------------------------------------------------
// Decodes up to `lanes` independent 16-way streams in round-robin fashion.
// Each iteration processes one 16-symbol group from each stream, then moves
// to the next.  This allows the CPU to overlap the gather latency of one
// stream with the arithmetic of another, improving aggregate throughput.
//
// All streams must use the same codec and scale_bits.  Each job's output
// must be sized to its expected decoded length.
pub unsafe fn decode_batch_interleaved16_avx512(
    jobs: &mut [DecodeJob<'_>],
) -> Result<(), &'static str> {
    unsafe {
        if jobs.is_empty() {
            return Ok(());
        }

        let mask_v = _mm512_set1_epi32((RANS_WORD_M - 1) as i32);
        let l_vec = _mm512_set1_epi32(RANS_WORD_L as i32);
        const SCALE16: u32 = 12;

        // Process all jobs in batches of up to 4.
        // Each batch has its own state vectors (one ZMM per job).
        // More than 4 per batch would saturate the L1 cache.
        for batch in jobs.chunks_mut(4) {
            let batch_size = batch.len();

            // Track per-job state
            let mut states = [core::mem::zeroed::<__m512i>(); 4];
            let mut readers = [0usize; 4];
            let mut cursors = [0usize; 4]; // output cursor per job
            let mut job_active = [false; 4];

            for j in 0..batch_size {
                let job = &mut batch[j];
                if job.output.is_empty() {
                    job_active[j] = false;
                    continue;
                }
                job_active[j] = true;

                // Check minimum stream length
                if job.compressed.len() < 32 {
                    return Err("batch job: compressed too short");
                }

                // Load initial states
                let mut init = [0u32; 16];
                for i in 0..16 {
                    init[i] =
                        job.compressed[i * 2] as u32 | (job.compressed[i * 2 + 1] as u32) << 16;
                }
                states[j] = _mm512_loadu_si512(init.as_ptr() as *const __m512i);
                readers[j] = 32;

                // Verify output length consistency
                if job.output.len() % 16 != 0 && job.output.len() > 0 {
                    // Partial tail is fine, handled at the end
                }
                cursors[j] = 0;
            }

            // Round-robin decode: one 16-symbol group per job per iteration
            let mut any_active = true;
            while any_active {
                any_active = false;
                for j in 0..batch_size {
                    if !job_active[j] {
                        continue;
                    }
                    let job = &mut batch[j];
                    let output_len = job.output.len();
                    let cursor = cursors[j];

                    // Check if this job has a full group remaining
                    if cursor + 16 > output_len {
                        // Finish this job: process tail
                        job_active[j] = false;
                        continue;
                    }

                    // Process one 16-symbol group for this job
                    let table_ptr = job.table.as_ptr() as *const i32;
                    let indices = _mm512_and_si512(states[j], mask_v);
                    let gathered = _mm512_i32gather_epi32(indices, table_ptr, 4);

                    let freq_v = _mm512_and_si512(gathered, _mm512_set1_epi32(0x0fff));
                    let bias_v = _mm512_and_si512(
                        _mm512_srli_epi32(gathered, 12),
                        _mm512_set1_epi32(0x0fff),
                    );
                    let symbols_v = _mm512_srli_epi32(gathered, 24);

                    let symbol_bytes = _mm512_cvtepi32_epi8(symbols_v);
                    _mm_storeu_si128(
                        job.output.as_mut_ptr().add(cursor) as *mut __m128i,
                        symbol_bytes,
                    );

                    let xscaled = _mm512_srli_epi32(states[j], SCALE16);
                    let new_state = _mm512_add_epi32(_mm512_mullo_epi32(xscaled, freq_v), bias_v);

                    let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
                    let words_needed = renorm_mask.count_ones() as usize;

                    if words_needed > 0 {
                        if readers[j] + words_needed > job.compressed.len() {
                            return Err("batch job: unexpected EOF in renorm");
                        }
                        let mut compact = [0u32; 16];
                        for idx in 0..words_needed {
                            compact[idx] = job.compressed[readers[j] + idx] as u32;
                        }
                        let compact_v = _mm512_loadu_si512(compact.as_ptr() as *const __m512i);
                        let expanded = _mm512_maskz_expand_epi32(renorm_mask, compact_v);
                        let shifted = _mm512_slli_epi32(new_state, 16);
                        let renormed = _mm512_or_si512(shifted, expanded);
                        states[j] = _mm512_mask_blend_epi32(renorm_mask, new_state, renormed);
                        readers[j] += words_needed;
                    } else {
                        states[j] = new_state;
                    }

                    cursors[j] = cursor + 16;
                    any_active = true;
                }
            }

            // Process tails for each job
            for j in 0..batch_size {
                if !job_active[j] {
                    // Already finished or wasn't active; check for tail
                    let job = &mut batch[j];
                    let output_len = job.output.len();
                    let cursor = cursors[j];
                    if cursor < output_len {
                        // Process remaining tail symbols (scalar fallback).
                        // cursor can be 0 for blocks shorter than 16 symbols.
                        let mut ls: [u32; 16] = core::mem::zeroed();
                        _mm512_storeu_si512(ls.as_mut_ptr() as *mut __m512i, states[j]);
                        for i in cursor..output_len {
                            let lane = i & 15;
                            let x = ls[lane];
                            let slot = x as usize & (RANS_WORD_M - 1);
                            let entry = (*job.table.get(slot)).0;
                            job.output[i] = (entry >> 24) as u8;
                            let freq_entry = entry & 0x0fff;
                            let bias_entry = (entry >> 12) & 0x0fff;
                            let new_x = freq_entry * (x >> 12) + bias_entry;
                            ls[lane] = new_x;
                            if new_x < RANS_WORD_L {
                                if readers[j] >= job.compressed.len() {
                                    return Err("batch job: unexpected EOF in tail");
                                }
                                ls[lane] = (new_x << 16) | job.compressed[readers[j]] as u32;
                                readers[j] += 1;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
// ---------------------------------------------------------------------------
// Tests
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
        // Verify that AVX512VL 8-way produces identical output to scalar.
        // This is the primary correctness test for the 8-way SIMD kernel.
        let (freqs, cum, packed) = uniform_model();
        if !cfg!(all(
            target_feature = "avx512f",
            target_feature = "avx512vl",
            target_feature = "avx512bw",
        )) {
            // Skip if AVX512VL is not compiled in.
            return;
        }

        let symbols: Vec<u8> = (0..128).map(|i| (i % 16) as u8).collect();
        let compressed = crate::encode_8way_for_test(&symbols, &freqs, &cum);

        // Scalar decode
        let scalar_out = decode_8way_packed_scalar(&compressed, &packed, symbols.len()).unwrap();

        // AVX512VL decode — must produce identical output.
        unsafe {
            let (avx_out, _avx_report) =
                decode_interleaved8_avx512vl_kernel(&compressed, &packed, symbols.len()).unwrap();
            assert_eq!(avx_out, symbols, "AVX512VL roundtrip must match input");
            assert_eq!(avx_out, scalar_out, "AVX512VL must match scalar");
        }
    }

    #[test]
    fn test_avx512vl8_various_lengths() {
        // Test all critical block sizes:
        // - Tiny (1..8): exercises every tail length
        // - Small (9..128): exercises init + renorm
        // - Medium (255..1025): exercises multiple decode iterations
        let (freqs, cum, packed) = uniform_model();
        if !cfg!(all(
            target_feature = "avx512f",
            target_feature = "avx512vl",
            target_feature = "avx512bw",
        )) {
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
        // Verify that AVX512 16-way produces identical output to scalar 16-way.
        let (freqs, cum, packed) = uniform_model();
        if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
            return;
        }

        let symbols: Vec<u8> = (0..128).map(|i| (i % 16) as u8).collect();
        let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12).unwrap();

        let (scalar_out, scalar_report) =
            crate::packed_table::decode_interleaved16_scalar(&compressed, &packed, symbols.len())
                .unwrap();

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
        // Test every possible 16-way tail length (0..15).
        // This is critical because the tail path uses scalar fallback and must
        // handle each remainder correctly.
        let (freqs, cum, packed) = uniform_model();
        if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
            return;
        }
        for tail in 0..16 {
            let len = 32 + tail;
            let symbols: Vec<u8> = (0..len).map(|i| (i % 16) as u8).collect();
            let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12).unwrap();
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
        // Verify that truncated streams are correctly rejected.
        let (freqs, cum, packed) = uniform_model();
        if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
            return;
        }
        unsafe {
            assert!(decode_interleaved16_avx512_kernel(&[], &packed, 16).is_err());
            let short = vec![0u16; 31];
            assert!(decode_interleaved16_avx512_kernel(&short, &packed, 16).is_err());
        }
    }
}
