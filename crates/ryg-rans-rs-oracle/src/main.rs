use ryg_rans_rs_oracle::{CaseManifest, CourtPath, Receipt};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let oracle = args
        .get(1)
        .map(|s| s.as_str())
        .filter(|p| Path::new(p).exists())
        .or_else(|| {
            let alt = "../oracle/adapter/rans_trace";
            if Path::new(alt).exists() {
                Some(alt)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            eprintln!("ERROR: oracle not found. Build it: cd oracle/adapter && make");
            std::process::exit(1);
        })?;

    let scale_bits: u32 = args.get(2).map(|s| s.parse().unwrap_or(12)).unwrap_or(12);
    let seed: u64 = args.get(3).map(|s| s.parse().unwrap_or(42)).unwrap_or(42);
    let num_cases: usize = args.get(4).map(|s| s.parse().unwrap_or(20)).unwrap_or(20);

    let evidence_root =
        std::env::var("RANS_EVIDENCE_DIR").unwrap_or_else(|_| "evidence".to_string());
    let receipt_dir = format!("{}/receipts", evidence_root);
    let manifest_dir = format!("{}/manifests", evidence_root);
    std::fs::create_dir_all(&receipt_dir)?;
    std::fs::create_dir_all(&manifest_dir)?;

    // Run all 4 courts
    let court_configs: Vec<(
        &str,
        CourtPath,
        fn(&str, u32, u64, usize, CourtPath) -> Result<(Receipt, CaseManifest, Vec<u8>), String>,
    )> = vec![
        (
            "BYTE.DIVISION",
            CourtPath::Division,
            ryg_rans_rs_oracle::run_byte_court,
        ),
        (
            "BYTE.RECIPROCAL",
            CourtPath::Reciprocal,
            ryg_rans_rs_oracle::run_byte_court,
        ),
        (
            "R64.DIVISION",
            CourtPath::Division,
            ryg_rans_rs_oracle::run_r64_court,
        ),
        (
            "R64.RECIPROCAL",
            CourtPath::Reciprocal,
            ryg_rans_rs_oracle::run_r64_court,
        ),
    ];

    let mut all_passed = true;

    for (name, path, court_fn) in &court_configs {
        println!("--- Court: {} ---", name);
        let (receipt, manifest, manifest_bytes) =
            court_fn(oracle, scale_bits, seed, num_cases, *path)?;
        println!(
            "  Verdict: {}  ({}/{})  residuals={}",
            receipt.verdict, receipt.pairs_matched, receipt.pairs_compared, receipt.residual_count
        );
        println!("  Manifest SHA-256: {}", receipt.manifest_sha256);
        println!("  Receipt SHA-256:  {}", receipt.receipt_sha256);

        if receipt.verdict != "admitted_match" {
            all_passed = false;
        }

        // Write receipt and manifest
        let r_path = format!("{}/receipt-{}.json", receipt_dir, receipt.court_id);
        std::fs::write(&r_path, serde_json::to_string_pretty(&receipt)?)?;
        println!("  Receipt: {}", r_path);

        let m_path = format!("{}/manifest-{}.json", manifest_dir, receipt.court_id);
        std::fs::write(&m_path, &manifest_bytes)?;
        println!("  Manifest: {}", m_path);
        println!();
    }

    // Generate evidence index
    let mut index = Vec::new();
    for (name, path, _) in &court_configs {
        let court_id = format!("RYG_RANS.{}.SINGLE_STATE.UNIFORM256.S{}", name, scale_bits);
        let r_path = format!("{}/receipt-{}.json", receipt_dir, court_id);
        if let Ok(content) = std::fs::read_to_string(&r_path) {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(content.as_bytes());
            let sha = format!("{:x}", h.finalize());
            index.push(serde_json::json!({
                "court_id": court_id,
                "sha256": sha,
            }));
        }
    }
    std::fs::write(
        format!("{}/index.json", evidence_root),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "code_commit": std::process::Command::new("git")
                .args(["rev-parse", "HEAD"]).output().ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            "receipts": index,
        }))?,
    )?;
    println!("Evidence index: {}/index.json", evidence_root);

    if all_passed {
        println!("ALL COURTS PASSED");
        Ok(())
    } else {
        eprintln!("SOME COURTS FAILED");
        std::process::exit(1);
    }
}
