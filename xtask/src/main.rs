use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo xtask <command>");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  gen               - Generate documentation (not implemented)");
        eprintln!("  check             - Verify all gates pass");
        eprintln!("  seal              - Run release-critical gates");
        eprintln!("  no-ffi            - Verify no FFI");
        eprintln!("  no-upstream-source - Verify no upstream source in production crates");
        eprintln!("  package-audit     - Verify cargo package (not implemented)");
        eprintln!("  residuals list    - List all residuals (not implemented)");
        eprintln!("  residuals verify  - Verify residuals are tracked (not implemented)");
        eprintln!("  docker preflight  - Docker non-interference preflight (not implemented)");
        eprintln!("  docker build      - Build Docker matrix images (not implemented)");
        eprintln!("  docker matrix     - Run Docker matrix (not implemented)");
        std::process::exit(1);
    }

    match args[1].as_str() {
        "gen" => {
            eprintln!("error: gate not implemented: gen");
            std::process::exit(1);
        }
        "check" => {
            let mut all_ok = true;

            // Check no-ffi
            if let Err(e) = check_no_ffi() {
                eprintln!("FAIL: no-ffi check: {}", e);
                all_ok = false;
            }

            // Check no-upstream-source in production crates
            if let Err(e) = check_no_upstream_source() {
                eprintln!("FAIL: no-upstream-source check: {}", e);
                all_ok = false;
            }

            // Check forbid(unsafe_code) in core
            if let Err(e) = check_forbid_unsafe_core() {
                eprintln!("FAIL: forbid(unsafe_code) core: {}", e);
                all_ok = false;
            }

            // Check docs/drafts/ directory exists
            if let Err(e) = check_docs_drafts_exists() {
                eprintln!("FAIL: docs/drafts check: {}", e);
                all_ok = false;
            }

            // Print actual test count from cargo test -p ryg-rans-rs-core
            if let Err(e) = check_test_count() {
                eprintln!("FAIL: test count check: {}", e);
                all_ok = false;
            }

            // Cargo tree check for facade crate (same as core check)
            if let Err(e) = check_no_ffi_facade() {
                eprintln!("FAIL: no-ffi facade check: {}", e);
                all_ok = false;
            }

            if all_ok {
                println!("All gates passed.");
            } else {
                std::process::exit(1);
            }
        }
        "seal" => {
            if let Err(e) = cmd_seal() {
                eprintln!("FAIL: seal: {}", e);
                std::process::exit(1);
            }
            println!("All seal gates passed.");
        }
        "no-ffi" => {
            if let Err(e) = check_no_ffi() {
                eprintln!("FAIL: {}", e);
                std::process::exit(1);
            }
            println!("no-ffi check passed.");
        }
        "no-upstream-source" => {
            if let Err(e) = check_no_upstream_source() {
                eprintln!("FAIL: {}", e);
                std::process::exit(1);
            }
            println!("no-upstream-source check passed.");
        }
        "package-audit" => {
            eprintln!("error: gate not implemented: package-audit");
            std::process::exit(1);
        }
        "residuals" => {
            eprintln!("error: gate not implemented: residuals");
            std::process::exit(1);
        }
        "docker" => {
            eprintln!("error: gate not implemented: docker");
            std::process::exit(1);
        }
        _ => {
            eprintln!("error: unknown command: {}", args[1]);
            std::process::exit(1);
        }
    }
}

fn check_no_ffi() -> Result<(), String> {
    // Check that no production crate links to C code
    let output = Command::new("cargo")
        .args(["tree", "-p", "ryg-rans-rs-core", "--edges", "normal"])
        .output()
        .map_err(|e| format!("cargo tree failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("cc ") || stdout.contains("gcc ") || stdout.contains("cmake ") {
        return Err(format!(
            "FFI dependency detected in ryg-rans-rs-core:\n{}",
            stdout
        ));
    }

    // Check for native library artifacts (.so, .a) in the target directory
    // Rust incremental compilation produces .o files, so we skip those.
    let target_dir = std::path::Path::new("target");
    if target_dir.exists() {
        let pred = |p: &std::path::Path| -> bool {
            p.extension().map_or(false, |e| e == "so" || e == "a")
        };
        let so_files: Vec<_> = walk_files(target_dir, &pred);
        let project_so: Vec<_> = so_files
            .into_iter()
            .filter(|p| {
                let s = p.to_string_lossy();
                s.contains("ryg_rans_rs")
            })
            .collect();
        if !project_so.is_empty() {
            return Err(format!(
                "Native library files found in target for ryg-rans-rs: {:?}",
                project_so
            ));
        }
    }

    Ok(())
}

fn check_no_upstream_source() -> Result<(), String> {
    // Check that production crate source files don't contain upstream C headers
    let prod_crates = [
        "crates/ryg-rans-rs-core",
        "crates/ryg-rans-rs-simd",
        "crates/ryg-rans-rs",
    ];

    for crate_dir in &prod_crates {
        let src_dir = std::path::Path::new(crate_dir).join("src");
        if !src_dir.exists() {
            continue;
        }
        let pred = |p: &std::path::Path| -> bool {
            p.extension()
                .map_or(false, |e| e == "c" || e == "cpp" || e == "h" || e == "hpp")
        };
        let c_files = walk_files(&src_dir, &pred);
        if !c_files.is_empty() {
            return Err(format!(
                "C/C++ source files found in {}: {:?}",
                crate_dir, c_files
            ));
        }
    }

    Ok(())
}

fn check_test_count() -> Result<(), String> {
    println!("Checking: cargo test -p ryg-rans-rs-core...");
    let output = Command::new("cargo")
        .args(["test", "-p", "ryg-rans-rs-core"])
        .output()
        .map_err(|e| format!("cargo test failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "cargo test -p ryg-rans-rs-core failed:\nstdout:{}\nstderr:{}",
            stdout, stderr
        ));
    }

    // Extract test count from the summary line: "test result: ok. N passed"
    let test_count = stdout
        .lines()
        .find(|l| l.contains("test result:") && l.contains("passed"))
        .map(|l| l.to_string())
        .unwrap_or_else(|| "unknown (summary not found)".to_string());

    println!("  Test count: {}", test_count);
    Ok(())
}

fn check_docs_drafts_exists() -> Result<(), String> {
    let drafts_dir = std::path::Path::new("docs/drafts");
    if !drafts_dir.exists() {
        return Err("docs/drafts/ directory not found".into());
    }
    if !drafts_dir.is_dir() {
        return Err("docs/drafts/ is not a directory".into());
    }
    println!("  docs/drafts/: exists");

    // Verify expected draft files are present
    let expected = [
        "residual-summary.md",
        "court-matrix.md",
        "claim-index.md",
        "port-parity.md",
        "unsafe-count.md",
    ];
    for name in &expected {
        let path = drafts_dir.join(name);
        if !path.exists() {
            return Err(format!("docs/drafts/{} not found", name));
        }
    }
    println!("  docs/drafts/: all expected draft files present");
    Ok(())
}

fn check_no_ffi_facade() -> Result<(), String> {
    println!("Checking: cargo tree -p ryg-rans-rs (no FFI deps)...");
    let output = Command::new("cargo")
        .args(["tree", "-p", "ryg-rans-rs", "--edges", "normal"])
        .output()
        .map_err(|e| format!("cargo tree failed: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.contains("cc ") || stdout.contains("gcc ") || stdout.contains("cmake ") {
        return Err(format!(
            "FFI dependency detected in ryg-rans-rs:\n{}",
            stdout
        ));
    }
    println!("  ryg-rans-rs: no CC/gcc/cmake dependencies");
    Ok(())
}

fn check_forbid_unsafe_core() -> Result<(), String> {
    let core_lib = std::path::Path::new("crates/ryg-rans-rs-core/src/lib.rs");
    if !core_lib.exists() {
        return Err("core lib.rs not found".into());
    }
    let content =
        std::fs::read_to_string(core_lib).map_err(|e| format!("read core lib.rs: {}", e))?;
    if !content.contains("#![forbid(unsafe_code)]") {
        return Err("core lib.rs missing #![forbid(unsafe_code)]".into());
    }
    Ok(())
}

fn cmd_seal() -> Result<(), String> {
    // 1. Run cargo check --workspace
    println!("Checking: cargo check --workspace...");
    let status = Command::new("cargo")
        .args(["check", "--workspace"])
        .status()
        .map_err(|e| format!("cargo check failed to execute: {}", e))?;
    if !status.success() {
        return Err("cargo check --workspace failed".into());
    }
    println!("  cargo check --workspace: passed");

    // 2. Run cargo test -p ryg-rans-rs-core
    println!("Checking: cargo test -p ryg-rans-rs-core...");
    let output = Command::new("cargo")
        .args(["test", "-p", "ryg-rans-rs-core"])
        .output()
        .map_err(|e| format!("cargo test failed to execute: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "cargo test -p ryg-rans-rs-core failed:\nstdout:{}\nstderr:{}",
            stdout, stderr
        ));
    }
    println!("  cargo test -p ryg-rans-rs-core: passed");

    // 3. Check parity.model.json exists and has well-formed JSON
    println!("Checking: docs-src/models/parity.model.json...");
    let parity_path = std::path::Path::new("docs-src/models/parity.model.json");
    if !parity_path.exists() {
        return Err("docs-src/models/parity.model.json not found".into());
    }
    let parity_content = std::fs::read_to_string(parity_path)
        .map_err(|e| format!("reading parity.model.json: {}", e))?;
    serde_json::from_str::<serde_json::Value>(&parity_content)
        .map_err(|e| format!("parity.model.json is not valid JSON: {}", e))?;
    println!("  docs-src/models/parity.model.json: valid JSON");

    // 4. Check upstream.json exists
    println!("Checking: docs-src/models/upstream.json...");
    if !std::path::Path::new("docs-src/models/upstream.json").exists() {
        return Err("docs-src/models/upstream.json not found".into());
    }
    println!("  docs-src/models/upstream.json: exists");

    // 5. Check that every 'full' surface has receipts
    println!("Checking: full surfaces have receipts...");
    let parity: serde_json::Value = serde_json::from_str(&parity_content)
        .map_err(|e| format!("re-parsing parity.model.json: {}", e))?;
    if let Some(surfaces) = parity.get("surfaces").and_then(|s| s.as_array()) {
        for surface in surfaces {
            let status = surface.get("status").and_then(|s| s.as_str()).unwrap_or("");
            let id = surface
                .get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            let receipts = surface.get("receipts").and_then(|r| r.as_array());

            if status == "full" {
                if let Some(r) = receipts {
                    if r.is_empty() {
                        return Err(format!(
                            "non-scaffold surface '{}' has an empty receipts list",
                            id
                        ));
                    }
                }
            }
        }
    }
    println!("  non-scaffold surfaces: all have receipts");

    // 5b. Verify receipt files exist on disk
    println!("Checking: receipt files exist on disk...");
    if let Some(surfaces) = parity.get("surfaces").and_then(|s| s.as_array()) {
        for surface in surfaces {
            let id = surface
                .get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            if let Some(receipts) = surface.get("receipts").and_then(|r| r.as_array()) {
                for receipt_val in receipts {
                    if let Some(receipt_id) = receipt_val.as_str() {
                        let receipt_path = format!("reports/drafts/receipt-{}.json", receipt_id);
                        if !std::path::Path::new(&receipt_path).exists() {
                            return Err(format!(
                                "receipt file missing for surface '{}': {}",
                                id, receipt_path
                            ));
                        }
                        // Validate receipt JSON has admitted_match verdict
                        let r_content = std::fs::read_to_string(&receipt_path)
                            .map_err(|e| format!("reading receipt {}: {}", receipt_path, e))?;
                        let r_json: serde_json::Value = serde_json::from_str(&r_content)
                            .map_err(|e| format!("parsing receipt {}: {}", receipt_path, e))?;
                        let verdict = r_json.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                        if verdict != "admitted_match" {
                            return Err(format!(
                                "surface '{}' cites receipt '{}' with verdict '{}', not 'admitted_match'",
                                id, receipt_id, verdict
                            ));
                        }
                    }
                }
            }
        }
    }
    println!("  all cited receipts: present and verified");

    // 6. Verify #![forbid(unsafe_code)] in core and casefile crates
    println!("Checking: #![forbid(unsafe_code)] in core crate...");
    check_forbid_unsafe("crates/ryg-rans-rs-core/src/lib.rs")?;
    println!("  core crate: has forbid(unsafe_code)");

    println!("Checking: #![forbid(unsafe_code)] in casefile crate...");
    check_forbid_unsafe("crates/ryg-rans-rs-casefile/src/lib.rs")?;
    println!("  casefile crate: has forbid(unsafe_code)");

    Ok(())
}

fn check_forbid_unsafe(path: &str) -> Result<(), String> {
    let file_path = std::path::Path::new(path);
    if !file_path.exists() {
        return Err(format!("{} not found", path));
    }
    let content =
        std::fs::read_to_string(file_path).map_err(|e| format!("read {}: {}", path, e))?;
    if !content.contains("#![forbid(unsafe_code)]") {
        return Err(format!("{} missing #![forbid(unsafe_code)]", path));
    }
    Ok(())
}

fn walk_files(
    dir: &std::path::Path,
    pred: &dyn Fn(&std::path::Path) -> bool,
) -> Vec<std::path::PathBuf> {
    let mut result = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                result.extend(walk_files(&path, pred));
            } else if pred(&path) {
                result.push(path);
            }
        }
    }
    result
}
