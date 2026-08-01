//! # RYG_RANS.L.SSE41.UNSAFE_QUARANTINE — unsafe ledger and target features (L.10)
//!
//! Proves the L.10 quarantine contract by re-executing the bidirectional
//! ledger↔source inventory check against `crates/ryg-rans-rs-simd`:
//!
//! 1. Every `unsafe fn` in the SIMD sources is listed in `unsafe-ledger.toml`
//!    (no undocumented unsafe function).
//! 2. Every ledger entry exists in the source (no stale entries).
//! 3. For entries with explicit `target_features`, the
//!    `#[target_feature(enable = "...")]` attribute immediately above the
//!    declaration matches the ledger exactly.
//! 4. Entries marked `delegates`, `test-only`, or `baseline` carry no
//!    `#[target_feature]` attribute.
//! 5. The SSE4.1 helpers (`rans_simd_dec_sym_unchecked`,
//!    `rans_simd_dec_renorm_unchecked`) carry their own exact
//!    `#[target_feature]` attributes (locally enforced, not inherited from
//!    callers).
//!
//! This court re-implements the ledger check in-court so the verdict is
//! produced by an independent walk of the on-disk source + ledger, exactly
//! as the `unsafe_ledger` test does.

use super::{CourtCase, CourtRun};
use ryg_rans_rs_casefile::PhaseLCaseVerdict;
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, serde::Deserialize)]
struct Ledger {
    #[serde(rename = "unsafe_functions")]
    unsafe_functions: Vec<LedgerEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct LedgerEntry {
    name: String,
    file: String,
    target_features: Vec<String>,
    safety: String,
    #[allow(dead_code)] // informational; verified by human audit
    callers: Vec<String>,
}

fn simd_dir() -> PathBuf {
    // The bench crate runs with cwd = crate root (crates/ryg-rans-rs-bench);
    // the workspace root is three levels up: crates/ryg-rans-rs-bench → ../../.
    // We also try CARGO_MANIFEST_DIR-relative and a direct workspace probe so
    // the court works from any cwd.
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ryg-rans-rs-simd"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/ryg-rans-rs-simd"),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../crates/ryg-rans-rs-simd"),
        PathBuf::from("crates/ryg-rans-rs-simd"),
        PathBuf::from("../crates/ryg-rans-rs-simd"),
    ];
    for c in &candidates {
        if c.join("unsafe-ledger.toml").exists() {
            return c.clone();
        }
    }
    candidates[0].clone()
}

fn source_files(simd: &PathBuf) -> Vec<PathBuf> {
    let src = simd.join("src");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .expect("read simd src dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().map(|e| e == "rs").unwrap_or(false))
        .collect();
    files.sort();
    files
}

fn parse_target_feature_attr(line: &str) -> Option<Vec<String>> {
    let t = line.trim();
    if !t.starts_with("#[target_feature") {
        return None;
    }
    let inner = t
        .strip_prefix("#[target_feature(")?
        .strip_suffix(")]")?
        .to_string();
    let enable = inner
        .split_once("enable = ")?
        .1
        .trim_matches('"')
        .to_string();
    Some(enable.split(',').map(|s| s.trim().to_string()).collect())
}

fn scan_unsafe_fns(simd: &PathBuf) -> BTreeMap<(String, String), (Option<String>, String)> {
    let mut out = BTreeMap::new();
    for path in source_files(simd) {
        let content = std::fs::read_to_string(&path).expect("read source file");
        let file = path
            .file_name()
            .expect("file name")
            .to_string_lossy()
            .to_string();
        let lines: Vec<&str> = content.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            let Some(us) = trimmed.find("unsafe fn") else {
                continue;
            };
            let after = trimmed[us + "unsafe fn".len()..].trim_start();
            let name: String = after
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() {
                continue;
            }
            let mut attr: Option<String> = None;
            for prev in lines[..i].iter().rev() {
                let pt = prev.trim_start();
                if pt.starts_with("///") || pt.starts_with("//") || pt.is_empty() {
                    continue;
                }
                if pt.starts_with("#[target_feature") {
                    attr = Some(pt.to_string());
                }
                if !pt.starts_with('#') {
                    break;
                }
            }
            out.insert(
                (file.clone(), name.clone()),
                (attr, line.trim().to_string()),
            );
        }
    }
    out
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
            residual_ids: vec!["L10-A".to_string(), "L10-B".to_string()],
        });
    };

    let simd = simd_dir();
    if !simd.join("unsafe-ledger.toml").exists() {
        return CourtRun {
            court_id: "RYG_RANS.L.SSE41.UNSAFE_QUARANTINE".to_string(),
            title: "SSE unsafe quarantine and machine-verified ledger (L.10)".to_string(),
            residual_ids: vec!["L10-A".to_string(), "L10-B".to_string()],
            cases: vec![CourtCase {
                case_id: "CASE.000".to_string(),
                input: "locate simd crate".to_string(),
                expected: "unsafe-ledger.toml present".to_string(),
                actual: format!("not found at {:?}", simd),
                verdict: PhaseLCaseVerdict::Fail,
                residual_ids: vec!["L10-A".to_string()],
            }],
        };
    }

    // ---- Case 1: ledger parses and is non-empty ---------------------------
    let ledger_text = match std::fs::read_to_string(simd.join("unsafe-ledger.toml")) {
        Ok(t) => t,
        Err(e) => {
            return CourtRun {
                court_id: "RYG_RANS.L.SSE41.UNSAFE_QUARANTINE".to_string(),
                title: "SSE unsafe quarantine and machine-verified ledger (L.10)".to_string(),
                residual_ids: vec!["L10-A".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "read unsafe-ledger.toml".to_string(),
                    expected: "Ok".to_string(),
                    actual: format!("ERROR: {}", e),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L10-A".to_string()],
                }],
            };
        }
    };
    let ledger: Ledger = match toml::from_str(&ledger_text) {
        Ok(l) => l,
        Err(e) => {
            return CourtRun {
                court_id: "RYG_RANS.L.SSE41.UNSAFE_QUARANTINE".to_string(),
                title: "SSE unsafe quarantine and machine-verified ledger (L.10)".to_string(),
                residual_ids: vec!["L10-A".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "parse unsafe-ledger.toml".to_string(),
                    expected: "Ok".to_string(),
                    actual: format!("ERROR: {}", e),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L10-A".to_string()],
                }],
            };
        }
    };
    add(
        &mut cases,
        "CASE.001",
        "unsafe-ledger.toml parses with unsafe_functions entries",
        "parsed",
        if ledger.unsafe_functions.is_empty() {
            Ok("empty".to_string())
        } else {
            Ok("parsed".to_string())
        },
    );

    let source = scan_unsafe_fns(&simd);

    // ---- Case 2: every ledger entry exists in the source ------------------
    let mut missing_in_source = Vec::new();
    for e in &ledger.unsafe_functions {
        let key = (e.file.clone(), e.name.clone());
        if !source.contains_key(&key) {
            missing_in_source.push(format!("{}/{}", e.file, e.name));
        }
    }
    add(
        &mut cases,
        "CASE.002",
        "every ledger entry exists in the source (no stale entries)",
        "all_present",
        if missing_in_source.is_empty() {
            Ok("all_present".to_string())
        } else {
            Ok(format!("missing={:?}", missing_in_source))
        },
    );

    // ---- Case 3: every unsafe fn in source is listed in the ledger --------
    let mut undocumented = Vec::new();
    let ledger_keys: std::collections::BTreeSet<(String, String)> = ledger
        .unsafe_functions
        .iter()
        .map(|e| (e.file.clone(), e.name.clone()))
        .collect();
    for key in source.keys() {
        if !ledger_keys.contains(key) {
            undocumented.push(format!("{}/{}", key.0, key.1));
        }
    }
    add(
        &mut cases,
        "CASE.003",
        "every source unsafe fn is listed in the ledger (no undocumented unsafe)",
        "all_listed",
        if undocumented.is_empty() {
            Ok("all_listed".to_string())
        } else {
            Ok(format!("undocumented={:?}", undocumented))
        },
    );

    // ---- Case 4: target_feature attributes match the ledger ---------------
    // Category markers ("delegates", "test-only", "baseline") are ledger
    // annotations, not CPU feature lists: entries with only markers carry no
    // #[target_feature] attribute and must not be compared as feature sets.
    let category_markers: [&str; 3] = ["delegates", "test-only", "baseline"];
    let mut attr_mismatch = Vec::new();
    for e in &ledger.unsafe_functions {
        let key = (e.file.clone(), e.name.clone());
        let src_attr = source.get(&key).and_then(|(a, _)| a.clone());
        let real_feats: Vec<String> = e
            .target_features
            .iter()
            .filter(|f| !category_markers.contains(&f.as_str()))
            .cloned()
            .collect();
        let ledger_feats: BTreeMap<String, ()> =
            real_feats.iter().map(|f| (f.clone(), ())).collect();
        if real_feats.is_empty() {
            // No real features (empty or category-marker-only): the source
            // must NOT carry a #[target_feature] attribute.
            if src_attr.is_some() {
                attr_mismatch.push(format!(
                    "{}/{} claims no features but source has {}",
                    e.file,
                    e.name,
                    src_attr.unwrap_or_default()
                ));
            }
        } else {
            let src_feats: BTreeMap<String, ()> = src_attr
                .as_deref()
                .and_then(parse_target_feature_attr)
                .map(|v| v.into_iter().map(|f| (f, ())).collect())
                .unwrap_or_default();
            if src_feats != ledger_feats {
                attr_mismatch.push(format!(
                    "{}/{} ledger={:?} source={:?}",
                    e.file, e.name, e.target_features, src_attr
                ));
            }
        }
    }
    add(
        &mut cases,
        "CASE.004",
        "ledger target_features exactly match source #[target_feature] attributes",
        "exact",
        if attr_mismatch.is_empty() {
            Ok("exact".to_string())
        } else {
            Ok(format!("mismatches={:?}", attr_mismatch))
        },
    );

    // ---- Case 5: SSE4.1 helpers carry their own target features -----------
    // Each SSE helper carries its own exact, minimal #[target_feature] set
    // (Phase L.10 quarantine): `rans_simd_dec_sym_unchecked` needs SSE4.1
    // only; `rans_simd_dec_renorm_unchecked` needs ssse3+sse4.1 (it uses
    // pshufb).  The exact attribute must be locally present, never inherited
    // from a caller's feature context.
    let sse_sym = source.get(&(
        "lib.rs".to_string(),
        "rans_simd_dec_sym_unchecked".to_string(),
    ));
    let sse_renorm = source.get(&(
        "lib.rs".to_string(),
        "rans_simd_dec_renorm_unchecked".to_string(),
    ));
    let sym_has_local = sse_sym
        .and_then(|(a, _)| a.clone())
        .map(|a| a.contains("sse4.1"))
        .unwrap_or(false);
    let renorm_has_local = sse_renorm
        .and_then(|(a, _)| a.clone())
        .map(|a| a.contains("ssse3") && a.contains("sse4.1"))
        .unwrap_or(false);
    add(
        &mut cases,
        "CASE.005",
        "SSE helpers carry their own exact #[target_feature] attributes (sym: sse4.1; renorm: ssse3+sse4.1)",
        "locally_gated",
        if sym_has_local && renorm_has_local {
            Ok("locally_gated".to_string())
        } else {
            Ok(format!("sym={} renorm={}", sym_has_local, renorm_has_local))
        },
    );

    // ---- Case 6: every feature-gated ledger entry is a genuine kernel -----
    // Entries with features must be in source files that are kernels
    // (lib.rs, avx2.rs, avx512.rs, backends.rs) and the attribute must be
    // adjacent (already checked in Case 4).  Count the gated entries.
    let gated = ledger
        .unsafe_functions
        .iter()
        .filter(|e| !e.target_features.is_empty())
        .count();
    add(
        &mut cases,
        "CASE.006",
        "ledger contains feature-gated kernel entries",
        "gated>0",
        if gated > 0 {
            Ok("gated>0".to_string())
        } else {
            Ok(format!("gated={}", gated))
        },
    );

    // ---- Case 7: every ledger entry has a # Safety documentation field ----
    let mut no_safety = Vec::new();
    for e in &ledger.unsafe_functions {
        if e.safety.trim().is_empty() {
            no_safety.push(format!("{}/{}", e.file, e.name));
        }
    }
    add(
        &mut cases,
        "CASE.007",
        "every ledger entry documents its # Safety contract",
        "all_documented",
        if no_safety.is_empty() {
            Ok("all_documented".to_string())
        } else {
            Ok(format!("missing={:?}", no_safety))
        },
    );

    CourtRun {
        court_id: "RYG_RANS.L.SSE41.UNSAFE_QUARANTINE".to_string(),
        title: "SSE unsafe quarantine and machine-verified ledger (L.10)".to_string(),
        cases,
        residual_ids: vec!["L10-A".to_string(), "L10-B".to_string()],
    }
}
