//! Deterministic benchmark corpora.
//!
//! Every corpus is reproducible from a seed + profile.  The same seed
//! always produces identical bytes, frequencies, and compressed streams.

use std::vec::Vec;

/// Model profile identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelProfile {
    Uniform256,
    Freq1Residual,
    Skewed2551,
    Sparse2,
    Sparse17,
    PrimeResidue,
    RenormBoundary,
    IncompressibleLike,
}

impl ModelProfile {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Uniform256 => "UNIFORM256",
            Self::Freq1Residual => "FREQ1_RESIDUAL",
            Self::Skewed2551 => "SKEWED_255_1",
            Self::Sparse2 => "SPARSE_2",
            Self::Sparse17 => "SPARSE_17",
            Self::PrimeResidue => "PRIME_RESIDUE",
            Self::RenormBoundary => "RENORM_BOUNDARY",
            Self::IncompressibleLike => "INCOMPRESSIBLE_LIKE",
        }
    }
}

/// A deterministic corpus with known frequencies and cumulative model.
pub struct Corpus {
    pub profile: ModelProfile,
    pub seed: u64,
    pub data: Vec<u8>,
    pub freqs: Vec<u32>,
    pub cum_freqs: Vec<u32>,
    pub scale_bits: u32,
    pub total: u32,
}

impl Corpus {
    /// Generate a corpus of exactly `length` bytes for the given profile and seed.
    pub fn generate(profile: ModelProfile, length: usize, seed: u64) -> Self {
        let scale_bits = 12;
        let total = 1u32 << scale_bits;

        let data = generate_data(profile, length, seed);
        let (freqs, cum_freqs) = build_normalized_model(&data, total);

        Corpus {
            profile,
            seed,
            data,
            freqs,
            cum_freqs,
            scale_bits,
            total,
        }
    }

    /// Generate a corpus and encode it into 16-way interleaved Word rANS format.
    pub fn encode_16way(&self) -> Vec<u16> {
        ryg_rans_rs_simd::packed_table::encode_interleaved16(
            &self.data,
            &self.freqs,
            &self.cum_freqs,
            self.scale_bits,
        )
        .expect("encode_interleaved16")
    }

    /// Build a packed decode table for this corpus.
    pub fn packed_table(&self) -> ryg_rans_rs_simd::packed_table::PackedWordTable {
        ryg_rans_rs_simd::packed_table::PackedWordTable::from_freqs(
            &self.freqs,
            &self.cum_freqs,
            self.scale_bits,
        )
        .expect("PackedWordTable")
    }
}

/// Generate raw byte data for the given profile.
fn generate_data(profile: ModelProfile, length: usize, seed: u64) -> Vec<u8> {
    use rand::rngs::StdRng;
    use rand::{Rng, RngCore, SeedableRng};
    let mut rng = StdRng::seed_from_u64(seed);
    // Helper to get a random byte from the seeded RNG.
    fn rng_u8(rng: &mut StdRng) -> u8 {
        (RngCore::next_u32(rng) >> 24) as u8
    }

    match profile {
        ModelProfile::Uniform256 => {
            let mut data = Vec::with_capacity(length);
            while data.len() < length {
                for s in 0u8..=255 {
                    for _ in 0..16 {
                        if data.len() < length {
                            data.push(s);
                        }
                    }
                }
            }
            data
        }
        ModelProfile::Freq1Residual => {
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                if rng.gen_ratio(1, 1000) {
                    data.push(rng_u8(&mut rng));
                } else {
                    data.push(0);
                }
            }
            data
        }
        ModelProfile::Skewed2551 => {
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                if rng.gen_ratio(255, 256) {
                    data.push(0);
                } else {
                    data.push(rng.gen_range(1u8..=255));
                }
            }
            data
        }
        ModelProfile::Sparse2 => {
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                data.push(if rng.gen_ratio(1, 2) { 0 } else { 1 });
            }
            data
        }
        ModelProfile::Sparse17 => {
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                data.push((RngCore::next_u32(&mut rng) % 17) as u8);
            }
            data
        }
        ModelProfile::PrimeResidue => {
            let p = 257u16;
            let mut x = (seed % 65521) as u16;
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                x = (x.wrapping_mul(251)) % p;
                data.push((x & 0xFF) as u8);
            }
            data
        }
        ModelProfile::RenormBoundary => {
            let mut data = Vec::with_capacity(length);
            for i in 0..length {
                if (i / 16) % 2 == 0 {
                    data.push(0);
                } else {
                    data.push(255);
                }
            }
            data
        }
        ModelProfile::IncompressibleLike => {
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                data.push(rng_u8(&mut rng));
            }
            data
        }
    }
}

/// Build normalized frequency model from data.
///
/// Guarantees that all frequencies sum exactly to `total` and that no
/// individual frequency exceeds 4095 (12-bit packed representation limit).
fn build_normalized_model(data: &[u8], total: u32) -> (Vec<u32>, Vec<u32>) {
    let mut freqs = vec![0u32; 256];
    for &b in data {
        freqs[b as usize] += 1;
    }
    let sum: u64 = freqs.iter().map(|&f| f as u64).sum();
    if sum == 0 {
        let uniform = total / 256;
        for f in &mut freqs {
            *f = uniform;
        }
    } else {
        // Scale frequencies proportionally, keeping each <= 4095.
        // Ensure every symbol that appeared in the data gets at least 1.
        let nonzero_count = freqs.iter().filter(|&&f| f > 0).count() as u64;
        let reserved = nonzero_count; // reserve 1 per observed symbol
        let mut allocated = 0u64;
        if reserved <= total as u64 {
            let available = total as u64 - reserved;
            let raw_clone = freqs.clone();
            for (raw, f) in raw_clone.iter().zip(freqs.iter_mut()) {
                if *raw > 0 {
                    let scaled = 1 + ((*raw as u64 * available) / sum).min(4094);
                    *f = scaled as u32;
                    allocated += *f as u64;
                } else {
                    *f = 0;
                }
            }
        } else {
            // More symbols than total — assign 1 each round-robin
            let mut remaining = total;
            for f in freqs.iter_mut().filter(|f| **f > 0) {
                if remaining > 0 {
                    *f = 1;
                    remaining -= 1;
                }
            }
            allocated = (total - remaining) as u64;
        }
        // Distribute remainder to largest frequencies that aren't already
        // at max, until we reach exactly total.  Cap at 4095 to keep within
        // the packed-table 12-bit field.
        let mut remaining = (total as u64).saturating_sub(allocated);
        while remaining > 0 {
            // Find the largest frequency that is < 4095
            let mut best_idx = 0usize;
            let mut best_val = 0u32;
            for i in 0..256 {
                if freqs[i] < 4095 && freqs[i] > best_val {
                    best_val = freqs[i];
                    best_idx = i;
                }
            }
            if best_val == 0 {
                // All frequencies at 0 or 4095; give to symbol 0 and symbol 1
                // (at least two symbols must share total=4096 with each <= 4095)
                if freqs[0] < 4095 {
                    let add = remaining.min((4095 - freqs[0]) as u64);
                    freqs[0] += add as u32;
                    remaining -= add;
                } else {
                    // Symbol 0 is at 4095; give to symbol 1
                    let add = remaining.min(1);
                    freqs[1] += add as u32;
                    remaining -= add;
                }
                continue;
            }
            let add = 1u64;
            freqs[best_idx] += add as u32;
            remaining -= add;
        }
    }
    let mut cum = vec![0u32; 257];
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }
    debug_assert_eq!(cum[256], total, "Normalized frequencies must sum to total");
    debug_assert!(
        freqs.iter().all(|&f| f <= 4095),
        "No frequency may exceed 4095"
    );
    for i in 0..256 {
        debug_assert!(
            freqs[i] as u64 + cum[i] as u64 <= cum[i + 1] as u64,
            "Cumulative frequencies must be non-decreasing"
        );
    }
    (freqs, cum)
}
