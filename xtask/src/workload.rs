//! # Public-corpus workload tooling (Phase O.10/O.11)
//!
//! `cargo xtask workload fetch public-rans-v1` downloads, hash-verifies, and
//! safely extracts the pinned public corpora into a cache directory outside
//! the Git repository.  `cargo xtask workload derive public-rans-v1` expands
//! the deterministic derivation policy into ordered `WorkloadBlock`
//! schedules — a small slice manifest that never materializes the tens of
//! gigabytes it describes.
//!
//! ## Trust model
//!
//! * **HTTPS only.**  Every pinned URL is `https://`; a non-HTTPS URL is a
//!   hard error (the spec records the one secure-endpoint caveat if ever
//!   needed).
//! * **Hash-pinned.**  Every archive and every extracted file must match
//!   `expected-source-hashes.json` (SHA-256 + byte length).  A mismatch
//!   aborts the fetch; nothing is ever partially trusted.
//! * **Never execute downloads.**  Archives are decoded with the maintained
//!   `zip`/`flate2`/`tar` crates — never shelled out.
//! * **Traversal-proof extraction.**  Archive entry names containing `..`,
//!   a leading `/`, or a backslash are rejected; every output path is
//!   verified to stay inside the extraction root.
//! * **Offline reuse.**  An archive already cached whose hash matches is
//!   reused without a download.
//!
//! ## Determinism
//!
//! The derive expands the policy with pure index arithmetic (no RNG, no
//! hash-map iteration order); the same policy bytes + same source hashes
//! produce the same schedule hash every time.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Entry point: `cargo xtask workload <fetch|derive> <workload-name>`.
pub fn cmd_workload(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("usage: cargo xtask workload <fetch|derive> [public-rans-v1]".into());
    }
    let sub = args[0].as_str();
    let name = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "public-rans-v1".to_string());
    let spec_dir = PathBuf::from("workloads").join(&name);
    if !spec_dir.is_dir() {
        return Err(format!(
            "unknown workload spec directory: {}",
            spec_dir.display()
        ));
    }
    match sub {
        "fetch" => workload_fetch(&spec_dir, &name),
        "derive" => workload_derive(&spec_dir, &name),
        other => Err(format!("unknown workload subcommand: {}", other)),
    }
}

/// Resolve the workload cache root: `RYG_RANS_WORKLOAD_DIR` or
/// `~/.cache/ryg-rans-rs/workloads/<name>`.  Always outside the repository.
fn cache_root(name: &str) -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("RYG_RANS_WORKLOAD_DIR") {
        let p = PathBuf::from(dir);
        std::fs::create_dir_all(&p).map_err(|e| format!("create cache dir: {}", e))?;
        return Ok(p.join(name));
    }
    let home = std::env::var("HOME").map_err(|_| "HOME not set".to_string())?;
    let p = PathBuf::from(home)
        .join(".cache")
        .join("ryg-rans-rs")
        .join("workloads")
        .join(name);
    std::fs::create_dir_all(&p).map_err(|e| format!("create cache dir: {}", e))?;
    Ok(p)
}

fn sha256_hex(data: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn sha256_file(p: &Path) -> Result<String, String> {
    let data = std::fs::read(p).map_err(|e| format!("read {}: {}", p.display(), e))?;
    Ok(sha256_hex(&data))
}

// ---------------------------------------------------------------------------
// fetch
// ---------------------------------------------------------------------------

/// Reject an archive entry path that could escape the extraction root.
///
/// Allowed: relative paths with `/` separators that never contain `..`
/// components, never start with `/`, and never contain a backslash.
fn sanitize_entry_name(name: &str) -> Result<&str, String> {
    if name.is_empty()
        || name.starts_with('/')
        || name.contains('\\')
        || name.split('/').any(|c| c == ".." || c.is_empty())
    {
        return Err(format!("unsafe archive path: {:?}", name));
    }
    Ok(name)
}

/// Safe zip extraction: every entry is sanitized and written under `root`.
fn extract_zip(archive: &Path, root: &Path) -> Result<(), String> {
    use std::io::Write;
    let file =
        std::fs::File::open(archive).map_err(|e| format!("open {}: {}", archive.display(), e))?;
    let mut zip =
        zip::ZipArchive::new(file).map_err(|e| format!("parse {}: {}", archive.display(), e))?;
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index(i)
            .map_err(|e| format!("zip entry {}: {}", i, e))?;
        let name = sanitize_entry_name(entry.name())?;
        let target = root.join(name);
        if !target.starts_with(root) {
            return Err(format!("escape detected: {}", target.display()));
        }
        if entry.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("mkdir {}: {}", target.display(), e))?;
        } else {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
            }
            let mut out = std::fs::File::create(&target)
                .map_err(|e| format!("create {}: {}", target.display(), e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("extract {}: {}", target.display(), e))?;
            out.flush()
                .map_err(|e| format!("flush {}: {}", target.display(), e))?;
        }
    }
    Ok(())
}

/// Safe gzip extraction: the payload is written to `root/<out_name>`
/// (the embedded filename, if any, is never trusted for pathing).
fn extract_gz(archive: &Path, out: &Path) -> Result<(), String> {
    use std::io::Write;
    let file =
        std::fs::File::open(archive).map_err(|e| format!("open {}: {}", archive.display(), e))?;
    let mut decoder = flate2::read::GzDecoder::new(file);
    let mut out_file =
        std::fs::File::create(out).map_err(|e| format!("create {}: {}", out.display(), e))?;
    std::io::copy(&mut decoder, &mut out_file)
        .map_err(|e| format!("gunzip {}: {}", archive.display(), e))?;
    out_file
        .flush()
        .map_err(|e| format!("flush {}: {}", out.display(), e))?;
    Ok(())
}

/// Safe tar.gz extraction: every member is sanitized and written under
/// `root`.
fn extract_targz(archive: &Path, root: &Path) -> Result<(), String> {
    use std::io::Write;
    let file =
        std::fs::File::open(archive).map_err(|e| format!("open {}: {}", archive.display(), e))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut tar = tar::Archive::new(decoder);
    let entries = tar
        .entries()
        .map_err(|e| format!("tar entries {}: {}", archive.display(), e))?;
    for entry in entries {
        let mut entry = entry.map_err(|e| format!("tar entry: {}", e))?;
        let entry_path = entry
            .path()
            .map_err(|e| format!("tar path: {}", e))?
            .to_str()
            .ok_or("non-UTF-8 tar path")?
            .to_string();
        let name = sanitize_entry_name(&entry_path)?;
        let target = root.join(name);
        if !target.starts_with(root) {
            return Err(format!("escape detected: {}", target.display()));
        }
        let entry_type = entry.header().entry_type();
        if entry_type.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|e| format!("mkdir {}: {}", target.display(), e))?;
        } else if entry_type.is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
            }
            let mut out = std::fs::File::create(&target)
                .map_err(|e| format!("create {}: {}", target.display(), e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("extract {}: {}", target.display(), e))?;
            out.flush()
                .map_err(|e| format!("flush {}: {}", target.display(), e))?;
        }
        // Symlinks/hardlinks are not expected in these corpora; skipping
        // them (rather than creating filesystem links) is the safe default.
    }
    Ok(())
}

/// Download one URL to `dest` over HTTPS.  Never executes content.
fn download_https(url: &str, dest: &Path) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!(
            "refusing non-HTTPS download URL '{}' (HTTPS-only policy; the \
             spec must record any secure-endpoint limitation explicitly)",
            url
        ));
    }
    let agent = ureq::Agent::new();
    let resp = agent
        .get(url)
        .call()
        .map_err(|e| format!("GET {}: {}", url, e))?;
    let mut body = Vec::new();
    resp.into_reader()
        .read_to_end(&mut body)
        .map_err(|e| format!("read {}: {}", url, e))?;
    std::fs::write(dest, &body).map_err(|e| format!("write {}: {}", dest.display(), e))?;
    Ok(())
}

fn workload_fetch(spec_dir: &Path, name: &str) -> Result<(), String> {
    let root = cache_root(name)?;
    let archives_dir = root.join("archives");
    let extracted_dir = root.join("extracted");
    std::fs::create_dir_all(&archives_dir).map_err(|e| format!("mkdir archives: {}", e))?;
    std::fs::create_dir_all(&extracted_dir).map_err(|e| format!("mkdir extracted: {}", e))?;

    // ---- Read the spec ----------------------------------------------------
    let sources_toml_path = spec_dir.join("sources.toml");
    let sources_raw = std::fs::read_to_string(&sources_toml_path)
        .map_err(|e| format!("read {}: {}", sources_toml_path.display(), e))?;
    let sources: toml::Value = toml::from_str(&sources_raw)
        .map_err(|e| format!("parse {}: {}", sources_toml_path.display(), e))?;
    let hashes_path = spec_dir.join("expected-source-hashes.json");
    let hashes_raw = std::fs::read_to_string(&hashes_path)
        .map_err(|e| format!("read {}: {}", hashes_path.display(), e))?;
    let hashes: serde_json::Value = serde_json::from_str(&hashes_raw)
        .map_err(|e| format!("parse {}: {}", hashes_path.display(), e))?;

    let mut receipt: serde_json::Value = serde_json::json!({
        "workload": name,
        "fetched_at": chrono_now(),
        "sources": {},
    });

    let source_records = sources
        .get("source")
        .and_then(|v| v.as_array())
        .ok_or("sources.toml has no [[source]] array")?
        .clone();

    for record in &source_records {
        let id = record
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("source record missing id")?;
        let url = record
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or("source record missing url")?;
        let extraction = record
            .get("extraction")
            .and_then(|v| v.as_str())
            .ok_or("source record missing extraction")?;
        let expected = hashes
            .get("sources")
            .and_then(|s| s.get(id))
            .ok_or_else(|| format!("expected-source-hashes.json missing source {}", id))?;
        let exp_sha = expected
            .get("archive_sha256")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let exp_bytes = expected
            .get("archive_bytes")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        // ---- Obtain the archive (offline reuse if already present) --------
        let archive_path = archives_dir.join(id);
        let mut reused = false;
        if archive_path.exists() {
            let cur_sha = sha256_file(&archive_path)?;
            let cur_bytes = std::fs::metadata(&archive_path)
                .map(|m| m.len())
                .unwrap_or(0);
            if cur_sha == exp_sha && cur_bytes == exp_bytes {
                reused = true;
            } else {
                return Err(format!(
                    "cached archive {} does not match the pinned hash \
                     (expected {} / {} bytes, got {} / {} bytes); delete the cache \
                     entry and re-fetch",
                    archive_path.display(),
                    exp_sha,
                    exp_bytes,
                    cur_sha,
                    cur_bytes
                ));
            }
        }
        if !reused {
            println!("  fetching {} <- {}", id, url);
            download_https(url, &archive_path)?;
            let got_sha = sha256_file(&archive_path)?;
            let got_bytes = std::fs::metadata(&archive_path)
                .map(|m| m.len())
                .unwrap_or(0);
            if got_sha != exp_sha || got_bytes != exp_bytes {
                return Err(format!(
                    "hash/size mismatch for {}: expected {} / {} bytes, got {} / {} bytes — \
                     refusing to trust the download",
                    id, exp_sha, exp_bytes, got_sha, got_bytes
                ));
            }
        }
        println!(
            "  verified archive {} ({} bytes, {})",
            id,
            exp_bytes,
            if reused { "cached" } else { "downloaded" }
        );

        // ---- Extract safely ------------------------------------------------
        // Layout: zip/tar.gz sources extract into `extracted/<id>/...`;
        // gz sources are single payloads written flat as `extracted/<id>`
        // (the gzip header's embedded name is never used for pathing).
        match extraction {
            "zip" => {
                let dir = extracted_dir.join(id);
                std::fs::remove_dir_all(&dir).ok();
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
                extract_zip(&archive_path, &dir)?;
            }
            "gz" => {
                let out = extracted_dir.join(id);
                // Either a stale file or (from an earlier buggy layout) a
                // stale directory may exist; remove both forms.
                std::fs::remove_file(&out).ok();
                std::fs::remove_dir_all(&out).ok();
                extract_gz(&archive_path, &out)?;
            }
            "tar.gz" => {
                let dir = extracted_dir.join(id);
                std::fs::remove_dir_all(&dir).ok();
                std::fs::create_dir_all(&dir)
                    .map_err(|e| format!("mkdir {}: {}", dir.display(), e))?;
                extract_targz(&archive_path, &dir)?;
            }
            other => return Err(format!("unsupported extraction type: {}", other)),
        }

        // ---- Verify every extracted file ------------------------------------
        let expected_files = expected
            .get("files")
            .and_then(|f| f.as_object())
            .ok_or("expected hashes missing files")?;
        let verify_root = if extraction == "gz" {
            extracted_dir.clone()
        } else {
            extracted_dir.join(id)
        };
        for (rel, meta) in expected_files {
            let exp_f = meta.get("sha256").and_then(|v| v.as_str()).unwrap_or("");
            let exp_len = meta.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0);
            let target = verify_root.join(rel);
            if !target.starts_with(&verify_root) || !target.exists() {
                return Err(format!(
                    "extracted file {} missing or escaping its root",
                    target.display()
                ));
            }
            let got_sha = sha256_file(&target)?;
            let got_len = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
            if got_sha != exp_f || got_len != exp_len {
                return Err(format!(
                    "extracted file {} hash/size mismatch (expected {} / {} bytes)",
                    rel, exp_f, exp_len
                ));
            }
        }
        println!("  verified {} extracted file(s)", expected_files.len());

        receipt["sources"][id] = serde_json::json!({
            "archive_sha256": exp_sha,
            "archive_bytes": exp_bytes,
            "reused_from_cache": reused,
            "verified_files": expected_files.len() as u64,
        });
    }

    let receipt_path = root.join("fetch-receipt.json");
    std::fs::write(
        &receipt_path,
        serde_json::to_string_pretty(&receipt).map_err(|e| format!("serialize receipt: {}", e))?,
    )
    .map_err(|e| format!("write {}: {}", receipt_path.display(), e))?;
    println!(
        "workload fetch complete: {} sources verified; receipt {}",
        source_records.len(),
        receipt_path.display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// derive
// ---------------------------------------------------------------------------

/// One extracted file reference: relative path within the source extraction
/// root, its pinned hash, and its length.
#[derive(Clone)]
struct SourceFile {
    rel: String,
    sha256: String,
    len: u64,
}

/// Per-source extracted file table (from expected-source-hashes.json).
fn source_files(hashes: &serde_json::Value, id: &str) -> Result<Vec<SourceFile>, String> {
    let files = hashes
        .get("sources")
        .and_then(|s| s.get(id))
        .and_then(|s| s.get("files"))
        .and_then(|f| f.as_object())
        .ok_or_else(|| format!("no files for source {}", id))?;
    let mut out = Vec::new();
    for (rel, meta) in files {
        out.push(SourceFile {
            rel: rel.clone(),
            sha256: meta
                .get("sha256")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            len: meta.get("bytes").and_then(|v| v.as_u64()).unwrap_or(0),
        });
    }
    // Deterministic order.
    out.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(out)
}

/// A deterministic slice cursor: cycles a source's file list and offsets.
struct SliceCursor {
    files: Vec<SourceFile>,
    file_idx: usize,
    offset: u64,
}

impl SliceCursor {
    /// Take the next `len`-byte slice.  `file_idx` advances only when the
    /// current file is exhausted; offsets wrap within a file (documented
    /// source-region reuse — schedules are logical, never materialized).
    ///
    /// # Oversized-block clamp
    ///
    /// When no file of the source is large enough for `len` (e.g. a 4 MiB
    /// block against the 1 MB largest Canterbury file), the slice is
    /// clamped to the largest file and that file is consumed whole.  This
    /// keeps the generator total and deterministic; the manifest records the
    /// true clamped length, and boundary-size coverage is preserved on the
    /// large sustained sources (enwik8, Pizza & Chili ≥ 50 MB).
    fn take(&mut self, len: u64) -> (String, String, u64, u64) {
        // Does ANY file of this source fit `len`?  If not, every slice of
        // this size must be clamped (checked once, before the loop).
        let fits_somewhere = self.files.iter().any(|f| f.len >= len);
        loop {
            let f = &self.files[self.file_idx];
            if self.offset + len <= f.len {
                let (sid, sha, off, l) = (f.rel.clone(), f.sha256.clone(), self.offset, len);
                self.offset += len;
                return (sid, sha, off, l);
            }
            // Advance to the next file (wrap-around restarts at offset 0 of
            // the first file — the deterministic region-reuse cycle).
            self.file_idx = (self.file_idx + 1) % self.files.len();
            self.offset = 0;
            if !fits_somewhere {
                // Clamp to the largest file and consume it whole.  This
                // branch is reached exactly once per call and terminates:
                // `fits_somewhere == false` implies no file can take a
                // `len`-sized slice.
                let largest = self
                    .files
                    .iter()
                    .enumerate()
                    .max_by_key(|(_, f)| f.len)
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                let f = &self.files[largest];
                let (sid, sha, off, l) = (f.rel.clone(), f.sha256.clone(), 0, f.len);
                self.file_idx = (largest + 1) % self.files.len();
                return (sid, sha, off, l);
            }
        }
    }

    /// Advance the offset base without consuming a slice (used by the
    /// pass/rotation schedules for deterministic phase shifts).
    fn advance(&mut self, by: u64) {
        let f = &self.files[self.file_idx];
        self.offset = (self.offset + by) % f.len;
    }
}

fn chrono_now() -> String {
    use std::process::Command;
    if let Ok(out) = Command::new("date")
        .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
        .output()
    {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout).trim().to_string();
        }
    }
    "unknown".to_string()
}

fn workload_derive(spec_dir: &Path, name: &str) -> Result<(), String> {
    // ---- Read the three pinned inputs --------------------------------------
    let sources_raw = std::fs::read_to_string(spec_dir.join("sources.toml"))
        .map_err(|e| format!("read sources.toml: {}", e))?;
    let sources: toml::Value =
        toml::from_str(&sources_raw).map_err(|e| format!("parse sources.toml: {}", e))?;
    let hashes_path = spec_dir.join("expected-source-hashes.json");
    let hashes_raw = std::fs::read_to_string(&hashes_path)
        .map_err(|e| format!("read expected-source-hashes.json: {}", e))?;
    let hashes: serde_json::Value = serde_json::from_str(&hashes_raw)
        .map_err(|e| format!("parse expected-source-hashes.json: {}", e))?;
    let deriv_raw = std::fs::read_to_string(spec_dir.join("derivation.toml"))
        .map_err(|e| format!("read derivation.toml: {}", e))?;
    let deriv: toml::Value =
        toml::from_str(&deriv_raw).map_err(|e| format!("parse derivation.toml: {}", e))?;

    let derivation_policy_sha256 = sha256_hex(deriv_raw.as_bytes());
    let source_hashes_sha256 = sha256_hex(hashes_raw.as_bytes());

    // Pre-build cursors for every source referenced by any schedule.
    let source_records = sources
        .get("source")
        .and_then(|v| v.as_array())
        .ok_or("sources.toml has no [[source]] array")?
        .clone();
    let mut cursors: BTreeMap<String, SliceCursor> = BTreeMap::new();
    for record in &source_records {
        let id = record
            .get("id")
            .and_then(|v| v.as_str())
            .ok_or("source id missing")?;
        cursors.insert(
            id.to_string(),
            SliceCursor {
                files: source_files(&hashes, id)?,
                file_idx: 0,
                offset: 0,
            },
        );
    }
    // A flat name lookup for the gz sources' single-file records.
    let file_by_rel: BTreeMap<String, String> = cursors
        .values()
        .flat_map(|c| c.files.iter().map(|f| (f.rel.clone(), f.sha256.clone())))
        .collect();

    let codec_8way = deriv
        .get("codec")
        .and_then(|c| c.get("codec_8way"))
        .and_then(|v| v.as_integer())
        .unwrap_or(7) as u16;
    let codec_16way = deriv
        .get("codec")
        .and_then(|c| c.get("codec_16way"))
        .and_then(|v| v.as_integer())
        .unwrap_or(8) as u16;
    let scale = deriv
        .get("codec")
        .and_then(|c| c.get("scale_bits"))
        .and_then(|v| v.as_integer())
        .unwrap_or(12) as u8;

    let mut schedules: Vec<ryg_rans_rs_casefile::WorkloadSchedule> = Vec::new();

    // ---- smoke -------------------------------------------------------------
    {
        let cfg = &deriv["schedules"]["smoke"];
        let target = cfg
            .get("logical_bytes_target")
            .and_then(|v| v.as_integer())
            .unwrap_or(4 * 1024 * 1024) as u64;
        let sources_list: Vec<String> = cfg
            .get("sources")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let n_groups = cfg
            .get("model_groups")
            .and_then(|v| v.as_integer())
            .unwrap_or(8) as u64;
        let natural_every = cfg
            .get("natural_mode_blocks")
            .and_then(|v| v.as_integer())
            .unwrap_or(16) as u64;
        let use_boundaries = cfg
            .get("use_boundaries")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // The block-size sequence: matrix + boundary triples (deterministic).
        let matrix: Vec<u64> = deriv["block_sizes"]["matrix"]
            .as_array()
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| v.as_integer().map(|i| i as u64))
            .collect();
        let mut sizes: Vec<u64> = matrix.clone();
        if use_boundaries {
            let boundaries: Vec<Vec<u64>> = deriv["block_sizes"]["boundaries"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|b| {
                    b.as_array()
                        .unwrap_or(&Vec::new())
                        .iter()
                        .filter_map(|v| v.as_integer().map(|i| i as u64))
                        .collect()
                })
                .collect();
            sizes.extend(boundaries.into_iter().flatten());
        }

        let mut blocks: Vec<ryg_rans_rs_casefile::WorkloadBlock> = Vec::new();
        let mut logical = 0u64;
        let mut idx = 0u64;
        // Coverage condition: every size in the matrix AND every boundary
        // triple must be emitted at least once (Phase O.11), even when the
        // logical target is reached early; then keep cycling until the
        // target is met.
        while logical < target || idx < sizes.len() as u64 {
            let size = sizes[(idx as usize) % sizes.len()];
            let src = &sources_list[(idx as usize) % sources_list.len()];
            let cursor = cursors.get_mut(src).ok_or("smoke source missing")?;
            let (rel, sha, off, len) = cursor.take(size);
            let group = if idx % natural_every == natural_every - 1 {
                u64::MAX // natural per-block model
            } else {
                idx % n_groups
            };
            let codec = if idx % 2 == 0 {
                codec_8way
            } else {
                codec_16way
            };
            let _ = &file_by_rel; // (hash already bound via the cursor)
            blocks.push(ryg_rans_rs_casefile::WorkloadBlock {
                block_index: idx,
                source_id: src.clone(),
                source_sha256: sha,
                offset: off,
                length: len,
                model_group: group,
                codec_id: codec,
                scale_bits: scale,
            });
            logical += len;
            idx += 1;
            let _ = rel;
        }
        let schedule_sha256 = schedule_hash(&blocks);
        schedules.push(ryg_rans_rs_casefile::WorkloadSchedule {
            name: "public-rans-smoke".to_string(),
            logical_bytes: logical,
            block_count: blocks.len() as u64,
            blocks,
            schedule_sha256,
        });
    }

    // ---- 1g -----------------------------------------------------------------
    {
        let cfg = &deriv["schedules"]["one_gib"];
        let target = cfg
            .get("logical_bytes_target")
            .and_then(|v| v.as_integer())
            .unwrap_or(1 << 30) as u64;
        let sources_list: Vec<String> = cfg
            .get("sources")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let sizes: Vec<u64> = cfg
            .get("block_sizes")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| v.as_integer().map(|i| i as u64))
            .collect();
        let n_groups = cfg
            .get("model_groups")
            .and_then(|v| v.as_integer())
            .unwrap_or(16) as u64;
        let natural_every = cfg
            .get("natural_mode_blocks")
            .and_then(|v| v.as_integer())
            .unwrap_or(64) as u64;
        let mut blocks: Vec<ryg_rans_rs_casefile::WorkloadBlock> = Vec::new();
        let mut logical = 0u64;
        let mut idx = 0u64;
        // Coverage condition: every size in the matrix AND every boundary
        // triple must be emitted at least once (Phase O.11), even when the
        // logical target is reached early; then keep cycling until the
        // target is met.
        while logical < target || idx < sizes.len() as u64 {
            let size = sizes[(idx as usize) % sizes.len()];
            let src = &sources_list[(idx as usize) % sources_list.len()];
            let cursor = cursors.get_mut(src).ok_or("1g source missing")?;
            let (_, sha, off, len) = cursor.take(size);
            let group = if idx % natural_every == natural_every - 1 {
                u64::MAX
            } else {
                idx % n_groups
            };
            blocks.push(ryg_rans_rs_casefile::WorkloadBlock {
                block_index: idx,
                source_id: src.clone(),
                source_sha256: sha,
                offset: off,
                length: len,
                model_group: group,
                codec_id: codec_16way,
                scale_bits: scale,
            });
            logical += len;
            idx += 1;
        }
        let schedule_sha256 = schedule_hash(&blocks);
        schedules.push(ryg_rans_rs_casefile::WorkloadSchedule {
            name: "public-rans-1g".to_string(),
            logical_bytes: logical,
            block_count: blocks.len() as u64,
            blocks,
            schedule_sha256,
        });
    }

    // ---- mixed-16g ----------------------------------------------------------
    {
        let cfg = &deriv["schedules"]["mixed_16g"];
        let target = cfg
            .get("logical_bytes_target")
            .and_then(|v| v.as_integer())
            .unwrap_or(16 << 30) as u64;
        let pass_sources: Vec<String> = cfg
            .get("pass_sources")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let block_size = cfg
            .get("pass_block_size")
            .and_then(|v| v.as_integer())
            .unwrap_or(262144) as u64;
        let n_groups = cfg
            .get("pass_model_groups")
            .and_then(|v| v.as_integer())
            .unwrap_or(32) as u64;
        let mut blocks: Vec<ryg_rans_rs_casefile::WorkloadBlock> = Vec::new();
        let mut logical = 0u64;
        let mut idx = 0u64;
        let mut pass = 0u64;
        while logical < target {
            let src = &pass_sources[(idx as usize) % pass_sources.len()];
            let cursor = cursors.get_mut(src).ok_or("mixed source missing")?;
            // Each pass starts at a deterministic shifted offset.
            if idx % pass_sources.len() as u64 == 0 {
                cursor.advance(pass * 65537 % (1 << 20));
            }
            let (_, sha, off, len) = cursor.take(block_size);
            let group = idx % n_groups;
            blocks.push(ryg_rans_rs_casefile::WorkloadBlock {
                block_index: idx,
                source_id: src.clone(),
                source_sha256: sha,
                offset: off,
                length: len,
                model_group: group,
                codec_id: codec_16way,
                scale_bits: scale,
            });
            logical += len;
            idx += 1;
            if idx % pass_sources.len() as u64 == 0 {
                pass += 1;
            }
        }
        let schedule_sha256 = schedule_hash(&blocks);
        schedules.push(ryg_rans_rs_casefile::WorkloadSchedule {
            name: "public-rans-mixed-16g".to_string(),
            logical_bytes: logical,
            block_count: blocks.len() as u64,
            blocks,
            schedule_sha256,
        });
    }

    // ---- stress-64g ----------------------------------------------------------
    {
        let cfg = &deriv["schedules"]["stress_64g"];
        let target = cfg
            .get("logical_bytes_target")
            .and_then(|v| v.as_integer())
            .unwrap_or(64 << 30) as u64;
        let rotations = cfg
            .get("rotations")
            .and_then(|v| v.as_integer())
            .unwrap_or(4) as u64;
        let rot_sources: Vec<String> = cfg
            .get("rotation_sources")
            .and_then(|v| v.as_array())
            .unwrap_or(&Vec::new())
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        let block_size = cfg
            .get("rotation_block_size")
            .and_then(|v| v.as_integer())
            .unwrap_or(1048576) as u64;
        let n_groups = cfg
            .get("rotation_model_groups")
            .and_then(|v| v.as_integer())
            .unwrap_or(64) as u64;
        let mut blocks: Vec<ryg_rans_rs_casefile::WorkloadBlock> = Vec::new();
        let mut logical = 0u64;
        let mut idx = 0u64;
        while logical < target {
            let rot = (idx as u64 / rot_sources.len() as u64) % rotations;
            let src = &rot_sources[(idx as usize) % rot_sources.len()];
            let cursor = cursors.get_mut(src).ok_or("rotation source missing")?;
            if idx % rot_sources.len() as u64 == 0 {
                // Deterministic per-rotation offset base (prime strides).
                cursor.advance(rot * 104729 % (4 << 20));
            }
            let (_, sha, off, len) = cursor.take(block_size);
            let group = (idx + rot * 31) % n_groups;
            blocks.push(ryg_rans_rs_casefile::WorkloadBlock {
                block_index: idx,
                source_id: src.clone(),
                source_sha256: sha,
                offset: off,
                length: len,
                model_group: group,
                codec_id: codec_16way,
                scale_bits: scale,
            });
            logical += len;
            idx += 1;
        }
        let schedule_sha256 = schedule_hash(&blocks);
        schedules.push(ryg_rans_rs_casefile::WorkloadSchedule {
            name: "public-rans-stress-64g".to_string(),
            logical_bytes: logical,
            block_count: blocks.len() as u64,
            blocks,
            schedule_sha256,
        });
    }

    let manifest = ryg_rans_rs_casefile::WorkloadManifest {
        workload: name.to_string(),
        version: "1".to_string(),
        derivation_policy_sha256: derivation_policy_sha256.clone(),
        source_hashes_sha256: source_hashes_sha256.clone(),
        schedules,
        created_at: chrono_now(),
    };

    let out_dir = cache_root(name)?.join("derived");
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir derived: {}", e))?;
    let out_path = out_dir.join(format!("{}.manifest.json", name));
    let json = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("serialize manifest: {}", e))?;
    std::fs::write(&out_path, &json).map_err(|e| format!("write {}: {}", out_path.display(), e))?;

    for s in &manifest.schedules {
        println!(
            "  schedule {}: {} blocks, {} logical bytes, sha256 {}",
            s.name, s.block_count, s.logical_bytes, s.schedule_sha256
        );
    }
    println!(
        "workload derive complete: {} schedules; manifest {} (derivation_policy_sha256 {}, source_hashes_sha256 {})",
        manifest.schedules.len(),
        out_path.display(),
        derivation_policy_sha256,
        source_hashes_sha256
    );
    Ok(())
}

/// Canonical ordered-schedule hash: SHA-256 of the canonical JSON of the
/// block records in order (Phase O.11 "hash the complete ordered schedule").
fn schedule_hash(blocks: &[ryg_rans_rs_casefile::WorkloadBlock]) -> String {
    let json = serde_json::to_string(blocks).unwrap_or_default();
    sha256_hex(json.as_bytes())
}
