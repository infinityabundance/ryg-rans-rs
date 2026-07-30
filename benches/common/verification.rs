//! Backend verification before benchmark timing.
//!
//! Every Criterion benchmark must validate its backend against a scalar
//! reference before registering or timing it.  This module provides the
//! verification infrastructure.

/// Result of backend verification.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub backend: &'static str,
    pub output_matches: bool,
    pub words_consumed_match: bool,
    pub final_states_match: bool,
    pub all_ok: bool,
}

/// Verify an 8-way decode result against the scalar 8-way reference.
pub fn verify_8way(
    backend_label: &'static str,
    output: &[u8],
    words_consumed: usize,
    final_states: &[u32; 8],
    reference_output: &[u8],
    reference_words: usize,
    reference_states: &[u32; 8],
) -> VerificationReport {
    let output_matches = output == reference_output;
    let words_consumed_match = words_consumed == reference_words;
    let final_states_match = final_states == reference_states;

    VerificationReport {
        backend: backend_label,
        output_matches,
        words_consumed_match,
        final_states_match,
        all_ok: output_matches && words_consumed_match && final_states_match,
    }
}

/// Verify a 16-way decode result against the scalar 16-way reference.
pub fn verify_16way(
    backend_label: &'static str,
    output: &[u8],
    words_consumed: usize,
    final_states: &[u32; 16],
    reference_output: &[u8],
    reference_words: usize,
    reference_states: &[u32; 16],
) -> VerificationReport {
    let output_matches = output == reference_output;
    let words_consumed_match = words_consumed == reference_words;
    let final_states_match = final_states == reference_states;

    VerificationReport {
        backend: backend_label,
        output_matches,
        words_consumed_match,
        final_states_match,
        all_ok: output_matches && words_consumed_match && final_states_match,
    }
}

/// Panic if verification fails.
pub fn assert_verified(report: &VerificationReport) {
    if !report.all_ok {
        panic!(
            "Backend '{}' verification FAILED:\n  output match: {}\n  words consumed match: {}\n  final states match: {}\n",
            report.backend,
            report.output_matches,
            report.words_consumed_match,
            report.final_states_match,
        );
    }
}
