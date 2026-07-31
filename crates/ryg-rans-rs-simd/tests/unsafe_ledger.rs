//! # Unsafe-ledger consistency test (Phase L.10)
//!
//! Verifies that the machine-readable unsafe ledger
//! (`crates/ryg-rans-rs-simd/unsafe-ledger.toml`) matches the source
//! inventory **exactly**, in both directions:
//!
//! 1. Every `unsafe fn` declared in `src/*.rs` is listed in the ledger
//!    (no undocumented unsafe function can be added silently).
//! 2. Every ledger entry exists in the source as an `unsafe fn` (no stale
//!    ledger entry for a deleted function).
//! 3. For entries with explicit `target_features`, the
//!    `#[target_feature(enable = "...")]` attribute immediately above the
//!    declaration matches the ledger value exactly.
//! 4. Entries marked `delegates`, `test-only`, or `baseline` in the ledger
//!    must **not** carry a `#[target_feature]` attribute — the ledger must
//!    not claim functions are feature-gated unless source inspection proves
//!    that statement.

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
    #[allow(dead_code)] // informational; verified by human audit, not by this test
    safety: String,
    #[allow(dead_code)] // informational; verified by human audit, not by this test
    callers: Vec<String>,
}

fn source_files() -> Vec<PathBuf> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .expect("read src dir")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().map(|e| e == "rs").unwrap_or(false))
        .collect();
    files.sort();
    files
}

/// Parse the feature list out of a `#[target_feature(enable = "a,b")]`
/// attribute line.  Returns `None` if the line is not such an attribute.
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

/// Scan the source for `unsafe fn` declarations.
///
/// Returns `(file, name) -> (attr_line or None, declaration line)`.
fn scan_unsafe_fns() -> BTreeMap<(String, String), (Option<String>, String)> {
    let mut out = BTreeMap::new();
    for path in source_files() {
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
            // Match `unsafe fn NAME(` (with optional `pub`).
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
            // Walk up to find the nearest #[target_feature] attribute,
            // skipping doc comments and other attributes.
            let mut attr: Option<String> = None;
            for prev in lines[..i].iter().rev() {
                let pt = prev.trim_start();
                if pt.starts_with("///") || pt.starts_with("//") || pt.is_empty() {
                    continue;
                }
                if pt.starts_with("#[target_feature") {
                    attr = Some(pt.to_string());
                }
                // Stop at the first non-doc, non-attribute line (the
                // previous item's body or declaration).
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

#[test]
fn ledger_matches_source_inventory() {
    let ledger_text = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("unsafe-ledger.toml"),
    )
    .expect("read unsafe-ledger.toml");
    let ledger: Ledger = toml::from_str(&ledger_text).expect("parse unsafe-ledger.toml");

    let source = scan_unsafe_fns();

    // ---- Direction 1: every ledger entry exists in the source ----
    let mut ledger_keys: BTreeMap<(String, String), &LedgerEntry> = BTreeMap::new();
    for e in &ledger.unsafe_functions {
        let key = (e.file.clone(), e.name.clone());
        assert!(
            source.contains_key(&key),
            "ledger entry {}/{} not found in source",
            e.file,
            e.name
        );
        ledger_keys.insert(key, e);
    }

    // ---- Direction 2: every source unsafe fn is in the ledger ----
    for (key, (_attr, decl)) in &source {
        assert!(
            ledger_keys.contains_key(key),
            "source {}/{} ({}) is missing from unsafe-ledger.toml",
            key.0,
            key.1,
            decl
        );
    }

    // ---- Direction 3: target-feature attributes match ----
    for ((file, name), entry) in &ledger_keys {
        let (src_attr, decl) = &source[&(file.clone(), name.clone())];
        let features = &entry.target_features;
        if features.is_empty() {
            panic!(
                "ledger {file}/{name}: target_features must be non-empty (use \"delegates\"/\"test-only\"/\"baseline\")"
            );
        }
        if features == &vec!["delegates".to_string()]
            || features == &vec!["test-only".to_string()]
            || features == &vec!["baseline".to_string()]
        {
            // No attribute expected.
            assert!(
                src_attr.is_none(),
                "{file}/{name}: ledger says {:?} but source has attribute {src_attr:?}",
                features[0]
            );
            continue;
        }
        // Explicit feature list: the source attribute must match exactly.
        let src_list = src_attr
            .as_ref()
            .and_then(|a| parse_target_feature_attr(a))
            .unwrap_or_else(|| {
                panic!(
                    "{file}/{name}: ledger expects features {features:?} but source has no/other attribute ({src_attr:?})"
                )
            });
        assert_eq!(
            &src_list, features,
            "{file}/{name}: attribute feature mismatch (decl: {decl})"
        );
    }
}
