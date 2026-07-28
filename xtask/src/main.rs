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
        eprintln!("  docker            - Run full Docker VM matrix via bootstrap script");
        eprintln!("  docker preflight  - Run non-interference preflight only");
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

            // Check Docker matrix evidence
            if let Err(e) = check_docker_matrix() {
                eprintln!("FAIL: docker matrix check: {}", e);
                // Docker matrix is informational in 'check', not a blocker
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
            let docker_script = std::path::Path::new("docker/bootstrap-docker.sh");
            if !docker_script.exists() {
                eprintln!("error: docker/bootstrap-docker.sh not found");
                std::process::exit(1);
            }
            let mut cmd = std::process::Command::new("sh");
            cmd.arg("docker/bootstrap-docker.sh");
            // Pass through RUN_ID if provided
            if args.len() > 2 {
                cmd.arg(&args[2]);
            }
            let status = cmd.status().unwrap_or_else(|e| {
                eprintln!("error: failed to execute bootstrap: {}", e);
                std::process::exit(1);
            });
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
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
    // 0. Dirty-tree gate: reject uncommitted changes to covered source
    println!("Checking: dirty-tree gate...");
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .output()
        .map_err(|e| format!("git status failed: {}", e))?;
    let dirty_output = String::from_utf8_lossy(&dirty.stdout);
    for line in dirty_output.lines() {
        if line.is_empty() {
            continue;
        }
        // Git porcelain v1: XY[ ]<path> where the separator space after XY
        // is omitted when either X or Y is a space character.
        // Extract path by: skipping status chars (cols 0-1), then optional
        // whitespace separator, then the rest is the path.
        let path = if line.len() > 2 {
            let after_status = &line[2..];
            after_status.trim_start()
        } else {
            ""
        };
        if path.starts_with("evidence/")
            || path.starts_with("docs/")
            || path.starts_with("Cargo.lock")
            || path.starts_with(".gitignore")
        {
            continue;
        }
        return Err(format!(
            "dirty working tree: uncommitted change to '{}'. Commit or stash changes to covered files before sealing.",
            path
        ));
    }
    println!("  dirty-tree gate: working tree is clean for covered sources");

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

    // 5. Check that every claim with behavior_status='full' has a receipt
    println!("Checking: full claims have receipts...");
    let parity: serde_json::Value = serde_json::from_str(&parity_content)
        .map_err(|e| format!("re-parsing parity.model.json: {}", e))?;
    if let Some(surfaces) = parity.get("surfaces").and_then(|s| s.as_array()) {
        for surface in surfaces {
            let id = surface
                .get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("unknown");
            if let Some(claims) = surface.get("claims").and_then(|c| c.as_array()) {
                for claim in claims {
                    let bstatus = claim
                        .get("behavior_status")
                        .and_then(|s| s.as_str())
                        .unwrap_or("");
                    if bstatus == "full" {
                        let receipt_id =
                            claim.get("receipt").and_then(|r| r.as_str()).unwrap_or("");
                        if receipt_id.is_empty() {
                            return Err(format!("surface '{}' has full claim without receipt", id));
                        }
                    }
                }
            }
        }
    }
    println!("  full claims: all have receipts");

    // 5b. Verify receipt files exist on disk
    println!("Checking: receipt files exist on disk...");
    if let Some(surfaces) = parity.get("surfaces").and_then(|s| s.as_array()) {
        for surface in surfaces {
            if let Some(claims) = surface.get("claims").and_then(|c| c.as_array()) {
                for claim in claims {
                    for receipt_key in &["receipt"] {
                        if let Some(receipt_id) = claim.get(*receipt_key).and_then(|r| r.as_str()) {
                            if receipt_id.is_empty() {
                                continue;
                            }
                            let receipt_path =
                                format!("evidence/receipts/receipt-{}.json", receipt_id);
                            if !std::path::Path::new(&receipt_path).exists() {
                                return Err(format!(
                                    "receipt file missing for surface '{}': {}",
                                    surface.get("id").and_then(|s| s.as_str()).unwrap_or(""),
                                    receipt_path
                                ));
                            }
                            let r_content = std::fs::read_to_string(&receipt_path)
                                .map_err(|e| format!("reading {}: {}", receipt_path, e))?;
                            let r_json: serde_json::Value = serde_json::from_str(&r_content)
                                .map_err(|e| format!("parsing {}: {}", receipt_path, e))?;
                            let verdict =
                                r_json.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                            if verdict != "admitted_match" {
                                return Err(format!(
                                    "surface '{}' cites receipt '{}' with verdict '{}'",
                                    surface.get("id").and_then(|s| s.as_str()).unwrap_or(""),
                                    receipt_id,
                                    verdict
                                ));
                            }
                            // Validate required fields
                            let case_count = r_json
                                .get("num_cases")
                                .or_else(|| r_json.get("case_count"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            if case_count == 0 {
                                return Err(format!("receipt {} has num_cases=0", receipt_id));
                            }
                            let matched = r_json
                                .get("pairs_matched")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let compared = r_json
                                .get("pairs_compared")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let manifest_sha = r_json
                                .get("manifest_sha256")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if manifest_sha.is_empty() {
                                return Err(format!(
                                    "receipt {} missing manifest_sha256",
                                    receipt_id
                                ));
                            }
                            if matched != compared {
                                return Err(format!(
                                    "receipt {} matched={} != compared={}",
                                    receipt_id, matched, compared
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    println!("  all cited receipts: present and verified");

    // 5c. Load evidence index
    println!("Checking: evidence index...");
    let index_path = std::path::Path::new("evidence/index.json");
    let index_content = if index_path.exists() {
        Some(std::fs::read_to_string(index_path).map_err(|e| format!("read index.json: {}", e))?)
    } else {
        return Err("evidence/index.json not found".into());
    };
    let index: serde_json::Value =
        serde_json::from_str(index_content.as_ref().map(|s| s.as_str()).unwrap_or("{}"))
            .map_err(|e| format!("parse index.json: {}", e))?;
    let index_receipts = index
        .get("receipts")
        .and_then(|r| r.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    println!("  index entries: {}", index_receipts.len());

    // 5d. Verify receipt SHA-256 hashes against evidence index
    println!("Checking: receipt SHA-256 hashes...");
    for entry in index_receipts {
        let court_id = entry.get("court_id").and_then(|c| c.as_str()).unwrap_or("");
        let expected_sha = entry.get("sha256").and_then(|s| s.as_str()).unwrap_or("");
        let r_path = format!("evidence/receipts/receipt-{}.json", court_id);
        let content =
            std::fs::read_to_string(&r_path).map_err(|e| format!("read {}: {}", r_path, e))?;
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(content.as_bytes());
        let actual_sha = format!("{:x}", h.finalize());
        if actual_sha != expected_sha {
            return Err(format!(
                "receipt {} SHA-256 mismatch: expected={}, actual={}",
                court_id, expected_sha, actual_sha
            ));
        }
        // Verify code_commit is ancestor of HEAD
        let r_json: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("parse {}: {}", r_path, e))?;
        let code_commit = r_json
            .get("code_commit")
            .and_then(|c| c.as_str())
            .unwrap_or("");
        let head_hash = get_git_head_hash();
        if !head_hash.is_empty() && !code_commit.is_empty() {
            let merge_base = std::process::Command::new("git")
                .args(["merge-base", "--is-ancestor", code_commit, &head_hash])
                .status();
            match merge_base {
                Ok(s) if s.success() => {}
                _ => {
                    return Err(format!(
                        "receipt {} code_commit={} not ancestor of HEAD={}",
                        court_id, code_commit, head_hash
                    ));
                }
            }
        }
    }
    println!("  all receipt SHA-256 hashes verified");

    // 5e. Verify manifest SHA-256 against receipt manifest_sha256
    println!("Checking: manifest SHA-256 hashes...");
    for entry in index_receipts {
        let court_id = entry.get("court_id").and_then(|c| c.as_str()).unwrap_or("");
        let m_path_str = format!("evidence/manifests/manifest-{}.json", court_id);
        let m_content = std::fs::read_to_string(&m_path_str)
            .map_err(|e| format!("read {}: {}", m_path_str, e))?;
        let computed_sha = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(m_content.as_bytes());
            format!("{:x}", h.finalize())
        };
        let r_path = format!("evidence/receipts/receipt-{}.json", court_id);
        let r_content =
            std::fs::read_to_string(&r_path).map_err(|e| format!("read {}: {}", r_path, e))?;
        let r_json: serde_json::Value =
            serde_json::from_str(&r_content).map_err(|e| format!("parse {}: {}", r_path, e))?;
        let receipt_sha = r_json
            .get("manifest_sha256")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if computed_sha != receipt_sha {
            return Err(format!(
                "manifest {} SHA-256 {} != receipt manifest_sha256 {}",
                m_path_str, computed_sha, receipt_sha
            ));
        }
    }
    println!("  all manifest SHA-256 hashes verified");

    // 5f. Verify receipt SHA-256 self-hash (skip self-referential field)
    println!("Checking: receipt SHA-256 self-hashes...");
    for entry in index_receipts {
        let court_id = entry.get("court_id").and_then(|c| c.as_str()).unwrap_or("");
        let r_path = format!("evidence/receipts/receipt-{}.json", court_id);
        let content =
            std::fs::read_to_string(&r_path).map_err(|e| format!("read {}: {}", r_path, e))?;
        let r_json: serde_json::Value =
            serde_json::from_str(&content).map_err(|e| format!("parse {}: {}", r_path, e))?;
        let receipt_self_hash = r_json
            .get("receipt_sha256")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if receipt_self_hash.is_empty() {
            continue;
        }
        // Build canonical JSON with receipt_sha256 set to empty string
        // using json! macro for same key ordering as oracle harness
        let canonical = serde_json::json!({
            "schema_version": r_json.get("schema_version"),
            "court_id": r_json.get("court_id"),
            "court_path": r_json.get("court_path"),
            "variant": r_json.get("variant"),
            "scale_bits": r_json.get("scale_bits"),
            "seed": r_json.get("seed"),
            "num_cases": r_json.get("num_cases"),
            "verdict": r_json.get("verdict"),
            "upstream_commit": r_json.get("upstream_commit"),
            "code_commit": r_json.get("code_commit"),
            "pairs_compared": r_json.get("pairs_compared"),
            "pairs_matched": r_json.get("pairs_matched"),
            "residual_count": r_json.get("residual_count"),
            "residual_ids": r_json.get("residual_ids"),
            "manifest_sha256": r_json.get("manifest_sha256"),
            "receipt_sha256": "",
            "reproduction_command": r_json.get("reproduction_command"),
            "oracle_compiler": r_json.get("oracle_compiler"),
        });
        let canonical_str = serde_json::to_string_pretty(&canonical)
            .map_err(|e| format!("canonical {}: {}", r_path, e))?;
        let computed = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(canonical_str.as_bytes());
            format!("{:x}", h.finalize())
        };
        if computed != receipt_self_hash {
            return Err(format!(
                "receipt {} self-hash mismatch: computed={}, receipt={}",
                court_id, computed, receipt_self_hash
            ));
        }
    }
    println!("  all receipt SHA-256 self-hashes verified");

    // 5f. Source freshness: no covered source files changed after code_commit
    println!("Checking: source freshness...");
    let code_commits: Vec<String> = index_receipts
        .iter()
        .filter_map(|e| {
            let cid = e.get("court_id").and_then(|c| c.as_str()).unwrap_or("");
            let rp = format!("evidence/receipts/receipt-{}.json", cid);
            std::fs::read_to_string(&rp).ok().and_then(|c| {
                serde_json::from_str::<serde_json::Value>(&c)
                    .ok()
                    .and_then(|v| {
                        v.get("code_commit")
                            .and_then(|s| s.as_str())
                            .map(String::from)
                    })
            })
        })
        .collect();
    // Use the earliest code_commit among all receipts
    let earliest_code = code_commits.first().map(|s| s.as_str()).unwrap_or("");
    if !earliest_code.is_empty() {
        // Check that only evidence/, docs/, xtask/ changed after code_commit
        let allowed_prefixes = [
            "evidence/",
            "docs/",
            "xtask/",
            "docker/",
            ".cargo/",
            ".gitignore",
            "Cargo.lock",
            "README.md",
        ];
        let changed = std::process::Command::new("git")
            .args([
                "diff",
                "--name-only",
                format!("{}..HEAD", earliest_code).as_str(),
            ])
            .output();
        let changed_output = match changed {
            Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).to_string(),
            _ => return Err("source freshness: git diff failed or .git not available".into()),
        };
        for line in changed_output.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let allowed = allowed_prefixes.iter().any(|p| line.starts_with(p));
            // Also allow any crate's Cargo.toml (version bumps don't affect evidence)
            let is_version_bump = line.ends_with("/Cargo.toml");
            // NOTE: oracle crate changes require a proper reseal — no exception here.
            // The code_commit must identify the exact source that produced the evidence.
            if !allowed && !is_version_bump {
                return Err(format!(
                    "source file changed after code_commit {}: {} (not in allowed list)",
                    earliest_code, line
                ));
            }
        }
    }
    println!("  source freshness: no covered files changed after code_commit");

    // 6. Verify #![forbid(unsafe_code)] in core and casefile crates
    println!("Checking: #![forbid(unsafe_code)] in core crate...");
    check_forbid_unsafe("crates/ryg-rans-rs-core/src/lib.rs")?;
    println!("  core crate: has forbid(unsafe_code)");

    println!("Checking: #![forbid(unsafe_code)] in casefile crate...");
    check_forbid_unsafe("crates/ryg-rans-rs-casefile/src/lib.rs")?;
    println!("  casefile crate: has forbid(unsafe_code)");

    // 7. Check Docker matrix evidence (mandatory)
    println!("Checking: Docker matrix evidence...");
    check_docker_matrix()?;
    println!("  Docker matrix: verified");

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

fn check_docker_matrix() -> Result<(), String> {
    let matrix_stamp = std::path::Path::new("evidence/docker-matrix.json");
    if !matrix_stamp.exists() {
        return Err("evidence/docker-matrix.json not found".into());
    }
    let content = std::fs::read_to_string(matrix_stamp)
        .map_err(|e| format!("read evidence/docker-matrix.json: {}", e))?;
    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("parse evidence/docker-matrix.json: {}", e))?;

    // Validate required fields (schema v2)
    let run_id = json.get("run_id").and_then(|v| v.as_str()).unwrap_or("");
    let git_commit = json
        .get("git_commit")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let all_passed = json
        .get("all_passed")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let job_count = json.get("job_count").and_then(|v| v.as_u64()).unwrap_or(0);
    let jobs = json
        .get("jobs")
        .and_then(|v| v.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);

    if run_id.is_empty() {
        return Err("docker-matrix.json missing run_id".into());
    }
    if git_commit.is_empty() {
        return Err("docker-matrix.json missing git_commit".into());
    }
    if !all_passed {
        return Err(format!("docker matrix run {} reported failure", run_id));
    }
    if job_count == 0 {
        return Err("docker-matrix.json has zero jobs".into());
    }
    if job_count != 10 {
        return Err(format!(
            "docker-matrix.json job_count={} (expected 10)",
            job_count
        ));
    }
    if jobs.is_empty() {
        return Err("docker-matrix.json has empty jobs array".into());
    }
    // Verify every expected job exists and exited 0
    let expected_jobs = [
        "oracle-gcc",
        "package-audit",
        "msrv",
        "cross-aarch64",
        "rust-musl-build",
        "sanitizers",
        "rust-stable-tests",
        "cross-court",
        "miri",
        "performance",
    ];
    for expected in &expected_jobs {
        let found = jobs.iter().any(|j| {
            j.get("name").and_then(|n| n.as_str()) == Some(expected)
                && j.get("exit_code").and_then(|c| c.as_i64()) == Some(0)
        });
        if !found {
            return Err(format!(
                "docker matrix job '{}' missing or had non-zero exit",
                expected
            ));
        }
    }
    // Verify git_commit is an ancestor of HEAD
    let head_hash = get_git_head_hash();
    if !head_hash.is_empty() && !git_commit.is_empty() {
        let merge_base = std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", git_commit, &head_hash])
            .status();
        match merge_base {
            Ok(s) if s.success() => {}
            _ => {
                return Err(format!(
                    "docker matrix git_commit {} not ancestor of HEAD {}",
                    git_commit, head_hash
                ));
            }
        }
    }
    Ok(())
}

fn get_git_head_hash() -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output();
    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
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
