//! # Property tests: deterministic model normalizer
//!
//! `FrequencyModel::build` must map **any** histogram to frequencies that
//! sum to exactly `2^scale_bits` (when representable), keep every observed
//! symbol at freq >= 1, and round-trip through the canonical byte encoding.
//! This is a randomized complement to the adversarial stress test in
//! `model_normalizer.rs`.

use proptest::prelude::*;
use ryg_rans_rs_cli::container::model::FrequencyModel;

proptest! {
    /// Any histogram with a bounded total must either build to an exact-sum
    /// model or fail with a typed error (unrepresentable at the scale); a
    /// successful build must round-trip through the canonical encoding.
    #[test]
    fn normalization_sum_and_roundtrip(
        counts in prop::collection::vec(0u64..1_000_000, 256),
        scale_bits in 1u8..=20,
    ) {
        let mut hist = [0u64; 256];
        let observed: Vec<usize> = counts
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c > 0)
            .map(|(i, _)| i)
            .collect();
        for (i, &c) in counts.iter().enumerate() {
            hist[i] = c;
        }
        match FrequencyModel::build(&hist, scale_bits) {
            Ok(model) => {
                let sum: u64 = model.frequencies.iter().map(|&f| f as u64).sum();
                assert_eq!(sum, 1u64 << scale_bits, "sum is exact");
                for &s in &observed {
                    assert!(model.frequencies[s] >= 1, "observed symbol {} kept", s);
                }
                // Canonical serialization round trip preserves the model.
                let bytes = model.to_bytes();
                let back = FrequencyModel::from_bytes(&bytes, scale_bits)
                    .expect("canonical model must parse");
                assert_eq!(model.frequencies, back.frequencies);
                assert_eq!(model.cumulative, back.cumulative);
            }
            // Unrepresentable: more active symbols than tokens at tiny
            // scales.  A typed error is correct; never a panic or wrong sum.
            Err(_) => assert!(
                observed.len() as u64 > (1u64 << scale_bits),
                "unexpected build failure"
            ),
        }
    }
}
