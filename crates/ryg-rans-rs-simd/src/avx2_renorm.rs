//! # AVX2 permutation-based renormalization
//!
//! AVX2 lacks AVX-512's `_mm256_maskz_expand_epi32` instruction for
//! distributing compact renormalization words to active lanes.  Instead,
//! we use a **permutation table** approach.  This module implements the
//! full renormalization pipeline for 8-way SIMD decode on AVX2.
//!
//! ## The Renormalization Problem
//!
//! In Word rANS, after each decode step, a state may drop below the
//! threshold `RANS_WORD_L` (65536).  When that happens, we need to
//! "renormalize" by shifting the state left by 16 bits and ORing in
//! a u16 word from the compressed stream.  Critically, **which lanes
//! need renormalization is data-dependent** — it varies each iteration
//! based on the symbols decoded.
//!
//! In SIMD, the challenge is: we have 8 independent states, each of which
//! may or may not need a word.  The renorm words are stored **compactly**
//! in the input stream — only the lanes that actually need renorming
//! consume a word.  We need to:
//!
//! 1. Determine which lanes need renorming (compute the mask).
//! 2. Count how many words to consume (popcount of the mask).
//! 3. Load exactly those words from the stream.
//! 4. Distribute them to the correct lanes.
//! 5. Left-shift each state by 16 and OR with the distributed word.
//! 6. Blend renormed lanes with unchanged lanes.
//!
//! AVX-512 does this in one instruction (`_mm256_maskz_expand_epi32`).
//! AVX2 requires a multi-step workaround.
//!
//! ## The Permutation Table Approach
//!
//! The key insight is that since the 8-bit mask has only 256 possible values,
//! we can **precompute** the shuffle pattern for every possible mask.  For
//! each mask value, we store 8 permutation indices that tell `vpermd` where
//! to place each compact word.
//!
//! The algorithm:
//!
//! 1. **Compute the mask**: Use the signed compare trick (XOR with 0x80000000)
//!    to produce an unsigned-less-than comparison, then compress the 32-bit
//!    movemask result into an 8-bit lane mask.
//!
//! 2. **If mask == 0**: No lanes need renormalization.  Return unchanged.
//!
//! 3. **Count words**: `mask_u8.count_ones()` gives the number of u16 words
//!    to consume from the compressed stream.
//!
//! 4. **Load compact words**: Copy the required number of u16 words from the
//!    input stream into a `[u32; 8]` scratch buffer (zero-extended to u32).
//!    Load this buffer into a YMM register.
//!
//! 5. **Look up permutation indices**: For the observed mask, load 8 × i32
//!    permutation indices from the precomputed table.
//!
//! 6. **Permute**: `_mm256_permutevar8x32_epi32(compact_v, perm_indices)`
//!    distributes the compact words to their correct lanes.  Active lane N
//!    receives the N-th compact word (in lane-order); inactive lanes receive
//!    word 0 (which will be discarded by blending).
//!
//! 7. **Shift and OR**: `_mm256_slli_epi32(states, 16)` then OR with the
//!    permuted words.  This produces candidate renormed states.
//!
//! 8. **Blend**: Use `_mm256_blendv_epi8` to select renormed lanes where
//!    the mask bit is 1, and original states where the mask bit is 0.
//!    The blend mask is constructed by broadcasting the 8-bit mask and
//!    extracting each lane's bit with `_mm256_srlv_epi32`.
//!
//! ## Why a table instead of computed shuffle masks?
//!
//! Computing the permutation indices on-the-fly would require a loop or
//! a series of conditional moves — both of which are slow and hard to
//! vectorize.  By precomputing the table at startup (or compile-time), we
//! reduce the per-iteration work to:
//!
//! - One `_mm256_load_si256` (aligned load from the table)
//! - One `_mm256_permutevar8x32_epi32` (vpermd)
//!
//! The table is only **256 × 8 × 4 = 8 KB**, fitting comfortably in L1
//! cache (which is 32 KB on modern x86).  The table is also fully
//! deterministic — the same mask always produces the same permutation,
//! which is essential for reproducible decoding.
//!
//! ## Comparison with AVX-512
//!
//! | Step | AVX2 (this module) | AVX-512 |
//! |------|-------------------|---------|
//! | Mask compute | XOR + signed compare + movemask compaction | `_mm256_cmplt_epu32_mask` (1 uop) |
//! | Expand | Precomputed table + vpermd | `_mm256_maskz_expand_epi32` (1 uop) |
//! | Blend | Broadcast + srlv + sub + blendv | `_mm256_mask_blend_epi32` (1 uop) |
//! | Table size | 8 KB | None |
//! | Per-iteration cost | ~5-6 uops | ~3 uops |
//!
//! The AVX2 approach is about 2–3× more expensive per renormalization
//! iteration, but still much faster than a scalar fallback (which would
//! require 8 separate branches).
//!
//! ## Table format
//!
//! For each 8-bit mask (256 entries), we store the source index for each of
//! the 8 output lanes:
//!
//! ```text
//! indices[mask][lane] = compact_word_index   // if lane is active
//!                      or 0                  // if lane is inactive (ignored by blend)
//! ```
//!
//! Active lanes receive compact words in **ascending lane order**:
//! - The lowest-numbered active lane gets compact word 0
//! - The next active lane gets compact word 1
//! - ... and so on.
//!
//! This ensures the renorm words are consumed in the same order as the
//! scalar decoder: lane 0 first, then lane 1, etc.
//!
//! Inactive lanes are assigned index 0 (safe because `blendv` discards them).
//! Any value ≤ 7 would work for inactive lanes since they're masked away,
//! but 0 is the simplest to verify in tests.
//!
//! ## Safety
//!
//! Requires AVX2 at runtime.  The input slice must have at least as many
//! u16 words as the popcount of the observed mask.

use crate::RANS_WORD_L;
use crate::backends::DecodeError;
use core::arch::x86_64::*;

/// Result of an 8-way AVX2 renormalization operation.
///
/// Contains the updated 8 states after renormalization, plus metadata:
/// - `observed_mask`: Which lanes needed renormalization (bit N = 1 means
///   lane N consumed a word).  This is useful for debugging and verification.
/// - `words_consumed`: Number of u16 words consumed from the input stream.
///   Equals `observed_mask.count_ones()` in normal operation, but the caller
///   should use this field rather than recomputing popcount to avoid bugs.
///
/// The calling decode kernel is responsible for advancing its reader position
/// by `words_consumed` and updating the state vector.
#[derive(Debug, Clone, Copy)]
pub struct Avx2Renorm8Result {
    pub states: [u32; 8],
    pub observed_mask: u8,
    pub words_consumed: usize,
}

/// The 256-entry permutation lookup table for AVX2 renormalization.
///
/// This table maps every possible 8-bit lane mask to a set of 8 permutation
/// indices that `vpermd` (`_mm256_permutevar8x32_epi32`) uses to distribute
/// compact renormalization words to their correct lanes.
///
/// ## Why a table?
///
/// The AVX2 `vpermd` instruction selects each output lane from any of the 8
/// source lanes using a 32-bit index per lane.  We need to map:
///
/// ```text
/// compact_words[0..popcount(mask)]  →  active lanes in ascending order
/// ```
///
/// This is a gather-like operation that would require a loop or conditionals
/// to compute on-the-fly.  Precomputing the 256-entry table is cheap (8 KB)
/// and turns the critical path into a single aligned load + vpermd.
///
/// ## Table construction
///
/// `indices[mask][lane]` =
/// - If bit `lane` of `mask` is 1: the sequential index of this lane among
///   active lanes (0 for the first active lane, 1 for the second, etc.)
/// - If bit `lane` of `mask` is 0: 0 (a safe value discarded by the blend)
///
/// For example, `mask = 0b00000101` (lanes 0 and 2 active):
/// ```text
/// indices[0b00000101] = [0, 0, 1, 0, 0, 0, 0, 0]
/// // lane 0 gets compact word 0
/// // lane 2 gets compact word 1
/// ```
///
/// `vpermd` with these indices would place compact_words[0] at lane 0,
/// compact_words[1] at lane 2, and compact_words[0] at all other lanes
/// (safe because blending removes them).
///
/// ## Alignment
///
/// `#[repr(align(32))]` ensures each row of 8 × i32 (32 bytes) is aligned
/// to a 256-bit vector boundary, so `_mm256_load_si256` (aligned load) can
/// be used instead of the slower `_mm256_loadu_si256`.
#[repr(align(32))]
#[derive(Clone)]
pub struct Avx2RenormPermutations {
    /// `indices[mask][lane]` — 256 masks × 8 lanes each.
    /// Must be accessed with mask values in range 0..=255 only.
    pub indices: [[i32; 8]; 256],
}

impl core::fmt::Debug for Avx2RenormPermutations {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Avx2RenormPermutations")
            .field("table_size", &self.indices.len())
            .finish()
    }
}

/// Build the 256-entry permutation table for AVX2 renormalization.
///
/// For each mask value from 0 to 255, computes the 8 permutation indices
/// that tell `vpermd` where to distribute compact renormalization words.
///
/// ## Algorithm
///
/// For mask bit = 1 at position `lane`:
///   - This lane is **active** (needs a renormalization word).
///   - Assign the next available compact word index (0, 1, 2, ...)
///     in ascending lane order (lane 0 first).
/// For mask bit = 0 at position `lane`:
///   - This lane is **inactive** (does not need a word).
///   - Assign index 0 (any value works, but 0 is simple to verify in tests).
///
/// ## Determinism
///
/// This function is fully deterministic — the same input always produces
/// the same table.  It can be called at startup or even in tests without
/// randomization concerns.  The table is verified exhaustively in unit
/// tests (`test_permutation_table_coverage`).
///
/// ## Memory
///
/// The table is 256 × 8 × 4 = 8,192 bytes (8 KB), fitting comfortably in
/// L1 cache (typically 32 KB on modern x86).  The 32-byte alignment ensures
/// each row can be loaded with a single aligned `vmovdqa` instruction.
///
/// ## Usage
///
/// This function is called once per decode operation by the `_checked`
/// wrappers in `backends.rs` and once at `Avx2Context::new()`.  For batch
/// operations (`decode_batch4_interleaved16_avx2`), a single table is shared
/// across all jobs.
pub fn build_avx2_renorm_table() -> Avx2RenormPermutations {
    let mut indices = [[0i32; 8]; 256];
    for mask in 0..=255u16 {
        let mut compact_idx = 0i32;
        for lane in 0..8u32 {
            if (mask >> lane) & 1 == 1 {
                indices[mask as usize][lane as usize] = compact_idx;
                compact_idx += 1;
            } else {
                // Inactive lane — safe value because blend removes it.
                indices[mask as usize][lane as usize] = 0;
            }
        }
    }
    Avx2RenormPermutations { indices }
}

/// Perform 8-way AVX2 renormalization.
///
/// This is the core SIMD renormalization primitive used by all AVX2 decode
/// kernels.  It determines which of the 8 lanes need renormalization (their
/// state is below `RANS_WORD_L`), consumes the required number of u16 words
/// from the input stream, distributes them to the correct lanes via the
/// permutation table, and returns the updated states.
///
/// ## Algorithm Details
///
/// ### Step 1: Compute the unsigned comparison mask (lines 107–138)
///
/// AVX2 lacks a direct unsigned compare instruction for 32-bit integers.
/// We work around this with the **signed compare trick**:
///
/// ```text
/// state < L  (unsigned)  ⇔  (state ^ 0x80000000) < (L ^ 0x80000000)  (signed)
/// ```
///
/// XOR with `0x80000000` flips the sign bit, effectively biasing unsigned
/// values into signed-comparable space.  Then `_mm256_cmpgt_epi32` performs
/// a signed greater-than comparison.  By swapping operands (`biased_l >
/// biased_states`), we get the equivalent of unsigned less-than.
///
/// The comparison result is a vector of all-ones (true) or all-zeros (false)
/// for each lane.  `_mm256_movemask_epi8` extracts the sign bit of each byte
/// into a 32-bit mask.  Since each 32-bit lane has its comparison result sign
/// in byte `4*i+3`, we compact those 8 bits into a u8 lane mask.
///
/// ### Step 2: Early exit if no lanes need renorm (lines 140–149)
///
/// If `mask_u8 == 0`, all states are ≥ L and no renormalization is needed.
/// We store the unchanged states and return with `words_consumed = 0`.
/// This is the common case for large states (most decodes produce states
/// well above L).
///
/// ### Step 3: Count words needed and check bounds (lines 151–156)
///
/// `mask_u8.count_ones()` gives the number of u16 words to consume.  We
/// verify the input has enough words before reading to avoid out-of-bounds
/// access.
///
/// ### Step 4: Pack compact words into u32 scratch buffer (lines 158–163)
///
/// The compact words are 16-bit, but `vpermd` operates on 32-bit lanes.
/// We zero-extend each u16 to u32 and store in a `[u32; 8]` buffer on the
/// stack, then load it into a YMM register.
///
/// ### Step 5: Load permutation indices from table (lines 165–167)
///
/// `perm_table.indices[mask_u8 as usize]` is a `[i32; 8]` exactly 32 bytes
/// (aligned to 32 bytes by the `#[repr(align(32))]` on `Avx2RenormPermutations`).
/// We load it with an aligned 256-bit load into a YMM register.
///
/// ### Step 6: Permute compact words to correct lanes (lines 169–172)
///
/// `_mm256_permutevar8x32_epi32(compact_v, perm_indices)` (vpermd):
/// For each output lane `i`, `output[i] = compact_v[perm_indices[i]]`.
/// Active lanes get the correct compact word; inactive lanes get word 0
/// (safe because the next step blends them away).
///
/// ### Step 7: Shift state and OR with expanded words (lines 174–176)
///
/// Each state is left-shifted by 16 bits (`_mm256_slli_epi32(states, 16)`)
/// to make room for the 16-bit renorm word, then ORed with the expanded
/// words.  This produces candidate renormed states for all lanes.
///
/// ### Step 8: Blend renormed with unchanged lanes (lines 178–194)
///
/// We need to select renormed states only where the mask bit = 1.  Since
/// AVX2 lacks a per-lane u32 blend, we use `_mm256_blendv_epi8` (byte-level
/// blend).  The blend mask must be all-ones (0xFFFFFFFF) for active lanes
/// and all-zeros for inactive lanes.
///
/// The blend mask is constructed by:
/// 1. Broadcasting the 8-bit mask to all 32-bit lanes.
/// 2. Shifting right by lane index to extract each lane's bit.
/// 3. Subtracting from zero: `0 - bit` produces all-ones for bit=1,
///    all-zeros for bit=0.
///
/// The final result selects renormed lanes where mask=1, original states
/// where mask=0.
///
/// ### Step 9: Store and return (lines 196–204)
///
/// The blended result is stored to a `[u32; 8]` array and returned via
/// `Avx2Renorm8Result` along with the mask and word count.
///
/// ## Why this is correct
///
/// The key invariant is that **renorm words are consumed in ascending lane
/// order** (lane 0 first).  This is guaranteed by the permutation table
/// construction: `build_avx2_renorm_table` assigns compact word 0 to the
/// lowest-numbered active lane, word 1 to the next, etc.  This matches the
/// scalar decoder's loop order and the encoder's flush order (reverse lane
/// order on encode, so forward decode reads ascending).
///
/// ## Safety
///
/// - Requires AVX2 CPU feature at runtime.
/// - `states` must be a valid `__m256i` (8 × u32).
/// - `input` must have at least `popcount(mask)` words available.
/// - `perm_table` must be a valid, pre-built `Avx2RenormPermutations`.
/// - The function is marked `#[target_feature(enable = "avx2")]` but this
///   does not check the feature at runtime — the caller must ensure AVX2
///   is available before calling.
#[target_feature(enable = "avx2")]
pub unsafe fn renorm8_avx2(
    states: __m256i,
    input: &[u16],
    perm_table: &Avx2RenormPermutations,
) -> Result<Avx2Renorm8Result, DecodeError> {
    unsafe {
        // ---- Step 1: Compute mask using signed compare trick ----
        // AVX2 has no unsigned compare instruction, so we XOR each state
        // with 0x80000000 (flipping the sign bit) and compare against
        // RANS_WORD_L ^ 0x80000000 using signed comparison.
        //
        // This works because:
        //   state < L  (unsigned)
        //   ⇔ state ^ 0x80000000 < L ^ 0x80000000  (signed)
        //
        // We use _mm256_cmpgt_epi32 (signed compare) by swapping operands:
        //   desired:  biased_L > biased_state
        //   ⇔ _mm256_cmpgt_epi32(biased_l, biased_state)
        let flip = _mm256_set1_epi32(i32::MIN); // 0x80000000
        let biased_states = _mm256_xor_si256(states, flip);
        let biased_l = _mm256_set1_epi32((RANS_WORD_L as i32) ^ i32::MIN);
        // cmplt equivalent: biased_l > biased_states
        let cmp = _mm256_cmpgt_epi32(biased_l, biased_states);
        // Extract high bit of each lane as mask bits
        let mask32: i32 = _mm256_movemask_epi8(cmp); // 32-bit movemask
        // _mm256_movemask_epi8 returns a 32-bit mask where bit N is the sign
        // of byte N.  For 32-bit dword lane i, bytes 4*i..4*i+3 correspond.
        // The comparison result (all-ones or all-zeros) has its sign in byte
        // 4*i+3, which is movemask bit 4*i+3.
        // We compact these 8 bits into a u8 lane mask.
        let mask_u8 = (((mask32 >> 3) & 1) as u8)
            | ((((mask32 >> 7) & 1) as u8) << 1)
            | ((((mask32 >> 11) & 1) as u8) << 2)
            | ((((mask32 >> 15) & 1) as u8) << 3)
            | ((((mask32 >> 19) & 1) as u8) << 4)
            | ((((mask32 >> 23) & 1) as u8) << 5)
            | ((((mask32 >> 27) & 1) as u8) << 6)
            | ((((mask32 >> 31) & 1) as u8) << 7);

        if mask_u8 == 0 {
            // No lanes need renormalization — return unchanged.
            let mut final_states = [0u32; 8];
            _mm256_storeu_si256(final_states.as_mut_ptr() as *mut __m256i, states);
            return Ok(Avx2Renorm8Result {
                states: final_states,
                observed_mask: 0,
                words_consumed: 0,
            });
        }

        let words_needed = mask_u8.count_ones() as usize;

        // Check input bounds before reading.
        if words_needed > input.len() {
            return Err(DecodeError::InputTooShort);
        }

        // ---- Step 2: Pack compact words into u32 scratch buffer ----
        let mut compact = [0u32; 8];
        for idx in 0..words_needed {
            compact[idx] = input[idx] as u32;
        }
        let compact_v = _mm256_loadu_si256(compact.as_ptr() as *const __m256i);

        // ---- Step 3: Load permutation indices from table ----
        let perm_indices =
            _mm256_load_si256(perm_table.indices[mask_u8 as usize].as_ptr() as *const __m256i);

        // ---- Step 4: Permute compact words to correct lanes ----
        // vpermd: for each output lane i, source = perm_indices[i]
        // Active lanes get the correct compact word; inactive lanes get word 0.
        let expanded = _mm256_permutevar8x32_epi32(compact_v, perm_indices);

        // ---- Step 5: Shift state left by 16 and OR with expanded words ----
        let shifted = _mm256_slli_epi32(states, 16);
        let renorm_candidates = _mm256_or_si256(shifted, expanded);

        // ---- Step 6: Blend — select renormed lanes where mask bit = 1 ----
        // We need an 8-way blend mask.  Convert the 8-bit mask to a vector
        // of all-ones for active lanes, all-zeros for inactive lanes.
        // _mm256_blendv_epi8 uses the high bit of each byte, so we broadcast
        // the mask and replicate each bit across 4 bytes.
        let mask_broadcast = _mm256_set1_epi32(mask_u8 as i32);
        // Shift so each lane's top bit matches whether it's active.
        // Lane i: shift right by i, mask with 1, multiply by 0xFF... to get all-ones.
        // Simpler: build blend mask via _mm256_cmpeq_epi32 on per-lane compare.
        // We compute: lane_active ? all_ones : all_zeros
        let lane_indices = _mm256_set_epi32(7, 6, 5, 4, 3, 2, 1, 0);
        let shifted_mask = _mm256_srlv_epi32(mask_broadcast, lane_indices);
        let lane_bit = _mm256_and_si256(shifted_mask, _mm256_set1_epi32(1));
        // Blend mask: all-ones (0xFFFFFFFF) for active lanes, all-zeros for inactive
        let blend_mask = _mm256_sub_epi32(_mm256_setzero_si256(), lane_bit);

        let result = _mm256_blendv_epi8(states, renorm_candidates, blend_mask);

        // ---- Step 7: Store result ----
        let mut final_states = [0u32; 8];
        _mm256_storeu_si256(final_states.as_mut_ptr() as *mut __m256i, result);

        Ok(Avx2Renorm8Result {
            states: final_states,
            observed_mask: mask_u8,
            words_consumed: words_needed,
        })
    }
}

/// Non-SIMD helper to compute the unsigned comparison mask for 8 states.
///
/// This is used in test code to verify that the AVX2 signed-compare trick
/// produces the same mask as the scalar unsigned comparison.
pub fn scalar_renorm_mask_8way(states: &[u32; 8]) -> u8 {
    let mut mask = 0u8;
    for i in 0..8 {
        if states[i] < RANS_WORD_L {
            mask |= 1 << i;
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permutation_table_coverage() {
        let table = build_avx2_renorm_table();
        // Verify that for every mask, active lanes get distinct compact indices
        // in ascending order, and inactive lanes get 0.
        for mask in 0..=255u8 {
            let mut expected_compact = 0i32;
            for lane in 0..8 {
                let idx = table.indices[mask as usize][lane as usize];
                if (mask >> lane) & 1 == 1 {
                    assert_eq!(
                        idx, expected_compact,
                        "mask {:08b} lane {}: expected compact idx {}, got {}",
                        mask, lane, expected_compact, idx
                    );
                    expected_compact += 1;
                } else {
                    // Inactive: should be 0 (safe value for blend removal)
                    assert_eq!(
                        idx, 0,
                        "mask {:08b} lane {}: inactive lane should have index 0, got {}",
                        mask, lane, idx
                    );
                }
            }
        }
    }

    #[test]
    fn test_permutation_table_deterministic() {
        let t1 = build_avx2_renorm_table();
        let t2 = build_avx2_renorm_table();
        for mask in 0..=255 {
            for lane in 0..8 {
                assert_eq!(
                    t1.indices[mask][lane], t2.indices[mask][lane],
                    "mask {} lane {} differs between runs",
                    mask, lane
                );
            }
        }
    }

    #[test]
    fn test_scalar_mask_matches_bits() {
        // Verify the scalar mask function
        let states = [0u32, 1, 65535, 65536, 100000, 200000, 50000, 70000];
        // RANS_WORD_L = 65536
        let mask = scalar_renorm_mask_8way(&states);
        assert_eq!((mask >> 0) & 1, 1);
        assert_eq!((mask >> 1) & 1, 1);
        assert_eq!((mask >> 2) & 1, 1);
        assert_eq!((mask >> 3) & 1, 0);
        assert_eq!((mask >> 4) & 1, 0);
        assert_eq!((mask >> 5) & 1, 0);
        assert_eq!((mask >> 6) & 1, 1); // 50000 < 65536
        assert_eq!((mask >> 7) & 1, 0); // 70000 >= 65536
    }

    #[test]
    fn test_scalar_exhaustive_256_masks() {
        // For every mask, construct states with known <L / >=L pattern
        // and verify the scalar mask matches.
        for mask in 0..=255u8 {
            let mut states = [RANS_WORD_L; 8];
            for lane in 0..8 {
                if (mask >> lane) & 1 == 1 {
                    states[lane] = lane as u32; // < RANS_WORD_L
                } else {
                    states[lane] = RANS_WORD_L + lane as u32; // >= RANS_WORD_L
                }
            }
            let observed = scalar_renorm_mask_8way(&states);
            assert_eq!(
                observed, mask,
                "exhaustive mask test: expected {:08b}, got {:08b}",
                mask, observed
            );
        }
    }

    /// Test that the signed-compare trick produces the correct mask.
    /// This test does NOT require AVX2 hardware — it tests the scalar
    /// reference for the comparison logic.
    #[test]
    fn test_signed_compare_trick_scalar() {
        // The signed compare trick: (state XOR 0x80000000) < (L XOR 0x80000000)
        // as a SIGNED comparison is equivalent to state < L as unsigned.
        for state in [0u32, 1, 65535, 65536, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF] {
            let biased_state = state ^ 0x80000000;
            let biased_l = RANS_WORD_L ^ 0x80000000;
            let unsigned_lt = state < RANS_WORD_L;
            let signed_lt = (biased_state as i32) < (biased_l as i32);
            assert_eq!(
                unsigned_lt, signed_lt,
                "signed compare trick fails for state=0x{:08X}, L=0x{:08X}",
                state, RANS_WORD_L
            );
        }
    }
}
