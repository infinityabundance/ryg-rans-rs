//! # Phase G evidence generator
//!
//! Generates the 16 AVX512 behavioral receipts (8 for AVX512VL.INTERLEAVED8
//! and 8 for AVX512.INTERLEAVED16) and merges them with the existing
//! 128 Phase A-F receipts.
//!
//! ## Usage
//!
//! ```sh
//! cd oracle/adapter && make
//! RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
//!     cargo run --release --bin run-phase-g
//! ```
//!
//! This generates:
//! - `evidence.staging/receipts/receipt-RYG_RANS.AVX512VL.INTERLEAVED8.*.json` (8 receipts)
//! - `evidence.staging/receipts/receipt-RYG_RANS.AVX512.INTERLEAVED16.*.json` (8 receipts)
//! - `evidence.staging/manifests/manifest-*.json` (16 manifests)
//! - `evidence.staging/index.json` (merged index with all 144 receipts)
//!
//! After generating, run `cargo xtask seal` to verify.

use ryg_rans_rs_casefile as casefile;
use ryg_rans_rs_oracle::ModelProfile;
use ryg_rans_rs_oracle::phase_g::{run_avx512_16_court, run_avx512vl8_court};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let oracle = args
        .get(1)
        .map(|s| s.as_str())
        .filter(|p| Path::new(p).exists())
        .or_else(|| {
            let alt = "oracle/adapter/rans_trace";
            if Path::new(alt).exists() {
                Some(alt)
            } else {
                None
            }
        })
        .ok_or_else(|| format!("ERROR: oracle not found. Build it: cd oracle/adapter && make"))?;

    let scale_bits: u32 = 12;
    let seed: u64 = 42;
    let override_cases: Option<usize> = None;

    let evidence_root = "evidence";
    let staging_root = format!(
        "{}.staging/phase-g-{}",
        evidence_root,
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );

    let receipt_dir = format!("{}/receipts", staging_root);
    let manifest_dir = format!("{}/manifests", staging_root);
    std::fs::create_dir_all(&receipt_dir)?;
    std::fs::create_dir_all(&manifest_dir)?;

    let profiles = [
        ModelProfile::Uniform256,
        ModelProfile::Freq1Residual,
        ModelProfile::Skewed2551,
        ModelProfile::Sparse2,
        ModelProfile::Sparse17,
        ModelProfile::PrimeResidue,
        ModelProfile::RenormBoundary,
        ModelProfile::LengthBoundary,
    ];

    let mut all_passed = true;
    let mut index = Vec::new();

    // ---- Generate AVX512VL.INTERLEAVED8 receipts ----
    println!("=== AVX512VL.INTERLEAVED8 Courts ===");
    for &profile in &profiles {
        let (receipt, manifest, _manifest_bytes) =
            run_avx512vl8_court(oracle, scale_bits, seed, profile, override_cases)?;

        println!(
            "  {}: verdict={} ({}/{})",
            receipt.court_id, receipt.verdict, receipt.pairs_matched, receipt.pairs_compared
        );

        if receipt.verdict != "admitted_match" {
            all_passed = false;
        }

        // Write receipt
        let r_path = format!("{}/receipt-{}.json", receipt_dir, receipt.court_id);
        let r_json = serde_json::to_string_pretty(&receipt)?;
        std::fs::write(&r_path, &r_json)?;

        // Write manifest
        let m_path = format!("{}/manifest-{}.json", manifest_dir, receipt.court_id);
        let m_json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(&m_path, &m_json)?;

        // Compute receipt SHA-256 for index
        let mut hasher = Sha256::new();
        hasher.update(r_json.as_bytes());
        let sha = format!("{:x}", hasher.finalize());

        index.push(serde_json::json!({
            "court_id": receipt.court_id,
            "sha256": sha,
        }));
    }

    // ---- Generate AVX512.INTERLEAVED16 receipts ----
    println!("=== AVX512.INTERLEAVED16 Courts ===");
    for &profile in &profiles {
        let (receipt, manifest, _manifest_bytes) =
            run_avx512_16_court(oracle, scale_bits, seed, profile, override_cases)?;

        println!(
            "  {}: verdict={} ({}/{})",
            receipt.court_id, receipt.verdict, receipt.pairs_matched, receipt.pairs_compared
        );

        if receipt.verdict != "admitted_match" {
            all_passed = false;
        }

        // Write receipt
        let r_path = format!("{}/receipt-{}.json", receipt_dir, receipt.court_id);
        let r_json = serde_json::to_string_pretty(&receipt)?;
        std::fs::write(&r_path, &r_json)?;

        // Write manifest
        let m_path = format!("{}/manifest-{}.json", manifest_dir, receipt.court_id);
        let m_json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(&m_path, &m_json)?;

        // Compute receipt SHA-256 for index
        let mut hasher = Sha256::new();
        hasher.update(r_json.as_bytes());
        let sha = format!("{:x}", hasher.finalize());

        index.push(serde_json::json!({
            "court_id": receipt.court_id,
            "sha256": sha,
        }));
    }

    // ---- Merge with existing evidence index ----
    // Load existing index if present
    let existing_index_path = format!("{}/index.json", evidence_root);
    let mut existing_receipts: Vec<serde_json::Value> = Vec::new();
    if Path::new(&existing_index_path).exists() {
        let existing_content = std::fs::read_to_string(&existing_index_path)?;
        if let Ok(existing) = serde_json::from_str::<serde_json::Value>(&existing_content) {
            if let Some(receipts) = existing.get("receipts").and_then(|r| r.as_array()) {
                existing_receipts = receipts.clone();
            }
        }
    }

    // Merge: existing receipts first, then new Phase G receipts
    let mut merged_receipts = existing_receipts.clone();
    let existing_ids: std::collections::HashSet<String> = existing_receipts
        .iter()
        .filter_map(|r| r.get("court_id").and_then(|c| c.as_str()).map(String::from))
        .collect();

    for entry in &index {
        let court_id = entry.get("court_id").and_then(|c| c.as_str()).unwrap_or("");
        if !existing_ids.contains(court_id) {
            merged_receipts.push(entry.clone());
        }
    }

    let code_commit = std::env::var("RANS_GIT_COMMIT").unwrap_or_else(|_| {
        String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        Some(o.stdout)
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
        )
        .unwrap_or_default()
        .trim()
        .to_string()
    });

    let index_json = serde_json::json!({
        "schema_version": 1,
        "code_commit": code_commit,
        "receipts": merged_receipts,
    });

    std::fs::write(
        format!("{}/index.json", staging_root),
        serde_json::to_string_pretty(&index_json)?,
    )?;

    println!();

    if all_passed {
        // Promote staging to canonical by merging Phase G receipts into existing evidence
        // Copy Phase G receipts and manifests to canonical evidence directory
        let canonical_receipt_dir = format!("{}/receipts", evidence_root);
        let canonical_manifest_dir = format!("{}/manifests", evidence_root);
        std::fs::create_dir_all(&canonical_receipt_dir)?;
        std::fs::create_dir_all(&canonical_manifest_dir)?;

        // Copy all staging receipts to canonical
        if let Ok(entries) = std::fs::read_dir(&receipt_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let fname = entry.file_name();
                    let fname_str = fname.to_string_lossy().to_string();
                    let dest = format!("{}/{}", canonical_receipt_dir, fname_str);
                    if !Path::new(&dest).exists() {
                        std::fs::copy(entry.path(), &dest)?;
                    }
                }
            }
        }

        // Copy all staging manifests to canonical
        if let Ok(entries) = std::fs::read_dir(&manifest_dir) {
            for entry in entries {
                if let Ok(entry) = entry {
                    let fname = entry.file_name();
                    let fname_str = fname.to_string_lossy().to_string();
                    let dest = format!("{}/{}", canonical_manifest_dir, fname_str);
                    if !Path::new(&dest).exists() {
                        std::fs::copy(entry.path(), &dest)?;
                    }
                }
            }
        }

        // Write merged index to canonical
        std::fs::write(
            format!("{}/index.json", evidence_root),
            serde_json::to_string_pretty(&index_json)?,
        )?;

        println!("ALL PHASE G COURTS PASSED");
        println!("Evidence merged into: {}", evidence_root);
        println!("Receipts: {}", canonical_receipt_dir);
        println!("Manifests: {}", canonical_manifest_dir);
        println!("Next step: run `cargo xtask seal`");
        Ok(())
    } else {
        eprintln!("SOME COURTS FAILED — staging dir kept at {}", staging_root);
        std::process::exit(1);
    }
}
