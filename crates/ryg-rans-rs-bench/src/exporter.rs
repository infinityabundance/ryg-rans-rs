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
//!   "tier":           "avx512",        // benchmark binary name
//!   "backend":        "avx512_16way",  // specific backend label
//!   "api":            "interleaved16", // API surface
//!   "profile":        "IncompressibleLike", // model profile
//!   "bytes":          1048576,          // input size in bytes
//!   "threads":        1,                // thread count (1 for single-threaded)
//!   "median_ns":      1234567.89,       // median wall-clock time (ns)
//!   "mean_ns":        1245678.90,       // mean wall-clock time (ns)
//!   "stddev_ns":      12345.67,         // standard deviation (ns)
//!   "throughput_gib_s": 7.89,           // median throughput in GiB/s
//!   "implementation_commit": "abc123def456", // git commit SHA
//!   "rustc":          "rustc 1.82.0 (...)", // rustc version string
//!   "cpu":            "AMD Ryzen 9 9950X 16-Core Processor", // CPU model name
//!   "target_features": ["avx512f", "avx512bw"]  // enabled target features
//! }
//! ```
//!
//! Fields are derived from two sources:
//! - **Criterion estimate files**: `median`, `mean`, `std_dev` from each
//!   `estimates.json` file under `target/criterion/<benchmark>/<backend>/<api>/<profile>/<size>/`.
//! - **System metadata**: `implementation_commit`, `rustc`, `cpu`, `target_features`
//!   are captured once at benchmark start by `common::metadata::BenchMetadata`.
//!
//! The `benchmark_id` is reconstructed from the directory path relative to the
//! Criterion output root, giving the 5-part key `<tier>/<backend>/<api>/<profile>/<size>`.
//! Byte counts are extracted by parsing size tokens like `1MiB`, `64KiB`, `4x1MiB`
//! from the path leaf (see `extract_bytes`).
//!
//! ## CSV schema
//!
//! The output `results.csv` contains one header row followed by one data row
//! per `BenchRecord`.  Fields that may contain commas, double-quotes, or
//! newlines are escaped using standard CSV quoting (wrapped in double-quotes,
//! internal double-quotes doubled).
//!
//! Columns:
//! ```text
//! benchmark_id,tier,backend,api,profile,bytes,threads,median_ns,mean_ns,stddev_ns,throughput_gib_s,commit
//! ```
//!
//! The throughput column is omitted from CSV because it is derived from
//! `bytes` and `median_ns` — consumers can recompute it as `(bytes / median_ns)
//! * 1e9 / (1024³)`.
//!
//! ## Usage
//!
//! The exporter is called automatically from `benches/` binaries via
//! `criterion_post_processing`.  It can also be invoked standalone:
//!
//! ```bash
//! # After benchmarks complete:
//! cargo run -p ryg-rans-rs-bench --bin export -- target/criterion artifacts/
//! # Writes artifacts/results.json and artifacts/results.csv
//! ```
//!
//! ## Integrity
//!
//! The JSON export returns a SHA-256 hex digest of the serialized content
//! alongside the file path.  CI pipelines can log this hash for traceability.
//! The CSV export does not include a hash — consumers should use the JSON hash
//! as the canonical content identifier and regenerate CSV from it.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::vec::Vec;

/// A single benchmark record for export.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BenchRecord {
    pub benchmark_id: String,
    pub tier: String,
    pub backend: String,
    pub api: String,
    pub profile: String,
    pub bytes: u64,
    pub threads: usize,
    pub median_ns: f64,
    pub mean_ns: f64,
    pub stddev_ns: f64,
    pub throughput_gib_s: f64,
    pub implementation_commit: String,
    pub rustc: String,
    pub cpu: String,
    pub target_features: Vec<String>,
}

/// Export benchmark records to JSON and CSV files.
pub fn export_summary(
    records: &[BenchRecord],
    output_dir: &Path,
) -> Result<(String, String), String> {
    // Create output directory
    fs::create_dir_all(output_dir).map_err(|e| format!("create dir: {}", e))?;

    // JSON export
    let json_path = output_dir.join("results.json");
    let json_content =
        serde_json::to_string_pretty(records).map_err(|e| format!("JSON serialize: {}", e))?;
    let mut json_file = fs::File::create(&json_path).map_err(|e| format!("create JSON: {}", e))?;
    json_file
        .write_all(json_content.as_bytes())
        .map_err(|e| format!("write JSON: {}", e))?;
    let json_hash = sha256_hex(json_content.as_bytes());

    // CSV export
    let csv_path = output_dir.join("results.csv");
    let mut csv_file = fs::File::create(&csv_path).map_err(|e| format!("create CSV: {}", e))?;

    // Header
    writeln!(csv_file, "benchmark_id,tier,backend,api,profile,bytes,threads,median_ns,mean_ns,stddev_ns,throughput_gib_s,commit")
        .map_err(|e| format!("write CSV header: {}", e))?;

    for r in records {
        writeln!(
            csv_file,
            "{},{},{},{},{},{},{},{},{},{},{},{}",
            csv_escape(&r.benchmark_id),
            csv_escape(&r.tier),
            csv_escape(&r.backend),
            csv_escape(&r.api),
            csv_escape(&r.profile),
            r.bytes,
            r.threads,
            r.median_ns,
            r.mean_ns,
            r.stddev_ns,
            r.throughput_gib_s,
            csv_escape(&r.implementation_commit),
        )
        .map_err(|e| format!("write CSV: {}", e))?;
    }

    Ok((json_path.to_string_lossy().to_string(), json_hash))
}

fn csv_escape(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// Load Criterion estimates from the target/criterion directory.
///
/// Walks the Criterion output tree and extracts median, mean, and stddev
/// for each benchmark.
pub fn load_criterion_estimates(
    criterion_dir: &Path,
    metadata: &crate::common::metadata::BenchMetadata,
) -> Result<Vec<BenchRecord>, String> {
    if !criterion_dir.exists() {
        return Ok(Vec::new());
    }

    let mut records = Vec::new();
    let criterion_dir = criterion_dir
        .canonicalize()
        .map_err(|e| format!("canonicalize: {}", e))?;

    // Walk the directory tree looking for estimate.json files.
    walk_dir(&criterion_dir, &criterion_dir, &mut records, metadata)?;

    Ok(records)
}

fn walk_dir(
    root: &Path,
    dir: &Path,
    records: &mut Vec<BenchRecord>,
    metadata: &crate::common::metadata::BenchMetadata,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("read dir {:?}: {}", dir, e))? {
        let entry = entry.map_err(|e| format!("entry: {}", e))?;
        let path = entry.path();

        if path.is_dir() {
            walk_dir(root, &path, records, metadata)?;
        } else if path.file_name().and_then(|n| n.to_str()) == Some("estimates.json") {
            // Parse the Criterion estimate file
            if let Some(record) = parse_estimate_file(&path, root, metadata)? {
                records.push(record);
            }
        }
    }
    Ok(())
}

fn parse_estimate_file(
    path: &Path,
    root: &Path,
    metadata: &crate::common::metadata::BenchMetadata,
) -> Result<Option<BenchRecord>, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("read {:?}: {}", path, e))?;

    // Criterion estimate.json contains fields like:
    // { "mean": {"point_estimate": ..., "standard_error": ..., ...},
    //   "std_dev": {...}, "median": {...}, ... }
    let est: HashMap<String, serde_json::Value> =
        serde_json::from_str(&content).map_err(|e| format!("parse {:?}: {}", path, e))?;

    let median_ns = est
        .get("median")
        .and_then(|v| v.get("point_estimate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let mean_ns = est
        .get("mean")
        .and_then(|v| v.get("point_estimate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let stddev_ns = est
        .get("std_dev")
        .and_then(|v| v.get("point_estimate"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    // Derive benchmark_id from path: relative to criterion dir
    let relative = path
        .strip_prefix(root)
        .map_err(|_| "path strip".to_string())?;
    let benchmark_id = relative
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.to_str())
        .unwrap_or("unknown")
        .to_string();

    // Try to extract bytes from throughput
    let bytes = extract_bytes(&benchmark_id).unwrap_or(0);

    // Parse benchmark_id components (clone to avoid borrow conflict)
    let id_clone = benchmark_id.clone();
    let parts: Vec<&str> = id_clone.split('/').collect();
    let tier = parts.first().copied().unwrap_or("unknown");
    let backend = parts.get(1).copied().unwrap_or("unknown");
    let api = parts.get(2).copied().unwrap_or("unknown");
    let profile = parts.get(3).copied().unwrap_or("unknown");

    Ok(Some(BenchRecord {
        benchmark_id,
        tier: tier.to_string(),
        backend: backend.to_string(),
        api: api.to_string(),
        profile: profile.to_string(),
        bytes,
        threads: 1,
        median_ns,
        mean_ns,
        stddev_ns,
        throughput_gib_s: if bytes > 0 && median_ns > 0.0 {
            (bytes as f64 / median_ns) * 1e9 / (1024.0 * 1024.0 * 1024.0)
        } else {
            0.0
        },
        implementation_commit: metadata.git_commit.clone(),
        rustc: metadata.rustc_version.clone(),
        cpu: metadata.cpu_model.clone(),
        target_features: metadata.target_features.clone(),
    }))
}

/// Extract byte count from benchmark ID like ".../1MiB" or ".../4x1MiB".
fn extract_bytes(id: &str) -> Option<u64> {
    // Look for patterns like "64KiB", "1MiB", "256KiB", "4x1MiB"
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
    fn test_csv_escape() {
        assert_eq!(csv_escape("hello"), "hello");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
    }
}
