//! # Unit tests for the canonical model normalizer
//!
//! Regression coverage for the Phase L.15 normalisation bug: Phase 3 of
//! `FrequencyModel::build` never decremented `excess`, so a dominant-symbol
//! histogram produced a wrong-sum model (4286 ≠ 4096) in debug builds and a
//! silently corrupt model in release builds (the old `debug_assert` was a
//! no-op there).  These tests pin the sum invariant for adversarial and
//! random histograms across the whole scale range.

use ryg_rans_rs_cli::container::model::FrequencyModel;

/// Every built model must sum to exactly `2^scale_bits`, for every scale and
/// many deterministic pseudo-random histograms, including degenerate ones
/// (single symbol, two symbols, all symbols, near-uniform, dominant-symbol).
#[test]
fn normalization_sum_invariant_random_histograms() {
    let mut x: u64 = 0x1234_5678_9abc_def0;
    for scale_bits in 1..=20u8 {
        let target = 1u64 << scale_bits;
        // A few handcrafted adversarial shapes.
        let shapes: Vec<[u64; 256]> = vec![
            dominant_single_symbol(target),
            two_symbol_skew(target),
            all_symbols_even(target),
            near_uniform(target),
            empty_histogram(),
        ];
        for h in shapes {
            if let Ok(model) = FrequencyModel::build(&h, scale_bits) {
                let sum: u64 = model.frequencies.iter().map(|&f| f as u64).sum();
                assert_eq!(sum, target, "shape at scale_bits={}", scale_bits);
                // Frequencies must be monotone-compatible with the cumulative.
                let mut acc = 0u64;
                for s in 0..256usize {
                    assert_eq!(model.cumulative[s] as u64, acc, "cum[{}]", s);
                    acc += model.frequencies[s] as u64;
                }
                assert_eq!(acc, target);
            }
        }
        // Random histograms.
        for _ in 0..200 {
            let mut h = [0u64; 256];
            let nonzero = 1 + (x % 256) as usize;
            for sym in 0..nonzero {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                h[sym] = 1 + x % 1000;
            }
            match FrequencyModel::build(&h, scale_bits) {
                Ok(model) => {
                    let sum: u64 = model.frequencies.iter().map(|&f| f as u64).sum();
                    assert_eq!(sum, target, "random at scale_bits={}", scale_bits);
                    // Every observed symbol keeps freq >= 1.
                    for sym in 0..nonzero {
                        assert!(model.frequencies[sym] >= 1, "symbol {} dropped", sym);
                    }
                    // Serialization round trip preserves the model.
                    let bytes = model.to_bytes();
                    let back = FrequencyModel::from_bytes(&bytes, scale_bits).expect("parse");
                    assert_eq!(
                        model.frequencies, back.frequencies,
                        "roundtrip scale_bits={}",
                        scale_bits
                    );
                }
                // At tiny scales with many active symbols the model is
                // unrepresentable (fewer tokens than symbols); the typed
                // error is the correct outcome — never a panic or a wrong
                // sum.
                Err(_) => assert!(
                    nonzero as u64 > target,
                    "unexpected build failure at scale_bits={}",
                    scale_bits
                ),
            }
        }
    }
}

/// The exact regression case: one dominant symbol plus 255 rare symbols at
/// scale_bits = 12 (the CLI bench corpus).  Old code produced sum 4286.
#[test]
fn normalization_dominant_symbol_regression() {
    let mut h = [0u64; 256];
    h[0] = 1_032_252;
    for sym in 1..256 {
        h[sym] = 64;
    }
    let model = FrequencyModel::build(&h, 12).expect("build");
    let sum: u64 = model.frequencies.iter().map(|&f| f as u64).sum();
    assert_eq!(sum, 4096);
    assert!(model.frequencies[0] > 1);
    assert!(model.frequencies[1..].iter().all(|&f| f >= 1));
}

/// A model with more active symbols than available tokens must return a
/// typed error, not loop forever or panic.
#[test]
fn normalization_unrepresentable_returns_typed_error() {
    // scale_bits = 1 → only 2 tokens, but 256 active symbols each need >= 1.
    let mut h = [0u64; 256];
    for f in h.iter_mut() {
        *f = 1;
    }
    let err = FrequencyModel::build(&h, 1);
    assert!(err.is_err(), "unrepresentable model must error");
}

/// `from_bytes` must reject a scale outside 1..=31 with a typed error.
#[test]
fn from_bytes_rejects_bad_scale() {
    let model = FrequencyModel::build_uniform(12);
    let bytes = model.to_bytes();
    assert!(FrequencyModel::from_bytes(&bytes, 0).is_err());
    assert!(FrequencyModel::from_bytes(&bytes, 32).is_err());
}

fn dominant_single_symbol(_target: u64) -> [u64; 256] {
    let mut h = [0u64; 256];
    h[0] = 1_000_000;
    h[1] = 1;
    h
}

fn two_symbol_skew(target: u64) -> [u64; 256] {
    let mut h = [0u64; 256];
    h[0] = target * 1000;
    h[1] = target;
    h
}

fn all_symbols_even(_target: u64) -> [u64; 256] {
    [7u64; 256]
}

fn near_uniform(_target: u64) -> [u64; 256] {
    let mut h = [0u64; 256];
    for (i, f) in h.iter_mut().enumerate() {
        *f = 3 + (i as u64) % 17;
    }
    h
}

fn empty_histogram() -> [u64; 256] {
    [0u64; 256]
}

/// Empty histograms must be rejected with a typed error, never panic.
#[test]
fn empty_histogram_is_typed_error() {
    let err = FrequencyModel::build(&[0u64; 256], 12);
    assert!(err.is_err());
}
