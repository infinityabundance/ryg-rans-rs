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

    /// Generate SHA-256 of the original data.
    pub fn data_hash(&self) -> [u8; 32] {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&self.data);
        let r = h.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&r);
        out
    }
}

/// Generate raw byte data for the given profile.
fn generate_data(profile: ModelProfile, length: usize, seed: u64) -> Vec<u8> {
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};
    let mut rng = StdRng::seed_from_u64(seed);

    match profile {
        ModelProfile::Uniform256 => {
            // Every symbol appears exactly 16 times per 4096-symbol block.
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
            // Mostly symbol 0, with rare symbols scattered in.
            let mut data = Vec::with_capacity(length);
            for i in 0..length {
                if rng.random_ratio(1, 1000) {
                    data.push(rng.random::<u8>());
                } else {
                    data.push(0);
                }
            }
            data
        }
        ModelProfile::Skewed2551 => {
            // Symbol 0 appears 255 times more often than any other.
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                if rng.random_ratio(255, 256) {
                    data.push(0);
                } else {
                    data.push(rng.random_range(1u8..=255));
                }
            }
            data
        }
        ModelProfile::Sparse2 => {
            // Only 2 symbols used.
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                data.push(if rng.random_bool(0.5) { 0 } else { 1 });
            }
            data
        }
        ModelProfile::Sparse17 => {
            // Only 17 symbols used.
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                data.push(rng.random_range(0u8..17));
            }
            data
        }
        ModelProfile::PrimeResidue => {
            // Deterministic non-uniform: multiplication modulo a prime.
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
            // Generates data likely to trigger renormalization.
            // Alternate between high-frequency and low-frequency symbols
            // to force frequent state < L checks.
            let mut data = Vec::with_capacity(length);
            for i in 0..length {
                if (i / 16) % 2 == 0 {
                    data.push(0); // high frequency
                } else {
                    data.push(255); // low frequency
                }
            }
            data
        }
        ModelProfile::IncompressibleLike => {
            // Every symbol roughly equally probable (maximum entropy).
            let mut data = Vec::with_capacity(length);
            for _ in 0..length {
                data.push(rng.random());
            }
            data
        }
    }
}

/// Build normalized frequency model from data.
fn build_normalized_model(data: &[u8], total: u32) -> (Vec<u32>, Vec<u32>) {
    let mut freqs = vec![0u32; 256];
    for &b in data {
        freqs[b as usize] += 1;
    }

    // If data is empty, use uniform model.
    let sum: u64 = freqs.iter().map(|&f| f as u64).sum();
    if sum == 0 {
        let uniform = total / 256;
        for f in &mut freqs {
            *f = uniform;
        }
    } else {
        // Normalize to total using deterministic rounding.
        for f in &mut freqs {
            *f = ((*f as u64 * total as u64) / sum) as u32;
        }
        // Adjust remainder to meet exact total.
        let current: u32 = freqs.iter().sum();
        if current < total {
            // Add remainder to largest frequency.
            if let Some(max_idx) = (0..256).max_by_key(|&i| freqs[i]) {
                freqs[max_idx] += total - current;
            }
        } else if current > total {
            // Subtract excess from largest frequency.
            if let Some(max_idx) = (0..256).max_by_key(|&i| freqs[i]) {
                if freqs[max_idx] >= current - total {
                    freqs[max_idx] -= current - total;
                }
            }
        }
    }

    // Build cumulative frequencies.
    let mut cum = vec![0u32; 257];
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    (freqs, cum)
}
