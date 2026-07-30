//! Model construction and detection helpers for benchmarks.

use std::vec::Vec;

/// Build a frequency model from raw data with deterministic normalization.
pub fn build_freqs(data: &[u8], total: u32) -> (Vec<u32>, Vec<u32>) {
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
        for f in &mut freqs {
            *f = ((*f as u64 * total as u64) / sum) as u32;
        }
        let current: u32 = freqs.iter().sum();
        if current < total {
            if let Some(max_idx) = (0..256).max_by_key(|&i| freqs[i]) {
                freqs[max_idx] += total - current;
            }
        } else if current > total {
            if let Some(max_idx) = (0..256).max_by_key(|&i| freqs[i]) {
                if freqs[max_idx] >= current - total {
                    freqs[max_idx] -= current - total;
                }
            }
        }
    }

    let mut cum = vec![0u32; 257];
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    (freqs, cum)
}

/// Check if a frequency model represents the uniform-256 distribution.
pub fn is_uniform256(freqs: &[u32]) -> bool {
    if freqs.len() < 256 {
        return false;
    }
    for i in 0..256 {
        if freqs[i] != 16 {
            return false;
        }
    }
    true
}
