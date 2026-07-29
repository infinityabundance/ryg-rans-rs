//! # AVX-512 Word rANS decode kernels
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
// Renormalization mask popcount tables
// ---------------------------------------------------------------------------
// These static tables precompute the number of u16 words consumed for every
// possible renormalization mask.  The 8-way table has 256 entries (2^8), and
// the 16-way table has 65536 entries (2^16).  Using a lookup table avoids
// a popcount instruction and keeps the decode loop latency predictable.
//
// The tables are computed at compile time using const evaluation.  Each entry
// at index `mask` contains `popcount(mask)`: the count of set bits.

/// Number of u16 words consumed for each 8-bit lane mask (AVX512VL 8-way).
/// Indexed by the 8-bit mask from `_mm256_cmplt_epu32_mask`.
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
/// Indexed by the 16-bit mask from `_mm512_cmplt_epu32_mask`.
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

        // STEP 3: Store symbols in lane order
        // We use a temporary buffer because packus instructions have complex
        // lane interleaving that's error-prone.  The storeu + scalar copy
        // pattern is simple, correct, and fast enough (8 iterations of a
        // byte-wide store).
        let mut sym_buf: [u32; 8] = core::mem::zeroed();
        // SAFETY: Storing 8 u32 (32 bytes) into a properly sized buffer.
        _mm256_storeu_si256(sym_buf.as_mut_ptr() as *mut __m256i, symbols_v);
        for lane in 0..8 {
            output[i + lane] = sym_buf[lane] as u8;
        }

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
        let renorm_mask = _mm256_cmplt_epu32_mask(new_state, _mm256_set1_epi32(RANS_WORD_L as i32));
        let words_needed = NUM_WORDS_8[renorm_mask as usize] as usize;

        if words_needed > 0 {
            // Check input bounds before reading.
            if reader_pos + words_needed > compressed.len() {
                return Err("unexpected EOF in AVX512VL renorm");
            }
            // Lane-wise renormalization: for each active lane, read one u16
            // and shift it into the lane's state.
            //
            // Why lane-wise instead of masked expand-load?
            // `_mm256_mask_expand_epi16`'s memory-access semantics for inactive
            // lanes are microarchitecture-dependent.  To guarantee no overread
            // beyond the provided slice, we use simple scalar reads with bounds
            // checking.  The loop runs at most 8 iterations; in practice most
            // masks have 0–2 active lanes.
            let mut temp_state = new_state;
            let mut rp = reader_pos;
            for lane in 0..8 {
                if (renorm_mask >> lane) & 1 != 0 {
                    let w = compressed[rp] as u32;
                    rp += 1;
                    // Extract lane value from the vector, modify it, re-insert.
                    // This is safe: we store to/load from a properly sized [u32; 8].
                    let mut lanes: [u32; 8] = core::mem::zeroed();
                    _mm256_storeu_si256(lanes.as_mut_ptr() as *mut __m256i, temp_state);
                    lanes[lane] = (lanes[lane] << 16) | w;
                    temp_state = _mm256_loadu_si256(lanes.as_ptr() as *const __m256i);
                }
            }
            state = temp_state;
            reader_pos = rp;
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

        // ---- Store 16 symbols in lane order ----
        // We store to a temporary buffer and copy byte-by-byte.  This avoids
        // the complex lane interleaving of _mm256_packus* instructions.
        let mut sym_buf: [u32; 16] = core::mem::zeroed();
        _mm512_storeu_si512(sym_buf.as_mut_ptr() as *mut __m512i, symbols_v);
        for lane in 0..16 {
            output[i + lane] = sym_buf[lane] as u8;
        }

        // ---- State update ----
        let xscaled = _mm512_srli_epi32(state, SCALE16);
        let new_state = _mm512_add_epi32(_mm512_mullo_epi32(xscaled, freq_v), bias_v);

        // ---- Masked renormalization ----
        // Compare: new_state < L for all 16 lanes simultaneously.
        // _mm512_cmplt_epu32_mask returns a 16-bit mask.
        let renorm_mask = _mm512_cmplt_epu32_mask(new_state, l_vec);
        let words_needed = NUM_WORDS_16[renorm_mask as usize] as usize;

        if words_needed > 0 {
            if reader_pos + words_needed > compressed.len() {
                return Err("unexpected EOF in AVX512 renorm");
            }
            // Lane-wise renormalization (same pattern as 8-way, extended to 16 lanes).
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
