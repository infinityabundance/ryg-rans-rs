#![no_std]
#![cfg(target_arch = "x86_64")]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

use alloc::vec;
use alloc::vec::Vec;
use core::arch::x86_64::*;

pub mod avx512;
pub mod backends;
pub mod packed_table;

// ---------------------------------------------------------------------------
// Re-export core word rANS constants for convenience
// ---------------------------------------------------------------------------

pub const RANS_WORD_M: usize = 4096;
pub const RANS_WORD_SCALE_BITS: u32 = 12;
pub const RANS_WORD_L: u32 = 1u32 << 16;

// ---------------------------------------------------------------------------
// Word rANS slot
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct RansWordSlot {
    pub freq: u16,
    pub bias: u16,
}

impl RansWordSlot {
    #[inline]
    pub fn pack(&self) -> u32 {
        (self.bias as u32) << 16 | self.freq as u32
    }
}

#[derive(Debug, Clone)]
pub struct RansWordTables<'a> {
    pub slots: &'a [RansWordSlot],
    pub slot2sym: &'a [u8],
}

// ---------------------------------------------------------------------------
// Word rANS table initialization
// ---------------------------------------------------------------------------

#[inline]
pub fn rans_word_tables_init_symbol(
    slots: &mut [RansWordSlot],
    slot2sym: &mut [u8],
    sym: u8,
    start: usize,
    freq: usize,
) {
    for i in 0..freq {
        let slot = start + i;
        if slot < slots.len() && slot < slot2sym.len() {
            slot2sym[slot] = sym;
            slots[slot] = RansWordSlot {
                freq: freq as u16,
                bias: i as u16,
            };
        }
    }
}

/// Build word rANS tables from frequency data.
pub fn build_word_tables(
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
) -> (Vec<RansWordSlot>, Vec<u8>) {
    let m = 1usize << scale_bits;
    let mut slots = alloc::vec![RansWordSlot { freq: 0, bias: 0 }; m];
    let mut slot2sym = alloc::vec![0u8; m];
    for i in 0..freqs.len().min(256) {
        let freq = freqs[i] as usize;
        if freq > 0 {
            let start = cum_freqs[i] as usize;
            rans_word_tables_init_symbol(&mut slots, &mut slot2sym, i as u8, start, freq);
        }
    }
    (slots, slot2sym)
}

// ---------------------------------------------------------------------------
// Scalar 8-way word rANS decode (reference for SIMD comparison)
// ---------------------------------------------------------------------------

/// Decode 8 symbols using 8 scalar word rANS states.
/// This is the scalar reference that SIMD decode must match exactly.
pub fn decode_8way_scalar(
    compressed: &[u16],
    tables: &RansWordTables,
    expected_len: usize,
) -> Result<Vec<u8>, &'static str> {
    if compressed.len() < 16 {
        return Err("compressed too short for 8 init states");
    }

    // Initialize 8 scalar states
    let mut states = [0u32; 8];
    for i in 0..8 {
        states[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
    }
    let mut pos = 16; // reader position in u16 words

    let n = expected_len;
    let even8 = n & !7;
    let mut output = alloc::vec![0u8; n];

    for i in (0..even8).step_by(8) {
        // Decode 8 symbols, renorm each
        for lane in 0..8 {
            let x = states[lane];
            let slot = x as usize & (RANS_WORD_M - 1);
            output[i + lane] = tables.slot2sym[slot];
            states[lane] = (tables.slots[slot].freq as u32) * (x >> (RANS_WORD_SCALE_BITS as u32))
                + tables.slots[slot].bias as u32;
        }
        // Renorm all 8 lanes in stream order
        for lane in 0..8 {
            if states[lane] < RANS_WORD_L {
                if pos >= compressed.len() {
                    return Err("unexpected EOF in scalar renorm");
                }
                states[lane] = (states[lane] << 16) | compressed[pos] as u32;
                pos += 1;
            }
        }
    }

    // Tail symbols
    for i in even8..n {
        let lane = i & 7;
        let x = states[lane];
        let slot = x as usize & (RANS_WORD_M - 1);
        output[i] = tables.slot2sym[slot];
        states[lane] = (tables.slots[slot].freq as u32) * (x >> (RANS_WORD_SCALE_BITS as u32))
            + tables.slots[slot].bias as u32;
        if states[lane] < RANS_WORD_L {
            if pos >= compressed.len() {
                return Err("unexpected EOF in tail renorm");
            }
            states[lane] = (states[lane] << 16) | compressed[pos] as u32;
            pos += 1;
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// SIMD rANS decoder — 4-lane SSE4.1 (requires SSSE3 + SSE4.1 target features)
// ---------------------------------------------------------------------------

/// SIMD decoder state: 4 × 32-bit lanes in an __m128i.
#[derive(Debug, Clone, Copy)]
pub struct RansSimdDec(pub __m128i);

/// Aligned shuffle masks for 16-byte aligned load.
#[repr(align(16))]
struct AlignedMasks([i8; 256]);

static SHUFFLE_MASKS: AlignedMasks = AlignedMasks([
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, // 0000
    0, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, // 0001
    -1, -1, -1, -1, 0, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, // 0010
    0, 1, -1, -1, 2, 3, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, // 0011
    -1, -1, -1, -1, -1, -1, -1, -1, 0, 1, -1, -1, -1, -1, -1, -1, // 0100
    0, 1, -1, -1, -1, -1, -1, -1, 2, 3, -1, -1, -1, -1, -1, -1, // 0101
    -1, -1, -1, -1, 0, 1, -1, -1, 2, 3, -1, -1, -1, -1, -1, -1, // 0110
    0, 1, -1, -1, 2, 3, -1, -1, 4, 5, -1, -1, -1, -1, -1, -1, // 0111
    -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 0, 1, -1, -1, // 1000
    0, 1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 2, 3, -1, -1, // 1001
    -1, -1, -1, -1, 0, 1, -1, -1, -1, -1, -1, -1, 2, 3, -1, -1, // 1010
    0, 1, -1, -1, 2, 3, -1, -1, -1, -1, -1, -1, 4, 5, -1, -1, // 1011
    -1, -1, -1, -1, -1, -1, -1, -1, 0, 1, -1, -1, 2, 3, -1, -1, // 1100
    0, 1, -1, -1, -1, -1, -1, -1, 2, 3, -1, -1, 4, 5, -1, -1, // 1101
    -1, -1, -1, -1, 0, 1, -1, -1, 2, 3, -1, -1, 4, 5, -1, -1, // 1110
    0, 1, -1, -1, 2, 3, -1, -1, 4, 5, -1, -1, 6, 7, -1, -1, // 1111
]);

/// Number of u16 words consumed for each 4-bit lane mask.
static NUM_WORDS: [u8; 16] = [0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4];

/// Initialize a SIMD decoder: loads 4 × 32-bit states from 8 × u16 words.
///
/// # Safety
///
/// - `reader` must have at least 8 u16 elements remaining.
/// - Caller must ensure SSE4.1 + SSSE3 are available at runtime.
#[inline]
pub unsafe fn rans_simd_dec_init(reader: &mut &[u16]) -> Option<RansSimdDec> {
    if reader.len() < 8 {
        return None;
    }
    let simd = _mm_loadu_si128(reader.as_ptr() as *const __m128i);
    *reader = &reader[8..];
    Some(RansSimdDec(simd))
}

/// Decode 4 symbols in parallel using the alias tables.
///
/// # Safety
///
/// - Requires SSE4.1 + SSSE3 target features.
/// - `tables` must have at least `RANS_WORD_M` entries.
#[inline]
pub unsafe fn rans_simd_dec_sym_unchecked(state: &mut RansSimdDec, tables: &RansWordTables) -> u32 {
    let x = state.0;

    let slots = _mm_and_si128(x, _mm_set1_epi32((RANS_WORD_M - 1) as i32));
    let i0 = _mm_cvtsi128_si32(slots) as usize;
    let i1 = _mm_extract_epi32(slots, 1) as usize;
    let i2 = _mm_extract_epi32(slots, 2) as usize;
    let i3 = _mm_extract_epi32(slots, 3) as usize;

    let s = (tables.slot2sym[i0] as u32)
        | ((tables.slot2sym[i1] as u32) << 8)
        | ((tables.slot2sym[i2] as u32) << 16)
        | ((tables.slot2sym[i3] as u32) << 24);

    let fb0 = tables.slots[i0].pack();
    let fb1 = tables.slots[i1].pack();
    let fb2 = tables.slots[i2].pack();
    let fb3 = tables.slots[i3].pack();

    let freq_bias_lo = _mm_cvtsi32_si128(fb0 as i32);
    let freq_bias_lo = _mm_insert_epi32(freq_bias_lo, fb1 as i32, 1);
    let freq_bias_hi = _mm_cvtsi32_si128(fb2 as i32);
    let freq_bias_hi = _mm_insert_epi32(freq_bias_hi, fb3 as i32, 1);
    let freq_bias = _mm_unpacklo_epi64(freq_bias_lo, freq_bias_hi);

    let xscaled = _mm_srli_epi32(x, RANS_WORD_SCALE_BITS as i32);
    let freq = _mm_and_si128(freq_bias, _mm_set1_epi32(0xffff));
    let bias = _mm_srli_epi32(freq_bias, 16);
    state.0 = _mm_add_epi32(_mm_mullo_epi32(xscaled, freq), bias);

    s
}

/// Renormalize 4 SIMD lanes using scratch buffer to avoid over-read.
///
/// # Safety
///
/// - Requires SSE4.1 + SSSE3 + SSE2 target features.
/// - `reader` must have at least `words_needed` elements (checked dynamically).
#[inline]
pub unsafe fn rans_simd_dec_renorm_unchecked(
    state: &mut RansSimdDec,
    reader: &mut &[u16],
) -> Option<()> {
    let x = state.0;

    let x_biased = _mm_xor_si128(x, _mm_set1_epi32(i32::MIN));
    let threshold = _mm_set1_epi32((RANS_WORD_L as i32).wrapping_add(i32::MIN));
    let greater = _mm_cmpgt_epi32(threshold, x_biased);
    let mask = _mm_movemask_ps(_mm_castsi128_ps(greater)) as usize;
    let words_needed = NUM_WORDS[mask] as usize;

    if words_needed == 0 {
        return Some(());
    }
    if reader.len() < words_needed {
        return None;
    }

    // Copy only needed words into a scratch buffer to avoid over-read
    let mut scratch = [0u16; 4];
    scratch[..words_needed].copy_from_slice(&reader[..words_needed]);

    let memvals = _mm_loadl_epi64(scratch.as_ptr().cast());
    let xshifted = _mm_slli_epi32(x, 16);

    let shufbase = &SHUFFLE_MASKS.0[mask * 16] as *const i8 as *const __m128i;
    let shufmask = _mm_load_si128(shufbase); // aligned load on repr(align(16)) static
    let newx = _mm_or_si128(xshifted, _mm_shuffle_epi8(memvals, shufmask));
    state.0 = _mm_blendv_epi8(x, newx, greater);

    *reader = &reader[words_needed..];
    Some(())
}

// ---------------------------------------------------------------------------
// Safe 8-way SIMD decode — caller provides feature assurance
// ---------------------------------------------------------------------------

/// Decode 8 symbols using SIMD when SSE4.1 is compiled in; scalar fallback otherwise.
pub fn decode_simd_8way(
    compressed: &[u16],
    tables: &RansWordTables,
    expected_len: usize,
) -> Result<Vec<u8>, &'static str> {
    if compressed.len() < 16 {
        return Err("compressed too short for init");
    }
    #[cfg(target_feature = "sse4.1")]
    {
        // SAFETY: compile-time target_feature gate ensures SSE4.1 availability.
        unsafe { simd_decode_inner(compressed, tables, expected_len) }
    }
    #[cfg(not(target_feature = "sse4.1"))]
    {
        decode_8way_scalar(compressed, tables, expected_len)
    }
}

/// Decode 8 symbols using SSE4.1 + SSSE3 SIMD path.
///
/// # Safety
///
/// Caller must ensure the CPU supports SSSE3 and SSE4.1 at runtime.
#[target_feature(enable = "ssse3,sse4.1")]
pub unsafe fn decode_simd_8way_unchecked(
    compressed: &[u16],
    tables: &RansWordTables,
    expected_len: usize,
) -> Result<Vec<u8>, &'static str> {
    if compressed.len() < 16 {
        return Err("compressed too short for SIMD init");
    }
    simd_decode_inner(compressed, tables, expected_len)
}

/// SSE4.1 SIMD inner decode (called from the #[target_feature]-gated wrapper).
fn simd_decode_inner(
    compressed: &[u16],
    tables: &RansWordTables,
    expected_len: usize,
) -> Result<Vec<u8>, &'static str> {
    let mut reader = compressed;

    let mut rans0 = unsafe { rans_simd_dec_init(&mut reader).ok_or("SIMD init0 failed")? };
    let mut rans1 = unsafe { rans_simd_dec_init(&mut reader).ok_or("SIMD init1 failed")? };

    let n = expected_len;
    let even8 = n & !7;
    let mut output = alloc::vec![0u8; n];

    for i in (0..even8).step_by(8) {
        let s03 = unsafe { rans_simd_dec_sym_unchecked(&mut rans0, tables) };
        let s47 = unsafe { rans_simd_dec_sym_unchecked(&mut rans1, tables) };
        output[i] = s03 as u8;
        output[i + 1] = (s03 >> 8) as u8;
        output[i + 2] = (s03 >> 16) as u8;
        output[i + 3] = (s03 >> 24) as u8;
        output[i + 4] = s47 as u8;
        output[i + 5] = (s47 >> 8) as u8;
        output[i + 6] = (s47 >> 16) as u8;
        output[i + 7] = (s47 >> 24) as u8;
        unsafe {
            rans_simd_dec_renorm_unchecked(&mut rans0, &mut reader).ok_or("SIMD renorm0 failed")?;
            rans_simd_dec_renorm_unchecked(&mut rans1, &mut reader).ok_or("SIMD renorm1 failed")?;
        }
    }

    // Tail symbols: fall back to scalar per-lane
    for i in even8..n {
        let lane_idx = i & 3;
        let which = if (i & 4) != 0 { &mut rans1 } else { &mut rans0 };
        let mut lanes: [u32; 4] = [0; 4];
        unsafe {
            _mm_storeu_si128(lanes.as_mut_ptr() as *mut __m128i, which.0);
        }
        let x = lanes[lane_idx];
        let slot = x as usize & (RANS_WORD_M - 1);
        output[i] = tables.slot2sym[slot];
        let new_x = (tables.slots[slot].freq as u32) * (x >> (RANS_WORD_SCALE_BITS as u32))
            + tables.slots[slot].bias as u32;
        lanes[lane_idx] = new_x;
        if new_x < RANS_WORD_L {
            let w = *reader.get(0).ok_or("SIMD tail EOF")? as u32;
            reader = &reader[1..];
            lanes[lane_idx] = (new_x << 16) | w;
        }
        unsafe {
            which.0 = _mm_loadu_si128(lanes.as_ptr() as *const __m128i);
        }
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Public 8-way encode helper (used by packed_table tests)
// ---------------------------------------------------------------------------

/// Encode symbols into the 8-way interleaved Word rANS format.
/// Used by tests in packed_table and court infrastructure.
pub fn encode_8way_for_test(input: &[u8], freqs: &[u32], cum: &[u32]) -> Vec<u16> {
    let mut buf = vec![0u16; input.len() * 4 + 128];
    let mut writer = buf.len();
    let mut states = [RANS_WORD_L; 8];
    for i in (0..input.len()).rev() {
        let s = input[i] as usize;
        let f = freqs[s];
        let st = cum[s];
        let idx = i & 7;
        if states[idx] >= ((RANS_WORD_L >> (RANS_WORD_SCALE_BITS as u32)) << 16) * f {
            writer -= 1;
            buf[writer] = (states[idx] & 0xffff) as u16;
            states[idx] >>= 16;
        }
        states[idx] = ((states[idx] / f) << (RANS_WORD_SCALE_BITS as u32)) + (states[idx] % f) + st;
    }
    for idx in (0..8).rev() {
        writer -= 2;
        buf[writer] = (states[idx] & 0xffff) as u16;
        buf[writer + 1] = ((states[idx] >> 16) & 0xffff) as u16;
    }
    buf[writer..].to_vec()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn encode_8way(input: &[u8], freqs: &[u32], cum: &[u32]) -> Vec<u16> {
        let mut buf = vec![0u16; 1024];
        let mut writer = buf.len();
        let mut states = [RANS_WORD_L; 8];
        for i in (0..input.len()).rev() {
            let s = input[i] as usize;
            let f = freqs[s];
            let st = cum[s];
            let idx = i & 7;
            if states[idx] >= ((RANS_WORD_L >> (RANS_WORD_SCALE_BITS as u32)) << 16) * f {
                writer -= 1;
                buf[writer] = (states[idx] & 0xffff) as u16;
                states[idx] >>= 16;
            }
            states[idx] =
                ((states[idx] / f) << (RANS_WORD_SCALE_BITS as u32)) + (states[idx] % f) + st;
        }
        for idx in (0..8).rev() {
            writer -= 2;
            buf[writer] = (states[idx] & 0xffff) as u16;
            buf[writer + 1] = ((states[idx] >> 16) & 0xffff) as u16;
        }
        buf[writer..].to_vec()
    }

    #[test]
    fn test_simd_vs_scalar_roundtrip() {
        let mut freqs = vec![0u32; 256];
        let mut cum = [0u32; 257];
        for i in 0..16 {
            freqs[i] = 256;
            cum[i + 1] = cum[i] + 256;
        }
        let (slots, slot2sym) = build_word_tables(&freqs, &cum, RANS_WORD_SCALE_BITS as u32);
        let tables = RansWordTables {
            slots: &slots,
            slot2sym: &slot2sym,
        };
        let input = [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let compressed = encode_8way(&input, &freqs, &cum);
        let scalar = decode_8way_scalar(&compressed, &tables, input.len()).unwrap();
        assert_eq!(scalar, input);
        let simd = decode_simd_8way(&compressed, &tables, input.len()).unwrap();
        assert_eq!(simd, input);
        assert_eq!(simd, scalar);
    }

    #[test]
    fn test_simd_all_lengths() {
        let mut freqs = vec![0u32; 256];
        let mut cum = [0u32; 257];
        for i in 0..16 {
            freqs[i] = 256;
            cum[i + 1] = cum[i] + 256;
        }
        let (slots, slot2sym) = build_word_tables(&freqs, &cum, RANS_WORD_SCALE_BITS as u32);
        let tables = RansWordTables {
            slots: &slots,
            slot2sym: &slot2sym,
        };
        let lengths: &[usize] = &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 63, 64, 65, 127, 128,
            129, 255, 256, 257, 1023,
        ];
        for &len in lengths {
            let input: Vec<u8> = (0..len).map(|i| (i % 16) as u8).collect();
            let compressed = encode_8way(&input, &freqs, &cum);
            let scalar = decode_8way_scalar(&compressed, &tables, len).unwrap();
            assert_eq!(scalar, input, "scalar len={}", len);
            let simd = decode_simd_8way(&compressed, &tables, len).unwrap();
            assert_eq!(simd, input, "simd len={}", len);
            assert_eq!(simd, scalar, "agree len={}", len);
        }
    }

    #[test]
    fn test_skewed_renorm() {
        let target = 1u32 << RANS_WORD_SCALE_BITS;
        let mut freqs = vec![0u32; 256];
        let mut cum = [0u32; 257];
        freqs[0] = target / 2;
        freqs[1] = target / 4;
        freqs[2] = target / 8;
        for i in 0..3 {
            cum[i + 1] = cum[i] + freqs[i];
        }
        freqs[3] = target - cum[3];
        cum[4] = target;
        let (slots, slot2sym) = build_word_tables(&freqs, &cum, RANS_WORD_SCALE_BITS as u32);
        let tables = RansWordTables {
            slots: &slots,
            slot2sym: &slot2sym,
        };
        let input: Vec<u8> = (0..256).map(|i| (i % 4) as u8).collect();
        let compressed = encode_8way(&input, &freqs, &cum);
        assert_eq!(
            decode_8way_scalar(&compressed, &tables, input.len()).unwrap(),
            input
        );
        assert_eq!(
            decode_simd_8way(&compressed, &tables, input.len()).unwrap(),
            input
        );
    }

    #[test]
    fn test_truncated_rejected() {
        let mut freqs = vec![0u32; 256];
        let mut cum = [0u32; 257];
        for i in 0..16 {
            freqs[i] = 256;
            cum[i + 1] = cum[i] + 256;
        }
        let (slots, slot2sym) = build_word_tables(&freqs, &cum, RANS_WORD_SCALE_BITS as u32);
        let tables = RansWordTables {
            slots: &slots,
            slot2sym: &slot2sym,
        };
        assert!(decode_simd_8way(&[], &tables, 8).is_err());
        assert!(decode_8way_scalar(&[], &tables, 8).is_err());
        assert!(decode_simd_8way(&[0u16; 16], &tables, 1000).is_err());
    }
}
