use ryg_rans_rs_oracle::{CaseManifest, CourtConfig, CourtPath, ModelProfile, Receipt};
use std::path::Path;

enum CourtMode {
    SingleState,
    Interleaved2,
}

struct FullConfig {
    variant: &'static str,
    path: CourtPath,
    profile: ModelProfile,
    mode: CourtMode,
}

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
        let scales: &[u32] = if config.profile == ModelProfile::ScaleSweep {
            &[10, 11, 13, 14, 15, 16]
        } else {
            &[config.profile.scale_bits().unwrap_or(scale_bits)]
        };
        for &scale in scales {
            let (receipt, _manifest, manifest_bytes) = if config.variant == "alias" {
                match config.mode {
                    CourtMode::SingleState => {
                        ryg_rans_rs_oracle::run_alias_court(oracle, scale, seed, config.profile)?
                    }
                    CourtMode::Interleaved2 => ryg_rans_rs_oracle::run_alias_interleaved_court(
                        oracle,
                        scale,
                        seed,
                        config.profile,
                    )?,
                }
            } else {
                match config.mode {
                    CourtMode::SingleState => ryg_rans_rs_oracle::run_court_with_profile(
                        oracle,
                        scale,
                        seed,
                        config.path,
                        config.variant,
                        config.profile,
                    )?,
                    CourtMode::Interleaved2 => ryg_rans_rs_oracle::run_interleaved_court(
                        oracle,
                        scale,
                        seed,
                        config.path,
                        config.variant,
                        config.profile,
                    )?,
                }
            };

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
        }
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

/// Return the supported court paths for a given variant.
/// Word rANS has only one path (division-based encode + table decode).
fn supported_paths(variant: &str) -> &'static [CourtPath] {
    match variant {
        "byte" | "r64" => &[CourtPath::Division, CourtPath::Reciprocal],
        "word" => &[CourtPath::Division],
        "alias" => &[CourtPath::Division],
        _ => &[],
    }
}

fn build_court_configs() -> Vec<FullConfig> {
    let mut configs = Vec::new();
    let variants = ["byte", "r64", "word", "alias"];
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

    // Single-state standard profiles at scale_bits=12
    for &variant in &variants {
        for &path in supported_paths(variant) {
            for &profile in &profiles {
                configs.push(FullConfig {
                    variant,
                    path,
                    profile,
                    mode: CourtMode::SingleState,
                });
            }
        }
    }

    // Single-state scale sweep (word rANS uses fixed scale_bits=12 per upstream)
    for &variant in &["byte", "r64"] {
        for &path in supported_paths(variant) {
            configs.push(FullConfig {
                variant,
                path,
                profile: ModelProfile::ScaleSweep,
                mode: CourtMode::SingleState,
            });
        }
    }

    // Interleaved2: interleaved courts for all variants
    for &variant in &variants {
        for &path in supported_paths(variant) {
            for &profile in &profiles {
                if profile == ModelProfile::ScaleSweep {
                    continue;
                }
                configs.push(FullConfig {
                    variant,
                    path,
                    profile,
                    mode: CourtMode::Interleaved2,
                });
            }
        }
    }

    configs
}
