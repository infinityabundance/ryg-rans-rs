//! Backend verification before benchmark timing.
//!
//! Every Criterion benchmark must validate its backend against a scalar
//! reference **before** registering or timing it.  This ensures that only
//! correct backends contribute to the measurement data — a miscompiled or
//! incorrectly ported SIMD kernel will be caught early rather than silently
//! producing wrong numbers.
//!
//! ## When verification runs
//!
//! Verification is invoked at **benchmark initialisation time**, not during
//! the timed measurement loop.  The typical call sequence in each benchmark
//! binary is:
//!
//! 1. Generate a test corpus (fixed seed, fixed length).
//! 2. Encode the corpus into 16-way Word rANS format (using the scalar
//!    encoder for determinism).
//! 3. Decode with the **scalar reference backend** → record reference output,
//!    words_consumed, final_states.
//! 4. Decode with the **backend under test** → record candidate output,
//!    words_consumed, final_states.
//! 5. Call `verify_8way` or `verify_16way` to produce a `VerificationReport`.
//! 6. Call `assert_verified` on the report — if the backend is wrong, the
//!    process panics **before any Criterion group is registered**.
//!
//! This means:
//! - If a benchmark binary has 5 backends and 3 of them fail verification,
//!   those 3 are never timed.  Criterion will report 0 measurements for them.
//! - The process exits with a non-zero status, making CI pipelines fail fast.
//!
//! ## Panic-on-failure policy
//!
//! `assert_verified` uses `panic!` rather than returning `Result` or printing
//! a warning for the following reasons:
//!
//! - **Safety-critical discipline**: rANS is used in data archival and
//!   transmission.  A backend that produces incorrect output is worse than
//!   no backend — it silently corrupts data.  Incorrect backends **must**
//!   abort the process.
//! - **CI transparency**: A panic with a descriptive message (showing exactly
//!   which of output/words_consumed/final_states mismatched) is immediately
//!   visible in CI logs and causes a non-zero exit code.
//! - **No fallback ambiguity**: If a backend fails verification, there is no
//!   "maybe it's okay for some inputs" — the verification corpus is a
//!   representative workload.  If the backend fails on that, it will fail on
//!   similar real data.
//!
//! ## VerificationReport fields
//!
//! The report breaks correctness into three independent checks:
//!
//! - `output_matches`: Are the decoded bytes identical?
//!   Byte-for-byte comparison of the full decoded output.
//! - `words_consumed_match`: Did both backends consume the same number of
//!   u16 words from the compressed stream?  A mismatch here indicates the
//!   backend is reading from the wrong positions or misinterpreting the
//!   renormalisation mask.
//! - `final_states_match`: After decoding all symbols, are the 8 or 16 final
//!   rANS states identical?  A mismatch while output_matches is true indicates
//!   a bug that would appear in subsequent blocks (multi-block streams).
//!
//! The combined `all_ok` field requires all three to be true.
//!
//! ## 8-way vs 16-way verification
//!
//! Two separate functions handle the two stream formats:
//!
//! - `verify_16way`: Compares output, words_consumed, and all 16 final states.
//! - `verify_8way`: Same three checks, but only compares the **first 8** of
//!   the 16-element `final_states` array (the `DecodeReport` type stores 16
//!   slots for uniformity; the upper 8 are zeroed for 8-way decodes).

/// Result of backend verification.
#[derive(Debug, Clone)]
pub struct VerificationReport {
    pub backend: &'static str,
    pub output_matches: bool,
    pub words_consumed_match: bool,
    pub final_states_match: bool,
    pub all_ok: bool,
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

/// Verify an 8-way decode result against the scalar 8-way reference.
///
/// Note: DecodeReport always stores 16 final states, but for 8-way decode
/// only the first 8 are meaningful.  We compare only the first 8.
pub fn verify_8way(
    backend_label: &'static str,
    output: &[u8],
    words_consumed: usize,
    final_states: &[u32; 16], // DecodeReport has 16, but we use first 8
    reference_output: &[u8],
    reference_words: usize,
    reference_states: &[u32; 8],
) -> VerificationReport {
    let output_matches = output == reference_output;
    let words_consumed_match = words_consumed == reference_words;
    let final_states_match = &final_states[..8] == &reference_states[..];
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
