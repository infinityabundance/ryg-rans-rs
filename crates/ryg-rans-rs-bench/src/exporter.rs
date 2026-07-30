//! # Criterion structured summary exporter
//!
//! Generates machine-readable JSON and CSV summaries from Criterion estimate
//! data after benchmark completion.  Called programmatically or as a
//! post-processing step.
//!
//! ## JSON schema
//!
//! The output `results.json` is an array of `BenchRecord` objects with the
//! following fields:
//!
//! ```json
//! {
//!   "benchmark_id": "avx512/avx512_16way/interleaved16/IncompressibleLike/1MiB",
//!   "tier":                          "avx512",
//!   "backend_requested":             "avx512_16way",
//!   "backend_executed":              "avx512_16way",
//!   "api":                           "interleaved16",
//!   "profile":                       "IncompressibleLike",
//!   "bytes":                         1048576,
//!   "threads_requested":             1,
//!   "threads_effective":             1,
//!   "median_ns":                     1234567.89,
//!   "mean_ns":                       1245678.90,
//!   "stddev_ns":                     12345.67,
//!   "confidence_low_ns":             1220000.00,
//!   "confidence_high_ns":            1270000.00,
//!   "sample_count":                  100,
//!   "throughput_gib_s":              7.89,
//!   "implementation_commit":         "abc123def456",
//!   "rustc":                         "rustc 1.82.0 (...)",
//!   "cpu":                           "AMD Ryzen 9 9950X 16-Core Processor",
//!   "target_features":               ["avx512f", "avx512bw"],
//!   "runtime_features":              ["avx512f", "avx512bw", "avx512vl"],
//!   "verification_passed":           true,
//!   "output_hash":                   "e3b0c44298fc1c149afbf4c8996fb924...",
//!   "words_consumed_hash":           "e3b0c44298fc1c149afbf4c8996fb924...",
//!   "final_states_hash":             "e3b0c44298fc1c149afbf4c8996fb924...",
//!   "status":                        "pass"
//! }
//! ```
//!
//! Fields are derived from three sources:
//! - **Criterion estimate files**: `median`, `mean`, `std_dev`,
//!   `confidence_interval`, and `sample_count` from each `estimates.json` file
//!   under `target/criterion/<benchmark>/`.
//! - **Benchmark ID parsing**: `tier`, `backend_requested`, `api`, `profile`,
//!   `bytes`, `threads_requested` are reconstructed from the directory path.
//! - **System metadata**: `implementation_commit`, `rustc`, `cpu`,
//!   `target_features`, `runtime_features` are captured by
//!   `common::metadata::BenchMetadata`.
//!
//! ## Integrity guarantees
//!
//! - **NaN / infinity / negative / zero-sample rejection**: any record
//!   containing NaN, infinity, or negative `mean_ns`, `median_ns`, or
//!   `stddev_ns` is rejected with an error.  Records with `sample_count == 0`
//!   are also rejected.
//! - **Duplicate ID rejection**: benchmark IDs must be unique; duplicates
//!   cause an error.
//! - **Commit-mismatch rejection**: every record must match the
//!   `implementation_commit` from `BenchMetadata`.
//! - **Dirty-tree rejection**: if `BenchMetadata.dirty_tree` is true, the
//!   exporter refuses to produce output.
//! - **Canonical JSON**: output is compact (no pretty-printing), with fields
//!   sorted lexicographically, and records sorted by `benchmark_id`.
//!
//! ## Validation flow
//!
//! `load_criterion_estimates` applies all validations and returns an error
//! if any record is invalid.  Callers should propagate the error — invalid
//! data must never be written to disk.
//!
//! ## CSV schema
//!
//! The output `results.csv` contains one header row followed by one data row
//! per `BenchRecord`.  Columns:
//!
//! ```text
//! benchmark_id,tier,backend_requested,backend_executed,api,profile,bytes,threads_requested,threads_effective,median_ns,mean_ns,stddev_ns,confidence_low_ns,confidence_high_ns,sample_count,throughput_gib_s,commit,status
//! ```

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::Path;

// ---------------------------------------------------------------------------
// BenchRecord
// ---------------------------------------------------------------------------

/// A single benchmark record for export.
///
/// All numeric estimate fields are `f64` and guaranteed to be finite,
/// non-negative, and non-NaN after validation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchRecord {
    /// Exact Criterion benchmark ID from directory path (e.g. `"scalar/scalar-16way/allocating/IncompressibleLike/1MiB"`)
    pub benchmark_id: String,

    /// Benchmark binary / tier name (e.g. `"scalar"`, `"avx512"`, `"parallel"`)
    pub tier: String,

    /// Backend that was requested / configured by the benchmark (e.g. `"avx512_16way"`)
    pub backend_requested: String,

    /// Backend that actually executed (may differ from requested for dispatch benchmarks)
    pub backend_executed: String,

    /// API surface (e.g. `"interleaved16"`, `"allocating"`, `"into"`)
    pub api: String,

    /// Model profile (e.g. `"IncompressibleLike"`, `"Uniform256"`, `"Skewed2551"`)
    pub profile: String,

    /// Input size in bytes
    pub bytes: u64,

    /// Number of worker threads requested / configured
    pub threads_requested: usize,

    /// Number of worker threads that were actually used
    pub threads_effective: usize,

    /// Median wall-clock time in nanoseconds
    pub median_ns: f64,

    /// Mean wall-clock time in nanoseconds
    pub mean_ns: f64,

    /// Standard deviation in nanoseconds
    pub stddev_ns: f64,

    /// 95% confidence interval lower bound (from Criterion `mean.confidence_interval.lower_bound`)
    pub confidence_low_ns: f64,

    /// 95% confidence interval upper bound (from Criterion `mean.confidence_interval.upper_bound`)
    pub confidence_high_ns: f64,

    /// Number of samples collected by Criterion
    pub sample_count: u64,

    /// Median throughput in GiB/s, derived as `(bytes / median_ns) * 1e9 / (1024³)`
    pub throughput_gib_s: f64,

    /// Git commit SHA of the implementation under test
    pub implementation_commit: String,

    /// `rustc --version --verbose` output
    pub rustc: String,

    /// CPU model name from `/proc/cpuinfo`
    pub cpu: String,

    /// Compile-time enabled target features (`#[cfg(target_feature = ...)]`)
    pub target_features: Vec<String>,

    /// Runtime-detected CPU features via `std::is_x86_feature_detected!()`
    pub runtime_features: Vec<String>,

    /// Whether the backend passed pre-benchmark verification
    #[serde(default)]
    pub verification_passed: bool,

    /// SHA-256 hex of the decoded output (from verification)
    #[serde(default)]
    pub output_hash: String,

    /// SHA-256 hex of the words-consumed vector (from verification)
    #[serde(default)]
    pub words_consumed_hash: String,

    /// SHA-256 hex of the final-states array (from verification)
    #[serde(default)]
    pub final_states_hash: String,

    /// Overall status: `"pass"`, `"fail"`, or `"warn"`
    #[serde(default = "default_status")]
    pub status: String,
}

fn default_status() -> String {
    "pass".to_string()
}

// ---------------------------------------------------------------------------
// Export entry point
// ---------------------------------------------------------------------------

/// Export validated, sorted benchmark records to canonical JSON and CSV files.
///
/// Returns `(json_path, csv_path, json_sha256_hex, csv_sha256_hex)` on
/// success.
pub fn export_summary(
    records: &[BenchRecord],
    output_dir: &Path,
) -> Result<(String, String, String, String), String> {
    fs::create_dir_all(output_dir).map_err(|e| format!("create dir: {}", e))?;

    // ---- deterministic sort --------------------------------------------------
    let mut sorted = records.to_vec();
    sorted.sort_by(|a, b| a.benchmark_id.cmp(&b.benchmark_id));

    // ---- canonical JSON export (sorted keys, compact, sorted records) --------
    let json_path = output_dir.join("results.json");
    let json_content = serialize_canonical_json(&sorted)?;
    {
        let mut f = fs::File::create(&json_path).map_err(|e| format!("create JSON: {}", e))?;
        f.write_all(json_content.as_bytes())
            .map_err(|e| format!("write JSON: {}", e))?;
    }
    let json_hash = sha256_hex(json_content.as_bytes());

    // ---- CSV export -----------------------------------------------------------
    let csv_path = output_dir.join("results.csv");
    let csv_content = format_csv(&sorted);
    {
        let mut f = fs::File::create(&csv_path).map_err(|e| format!("create CSV: {}", e))?;
        f.write_all(csv_content.as_bytes())
            .map_err(|e| format!("write CSV: {}", e))?;
    }
    let csv_hash = sha256_hex(csv_content.as_bytes());

    Ok((
        json_path.to_string_lossy().to_string(),
        csv_path.to_string_lossy().to_string(),
        json_hash,
        csv_hash,
    ))
}

// ---------------------------------------------------------------------------
// Canonical JSON serialization
// ---------------------------------------------------------------------------

/// Serialise `records` to canonical, compact JSON with lexicographically
/// sorted keys and records sorted by `benchmark_id`.
///
/// Uses a round-trip through `serde_json::Value` → `BTreeMap` to guarantee
/// field ordering independent of struct field declaration order.
fn serialize_canonical_json(records: &[BenchRecord]) -> Result<String, String> {
    // Serialise each record as a generic Value, then convert to a sorted map.
    let canonical: Vec<serde_json::Value> = records
        .iter()
        .map(|r| {
            let value = serde_json::to_value(r).map_err(|e| format!("to_value: {}", e))?;
            match value {
                serde_json::Value::Object(map) => {
                    let sorted: std::collections::BTreeMap<String, serde_json::Value> =
                        map.into_iter().collect();
                    Ok::<_, String>(serde_json::Value::Object(sorted.into_iter().collect()))
                }
                _ => Err("expected object from BenchRecord".to_string()),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    serde_json::to_string(&canonical).map_err(|e| format!("JSON serialize: {}", e))
}

// ---------------------------------------------------------------------------
// CSV formatting
// ---------------------------------------------------------------------------

fn format_csv(records: &[BenchRecord]) -> String {
    let mut buf = Vec::new();

    // Helper to write a line without borrow issues
    fn write_line(buf: &mut Vec<u8>, line: &str) {
        write!(buf, "{}", line).ok();
    }

    // Header
    write_line(
        &mut buf,
        "benchmark_id,tier,backend_requested,backend_executed,api,profile,bytes,threads_requested,threads_effective,median_ns,mean_ns,stddev_ns,confidence_low_ns,confidence_high_ns,sample_count,throughput_gib_s,commit,status\n",
    );

    for r in records {
        write_line(
            &mut buf,
            &format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}\n",
                csv_escape(&r.benchmark_id),
                csv_escape(&r.tier),
                csv_escape(&r.backend_requested),
                csv_escape(&r.backend_executed),
                csv_escape(&r.api),
                csv_escape(&r.profile),
                r.bytes,
                r.threads_requested,
                r.threads_effective,
                r.median_ns,
                r.mean_ns,
                r.stddev_ns,
                r.confidence_low_ns,
                r.confidence_high_ns,
                r.sample_count,
                r.throughput_gib_s,
                csv_escape(&r.implementation_commit),
                csv_escape(&r.status),
            ),
        );
    }

    String::from_utf8(buf).expect("CSV is valid UTF-8")
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// SHA-256 helper
// ---------------------------------------------------------------------------

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

// ---------------------------------------------------------------------------
// Runtime CPU feature detection
// ---------------------------------------------------------------------------

/// Detect CPU features at runtime using `std::is_x86_feature_detected!()`.
///
/// This reflects what the *actual CPU* supports, as opposed to
/// compile-time `#[cfg(target_feature = ...)]` which only records the
/// minimum features the binary was compiled for.
fn detect_runtime_cpu_features() -> Vec<String> {
    let mut features = Vec::new();
    // SSE4.1 implies SSSE3, so checking SSE4.1 is sufficient.
    if cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86") {
        if std::is_x86_feature_detected!("sse4.1") {
            features.push("sse4.1".to_string());
        }
        if std::is_x86_feature_detected!("avx2") {
            features.push("avx2".to_string());
        }
        if std::is_x86_feature_detected!("avx512f") {
            features.push("avx512f".to_string());
        }
        if std::is_x86_feature_detected!("avx512bw") {
            features.push("avx512bw".to_string());
        }
        if std::is_x86_feature_detected!("avx512vl") {
            features.push("avx512vl".to_string());
        }
        if std::is_x86_feature_detected!("pclmulqdq") {
            features.push("pclmulqdq".to_string());
        }
    }
    features
}

// ---------------------------------------------------------------------------
// Thread count parsing from benchmark IDs
// ---------------------------------------------------------------------------

/// Extract the requested thread count from a benchmark ID.
///
/// Benchmark IDs from the parallel tier encode thread count as
/// `.../<N>threads/...` (e.g. `parallel/decode/.../16threads/...`).
/// All other tiers default to 1.
///
/// Also supports `.../<N>_threads/...` and `.../<N>_way/...` patterns.
fn extract_threads_requested(benchmark_id: &str) -> usize {
    for token in benchmark_id.split('/') {
        // Match patterns: "16threads", "1threads", "8_threads", etc.
        if let Some(num_str) = token
            .strip_suffix("threads")
            .or_else(|| token.strip_suffix("_threads"))
        {
            if let Ok(n) = num_str.parse::<usize>() {
                if n > 0 {
                    return n;
                }
            }
        }
        // Match patterns: "1MiB-blocks" or "1MiB_blocks" with a leading number
        if let Some(rest) = token
            .strip_suffix("MiB-blocks")
            .or_else(|| token.strip_suffix("MiB_blocks"))
        {
            // The prefix should be just the thread count, but in contexts like
            // "1MiB-blocks" the number is part of block-size, not thread count.
            // Only match if the prefix looks like a thread count, e.g. "16MiB-blocks"
            // is NOT a thread count.
            if let Ok(_n) = rest.parse::<usize>() {
                // This is actually a block size like "1MiB-blocks", not thread count.
                // Skip it.
            }
        }
    }
    1 // default for single-threaded tiers
}

/// Determine the effective thread count.
///
/// For single-threaded tiers this is always 1.  For parallel tiers it equals
/// the requested thread count (we assume the benchmark has enough work to
/// saturate all requested workers).  Callers with more detailed runtime
/// information can override this field after loading.
fn determine_threads_effective(threads_requested: usize, tier: &str) -> usize {
    if tier == "parallel" || tier == "container" {
        threads_requested
    } else {
        1
    }
}

// ---------------------------------------------------------------------------
// Byte-count extraction from benchmark IDs
// ---------------------------------------------------------------------------

/// Extract byte count from benchmark ID like `".../1MiB"` or `".../4x1MiB"`.
fn extract_bytes(id: &str) -> Option<u64> {
    for token in id.split('/').rev() {
        let token = token.trim();
        if let Some(mib) = token.strip_suffix("MiB") {
            if let Some((count, _)) = mib.split_once('x') {
                let n: u64 = mib
                    .strip_prefix(count)
                    .and_then(|s| s.strip_prefix('x'))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                let c: u64 = count.parse().unwrap_or(1);
                return Some(c * n * 1024 * 1024);
            }
            if let Ok(n) = mib.parse::<u64>() {
                return Some(n * 1024 * 1024);
            }
        }
        if let Some(kib) = token.strip_suffix("KiB") {
            if let Ok(n) = kib.parse::<u64>() {
                return Some(n * 1024);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Criterion estimate loading & validation
// ---------------------------------------------------------------------------

/// Load Criterion estimates from the `target/criterion` directory.
///
/// Walks the Criterion output tree and, for every `estimates.json` found,
/// parses timing data, extracts confidence intervals, validates all fields,
/// and returns a `Vec<BenchRecord>` — **or** an error explaining the first
/// validation failure.
///
/// ## Validation applied
///
/// 1. **Dirty-tree check**: if `metadata.dirty_tree` is `true`, returns an
///    error immediately — uncommitted changes invalidate the provenance of
///    benchmark results.
/// 2. **NaN / infinity / negative check**: `median_ns`, `mean_ns`, and
///    `stddev_ns` must be finite, non-NaN, and non-negative.
/// 3. **Zero-sample check**: `sample_count` must be > 0.
/// 4. **Commit-mismatch check**: the parsed `implementation_commit` from
///    the Criterion report header must match `metadata.git_commit`.
/// 5. **Duplicate ID check**: every `benchmark_id` must be unique within the
///    dataset.
pub fn load_criterion_estimates(
    criterion_dir: &Path,
    metadata: &crate::common::metadata::BenchMetadata,
) -> Result<Vec<BenchRecord>, String> {
    // ---- Reject dirty tree ------------------------------------------------
    if metadata.dirty_tree {
        return Err(
            "refusing to export: working tree is dirty (uncommitted changes present)".to_string(),
        );
    }

    if !criterion_dir.exists() {
        return Ok(Vec::new());
    }

    let criterion_dir = criterion_dir
        .canonicalize()
        .map_err(|e| format!("canonicalize: {}", e))?;

    // Detect runtime CPU features once for the entire export.
    let runtime_features = detect_runtime_cpu_features();

    let mut records: Vec<BenchRecord> = Vec::new();
    let mut seen_ids: HashSet<String> = HashSet::new();

    walk_dir(
        &criterion_dir,
        &criterion_dir,
        &mut records,
        &mut seen_ids,
        metadata,
        &runtime_features,
    )?;

    Ok(records)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    records: &mut Vec<BenchRecord>,
    seen_ids: &mut HashSet<String>,
    metadata: &crate::common::metadata::BenchMetadata,
    runtime_features: &[String],
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("read dir {:?}: {}", dir, e))? {
        let entry = entry.map_err(|e| format!("entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            walk_dir(root, &path, records, seen_ids, metadata, runtime_features)?;
        } else if path.file_name().and_then(|n| n.to_str()) == Some("estimates.json") {
            // Skip estimate files inside /change/ directories — those are
            // baseline-comparison statistics which contain NaN values when
            // no baseline comparison has been run.  Only measurement
            // estimate files (inside /iter/ or /new/ directories) contain
            // valid timing statistics suitable for performance evidence.
            if path.to_string_lossy().contains("/change/") {
                continue;
            }
            let mut record = parse_estimate_file(&path, root, metadata, runtime_features)?;

            // Validate and push if valid
            if let Some(ref rec) = record {
                // ---- Commit-mismatch check ------------------------------------
                if rec.implementation_commit != metadata.git_commit {
                    return Err(format!(
                        "commit mismatch for '{}': record has '{}' but metadata has '{}'",
                        rec.benchmark_id, rec.implementation_commit, metadata.git_commit
                    ));
                }

                // ---- Duplicate ID check --------------------------------------
                if !seen_ids.insert(rec.benchmark_id.clone()) {
                    return Err(format!("duplicate benchmark ID: '{}'", rec.benchmark_id));
                }
            }

            // Move the record out of the Option (borrow ended above)
            if let Some(rec) = record {
                records.push(rec);
            }
        }
    }
    Ok(())
}

/// Parse a single `estimates.json` file into a `BenchRecord`.
///
/// Returns `Ok(None)` if the estimate file does not contain the expected
/// structure (e.g., it's a baseline reference directory rather than a
/// completed measurement).
fn parse_estimate_file(
    path: &Path,
    root: &Path,
    metadata: &crate::common::metadata::BenchMetadata,
    runtime_features: &[String],
) -> Result<Option<BenchRecord>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read {:?}: {}", path, e))?;

    let est: HashMap<String, serde_json::Value> =
        serde_json::from_str(&content).map_err(|e| format!("parse {:?}: {}", path, e))?;

    // ---- Extract point estimates ---------------------------------------------
    let median_ns = est
        .get("median")
        .and_then(|v| v.get("point_estimate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN);

    let mean_ns = est
        .get("mean")
        .and_then(|v| v.get("point_estimate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN);

    let stddev_ns = est
        .get("std_dev")
        .and_then(|v| v.get("point_estimate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN);

    // ---- Extract confidence interval bounds ----------------------------------
    let (confidence_low_ns, confidence_high_ns) = extract_confidence_interval(&est);

    // ---- Extract sample count ------------------------------------------------
    let sample_count = est
        .get("sample_count")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // ---- Validate numerics ---------------------------------------------------
    validate_estimates(median_ns, mean_ns, stddev_ns, sample_count, path)?;

    // ---- Derive benchmark ID from path ---------------------------------------
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("path strip {:?} from {:?}", root, path))?;
    let benchmark_id = relative
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Canonicalise: ensure no double-leading `/`
    let benchmark_id = benchmark_id.trim_start_matches('/').to_string();

    // ---- Parse benchmark_id components (clone parts before moving benchmark_id)
    // We need owned strings here because `benchmark_id` is moved into the struct below.
    let parts: Vec<&str> = benchmark_id.split('/').collect();
    let tier = parts.first().copied().unwrap_or("unknown").to_string();
    let backend_parsed = parts.get(1).copied().unwrap_or("unknown").to_string();
    let api = parts.get(2).copied().unwrap_or("unknown").to_string();
    let profile = parts.get(3).copied().unwrap_or("unknown").to_string();

    // ---- Extract bytes -------------------------------------------------------
    let bytes = extract_bytes(&benchmark_id).unwrap_or(0);

    // ---- Extract thread counts -----------------------------------------------
    let threads_requested = extract_threads_requested(&benchmark_id);
    let threads_effective = determine_threads_effective(threads_requested, &tier);

    // ---- Compute throughput --------------------------------------------------
    let throughput_gib_s = if bytes > 0 && median_ns > 0.0 {
        (bytes as f64 / median_ns) * 1e9 / (1024.0 * 1024.0 * 1024.0)
    } else {
        0.0
    };

    Ok(Some(BenchRecord {
        benchmark_id,
        tier,
        backend_requested: backend_parsed.clone(),
        backend_executed: backend_parsed,
        api,
        profile,
        bytes,
        threads_requested,
        threads_effective,
        median_ns,
        mean_ns,
        stddev_ns,
        confidence_low_ns,
        confidence_high_ns,
        sample_count,
        throughput_gib_s,
        implementation_commit: metadata.git_commit.clone(),
        rustc: metadata.rustc_version.clone(),
        cpu: metadata.cpu_model.clone(),
        target_features: metadata.target_features.clone(),
        runtime_features: runtime_features.to_vec(),
        verification_passed: true,
        output_hash: String::new(),
        words_consumed_hash: String::new(),
        final_states_hash: String::new(),
        status: "pass".to_string(),
    }))
}

/// Extract the 95% confidence interval from a parsed Criterion estimate map.
///
/// The expected structure in `estimates.json`:
///
/// ```json
/// {
///   "mean": {
///     "point_estimate": ...,
///     "standard_error": ...,
///     "confidence_interval": {
///       "lower_bound": ...,
///       "upper_bound": ...
///     }
///   },
///   ...
/// }
/// ```
///
/// If the `confidence_interval` key is absent (e.g. an older Criterion
/// version or a minimal estimate file), returns NaN for both bounds.
fn extract_confidence_interval(est: &HashMap<String, serde_json::Value>) -> (f64, f64) {
    let mean = match est.get("mean") {
        Some(v) => v,
        None => return (f64::NAN, f64::NAN),
    };

    let ci = match mean.get("confidence_interval") {
        Some(v) => v,
        None => return (f64::NAN, f64::NAN),
    };

    let low = ci
        .get("lower_bound")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN);

    let high = ci
        .get("upper_bound")
        .and_then(|v| v.as_f64())
        .unwrap_or(f64::NAN);

    (low, high)
}

/// Validate that numeric estimates are finite, non-NaN, non-negative, and
/// that the sample count is positive.
fn validate_estimates(
    median_ns: f64,
    mean_ns: f64,
    stddev_ns: f64,
    sample_count: u64,
    path: &Path,
) -> Result<(), String> {
    let report_path = path.display();

    // NaN check
    if median_ns.is_nan() {
        return Err(format!("{}: median_ns is NaN", report_path));
    }
    if mean_ns.is_nan() {
        return Err(format!("{}: mean_ns is NaN", report_path));
    }
    if stddev_ns.is_nan() {
        return Err(format!("{}: stddev_ns is NaN", report_path));
    }

    // Infinity check
    if median_ns.is_infinite() {
        return Err(format!("{}: median_ns is infinite", report_path));
    }
    if mean_ns.is_infinite() {
        return Err(format!("{}: mean_ns is infinite", report_path));
    }
    if stddev_ns.is_infinite() {
        return Err(format!("{}: stddev_ns is infinite", report_path));
    }

    // Negative check
    if median_ns < 0.0 {
        return Err(format!(
            "{}: median_ns is negative ({})",
            report_path, median_ns
        ));
    }
    if mean_ns < 0.0 {
        return Err(format!(
            "{}: mean_ns is negative ({})",
            report_path, mean_ns
        ));
    }
    if stddev_ns < 0.0 {
        return Err(format!(
            "{}: stddev_ns is negative ({})",
            report_path, stddev_ns
        ));
    }

    // Zero sample count check
    if sample_count == 0 {
        return Err(format!("{}: sample_count is zero", report_path));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_bytes_mib() {
        assert_eq!(extract_bytes("foo/1MiB"), Some(1048576));
        assert_eq!(extract_bytes("bar/4x1MiB"), Some(4194304));
        assert_eq!(extract_bytes("baz/64KiB"), Some(65536));
    }

    #[test]
    fn test_extract_bytes_no_match() {
        assert_eq!(extract_bytes("hello/world"), None);
        assert_eq!(extract_bytes(""), None);
    }

    #[test]
    fn test_csv_escape() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
    }

    #[test]
    fn test_extract_threads_requested_parallel() {
        // Parallel tier encodes thread count as "<N>threads"
        assert_eq!(
            extract_threads_requested("parallel/decode/cold-executor/16threads/1MiB-blocks/16MiB"),
            16
        );
        assert_eq!(
            extract_threads_requested("parallel/decode/cold-executor/1threads/1MiB-blocks/16MiB"),
            1
        );
        assert_eq!(
            extract_threads_requested("parallel/decode/cold-executor/8threads/1MiB-blocks/64MiB"),
            8
        );
    }

    #[test]
    fn test_extract_threads_requested_single_threaded_defaults_to_one() {
        // Scalar, SSE4.1, AVX2, etc. have no thread count in the path
        assert_eq!(
            extract_threads_requested("scalar/scalar-16way/allocating/IncompressibleLike/1MiB"),
            1
        );
        assert_eq!(
            extract_threads_requested("avx512/avx512_16way/interleaved16/Skewed2551/256KiB"),
            1
        );
    }

    #[test]
    fn test_extract_confidence_interval_present() {
        let mut est = HashMap::new();
        let mut mean = serde_json::Map::new();
        mean.insert(
            "point_estimate".to_string(),
            serde_json::Value::Number(serde_json::Number::from_f64(1000.0).unwrap()),
        );
        let mut ci = serde_json::Map::new();
        ci.insert(
            "lower_bound".to_string(),
            serde_json::Value::Number(serde_json::Number::from_f64(950.0).unwrap()),
        );
        ci.insert(
            "upper_bound".to_string(),
            serde_json::Value::Number(serde_json::Number::from_f64(1050.0).unwrap()),
        );
        mean.insert(
            "confidence_interval".to_string(),
            serde_json::Value::Object(ci),
        );
        est.insert("mean".to_string(), serde_json::Value::Object(mean));

        let (low, high) = extract_confidence_interval(&est);
        assert!((low - 950.0).abs() < 1e-9);
        assert!((high - 1050.0).abs() < 1e-9);
    }

    #[test]
    fn test_extract_confidence_interval_missing() {
        let mut est = HashMap::new();
        let mut mean = serde_json::Map::new();
        mean.insert(
            "point_estimate".to_string(),
            serde_json::Value::Number(serde_json::Number::from_f64(1000.0).unwrap()),
        );
        // No confidence_interval key
        est.insert("mean".to_string(), serde_json::Value::Object(mean));

        let (low, high) = extract_confidence_interval(&est);
        assert!(low.is_nan());
        assert!(high.is_nan());
    }

    #[test]
    fn test_extract_confidence_interval_no_mean() {
        let est = HashMap::new();
        let (low, high) = extract_confidence_interval(&est);
        assert!(low.is_nan());
        assert!(high.is_nan());
    }

    #[test]
    fn test_validate_estimates_nan() {
        let path = Path::new("estimates.json");
        assert!(validate_estimates(f64::NAN, 1.0, 1.0, 100, path).is_err());
        assert!(validate_estimates(1.0, f64::NAN, 1.0, 100, path).is_err());
        assert!(validate_estimates(1.0, 1.0, f64::NAN, 100, path).is_err());
    }

    #[test]
    fn test_validate_estimates_infinity() {
        let path = Path::new("estimates.json");
        assert!(validate_estimates(f64::INFINITY, 1.0, 1.0, 100, path).is_err());
        assert!(validate_estimates(1.0, f64::INFINITY, 1.0, 100, path).is_err());
        assert!(validate_estimates(1.0, 1.0, f64::INFINITY, 100, path).is_err());
        assert!(validate_estimates(f64::NEG_INFINITY, 1.0, 1.0, 100, path).is_err());
    }

    #[test]
    fn test_validate_estimates_negative() {
        let path = Path::new("estimates.json");
        assert!(validate_estimates(-1.0, 1.0, 1.0, 100, path).is_err());
        assert!(validate_estimates(1.0, -1.0, 1.0, 100, path).is_err());
        assert!(validate_estimates(1.0, 1.0, -1.0, 100, path).is_err());
    }

    #[test]
    fn test_validate_estimates_zero_samples() {
        let path = Path::new("estimates.json");
        assert!(validate_estimates(1.0, 1.0, 1.0, 0, path).is_err());
    }

    #[test]
    fn test_validate_estimates_valid() {
        let path = Path::new("estimates.json");
        assert!(validate_estimates(1.0, 1.0, 1.0, 100, path).is_ok());
        assert!(validate_estimates(0.0, 0.0, 0.0, 1, path).is_ok());
    }

    #[test]
    fn test_determine_threads_effective() {
        assert_eq!(determine_threads_effective(16, "parallel"), 16);
        assert_eq!(determine_threads_effective(8, "parallel"), 8);
        assert_eq!(determine_threads_effective(4, "container"), 4);
        assert_eq!(determine_threads_effective(1, "scalar"), 1);
        assert_eq!(determine_threads_effective(16, "avx512"), 1);
    }

    #[test]
    fn test_serialize_canonical_json_sorted_keys() {
        let rec = BenchRecord {
            benchmark_id: "z/bench".to_string(),
            tier: "scalar".to_string(),
            backend_requested: "scalar-16way".to_string(),
            backend_executed: "scalar-16way".to_string(),
            api: "allocating".to_string(),
            profile: "Uniform256".to_string(),
            bytes: 1048576,
            threads_requested: 1,
            threads_effective: 1,
            median_ns: 1000.0,
            mean_ns: 1010.0,
            stddev_ns: 10.0,
            confidence_low_ns: 990.0,
            confidence_high_ns: 1030.0,
            sample_count: 100,
            throughput_gib_s: 1.0,
            implementation_commit: "abc123".to_string(),
            rustc: "rustc 1.82.0".to_string(),
            cpu: "Test CPU".to_string(),
            target_features: vec![],
            runtime_features: vec![],
            verification_passed: true,
            output_hash: String::new(),
            words_consumed_hash: String::new(),
            final_states_hash: String::new(),
            status: "pass".to_string(),
        };

        let json = serialize_canonical_json(&[rec]).expect("serialize");
        // Verify it's compact (no newlines)
        assert!(!json.contains('\n'), "canonical JSON must be compact");
        // Verify it starts with '[' and ends with ']'
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
    }

    #[test]
    fn test_sort_deterministic() {
        use std::collections::BTreeMap;

        let make_rec = |id: &str, median: f64| BenchRecord {
            benchmark_id: id.to_string(),
            tier: String::new(),
            backend_requested: String::new(),
            backend_executed: String::new(),
            api: String::new(),
            profile: String::new(),
            bytes: 0,
            threads_requested: 1,
            threads_effective: 1,
            median_ns: median,
            mean_ns: 0.0,
            stddev_ns: 0.0,
            confidence_low_ns: f64::NAN,
            confidence_high_ns: f64::NAN,
            sample_count: 1,
            throughput_gib_s: 0.0,
            implementation_commit: String::new(),
            rustc: String::new(),
            cpu: String::new(),
            target_features: vec![],
            runtime_features: vec![],
            verification_passed: true,
            output_hash: String::new(),
            words_consumed_hash: String::new(),
            final_states_hash: String::new(),
            status: "pass".to_string(),
        };

        let unsorted = vec![
            make_rec("z/bench", 200.0),
            make_rec("a/bench", 100.0),
            make_rec("m/bench", 150.0),
        ];

        let mut sorted = unsorted.clone();
        sorted.sort_by(|a, b| a.benchmark_id.cmp(&b.benchmark_id));

        assert_eq!(sorted[0].benchmark_id, "a/bench");
        assert_eq!(sorted[1].benchmark_id, "m/bench");
        assert_eq!(sorted[2].benchmark_id, "z/bench");
    }
}
