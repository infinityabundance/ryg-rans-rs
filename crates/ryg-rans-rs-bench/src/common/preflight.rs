//! # Benchmark preflight evidence channel (residual L1-D)
//!
//! Criterion measures time; it does not prove the measured code was
//! correct.  Every benchmark case that feeds the sealed performance
//! evidence must therefore emit a **preflight record** before timing: the
//! requested/executed backend, the verification outcome, and the hashes of
//! input, output, words consumed, and final states.  The performance
//! exporter joins Criterion timing to these records by exact benchmark ID
//! and refuses to fabricate a pass.
//!
//! Records are emitted as canonical JSON files named
//! `<benchmark-id-sanitized>.json` in the run's preflight directory.

use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Status of a benchmark case's preflight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BenchmarkCaseStatus {
    /// Preflight passed; timing followed.
    Passed,
    /// Preflight failed; the case must not be counted as executed evidence.
    Failed,
    /// The case is unsupported on this CPU/build and was not timed.
    Unsupported,
}

/// The structured verification record a benchmark emits before timing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkPreflightRecord {
    /// Exact Criterion benchmark ID (group/function/value hierarchy).
    pub benchmark_id: String,
    /// Backend the benchmark was configured/requested with.
    pub backend_requested: String,
    /// Backend that actually executed (verified by the benchmark itself).
    pub backend_executed: String,
    /// Whether output matched the reference (never hardcoded).
    pub verification_passed: bool,
    /// SHA-256 hex of the input corpus.
    pub input_sha256: String,
    /// SHA-256 hex of the decoded/verified output.
    pub output_sha256: String,
    /// SHA-256 hex of the reference output (expected).
    pub reference_output_sha256: String,
    /// Words consumed by the decode (where the backend exposes it).
    pub words_consumed: Option<usize>,
    /// Reference words consumed.
    pub reference_words_consumed: Option<usize>,
    /// SHA-256 hex of the final-states vector (where exposed).
    pub final_states_sha256: Option<String>,
    /// SHA-256 hex of the reference final-states vector.
    pub reference_final_states_sha256: Option<String>,
    /// Worker threads requested.
    pub threads_requested: usize,
    /// Worker threads effective (verified from the executor report).
    pub threads_effective: usize,
    /// Number of blocks in the workload.
    pub block_count: usize,
    /// Executor queue capacity.
    pub queue_capacity: usize,
    /// Allocation mode (e.g. "allocating", "into").
    pub allocation_mode: String,
    /// Preflight status.
    pub status: BenchmarkCaseStatus,
}

impl BenchmarkPreflightRecord {
    /// SHA-256 hex of the words-consumed vector, or empty when absent.
    pub fn words_consumed_sha256(&self) -> String {
        match (self.words_consumed, self.reference_words_consumed) {
            (Some(w), Some(r)) if w == r => sha256_of_usize(w),
            _ => String::new(),
        }
    }

    /// SHA-256 hex of the final-states vector, or empty when absent.
    pub fn final_states_sha256(&self) -> String {
        match (
            &self.final_states_sha256,
            &self.reference_final_states_sha256,
        ) {
            (Some(a), Some(b)) if a == b => a.clone(),
            _ => String::new(),
        }
    }

    /// Validate that the record is internally consistent.
    pub fn validate(&self) -> Result<(), String> {
        if self.verification_passed {
            if self.output_sha256.is_empty() {
                return Err(format!(
                    "{}: verification_passed with empty output hash",
                    self.benchmark_id
                ));
            }
            if self.reference_output_sha256.is_empty() {
                return Err(format!(
                    "{}: verification_passed with empty reference hash",
                    self.benchmark_id
                ));
            }
            if self.output_sha256 != self.reference_output_sha256 {
                return Err(format!(
                    "{}: output hash {} != reference {}",
                    self.benchmark_id, self.output_sha256, self.reference_output_sha256
                ));
            }
        }
        if self.threads_effective == 0 || self.threads_effective > self.threads_requested.max(1) * 4
        {
            return Err(format!(
                "{}: implausible effective threads {} (requested {})",
                self.benchmark_id, self.threads_effective, self.threads_requested
            ));
        }
        Ok(())
    }
}

fn sha256_of_usize(v: usize) -> String {
    let mut h = sha2::Sha256::new();
    h.update(v.to_le_bytes());
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Registry of preflight records for one benchmark run.
///
/// Files are named by their benchmark ID with `/` replaced by `__` (the
/// same sanitization Criterion applies to directory names, so the mapping
/// is stable and reversible for lookup).
pub struct PreflightRegistry {
    records: HashMap<String, BenchmarkPreflightRecord>,
}

impl PreflightRegistry {
    /// Load every preflight record from `dir`.
    pub fn load(dir: &Path) -> Result<Self, String> {
        let mut records = HashMap::new();
        if !dir.exists() {
            return Err(format!(
                "preflight directory {:?} does not exist (verification cannot be assumed)",
                dir
            ));
        }
        for entry in std::fs::read_dir(dir).map_err(|e| format!("read {:?}: {}", dir, e))? {
            let entry = entry.map_err(|e| format!("entry: {}", e))?;
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let content =
                std::fs::read_to_string(&path).map_err(|e| format!("read {:?}: {}", path, e))?;
            let rec: BenchmarkPreflightRecord =
                serde_json::from_str(&content).map_err(|e| format!("parse {:?}: {}", path, e))?;
            rec.validate()
                .map_err(|e| format!("{} (from {:?})", e, path))?;
            // Duplicate IDs are a hard error: one record per benchmark.
            if records.insert(rec.benchmark_id.clone(), rec).is_some() {
                return Err(format!("duplicate preflight record for {}", path.display()));
            }
        }
        Ok(PreflightRegistry { records })
    }

    /// Look up the preflight record for a benchmark ID.
    pub fn get(&self, benchmark_id: &str) -> Option<&BenchmarkPreflightRecord> {
        self.records.get(benchmark_id)
    }
}

/// Sanitize a benchmark ID for use as a file name (mirrors Criterion).
pub fn sanitize_id(id: &str) -> String {
    id.replace(['/', '\\', ' '], "_")
}

/// Write a preflight record to `dir` (creating it if needed).
pub fn emit_record(dir: &Path, record: &BenchmarkPreflightRecord) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {:?}: {}", dir, e))?;
    let path = dir.join(format!("{}.json", sanitize_id(&record.benchmark_id)));
    let json = serde_json::to_string_pretty(record).map_err(|e| format!("serialize: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("write {:?}: {}", path, e))?;
    Ok(path)
}
