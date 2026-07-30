//! # AVX2 permutation-based renormalization
//!
//! AVX2 lacks AVX-512's `_mm256_maskz_expand_epi32` instruction for
//! distributing compact renormalization words to active lanes.  Instead,
//! we use a **permutation table** approach:
//!
//! 1. Compare each state against RANS_WORD_L using unsigned comparison to
//!    produce an 8-bit mask (which lanes need renormalization).
//! 2. Count ones in the mask to determine how many u16 words to consume.
//! 3. Copy those compact words into a u32 scratch buffer.
//! 4. Load the permutation indices for this mask from a 256-entry table.
//! 5. Use `_mm256_permutevar8x32_epi32` (vpermd) to distribute compact
//!    words to their correct lanes.
//! 6. Shift each state left by 16 bits and OR with the expanded words.
//! 7. Blend the renormalized lanes with the unchanged lanes using
//!    `_mm256_blendv_epi8`.
//!
//! ## Table format
//!
//! For each 8-bit mask (256 entries), store the source index for each of
//! the 8 output lanes.  Active lanes receive compact words in ascending
//! lane order.  Inactive lanes may reference any safe source index because
//! they will be removed by blending.
//!
//! ## Safety
//!
//! Requires AVX2 at runtime.  The input slice must have at least as many
//! u16 words as the popcount of the observed mask.

use crate::RANS_WORD_L;
use crate::backends::DecodeError;
use core::arch::x86_64::*;

/// Result of an 8-way AVX2 renormalization operation.
#[derive(Debug, Clone, Copy)]
pub struct Avx2Renorm8Result {
    pub states: [u32; 8],
    pub observed_mask: u8,
    pub words_consumed: usize,
}

/// The 256-entry permutation lookup table for AVX2 renormalization.
///
/// `indices[mask][lane]` gives the source index for `vpermd` to place
/// the correct compact word at `lane`.  Active lane 0 gets compact word 0,
/// active lane 1 gets compact word 1, etc.  Inactive lanes are set to 0
/// (they will be masked away by blend).
#[repr(align(32))]
#[derive(Clone)]
pub struct Avx2RenormPermutations {
    pub indices: [[i32; 8]; 256],
}

impl core::fmt::Debug for Avx2RenormPermutations {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Avx2RenormPermutations")
            .field("table_size", &self.indices.len())
            .finish()
    }
}

/// Build the 256-entry permutation table.
///
/// For each mask (0..255), compute the source index for each of the 8 lanes.
/// For mask bit = 1 (active), assign the next compact word index.
/// For mask bit = 0 (inactive), assign 0 (safe because blending removes it).
///
/// This function is deterministic and should be called once at startup or
/// tested exhaustively at compile-time.
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
/// Takes 8 states as a `__m256i`, determines which lanes have state <
/// RANS_WORD_L, reads the required number of u16 renorm words from
/// `input`, distributes them to the correct lanes via the permutation
/// table, and returns the updated states plus metadata.
///
/// # Safety
///
/// - Requires AVX2 CPU feature at runtime.
/// - `states` must be a valid `__m256i` (8 × u32).
/// - `input` must have at least `popcount(mask)` words available.
/// - `perm_table` must be the pre-built `Avx2RenormPermutations`.
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
