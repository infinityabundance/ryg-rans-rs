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
        let nonzero_count = freqs.iter().filter(|&&f| f > 0).count() as u64;
        let reserved = nonzero_count;
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
            let mut remaining = total;
            for f in freqs.iter_mut().filter(|f| **f > 0) {
                if remaining > 0 {
                    *f = 1;
                    remaining -= 1;
                }
            }
            allocated = (total - remaining) as u64;
        }
        let mut remaining = (total as u64).saturating_sub(allocated);
        while remaining > 0 {
            let mut best_idx = 0usize;
            let mut best_val = 0u32;
            for i in 0..256 {
                if freqs[i] < 4095 && freqs[i] > best_val {
                    best_val = freqs[i];
                    best_idx = i;
                }
            }
            if best_val == 0 {
                if freqs[0] < 4095 {
                    let add = remaining.min((4095 - freqs[0]) as u64);
                    freqs[0] += add as u32;
                    remaining -= add;
                } else {
                    let add = remaining.min(1);
                    freqs[1] += add as u32;
                    remaining -= add;
                }
                continue;
            }
            freqs[best_idx] += 1;
            remaining -= 1;
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
