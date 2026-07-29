//! # Canonical model encoding and normalization
//!
//! Implements the normative deterministic model normalization algorithm.
//! All arithmetic is integer-only.  Results are platform-independent.

use crate::error::{AppError, FormatError};
use sha2::{Digest, Sha256};

/// A canonical sparse frequency model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrequencyModel {
    /// Normalized frequencies, indexed by symbol.
    pub frequencies: Vec<u32>,
    /// Cumulative frequencies (length 257).
    pub cumulative: Vec<u32>,
    /// Scale bits used.
    pub scale_bits: u8,
    /// Number of active (non-zero) symbols.
    pub active_symbols: usize,
}

impl FrequencyModel {
    /// Build a normalized model from a raw histogram.
    ///
    /// Algorithm:
    /// 1. Count occurrences per symbol (0..255).
    /// 2. Compute quota: `floor(count[sym] * total / total_count)`.
    /// 3. Enforce minimum 1 per observed symbol.
    /// 4. Distribute remainder in descending remainder order.
    /// 5. If over target due to minimum-1 enforcement, remove from
    ///    symbols with smallest remainder, never going below 1.
    ///
    /// This is fully deterministic — no floating point, no platform dependency.
    pub fn build(histogram: &[u64; 256], scale_bits: u8) -> Result<Self, AppError> {
        let target_total = 1u64 << scale_bits;

        // Count total observed symbols
        let total_count: u64 = histogram.iter().sum();
        if total_count == 0 {
            return Err(AppError::Format(FormatError {
                detail: "empty histogram: no data to build model".into(),
                block_index: None,
                offset: None,
            }));
        }

        let mut frequencies = [0u64; 256];
        let mut remainders = [0u64; 256];
        let mut active_count = 0usize;

        // Phase 1: compute base frequency and remainder for each symbol
        for (sym, &count) in histogram.iter().enumerate() {
            if count == 0 {
                continue;
            }
            let quota_num = count * target_total;
            let base = quota_num / total_count;
            let rem = quota_num % total_count;

            frequencies[sym] = base.max(1); // minimum 1
            remainders[sym] = rem;
            active_count += 1;
        }

        // Phase 2: compute current sum and adjust
        let mut current_sum: u64 = frequencies.iter().sum();

        // If below target, add units in descending remainder order
        if current_sum < target_total {
            // Collect symbols with non-zero remainders, sorted by remainder descending
            let mut candidates: Vec<(u64, usize)> = remainders
                .iter()
                .enumerate()
                .filter(|&(sym_idx, &r)| r > 0 && frequencies[sym_idx] > 0)
                .map(|(sym, &rem)| (rem, sym))
                .collect();
            // Sort by remainder descending, then by original count descending, then by symbol
            candidates.sort_by(|a, b| {
                b.0.cmp(&a.0) // remainder descending
                    .then_with(|| histogram[a.1].cmp(&histogram[b.1]).reverse()) // count descending
                    .then_with(|| a.1.cmp(&b.1)) // symbol ascending
            });

            for (_, sym) in candidates.iter().cycle() {
                if current_sum >= target_total {
                    break;
                }
                frequencies[*sym] += 1;
                current_sum += 1;
            }
        }

        // If above target (due to minimum-1 enforcement), remove units
        if current_sum > target_total {
            let excess = (current_sum - target_total) as usize;

            // Collect symbols eligible for reduction (frequency > 1)
            let mut candidates: Vec<(u64, usize)> = (0..256)
                .filter(|&sym| frequencies[sym] > 1)
                .map(|sym| (remainders[sym], sym))
                .collect();
            // Sort by remainder ascending, then by frequency descending, then by symbol descending
            candidates.sort_by(|a, b| {
                a.0.cmp(&b.0) // remainder ascending
                    .then_with(|| b.1.cmp(&a.1)) // frequency descending
                    .then_with(|| b.1.cmp(&a.1)) // symbol descending
            });

            for (_, sym) in candidates.iter() {
                if excess == 0 {
                    break;
                }
                if frequencies[*sym] > 1 {
                    frequencies[*sym] -= 1;
                }
            }
        }

        // Ensure exact match
        debug_assert_eq!(frequencies.iter().sum::<u64>(), target_total);

        // Convert to u32 (safe: target_total <= u32::MAX for scale_bits <= 31)
        let freq_u32: Vec<u32> = frequencies.iter().map(|&f| f as u32).collect();

        // Build cumulative
        let mut cum = Vec::with_capacity(257);
        let mut acc = 0u32;
        cum.push(0);
        for &f in freq_u32.iter() {
            acc = acc.checked_add(f).unwrap();
            cum.push(acc);
        }

        Ok(Self {
            frequencies: freq_u32,
            cumulative: cum,
            scale_bits,
            active_symbols: active_count,
        })
    }

    /// Serialize to canonical binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let entries: Vec<(u8, u32)> = (0..=255u16)
            .filter(|&s| self.frequencies[s as usize] > 0)
            .map(|s| (s as u8, self.frequencies[s as usize]))
            .collect();

        let mut buf = Vec::with_capacity(2 + entries.len() * 5);
        buf.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        for (sym, freq) in &entries {
            buf.push(*sym);
            buf.extend_from_slice(&freq.to_le_bytes());
        }
        buf
    }

    /// Parse from canonical binary format.
    pub fn from_bytes(bytes: &[u8], scale_bits: u8) -> Result<Self, AppError> {
        if bytes.len() < 2 {
            return Err(AppError::Format(FormatError {
                detail: "model too short".into(),
                block_index: None,
                offset: None,
            }));
        }

        let entry_count = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        let expected_len = 2 + entry_count * 5;
        if bytes.len() < expected_len {
            return Err(AppError::Format(FormatError {
                detail: format!(
                    "model truncated: expected {} bytes, got {}",
                    expected_len,
                    bytes.len()
                ),
                block_index: None,
                offset: None,
            }));
        }

        let mut frequencies = vec![0u32; 256];
        let mut prev_sym: Option<u8> = None;

        for i in 0..entry_count {
            let offset = 2 + i * 5;
            let sym = bytes[offset];
            let freq = u32::from_le_bytes([
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
                bytes[offset + 4],
            ]);

            // Check ascending order
            if let Some(prev) = prev_sym {
                if sym <= prev {
                    return Err(AppError::Format(FormatError {
                        detail: format!("duplicate or non-ascending symbol {} after {}", sym, prev),
                        block_index: None,
                        offset: None,
                    }));
                }
            }

            if freq == 0 {
                return Err(AppError::Format(FormatError {
                    detail: format!("zero frequency for symbol {}", sym),
                    block_index: None,
                    offset: None,
                }));
            }

            frequencies[sym as usize] = freq;
            prev_sym = Some(sym);
        }

        // Verify sum
        let total = 1u64 << scale_bits;
        let sum: u64 = frequencies.iter().map(|&f| f as u64).sum();
        if sum != total {
            return Err(AppError::Format(FormatError {
                detail: format!(
                    "model frequency sum {} does not match target {}",
                    sum, total
                ),
                block_index: None,
                offset: None,
            }));
        }

        // Build cumulative
        let mut cum = Vec::with_capacity(257);
        let mut acc = 0u32;
        cum.push(0);
        for &f in frequencies.iter() {
            acc = acc.checked_add(f).unwrap();
            cum.push(acc);
        }

        let active = frequencies.iter().filter(|&&f| f > 0).count();

        Ok(Self {
            frequencies,
            cumulative: cum,
            scale_bits,
            active_symbols: active,
        })
    }

    /// Compute SHA-256 of the canonical serialization.
    pub fn sha256(&self) -> [u8; 32] {
        let bytes = self.to_bytes();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Build a uniform model: all 256 symbols get equal (or nearly equal) frequency.
    pub fn build_uniform(scale_bits: u8) -> Self {
        let total = 1u64 << scale_bits;
        let base = total / 256;
        let mut frequencies = vec![base as u32; 256];
        let remainder = total - base * 256;
        if remainder > 0 {
            frequencies[255] += remainder as u32;
        }

        let mut cum = Vec::with_capacity(257);
        let mut acc = 0u32;
        cum.push(0);
        for &f in frequencies.iter() {
            acc = acc.checked_add(f).unwrap();
            cum.push(acc);
        }

        Self {
            frequencies,
            cumulative: cum,
            scale_bits,
            active_symbols: 256,
        }
    }
}

/// Compute a histogram (count of each byte value).
pub fn compute_histogram(data: &[u8]) -> [u64; 256] {
    let mut hist = [0u64; 256];
    for &b in data {
        hist[b as usize] += 1;
    }
    hist
}
