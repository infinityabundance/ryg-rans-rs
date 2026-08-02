//! # Phase L.19 courts — Rust-side guarantee courts
//!
//! The 14 Phase L behavioural courts execute **real code paths** and record
//! per-case verdicts.  Unlike the upstream-parity courts (oracle crate), these
//! courts prove Rust-side guarantees introduced by Phase L:
//!
//! 1. `RYG_RANS.L.VERIFY.DECODED_HASH` — decoded-output integrity (L.2)
//! 2. `RYG_RANS.L.INTEGRITY.STRICT` — strict vs compatibility integrity policy
//! 3. `RYG_RANS.L.CANCEL.COMPLETENESS` — cancellation completeness (L.3)
//! 4. `RYG_RANS.L.EXECUTOR.BOUNDED` — bounded live pipeline (L.4)
//! 5. `RYG_RANS.L.REORDER.ATOMIC_COMMIT` — atomic reorder commit (L.5)
//! 6. `RYG_RANS.L.CONFIG.WIRING` — every `ParallelConfig` field is wired (L.6)
//! 7. `RYG_RANS.L.SCRATCH.INTEGRATION` — `WorkerScratch` in production (L.7)
//! 8. `RYG_RANS.L.MODEL_CACHE.INTEGRATION` — `ModelCache` in production (L.8)
//! 9. `RYG_RANS.L.BACKEND.EXPLICIT` — exact backend semantics (L.9)
//! 10. `RYG_RANS.L.SSE41.UNSAFE_QUARANTINE` — unsafe ledger + features (L.10)
//! 11. `RYG_RANS.L.PERFORMANCE.EXPORT` — exporter correctness (L.1/L.18)
//! 12. `RYG_RANS.L.PERFORMANCE.ARCHIVE` — deterministic archive round-trip (L.1-K)
//! 13. `RYG_RANS.L.PERFORMANCE.RECEIPT_CHAIN` — receipt hash chain (L.1-L)
//! 14. `RYG_RANS.L.PUBLIC_API.REACHABILITY` — no disconnected public API (L.13)
//!
//! ## Evidence model
//!
//! Each court produces a [`CourtRun`] containing one [`CourtCase`] per
//! scenario.  The xtask `courts-run` command serializes the run into a
//! `PhaseLCourtManifest` (full per-case record) and a `PhaseLCourtReceipt`
//! (aggregate verdict + hashes), then updates `evidence/index.json` and the
//! parity model citations.
//!
//! ## Verdict vocabulary
//!
//! Per-case verdicts are [`ryg_rans_rs_casefile::PhaseLCaseVerdict`]
//! (`pass`/`fail`/`skipped`).  Aggregate receipt verdicts are
//! [`ryg_rans_rs_casefile::PhaseLCourtVerdict`]
//! (`passed`/`failed`/`skipped`).  Unknown serialized values are rejected by
//! serde — no free-form verdict strings.

use ryg_rans_rs_casefile::{
    PHASE_L_SCHEMA_VERSION, PhaseLCaseVerdict, PhaseLCourtCase, PhaseLCourtManifest,
    PhaseLCourtReceipt, PhaseLCourtVerdict,
};
use sha2::{Digest, Sha256};

/// One executable scenario inside a court.
#[derive(Debug, Clone)]
pub struct CourtCase {
    pub case_id: String,
    pub input: String,
    pub expected: String,
    pub actual: String,
    pub verdict: PhaseLCaseVerdict,
    pub residual_ids: Vec<String>,
}

/// The result of running one court: its ID, title, and per-case verdicts.
#[derive(Debug, Clone)]
pub struct CourtRun {
    pub court_id: String,
    pub title: String,
    pub cases: Vec<CourtCase>,
    /// Residual IDs this court references (empty when none).
    pub residual_ids: Vec<String>,
}

impl CourtRun {
    /// Count cases by verdict.
    pub fn counts(&self) -> (u64, u64, u64) {
        let mut passed = 0u64;
        let mut failed = 0u64;
        let mut skipped = 0u64;
        for c in &self.cases {
            match c.verdict {
                PhaseLCaseVerdict::Pass => passed += 1,
                PhaseLCaseVerdict::Fail => failed += 1,
                PhaseLCaseVerdict::Skipped => skipped += 1,
            }
        }
        (passed, failed, skipped)
    }

    /// Aggregate verdict: `Failed` if any case failed, `Passed` if at least
    /// one case ran and none failed, `Skipped` if every case was skipped.
    pub fn verdict(&self) -> PhaseLCourtVerdict {
        let (passed, failed, _skipped) = self.counts();
        if failed > 0 {
            PhaseLCourtVerdict::Failed
        } else if passed > 0 {
            PhaseLCourtVerdict::Passed
        } else {
            PhaseLCourtVerdict::Skipped
        }
    }
}

/// Convert a court run into a manifest + receipt pair, computing both hashes
/// with the canonical scheme (L1-L / L1-R doctrine):
///
/// - `manifest_sha256` — SHA-256 of the exact manifest file bytes.
/// - `receipt_sha256` — SHA-256 of the canonical receipt content with the
///   `receipt_sha256` field emptied (never called the file hash).
///
/// The returned `(manifest_json, receipt_json)` are the exact bytes that must
/// be written to disk so the hashes verify.
pub fn seal_court(
    run: &CourtRun,
    implementation_commit: &str,
    evidence_commit: &str,
    reproduction_command: &str,
) -> (PhaseLCourtManifest, PhaseLCourtReceipt) {
    let (passed, failed, skipped) = run.counts();
    let verdict = run.verdict();

    let manifest = PhaseLCourtManifest {
        schema_version: PHASE_L_SCHEMA_VERSION,
        court_id: run.court_id.clone(),
        title: run.title.clone(),
        implementation_commit: implementation_commit.to_string(),
        expected_results: "every executable case passes; no residuals".to_string(),
        actual_results: format!(
            "{} cases: {} passed, {} failed, {} skipped",
            run.cases.len(),
            passed,
            failed,
            skipped
        ),
        cases: run
            .cases
            .iter()
            .map(|c| PhaseLCourtCase {
                case_id: c.case_id.clone(),
                input: c.input.clone(),
                expected: c.expected.clone(),
                actual: c.actual.clone(),
                verdict: c.verdict.clone(),
                residual_ids: c.residual_ids.clone(),
            })
            .collect(),
    };
    let manifest_json =
        serde_json::to_string_pretty(&manifest).expect("serialize Phase L manifest");
    let manifest_sha256 = sha256_hex(manifest_json.as_bytes());

    let receipt = PhaseLCourtReceipt {
        schema_version: PHASE_L_SCHEMA_VERSION,
        court_id: run.court_id.clone(),
        title: run.title.clone(),
        verdict: verdict.clone(),
        implementation_commit: implementation_commit.to_string(),
        evidence_commit: evidence_commit.to_string(),
        num_cases: run.cases.len() as u64,
        cases_passed: passed,
        cases_failed: failed,
        cases_skipped: skipped,
        residual_count: run.residual_ids.len() as u32,
        residual_ids: run.residual_ids.clone(),
        manifest_sha256,
        receipt_sha256: String::new(),
        reproduction_command: reproduction_command.to_string(),
    };
    // Canonical self-hash: serialize with receipt_sha256 emptied.
    let receipt_json = serde_json::to_string_pretty(&receipt).expect("serialize Phase L receipt");
    let receipt_sha256 = sha256_hex(receipt_json.as_bytes());

    let mut receipt = receipt;
    receipt.receipt_sha256 = receipt_sha256;
    (manifest, receipt)
}

/// SHA-256 hex helper.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

// ---------------------------------------------------------------------------
// Court implementations
// ---------------------------------------------------------------------------

pub mod backend_explicit;
pub mod cancel_completeness;
pub mod config_wiring;
pub mod executor_bounded;
pub mod integrity_strict;
pub mod model_cache_integration;
pub mod performance_archive;
pub mod performance_export;
pub mod performance_receipt_chain;
pub mod phase_o_cache;
pub mod public_api_reachability;
pub mod reorder_atomic_commit;
pub mod scratch_integration;
pub mod unsafe_quarantine;
pub mod verify_decoded_hash;

/// Run every Phase L court and return the sealed (manifest, receipt) pairs.
///
/// `implementation_commit` is the exact commit whose code the courts execute;
/// `evidence_commit` is the commit generating the evidence (they differ when
/// evidence-only commits follow the implementation commit).
pub fn run_all_courts(
    implementation_commit: &str,
    evidence_commit: &str,
) -> Vec<(PhaseLCourtManifest, PhaseLCourtReceipt)> {
    let mut out = Vec::new();
    let courts: Vec<fn() -> CourtRun> = vec![
        verify_decoded_hash::court,
        integrity_strict::court,
        cancel_completeness::court,
        executor_bounded::court,
        reorder_atomic_commit::court,
        config_wiring::court,
        scratch_integration::court,
        model_cache_integration::court,
        backend_explicit::court,
        unsafe_quarantine::court,
        performance_export::court,
        performance_archive::court,
        performance_receipt_chain::court,
        public_api_reachability::court,
        // Phase O cache courts (O.20).
        phase_o_cache::court_exact_bytes,
        phase_o_cache::court_zero_capacity,
        phase_o_cache::court_oversized,
        phase_o_cache::court_unique_keys,
        phase_o_cache::court_single_flight,
        phase_o_cache::court_failure_equivalence,
        phase_o_cache::court_cancellation,
        phase_o_cache::court_metrics,
        phase_o_cache::court_workload_public_rans_v1,
    ];
    for court_fn in courts {
        let run = court_fn();
        let repro = format!(
            "cargo xtask courts-run --implementation-commit {} --only {}",
            implementation_commit, run.court_id
        );
        out.push(seal_court(
            &run,
            implementation_commit,
            evidence_commit,
            &repro,
        ));
    }
    out
}

/// Execute every court and assert every executable case passes.
///
/// This makes the behavioural courts part of `cargo test --workspace`: a
/// court whose guarantee regresses fails the test suite before evidence
/// generation can even be attempted (the xtask courts-run command remains
/// the evidence-producing path; this test is the fast feedback loop).
#[cfg(test)]
mod court_tests {
    use super::*;

    #[test]
    fn every_court_passes() {
        let sealed = run_all_courts("test", "test");
        assert!(!sealed.is_empty(), "no courts registered");
        let mut failures = Vec::new();
        for (manifest, receipt) in &sealed {
            if receipt.verdict != ryg_rans_rs_casefile::PhaseLCourtVerdict::Passed {
                failures.push(format!(
                    "{}: {:?} ({} passed / {} failed / {} skipped)",
                    manifest.court_id,
                    receipt.verdict,
                    receipt.cases_passed,
                    receipt.cases_failed,
                    receipt.cases_skipped
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "court failures:\n{}",
            failures.join("\n")
        );
    }
}
