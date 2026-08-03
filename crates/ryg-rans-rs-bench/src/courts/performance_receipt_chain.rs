//! # RYG_RANS.L.PERFORMANCE.RECEIPT_CHAIN — receipt hash chain (L.1-L)
//!
//! Proves the L.1-L receipt-chain model against the **actual sealed
//! performance run** on disk:
//!
//! - `receipt_file_sha256` hashes the exact final receipt file bytes stored
//!   on disk.
//! - `receipt_canonical_sha256` hashes canonical receipt content with its
//!   self-hash field omitted — the two are distinct and both verify.
//! - The run-local index (`evidence/performance/runs/<run>/index.json`)
//!   entries match the receipt files.
//! - The canonical top-level index (`evidence/performance/index.json`)
//!   identifies the active run and hashes the run-local index.
//! - Every receipt's `implementation_commit` equals the run's
//!   implementation commit.
//! - Manifest SHA-256 matches each receipt's `manifest_sha256`.
//!
//! The court reads the sealed artifacts and re-computes hashes independently
//! (SHA-256 of the on-disk bytes), so a doctored file fails the chain.

use super::{CourtCase, CourtRun};
use ryg_rans_rs_casefile::PhaseLCaseVerdict;
use std::path::{Path, PathBuf};

fn sha256_bytes(data: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn read_str(p: &Path) -> Option<String> {
    std::fs::read_to_string(p).ok()
}

/// Locate the performance evidence root relative to the bench crate.
fn perf_root() -> std::path::PathBuf {
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../evidence/performance"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evidence/performance"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../evidence/performance"),
        PathBuf::from("evidence/performance"),
        PathBuf::from("../evidence/performance"),
    ];
    for c in &candidates {
        if c.join("index.json").exists() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

pub fn court() -> CourtRun {
    let mut cases = Vec::new();
    let add = |cases: &mut Vec<CourtCase>,
               id: &str,
               input: &str,
               expected: &str,
               actual: Result<String, String>| {
        let actual_str = match &actual {
            Ok(a) => a.clone(),
            Err(e) => format!("ERROR: {}", e),
        };
        let verdict = match &actual {
            Ok(a) if a == expected => PhaseLCaseVerdict::Pass,
            _ => PhaseLCaseVerdict::Fail,
        };
        cases.push(CourtCase {
            case_id: id.to_string(),
            input: input.to_string(),
            expected: expected.to_string(),
            actual: actual_str,
            verdict,
            residual_ids: vec!["L1-L".to_string()],
        });
    };

    let root = perf_root();
    let index_path = root.join("index.json");
    let index_content = match read_str(&index_path) {
        Some(c) => c,
        None => {
            return CourtRun {
                court_id: "RYG_RANS.L.PERFORMANCE.RECEIPT_CHAIN".to_string(),
                title: "Performance receipt hash chain (L.1-L)".to_string(),
                residual_ids: vec!["L1-L".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "locate evidence/performance/index.json".to_string(),
                    expected: "present".to_string(),
                    actual: format!("missing (searched from {:?})", root),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L1-L".to_string()],
                }],
            };
        }
    };
    let index: serde_json::Value = match serde_json::from_str(&index_content) {
        Ok(v) => v,
        Err(e) => {
            return CourtRun {
                court_id: "RYG_RANS.L.PERFORMANCE.RECEIPT_CHAIN".to_string(),
                title: "Performance receipt hash chain (L.1-L)".to_string(),
                residual_ids: vec!["L1-L".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "parse evidence/performance/index.json".to_string(),
                    expected: "valid JSON".to_string(),
                    actual: format!("ERROR: {}", e),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L1-L".to_string()],
                }],
            };
        }
    };

    // ---- Case 1: top-level index has exactly fifteen entries -------------
    // Phase O added five cache surfaces (RYG_RANS.PERF.CACHE.*) to the ten
    // Phase L surfaces; the count is pinned here so the court breaks loudly
    // if the surface model ever changes again.
    let entries = index
        .get("receipts")
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default();
    add(
        &mut cases,
        "CASE.001",
        "top-level performance index entries",
        "15",
        if entries.len() == 15 {
            Ok("15".to_string())
        } else {
            Ok(format!("{}", entries.len()))
        },
    );

    // ---- Case 2: active run dir exists and its index parses ---------------
    let active_run = index
        .get("active_run")
        .and_then(|a| a.as_str())
        .unwrap_or("");
    let run_dir = root.join(active_run.trim_start_matches("evidence/performance/"));
    let run_index_content = read_str(&run_dir.join("index.json"));
    let run_index_ok = run_index_content.is_some()
        && serde_json::from_str::<serde_json::Value>(run_index_content.as_deref().unwrap_or("{}"))
            .is_ok();
    add(
        &mut cases,
        "CASE.002",
        &format!("active run {} has a parseable run index", active_run),
        "present",
        if run_index_ok {
            Ok("present".to_string())
        } else {
            Ok(format!("missing or unparseable: {}", run_dir.display()))
        },
    );

    // ---- Case 3: run-index sha256 in the top-level index verifies ---------
    let run_index_json = run_index_content.unwrap_or_default();
    let run_index_sha = sha256_bytes(run_index_json.as_bytes());
    let declared_run_index_sha = index
        .get("run_index_sha256")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    add(
        &mut cases,
        "CASE.003",
        "top-level run_index_sha256 equals SHA-256 of the run index file",
        "matches",
        if run_index_sha == declared_run_index_sha && !declared_run_index_sha.is_empty() {
            Ok("matches".to_string())
        } else {
            Ok(format!(
                "computed={} declared={}",
                run_index_sha, declared_run_index_sha
            ))
        },
    );

    // ---- Case 4: every receipt file exists and its file hash matches ------
    // Receipts and manifests live under the run directory, not at the top
    // level of evidence/performance/.
    let run_receipts_dir = run_dir.join("receipts");
    let run_manifests_dir = run_dir.join("manifests");
    let mut all_file_hashes_ok = true;
    let mut bad_entries = Vec::new();
    let mut implementation_commits: Vec<String> = Vec::new();
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
        let rp = run_receipts_dir.join(format!("receipt-{}.json", pid));
        match std::fs::read(&rp) {
            Ok(bytes) => {
                let actual_file_sha = sha256_bytes(&bytes);
                if actual_file_sha != file_sha {
                    all_file_hashes_ok = false;
                    bad_entries.push(format!("{} file-hash mismatch", pid));
                }
                // Canonical self-hash: the sealer serialized the typed
                // `PerformanceReceipt` struct (serde derive, struct field
                // order) with `receipt_sha256` emptied and hashed those exact
                // bytes.  Re-serializing through `serde_json::Value` would
                // sort keys (BTreeMap), so the court must use the same typed
                // struct — never a generic JSON value (residual L1-L).
                if let Ok(content) = String::from_utf8(bytes.clone()) {
                    match serde_json::from_str::<ryg_rans_rs_casefile::PerformanceReceipt>(&content)
                    {
                        Ok(mut rec) => {
                            rec.receipt_sha256 = String::new();
                            let canonical = serde_json::to_string_pretty(&rec).unwrap_or_default();
                            let computed_canon = sha256_bytes(canonical.as_bytes());
                            if computed_canon != canon_sha {
                                all_file_hashes_ok = false;
                                bad_entries.push(format!("{} canonical-hash mismatch", pid));
                            }
                        }
                        Err(_) => {
                            all_file_hashes_ok = false;
                            bad_entries.push(format!("{} unparseable receipt", pid));
                        }
                    }
                }
                // Collect implementation commits.
                if let Ok(content) = String::from_utf8(bytes) {
                    if let Ok(rec) = serde_json::from_str::<serde_json::Value>(&content) {
                        if let Some(c) = rec.get("implementation_commit").and_then(|s| s.as_str()) {
                            implementation_commits.push(c.to_string());
                        }
                    }
                }
            }
            Err(_) => {
                all_file_hashes_ok = false;
                bad_entries.push(format!("{} file missing", pid));
            }
        }
    }
    add(
        &mut cases,
        "CASE.004",
        "all fifteen receipt final-file hashes AND canonical self-hashes verify",
        "all_verify",
        if all_file_hashes_ok && bad_entries.is_empty() {
            Ok("all_verify".to_string())
        } else {
            Ok(format!("bad={:?}", bad_entries))
        },
    );

    // ---- Case 5: every receipt binds the same implementation commit -------
    let commits_uniform = !implementation_commits.is_empty()
        && implementation_commits
            .iter()
            .all(|c| c == &implementation_commits[0]);
    let index_impl_commit = index
        .get("implementation_commit")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    add(
        &mut cases,
        "CASE.005",
        "all receipts share the index implementation_commit",
        "uniform",
        if commits_uniform && implementation_commits[0] == index_impl_commit {
            Ok("uniform".to_string())
        } else {
            Ok(format!(
                "commits={:?} index={}",
                implementation_commits, index_impl_commit
            ))
        },
    );

    // ---- Case 6: every manifest exists and its hash matches ----------------
    let mut manifests_ok = true;
    let mut bad_manifests = Vec::new();
    for e in &entries {
        let pid = e
            .get("performance_id")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let mp = run_manifests_dir.join(format!("manifest-{}.json", pid));
        let rp = run_receipts_dir.join(format!("receipt-{}.json", pid));
        match (read_str(&mp), read_str(&rp)) {
            (Some(m_content), Some(r_content)) => {
                let m_sha = sha256_bytes(m_content.as_bytes());
                let rec: serde_json::Value = serde_json::from_str(&r_content).unwrap_or_default();
                let declared = rec
                    .get("manifest_sha256")
                    .and_then(|s| s.as_str())
                    .unwrap_or("");
                if m_sha != declared {
                    manifests_ok = false;
                    bad_manifests.push(format!("{} manifest hash mismatch", pid));
                }
            }
            _ => {
                manifests_ok = false;
                bad_manifests.push(format!("{} manifest missing", pid));
            }
        }
    }
    add(
        &mut cases,
        "CASE.006",
        "every receipt's manifest_sha256 matches its manifest file",
        "all_match",
        if manifests_ok {
            Ok("all_match".to_string())
        } else {
            Ok(format!("bad={:?}", bad_manifests))
        },
    );

    // ---- Case 7: expected performance ID set equals index set -------------
    let expected: [&str; 10] = [
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
    ];
    let mut index_ids: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("performance_id").and_then(|s| s.as_str()))
        .map(String::from)
        .collect();
    index_ids.sort();
    let mut expected_ids: Vec<String> = expected.iter().map(|s| s.to_string()).collect();
    expected_ids.sort();
    add(
        &mut cases,
        "CASE.007",
        "expected performance ID set equals index ID set",
        "set_equal",
        if index_ids == expected_ids {
            Ok("set_equal".to_string())
        } else {
            Ok(format!("index={:?} expected={:?}", index_ids, expected_ids))
        },
    );

    // ---- Case 8: raw Criterion archive exists -----------------------------
    let archive = run_dir.join("criterion.tar.zst");
    add(
        &mut cases,
        "CASE.008",
        "raw Criterion archive (criterion.tar.zst) present in the run dir",
        "present",
        if archive.exists() {
            Ok("present".to_string())
        } else {
            Ok("missing".to_string())
        },
    );

    // ---- Case 9: results JSON and CSV exist with declared hashes ----------
    let mut artifacts_ok = true;
    let mut bad_artifacts = Vec::new();
    for e in &entries {
        let pid = e
            .get("performance_id")
            .and_then(|s| s.as_str())
            .unwrap_or("");
        let surface_dir = run_dir.join(pid);
        for (fname, key) in [
            ("results.json", "results_json_sha256"),
            ("results.csv", "results_csv_sha256"),
        ] {
            let p = surface_dir.join(fname);
            let rp = run_receipts_dir.join(format!("receipt-{}.json", pid));
            let declared = match read_str(&rp) {
                Some(c) => match serde_json::from_str::<serde_json::Value>(&c) {
                    Ok(v) => v
                        .get(key)
                        .and_then(|s| s.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default(),
                    Err(_) => String::new(),
                },
                None => String::new(),
            };
            match std::fs::read(&p) {
                Ok(bytes) => {
                    let actual = sha256_bytes(&bytes);
                    if actual != declared {
                        artifacts_ok = false;
                        bad_artifacts.push(format!("{} {}", pid, key));
                    }
                }
                Err(_) => {
                    artifacts_ok = false;
                    bad_artifacts.push(format!("{} missing {}", pid, fname));
                }
            }
        }
    }
    // host.json and commands.log live at the run level; verify them against
    // the first receipt's declared hashes.
    let first_rec = run_receipts_dir.join("receipt-RYG_RANS.PERF.BYTE.json");
    if let (Some(rj), Some(hbytes), Some(cbytes)) = (
        read_str(&first_rec),
        std::fs::read(run_dir.join("host.json")).ok(),
        std::fs::read(run_dir.join("commands.log")).ok(),
    ) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&rj) {
            let hdecl = v
                .get("host_metadata_sha256")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let cdecl = v
                .get("commands_log_sha256")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let hact = sha256_bytes(&hbytes);
            let cact = sha256_bytes(&cbytes);
            if hact != hdecl || cact != cdecl {
                artifacts_ok = false;
                bad_artifacts.push(format!(
                    "host/commands: h {}=={} c {}=={}",
                    hact == hdecl,
                    cact == cdecl,
                    hact,
                    cact
                ));
            }
        }
    }
    add(
        &mut cases,
        "CASE.009",
        "results.json / results.csv / host / commands artifacts hash-verify",
        "all_verify",
        if artifacts_ok {
            Ok("all_verify".to_string())
        } else {
            Ok(format!("bad={:?}", bad_artifacts))
        },
    );

    CourtRun {
        court_id: "RYG_RANS.L.PERFORMANCE.RECEIPT_CHAIN".to_string(),
        title: "Performance receipt hash chain (L.1-L)".to_string(),
        cases,
        residual_ids: vec!["L1-L".to_string()],
    }
}
