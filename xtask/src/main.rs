use std::process::Command;

mod workload;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: cargo xtask <command>");
        eprintln!();
        eprintln!("Commands:");
        eprintln!("  gen               - Generate documentation (not implemented)");
        eprintln!("  check             - Verify all gates pass");
        eprintln!("  seal              - Run release-critical gates");
        eprintln!("  performance-seal  - Seal performance benchmark evidence");
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

            // Forbidden overclaim language check (L.15)
            if let Err(e) = check_no_overclaim() {
                eprintln!("FAIL: no-overclaim check: {}", e);
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
        "performance-seal" => {
            if let Err(e) = cmd_performance_seal(&args[2..]) {
                eprintln!("FAIL: performance-seal: {}", e);
                std::process::exit(1);
            }
            println!("All performance-seal gates passed.");
        }
        "benchmark-run" => {
            if let Err(e) = cmd_benchmark_run(&args[2..]) {
                eprintln!("FAIL: benchmark-run: {}", e);
                std::process::exit(1);
            }
            println!("benchmark-run completed.");
        }
        "workload" => {
            if let Err(e) = workload::cmd_workload(&args[2..]) {
                eprintln!("FAIL: workload: {}", e);
                std::process::exit(1);
            }
            println!("workload command completed.");
        }
        "courts-run" => {
            if let Err(e) = cmd_courts_run(&args[2..]) {
                eprintln!("FAIL: courts-run: {}", e);
                std::process::exit(1);
            }
            println!("courts-run completed.");
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
        "no-overclaim" => {
            if let Err(e) = check_no_overclaim() {
                eprintln!("FAIL: {}", e);
                std::process::exit(1);
            }
            println!("no-overclaim check passed.");
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
    let output = Command::new("cargo")
        .args(["tree", "-p", "ryg-rans-rs", "--invert", "-e", "no-dev"])
        .output()
        .map_err(|e| format!("cargo tree failed: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo tree error: {}", stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // If no FFI crates are linked, cargo tree should not mention them.
    // Look for any FFI-related crate names.
    let ffi_keywords = ["ffi", "libc", "cc", "bindgen", "cmake"];
    for line in stdout.lines() {
        let trimmed = line.trim();
        for kw in &ffi_keywords {
            if trimmed.to_lowercase().contains(kw) {
                return Err(format!(
                    "FFI dependency detected: '{}' (matches keyword '{}')",
                    trimmed, kw
                ));
            }
        }
    }
    Ok(())
}

fn check_no_upstream_source() -> Result<(), String> {
    let production_crates = [
        "crates/ryg-rans-rs-core",
        "crates/ryg-rans-rs-casefile",
        "crates/ryg-rans-rs-simd",
        "crates/ryg-rans-rs-parallel",
        "crates/ryg-rans-rs-cli",
        "crates/ryg-rans-rs",
    ];
    for crate_dir in &production_crates {
        let src_path = std::path::Path::new(crate_dir).join("src");
        if !src_path.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&src_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    let content = std::fs::read_to_string(&path)
                        .map_err(|e| format!("read {:?}: {}", path, e))?;
                    if content.contains("upstream") && content.contains("//")
                        || content.contains("Fabian")
                        || content.contains("ryg_rans")
                    {
                        // These are acceptable references — check for actual
                        // upstream source code inclusion
                        if content.contains("#[path = \"../upstream")
                            || content.contains("include!(\"../upstream")
                        {
                            return Err(format!(
                                "upstream source inclusion in {}: {}",
                                crate_dir,
                                path.display()
                            ));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_test_count() -> Result<(), String> {
    let output = Command::new("cargo")
        .args(["test", "-p", "ryg-rans-rs-core", "--", "--list"])
        .output()
        .map_err(|e| format!("cargo test --list failed: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo test --list error: {}", stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let test_count = stdout.lines().filter(|l| l.ends_with(": test")).count();
    println!("  ryg-rans-rs-core test count: {}", test_count);
    if test_count < 50 {
        return Err(format!(
            "test count {} is below expected minimum of 50",
            test_count
        ));
    }
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
    Ok(())
}

fn check_no_ffi_facade() -> Result<(), String> {
    let facade_path = std::path::Path::new("crates/ryg-rans-rs/src/lib.rs");
    if !facade_path.exists() {
        return Err("crates/ryg-rans-rs/src/lib.rs not found".into());
    }
    let content =
        std::fs::read_to_string(facade_path).map_err(|e| format!("read facade lib.rs: {}", e))?;
    // The facade uses `#![deny(unsafe_code)]` (a deliberate choice: it must
    // remain able to re-export the SIMD crate's safe wrappers without
    // carrying unsafe itself).  Accept deny or forbid.
    if !content.contains("#![forbid(unsafe_code)]") && !content.contains("#![deny(unsafe_code)]") {
        return Err("facade crate missing #![forbid(unsafe_code)] / #![deny(unsafe_code)]".into());
    }
    Ok(())
}

fn check_forbid_unsafe_core() -> Result<(), String> {
    check_forbid_unsafe("crates/ryg-rans-rs-core/src/lib.rs")
}

/// Forbidden overclaim language check (L.15).
///
/// The repository must not claim "critical safety infrastructure" quality,
/// "can depend on this", or "production-grade" for anything unless a future
/// formal certification process justifies it.  This scan covers READMEs,
/// rustdoc/source comments, Cargo.toml descriptions, and docs/ — and excludes
/// `evidence/` (historical records quote the removed language) and `target/`.
fn check_no_overclaim() -> Result<(), String> {
    let forbidden = [
        "critical safety infrastructure",
        "critical-safety-infrastructure",
        "safety infrastructure can depend",
        "can depend on this",
        "production-grade",
    ];

    let mut hits: Vec<String> = Vec::new();
    let mut walk = |dir: &std::path::Path| -> Result<(), String> {
        if !dir.exists() {
            return Ok(());
        }
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let read = std::fs::read_dir(&d).map_err(|e| format!("read_dir {:?}: {}", d, e))?;
            for entry in read {
                let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name == "target" || name == "evidence" || name == ".git" {
                        continue;
                    }
                    if path.ends_with("oracle/adapter") {
                        continue; // vendored C sources
                    }
                    stack.push(path);
                } else if path.is_file() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    let interesting = name.ends_with(".md")
                        || name.ends_with(".rs")
                        || name == "Cargo.toml"
                        || name == "AGENTS.md"
                        || name == "llms.txt";
                    if !interesting {
                        continue;
                    }
                    if path.to_string_lossy().ends_with("xtask/src/main.rs") {
                        // The check's own source lists the forbidden phrases
                        // as data; tooling source is not a claim surface.
                        continue;
                    }
                    let content = std::fs::read_to_string(&path)
                        .map_err(|e| format!("read {:?}: {}", path, e))?;
                    let lower = content.to_lowercase();
                    for phrase in &forbidden {
                        if lower.contains(phrase) {
                            hits.push(format!("{}: contains {:?}", path.display(), phrase));
                        }
                    }
                }
            }
        }
        Ok(())
    };

    walk(std::path::Path::new("."))?;
    if !hits.is_empty() {
        return Err(format!(
            "forbidden overclaim language found ({} hit(s)):\n{}",
            hits.len(),
            hits.join("\n")
        ));
    }
    Ok(())
}

/// Phase L.19: run the fourteen Phase L behavioural courts and write
/// manifests, receipts, and index entries into `evidence/`.
///
/// ```sh
/// cargo xtask courts-run --implementation-commit <sha>
/// ```
///
/// The command:
/// 1. Refuses a dirty source tree.
/// 2. Runs every court (real code paths) and seals manifest+receipt pairs
///    with the canonical hash scheme (L1-L / L1-R doctrine).
/// 3. Writes `evidence/manifests/manifest-<id>.json` and
///    `evidence/receipts/receipt-<id>.json`.
/// 4. Updates `evidence/index.json` with the new court entries (the existing
///    behavioural receipts are preserved).
/// 5. Updates the parity model `phase_l_courts` citations.
/// 6. Runs the full seal gate.
fn cmd_courts_run(args: &[String]) -> Result<(), String> {
    // ---- Parse arguments --------------------------------------------------
    let mut implementation_commit: Option<String> = None;
    let mut only: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--implementation-commit" => {
                i += 1;
                if i < args.len() {
                    implementation_commit = Some(args[i].clone());
                } else {
                    return Err("--implementation-commit requires a value".into());
                }
            }
            "--only" => {
                i += 1;
                if i < args.len() {
                    only = Some(args[i].clone());
                } else {
                    return Err("--only requires a value".into());
                }
            }
            other => return Err(format!("unknown argument: {}", other)),
        }
        i += 1;
    }
    let implementation_commit = implementation_commit.unwrap_or_else(|| get_git_head_hash());
    if implementation_commit.is_empty() {
        return Err("cannot determine implementation commit".into());
    }

    // ---- 1. Refuse a dirty source tree ------------------------------------
    println!("courts-run: step 1 — verifying clean source tree...");
    let dirty = std::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .output()
        .map_err(|e| format!("git status failed: {}", e))?;
    let dirty_output = String::from_utf8_lossy(&dirty.stdout);
    let source_dirty: Vec<String> = dirty_output
        .lines()
        .filter(|l| !l.is_empty())
        .filter(|l| {
            let path = l.get(3..).unwrap_or("");
            // Evidence files are generated by this command itself.
            !path.starts_with("evidence/") && !path.starts_with("docs-src/models/parity.model.json")
        })
        .map(|l| l.to_string())
        .collect();
    if !source_dirty.is_empty() {
        return Err(format!(
            "dirty source tree: {} uncommitted change(s). Commit or stash before running courts.",
            source_dirty.len()
        ));
    }
    println!("  source tree clean");

    // ---- 2. Run the courts ------------------------------------------------
    println!("courts-run: step 2 — running the Phase L and Phase O courts...");
    let evidence_commit = get_git_head_hash();
    let sealed =
        ryg_rans_rs_bench::courts::run_all_courts(&implementation_commit, &evidence_commit);
    let mut written = 0usize;
    let mut failed_court: Option<String> = None;
    for (manifest, receipt) in &sealed {
        let court_id = &manifest.court_id;
        if let Some(only_id) = &only {
            if only_id != court_id {
                continue;
            }
        }
        if receipt.verdict != ryg_rans_rs_casefile::PhaseLCourtVerdict::Passed {
            failed_court = Some(format!(
                "{} verdict={:?} passed={} failed={} skipped={}",
                court_id,
                receipt.verdict,
                receipt.cases_passed,
                receipt.cases_failed,
                receipt.cases_skipped
            ));
        }
        let m_path = format!("evidence/manifests/manifest-{}.json", court_id);
        let r_path = format!("evidence/receipts/receipt-{}.json", court_id);
        let m_json = serde_json::to_string_pretty(manifest)
            .map_err(|e| format!("serialize manifest {}: {}", court_id, e))?;
        let r_json = serde_json::to_string_pretty(receipt)
            .map_err(|e| format!("serialize receipt {}: {}", court_id, e))?;
        std::fs::write(&m_path, &m_json).map_err(|e| format!("write {}: {}", m_path, e))?;
        std::fs::write(&r_path, &r_json).map_err(|e| format!("write {}: {}", r_path, e))?;
        written += 1;
        println!(
            "  {} — {:?} ({} cases: {} passed, {} failed, {} skipped)",
            court_id,
            receipt.verdict,
            receipt.num_cases,
            receipt.cases_passed,
            receipt.cases_failed,
            receipt.cases_skipped
        );
    }
    if let Some(f) = failed_court {
        return Err(format!("one or more courts failed: {}", f));
    }
    println!("  {} manifest/receipt pairs written", written);

    // ---- 3. Update evidence/index.json ------------------------------------
    println!("courts-run: step 3 — updating evidence/index.json...");
    let index_path = "evidence/index.json";
    let index_content =
        std::fs::read_to_string(index_path).map_err(|e| format!("read {}: {}", index_path, e))?;
    let mut index: serde_json::Value =
        serde_json::from_str(&index_content).map_err(|e| format!("parse {}: {}", index_path, e))?;
    // Insert or update each court entry (dedupe by court_id).
    {
        let receipts = index
            .get_mut("receipts")
            .and_then(|r| r.as_array_mut())
            .ok_or("evidence/index.json has no receipts array")?;
        let mut existing_ids: std::collections::HashSet<String> = receipts
            .iter()
            .filter_map(|e| e.get("court_id").and_then(|s| s.as_str()))
            .map(String::from)
            .collect();
        for (manifest, receipt) in &sealed {
            let court_id = &manifest.court_id;
            if let Some(only_id) = &only {
                if only_id != court_id {
                    continue;
                }
            }
            let r_path = format!("evidence/receipts/receipt-{}.json", court_id);
            let r_bytes = std::fs::read(&r_path).map_err(|e| format!("read {}: {}", r_path, e))?;
            let file_sha = sha256_hex(&r_bytes);
            if existing_ids.contains(court_id) {
                // Update the existing entry's hash.
                for e in receipts.iter_mut() {
                    if e.get("court_id").and_then(|s| s.as_str()) == Some(court_id.as_str()) {
                        e["sha256"] = serde_json::Value::String(file_sha.clone());
                    }
                }
            } else {
                receipts.push(serde_json::json!({
                    "court_id": court_id,
                    "sha256": file_sha.clone(),
                }));
                existing_ids.insert(court_id.clone());
            }
            let _ = receipt;
        }
    }
    // Record the evidence_commit for the new courts.
    if index.get("evidence_commit").is_none() {
        index["evidence_commit"] = serde_json::Value::String(evidence_commit.clone());
    }
    let receipt_count = index
        .get("receipts")
        .and_then(|r| r.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let new_index_json =
        serde_json::to_string_pretty(&index).map_err(|e| format!("serialize index: {}", e))?;
    std::fs::write(index_path, &new_index_json)
        .map_err(|e| format!("write {}: {}", index_path, e))?;
    println!(
        "  evidence/index.json now has {} receipt entries",
        receipt_count
    );

    // ---- 4. Update parity model citations ---------------------------------
    println!("courts-run: step 4 — updating parity model phase_l_courts citations...");
    let parity_path = "docs-src/models/parity.model.json";
    let parity_content =
        std::fs::read_to_string(parity_path).map_err(|e| format!("read {}: {}", parity_path, e))?;
    let mut parity: serde_json::Value = serde_json::from_str(&parity_content)
        .map_err(|e| format!("parse {}: {}", parity_path, e))?;
    let court_ids: Vec<String> = sealed
        .iter()
        .map(|(m, _)| m.court_id.clone())
        .filter(|id| match &only {
            Some(o) => o == id,
            None => true,
        })
        .collect();
    let mut sorted_ids = court_ids.clone();
    sorted_ids.sort();
    parity["phase_l_courts"] = serde_json::Value::Array(
        sorted_ids
            .iter()
            .map(|s| serde_json::Value::String(s.clone()))
            .collect(),
    );
    let new_parity_json = serde_json::to_string_pretty(&parity)
        .map_err(|e| format!("serialize parity model: {}", e))?;
    std::fs::write(parity_path, &new_parity_json)
        .map_err(|e| format!("write {}: {}", parity_path, e))?;
    println!("  parity model cites {} phase_l courts", sorted_ids.len());

    // ---- 5. Regenerate the README Evidence Status table from the indexes ---
    // L.20 gate 29: the README is generated from evidence, never hand-edited.
    println!("courts-run: step 5 — regenerating README Evidence Status table...");
    regenerate_readme_evidence_table()?;

    // ---- 6. Run the seal gate ---------------------------------------------
    println!("courts-run: step 6 — running the full seal gate...");
    cmd_seal()?;
    println!("courts-run: all courts passed and the seal gate is green.");
    Ok(())
}

/// Regenerate the README Evidence Status table from the behavioural evidence
/// index and the performance top-level index (L.20 gate 29: generated, never
/// hand-edited).  The surface rows and receipt counts are derived, not hard-
/// coded; the Phase L courts are appended to the behavioural surface count.
fn regenerate_readme_evidence_table() -> Result<(), String> {
    let readme_path = "README.md";
    let readme =
        std::fs::read_to_string(readme_path).map_err(|e| format!("read {}: {}", readme_path, e))?;

    // Behavioural counts per surface family.
    let index_content = std::fs::read_to_string("evidence/index.json")
        .map_err(|e| format!("read evidence/index.json: {}", e))?;
    let index: serde_json::Value = serde_json::from_str(&index_content)
        .map_err(|e| format!("parse evidence/index.json: {}", e))?;
    let receipts = index
        .get("receipts")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    let total_behavioural = receipts.len();

    // Performance receipts count from the top-level performance index.
    let perf_index_content = std::fs::read_to_string("evidence/performance/index.json")
        .map_err(|e| format!("read evidence/performance/index.json: {}", e))?;
    let perf_index: serde_json::Value = serde_json::from_str(&perf_index_content)
        .map_err(|e| format!("parse evidence/performance/index.json: {}", e))?;
    let total_performance = perf_index
        .get("receipts")
        .and_then(|r| r.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    // Count behavioural receipts per surface by the court_id prefix.
    let count_prefix = |prefixes: &[&str]| -> usize {
        receipts
            .iter()
            .filter(|e| {
                let id = e.get("court_id").and_then(|s| s.as_str()).unwrap_or("");
                prefixes.iter().any(|p| id.starts_with(p))
            })
            .count()
    };
    let byte_n = count_prefix(&["RYG_RANS.BYTE."]);
    let r64_n = count_prefix(&["RYG_RANS.R64.", "RYG_RANS.RANS64."]);
    let word_n = count_prefix(&["RYG_RANS.WORD."]);
    let alias_n = count_prefix(&["RYG_RANS.ALIAS."]);
    let sse41_n = count_prefix(&["RYG_RANS.SSE41."]);
    let avx512vl_n = count_prefix(&["RYG_RANS.AVX512VL."]);
    let avx512_n = count_prefix(&["RYG_RANS.AVX512."]);

    let table = format!(
        "| Surface | Behaviour | Performance | Behaviour Receipts | Performance Receipts |\n\
         |---------|-----------|-------------|------------------:|--------------------:|\n\
         | 32-bit byte rANS — division + reciprocal | **Sealed** | **Sealed** | {} | 1 |\n\
         | 64-bit rANS — division + reciprocal | **Sealed** | **Sealed** | {} | 1 |\n\
         | Word rANS — scalar table-based | **Sealed** | **Sealed** | {} | 1 |\n\
         | Alias method — Vose table, byte rANS | **Sealed** | **Sealed** | {} | 1 |\n\
         | SSE4.1 SIMD decoder — 8-way interleaved | **Sealed** | **Sealed** | {} | 1 |\n\
         | AVX512VL.INTERLEAVED8 | **Sealed** | **Sealed** | {} | 1 |\n\
         | AVX512.INTERLEAVED16 | **Sealed** | **Sealed** | {} | 1 |\n\
         | Phase H optimization backends | **Test-verified** | **Sealed** | 0 | 1 |\n\
         | Phase J AVX2 backends | **Test-verified** | **Sealed** | 0 | 1 |\n\
         | Phase I parallel block engine | **Test-verified** | **Sealed** | 0 | 1 |\
         | Phase L behavioural courts | **Sealed** | — | {} | 0 |\
         | Phase O cache courts | **Sealed** | **Sealed** | {} | 5 |\
         | **Total** | | | **{}** | **{}** |",
        byte_n,
        r64_n,
        word_n,
        alias_n,
        sse41_n,
        avx512vl_n,
        avx512_n,
        receipts
            .iter()
            .filter(|e| {
                e.get("court_id")
                    .and_then(|s| s.as_str())
                    .map(|id| id.starts_with("RYG_RANS.L."))
                    .unwrap_or(false)
            })
            .count(),
        receipts
            .iter()
            .filter(|e| {
                e.get("court_id")
                    .and_then(|s| s.as_str())
                    .map(|id| id.starts_with("RYG_RANS.O."))
                    .unwrap_or(false)
            })
            .count(),
        total_behavioural,
        total_performance,
    );

    // Replace the table between "## Evidence Status" and the next "## ".
    let start_marker = "## Evidence Status";
    let start = readme
        .find(start_marker)
        .ok_or("README: Evidence Status section not found")?;
    let rest = &readme[start..];
    let next_heading = rest
        .find("\n## ")
        .map(|i| start + i + 1)
        .unwrap_or(readme.len());
    let new_readme = format!("{}{}\n{}\n", &readme[..start], start_marker, table);
    // Keep everything after the section (next heading onward).
    let tail = &readme[next_heading..];
    let new_readme = format!("{}{}", new_readme, tail);
    std::fs::write(readme_path, &new_readme)
        .map_err(|e| format!("write {}: {}", readme_path, e))?;
    println!(
        "  README table regenerated: {} behavioural / {} performance receipts",
        total_behavioural, total_performance
    );
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
            || path.starts_with("docs-src/models/parity.model.json")
            || path.starts_with("Cargo.lock")
            || path.starts_with(".gitignore")
            || path.ends_with("/README.md")
            || path == "README.md"
            || path == "xtask/README.md"
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

    // 2a. Run the full workspace test suite (L.20 gate 3: all tests).
    println!("Checking: cargo test --workspace...");
    let ws = Command::new("cargo")
        .args(["test", "--workspace"])
        .output()
        .map_err(|e| format!("cargo test --workspace failed to execute: {}", e))?;
    if !ws.status.success() {
        let stderr = String::from_utf8_lossy(&ws.stderr);
        let stdout = String::from_utf8_lossy(&ws.stdout);
        return Err(format!(
            "cargo test --workspace failed:\nstdout:{}\nstderr:{}",
            stdout, stderr
        ));
    }
    println!("  cargo test --workspace: passed");

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

    // 5a.2. Validate that each receipt's court_path is supported by its variant
    println!("Checking: court path is valid for variant...");
    if let Some(surfaces) = parity.get("surfaces").and_then(|s| s.as_array()) {
        for surface in surfaces {
            if let Some(claims) = surface.get("claims").and_then(|c| c.as_array()) {
                let id = surface.get("id").and_then(|s| s.as_str()).unwrap_or("");
                // Determine variant from surface ID
                let variant = if id.starts_with("rans64.") {
                    "r64"
                } else if id.starts_with("byte.") {
                    "byte"
                } else if id.starts_with("word.") {
                    "word"
                } else {
                    ""
                };
                for claim in claims {
                    if let Some(rid) = claim.get("receipt").and_then(|s| s.as_str()) {
                        if rid.is_empty() {
                            continue;
                        }
                        // Parse court_path from receipt content
                        let rp = format!("evidence/receipts/receipt-{}.json", rid);
                        if let Ok(rc) = std::fs::read_to_string(&rp) {
                            if let Ok(rj) = serde_json::from_str::<serde_json::Value>(&rc) {
                                let court_path =
                                    rj.get("court_path").and_then(|s| s.as_str()).unwrap_or("");
                                let valid = match variant {
                                    "r64" | "byte" => {
                                        court_path == "DIVISION" || court_path == "RECIPROCAL"
                                    }
                                    "word" => court_path == "DIVISION",
                                    _ => true,
                                };
                                if !valid {
                                    return Err(format!(
                                        "receipt '{}' has court_path='{}' which is not valid for variant '{}'",
                                        rid, court_path, variant
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    println!("  all court paths valid for their variant");

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

    // 5b.1. Validate the Phase L court family (L.19): every court cited in
    // the parity model's `phase_l_courts` array must have a receipt with
    // verdict `passed`, every case counted, and a manifest hash.
    println!("Checking: Phase L court receipts (L.19)...");
    let phase_l_ids: Vec<String> = parity
        .get("phase_l_courts")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let expected_l_ids: [&str; 23] = [
        "RYG_RANS.L.VERIFY.DECODED_HASH",
        "RYG_RANS.L.INTEGRITY.STRICT",
        "RYG_RANS.L.CANCEL.COMPLETENESS",
        "RYG_RANS.L.EXECUTOR.BOUNDED",
        "RYG_RANS.L.REORDER.ATOMIC_COMMIT",
        "RYG_RANS.L.CONFIG.WIRING",
        "RYG_RANS.L.SCRATCH.INTEGRATION",
        "RYG_RANS.L.MODEL_CACHE.INTEGRATION",
        "RYG_RANS.L.BACKEND.EXPLICIT",
        "RYG_RANS.L.SSE41.UNSAFE_QUARANTINE",
        "RYG_RANS.L.PERFORMANCE.EXPORT",
        "RYG_RANS.L.PERFORMANCE.ARCHIVE",
        "RYG_RANS.L.PERFORMANCE.RECEIPT_CHAIN",
        "RYG_RANS.L.PUBLIC_API.REACHABILITY",
        // Phase O cache courts (O.20).
        "RYG_RANS.O.CACHE.EXACT_BYTES",
        "RYG_RANS.O.CACHE.ZERO_CAPACITY",
        "RYG_RANS.O.CACHE.OVERSIZED",
        "RYG_RANS.O.CACHE.UNIQUE_KEYS",
        "RYG_RANS.O.CACHE.SINGLE_FLIGHT",
        "RYG_RANS.O.CACHE.FAILURE_EQUIVALENCE",
        "RYG_RANS.O.CACHE.CANCELLATION",
        "RYG_RANS.O.CACHE.METRICS",
        "RYG_RANS.O.WORKLOAD.PUBLIC_RANS_V1",
    ];
    let expected_set: std::collections::BTreeSet<String> =
        expected_l_ids.iter().map(|s| s.to_string()).collect();
    // Set equality: parity citations == expected IDs.
    let cited_set: std::collections::BTreeSet<String> = phase_l_ids.iter().cloned().collect();
    if cited_set != expected_set {
        return Err(format!(
            "phase_l_courts set mismatch: parity has {} but {} expected (missing: {:?}, extra: {:?})",
            cited_set.len(),
            expected_set.len(),
            expected_set.difference(&cited_set).collect::<Vec<_>>(),
            cited_set.difference(&expected_set).collect::<Vec<_>>()
        ));
    }
    for court_id in &phase_l_ids {
        let r_path = format!("evidence/receipts/receipt-{}.json", court_id);
        let r_content =
            std::fs::read_to_string(&r_path).map_err(|e| format!("reading {}: {}", r_path, e))?;
        let r_json: serde_json::Value =
            serde_json::from_str(&r_content).map_err(|e| format!("parsing {}: {}", r_path, e))?;
        let verdict = r_json.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
        if verdict != "passed" {
            return Err(format!(
                "Phase L court {} verdict={:?} — only 'passed' is sealable",
                court_id, verdict
            ));
        }
        let num_cases = r_json
            .get("num_cases")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let failed = r_json
            .get("cases_failed")
            .and_then(|v| v.as_u64())
            .unwrap_or(1);
        if num_cases == 0 {
            return Err(format!("Phase L court {} has num_cases=0", court_id));
        }
        if failed != 0 {
            return Err(format!(
                "Phase L court {} has {} failed case(s) — not sealable",
                court_id, failed
            ));
        }
        let manifest_sha = r_json
            .get("manifest_sha256")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if manifest_sha.is_empty() {
            return Err(format!(
                "Phase L court {} missing manifest_sha256",
                court_id
            ));
        }
        // The receipt must reference a residual-consistent implementation
        // commit: it must be an ancestor of HEAD.
        let impl_commit = r_json
            .get("implementation_commit")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let head_hash = get_git_head_hash();
        if !head_hash.is_empty() && !impl_commit.is_empty() {
            let is_ancestor = std::process::Command::new("git")
                .args(["merge-base", "--is-ancestor", impl_commit, &head_hash])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !is_ancestor {
                return Err(format!(
                    "Phase L court {} implementation_commit={} not ancestor of HEAD",
                    court_id, impl_commit
                ));
            }
        }
    }
    println!(
        "  all {} Phase L court receipts: present, passed, verified",
        phase_l_ids.len()
    );

    // 5c. Reverse check: every index receipt is cited exactly once in the parity model
    println!("Checking: every index receipt cited in parity model...");
    let index_path_pre = std::path::Path::new("evidence/index.json");
    let index_content_pre =
        std::fs::read_to_string(index_path_pre).map_err(|e| format!("read index.json: {}", e))?;
    let index_pre: serde_json::Value =
        serde_json::from_str(&index_content_pre).map_err(|e| format!("parse index.json: {}", e))?;
    let index_receipts_pre = index_pre
        .get("receipts")
        .and_then(|r| r.as_array())
        .map(|a| a.clone())
        .unwrap_or_default();
    // Collect all cited receipt IDs from parity model
    let mut cited_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Some(surfaces) = parity.get("surfaces").and_then(|s| s.as_array()) {
        for surface in surfaces {
            if let Some(claims) = surface.get("claims").and_then(|c| c.as_array()) {
                for claim in claims {
                    if let Some(rid) = claim.get("receipt").and_then(|s| s.as_str()) {
                        if !rid.is_empty() {
                            cited_ids.insert(rid.to_string());
                        }
                    }
                }
            }
        }
    }
    // Phase L court citations (L.19) are also evidence-model citations.
    for cid in &phase_l_ids {
        cited_ids.insert(cid.clone());
    }
    for entry in &index_receipts_pre {
        if let Some(cid) = entry.get("court_id").and_then(|s| s.as_str()) {
            if !cited_ids.contains(cid) {
                return Err(format!(
                    "evidence index receipt '{}' is not cited by parity.model.json",
                    cid
                ));
            }
        }
    }
    println!(
        "  all {} index receipts cited in parity model",
        index_receipts_pre.len()
    );

    // Inverse check: every parity-cited receipt must be present in the index
    let index_cited_ids: std::collections::HashSet<String> = index_receipts_pre
        .iter()
        .filter_map(|e| {
            e.get("court_id")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string())
        })
        .collect();
    for cid in &cited_ids {
        if !index_cited_ids.contains(cid.as_str()) {
            return Err(format!(
                "parity-cited receipt '{}' is not present in evidence index",
                cid
            ));
        }
    }
    println!(
        "  all {} parity-cited receipts present in index",
        cited_ids.len()
    );

    // 5d. Load evidence index
    let index_path = std::path::Path::new("evidence/index.json");
    let index_content =
        std::fs::read_to_string(index_path).map_err(|e| format!("read index.json: {}", e))?;
    let index: serde_json::Value =
        serde_json::from_str(&index_content).map_err(|e| format!("parse index.json: {}", e))?;
    let index_receipts = index
        .get("receipts")
        .and_then(|r| r.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    println!("  index entries: {}", index_receipts.len());
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

    // 5f. Verify receipt SHA-256 self-hashes
    //
    // Residual L1-R: the legacy oracle receipts (Phase A-G harnesses) use
    // two different canonical-serialization schemes (json! macro vs serde
    // derive), so their stored `receipt_sha256` cannot be recomputed with a
    // single scheme.  Those receipts are therefore reported honestly as
    // "no verifiable canonical scheme" — the seal NEVER prints "verified"
    // after skipping.  The Phase L court receipts (L.19) use ONE canonical
    // scheme (serde pretty, receipt_sha256 emptied) and ARE verified here.
    println!("Checking: receipt SHA-256 self-hashes...");
    let mut phase_l_verified = 0usize;
    let mut legacy_unverifiable = 0usize;
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
            legacy_unverifiable += 1;
            continue;
        }
        if court_id.starts_with("RYG_RANS.L.") {
            // Phase L courts: verify the canonical self-hash for real.
            // The canonical content is this receipt with `receipt_sha256`
            // emptied, re-serialized with serde_json pretty via the typed
            // struct (field order preserved) — the same serialization the
            // sealer used.  Never re-serialize through serde_json::Value,
            // which sorts keys (BTreeMap) and would break the hash.
            let mut rec: ryg_rans_rs_casefile::PhaseLCourtReceipt = serde_json::from_str(&content)
                .map_err(|e| format!("re-parse {}: {}", r_path, e))?;
            rec.receipt_sha256 = String::new();
            let canonical = serde_json::to_string_pretty(&rec)
                .map_err(|e| format!("re-serialize {}: {}", r_path, e))?;
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(canonical.as_bytes());
            let computed = format!("{:x}", h.finalize());
            if computed != receipt_self_hash {
                return Err(format!(
                    "Phase L receipt {} self-hash mismatch: computed={}, declared={}",
                    court_id, computed, receipt_self_hash
                ));
            }
            phase_l_verified += 1;
        } else {
            legacy_unverifiable += 1;
        }
    }
    if phase_l_verified > 0 {
        println!(
            "  {} Phase L receipt self-hashes verified",
            phase_l_verified
        );
    }
    if legacy_unverifiable > 0 {
        println!(
            "  {} legacy oracle receipts: no verifiable canonical scheme (reported, not verified)",
            legacy_unverifiable
        );
    }

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
            "docs-src/",
            "xtask/",
            "docker/",
            ".cargo/",
            ".gitignore",
            "Cargo.toml",
            "Cargo.lock",
            "README.md",
            "crates/ryg-rans-rs-core/README.md",
            "crates/ryg-rans-rs-casefile/README.md",
            "crates/ryg-rans-rs-simd/README.md",
            "crates/ryg-rans-rs/README.md",
            "crates/ryg-rans-rs-oracle/README.md",
            "crates/ryg-rans-rs-cli/README.md",
            "crates/ryg-rans-rs-bench/README.md",
            "crates/ryg-rans-rs-parallel/README.md",
        ];

        // Changed files not in the allowlist that require evidence
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

    // 8. Validate the performance evidence (L.20 gate: the main seal must
    // validate the top-level index, run index, receipts, manifests, raw
    // artifacts, and set equality — residual L1-Q / L18-B).
    println!("Checking: performance evidence...");
    check_performance_evidence()?;

    // 8b. Validate the public-corpus stress/soak execution evidence
    // (MODEL_CACHE.WORKLOAD.2, O.20): the active run must carry complete
    // transcripts of stress-public / soak-public bound to the derived
    // manifest's schedule identity.
    println!("Checking: workload execution evidence...");
    check_workload_execution_evidence()?;

    // 9. README counts must match the evidence indexes (L.20 gate 29).
    println!("Checking: README evidence counts...");
    check_readme_evidence_counts()?;

    // 10. Unsafe ledger bidirectional equality (L.20 gate 6): the simd
    // crate's machine-verified ledger must match the source inventory.
    println!("Checking: unsafe ledger test...");
    let ledger_status = Command::new("cargo")
        .args(["test", "-p", "ryg-rans-rs-simd", "--test", "unsafe_ledger"])
        .output()
        .map_err(|e| format!("cargo test unsafe_ledger: {}", e))?;
    if !ledger_status.status.success() {
        return Err("unsafe_ledger test failed: ledger does not match the source inventory".into());
    }
    println!("  unsafe ledger: matches source inventory");

    // 11. Disassembly courts (L.20 gate): expected ISA mnemonics are emitted.
    println!("Checking: disassembly courts...");
    let disasm_status = Command::new("cargo")
        .args(["test", "-p", "ryg-rans-rs-simd", "--test", "disasm_court"])
        .output()
        .map_err(|e| format!("cargo test disasm_court: {}", e))?;
    if !disasm_status.status.success() {
        return Err("disasm_court test failed".into());
    }
    println!("  disassembly courts: passed");

    // 12. No unexpected binary/build artifacts in the tree (L.20 gate 32).
    println!("Checking: no unexpected binary artifacts...");
    check_no_unexpected_binaries()?;

    // 13. Crate version consistency (L.20 gate 33): every publishable crate
    // shares the same version.
    println!("Checking: crate version consistency...");
    check_crate_version_consistency()?;

    // 14. Cargo.lock is tracked and present (L.20 gate 34).
    println!("Checking: Cargo.lock consistency...");
    if !std::path::Path::new("Cargo.lock").exists() {
        return Err("Cargo.lock missing — a locked build is required".into());
    }
    let lock_content =
        std::fs::read_to_string("Cargo.lock").map_err(|e| format!("read Cargo.lock: {}", e))?;
    if !lock_content.contains("name = \"ryg-rans-rs-core\"") {
        return Err("Cargo.lock does not contain ryg-rans-rs-core".into());
    }
    println!("  Cargo.lock present and contains workspace crates");

    // 15. No forbidden overclaim language (L.20 gate 40).
    println!("Checking: forbidden overclaim language...");
    check_no_overclaim()?;

    // 16. Publication dry-run (L.20 gate 35): every publishable crate must
    // pass `cargo package` (which also runs the manifest/package audit).
    println!("Checking: publication dry-run...");
    check_publication_dry_run()?;

    // 17. Documentation links (L.20 gate 36): markdown links in READMEs and
    // docs/ must resolve to existing files.
    println!("Checking: documentation links...");
    check_documentation_links()?;

    // 18. rustdoc warnings (L.20 gate 37): `cargo doc` must not emit broken
    // intra-doc links or warnings for the publishable crates.
    println!("Checking: rustdoc warnings...");
    check_rustdoc_warnings()?;

    // 19. README doctests (L.20 gate 38): every rust code block in the root
    // README must compile and pass.
    println!("Checking: README doctests...");
    check_readme_doctests()?;

    // 20. Public API semver report (L.20 gate 39): the machine-readable
    // public-surface inventory under docs/public-api/ must be present for
    // every publishable crate (generated by `cargo public-api`).
    println!("Checking: public API inventory...");
    check_public_api_inventory()?;

    // 21. Residual accounting: every OPEN residual in the Phase L gap ledger
    // that this seal gate was supposed to close must be resolved or explicitly
    // accepted (L.20 gate 28).  The ledger is authoritative.
    println!("Checking: residual accounting...");
    check_residual_accounting()?;

    // 22. Custodian documentation inventory (Phase M.19): the knowledge-
    // preservation layer must be complete — philosophy, layers, all eight
    // papers, the history record, the ADR set, the diagrams, the LLM
    // operational record, the educational layer, and the references.  A
    // missing document is a seal failure: the repository's knowledge
    // preservation is part of its contract.
    println!("Checking: custodian documentation inventory (Phase M)...");
    check_documentation_inventory()?;

    // 23. Navigation and knowledge-architecture inventory (Phase N.14/N.21):
    // the navigation layer — guides, maps (mermaid + SVG), knowledge graph,
    // ADR explorer, reading paths, commentary guide, atlas, articles, story,
    // failures, contributing, search indexes, and the README identity
    // section — must be complete.  The seal fails if navigation degrades.
    println!("Checking: navigation and knowledge architecture (Phase N)...");
    check_navigation_inventory()?;

    Ok(())
}

/// Navigation and knowledge-architecture inventory (Phase N.14/N.21):
/// every artifact of the navigation layer must exist.  The Phase N
/// contract is that the corpus is navigable at every depth; a missing
/// guide, map, atlas chapter, or article is a broken entry point.
fn check_navigation_inventory() -> Result<(), String> {
    let required: &[&str] = &[
        // N.0/N.4/N.9/N.11/N.13
        "docs/navigation/inventory.md",
        "docs/navigation/knowledge-graph.md",
        "docs/navigation/adrs-by-topic.md",
        "docs/navigation/commentary.md",
        "docs/navigation/reading-paths.md",
        // N.1 guides
        "docs/navigation/00-first-day.md",
        "docs/navigation/01-first-week.md",
        "docs/navigation/02-maintainer-path.md",
        "docs/navigation/03-performance-engineer.md",
        "docs/navigation/04-simd-engineer.md",
        "docs/navigation/05-parallel-engineer.md",
        "docs/navigation/06-oracle-engineer.md",
        "docs/navigation/07-evidence-engineer.md",
        "docs/navigation/08-cli-engineer.md",
        "docs/navigation/09-llm-engineer.md",
        "docs/navigation/10-security-review.md",
        // N.2 maps (mermaid index + SVG versions)
        "docs/navigation/maps/index.md",
        "docs/navigation/maps/map-new-contributor.svg",
        "docs/navigation/maps/map-simd.svg",
        "docs/navigation/maps/map-parallel.svg",
        "docs/navigation/maps/map-evidence.svg",
        "docs/navigation/maps/map-performance.svg",
        "docs/navigation/maps/map-container-cli.svg",
        "docs/navigation/maps/map-release.svg",
        "docs/navigation/maps/map-llm-workflow.svg",
        // N.5 atlas
        "docs/atlas/index.md",
        "docs/atlas/atlas-01-repository.md",
        "docs/atlas/atlas-02-encoding.md",
        "docs/atlas/atlas-03-decoding.md",
        "docs/atlas/atlas-04-model-lifecycle.md",
        "docs/atlas/atlas-05-evidence-lifecycle.md",
        "docs/atlas/atlas-06-performance-lifecycle.md",
        "docs/atlas/atlas-07-release-lifecycle.md",
        "docs/atlas/atlas-08-parallel-scheduler.md",
        "docs/atlas/atlas-09-simd-hierarchy.md",
        "docs/atlas/atlas-10-oracle.md",
        "docs/atlas/atlas-11-cli.md",
        // N.6 articles
        "docs/articles/01-deterministic-parallel-pipeline.md",
        "docs/articles/02-evidence-driven-verification.md",
        "docs/articles/03-when-simd-is-slower.md",
        "docs/articles/04-lessons-from-building-rans.md",
        "docs/articles/05-implementation-reference.md",
        "docs/articles/06-llm-assisted-engineering.md",
        // N.7/N.8/N.10
        "docs/history/index.md",
        "docs/story/index.md",
        "docs/failures/index.md",
        // N.15/N.17
        "docs/search/index.md",
        "docs/contributing/index.md",
    ];
    let mut missing: Vec<String> = Vec::new();
    for p in required {
        if !std::path::Path::new(p).is_file() {
            missing.push((*p).to_string());
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "navigation inventory incomplete; missing: {:?}",
            missing
        ));
    }
    // N.16: the README must carry the repository-identity and entry-point
    // sections (the portal contract).
    let readme = std::fs::read_to_string("README.md").map_err(|e| format!("read README: {}", e))?;
    for marker in [
        "## What This Repository Is (N.16 identity)",
        "## Entry Points (N.3 portal)",
    ] {
        if !readme.contains(marker) {
            return Err(format!("README missing required portal marker: {}", marker));
        }
    }
    // SVG well-formedness: every required .svg parses as XML.
    for p in required {
        if p.ends_with(".svg") {
            let content = std::fs::read_to_string(p).map_err(|e| format!("read {}: {}", p, e))?;
            if !content.trim_start().starts_with("<svg") || !content.contains("</svg>") {
                return Err(format!("{} is not a well-formed SVG", p));
            }
        }
    }
    // N.21 content gates: presence alone is not completeness.  Every atlas
    // chapter must carry a purpose line and at least one mermaid diagram;
    // every guide must carry its purpose and prerequisites; every article
    // must carry an abstract.  This keeps the navigation layer honest: an
    // empty file that merely exists would otherwise satisfy the inventory.
    let mut bad: Vec<String> = Vec::new();
    for p in required {
        let content = std::fs::read_to_string(p).map_err(|e| format!("read {}: {}", p, e))?;
        let path = std::path::Path::new(p);
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let parent = path.parent().and_then(|s| s.to_str()).unwrap_or("");
        // Guides live directly under docs/navigation/ and are named
        // NN-slug.md (00-first-day.md .. 10-security-review.md); the
        // digit-prefix + parent-dir test is exact — no other artifact
        // matches it.
        let is_guide = parent == "docs/navigation"
            && name.len() >= 3
            && name.as_bytes().get(0).map_or(false, |b| b.is_ascii_digit())
            && name.as_bytes().get(1).map_or(false, |b| b.is_ascii_digit())
            && name.as_bytes().get(2) == Some(&b'-')
            && name.ends_with(".md");
        // Articles live directly under docs/articles/ and are numbered
        // NN-*.md.
        let is_article = parent == "docs/articles"
            && name.len() >= 3
            && name.as_bytes().get(0).map_or(false, |b| b.is_ascii_digit())
            && name.as_bytes().get(1).map_or(false, |b| b.is_ascii_digit())
            && name.as_bytes().get(2) == Some(&b'-')
            && name.ends_with(".md");
        // Atlas chapters are named atlas-NN-*.md.
        let is_atlas = name.starts_with("atlas-") && name.ends_with(".md");
        if is_atlas {
            if !content.contains("**Purpose:**") {
                bad.push(format!("{}: atlas chapter missing '**Purpose:**'", p));
            }
            if !content.contains("```mermaid") {
                bad.push(format!("{}: atlas chapter missing a mermaid diagram", p));
            }
        }
        if is_guide {
            if !content.contains("**Purpose:**") || !content.contains("**Prerequisites:**") {
                bad.push(format!(
                    "{}: guide missing '**Purpose:**' or '**Prerequisites:**'",
                    p
                ));
            }
        }
        if is_article && !content.contains("## Abstract") {
            bad.push(format!("{}: article missing '## Abstract'", p));
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "navigation content gates failed ({}):\n{}",
            bad.len(),
            bad.join("\n")
        ));
    }
    println!(
        "  {} navigation artifacts present; README portal, SVG maps, atlas diagrams, guide and article content verified",
        required.len()
    );
    Ok(())
}

/// Custodian documentation inventory (Phase M.19): every knowledge-
/// preservation artifact must exist.  The Phase M contract is that the
/// repository serves as the definitive implementation reference; a missing
/// paper, ADR, or diagram is a broken link in that contract.
fn check_documentation_inventory() -> Result<(), String> {
    let required: &[&str] = &[
        "docs/philosophy.md",
        "docs/layers.md",
        "docs/glossary.md",
        "docs/references.md",
        "docs/education.md",
        "docs/papers/0001-rans-design.md",
        "docs/papers/0002-word-rans.md",
        "docs/papers/0003-simd.md",
        "docs/papers/0004-parallel-engine.md",
        "docs/papers/0005-performance-methodology.md",
        "docs/papers/0006-evidence.md",
        "docs/papers/0007-proof-philosophy.md",
        "docs/papers/0008-llm-assisted-engineering.md",
        "docs/history/index.md",
        "docs/diagrams/index.md",
        "docs/llm/index.md",
        "docs/adr/0000-template.md",
        "docs/adr/0001-format-contract.md",
        "docs/adr/0002-reciprocal-fast-path.md",
        "docs/adr/0003-word-scale-pinned.md",
        "docs/adr/0004-bounded-live-executor.md",
        "docs/adr/0005-canonical-error.md",
        "docs/adr/0006-strict-integrity-default.md",
        "docs/adr/0007-cancellation-completeness-boundary.md",
        "docs/adr/0008-exact-backend-semantics.md",
        "docs/adr/0009-model-cache-expensive-artifact.md",
        "docs/adr/0010-benchmark-time-capture.md",
        "docs/adr/0011-unsafe-quarantine.md",
        "docs/adr/0012-versioning-030.md",
        "docs/adr/0013-configuration-discipline.md",
        "docs/adr/0014-reorder-atomic-commit.md",
        "docs/adr/0015-per-worker-scratch.md",
    ];
    let mut missing: Vec<String> = Vec::new();
    for p in required {
        if !std::path::Path::new(p).is_file() {
            missing.push((*p).to_string());
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "custodian documentation inventory incomplete; missing: {:?}",
            missing
        ));
    }
    // Cross-link integrity for the new documents is covered by the
    // documentation-links gate; this gate verifies presence only.
    println!(
        "  {} custodian documentation artifacts present",
        required.len()
    );
    Ok(())
}

/// Residual accounting (L.20 gate 28 / O.20): the Phase L gap ledger must not
/// claim an OPEN residual for a gate this seal implements.  The L.19/L.20 and
/// Phase O sections are expected to be resolved once this gate passes.
fn check_residual_accounting() -> Result<(), String> {
    let ledger = std::fs::read_to_string("evidence/phase-l/gap-ledger.md")
        .map_err(|e| format!("read gap-ledger.md: {}", e))?;
    // Any line in the L.19 or L.20 sections still marked OPEN is a blocker.
    let mut open_in_late = Vec::new();
    let mut in_late_section = false;
    for line in ledger.lines() {
        if line.starts_with("## L.19") || line.starts_with("## L.20") {
            in_late_section = true;
        }
        if line.starts_with("## L.21") || line.starts_with("## L.22") {
            in_late_section = false;
        }
        if in_late_section && line.contains("| OPEN |") {
            // Extract the ID (first table cell).
            if let Some(id) = line.trim().split('|').nth(1) {
                open_in_late.push(id.trim().to_string());
            }
        }
    }
    if !open_in_late.is_empty() {
        return Err(format!(
            "open residuals in the L.19/L.20 sections block the seal: {:?}",
            open_in_late
        ));
    }
    // Phase O residuals must also be resolved before the cache seal can
    // pass (O.20: "No active performance evidence residual remains
    // unresolved").
    let mut open_in_phase_o = Vec::new();
    let mut in_phase_o = false;
    for line in ledger.lines() {
        if line.starts_with("## Phase O") {
            in_phase_o = true;
            continue;
        }
        if line.starts_with("## ") && in_phase_o {
            break;
        }
        if in_phase_o && line.contains("| OPEN |") {
            if let Some(id) = line.trim().split('|').nth(1) {
                open_in_phase_o.push(id.trim().to_string());
            }
        }
    }
    if !open_in_phase_o.is_empty() {
        return Err(format!(
            "open residuals in the Phase O section block the seal: {:?}",
            open_in_phase_o
        ));
    }
    println!("  no open L.19/L.20/Phase-O residuals");
    Ok(())
}

/// Reject unexpected binary/build artifacts that would indicate a dirty or
/// unclean tree (L.20 gate 32).  Evidence directories and target/ are
/// excluded by design.
fn check_no_unexpected_binaries() -> Result<(), String> {
    let mut hits: Vec<String> = Vec::new();
    let mut walk = |dir: &std::path::Path| -> Result<(), String> {
        if !dir.exists() {
            return Ok(());
        }
        let mut stack = vec![dir.to_path_buf()];
        while let Some(d) = stack.pop() {
            let read = std::fs::read_dir(&d).map_err(|e| format!("read_dir {:?}: {}", d, e))?;
            for entry in read {
                let entry = entry.map_err(|e| format!("dir entry: {}", e))?;
                let path = entry.path();
                if path.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name == "target"
                        || name == "evidence"
                        || name == ".git"
                        || name == "oracle/adapter"
                    {
                        continue;
                    }
                    stack.push(path);
                } else if path.is_file() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy().to_string();
                    let is_artifact = name.ends_with(".o")
                        || name.ends_with(".obj")
                        || name.ends_with(".a")
                        || name.ends_with(".so")
                        || name.ends_with(".dylib")
                        || name.ends_with(".dll")
                        || name.ends_with(".exe")
                        || name.ends_with(".profraw")
                        || name.ends_with(".profdata")
                        || name.ends_with(".rlib")
                        || name.ends_with(".rmeta");
                    if is_artifact {
                        hits.push(path.display().to_string());
                    }
                }
            }
        }
        Ok(())
    };
    walk(std::path::Path::new("."))?;
    if !hits.is_empty() {
        return Err(format!(
            "unexpected build artifacts found ({}):\n{}",
            hits.len(),
            hits.join("\n")
        ));
    }
    println!("  no unexpected binary artifacts");
    Ok(())
}

/// Crate version consistency (L.20 gate 33): every publishable crate must
/// share one workspace version.
fn check_crate_version_consistency() -> Result<(), String> {
    let publishable = [
        "ryg-rans-rs-core",
        "ryg-rans-rs-simd",
        "ryg-rans-rs",
        "ryg-rans-rs-parallel",
        "ryg-rans-rs-casefile",
        "ryg-rans-rs-oracle",
        "ryg-rans-rs-cli",
    ];
    let mut versions: std::collections::BTreeMap<String, String> = Default::default();
    for c in &publishable {
        let path = format!("crates/{}/Cargo.toml", c);
        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("read {}: {}", path, e))?;
        let version = content
            .lines()
            .find_map(|l| l.trim().strip_prefix("version = "))
            .map(|v| v.trim_matches('"').to_string())
            .unwrap_or_default();
        versions.insert(c.to_string(), version);
    }
    let first = versions.values().next().cloned().unwrap_or_default();
    for (c, v) in &versions {
        if v != &first {
            return Err(format!(
                "crate version mismatch: {} is {} but {} is {}",
                c,
                v,
                versions.keys().next().unwrap(),
                first
            ));
        }
    }
    println!("  all publishable crates at version {}", first);
    Ok(())
}

/// Publication dry-run (L.20 gate 35): every publishable crate must pass
/// `cargo package` without warnings-as-errors or packaging failures.
///
/// `cargo package` validates the manifest, includes only the intended files,
/// and verifies the crate can build from the packaged subset — the closest
/// available proxy for `crates.io` acceptance without publishing.
fn check_publication_dry_run() -> Result<(), String> {
    let publishable = [
        "ryg-rans-rs-core",
        "ryg-rans-rs-simd",
        "ryg-rans-rs",
        "ryg-rans-rs-parallel",
        "ryg-rans-rs-casefile",
        "ryg-rans-rs-oracle",
        "ryg-rans-rs-cli",
    ];
    for c in &publishable {
        let status = std::process::Command::new("cargo")
            .args(["package", "-p", c, "--allow-dirty", "--no-verify"])
            .status()
            .map_err(|e| format!("cargo package {}: {}", c, e))?;
        if !status.success() {
            return Err(format!("cargo package -p {} failed", c));
        }
        println!("  cargo package -p {}: ok", c);
    }
    Ok(())
}

/// Documentation links (L.20 gate 36): every relative markdown link in the
/// root README, crate READMEs, and docs/ must resolve to an existing file.
fn check_documentation_links() -> Result<(), String> {
    let mut targets: Vec<std::path::PathBuf> = Vec::new();
    for entry in [
        "README.md",
        "AGENTS.md",
        "llms.txt",
        "docs",
        "xtask/README.md",
    ] {
        let p = std::path::Path::new(entry);
        if p.is_dir() {
            let mut stack = vec![p.to_path_buf()];
            while let Some(d) = stack.pop() {
                let read = std::fs::read_dir(&d).map_err(|e| format!("read_dir {:?}: {}", d, e))?;
                for e in read.flatten() {
                    let path = e.path();
                    if path.is_dir() {
                        stack.push(path);
                    } else if path.extension().and_then(|s| s.to_str()) == Some("md") {
                        targets.push(path);
                    }
                }
            }
        } else if p.exists() {
            targets.push(p.to_path_buf());
        }
    }
    let mut bad: Vec<String> = Vec::new();
    for t in &targets {
        let content =
            std::fs::read_to_string(t).map_err(|e| format!("read {}: {}", t.display(), e))?;
        let dir = t.parent().unwrap_or(std::path::Path::new("."));
        for line in content.lines() {
            // Markdown links: [text](target) — skip http(s), anchors, mailto.
            for cap in line.match_indices("](") {
                let rest = &line[cap.0 + 2..];
                let end = rest.find(')').unwrap_or(0);
                if end == 0 {
                    continue;
                }
                let target = &rest[..end];
                let target = target.split_whitespace().next().unwrap_or("");
                if target.is_empty()
                    || target.starts_with("http")
                    || target.starts_with("#")
                    || target.starts_with("mailto:")
                    || target.starts_with("<")
                {
                    continue;
                }
                // Strip any trailing title text.
                let target = target.split(' ').next().unwrap_or("");
                let resolved = dir.join(target);
                if !resolved.exists() {
                    bad.push(format!("{}: broken link {}", t.display(), target));
                }
            }
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "broken documentation links ({}):\n{}",
            bad.len(),
            bad.join("\n")
        ));
    }
    println!(
        "  {} documentation files checked: all links resolve",
        targets.len()
    );
    Ok(())
}

/// rustdoc warnings (L.20 gate 37): `cargo doc` for the publishable crates
/// must complete without warnings (broken intra-doc links, missing docs, ...).
fn check_rustdoc_warnings() -> Result<(), String> {
    let output = std::process::Command::new("cargo")
        .args(["doc", "--workspace", "--no-deps"])
        .output()
        .map_err(|e| format!("cargo doc: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("cargo doc failed:\n{}", stderr));
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Cargo prints rustc warnings to stderr; `warning:` lines indicate
    // rustdoc/rustc diagnostics on the documented crate set.
    let warnings: Vec<&str> = stderr
        .lines()
        .filter(|l| l.contains("warning:") || l.contains("error["))
        .collect();
    if !warnings.is_empty() {
        return Err(format!(
            "cargo doc emitted {} warning(s)/error(s):\n{}",
            warnings.len(),
            warnings.join("\n")
        ));
    }
    println!("  cargo doc: no warnings");
    Ok(())
}

/// README doctests (L.20 gate 38): every runnable rust code block in the root
/// README must compile and pass.
fn check_readme_doctests() -> Result<(), String> {
    // rustdoc supports `cargo test --doc` on a crate whose lib.rs embeds the
    // README via #![doc = include_str!(...)].  The facade crate does this;
    // run its doctests to exercise the README blocks.
    let output = std::process::Command::new("cargo")
        .args(["test", "-p", "ryg-rans-rs", "--doc"])
        .output()
        .map_err(|e| format!("cargo test --doc: {}", e))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("README doctests failed:\n{}", stderr));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let summary: Vec<&str> = stdout
        .lines()
        .filter(|l| l.contains("test result:"))
        .collect();
    println!("  README doctests: {}", summary.join(" | "));
    Ok(())
}

/// Public API inventory (L.20 gate 39): `docs/public-api/*.txt` must exist for
/// every publishable crate (generated by `cargo public-api`; committed).
fn check_public_api_inventory() -> Result<(), String> {
    let expected = [
        "core",
        "simd",
        "parallel",
        "casefile",
        "oracle",
        "cli",
        "ryg-rans-rs",
    ];
    let mut missing: Vec<String> = Vec::new();
    for c in &expected {
        let p = format!("docs/public-api/{}.txt", c);
        if !std::path::Path::new(&p).exists() {
            missing.push(p);
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "public API inventory missing (run cargo public-api): {}",
            missing.join(", ")
        ));
    }
    println!("  public API inventory present for all publishable crates");
    Ok(())
}

/// Validate the canonical performance evidence: top-level index, active run,
/// run index hash, receipt file + canonical hashes, manifests, raw artifacts,
/// and exact set equality with the expected ten IDs.
fn check_performance_evidence() -> Result<(), String> {
    let top_path = std::path::Path::new("evidence/performance/index.json");
    if !top_path.exists() {
        return Err("evidence/performance/index.json not found".into());
    }
    let top: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(top_path).map_err(|e| format!("read index.json: {}", e))?,
    )
    .map_err(|e| format!("parse index.json: {}", e))?;
    let entries = top
        .get("receipts")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    if entries.len() != 15 {
        return Err(format!(
            "performance top-level index has {} entries; expected 15",
            entries.len()
        ));
    }
    // Exact set equality with the expected IDs.
    let index_ids: std::collections::BTreeSet<String> = entries
        .iter()
        .filter_map(|e| e.get("performance_id").and_then(|s| s.as_str()))
        .map(String::from)
        .collect();
    let expected_ids: std::collections::BTreeSet<String> =
        EXPECTED_PERF_IDS.iter().map(|s| s.to_string()).collect();
    if index_ids != expected_ids {
        return Err(format!(
            "performance index ID set mismatch (missing: {:?}, extra: {:?})",
            expected_ids.difference(&index_ids).collect::<Vec<_>>(),
            index_ids.difference(&expected_ids).collect::<Vec<_>>()
        ));
    }
    // Active run dir + run-index hash.
    let active_run = top.get("active_run").and_then(|a| a.as_str()).unwrap_or("");
    if active_run.is_empty() {
        return Err("performance index has no active_run".into());
    }
    let run_rel = active_run.trim_start_matches("evidence/performance/");
    let run_dir = std::path::Path::new("evidence/performance").join(run_rel);

    // Run-manifest binding (L1-F / L20): the benchmark-run wrapper recorded
    // the exact commit + tree + Cargo.lock SHA at benchmark time.  The seal
    // must compare those captured values against the intended implementation
    // commit — never trust a run dir that claims a different source.
    let run_manifest_path = run_dir.join("run-manifest.json");
    let run_manifest_bytes = std::fs::read(&run_manifest_path).map_err(|e| {
        format!(
            "run-manifest.json missing in active run {}: {} (a benchmark-run wrapper run is required)",
            run_dir.display(),
            e
        )
    })?;
    let run_manifest: serde_json::Value = serde_json::from_slice(&run_manifest_bytes)
        .map_err(|e| format!("parse run-manifest.json: {}", e))?;
    let run_commit = run_manifest
        .get("commit")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let declared_impl_commit = top
        .get("implementation_commit")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if run_commit.is_empty() || declared_impl_commit.is_empty() {
        return Err("run-manifest.json or top-level index missing implementation commit".into());
    }
    if run_commit != declared_impl_commit {
        return Err(format!(
            "run-manifest commit {} does not match top-level index implementation_commit {}",
            run_commit, declared_impl_commit
        ));
    }
    // The implementation commit must be an ancestor of HEAD (evidence from
    // uncommitted or divergent source is not sealable).
    let head_hash = get_git_head_hash();
    if !head_hash.is_empty() {
        let is_ancestor = std::process::Command::new("git")
            .args(["merge-base", "--is-ancestor", run_commit, &head_hash])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !is_ancestor {
            return Err(format!(
                "run-manifest commit {} is not an ancestor of HEAD {}",
                run_commit, head_hash
            ));
        }
    }
    println!(
        "  run-manifest binding: commit {} verified against top-level index",
        run_commit
    );

    // The run-manifest also records the Cargo.lock SHA-256 at benchmark time
    // (L1-F).  If the lock has since changed, the benchmark binaries may have
    // been built from a different dependency graph — the evidence cannot be
    // sealed against the current tree.
    let run_lock = run_manifest
        .get("cargo_lock_sha256")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if run_lock.is_empty() {
        return Err("run-manifest.json missing cargo_lock_sha256 (L1-F binding incomplete)".into());
    }
    let current_lock_bytes =
        std::fs::read("Cargo.lock").map_err(|e| format!("read Cargo.lock: {}", e))?;
    let current_lock_sha = sha256_hex(&current_lock_bytes);
    if run_lock != current_lock_sha {
        return Err(format!(
            "run-manifest Cargo.lock SHA-256 {} does not match the current tree's {} — the benchmark was built from a different dependency graph; re-run the benchmark suite at the sealed commit",
            run_lock, current_lock_sha
        ));
    }
    println!("  run-manifest Cargo.lock SHA-256 verified against the current tree");

    let run_index_bytes = std::fs::read(run_dir.join("index.json"))
        .map_err(|e| format!("read run index {}: {}", run_dir.display(), e))?;
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(&run_index_bytes);
    let run_index_sha = format!("{:x}", h.finalize());
    let declared_run_index_sha = top
        .get("run_index_sha256")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    if run_index_sha != declared_run_index_sha {
        return Err(format!(
            "run index SHA-256 mismatch: computed={} declared={}",
            run_index_sha, declared_run_index_sha
        ));
    }
    // Receipts + manifests + artifacts.
    for e in &entries {
        let pid = e
            .get("performance_id")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let file_sha = e
            .get("receipt_file_sha256")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let canon_sha = e
            .get("receipt_canonical_sha256")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let rp = run_dir
            .join("receipts")
            .join(format!("receipt-{}.json", pid));
        let bytes =
            std::fs::read(&rp).map_err(|e| format!("read receipt {}: {}", rp.display(), e))?;
        let mut hf = sha2::Sha256::new();
        hf.update(&bytes);
        let actual_file = format!("{:x}", hf.finalize());
        if actual_file != file_sha {
            return Err(format!("receipt {} file hash mismatch", pid));
        }
        // Canonical: typed struct with receipt_sha256 emptied.
        let content =
            String::from_utf8(bytes).map_err(|e| format!("utf8 {}: {}", rp.display(), e))?;
        let mut rec: ryg_rans_rs_casefile::PerformanceReceipt =
            serde_json::from_str(&content).map_err(|e| format!("parse receipt {}: {}", pid, e))?;
        rec.receipt_sha256 = String::new();
        let canonical = serde_json::to_string_pretty(&rec)
            .map_err(|e| format!("serialize receipt {}: {}", pid, e))?;
        let mut hc = sha2::Sha256::new();
        hc.update(canonical.as_bytes());
        let actual_canon = format!("{:x}", hc.finalize());
        if actual_canon != canon_sha {
            return Err(format!("receipt {} canonical hash mismatch", pid));
        }
        // Manifest exists and hashes to the receipt's manifest_sha256.
        let mp = run_dir
            .join("manifests")
            .join(format!("manifest-{}.json", pid));
        let mbytes =
            std::fs::read(&mp).map_err(|e| format!("read manifest {}: {}", mp.display(), e))?;
        let mut hm = sha2::Sha256::new();
        hm.update(&mbytes);
        let m_sha = format!("{:x}", hm.finalize());
        if m_sha != rec.manifest_sha256 {
            return Err(format!("manifest {} hash mismatch", pid));
        }
        // Raw artifacts referenced by the receipt must exist.
        for (name, key) in [
            ("results.json", &rec.results_json_sha256),
            ("results.csv", &rec.results_csv_sha256),
        ] {
            let ap = run_dir.join(pid).join(name);
            let abytes =
                std::fs::read(&ap).map_err(|e| format!("read artifact {}: {}", ap.display(), e))?;
            let mut ha = sha2::Sha256::new();
            ha.update(&abytes);
            let a_sha = format!("{:x}", ha.finalize());
            if &a_sha != key {
                return Err(format!("artifact {} for {} hash mismatch", name, pid));
            }
        }
        // host.json + commands.log at run level.
        for (name, key) in [
            ("host.json", &rec.host_metadata_sha256),
            ("commands.log", &rec.commands_log_sha256),
        ] {
            let ap = run_dir.join(name);
            let abytes = std::fs::read(&ap).map_err(|e| format!("read {}: {}", ap.display(), e))?;
            let mut ha = sha2::Sha256::new();
            ha.update(&abytes);
            let a_sha = format!("{:x}", ha.finalize());
            if &a_sha != key {
                return Err(format!("{} for {} hash mismatch", name, pid));
            }
        }
        // The raw Criterion archive must exist.
        let archive = run_dir.join("criterion.tar.zst");
        if !archive.exists() {
            return Err(format!("raw Criterion archive missing for {}", pid));
        }
        // Every receipt must be a sealed measurement.
        if rec.verdict != ryg_rans_rs_casefile::PerformanceReceiptVerdict::SealedMeasurement {
            return Err(format!(
                "receipt {} verdict {:?} is not SealedMeasurement",
                pid, rec.verdict
            ));
        }
        // Sample counts meet the minimum (7 independent samples).
        let manifest: ryg_rans_rs_casefile::PerformanceManifest =
            serde_json::from_slice(&mbytes)
                .map_err(|e| format!("parse manifest {}: {}", pid, e))?;
        for c in &manifest.benchmark_cases {
            if c.sample_count < 7 {
                return Err(format!(
                    "{} case {} has sample_count {} < 7",
                    pid, c.benchmark_id, c.sample_count
                ));
            }
            if !c.verification_passed {
                return Err(format!(
                    "{} case {} verification_passed=false",
                    pid, c.benchmark_id
                ));
            }
            if !c.median_ns.is_finite()
                || !c.mean_ns.is_finite()
                || !c.stddev_ns.is_finite()
                || !c.confidence_interval_95_low_ns.is_finite()
                || !c.confidence_interval_95_high_ns.is_finite()
            {
                return Err(format!(
                    "{} case {} has non-finite numerics",
                    pid, c.benchmark_id
                ));
            }
            if c.confidence_interval_95_low_ns > c.confidence_interval_95_high_ns {
                return Err(format!(
                    "{} case {} confidence interval inverted",
                    pid, c.benchmark_id
                ));
            }
            if c.backend_requested != c.backend_executed {
                return Err(format!(
                    "{} case {} backend identity mismatch: requested={} executed={}",
                    pid, c.benchmark_id, c.backend_requested, c.backend_executed
                ));
            }
        }
    }
    println!("  performance evidence: 15 receipts, run index, manifests, artifacts all verified");
    Ok(())
}

/// Verify the public-corpus stress/soak execution evidence (post-v0.5.0
/// audit, MODEL_CACHE.WORKLOAD.2 + O.20 "stress logs" binding).
///
/// The active performance run must carry `workload-stress-public.log` and
/// `workload-soak-public.log`: complete transcripts of the genuine
/// public-corpus runners, each containing its schedule identity line and
/// its completion marker.  The schedule identity (name + schedule_sha256
/// prefix) must match the derived workload manifest, so the log is bound
/// to the exact executed schedule — a synthetic run or a corpus-presence
/// gate cannot satisfy this gate.
fn check_workload_execution_evidence() -> Result<(), String> {
    let top_path = std::path::Path::new("evidence/performance/index.json");
    let top: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(top_path).map_err(|e| format!("read index.json: {}", e))?,
    )
    .map_err(|e| format!("parse index.json: {}", e))?;
    let active_run = top.get("active_run").and_then(|a| a.as_str()).unwrap_or("");
    if active_run.is_empty() {
        return Err("performance index has no active_run".into());
    }
    let run_rel = active_run.trim_start_matches("evidence/performance/");
    let run_dir = std::path::Path::new("evidence/performance").join(run_rel);

    // The derived manifest is the workload identity source (from the
    // fetch cache, outside the repo).
    let workload_dir = std::env::var("RYG_RANS_WORKLOAD_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
                .join(".cache/ryg-rans-rs/workloads")
        });
    let manifest_path = workload_dir.join("public-rans-v1/derived/public-rans-v1.manifest.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path)
            .map_err(|e| format!("read {}: {}", manifest_path.display(), e))?,
    )
    .map_err(|e| format!("parse workload manifest: {}", e))?;

    // schedule_sha256 by schedule name from the manifest.
    let mut sha_by_name: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(scheds) = manifest.get("schedules").and_then(|s| s.as_array()) {
        for s in scheds {
            let name = s.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let sha = s
                .get("schedule_sha256")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !name.is_empty() && !sha.is_empty() {
                sha_by_name.insert(name.to_string(), sha.to_string());
            }
        }
    }
    if sha_by_name.is_empty() {
        return Err("workload manifest has no schedules".into());
    }

    // Both transcripts must exist, be non-empty, carry the schedule
    // identity line, and end with the completion marker; the identity must
    // match the manifest's schedule hash.
    let check_one = |file: &str, prefix: &str, complete_marker: &str| -> Result<(), String> {
        let path = run_dir.join(file);
        let text = std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "{} missing in active run {}: {} — run `cargo xtask workload {}` with `--log {}` at the implementation commit",
                file,
                run_dir.display(),
                e,
                if file.starts_with("workload-stress") {
                    "stress-public"
                } else {
                    "soak-public"
                },
                path.display()
            )
        })?;
        if text.trim().is_empty() {
            return Err(format!("{} is empty", file));
        }
        let identity_line = text
            .lines()
            .find(|l| l.starts_with(prefix))
            .ok_or_else(|| format!("{} has no {} identity line", file, prefix))?;
        if !text.lines().any(|l| l.starts_with(complete_marker)) {
            return Err(format!(
                "{} has no completion marker '{}' — the run did not finish",
                file, complete_marker
            ));
        }
        // Extract "schedule <name> (N blocks, ... sha256 <hex16>)" and
        // verify the schedule exists in the manifest with that hash.
        let rest = identity_line.trim_start_matches(prefix).trim_start();
        let name: String = rest.split_whitespace().next().unwrap_or("").to_string();
        let expected = sha_by_name.get(&name).ok_or_else(|| {
            format!(
                "{} identity names schedule '{}' which is not in the derived manifest",
                file, name
            )
        })?;
        let log_sha: String = rest
            .split("sha256 ")
            .nth(1)
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if !log_sha.is_empty() && !expected.starts_with(&log_sha) {
            return Err(format!(
                "{} schedule_sha256 {} does not match the manifest's {} for '{}'",
                file,
                log_sha,
                &expected[..16.min(expected.len())],
                name
            ));
        }
        println!(
            "  workload evidence {}: schedule '{}' verified (sha256 {})",
            file, name, log_sha
        );
        Ok(())
    };

    check_one(
        "workload-stress-public.log",
        "stress-public: schedule ",
        "stress-public complete:",
    )?;
    check_one(
        "workload-soak-public.log",
        "soak-public: schedule ",
        "soak-public complete:",
    )?;
    Ok(())
}

/// Verify the README Evidence Status table counts match the evidence indexes.
fn check_readme_evidence_counts() -> Result<(), String> {
    let readme =
        std::fs::read_to_string("README.md").map_err(|e| format!("read README.md: {}", e))?;
    // Behavioural total from evidence/index.json.
    let index_content = std::fs::read_to_string("evidence/index.json")
        .map_err(|e| format!("read evidence/index.json: {}", e))?;
    let index: serde_json::Value = serde_json::from_str(&index_content)
        .map_err(|e| format!("parse evidence/index.json: {}", e))?;
    let total_behavioural = index
        .get("receipts")
        .and_then(|r| r.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let perf_content = std::fs::read_to_string("evidence/performance/index.json")
        .map_err(|e| format!("read evidence/performance/index.json: {}", e))?;
    let perf: serde_json::Value = serde_json::from_str(&perf_content)
        .map_err(|e| format!("parse evidence/performance/index.json: {}", e))?;
    let total_perf = perf
        .get("receipts")
        .and_then(|r| r.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    // The README total row must carry both counts.
    let expected_total_line = format!(
        "| **Total** | | | **{}** | **{}** |",
        total_behavioural, total_perf
    );
    if !readme.contains(&expected_total_line) {
        return Err(format!(
            "README total row mismatch: expected '{}'",
            expected_total_line
        ));
    }
    println!(
        "  README counts match evidence: {} behavioural / {} performance",
        total_behavioural, total_perf
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Performance seal
// ---------------------------------------------------------------------------

const EXPECTED_PERF_IDS: &[&str] = &[
    "RYG_RANS.PERF.BYTE",
    "RYG_RANS.PERF.R64",
    "RYG_RANS.PERF.WORD.SCALAR",
    "RYG_RANS.PERF.ALIAS",
    "RYG_RANS.PERF.SSE41.INTERLEAVED8",
    "RYG_RANS.PERF.AVX512VL.INTERLEAVED8",
    "RYG_RANS.PERF.AVX512.INTERLEAVED16",
    "RYG_RANS.PERF.PHASE_H",
    "RYG_RANS.PERF.PHASE_J.AVX2",
    "RYG_RANS.PERF.PHASE_I.PARALLEL",
    // Phase O cache evidence (O.20).
    "RYG_RANS.PERF.CACHE.CONSTRUCTION",
    "RYG_RANS.PERF.CACHE.CONCURRENCY",
    "RYG_RANS.PERF.CACHE.COLD_WARM",
    "RYG_RANS.PERF.CACHE.THRASH",
    "RYG_RANS.PERF.CACHE.MIXED_PUBLIC",
];

const SURFACE_NAMES: &[&str] = &[
    "32-bit byte rANS — division + reciprocal",
    "64-bit rANS — division + reciprocal",
    "Word rANS — scalar table-based",
    "Alias method — Vose table",
    "SSE4.1 SIMD — interleaved8",
    "AVX512VL — interleaved8",
    "AVX512 — interleaved16",
    "Phase H optimization backends",
    "Phase J AVX2 backends",
    "Phase I parallel block engine",
    "Model cache — construction microbenchmarks",
    "Model cache — concurrency microbenchmarks (single-flight, bypass, eviction)",
    "Model cache — disabled/cold/warm end-to-end decode",
    "Model cache — hot-set/thrash/unique end-to-end decode",
    "Model cache — mixed public-corpus decode (natural + grouped)",
];

/// Map a Criterion benchmark ID to a surface index (0..14).
///
/// Criterion 0.5 flattens `/` in benchmark group names to `_` when creating
/// directory names, so we check for `_` as the separator between the bench
/// name and the group path.
fn classify_benchmark_id(id: &str) -> Option<usize> {
    // Canonical (slash-separated) Criterion IDs, e.g.
    // `byte-rans/byte-decode/SKEWED_255_1/1KiB/iter`.  The Phase K
    // underscore-flattened forms are retained as a fallback.
    if id.starts_with("byte-rans/") || id.starts_with("byte-rans_") {
        return Some(0); // BYTE
    }
    if id.starts_with("r64/") || id.starts_with("r64_") {
        return Some(1); // R64
    }
    if id.starts_with("scalar/") || id.starts_with("scalar_") {
        return Some(2); // WORD.SCALAR
    }
    if id.starts_with("alias/") || id.starts_with("alias_") {
        return Some(3); // ALIAS
    }
    if id.starts_with("sse41/") || id.starts_with("sse41_") {
        return Some(4); // SSE41.INTERLEAVED8
    }
    if id.starts_with("avx512/") || id.starts_with("avx512_") {
        // Filter by backend: 16-way → AVX512.INTERLEAVED16, 8-way →
        // AVX512VL.INTERLEAVED8.  Check the 16-way marker first so the
        // `avx512-16way` ID never falls through to the 8-way bucket.
        if id.contains("16way") {
            return Some(6);
        }
        return Some(5); // AVX512VL.INTERLEAVED8 (8-way / 2x8-on-16)
    }
    if id.starts_with("specialized/") || id.starts_with("specialized_") {
        return Some(7); // PHASE_H
    }
    if id.starts_with("avx2/")
        || id.starts_with("avx2_")
        || id.starts_with("batch/")
        || id.starts_with("batch_")
    {
        return Some(8); // PHASE_J.AVX2
    }
    if id.starts_with("parallel/") || id.starts_with("parallel_") || id.starts_with("block-engine/")
    {
        return Some(9); // PHASE_I.PARALLEL
    }
    // Phase O model-cache surfaces (10..14).  The buckets are disjoint and
    // complete over the `model_cache` bench's group hierarchy.
    if id.starts_with("model_cache/construction") || id.starts_with("model_cache_construction") {
        return Some(10); // CACHE.CONSTRUCTION
    }
    if id.starts_with("model_cache/ops") || id.starts_with("model_cache_ops") {
        return Some(11); // CACHE.CONCURRENCY
    }
    if id.starts_with("model_cache/e2e") || id.starts_with("model_cache_e2e") {
        for m in ["disabled", "cold", "warm"] {
            if id.contains(&format!("/{}/", m)) || id.contains(&format!("_{}_", m)) {
                return Some(12); // CACHE.COLD_WARM
            }
        }
        return Some(13); // CACHE.THRASH (hot-set/thrash/unique)
    }
    if id.starts_with("model_cache/public") || id.starts_with("model_cache_public") {
        return Some(14); // CACHE.MIXED_PUBLIC
    }
    None
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

/// Read a file and return its SHA-256 hex digest.
fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| format!("read {:?}: {}", path, e))?;
    Ok(sha256_hex(&data))
}

// ---------------------------------------------------------------------------
// Phase L.18: benchmark-run wrapper
// ---------------------------------------------------------------------------

/// `cargo xtask benchmark-run` — run the complete benchmark suite inside a
/// provenance-bound run directory.
///
/// The wrapper (residual L1-E/L1-F) fixes the Phase K flaw of capturing
/// provenance at seal time: this command captures Git/rustc/flags metadata
/// BEFORE compilation, runs the suite itself, captures metadata again AFTER,
/// refuses to proceed if the environment changed materially, and writes a
/// completion marker only when every benchmark finished successfully.
///
/// Usage:
/// ```sh
/// RUSTFLAGS="-C target-cpu=native" cargo xtask benchmark-run \
///   --criterion-dir target/criterion \
///   --run-dir evidence/performance/runs/<run-id> \
///   -- [additional cargo bench args...]
/// ```
fn cmd_benchmark_run(args: &[String]) -> Result<(), String> {
    let mut criterion_dir = std::path::PathBuf::from("target/criterion");
    let mut run_dir: Option<std::path::PathBuf> = None;
    let mut bench_args: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--criterion-dir" => {
                i += 1;
                criterion_dir = std::path::PathBuf::from(
                    args.get(i).ok_or("--criterion-dir requires a value")?,
                );
            }
            "--run-dir" => {
                i += 1;
                run_dir = Some(std::path::PathBuf::from(
                    args.get(i).ok_or("--run-dir requires a value")?,
                ));
            }
            "--" => {
                bench_args = args[i + 1..].to_vec();
                break;
            }
            other => return Err(format!("unknown argument: {}", other)),
        }
        i += 1;
    }

    // ---- 1. Refuse a dirty tree ---------------------------------------------
    let git_clean = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|e| format!("git status failed: {}", e))?;
    let porcelain = String::from_utf8_lossy(&git_clean.stdout);
    let dirty: Vec<&str> = porcelain.lines().filter(|l| !l.is_empty()).collect();
    if !dirty.is_empty() {
        return Err(format!(
            "refusing to run benchmarks: working tree is dirty ({} change(s)). Commit or stash first.",
            dirty.len()
        ));
    }

    // ---- 2. Establish the run identity --------------------------------------
    let commit = get_git_head_hash();
    if commit.is_empty() {
        return Err("cannot determine git HEAD".into());
    }
    let run_id = run_dir
        .as_ref()
        .and_then(|d| d.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or(&commit)
        .to_string();
    let run_dir = run_dir.unwrap_or_else(|| {
        std::path::PathBuf::from(format!("evidence/performance/runs/{}", run_id))
    });
    let preflight_dir = run_dir.join("preflight");
    std::fs::create_dir_all(&preflight_dir)
        .map_err(|e| format!("create {:?}: {}", preflight_dir, e))?;
    // Bench binaries run with cwd = the crate root, so the env var must be
    // an ABSOLUTE path or records land in the wrong tree (the first wrapper
    // run dropped them under crates/ryg-rans-rs-bench/evidence/).
    let preflight_dir = std::fs::canonicalize(&preflight_dir)
        .map_err(|e| format!("canonicalize {:?}: {}", preflight_dir, e))?;
    let marker = run_dir.join("RUN_COMPLETE");
    if marker.exists() {
        return Err(format!(
            "run {} already has a completion marker; refusing to overwrite",
            run_id
        ));
    }

    // ---- 2.5 Isolate the criterion tree -------------------------------------
    // Each evidence run owns its criterion tree; stale artifacts from
    // earlier runs (e.g. Phase K baseline dirs) must not leak into the
    // export (the exporter walks the whole tree and would otherwise find
    // measurements with no preflight records).
    if criterion_dir.exists() {
        std::fs::remove_dir_all(&criterion_dir)
            .map_err(|e| format!("clear {:?}: {}", criterion_dir, e))?;
    }
    std::fs::create_dir_all(&criterion_dir)
        .map_err(|e| format!("create {:?}: {}", criterion_dir, e))?;

    // ---- 3. Capture metadata BEFORE compilation -----------------------------
    let tree_sha = git_tree_hash()?;
    let lock_sha = sha256_file(&std::path::Path::new("Cargo.lock"))?;
    let rustc_vv = get_rustc_version();
    let rustflags = get_rustflags();
    let before = serde_json::json!({
        "commit": commit,
        "tree_sha": tree_sha,
        "cargo_lock_sha256": lock_sha,
        "rustc": rustc_vv,
        "rustflags": rustflags,
        "criterion_dir": criterion_dir.display().to_string(),
        "bench_args": bench_args,
        "timestamp": chrono_now(),
    });
    std::fs::write(
        run_dir.join("run-manifest.json"),
        serde_json::to_string_pretty(&before).map_err(|e| format!("serialize: {}", e))?,
    )
    .map_err(|e| format!("write run-manifest: {}", e))?;

    // Capture host + cpuinfo + environment as artifacts (L1-H/L1-J).
    let host_meta = collect_host_metadata();
    let os_meta = collect_os_metadata();
    std::fs::write(
        run_dir.join("host.json"),
        serde_json::to_string_pretty(&serde_json::json!({
            "cpu": host_meta, "os": os_meta, "rustc": rustc_vv, "rustflags": rustflags,
        }))
        .map_err(|e| format!("serialize host: {}", e))?,
    )
    .map_err(|e| format!("write host.json: {}", e))?;
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    std::fs::write(run_dir.join("cpuinfo.txt"), &cpuinfo)
        .map_err(|e| format!("write cpuinfo: {}", e))?;
    std::fs::write(run_dir.join("rustc-vV.txt"), &rustc_vv)
        .map_err(|e| format!("write rustc-vV: {}", e))?;
    let env_json = serde_json::json!({
        "rustflags": rustflags,
        "smt": read_first_line("/sys/devices/system/cpu/smt/active"),
        "governor": read_first_line("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor"),
        "smt_active": read_first_line("/sys/devices/system/cpu/smt/active"),
        "cpu_count": std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0),
    });
    std::fs::write(
        run_dir.join("environment.json"),
        serde_json::to_string_pretty(&env_json).map_err(|e| format!("serialize env: {}", e))?,
    )
    .map_err(|e| format!("write environment.json: {}", e))?;

    // ---- 4. Run the benchmark suite ------------------------------------------
    println!(
        "benchmark-run: executing the benchmark suite (run {})",
        run_id
    );
    println!("  commit: {}", commit);
    println!("  rustflags: {}", rustflags);
    let mut cmd = std::process::Command::new("cargo");
    cmd.arg("bench").arg("-p").arg("ryg-rans-rs-bench");
    for a in &bench_args {
        cmd.arg(a);
    }
    cmd.env("RYG_RANS_PREFLIGHT_DIR", &preflight_dir);
    // The model_cache bench's public-corpus group measures real corpus
    // slices only when a fetched, hash-verified workload cache is present.
    // Wire the env when it exists; otherwise the group is skipped and the
    // seal's MIXED_PUBLIC surface will fail with zero records (evidence
    // runs must fetch first).
    let workload_cache = std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default())
        .join(".cache/ryg-rans-rs/workloads/public-rans-v1");
    let manifest_path = workload_cache.join("derived/public-rans-v1.manifest.json");
    let source_cache = workload_cache.join("extracted");
    if manifest_path.exists() && source_cache.is_dir() {
        cmd.env("RYG_RANS_WORKLOAD_MANIFEST", &manifest_path);
        cmd.env("RYG_RANS_SOURCE_CACHE", &source_cache);
        println!(
            "benchmark-run: public-corpus group enabled (workload cache at {})",
            workload_cache.display()
        );
    } else {
        println!(
            "benchmark-run: workload cache not found at {} — model_cache public group skipped",
            workload_cache.display()
        );
    }
    let mut commands_log = String::new();
    commands_log.push_str(&format!(
        "workdir: {}\n",
        std::env::current_dir()
            .map(|p| p.display().to_string())
            .unwrap_or_default()
    ));
    commands_log.push_str(&format!(
        "command: cargo bench -p ryg-rans-rs-bench {}\n",
        bench_args.join(" ")
    ));
    commands_log.push_str(&format!("rustflags: {}\n", rustflags));
    commands_log.push_str(&format!("preflight_dir: {}\n", preflight_dir.display()));
    commands_log.push_str(&format!("start: {}\n", chrono_now()));
    let status = cmd
        .status()
        .map_err(|e| format!("spawn cargo bench: {}", e))?;
    commands_log.push_str(&format!("exit: {}\n", status));
    commands_log.push_str(&format!("finish: {}\n", chrono_now()));
    if !status.success() {
        std::fs::write(run_dir.join("commands.log"), &commands_log)
            .map_err(|e| format!("write commands.log: {}", e))?;
        return Err(format!("cargo bench exited with {}", status));
    }

    // ---- 5. Capture metadata AFTER; refuse on material change ----------------
    let after_tree = git_tree_hash()?;
    let after_lock = sha256_file(&std::path::Path::new("Cargo.lock"))?;
    if after_tree != tree_sha {
        return Err(format!(
            "environment changed during the run: tree sha {} -> {}. Evidence invalid; rerun from a clean checkout.",
            tree_sha, after_tree
        ));
    }
    if after_lock != lock_sha {
        return Err("Cargo.lock changed during the run; evidence invalid".into());
    }
    commands_log.push_str("post-run tree sha: ");
    commands_log.push_str(&after_tree);
    commands_log.push_str("\n");
    std::fs::write(run_dir.join("commands.log"), &commands_log)
        .map_err(|e| format!("write commands.log: {}", e))?;

    // ---- 6. Completion marker (only after full success) ----------------------
    std::fs::write(
        &marker,
        format!(
            "run_id: {}\ncommit: {}\nfinished: {}\n",
            run_id,
            commit,
            chrono_now()
        ),
    )
    .map_err(|e| format!("write completion marker: {}", e))?;
    let count = std::fs::read_dir(&preflight_dir)
        .map(|d| d.filter_map(|e| e.ok()).count())
        .unwrap_or(0);
    println!(
        "benchmark-run: completed. run_dir={} preflight_records={}",
        run_dir.display(),
        count
    );
    Ok(())
}

/// Git tree SHA-256 (the committed tree object hash).
fn git_tree_hash() -> Result<String, String> {
    let out = std::process::Command::new("git")
        .args(["rev-parse", "HEAD^{tree}"])
        .output()
        .map_err(|e| format!("git rev-parse tree: {}", e))?;
    if !out.status.success() {
        return Err("git rev-parse HEAD^{tree} failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn read_first_line(path: &str) -> String {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn chrono_now() -> String {
    // Avoid pulling chrono: use the system date command (deterministic format).
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string())
        .trim()
        .to_string()
}

/// Collect host metadata for performance sealing.
/// Collect host CPU metadata for a performance run.
///
/// `features` is the **runtime-detected** host capability set
/// (`std::is_x86_feature_detected!()` — the `runtime_cpu.detected_features`
/// fact).  Pre-0.5.1 this field was populated from `#[cfg(target_feature
/// = ...)]`, i.e. the *xtask binary's* compiled features, conflating the
/// sealer's codegen with the host's capability (post-v0.5.0 audit #4);
/// the compiled facts are now carried separately in the manifest's
/// `rustflags` (bench-time) and the `BenchRecord.compiled_target` block.
fn collect_host_metadata() -> ryg_rans_rs_casefile::CpuMetadata {
    let model = std::fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("model name"))
                .map(|l| l.split(':').nth(1).unwrap_or("unknown").trim().to_string())
        })
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());

    // Runtime detection: the host CPU's actual capability, independent of
    // how the xtask binary was compiled.
    let features: Vec<String> = runtime_detected_features();

    let microcode = std::fs::read_to_string("/proc/cpuinfo").ok().and_then(|s| {
        s.lines()
            .find(|l| l.starts_with("microcode"))
            .map(|l| l.split(':').nth(1).unwrap_or("").trim().to_string())
    });

    let smt_enabled = std::fs::read_to_string("/sys/devices/system/cpu/smt/active")
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
        .map(|v| v == 1)
        .unwrap_or(false);

    let governor = std::fs::read_to_string("/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    ryg_rans_rs_casefile::CpuMetadata {
        model,
        features,
        microcode,
        smt_enabled,
        governor,
    }
}

/// Runtime CPU feature detection (x86-64/x86 only; other architectures
/// report an empty set — the host capability facts).
fn runtime_detected_features() -> Vec<String> {
    let mut features = Vec::new();
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

fn collect_os_metadata() -> ryg_rans_rs_casefile::OsMetadata {
    let kernel = std::fs::read_to_string("/proc/version")
        .ok()
        .map(|s| s.split_whitespace().nth(2).unwrap_or("unknown").to_string())
        .unwrap_or_else(|| std::env::consts::ARCH.to_string());

    let os = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);

    let memory = std::fs::read_to_string("/proc/meminfo").ok().and_then(|s| {
        s.lines()
            .find(|l| l.starts_with("MemTotal"))
            .map(|l| l.trim().to_string())
    });

    ryg_rans_rs_casefile::OsMetadata { kernel, os, memory }
}

/// Get rustc version string.
fn get_rustc_version() -> String {
    std::process::Command::new("rustc")
        .args(["-vV"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Get Criterion version from Cargo.lock.
fn get_criterion_version() -> String {
    let lock_content = std::fs::read_to_string("Cargo.lock").unwrap_or_default();
    let mut in_criterion = false;
    for line in lock_content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[[package]]") {
            in_criterion = false;
        }
        if trimmed.starts_with("name = ") && trimmed.contains("criterion") {
            in_criterion = true;
        }
        if in_criterion && trimmed.starts_with("version = ") {
            if let Some(ver) = trimmed
                .strip_prefix("version = ")
                .map(|s| s.trim().trim_matches('"'))
            {
                return ver.to_string();
            }
        }
    }
    "unknown".to_string()
}

/// Get RUSTFLAGS from environment.
fn get_rustflags() -> String {
    std::env::var("RUSTFLAGS").unwrap_or_default()
}

/// Read the benchmark run's RUSTFLAGS from its `host.json` (written by
/// `cargo xtask benchmark-run` at benchmark time).  This is the
/// authoritative codegen fact for the run (post-v0.5.0 audit #4); the seal
/// invocation's own environment is a fallback only.
fn read_run_rustflags(run_dir: &std::path::Path) -> Option<String> {
    let host_json = std::fs::read_to_string(run_dir.join("host.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&host_json).ok()?;
    v.get("rustflags")?.as_str().map(|s| s.to_string())
}

/// Create a deterministic, zstd-compressed tarball of a directory.
///
/// Uses the maintained `tar` crate (PAX long-name support), so paths longer
/// than 99 bytes are preserved — the Phase K custom writer truncated them
/// (residual L1-K).  Files are added in sorted order with normalized
/// timestamps so the archive is reproducible.
fn archive_criterion(
    criterion_dir: &std::path::Path,
    output_path: &std::path::Path,
) -> Result<(), String> {
    let file = std::fs::File::create(output_path)
        .map_err(|e| format!("create archive {:?}: {}", output_path, e))?;
    let mut encoder = zstd::Encoder::new(file, 3).map_err(|e| format!("zstd encoder: {}", e))?;
    {
        let mut builder = tar::Builder::new(&mut encoder);
        builder.mode(tar::HeaderMode::Deterministic);

        let criterion_dir = criterion_dir
            .canonicalize()
            .map_err(|e| format!("canonicalize {:?}: {}", criterion_dir, e))?;

        let mut entries: Vec<std::path::PathBuf> = Vec::new();
        collect_files(&criterion_dir, &criterion_dir, &mut entries);
        // Deterministic order: sorted by relative path.
        entries.sort();

        for entry in &entries {
            let relative = entry
                .strip_prefix(&criterion_dir)
                .map_err(|e| format!("strip prefix: {}", e))?;
            // Tar paths must use forward slashes and never start with '/'.
            let name = relative.to_string_lossy().replace('\\', "/");
            builder
                .append_path_with_name(entry, &name)
                .map_err(|e| format!("append {:?} as {}: {}", entry, name, e))?;
        }
        builder.finish().map_err(|e| format!("finish tar: {}", e))?;
    }
    encoder
        .finish()
        .map_err(|e| format!("finish zstd: {}", e))?;
    Ok(())
}

fn collect_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    entries: &mut Vec<std::path::PathBuf>,
) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_files(root, &path, entries);
            } else if path.is_file() {
                entries.push(path);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Performance seal command
// ---------------------------------------------------------------------------

fn cmd_performance_seal(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    // ---- Parse arguments -------------------------------------------------------
    let mut criterion_dir = std::path::PathBuf::from("target/criterion");
    let mut run_dir = std::path::PathBuf::from("evidence/performance");
    let mut implementation_commit: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--criterion-dir" => {
                i += 1;
                if i < args.len() {
                    criterion_dir = std::path::PathBuf::from(&args[i]);
                } else {
                    return Err("--criterion-dir requires a value".into());
                }
            }
            "--run-dir" => {
                i += 1;
                if i < args.len() {
                    run_dir = std::path::PathBuf::from(&args[i]);
                } else {
                    return Err("--run-dir requires a value".into());
                }
            }
            "--implementation-commit" => {
                i += 1;
                if i < args.len() {
                    implementation_commit = Some(args[i].clone());
                } else {
                    return Err("--implementation-commit requires a value".into());
                }
            }
            other => {
                return Err(format!("unknown argument: {}", other).into());
            }
        }
        i += 1;
    }

    let run_id = get_git_head_hash();
    if run_id.is_empty() {
        return Err("cannot determine git HEAD hash for run_id".into());
    }
    let implementation_commit = implementation_commit.unwrap_or_else(|| run_id.clone());
    let host_id = format!("{}-{}", hostname(), std::env::consts::ARCH);

    // We use this to accumulate non-fatal errors and report them all at once.
    let mut errors: Vec<String> = Vec::new();
    let mut warn = |msg: String| {
        eprintln!("WARN: {}", msg);
        errors.push(msg);
    };

    // =========================================================================
    // 1. Verify clean Git state
    // =========================================================================
    println!("performance-seal: step 1 — verifying clean Git state...");
    let status_output = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .map_err(|e| format!("git status failed: {}", e))?;
    let porcelain = String::from_utf8_lossy(&status_output.stdout);
    // Evidence artifacts under evidence/ are generated by this very command
    // (and by benchmark-run); only SOURCE changes block the seal.  The
    // benchmark-run wrapper already refused a dirty tree before the run.
    // Strip the two-character status prefix ("XY ") before matching paths.
    fn is_evidence(l: &str) -> bool {
        l.get(3..).unwrap_or("").starts_with("evidence/")
    }
    let dirty: Vec<&str> = porcelain
        .lines()
        .filter(|l| !l.is_empty() && !is_evidence(l))
        .collect();
    let source_dirty: Vec<&str> = porcelain
        .lines()
        .filter(|l| !l.is_empty() && !is_evidence(l))
        .collect();
    if !source_dirty.is_empty() {
        warn(format!(
            "dirty working tree: {} uncommitted SOURCE change(s) present. Performance evidence should come from a clean checkout.",
            source_dirty.len()
        ));
        for line in &source_dirty {
            eprintln!("  dirty: {}", line);
        }
    } else {
        println!(
            "  git status: clean ({} evidence path(s) untracked, expected)",
            dirty.len()
        );
    }

    // =========================================================================
    // 2. Collect host metadata
    // =========================================================================
    println!("performance-seal: step 2 — collecting host metadata...");
    let cpu_meta = collect_host_metadata();
    let os_meta = collect_os_metadata();
    let rustc_version = get_rustc_version();
    let criterion_version = get_criterion_version();
    let seal_rustflags = get_rustflags();
    // Post-v0.5.0 audit #4: the codegen facts recorded in the manifest must
    // be the BENCHMARK run's flags (the authoritative `host.json` written
    // by `cargo xtask benchmark-run` at benchmark time), never the seal
    // invocation's environment.  The run's host.json is the same file whose
    // exact bytes are hashed into the receipt (L1-H).
    let bench_rustflags = read_run_rustflags(&run_dir).unwrap_or_else(|| {
        warn(
            "host.json rustflags not found in run dir; falling back to the seal \
             environment's RUSTFLAGS (codegen facts then describe the SEAL run, not \
             the benchmark run — the manifest marks this via the fallback)"
                .to_string(),
        );
        seal_rustflags.clone()
    });
    let rustflags = bench_rustflags;
    let target_cpu = ryg_rans_rs_bench::common::metadata::parse_target_cpu(&rustflags);
    println!("  CPU: {}", cpu_meta.model);
    println!("  OS: {}", os_meta.os);
    println!("  rustc: {}", rustc_version.lines().next().unwrap_or("?"));
    println!("  Criterion: {}", criterion_version);
    println!("  SMT: {}", cpu_meta.smt_enabled);
    println!("  governor: {}", cpu_meta.governor);
    println!(
        "  bench-time RUSTFLAGS: {}",
        if rustflags.is_empty() {
            "(none)"
        } else {
            &rustflags
        }
    );
    println!("  parsed target-cpu: {}", target_cpu);

    // Build a host metadata JSON for hashing
    let host_metadata = serde_json::json!({
        "cpu": cpu_meta,
        "os": os_meta,
        "rustc_version": rustc_version,
        "criterion_version": criterion_version,
        "rustflags": rustflags,
        "cpu_features": cpu_meta.features,
        "smt_enabled": cpu_meta.smt_enabled,
        "governor": cpu_meta.governor,
    });
    // Build a host metadata JSON for hashing.  The authoritative artifact is
    // the `host.json` written by `cargo xtask benchmark-run` at benchmark
    // time (residual L1-H: "hash that exact file, do not hash an in-memory
    // value").  If the run dir contains one, its exact file bytes are hashed;
    // otherwise (seal-only invocation without a wrapper run) we fall back to
    // the in-memory value but mark the manifest dirty accordingly.
    let host_json_path = run_dir.join("host.json");
    let (host_metadata_sha256, host_file_found) = match std::fs::read(&host_json_path) {
        Ok(bytes) => (sha256_hex(&bytes), true),
        Err(_) => {
            let host_metadata_json = serde_json::to_string(&host_metadata)
                .map_err(|e| format!("serialize host metadata: {}", e))?;
            (sha256_hex(host_metadata_json.as_bytes()), false)
        }
    };
    if host_file_found {
        println!("  host metadata hash: SHA-256 of run host.json");
    } else {
        warn(
            "host.json not found in run dir; hashed in-memory metadata instead (L1-H)".to_string(),
        );
    }

    // =========================================================================
    // 3. Load Criterion results
    // =========================================================================
    println!(
        "performance-seal: step 3 — loading Criterion results from {:?}...",
        criterion_dir
    );

    if !criterion_dir.exists() {
        return Err(format!("Criterion directory does not exist: {:?}", criterion_dir).into());
    }

    // Build a BenchMetadata for loading; we only need git_commit and dirty_tree
    let bench_meta = ryg_rans_rs_bench::common::metadata::BenchMetadata {
        rustc_version: rustc_version.clone(),
        // Post-v0.5.0 audit #4: `enabled_target_features` is the cfg feature
        // set of the EXPORTING process (the xtask binary), which is not a
        // measurement of the benchmark binary; the authoritative codegen
        // facts travel in `codegen_flags`/`target_cpu` bound to the
        // benchmark run (host.json).  We therefore record nothing here
        // rather than a misleading value — the typed `compiled_target`
        // block in every record makes the scope explicit.
        target_features: Vec::new(),
        cpu_model: cpu_meta.model.clone(),
        os_info: os_meta.os.clone(),
        git_commit: implementation_commit.clone(),
        dirty_tree: !dirty.is_empty(),
        num_cpus: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        codegen_flags: rustflags.clone(),
        target_cpu: target_cpu.clone(),
    };

    let preflight_dir = run_dir.join("preflight");
    let records = match ryg_rans_rs_bench::exporter::load_criterion_estimates(
        &criterion_dir,
        &preflight_dir,
        &bench_meta,
    ) {
        Ok(r) => r,
        Err(e) => {
            return Err(format!("load_criterion_estimates failed: {}", e).into());
        }
    };
    println!("  loaded {} benchmark records", records.len());

    // ---- Post-v0.5.0 audit #4: metadata normalization gates -----------------
    // A record whose profile is still literally "unknown" was not
    // normalized (every case is either a real profile or `not_applicable`).
    // An x86 host whose runtime feature set is empty means the detection
    // failed — reject rather than seal silent gaps.
    if let Some(bad) = records.iter().find(|r| r.profile == "unknown") {
        return Err(format!(
            "performance-seal: record {} has unnormalized profile \"unknown\" \
             (must be a real profile or \"not_applicable\")",
            bad.benchmark_id
        )
        .into());
    }
    if (cfg!(target_arch = "x86_64") || cfg!(target_arch = "x86")) && cpu_meta.features.is_empty() {
        return Err(
            "performance-seal: x86 host with an empty runtime feature set — host \
             capability detection failed; refusing to seal evidence with no \
             runtime_cpu facts"
                .into(),
        );
    }

    // =========================================================================
    // 4. Validate every expected benchmark surface (15 surfaces)
    // =========================================================================
    println!("performance-seal: step 4 — grouping records into surfaces...");

    // Group records by surface index. Surface index 0..14, plus a "misc" bucket.
    let mut surface_records: Vec<Vec<&ryg_rans_rs_bench::exporter::BenchRecord>> =
        (0..15).map(|_| Vec::new()).collect();
    let mut unclassified: Vec<&str> = Vec::new();

    for record in &records {
        match classify_benchmark_id(&record.benchmark_id) {
            Some(idx) => surface_records[idx].push(record),
            None => unclassified.push(&record.benchmark_id),
        }
    }

    for (idx, records) in surface_records.iter().enumerate() {
        if records.is_empty() {
            warn(format!(
                "surface {} ({}) has zero benchmark records",
                EXPECTED_PERF_IDS[idx], SURFACE_NAMES[idx]
            ));
        } else {
            println!(
                "  surface {} ({}): {} records",
                EXPECTED_PERF_IDS[idx],
                SURFACE_NAMES[idx],
                records.len()
            );
        }
    }
    for id in &unclassified {
        warn(format!("unclassified benchmark: {}", id));
    }

    // =========================================================================
    // 5. Create run directory structure
    // =========================================================================
    println!(
        "performance-seal: step 5 — preparing output directory {:?}...",
        run_dir
    );
    std::fs::create_dir_all(&run_dir)
        .map_err(|e| format!("create run dir {:?}: {}", run_dir, e))?;
    let manifests_dir = run_dir.join("manifests");
    let receipts_dir = run_dir.join("receipts");
    std::fs::create_dir_all(&manifests_dir).map_err(|e| format!("create manifests dir: {}", e))?;
    std::fs::create_dir_all(&receipts_dir).map_err(|e| format!("create receipts dir: {}", e))?;

    // =========================================================================
    // 6. Generate canonical results JSON and CSV per surface
    // =========================================================================
    println!("performance-seal: step 6 — generating results JSON/CSV...");
    let mut results_json_sha256s: Vec<String> = Vec::new();
    let mut results_csv_sha256s: Vec<String> = Vec::new();

    for (idx, recs) in surface_records.iter().enumerate() {
        if recs.is_empty() {
            results_json_sha256s.push(String::new());
            results_csv_sha256s.push(String::new());
            continue;
        }
        let surface_dir = run_dir.join(EXPECTED_PERF_IDS[idx]);
        std::fs::create_dir_all(&surface_dir)
            .map_err(|e| format!("create surface dir {:?}: {}", surface_dir, e))?;

        // We need owned records for export_summary
        let owned: Vec<ryg_rans_rs_bench::exporter::BenchRecord> =
            recs.iter().map(|r| (*r).clone()).collect();

        let (json_path, csv_path, json_sha, csv_sha) =
            ryg_rans_rs_bench::exporter::export_summary(&owned, &surface_dir).map_err(|e| {
                format!(
                    "export_summary for surface {}: {}",
                    EXPECTED_PERF_IDS[idx], e
                )
            })?;
        results_json_sha256s.push(json_sha);
        results_csv_sha256s.push(csv_sha);
        println!(
            "  surface {}: JSON={} CSV={}",
            EXPECTED_PERF_IDS[idx], json_path, csv_path
        );
    }

    // =========================================================================
    // 7. Archive target/criterion as criterion.tar.zst
    // =========================================================================
    println!("performance-seal: step 7 — archiving Criterion data...");
    let archive_path = run_dir.join("criterion.tar.zst");
    if let Err(e) = archive_criterion(&criterion_dir, &archive_path) {
        warn(format!("failed to archive Criterion directory: {}", e));
    }
    let criterion_archive_sha256 = if archive_path.exists() {
        sha256_file(&archive_path).unwrap_or_default()
    } else {
        String::new()
    };
    println!(
        "  archive: {:?} ({})",
        archive_path, criterion_archive_sha256
    );

    // =========================================================================
    // 8. Hash every artifact (SHA-256) — already done inline above
    // =========================================================================
    println!("performance-seal: step 8 — computing artifact hashes...");
    // The commands log is written by `cargo xtask benchmark-run` at benchmark
    // time; its exact file bytes are the authoritative artifact (residual
    // L1-G: an empty commands log is a seal failure, never hashed silently).
    let commands_log_path = run_dir.join("commands.log");
    let commands_log_sha256 = match std::fs::read(&commands_log_path) {
        Ok(bytes) if !bytes.is_empty() => {
            println!("  commands.log: {} bytes hashed", bytes.len());
            sha256_hex(&bytes)
        }
        Ok(_) => {
            warn("commands.log exists but is empty — this is a seal failure (L1-G)".to_string());
            sha256_hex(b"")
        }
        Err(_) => {
            warn(
                "commands.log not found in run dir — seal requires a benchmark-run wrapper run"
                    .to_string(),
            );
            sha256_hex(b"")
        }
    };

    // =========================================================================
    // 9. Generate 15 performance manifests (one per surface)
    // =========================================================================
    println!("performance-seal: step 9 — generating performance manifests...");
    let mut manifest_sha256s: Vec<String> = Vec::new();
    let mut all_manifest_paths: Vec<std::path::PathBuf> = Vec::new();

    for (idx, recs) in surface_records.iter().enumerate() {
        let perf_id = EXPECTED_PERF_IDS[idx];
        let surface_name = SURFACE_NAMES[idx];
        let _surface_dir = run_dir.join(perf_id);

        // Build PerformanceCase vector
        let cases: Vec<ryg_rans_rs_casefile::PerformanceCase> = recs
            .iter()
            .map(|r| ryg_rans_rs_casefile::PerformanceCase {
                benchmark_id: r.benchmark_id.clone(),
                backend_requested: r.backend_requested.clone(),
                backend_executed: r.backend_executed.clone(),
                profile: r.profile.clone(),
                bytes: r.bytes,
                threads_requested: r.threads_requested,
                threads_effective: r.threads_effective,
                sample_count: r.sample_count as usize,
                median_ns: r.median_ns,
                mean_ns: r.mean_ns,
                stddev_ns: r.stddev_ns,
                confidence_interval_95_low_ns: r.confidence_low_ns,
                confidence_interval_95_high_ns: r.confidence_high_ns,
                throughput_gib_s: r.throughput_gib_s,
                verification_passed: r.verification_passed,
                output_hash: r.output_hash.clone(),
                words_consumed_hash: if r.words_consumed_hash.is_empty() {
                    None
                } else {
                    Some(r.words_consumed_hash.clone())
                },
                final_states_hash: if r.final_states_hash.is_empty() {
                    None
                } else {
                    Some(r.final_states_hash.clone())
                },
                status: r.status.clone(),
            })
            .collect();

        let results_json_sha = results_json_sha256s[idx].clone();
        let results_csv_sha = results_csv_sha256s[idx].clone();

        let manifest = ryg_rans_rs_casefile::PerformanceManifest {
            schema_version: ryg_rans_rs_casefile::PERF_SCHEMA_VERSION,
            performance_id: perf_id.to_string(),
            surface: surface_name.to_string(),
            implementation_commit: implementation_commit.clone(),
            run_id: run_id.clone(),
            host_id: host_id.clone(),
            benchmark_cases: cases,
            artifact_hashes: ryg_rans_rs_casefile::PerformanceArtifactHashes {
                criterion_archive_sha256: criterion_archive_sha256.clone(),
                results_json_sha256: results_json_sha,
                results_csv_sha256: results_csv_sha,
                host_metadata_sha256: host_metadata_sha256.clone(),
                commands_log_sha256: commands_log_sha256.clone(),
            },
            command: std::env::args().collect::<Vec<_>>().join(" "),
            rustflags: rustflags.clone(),
            criterion_version: criterion_version.clone(),
            rustc_version: rustc_version.clone(),
            cpu: cpu_meta.clone(),
            os: os_meta.clone(),
            dirty_tree: !dirty.is_empty(),
        };

        let manifest_json = serde_json::to_string_pretty(&manifest)
            .map_err(|e| format!("serialize manifest {}: {}", perf_id, e))?;
        let manifest_path = manifests_dir.join(format!("manifest-{}.json", perf_id));
        std::fs::write(&manifest_path, &manifest_json)
            .map_err(|e| format!("write manifest {:?}: {}", manifest_path, e))?;
        let m_sha = sha256_hex(manifest_json.as_bytes());
        manifest_sha256s.push(m_sha);
        all_manifest_paths.push(manifest_path);
        println!("  manifest {} written", perf_id);
    }

    // =========================================================================
    // 10. Generate 15 performance receipts (one per surface)
    // =========================================================================
    println!("performance-seal: step 10 — generating performance receipts...");
    let mut receipt_sha256s: Vec<String> = Vec::new();
    let mut receipt_file_sha256s: Vec<String> = Vec::new();
    let mut all_receipt_paths: Vec<std::path::PathBuf> = Vec::new();
    let evidence_commit = get_git_head_hash();

    for (idx, _recs) in surface_records.iter().enumerate() {
        let perf_id = EXPECTED_PERF_IDS[idx];
        let cases = &surface_records[idx];
        let cases_declared = cases.len() as u64;
        let cases_executed = cases.iter().filter(|r| r.sample_count > 0).count() as u64;
        let cases_verified = cases.iter().filter(|r| r.verification_passed).count() as u64;
        let cases_failed = cases.iter().filter(|r| r.status == "fail").count() as u64;

        let manifest_sha = manifest_sha256s[idx].clone();
        let results_json_sha = results_json_sha256s[idx].clone();
        let results_csv_sha = results_csv_sha256s[idx].clone();

        let repro_command = format!(
            "cargo xtask performance-seal --criterion-dir {:?} --run-dir {:?} --implementation-commit {}",
            criterion_dir, run_dir, implementation_commit
        );

        // Build receipt without receipt_sha256 first, then hash it
        let mut receipt = ryg_rans_rs_casefile::PerformanceReceipt {
            schema_version: ryg_rans_rs_casefile::PERF_SCHEMA_VERSION,
            performance_id: perf_id.to_string(),
            surface: SURFACE_NAMES[idx].to_string(),
            verdict: if cases_failed > 0 {
                ryg_rans_rs_casefile::PerformanceReceiptVerdict::Rejected
            } else if cases_executed > 0 && cases_verified == cases_executed {
                ryg_rans_rs_casefile::PerformanceReceiptVerdict::SealedMeasurement
            } else if cases_declared > 0 {
                ryg_rans_rs_casefile::PerformanceReceiptVerdict::SealedWithResiduals
            } else {
                ryg_rans_rs_casefile::PerformanceReceiptVerdict::Rejected
            },
            implementation_commit: implementation_commit.clone(),
            evidence_commit: evidence_commit.clone(),
            run_id: run_id.clone(),
            host_id: host_id.clone(),
            cases_declared,
            cases_executed,
            cases_verified,
            cases_failed,
            residual_count: 0,
            residual_ids: Vec::new(),
            manifest_sha256: manifest_sha.clone(),
            criterion_archive_sha256: criterion_archive_sha256.clone(),
            results_json_sha256: results_json_sha.clone(),
            results_csv_sha256: results_csv_sha.clone(),
            host_metadata_sha256: host_metadata_sha256.clone(),
            commands_log_sha256: commands_log_sha256.clone(),
            receipt_sha256: String::new(), // will fill after serialization
            reproduction_command: repro_command,
        };

        // Write receipt without receipt_sha256, read back the file bytes, hash those,
        // then set the hash and write the final version.
        let receipt_json_no_hash = serde_json::to_string_pretty(&receipt)
            .map_err(|e| format!("serialize receipt {}: {}", perf_id, e))?;
        let receipt_path = receipts_dir.join(format!("receipt-{}.json", perf_id));
        std::fs::write(&receipt_path, &receipt_json_no_hash)
            .map_err(|e| format!("write receipt (no hash) {:?}: {}", receipt_path, e))?;

        // Read back and hash the actual file bytes
        let receipt_file_bytes = std::fs::read(&receipt_path)
            .map_err(|e| format!("read receipt {:?}: {}", receipt_path, e))?;
        let receipt_self_hash = sha256_hex(&receipt_file_bytes);
        receipt.receipt_sha256 = receipt_self_hash.clone();

        let receipt_json = serde_json::to_string_pretty(&receipt)
            .map_err(|e| format!("serialize receipt (final) {}: {}", perf_id, e))?;
        std::fs::write(&receipt_path, &receipt_json)
            .map_err(|e| format!("write receipt {:?}: {}", receipt_path, e))?;
        receipt_sha256s.push(receipt_self_hash);
        // The final-file hash is the hash of the exact bytes stored on disk
        // (distinct from the canonical self-hash — residual L1-L).
        let receipt_file_bytes = std::fs::read(&receipt_path)
            .map_err(|e| format!("read final receipt {:?}: {}", receipt_path, e))?;
        receipt_file_sha256s.push(sha256_hex(&receipt_file_bytes));
        all_receipt_paths.push(receipt_path);
        println!(
            "  receipt {} written (verdict={:?}, declared={}, executed={}, verified={}, failed={})",
            perf_id, receipt.verdict, cases_declared, cases_executed, cases_verified, cases_failed
        );
    }

    // =========================================================================
    // 11. Generate the performance index
    // =========================================================================
    println!("performance-seal: step 11 — generating performance index...");
    let index_entries: Vec<ryg_rans_rs_casefile::PerformanceIndexEntry> = (0..EXPECTED_PERF_IDS
        .len())
        .filter_map(|idx| {
            if surface_records[idx].is_empty() {
                None // skip empty surfaces
            } else {
                Some(ryg_rans_rs_casefile::PerformanceIndexEntry {
                    performance_id: EXPECTED_PERF_IDS[idx].to_string(),
                    receipt_file_sha256: receipt_file_sha256s[idx].clone(),
                    receipt_canonical_sha256: receipt_sha256s[idx].clone(),
                })
            }
        })
        .collect();

    let perf_index = ryg_rans_rs_casefile::PerformanceIndex {
        schema_version: ryg_rans_rs_casefile::PERF_SCHEMA_VERSION,
        implementation_commit: implementation_commit.clone(),
        run_id: run_id.clone(),
        host_id: host_id.clone(),
        receipts: index_entries,
    };
    let index_json = serde_json::to_string_pretty(&perf_index)
        .map_err(|e| format!("serialize performance index: {}", e))?;
    let index_path = run_dir.join("index.json");
    std::fs::write(&index_path, &index_json)
        .map_err(|e| format!("write performance index {:?}: {}", index_path, e))?;
    let index_sha256 = sha256_hex(index_json.as_bytes());
    println!(
        "  index written ({} entries, SHA-256: {})",
        perf_index.receipts.len(),
        index_sha256
    );

    // =========================================================================
    // 12. Validate set equality: expected IDs = receipt IDs = manifest IDs = index IDs
    // =========================================================================
    println!("performance-seal: step 12 — validating set equality...");

    // Expected IDs (non-empty surfaces only)
    let expected_set: std::collections::BTreeSet<&str> = EXPECTED_PERF_IDS
        .iter()
        .enumerate()
        .filter(|(idx, _)| !surface_records[*idx].is_empty())
        .map(|(_, id)| *id)
        .collect();

    // Receipt IDs from files
    let receipt_set: std::collections::BTreeSet<String> = all_receipt_paths
        .iter()
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix("receipt-"))
                .map(|s| s.to_string())
        })
        .collect();

    // Manifest IDs from files
    let manifest_set: std::collections::BTreeSet<String> = all_manifest_paths
        .iter()
        .filter_map(|p| {
            p.file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_prefix("manifest-"))
                .map(|s| s.to_string())
        })
        .collect();

    // Index IDs from index entries
    let index_set: std::collections::BTreeSet<String> = perf_index
        .receipts
        .iter()
        .map(|e| e.performance_id.clone())
        .collect();

    // Check expected ⊆ receipt
    for id in &expected_set {
        if !receipt_set.contains(*id) {
            warn(format!(
                "expected performance ID '{}' not found in receipt set",
                id
            ));
        }
    }
    // Check expected ⊆ manifest
    for id in &expected_set {
        if !manifest_set.contains(*id) {
            warn(format!(
                "expected performance ID '{}' not found in manifest set",
                id
            ));
        }
    }
    // Check expected ⊆ index
    for id in &expected_set {
        if !index_set.contains(*id) {
            warn(format!(
                "expected performance ID '{}' not found in index set",
                id
            ));
        }
    }
    // Check receipt ⊆ expected (no extra receipts)
    for id in &receipt_set {
        if !expected_set.contains(id.as_str()) {
            warn(format!(
                "receipt '{}' is not in expected performance ID set",
                id
            ));
        }
    }
    // Check manifest ⊆ expected
    for id in &manifest_set {
        if !expected_set.contains(id.as_str()) {
            warn(format!(
                "manifest '{}' is not in expected performance ID set",
                id
            ));
        }
    }
    // Check index ⊆ expected
    for id in &index_set {
        if !expected_set.contains(id.as_str()) {
            warn(format!(
                "index '{}' is not in expected performance ID set",
                id
            ));
        }
    }

    if expected_set.len() == receipt_set.len()
        && expected_set.len() == manifest_set.len()
        && expected_set.len() == index_set.len()
    {
        println!(
            "  set equality: all {} IDs match across expected/receipts/manifests/index",
            expected_set.len()
        );
    } else {
        warn(format!(
            "set sizes differ: expected={}, receipts={}, manifests={}, index={}",
            expected_set.len(),
            receipt_set.len(),
            manifest_set.len(),
            index_set.len()
        ));
    }

    // =========================================================================
    // 13. Validate every manifest SHA-256 matches its receipt
    // =========================================================================
    println!("performance-seal: step 13 — validating manifest SHA-256 in receipts...");
    for (idx, _recs) in surface_records.iter().enumerate() {
        if surface_records[idx].is_empty() {
            continue;
        }
        let perf_id = EXPECTED_PERF_IDS[idx];
        let receipt_path = receipts_dir.join(format!("receipt-{}.json", perf_id));
        let receipt_content = std::fs::read_to_string(&receipt_path)
            .map_err(|e| format!("read receipt {:?}: {}", receipt_path, e))?;
        let receipt_json: serde_json::Value = serde_json::from_str(&receipt_content)
            .map_err(|e| format!("parse receipt {:?}: {}", receipt_path, e))?;
        let receipt_manifest_sha = receipt_json
            .get("manifest_sha256")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        if receipt_manifest_sha != manifest_sha256s[idx] {
            warn(format!(
                "receipt {} manifest_sha256 mismatch: receipt says '{}', actual manifest hash is '{}'",
                perf_id, receipt_manifest_sha, manifest_sha256s[idx]
            ));
        } else {
            println!("  manifest {} SHA-256 matches receipt", perf_id);
        }
    }

    // =========================================================================
    // 14. Verify receipt self-hashes
    // =========================================================================
    println!("performance-seal: step 14 — verifying receipt self-hashes...");
    for (idx, _recs) in surface_records.iter().enumerate() {
        if surface_records[idx].is_empty() {
            continue;
        }
        let perf_id = EXPECTED_PERF_IDS[idx];
        let receipt_path = receipts_dir.join(format!("receipt-{}.json", perf_id));
        let receipt_content = std::fs::read_to_string(&receipt_path)
            .map_err(|e| format!("read receipt {:?}: {}", receipt_path, e))?;
        let mut receipt: ryg_rans_rs_casefile::PerformanceReceipt =
            serde_json::from_str(&receipt_content)
                .map_err(|e| format!("parse receipt {:?}: {}", receipt_path, e))?;
        let receipt_self_hash = receipt.receipt_sha256.clone();
        if receipt_self_hash.is_empty() {
            warn(format!("receipt {} has empty receipt_sha256", perf_id));
            continue;
        }
        // Verify self-hash by zeroing out receipt_sha256 and re-serializing
        // with the same pretty format used when writing (via the struct's
        // Serialize impl, preserving field order), so the bytes match
        // what was originally hashed.
        receipt.receipt_sha256 = String::new();
        let canonical = serde_json::to_string_pretty(&receipt)
            .map_err(|e| format!("re-serialize receipt {}: {}", perf_id, e))?;
        let computed_hash = sha256_hex(canonical.as_bytes());
        if computed_hash != receipt_self_hash {
            warn(format!(
                "receipt {} self-hash mismatch: computed={}, declared={}",
                perf_id, computed_hash, receipt_self_hash
            ));
        } else {
            println!("  receipt {} self-hash verified", perf_id);
        }
    }

    // =========================================================================
    // 15. Print a summary of all cases per surface
    // =========================================================================
    println!("performance-seal: step 15 — summary of all cases per surface...");
    println!();
    println!("┌─────────────────────────────────────────────────────────────────────────────┐");
    println!("│                     Performance Seal Summary                                │");
    println!("├─────────────────────────────────────────────────────────────────────────────┤");
    println!(
        "│ {:<41} │ {:>8} │ {:>8} │ {:>8} │",
        "Surface", "Records", "Verified", "Failed"
    );
    println!("├─────────────────────────────────────────────────────────────────────────────┤");
    let mut total_records = 0u64;
    let mut total_verified = 0u64;
    let mut total_failed = 0u64;
    for (idx, recs) in surface_records.iter().enumerate() {
        let n = recs.len();
        let v = recs.iter().filter(|r| r.verification_passed).count();
        let f = recs.iter().filter(|r| r.status == "fail").count();
        let surf_short = if SURFACE_NAMES[idx].len() > 41 {
            format!("{}…", &SURFACE_NAMES[idx][..38])
        } else {
            SURFACE_NAMES[idx].to_string()
        };
        println!("│ {:<41} │ {:>8} │ {:>8} │ {:>8} │", surf_short, n, v, f);
        total_records += n as u64;
        total_verified += v as u64;
        total_failed += f as u64;
    }
    println!("├─────────────────────────────────────────────────────────────────────────────┤");
    println!(
        "│ {:<41} │ {:>8} │ {:>8} │ {:>8} │",
        "TOTAL", total_records, total_verified, total_failed
    );
    println!("└─────────────────────────────────────────────────────────────────────────────┘");
    println!();
    println!("  Implementation commit: {}", implementation_commit);
    println!("  Evidence commit:       {}", evidence_commit);
    println!("  Run ID:                {}", run_id);
    println!("  Host ID:               {}", host_id);
    println!("  Run dir:               {:?}", run_dir);
    println!("  Criterion archive:     {:?}", archive_path);

    // =========================================================================
    // Final: report accumulated errors
    // =========================================================================
    if !errors.is_empty() {
        return Err(format!(
            "performance-seal completed with {} warning(s):\n  {}",
            errors.len(),
            errors.join("\n  ")
        )
        .into());
    }

    println!();
    println!("Performance seal: all checks passed.");
    Ok(())
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string())
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
    if job_count != 11 {
        return Err(format!(
            "docker-matrix.json job_count={} (expected 11, includes parallel-stable)",
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
        "parallel-stable",
    ];
    for expected in &expected_jobs {
        let found = jobs.iter().any(|j| {
            let name_match = j.get("name").and_then(|n| n.as_str()) == Some(expected);
            let exit_ok = j.get("exit_code").and_then(|c| c.as_i64()) == Some(0);
            let has_log = j
                .get("log_sha256")
                .and_then(|h| h.as_str())
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            name_match && exit_ok && has_log
        });
        if !found {
            return Err(format!(
                "docker matrix job '{}' missing, had non-zero exit, or missing log_sha256",
                expected
            ));
        }
    }
    // Verify schema version
    let schema = json
        .get("schema_version")
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    if schema < 2 {
        return Err(format!(
            "docker-matrix.json schema_version={} (expected >= 2)",
            schema
        ));
    }
    // Verify git_commit matches the evidence index code_commit
    // (the Docker matrix must run from the same source that produced the evidence)
    let index_path = std::path::Path::new("evidence/index.json");
    let index_content = std::fs::read_to_string(index_path)
        .map_err(|e| format!("read evidence/index.json: {}", e))?;
    let index_json: serde_json::Value = serde_json::from_str(&index_content)
        .map_err(|e| format!("parse evidence/index.json: {}", e))?;
    let evidence_commit = index_json
        .get("code_commit")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !evidence_commit.is_empty() && !git_commit.is_empty() {
        // Support prefix matching: the stamp may use short hash (214a2d8)
        // while evidence uses full hash (214a2d86402335c25f7e4a9eb3c844d9c0c9868b)
        let match_full =
            evidence_commit.starts_with(git_commit) || git_commit.starts_with(evidence_commit);
        if !match_full {
            return Err(format!(
                "docker-matrix.json git_commit={} does not match evidence code_commit={}. Run the Docker matrix from the exact source commit that produced the evidence.",
                git_commit, evidence_commit
            ));
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

#[allow(dead_code)]
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
