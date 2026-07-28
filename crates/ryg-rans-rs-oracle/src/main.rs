use ryg_rans_rs_oracle::{CourtConfig, CourtPath, ModelProfile};
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
    let _num_cases: usize = args.get(4).map(|s| s.parse().unwrap_or(20)).unwrap_or(20);

    let evidence_root =
        std::env::var("RANS_EVIDENCE_DIR").unwrap_or_else(|_| "evidence".to_string());
    let receipt_dir = format!("{}/receipts", evidence_root);
    let manifest_dir = format!("{}/manifests", evidence_root);
    std::fs::create_dir_all(&receipt_dir)?;
    std::fs::create_dir_all(&manifest_dir)?;

    // Build court configurations
    let court_configs = build_court_configs();

    let mut index = Vec::new();
    let mut all_passed = true;

    for config in &court_configs {
        // For ScaleSweep, run at each scale_bits value from 10 to 16
        let scales: &[u32] = if config.profile == ModelProfile::ScaleSweep {
            &[10, 11, 13, 14, 15, 16]
        } else {
            &[config.profile.scale_bits().unwrap_or(scale_bits)]
        };
        for &scale in scales {
            let (receipt, _manifest, manifest_bytes) = ryg_rans_rs_oracle::run_court_with_profile(
                oracle,
                scale,
                seed,
                config.path,
                config.variant,
                config.profile,
            )?;

            println!("--- Court: {} ---", receipt.court_id);
            println!(
                "  Verdict: {}  ({}/{})  residuals={}",
                receipt.verdict,
                receipt.pairs_matched,
                receipt.pairs_compared,
                receipt.residual_count
            );
            println!("  Profile: {}", receipt.profile);
            println!("  scale_bits: {}", receipt.scale_bits);
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

            // Add to index
            if let Ok(content) = std::fs::read_to_string(&r_path) {
                use sha2::Digest;
                let mut h = sha2::Sha256::new();
                h.update(content.as_bytes());
                let sha = format!("{:x}", h.finalize());
                index.push(serde_json::json!({
                    "court_id": receipt.court_id,
                    "sha256": sha,
                }));
            }
        } // end for &scale
    }

    // Write evidence index
    std::fs::write(
        format!("{}/index.json", evidence_root),
        serde_json::to_string_pretty(&serde_json::json!({
            "schema_version": 1,
            "code_commit": std::env::var("RANS_GIT_COMMIT").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| {
                std::process::Command::new("git")
                    .args(["rev-parse", "HEAD"]).output().ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            }),
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

fn build_court_configs() -> Vec<CourtConfig> {
    let mut configs = Vec::new();
    let variants = ["byte", "r64"];
    let paths = [CourtPath::Division, CourtPath::Reciprocal];
    let profiles = [
        ModelProfile::Uniform256,
        ModelProfile::Freq1,
        ModelProfile::Skewed2551,
        ModelProfile::Sparse2,
        ModelProfile::Sparse17,
        ModelProfile::PrimeResidue,
        ModelProfile::RenormBoundary,
        ModelProfile::LengthBoundary,
    ];

    // Standard profiles at scale_bits=12
    for &variant in &variants {
        for &path in &paths {
            for &profile in &profiles {
                configs.push(CourtConfig {
                    variant,
                    path,
                    profile,
                });
            }
        }
    }

    // Scale sweep: Uniform256 at scale_bits 10, 11, 13, 14, 15, 16
    for &variant in &variants {
        for &path in &paths {
            configs.push(CourtConfig {
                variant,
                path,
                profile: ModelProfile::ScaleSweep,
            });
        }
    }

    configs
}
