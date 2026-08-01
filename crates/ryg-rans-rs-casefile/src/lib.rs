//! # ryg-rans-rs-casefile
//!
//! **Typed evidence schema foundation for rANS forensic court proceedings.**
//!
//! This crate provides the data types used throughout the ryg-rans-rs forensic
//! testing infrastructure to record, reproduce, and track court proceedings
//! between the Rust implementation and the compiled C/C++ oracle.
//!
//! ## Core Types
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`Casefile`] | A complete, self-contained test case: inputs, model, expected outputs, and environment metadata |
//! | [`Receipt`] | The verdict of a completed court: which cases were compared, how many matched, and which residuals remain |
//! | [`Residual`] | A single observed difference between implementations: class, severity, status, and reproduction command |
//!
//! ## Design
//!
//! - **Deterministic**: All fields are explicitly typed with no ambient state.
//!   Case generation must use fixed seeds and named PRNG algorithms.
//! - **Content-addressed**: Large payloads are referenced by SHA-256 hash and
//!   stored separately from the casefile manifest.
//! - **Self-describing**: Every casefile records its schema version, upstream
//!   commit, compiler, host architecture, and endianness.
//!
//! ## Usage
//!
//! ```rust
//! # use ryg_rans_rs_casefile::*;
//! let case = Casefile::new("RYG_RANS.BYTE.BITSTREAM.000001", "byte32");
//! println!("Case: {} (variant: {})", case.case_id, case.variant);
//!
//! let residual = Residual {
//!     case_id: "RYG_RANS.BYTE.BITSTREAM.000001",
//!     court_id: "RYG_RANS.BYTE.BITSTREAM",
//!     variant: "byte32",
//!     upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
//!     class: "byte_mismatch",
//!     severity: "S1",
//!     status: "open",
//! };
//! println!("{}", residual);
//! ```

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(feature = "std")]
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

/// Schema version for casefiles.
pub const CASEFILE_SCHEMA_VERSION: u32 = 1;

/// A deterministic test case for rANS encoding/decoding.
///
/// Contains everything needed to reproduce a specific test: input data,
/// frequency model, scale bits, interleave setting, and expected outputs.
/// Large payloads are referenced by content hash and stored separately.
#[derive(Clone, Debug)]
pub struct Casefile {
    pub schema_version: u32,
    pub case_id: &'static str,
    pub upstream_commit: &'static str,
    pub variant: &'static str,
    pub operation: &'static str,
    pub seed: u64,
    pub input_sha256: Option<[u8; 32]>,
    pub input: Vec<u8>,
    pub scale_bits: u32,
    pub frequencies: Vec<u32>,
    pub cumulative_frequencies: Vec<u32>,
    pub interleave: u32,
}

impl Casefile {
    /// Create a new casefile with the given case ID and variant.
    ///
    /// Automatically sets the upstream commit to the pinned revision
    /// and initializes default values for all fields.
    pub fn new(case_id: &'static str, variant: &'static str) -> Self {
        Self {
            schema_version: CASEFILE_SCHEMA_VERSION,
            case_id,
            upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
            variant,
            operation: "encode_decode",
            seed: 0,
            input_sha256: None,
            input: Vec::new(),
            scale_bits: 14,
            frequencies: Vec::new(),
            cumulative_frequencies: Vec::new(),
            interleave: 1,
        }
    }
}

/// A court receipt documenting the result of an oracle comparison.
///
/// Receipts are the primary evidence artifact. A receipt with
/// `admitted_match` is required before any surface can be labelled `full`
/// in the parity model.
#[derive(Clone, Debug)]
pub struct Receipt {
    pub schema_version: u32,
    pub court_id: &'static str,
    pub case_count: u32,
    pub verdict: &'static str,
    pub upstream_commit: &'static str,
    pub rust_commit: Option<&'static str>,
    pub pairs_compared: u64,
    pub pairs_matched: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<&'static str>,
    pub timestamp: Option<u64>,
}

/// A residual documenting an observed difference between implementations.
///
/// Residuals are first-class engineering artifacts. Every observed difference
/// must be recorded, classified, and tracked until resolved or explicitly
/// admitted as a safe divergence.
#[derive(Clone, Debug)]
pub struct Residual {
    pub case_id: &'static str,
    pub court_id: &'static str,
    pub variant: &'static str,
    pub upstream_commit: &'static str,
    pub class: &'static str,
    pub severity: &'static str,
    pub status: &'static str,
}

impl fmt::Display for Residual {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}): {} [{}] - {}",
            self.case_id, self.court_id, self.class, self.severity, self.status
        )
    }
}

// ---------------------------------------------------------------------------
// Performance types — available when the `std` feature is enabled.
// ---------------------------------------------------------------------------

/// Schema version for performance evidence.
pub const PERF_SCHEMA_VERSION: u32 = 1;

/// CPU metadata for performance runs.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CpuMetadata {
    pub model: String,
    pub features: Vec<String>,
    pub microcode: Option<String>,
    pub smt_enabled: bool,
    pub governor: String,
}

/// OS metadata for performance runs.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct OsMetadata {
    pub kernel: String,
    pub os: String,
    pub memory: Option<String>,
}

/// Artifact hashes for a performance run.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PerformanceArtifactHashes {
    pub criterion_archive_sha256: String,
    pub results_json_sha256: String,
    pub results_csv_sha256: String,
    pub host_metadata_sha256: String,
    pub commands_log_sha256: String,
}

/// A single benchmark case within a performance receipt.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PerformanceCase {
    pub benchmark_id: String,
    pub backend_requested: String,
    pub backend_executed: String,
    pub profile: String,
    pub bytes: u64,
    pub threads_requested: usize,
    pub threads_effective: usize,
    pub sample_count: usize,
    pub median_ns: f64,
    pub mean_ns: f64,
    pub stddev_ns: f64,
    pub confidence_interval_95_low_ns: f64,
    pub confidence_interval_95_high_ns: f64,
    pub throughput_gib_s: f64,
    pub verification_passed: bool,
    pub output_hash: String,
    pub words_consumed_hash: Option<String>,
    pub final_states_hash: Option<String>,
    pub status: String,
}

/// Performance manifest — describes one performance sealing run for one surface.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PerformanceManifest {
    pub schema_version: u32,
    pub performance_id: String,
    pub surface: String,
    pub implementation_commit: String,
    pub run_id: String,
    pub host_id: String,
    pub benchmark_cases: Vec<PerformanceCase>,
    pub artifact_hashes: PerformanceArtifactHashes,
    pub command: String,
    pub rustflags: String,
    pub criterion_version: String,
    pub rustc_version: String,
    pub cpu: CpuMetadata,
    pub os: OsMetadata,
    pub dirty_tree: bool,
}

/// Performance receipt — seals a performance manifest.
///
/// `verdict` is the typed [`PerformanceReceiptVerdict`]; unknown serialized
/// values are rejected by serde (residual L1-M).
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PerformanceReceipt {
    pub schema_version: u32,
    pub performance_id: String,
    pub surface: String,
    pub verdict: PerformanceReceiptVerdict,
    pub implementation_commit: String,
    pub evidence_commit: String,
    pub run_id: String,
    pub host_id: String,
    pub cases_declared: u64,
    pub cases_executed: u64,
    pub cases_verified: u64,
    pub cases_failed: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<String>,
    pub manifest_sha256: String,
    pub criterion_archive_sha256: String,
    pub results_json_sha256: String,
    pub results_csv_sha256: String,
    pub host_metadata_sha256: String,
    pub commands_log_sha256: String,
    pub receipt_sha256: String,
    pub reproduction_command: String,
}

/// Typed performance receipt verdict (residual L1-M).  Unknown serialized
/// values are rejected by serde.
#[cfg(feature = "std")]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PerformanceReceiptVerdict {
    /// Measurement and provenance are sealed; every executed case verified.
    SealedMeasurement,
    /// Sealed with explicit performance residuals (a slower backend is still
    /// a sealed measurement).
    SealedWithResiduals,
    /// The receipt does not represent a valid sealed measurement.
    Rejected,
}

/// Performance index entry.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PerformanceIndexEntry {
    pub performance_id: String,
    /// SHA-256 of the exact final receipt file bytes on disk.
    pub receipt_file_sha256: String,
    /// SHA-256 of the canonical receipt content with its self-hash field
    /// omitted/empty (never called the file hash — residual L1-L).
    pub receipt_canonical_sha256: String,
}

/// Performance index.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PerformanceIndex {
    pub schema_version: u32,
    pub implementation_commit: String,
    pub run_id: String,
    pub host_id: String,
    pub receipts: Vec<PerformanceIndexEntry>,
}

// ---------------------------------------------------------------------------
// Phase L court types — available when the `std` feature is enabled.
// ---------------------------------------------------------------------------

/// Schema version for Phase L court evidence.
pub const PHASE_L_SCHEMA_VERSION: u32 = 1;

/// Per-case verdict in a Phase L court manifest (residual L.19).
#[cfg(feature = "std")]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseLCaseVerdict {
    /// The case behaved exactly as specified.
    Pass,
    /// The case did not behave as specified.
    Fail,
    /// The case was skipped because the precondition could not be satisfied
    /// on this host (e.g. a SIMD backend unavailable in the build).
    Skipped,
}

/// One executable case inside a Phase L court.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PhaseLCourtCase {
    /// Stable case identifier within the court (e.g. `CASES.0001`).
    pub case_id: String,
    /// Human-readable description of the input scenario.
    pub input: String,
    /// The documented/expected observable result.
    pub expected: String,
    /// The actual observed result.
    pub actual: String,
    /// Per-case verdict.
    pub verdict: PhaseLCaseVerdict,
    /// Residual IDs referenced by this case (empty when none).
    pub residual_ids: Vec<String>,
}

/// Phase L court manifest — the full per-case evidence record.
///
/// Mirrors the behavioral oracle manifests but is generated by Rust-side
/// guarantee courts rather than by upstream-C comparison.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PhaseLCourtManifest {
    pub schema_version: u32,
    /// Court ID (e.g. `RYG_RANS.L.VERIFY.DECODED_HASH`).
    pub court_id: String,
    /// Human-readable court title.
    pub title: String,
    /// The exact implementation commit the court executed.
    pub implementation_commit: String,
    /// The expected aggregate result (all cases pass, no residuals).
    pub expected_results: String,
    /// The actual aggregate result.
    pub actual_results: String,
    /// Every case, in execution order.
    pub cases: Vec<PhaseLCourtCase>,
}

/// Aggregate verdict for a Phase L court receipt (residual L.19).
#[cfg(feature = "std")]
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseLCourtVerdict {
    /// Every executable case passed.
    Passed,
    /// One or more cases failed.
    Failed,
    /// All cases were skipped (no surface exercised).
    Skipped,
}

/// Phase L court receipt — seals a Phase L court manifest.
#[cfg(feature = "std")]
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PhaseLCourtReceipt {
    pub schema_version: u32,
    /// Court ID (e.g. `RYG_RANS.L.VERIFY.DECODED_HASH`).
    pub court_id: String,
    /// Human-readable court title.
    pub title: String,
    /// Aggregate verdict.
    pub verdict: PhaseLCourtVerdict,
    /// The exact implementation commit the court executed.
    pub implementation_commit: String,
    /// The commit that generated this evidence.
    pub evidence_commit: String,
    pub num_cases: u64,
    pub cases_passed: u64,
    pub cases_failed: u64,
    pub cases_skipped: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<String>,
    /// SHA-256 of the exact manifest file bytes.
    pub manifest_sha256: String,
    /// Canonical self-hash: SHA-256 of this receipt with `receipt_sha256`
    /// emptied (never called the file hash — residual L1-L principle).
    pub receipt_sha256: String,
    /// Command that reproduces this court.
    pub reproduction_command: String,
}
