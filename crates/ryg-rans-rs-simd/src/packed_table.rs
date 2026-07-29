//! # Packed decode table for Word rANS
//!
//! A `u32`-packed representation of the 4096-slot Word rANS decode table,
//! optimized for AVX-512 gather operations.
//!
//! ## Layout
//!
//! Each 32-bit entry packs three fields:
//!
//! ```text
//! bits  0..11   frequency  (12 bits, max 4095)
//! bits 12..23   bias       (12 bits, max 4095)
//! bits 24..31   symbol     (8 bits)
//! ```
//!
//! ## Invariants
//!
//! - Exactly 4096 entries (one per cumulative-frequency slot).
//! - `frequency` in `0..=4095`.
//! - `bias` in `0..=4095`.
//! - `symbol` in `0..=255`.
//! - `frequency + bias` produces correct `(freq * (x >> 12) + bias)` decode.
//! - All entries are initialized (no padding or uninit slots).
//!
//! ## Construction
//!
//! `PackedWordTable::from_freqs()` validates the frequency model before
//! constructing the table, returning `Err(ModelError)` on invalid input.
//!
//! ## Equivalence
//!
//! `PackedWordTable` is provably equivalent to the existing `RansWordSlot` +
//! `slot2sym` representation: round-trip conversion tests verify every entry.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt;

use crate::{RANS_WORD_M, RANS_WORD_SCALE_BITS, RansWordSlot};

/// A single packed entry in the Word rANS decode table.
///
/// Layout: `freq | (bias << 12) | ((symbol as u32) << 24)`.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackedWordEntry(pub u32);

impl PackedWordEntry {
    /// Pack fields into a single `u32`.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if any field exceeds its bit-width.
    #[inline]
    pub fn pack(freq: u16, bias: u16, symbol: u8) -> Self {
        debug_assert!((freq as u32) < 4096, "freq {} exceeds 12 bits", freq);
        debug_assert!((bias as u32) < 4096, "bias {} exceeds 12 bits", bias);
        let f = (freq as u32) & 0x0fff;
        let b = ((bias as u32) & 0x0fff) << 12;
        let s = (symbol as u32) << 24;
        PackedWordEntry(f | b | s)
    }

    /// Extract the frequency (bits 0..11).
    #[inline]
    pub fn freq(&self) -> u32 {
        self.0 & 0x0fff
    }

    /// Extract the bias (bits 12..23).
    #[inline]
    pub fn bias(&self) -> u32 {
        (self.0 >> 12) & 0x0fff
    }

    /// Extract the symbol (bits 24..31).
    #[inline]
    pub fn symbol(&self) -> u8 {
        (self.0 >> 24) as u8
    }

    /// Convert from an existing `RansWordSlot` + symbol pair.
    #[inline]
    pub fn from_slot(slot: &RansWordSlot, symbol: u8) -> Self {
        Self::pack(slot.freq, slot.bias, symbol)
    }
}

/// 4096-slot packed Word rANS decode table, heap-allocated with explicit alignment.
///
/// Alignment of 64 bytes (cache-line) to benefit gathers.
#[repr(align(64))]
#[derive(Clone, Debug)]
pub struct PackedWordTable {
    /// 4096 packed entries, indexed by `cumulative_freq & 0xfff`.
    entries: Box<[PackedWordEntry; RANS_WORD_M]>,
}

impl PackedWordTable {
    /// Construct a packed table from frequency and cumulative-frequency arrays.
    ///
    /// Validates:
    /// - `scale_bits` must equal `RANS_WORD_SCALE_BITS` (12).
    /// - No frequency exceeds 4095.
    /// - Cumulative frequencies are monotonically non-decreasing.
    /// - The sum of frequencies equals `1 << scale_bits`.
    //
    /// Returns `Err(ModelError)` on any invariant violation.
    pub fn from_freqs(
        freqs: &[u32],
        cum_freqs: &[u32],
        scale_bits: u32,
    ) -> Result<Self, ryg_rans_rs_core::ModelError> {
        use ryg_rans_rs_core::ModelError;

        if scale_bits != RANS_WORD_SCALE_BITS as u32 {
            return Err(ModelError::InvalidScaleBits);
        }

        let m = 1usize << scale_bits;
        if m != RANS_WORD_M {
            return Err(ModelError::InvalidScaleBits);
        }

        // Validate non-decreasing cumulative.
        let n = cum_freqs.len().min(freqs.len() + 1).min(257);
        for i in 1..n {
            if cum_freqs[i] < cum_freqs[i - 1] {
                return Err(ModelError::TotalMismatch);
            }
        }

        // Check sum equals total.
        let total = cum_freqs.get(n.saturating_sub(1)).copied().unwrap_or(0);
        if total != (1u32 << scale_bits) {
            return Err(ModelError::TotalMismatch);
        }

        // Build packed entries.
        let mut entries = Vec::with_capacity(m);
        let nsyms = freqs.len().min(256);
        for slot_idx in 0..m {
            // Find which symbol owns this slot.
            let mut sym = 0u8;
            let mut freq = 0u16;
            let mut bias = 0u16;
            for sym_idx in 0..nsyms {
                let start = cum_freqs[sym_idx] as usize;
                let end = cum_freqs[sym_idx + 1] as usize;
                if slot_idx >= start && slot_idx < end {
                    sym = sym_idx as u8;
                    freq = (end - start) as u16;
                    bias = (slot_idx - start) as u16;
                    break;
                }
            }
            if freq == 0 {
                // Unused slot — should not happen for valid models, but handle.
                return Err(ModelError::ZeroFrequency);
            }
            if (freq as u32) >= 4096 || (bias as u32) >= 4096 {
                return Err(ModelError::FrequencyOutOfRange);
            }
            entries.push(PackedWordEntry::pack(freq, bias, sym));
        }

        let boxed_slice: Box<[PackedWordEntry]> = entries.into_boxed_slice();
        let array: Box<[PackedWordEntry; RANS_WORD_M]> = boxed_slice
            .try_into()
            .map_err(|_| ModelError::WorkspaceTooSmall)?;

        Ok(PackedWordTable { entries: array })
    }

    /// Access the packed entries as a slice.
    #[inline]
    pub fn as_slice(&self) -> &[PackedWordEntry] {
        &self.entries[..]
    }

    /// Access the raw pointer for gather operations.
    #[inline]
    pub fn as_ptr(&self) -> *const PackedWordEntry {
        self.entries.as_ptr()
    }

    /// Access a single entry by slot index.
    #[inline]
    pub fn get(&self, slot: usize) -> &PackedWordEntry {
        debug_assert!(slot < RANS_WORD_M);
        &self.entries[slot]
    }

    /// Number of entries (always 4096).
    #[inline]
    pub fn len(&self) -> usize {
        RANS_WORD_M
    }

    /// Verify equivalence with the existing `RansWordSlot` + `slot2sym` representation.
    ///
    /// Returns `Ok(())` if every entry matches, otherwise returns the first
    /// mismatching slot index and details.
    pub fn verify_equivalence(
        &self,
        slots: &[RansWordSlot],
        slot2sym: &[u8],
    ) -> Result<(), EquivalenceError> {
        let n = self.entries.len().min(slots.len()).min(slot2sym.len());
        for i in 0..n {
            let expected = PackedWordEntry::from_slot(&slots[i], slot2sym[i]);
            if self.entries[i] != expected {
                return Err(EquivalenceError {
                    slot: i,
                    expected,
                    actual: self.entries[i],
                });
            }
        }
        Ok(())
    }
}

/// Error returned when packed-table equivalence verification fails.
#[derive(Clone, Debug)]
pub struct EquivalenceError {
    pub slot: usize,
    pub expected: PackedWordEntry,
    pub actual: PackedWordEntry,
}

impl fmt::Display for EquivalenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "packed table mismatch at slot {}: expected {:08x}, got {:08x}",
            self.slot, self.expected.0, self.actual.0
        )
    }
}

// ---------------------------------------------------------------------------
// Scalar eight-way decode using packed table (verification reference)
// ---------------------------------------------------------------------------

/// 8-way decode report with real words consumed and final states.
#[derive(Debug, Clone, Copy)]
pub struct DecodeReport8 {
    /// Total u16 words consumed from the compressed stream.
    pub words_consumed: usize,
    /// Final state of each of the 8 lanes after decode completes.
    pub final_states: [u32; 8],
}

/// Decode 8 symbols using packed table (scalar reference).
///
/// Returns output bytes only.  See `decode_8way_packed_scalar_with_report`
/// for a version that also returns words consumed and final states.
pub fn decode_8way_packed_scalar(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<Vec<u8>, &'static str> {
    decode_8way_packed_scalar_with_report(compressed, table, expected_len).map(|(out, _)| out)
}

/// Decode 8 symbols using packed table (scalar reference) with report.
///
/// Returns decoded output plus a `DecodeReport8` containing actual words
/// consumed from the input stream and the final state of each lane.
pub fn decode_8way_packed_scalar_with_report(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport8), &'static str> {
    if compressed.len() < 16 {
        return Err("compressed too short for 8 init states");
    }
    let mut states = [0u32; 8];
    for i in 0..8 {
        states[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
    }
    let mut pos = 16;
    let n = expected_len;
    let even8 = n & !7;
    let mut output = alloc::vec![0u8; n];

    for i in (0..even8).step_by(8) {
        for lane in 0..8 {
            let x = states[lane];
            let slot = x as usize & (RANS_WORD_M - 1);
            let entry = table.get(slot);
            output[i + lane] = entry.symbol();
            states[lane] = entry.freq() * (x >> (RANS_WORD_SCALE_BITS as u32)) + entry.bias();
        }
        for lane in 0..8 {
            if states[lane] < crate::RANS_WORD_L {
                if pos >= compressed.len() {
                    return Err("unexpected EOF in packed scalar renorm");
                }
                states[lane] = (states[lane] << 16) | compressed[pos] as u32;
                pos += 1;
            }
        }
    }

    for i in even8..n {
        let lane = i & 7;
        let x = states[lane];
        let slot = x as usize & (RANS_WORD_M - 1);
        let entry = table.get(slot);
        output[i] = entry.symbol();
        states[lane] = entry.freq() * (x >> (RANS_WORD_SCALE_BITS as u32)) + entry.bias();
        if states[lane] < crate::RANS_WORD_L {
            if pos >= compressed.len() {
                return Err("unexpected EOF in packed tail renorm");
            }
            states[lane] = (states[lane] << 16) | compressed[pos] as u32;
            pos += 1;
        }
    }

    Ok((
        output,
        DecodeReport8 {
            words_consumed: pos,
            final_states: states,
        },
    ))
}

// ---------------------------------------------------------------------------
// Scalar sixteen-way encoder/decoder (new format)
// ---------------------------------------------------------------------------

/// Decode report containing final states and consumption count.
#[derive(Debug, Clone, Copy)]
pub struct DecodeReport {
    pub words_consumed: usize,
    pub final_states: [u32; 16],
}

/// Encode symbols into the 16-way interleaved Word rANS format.
///
/// The encoder processes symbols in **reverse** order (last symbol first),
/// assigning each to lane `i & 15`. States are flushed in **reverse lane
/// order** (15, 14, ..., 0) so that the forward stream initializes lanes
/// in ascending order (0, 1, ..., 15).
pub fn encode_interleaved16(
    symbols: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
) -> Vec<u16> {
    assert_eq!(scale_bits, RANS_WORD_SCALE_BITS as u32);
    let total = 1u32 << scale_bits;
    let capacity = symbols.len() * 2 + 64;
    let mut buf = vec![0u16; capacity];
    let mut writer = capacity; // backward writer

    // Initialize 16 states to L
    let mut states = [crate::RANS_WORD_L; 16];

    // Encode in reverse order
    for i in (0..symbols.len()).rev() {
        let s = symbols[i] as usize;
        let f = freqs[s];
        let st = cum_freqs[s];
        let lane = i & 15;
        // Renorm check: if state >= ((L >> scale_bits) << 16) * freq
        let threshold = ((crate::RANS_WORD_L >> scale_bits) << 16) * f;
        if states[lane] >= threshold {
            writer -= 1;
            buf[writer] = (states[lane] & 0xffff) as u16;
            states[lane] >>= 16;
        }
        states[lane] = ((states[lane] / f) << scale_bits) + (states[lane] % f) + st;
    }

    // Flush states in REVERSE lane order (15 down to 0)
    for idx in (0..16).rev() {
        writer -= 2;
        buf[writer] = (states[idx] & 0xffff) as u16;
        buf[writer + 1] = ((states[idx] >> 16) & 0xffff) as u16;
    }

    buf[writer..].to_vec()
}

/// Scalar sixteen-way decoder (safe reference).
///
/// Decodes symbols from a 16-way interleaved Word rANS stream.
/// Returns the decoded output and a `DecodeReport` with final states
/// and word consumption count.
pub fn decode_interleaved16_scalar(
    compressed: &[u16],
    table: &PackedWordTable,
    expected_len: usize,
) -> Result<(Vec<u8>, DecodeReport), &'static str> {
    if compressed.len() < 32 {
        return Err("compressed too short for 16 init states");
    }

    // Initialize 16 states from the first 32 u16 words (16 * 2).
    let mut states = [0u32; 16];
    for i in 0..16 {
        states[i] = compressed[i * 2] as u32 | (compressed[i * 2 + 1] as u32) << 16;
    }
    let mut pos = 32; // reader position in u16 words

    let n = expected_len;
    let even16 = n & !15;
    let mut output = alloc::vec![0u8; n];

    // Process complete groups of 16
    for i in (0..even16).step_by(16) {
        // Decode all 16 lanes
        for lane in 0..16 {
            let x = states[lane];
            let slot = x as usize & (RANS_WORD_M - 1);
            let entry = table.get(slot);
            output[i + lane] = entry.symbol();
            states[lane] = entry.freq() * (x >> (RANS_WORD_SCALE_BITS as u32)) + entry.bias();
        }
        // Renorm all 16 lanes in ascending lane order
        for lane in 0..16 {
            if states[lane] < crate::RANS_WORD_L {
                if pos >= compressed.len() {
                    return Err("unexpected EOF in 16-way renorm");
                }
                states[lane] = (states[lane] << 16) | compressed[pos] as u32;
                pos += 1;
            }
        }
    }

    // Tail symbols (partial group)
    for i in even16..n {
        let lane = i & 15;
        let x = states[lane];
        let slot = x as usize & (RANS_WORD_M - 1);
        let entry = table.get(slot);
        output[i] = entry.symbol();
        states[lane] = entry.freq() * (x >> (RANS_WORD_SCALE_BITS as u32)) + entry.bias();
        if states[lane] < crate::RANS_WORD_L {
            if pos >= compressed.len() {
                return Err("unexpected EOF in 16-way tail renorm");
            }
            states[lane] = (states[lane] << 16) | compressed[pos] as u32;
            pos += 1;
        }
    }

    let report = DecodeReport {
        words_consumed: pos,
        final_states: states,
    };

    Ok((output, report))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_word_tables;
    use alloc::vec;

    fn uniform256() -> (Vec<u32>, Vec<u32>) {
        let total = 1u32 << 12;
        let base = total / 256;
        let mut freqs = vec![base; 256];
        freqs[255] += total - freqs.iter().sum::<u32>();
        let mut cum = vec![0u32; 257];
        for i in 0..256 {
            cum[i + 1] = cum[i] + freqs[i];
        }
        (freqs, cum)
    }

    #[test]
    fn test_packed_table_equivalence() {
        let (freqs, cum) = uniform256();
        let (slots, slot2sym) = build_word_tables(&freqs, &cum, 12);
        let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
        assert!(packed.verify_equivalence(&slots, &slot2sym).is_ok());
    }

    #[test]
    fn test_packed_table_field_extraction() {
        let entry = PackedWordEntry::pack(100, 200, 42);
        assert_eq!(entry.freq(), 100);
        assert_eq!(entry.bias(), 200);
        assert_eq!(entry.symbol(), 42);

        // Max values
        let entry = PackedWordEntry::pack(4095, 4095, 255);
        assert_eq!(entry.freq(), 4095);
        assert_eq!(entry.bias(), 4095);
        assert_eq!(entry.symbol(), 255);
    }

    #[test]
    fn test_packed_table_invalid_scale_bits() {
        let (freqs, cum) = uniform256();
        assert!(PackedWordTable::from_freqs(&freqs, &cum, 13).is_err());
    }

    #[test]
    fn test_packed_scalar_matches_existing_scalar() {
        let (freqs, cum) = uniform256();
        let (slots, slot2sym) = build_word_tables(&freqs, &cum, 12);
        let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();

        // Encode some data with the existing 8-way encoder
        let symbols: Vec<u8> = (0..100).map(|i| (i % 16) as u8).collect();
        let compressed = crate::encode_8way_for_test(&symbols, &freqs, &cum);

        // Decode with existing scalar
        let tables = crate::RansWordTables {
            slots: &slots,
            slot2sym: &slot2sym,
        };
        let existing = crate::decode_8way_scalar(&compressed, &tables, symbols.len()).unwrap();

        // Decode with packed scalar
        let packed_result = decode_8way_packed_scalar(&compressed, &packed, symbols.len()).unwrap();

        assert_eq!(existing, symbols, "existing scalar must roundtrip");
        assert_eq!(packed_result, symbols, "packed scalar must roundtrip");
        assert_eq!(packed_result, existing, "packed must match existing scalar");
    }

    #[test]
    fn test_interleaved16_roundtrip() {
        let (freqs, cum) = uniform256();
        let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();

        let lengths: &[usize] = &[
            0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 31, 32, 33, 63, 64, 65,
            127, 128, 129, 255, 256, 257, 511, 512, 513, 1023, 1024, 1025,
        ];
        for &len in lengths {
            let symbols: Vec<u8> = (0..len).map(|i| (i % 16) as u8).collect();
            if symbols.is_empty() {
                continue;
            }
            let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);
            let (decoded, report) =
                decode_interleaved16_scalar(&compressed, &packed, symbols.len()).unwrap();
            assert_eq!(decoded, symbols, "16-way roundtrip failed for len={}", len);
            // Verify words consumed
            assert!(
                report.words_consumed <= compressed.len(),
                "words consumed {} > compressed len {}",
                report.words_consumed,
                compressed.len()
            );
        }
    }

    #[test]
    fn test_interleaved16_tail_lengths() {
        let (freqs, cum) = uniform256();
        let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();

        // Test every possible tail length 0..16
        for tail in 0..16 {
            let len = 32 + tail; // two full groups plus tail
            let symbols: Vec<u8> = (0..len).map(|i| (i % 16) as u8).collect();
            let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);
            let (decoded, _report) =
                decode_interleaved16_scalar(&compressed, &packed, symbols.len()).unwrap();
            assert_eq!(decoded, symbols, "16-way tail={} roundtrip failed", tail);
        }
    }

    #[test]
    fn test_interleaved16_truncated_rejected() {
        let (freqs, cum) = uniform256();
        let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();

        // Empty input
        assert!(decode_interleaved16_scalar(&[], &packed, 16).is_err());

        // Too few init words (31 instead of 32)
        let short = vec![0u16; 31];
        assert!(decode_interleaved16_scalar(&short, &packed, 16).is_err());

        // Init words exist but decode runs out of renorm data
        let symbols: Vec<u8> = (0..50).map(|i| (i % 16) as u8).collect();
        let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);
        // Truncate by removing some renorm words
        let truncated = &compressed[..compressed.len().saturating_sub(10)];
        let result = decode_interleaved16_scalar(truncated, &packed, symbols.len());
        // Expect an error (truncated stream)
        assert!(result.is_err(), "truncated 16-way stream should fail");
    }

    #[test]
    fn test_state_lane_ordering() {
        // Verify that the stream layout is correct by inspecting the first
        // 32 u16 words (the 16 initial states).
        let (freqs, cum) = uniform256();
        let symbols: Vec<u8> = (0..32).map(|i| (i % 16) as u8).collect();
        let compressed = encode_interleaved16(&symbols, &freqs, &cum, 12);

        // The first 32 words should be the initial states in lane order 0..15.
        // Each state is [low16, high16].
        assert!(
            compressed.len() >= 32,
            "compressed must have at least 32 words"
        );
        // We can verify structural ordering: the states must be in ascending lane order.
        // Read state 0:
        let s0_lo = compressed[0] as u32;
        let s0_hi = compressed[1] as u32;
        let state0 = s0_lo | (s0_hi << 16);
        // Read state 15:
        let s15_lo = compressed[30] as u32;
        let s15_hi = compressed[31] as u32;
        let state15 = s15_lo | (s15_hi << 16);

        // All initial states must be >= L.
        assert!(state0 >= crate::RANS_WORD_L, "state0 must be >= L");
        assert!(state15 >= crate::RANS_WORD_L, "state15 must be >= L");

        // Decode and verify full roundtrip
        let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
        let (decoded, _) =
            decode_interleaved16_scalar(&compressed, &packed, symbols.len()).unwrap();
        assert_eq!(decoded, symbols, "16-way roundtrip with state verification");
    }
}
