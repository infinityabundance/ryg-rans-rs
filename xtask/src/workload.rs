//! # Public-corpus workload tooling (Phase O.10/O.11)
//!
//! `cargo xtask workload fetch public-rans-v1` downloads, hash-verifies, and
//! safely extracts the pinned public corpora into a cache directory outside
//! the Git repository.  `cargo xtask workload derive public-rans-v1` expands
//! the deterministic derivation policy into ordered `WorkloadBlock`
//! schedules — a small slice manifest that never materializes the tens of
//! gigabytes it describes.
//!
//! ## Two distinct execution families (post-v0.5.0 audit, MODEL_CACHE.WORKLOAD.2)
//!
//! * **`synthetic-cache-stress` / `synthetic-cache-soak`** (aliases `stress` /
//!   `soak`) run the Phase O.12 cache-behaviour classes — cold/warm single
//!   model, hot set, capacity+1 thrash, unique models, mixed — on
//!   deterministic **xorshift pattern payloads with constant seeds**.  These
//!   payloads are *not* derived from the public corpus; the commands are
//!   honest about that (`synthetic-cache-stress-v1` labels in the output).
//!   Their purpose is to force exact cache access patterns that public data
//!   does not naturally produce (one model, 65 models against 64 slots, ...).
//! * **`stress-public` / `soak-public`** execute the **derived public
//!   schedule itself**: every block is `source_id` + `source_sha256` +
//!   `offset` + `length` resolved to the hash-verified extracted source
//!   bytes, sliced, encoded with the block's declared codec/scale, and
//!   decoded through the parallel engine.  `--schedule` selects which
//!   derived schedule actually runs (`public-rans-smoke`, `public-rans-1g`,
//!   `public-rans-mixed-16g`, `public-rans-stress-64g`).  Natural mode
//!   derives each model from the block's own bytes; grouped mode trains one
//!   model per group from the declared public training region
//!   (derivation.toml `[models]`) and reuses it for the group.
//!
//! The two families are deliberately separate: cache-behaviour classes are
//! inherently synthetic, and public-corpus identity belongs only to runs
//! that derive their bytes from the pinned sources.
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

// The public stress/soak runners (MODEL_CACHE.WORKLOAD.2) operate at module
// level and use the parallel engine types directly.
use ryg_rans_rs_parallel::{
    CancellationToken, CodecPolicy, DecodeBlockJob, EncodeBlockJob, ModelArtifactCache,
    ModelPolicy, ParallelConfig, ParallelDecoder, ThreadCount,
};

/// Entry point: `cargo xtask workload <subcommand> [public-rans-v1] [args]`.
///
/// Subcommands:
///
/// * `fetch` — download, hash-verify, and safely extract the pinned sources.
/// * `derive` — expand the derivation policy into the ordered slice manifest.
/// * `synthetic-cache-stress` / `synthetic-cache-soak` (aliases `stress` /
///   `soak`) — the Phase O.12 cache-behaviour matrix on deterministic
///   synthetic payloads (honestly labeled `synthetic-cache-stress-v1`).
/// * `stress-public` / `soak-public` — execute a derived public schedule on
///   the actual verified corpus bytes (MODEL_CACHE.WORKLOAD.2 fix).
/// * `policy-sim` — offline FIFO/LRU shadow simulation.
pub fn cmd_workload(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err(
            "usage: cargo xtask workload <fetch|derive|stress|soak|synthetic-cache-stress|\
             synthetic-cache-soak|stress-public|soak-public|policy-sim> [public-rans-v1]"
                .into(),
        );
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
        // Synthetic cache-behaviour runners (honestly labeled; the payloads
        // are deterministic xorshift patterns with constant seeds, NOT
        // derived from the public corpus — MODEL_CACHE.WORKLOAD.2).
        "stress" | "synthetic-cache-stress" => {
            cmd_workload_synthetic_stress(&spec_dir, &name, &args[2..])
        }
        "soak" | "synthetic-cache-soak" => cmd_workload_synthetic_soak(&spec_dir, &name),
        // Genuine public-corpus runners (consume the derived manifest + the
        // hash-verified extracted bytes — MODEL_CACHE.WORKLOAD.2 fix).
        "stress-public" => cmd_workload_stress_public(&spec_dir, &name, &args[2..]),
        "soak-public" => cmd_workload_soak_public(&spec_dir, &name, &args[2..]),
        "policy-sim" => cmd_workload_policy_sim(&name),
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

// ---------------------------------------------------------------------------
// Synthetic cache-behaviour stress / soak (Phase O.12/O.18) — honestly
// labeled `synthetic-cache-stress-v1` / `synthetic-cache-soak-v1`
// (post-v0.5.0 audit, MODEL_CACHE.WORKLOAD.2)
// ---------------------------------------------------------------------------

/// Synthetic cache-behaviour stress matrix: `cargo xtask workload
/// synthetic-cache-stress public-rans-v1` (alias `stress`).
///
/// Runs the Phase O.12 cache-behaviour classes on **deterministic synthetic
/// payloads** (xorshift patterns with constant seeds):
///
/// * workers: 1, 2, 4, 8, 16, 32 (`--workers N` overrides)
/// * block sizes: tiny (4 KiB) and large (1 MiB)
/// * cache modes: cold single model, warm single model, hot set (16),
///   thrash (65 patterns against a 64-slot cache), unique models, mixed
///   reuse (4 hot + 12 cold patterns)
/// * constrained cache budgets (`--cache-entries N --cache-bytes N`)
///   and (via `--simd-off`) a scalar-only build
///
/// **Honesty contract (MODEL_CACHE.WORKLOAD.2):** the payloads are the
/// xorshift expansion of CONSTANT seeds — they are not derived from the
/// public corpus, the derived manifest is not consulted, and the extracted
/// source tree is not required.  The command is labeled
/// `synthetic-cache-stress-v1`; genuine public-corpus execution is
/// `cargo xtask workload stress-public public-rans-v1`.  (The pre-0.5.1
/// `--schedule` flag was removed as inert: schedule selection belongs to
/// the public runner.)
///
/// Every decode asserts: decoded bytes == expected, no panics, and the
/// cache's exact invariants (build accounting, hit/miss sum, byte/count
/// bounds).  The grouped-model blocks reuse the pattern's own histogram, so
/// the decode-side model reuse is real and labeled (Phase O.13).
pub fn cmd_workload_synthetic_stress(
    _spec_dir: &Path,
    _name: &str,
    args: &[String],
) -> Result<(), String> {
    let mut workers_override: Option<usize> = None;
    let mut simd_off = false;
    let mut cache_entries: usize = 64;
    let mut cache_bytes: u64 = 16 * 1024 * 1024;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--workers" => {
                i += 1;
                workers_override = args.get(i).and_then(|s| s.parse::<usize>().ok());
            }
            "--simd-off" => simd_off = true,
            "--cache-entries" => {
                i += 1;
                cache_entries = args
                    .get(i)
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or("--cache-entries requires a usize")?;
            }
            "--cache-bytes" => {
                i += 1;
                cache_bytes = args
                    .get(i)
                    .and_then(|s| s.parse::<u64>().ok())
                    .ok_or("--cache-bytes requires a u64")?;
            }
            other => return Err(format!("unknown synthetic stress option: {}", other)),
        }
        i += 1;
    }

    println!("synthetic-cache-stress-v1: deterministic xorshift payloads (constant seeds)");
    println!("  payloads are NOT derived from the public corpus; use `stress-public` for that");

    // Worker matrix.
    let workers: Vec<usize> = match workers_override {
        Some(w) => vec![w],
        None => vec![1, 2, 4, 8, 16, 32],
    };
    let block_sizes: [usize; 2] = [4096, 1048576];
    let modes: [&str; 6] = [
        "cold-single",
        "warm-single",
        "hot-set",
        "thrash",
        "unique",
        "mixed",
    ];

    let mut total_cases = 0usize;
    let mut total_blocks = 0u64;
    let mut total_bytes = 0u64;
    let mut failures: Vec<String> = Vec::new();

    for &w in &workers {
        for &size in &block_sizes {
            for mode in modes {
                let result = run_stress_case(mode, w, size, simd_off, cache_entries, cache_bytes);
                match result {
                    Ok(stats) => {
                        total_cases += 1;
                        total_blocks += stats.blocks;
                        total_bytes += stats.bytes;
                        println!(
                            "  stress {:>10} w={:>2} size={:>7} : {} blocks, {} MiB, hits={} builds={} evictions={} — OK",
                            mode,
                            w,
                            size,
                            stats.blocks,
                            stats.bytes / (1024 * 1024),
                            stats.hits,
                            stats.builds,
                            stats.evictions,
                        );
                    }
                    Err(e) => {
                        failures.push(format!("{} w={} size={}: {}", mode, w, size, e));
                        println!(
                            "  stress {:>10} w={:>2} size={:>7} : FAILED — {}",
                            mode, w, size, e
                        );
                    }
                }
            }
        }
    }

    println!(
        "synthetic-cache-stress-v1 complete: {} cases, {} blocks, {} MiB decoded; {} failure(s)",
        total_cases,
        total_blocks,
        total_bytes / (1024 * 1024),
        failures.len()
    );
    if !failures.is_empty() {
        return Err(format!("stress failures:\n{}", failures.join("\n")));
    }
    Ok(())
}

/// Statistics of one stress case.
struct StressStats {
    blocks: u64,
    bytes: u64,
    hits: u64,
    builds: u64,
    evictions: u64,
}

/// Run one synthetic cache-behaviour case: build encoded blocks from the
/// mode's deterministic xorshift patterns (constant seeds — NOT public
/// corpus bytes), decode with the configured cache mode, and assert every
/// invariant.  `cache_entries`/`cache_bytes` constrain the budget.
fn run_stress_case(
    mode: &str,
    workers: usize,
    block_size: usize,
    simd_off: bool,
    cache_entries: usize,
    cache_bytes: u64,
) -> Result<StressStats, String> {
    use ryg_rans_rs_parallel::{
        CodecPolicy, DecodeBlockJob, EncodeBlockJob, ModelArtifactCache, ModelPolicy,
        ParallelConfig, ParallelDecoder, ThreadCount,
    };

    // ---- Model selection per mode -------------------------------------------
    // `pattern_for(i)` returns the data pattern for block i:
    // - cold/warm single: one pattern (identical blocks → identical models)
    // - hot-set: 16 patterns, cyclic
    // - thrash: 65 patterns (one more than the cache capacity), cyclic
    // - unique: a distinct pattern per block
    // - mixed: cycle 4 hot patterns then 12 unique (mixed reuse)
    // The block PAYLOAD is the pattern itself (repeated to block_size), so
    // per-block histogram models are identical within a pattern — the
    // grouped-model reuse is real and labeled (Phase O.13).  The seeds are
    // constants: synthetic by design, never presented as corpus-derived.
    let patterns: Vec<Vec<u8>> = match mode {
        "cold-single" | "warm-single" => (0..1)
            .map(|p| pattern_bytes(p as u64, block_size))
            .collect(),
        "hot-set" => (0..16)
            .map(|p| pattern_bytes(p as u64, block_size))
            .collect(),
        "thrash" => (0..65)
            .map(|p| pattern_bytes(p as u64, block_size))
            .collect(),
        "unique" => (0..256)
            .map(|p| pattern_bytes(p as u64 + 1, block_size))
            .collect(),
        "mixed" => {
            let mut v = (0..4)
                .map(|p| pattern_bytes(p as u64, block_size))
                .collect::<Vec<_>>();
            v.extend((0..12).map(|p| pattern_bytes(p as u64 + 1000, block_size)));
            v
        }
        other => return Err(format!("unknown stress mode: {}", other)),
    };
    // Thrash must exceed the cache capacity so the cyclic model set forces
    // evictions; other modes use a full queue of blocks.
    let n_blocks = if mode == "thrash" {
        2 * cache_entries + 2
    } else {
        64usize
    };
    let pattern_for = |i: usize| -> &Vec<u8> {
        match mode {
            "cold-single" | "warm-single" => &patterns[0],
            "hot-set" => &patterns[i % 16],
            "thrash" => &patterns[i % 65],
            "unique" => &patterns[i % 256],
            "mixed" => {
                if i % 8 < 4 {
                    &patterns[i % 4]
                } else {
                    &patterns[4 + (i % 12)]
                }
            }
            _ => &patterns[0],
        }
    };

    // ---- Encode ---------------------------------------------------------------
    let mut encode_jobs = Vec::with_capacity(n_blocks);
    let mut expected = Vec::with_capacity(block_size * n_blocks);
    for i in 0..n_blocks {
        let pat = pattern_for(i);
        let data = pat.clone();
        expected.extend_from_slice(&data);
        encode_jobs.push(EncodeBlockJob::new(
            i as u64,
            data,
            CodecPolicy::Auto,
            ModelPolicy::PerBlock,
            12,
        ));
    }
    let enc_cfg = ParallelConfig {
        threads: ThreadCount::Exact(std::num::NonZeroUsize::new(4).unwrap()),
        parallel_threshold_bytes: 0,
        max_buffered_output_bytes: 1 << 30,
        max_buffered_input_bytes: 1 << 30,
        ..Default::default()
    };
    let enc = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(encode_jobs, &enc_cfg)
        .map_err(|e| format!("encode: {:?}", e))?;
    let decode_jobs: Vec<DecodeBlockJob> = enc
        .blocks
        .into_iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block,
        })
        .collect();

    // ---- Cache configuration per mode ----------------------------------------
    let cache = ModelArtifactCache::bounded(cache_entries.max(1), cache_bytes);
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(std::num::NonZeroUsize::new(workers.max(1)).unwrap()),
        parallel_threshold_bytes: 0,
        disable_simd: simd_off,
        max_buffered_output_bytes: 4 << 30,
        max_buffered_input_bytes: 4 << 30,
        // Reorder window = max_in_flight.max(workers) + workers must exceed
        // the largest stress case (2*cache_entries+2 thrash blocks at 32
        // workers).
        max_in_flight_blocks: std::num::NonZeroUsize::new(2 * cache_entries + 2 + 64).unwrap(),
        ..Default::default()
    };

    // ---- Warm pre-population ---------------------------------------------------
    if mode == "warm-single" {
        // The shared model is the histogram of the identical payload; extract
        // it from the first encoded block (header 104 + 1024 model bytes)
        // together with the block's actual codec ID (bytes 16..18, u16 LE),
        // so the prewarmed key exactly matches the decode key.
        let first = &decode_jobs[0].block_data;
        let model = first
            .get(104..104 + 1024)
            .ok_or("warm: block carries no 1024-byte model")?
            .to_vec();
        let codec = u16::from_le_bytes([first[16], first[17]]);
        cache
            .get_or_build(codec, 12, &model, None, || {
                ryg_rans_rs_parallel::build_validated_model_artifacts(codec, 12, &model)
            })
            .map_err(|e| format!("warm prewarm: {:?}", e))?;
    }

    // ---- Decode + assert ---------------------------------------------------------
    let decoder = ParallelDecoder::with_model_cache(cfg, cache.clone());
    let pre = cache.metrics();
    let decoded = decoder
        .decode_blocks(decode_jobs)
        .map_err(|e| format!("decode: {:?}", e))?;
    let post = cache.metrics();

    // Byte-exact output.
    let mut out = Vec::with_capacity(expected.len());
    for b in &decoded.blocks {
        out.extend_from_slice(&b.output);
    }
    if out != expected {
        return Err(format!(
            "decoded output mismatch: {} bytes vs {} expected",
            out.len(),
            expected.len()
        ));
    }

    // Cache invariants (Phase O.18 completion assertions).
    let builds = post.builds_started - pre.builds_started;
    let completed = post.builds_completed - pre.builds_completed;
    let failed = post.build_failures - pre.build_failures;
    if completed + failed > builds {
        return Err("invariant: builds_completed + build_failures > builds_started".into());
    }
    if post.current_entries > cache_entries {
        return Err(format!(
            "invariant: current_entries {} > capacity {}",
            post.current_entries, cache_entries
        ));
    }
    if post.current_bytes > cache_bytes {
        return Err(format!(
            "invariant: current_bytes {} > budget {}",
            post.current_bytes, cache_bytes
        ));
    }
    if !post.invariant_hit_miss_sum() {
        return Err(format!(
            "invariant: hits + misses != lookups ({} + {} != {})",
            post.hits, post.misses, post.lookups
        ));
    }
    // Mode-specific expectations.
    match mode {
        "cold-single" => {
            if builds != 1 {
                return Err(format!(
                    "cold-single: expected 1 build, got {} (hits={})",
                    builds,
                    post.hits - pre.hits
                ));
            }
        }
        "warm-single" => {
            if builds != 0 {
                return Err(format!("warm-single: expected 0 builds, got {}", builds));
            }
        }
        "hot-set" => {
            if builds > 16 {
                return Err(format!("hot-set: expected <= 16 builds, got {}", builds));
            }
        }
        "thrash" => {
            let evictions = post.entry_evictions - pre.entry_evictions;
            if builds == 0 || evictions == 0 {
                return Err(format!(
                    "thrash: expected rebuilds and evictions, got builds={} evictions={}",
                    builds, evictions
                ));
            }
        }
        _ => {}
    }

    Ok(StressStats {
        blocks: n_blocks as u64,
        bytes: expected.len() as u64,
        hits: post.hits - pre.hits,
        builds,
        evictions: post.entry_evictions - pre.entry_evictions,
    })
}

/// A deterministic byte pattern (xorshift64) used as the block payload and
/// the shared-model source.  `seed == 0` yields a uniform distribution.
/// The seed is a CONSTANT chosen by the mode schedule: synthetic payloads
/// by design (MODEL_CACHE.WORKLOAD.2 — never presented as corpus-derived).
fn pattern_bytes(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1) | 1;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        out.push((s & 0xff) as u8);
    }
    out
}

/// Synthetic soak runner: `cargo xtask workload synthetic-cache-soak
/// public-rans-v1` (alias `soak`).
///
/// Runs a sustained decode workload on deterministic synthetic xorshift
/// patterns (constant seeds) while checking the cache invariants
/// periodically.  The payloads are NOT derived from the public corpus — the
/// command is labeled `synthetic-cache-soak-v1` (MODEL_CACHE.WORKLOAD.2);
/// use `soak-public` for genuine corpus execution.  At completion it asserts
/// the O.18 soak contract:
///
/// ```text
/// current_bytes <= budget, current_entries <= capacity
/// builds_completed + build_failures <= builds_started
/// hits + misses == lookups
/// duplicate successful builds == 0
/// decoded output == source input
/// ```
///
/// The soak duration is bounded by `RYG_RANS_SOAK_ROUNDS` (default 64
/// rounds of 32 blocks each) so it terminates in a practical time while
/// still exercising phase shifts and cache churn.
pub fn cmd_workload_synthetic_soak(_spec_dir: &Path, _name: &str) -> Result<(), String> {
    use ryg_rans_rs_parallel::{
        CodecPolicy, DecodeBlockJob, EncodeBlockJob, ModelArtifactCache, ModelPolicy,
        ParallelConfig, ParallelDecoder, ThreadCount,
    };
    let rounds: u64 = std::env::var("RYG_RANS_SOAK_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(64);

    println!("synthetic-cache-soak-v1: deterministic xorshift payloads (constant seeds)");
    println!("  payloads are NOT derived from the public corpus; use `soak-public` for that");

    let cache = ModelArtifactCache::bounded(64, 16 * 1024 * 1024);
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(std::num::NonZeroUsize::new(8).unwrap()),
        parallel_threshold_bytes: 0,
        max_buffered_output_bytes: 1 << 30,
        max_buffered_input_bytes: 1 << 30,
        max_in_flight_blocks: std::num::NonZeroUsize::new(128).unwrap(),
        ..Default::default()
    };
    let decoder = ParallelDecoder::with_model_cache(cfg, cache.clone());

    // The rounds rotate through pattern groups to phase-shift the cache
    // residency.  Round r uses pattern group (r % 3): groups 0..5 (hot),
    // then 0..65 (thrash), then 1000..1016 (new set → reacquisition).
    let mut blocks_decoded = 0u64;
    let mut bytes_decoded = 0u64;
    let mut duplicate_builds = 0u64;

    for round in 0..rounds {
        let pattern_base: u64 = match round % 3 {
            0 => 0,                      // hot set A (patterns 0..5)
            1 => 100,                    // hot set B (patterns 100..105)
            _ => 1000 + round * 7 % 256, // shifting set
        };
        let n = 32usize;
        let mut jobs = Vec::with_capacity(n);
        for i in 0..n {
            let seed = pattern_base + (i % 6) as u64;
            let data = pattern_bytes(seed + 1, 8192);
            bytes_decoded += data.len() as u64;
            jobs.push(EncodeBlockJob::new(
                i as u64,
                data,
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            ));
        }
        let enc = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(
            jobs,
            &ParallelConfig {
                threads: ThreadCount::Exact(std::num::NonZeroUsize::new(4).unwrap()),
                parallel_threshold_bytes: 0,
                max_buffered_output_bytes: 1 << 30,
                max_buffered_input_bytes: 1 << 30,
                ..Default::default()
            },
        )
        .map_err(|e| format!("soak encode round {}: {:?}", round, e))?;
        let djobs: Vec<DecodeBlockJob> = enc
            .blocks
            .into_iter()
            .map(|b| DecodeBlockJob {
                block_index: b.block_index,
                block_data: b.block,
            })
            .collect();
        let m_pre = cache.metrics();
        let decoded = decoder
            .decode_blocks(djobs)
            .map_err(|e| format!("soak decode round {}: {:?}", round, e))?;
        blocks_decoded += decoded.blocks.len() as u64;
        let m_post = cache.metrics();
        duplicate_builds += m_post
            .builds_started
            .saturating_sub(m_pre.builds_started)
            .saturating_sub(
                m_post.builds_completed + m_post.build_failures
                    - (m_pre.builds_completed + m_pre.build_failures),
            );

        // ---- Periodic invariant checks (every 16 rounds) --------------------
        if round % 16 == 15 {
            let m = cache.metrics();
            if m.builds_completed + m.build_failures > m.builds_started {
                return Err(format!(
                    "soak round {}: build accounting invariant violated",
                    round
                ));
            }
            if m.current_entries > 64 || m.current_bytes > 16 * 1024 * 1024 {
                return Err(format!("soak round {}: capacity bound violated", round));
            }
            if m.hits + m.misses != m.lookups {
                return Err(format!(
                    "soak round {}: hit/miss sum invariant violated",
                    round
                ));
            }
            println!(
                "  soak round {:>3}: {} blocks, {} MiB, cache entries={} bytes={} hits={} builds={}",
                round,
                blocks_decoded,
                bytes_decoded / (1024 * 1024),
                m.current_entries,
                m.current_bytes,
                m.hits,
                m.builds_started,
            );
        }
    }

    let m = cache.metrics();
    // O.18 completion assertions.
    if m.builds_completed + m.build_failures > m.builds_started {
        return Err("build accounting invariant violated at completion".into());
    }
    if duplicate_builds != 0 {
        return Err(format!(
            "duplicate successful builds detected: {}",
            duplicate_builds
        ));
    }
    if m.current_entries > 64 || m.current_bytes > 16 * 1024 * 1024 {
        return Err("capacity bound violated at completion".into());
    }
    if m.hits + m.misses != m.lookups {
        return Err("hit/miss sum invariant violated at completion".into());
    }
    println!(
        "synthetic-cache-soak-v1 complete: {} rounds, {} blocks, {} MiB decoded; hits={} builds={} evictions={} entries={} bytes={} — invariants hold",
        rounds,
        blocks_decoded,
        bytes_decoded / (1024 * 1024),
        m.hits,
        m.builds_started,
        m.entry_evictions,
        m.current_entries,
        m.current_bytes,
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Public-corpus stress / soak — genuine derived-schedule execution
// (MODEL_CACHE.WORKLOAD.2 fix: the derived manifest IS the workload)
// ---------------------------------------------------------------------------

/// Streaming SHA-256 of a file (never loads the file into memory — the
/// public sources include 1 GiB files; hashing them must not spike RAM).
fn sha256_file_streaming(p: &Path) -> Result<String, String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(p).map_err(|e| format!("open {}: {}", p.display(), e))?;
    let mut h = sha2::Sha256::new();
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("read {}: {}", p.display(), e))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}

/// A resolved public source file: path + length, hash-verified once.
struct ResolvedSource {
    path: PathBuf,
    len: u64,
}

/// Resolves schedule blocks against the fetched, hash-verified extracted
/// source tree (MODEL_CACHE.WORKLOAD.2).
///
/// Layout (from `workload fetch`): archive sources (zip/tar.gz) extract
/// into `extracted/<id>/<files...>`; plain-gz sources are single payloads
/// written flat as `extracted/<id>`.  A `source_id` is a FAMILY, not a
/// file, so a block's `source_sha256` selects the exact file — the
/// resolver walks the family, stream-hashes each candidate file, and
/// matches the pinned hash.  Every file is verified ONCE and the path is
/// cached; a mismatch or a missing file is a hard error (never a
/// substituted file, never a silent skip).
struct PublicSourceResolver {
    extracted: PathBuf,
    by_sha: std::collections::HashMap<String, ResolvedSource>,
}

impl PublicSourceResolver {
    fn new(extracted: &Path) -> Self {
        Self {
            extracted: extracted.to_path_buf(),
            by_sha: std::collections::HashMap::new(),
        }
    }

    /// Locate and hash-verify the file for `source_id` whose SHA-256 is
    /// `sha`.  Resolves once, caches by hash.  Returns `()` on success;
    /// callers that need the file metadata use [`Self::get`].
    fn resolve(&mut self, source_id: &str, sha: &str) -> Result<(), String> {
        if self.by_sha.contains_key(sha) {
            return Ok(());
        }
        let dir = self.extracted.join(source_id);
        let mut candidates: Vec<PathBuf> = Vec::new();
        if dir.is_dir() {
            for e in std::fs::read_dir(&dir)
                .map_err(|e| format!("read dir {}: {}", dir.display(), e))?
                .flatten()
            {
                let p = e.path();
                if p.is_file() {
                    candidates.push(p);
                }
            }
        } else if dir.is_file() {
            candidates.push(dir.clone());
        } else {
            return Err(format!(
                "source {}: {} is neither a file nor a directory (run `cargo xtask workload fetch` first)",
                source_id,
                dir.display()
            ));
        }
        if candidates.is_empty() {
            return Err(format!("no files under {}", dir.display()));
        }
        // Deterministic order (the schedule pins the FILE by hash, so the
        // selection is hash-driven, but a stable walk order keeps the
        // error messages reproducible).
        candidates.sort();
        for p in &candidates {
            let got = sha256_file_streaming(p)?;
            if got == sha {
                let len = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
                self.by_sha.insert(
                    sha.to_string(),
                    ResolvedSource {
                        path: p.clone(),
                        len,
                    },
                );
                return Ok(());
            }
        }
        Err(format!(
            "no file under {} matches the pinned sha256 {}... ({} candidate(s)); the extracted tree does not match the manifest — re-run `cargo xtask workload fetch`",
            dir.display(),
            &sha[..16.min(sha.len())],
            candidates.len()
        ))
    }

    /// Borrow the resolved metadata for a pinned hash (must be resolved
    /// first).
    fn get(&self, sha: &str) -> Result<&ResolvedSource, String> {
        self.by_sha
            .get(sha)
            .ok_or_else(|| format!("resolve before get for sha {}", &sha[..16.min(sha.len())]))
    }

    /// Read the byte slice `[offset, offset+len)` of the file pinned by
    /// `sha` (streaming seek+read — never materializes the whole file).
    fn slice(&self, sha: &str, offset: u64, len: u64) -> Result<Vec<u8>, String> {
        use std::io::{Seek, SeekFrom};
        let rs = self
            .by_sha
            .get(sha)
            .ok_or_else(|| format!("slice before resolve for sha {}", &sha[..16.min(sha.len())]))?;
        let end = offset
            .checked_add(len)
            .ok_or_else(|| format!("slice overflow {}:{}", sha, offset))?;
        if end > rs.len {
            return Err(format!(
                "slice {} [{}, {}) beyond file length {}",
                &sha[..16.min(sha.len())],
                offset,
                end,
                rs.len
            ));
        }
        let mut f = std::fs::File::open(&rs.path)
            .map_err(|e| format!("open {}: {}", rs.path.display(), e))?;
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("seek {}: {}", rs.path.display(), e))?;
        let mut out = vec![0u8; len as usize];
        f.read_exact(&mut out)
            .map_err(|e| format!("read {}: {}", rs.path.display(), e))?;
        Ok(out)
    }
}

/// The model mode of a public stress/soak case (Phase O.13).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PublicMode {
    /// Every block derives its own model from its own bytes.
    Natural,
    /// Every block in a model group reuses the group's trained model
    /// (trained on the declared public training region).
    Grouped,
}

impl PublicMode {
    fn name(self) -> &'static str {
        match self {
            PublicMode::Natural => "natural",
            PublicMode::Grouped => "grouped",
        }
    }

    fn parse(s: &str) -> Result<Vec<PublicMode>, String> {
        match s {
            "natural" => Ok(vec![PublicMode::Natural]),
            "grouped" => Ok(vec![PublicMode::Grouped]),
            "both" => Ok(vec![PublicMode::Natural, PublicMode::Grouped]),
            other => Err(format!(
                "unknown mode {} (expected natural | grouped | both)",
                other
            )),
        }
    }
}

/// Select a schedule by name; the four derived names are the only valid
/// values (MODEL_CACHE.WORKLOAD.2: `--schedule` must select an actual
/// schedule, never be inert).
fn select_schedule<'a>(
    manifest: &'a ryg_rans_rs_casefile::WorkloadManifest,
    name: &str,
) -> Result<&'a ryg_rans_rs_casefile::WorkloadSchedule, String> {
    const KNOWN: [&str; 4] = [
        "public-rans-smoke",
        "public-rans-1g",
        "public-rans-mixed-16g",
        "public-rans-stress-64g",
    ];
    if !KNOWN.contains(&name) {
        return Err(format!(
            "unknown schedule {:?} (expected {})",
            name,
            KNOWN.join(" | ")
        ));
    }
    manifest
        .schedules
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| {
            format!(
                "schedule {} not found in manifest (run `cargo xtask workload derive`)",
                name
            )
        })
}

/// Parallel config for the public runner: the model cache measures cache
/// behavior, not output memory bounds, so the I/O budgets are raised; the
/// reorder window is sized to the window length (window + workers + margin).
fn public_config(workers: usize, window_len: usize, simd_off: bool) -> ParallelConfig {
    ParallelConfig {
        threads: ThreadCount::Exact(std::num::NonZeroUsize::new(workers.max(1)).unwrap()),
        parallel_threshold_bytes: 0,
        disable_simd: simd_off,
        max_buffered_output_bytes: 8 << 30,
        max_buffered_input_bytes: 8 << 30,
        max_in_flight_blocks: std::num::NonZeroUsize::new(window_len + workers + 16).unwrap(),
        ..Default::default()
    }
}

/// Cache budget for the public runner, read from the workload spec's
/// `[cache] default` section so the executed policy is the declared policy.
fn public_cache_budget(spec_dir: &Path) -> Result<(usize, u64), String> {
    let raw = std::fs::read_to_string(spec_dir.join("derivation.toml"))
        .map_err(|e| format!("read derivation.toml: {}", e))?;
    let deriv: toml::Value =
        toml::from_str(&raw).map_err(|e| format!("parse derivation.toml: {}", e))?;
    let entries = deriv
        .get("cache")
        .and_then(|c| c.get("default"))
        .and_then(|d| d.get("max_entries"))
        .and_then(|v| v.as_integer())
        .unwrap_or(64) as usize;
    let bytes = deriv
        .get("cache")
        .and_then(|c| c.get("default"))
        .and_then(|d| d.get("max_total_bytes"))
        .and_then(|v| v.as_integer())
        .unwrap_or(16 * 1024 * 1024) as u64;
    Ok((entries, bytes))
}

/// Deterministic ordered list of the schedule's distinct pinned files
/// (first-appearance order) as `(source_id, sha256)` pairs — the
/// `num_sources` of the derivation policy `source[g % num_sources]` (the
/// same interpretation the sealed MIXED_PUBLIC bench uses).  The
/// `source_id` travels with the sha so the training region can be resolved
/// (a sha alone cannot locate the file — a `source_id` is a family).
fn distinct_source_shas(
    schedule: &ryg_rans_rs_casefile::WorkloadSchedule,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for b in &schedule.blocks {
        if !out.iter().any(|(_, sha)| sha == &b.source_sha256) {
            out.push((b.source_id.clone(), b.source_sha256.clone()));
        }
    }
    out
}

/// Train the model for group `g` from the declared public training region
/// (derivation.toml `[models]`: `source[g % num_sources]`, bytes
/// `[0, min(training_region_bytes, file_len))`).  The model bytes are
/// produced by encoding the training region with `PerBlock` and extracting
/// the embedded 1024-byte model (header offset 104) — the same public-API
/// route the bench's grouped path uses.  Trained once per group, cached in
/// `group_models`.
fn train_group_model(
    g: u64,
    distinct_sources: &[(String, String)],
    training_region: u64,
    scale_bits: u8,
    resolver: &mut PublicSourceResolver,
    group_models: &mut std::collections::HashMap<u64, Vec<u8>>,
) -> Result<(), String> {
    if group_models.contains_key(&g) {
        return Ok(());
    }
    let n = distinct_sources.len() as u64;
    if n == 0 {
        return Err("schedule has no distinct source files — cannot train group models".into());
    }
    let (src_id, sha) = &distinct_sources[(g % n) as usize];
    // Resolve the training file first so its length is known for the clamp.
    resolver.resolve(src_id, sha)?;
    let file_len = resolver.get(sha)?.len;
    let region_len = training_region.min(file_len);
    let region = resolver.slice(sha, 0, region_len)?;
    let job = EncodeBlockJob::new(
        0,
        region,
        CodecPolicy::Auto,
        ModelPolicy::PerBlock,
        scale_bits,
    );
    let enc = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(
        vec![job],
        &public_config(1, 8, false),
    )
    .map_err(|e| format!("train group {}: {:?}", g, e))?;
    let block0 = &enc.blocks[0].block;
    let model = block0
        .get(104..104 + 1024)
        .ok_or_else(|| format!("group {}: block carries no 1024-byte model", g))?
        .to_vec();
    group_models.insert(g, model);
    Ok(())
}

/// Encode one window with grouped-model fallback.
///
/// The whole window is encoded as one batch (indices 0..W, window-relative).
/// In grouped mode a block whose data contains a symbol absent from its
/// group's trained model fails with a typed `EncodeFailed` carrying the
/// block index; that block is downgraded to `PerBlock` and counted as a
/// fallback, and the batch is retried.  Fallbacks are rare (the training
/// region and the block data come from the same corpus family), so the
/// retry loop is short; every downgrade is counted and reported (never
/// silently dropped — Phase O.13 honesty).
///
/// Returns the source slices (for byte-exact verification) and the decode
/// jobs with window-relative header indices (the decode batch must be a
/// 0-based contiguous set — Phase L.5 reorder contract).
fn encode_window(
    window: &[ryg_rans_rs_casefile::WorkloadBlock],
    mode: PublicMode,
    resolver: &mut PublicSourceResolver,
    group_models: &mut std::collections::HashMap<u64, Vec<u8>>,
    distinct_sources: &[(String, String)],
    training_region: u64,
    fallbacks: &mut u64,
) -> Result<(Vec<Vec<u8>>, Vec<DecodeBlockJob>), String> {
    let mut sources: Vec<Vec<u8>> = Vec::with_capacity(window.len());
    let mut jobs: Vec<EncodeBlockJob> = Vec::with_capacity(window.len());
    for (j, blk) in window.iter().enumerate() {
        resolver.resolve(&blk.source_id, &blk.source_sha256)?;
        let data = resolver.slice(&blk.source_sha256, blk.offset, blk.length)?;
        let policy = match mode {
            PublicMode::Natural => ModelPolicy::PerBlock,
            PublicMode::Grouped => {
                if blk.model_group == u64::MAX {
                    // Natural block in the schedule: never grouped.
                    ModelPolicy::PerBlock
                } else {
                    train_group_model(
                        blk.model_group,
                        distinct_sources,
                        training_region,
                        blk.scale_bits,
                        resolver,
                        group_models,
                    )?;
                    ModelPolicy::External {
                        model: group_models[&blk.model_group].clone(),
                    }
                }
            }
        };
        sources.push(data.clone());
        jobs.push(EncodeBlockJob::new(
            j as u64,
            data,
            CodecPolicy::Explicit(blk.codec_id),
            policy,
            blk.scale_bits,
        ));
    }

    // Batch encode with grouped fallback retries.
    let enc_cfg = public_config(1, window.len(), false);
    let encoded = loop {
        match ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs.clone(), &enc_cfg) {
            Ok(enc) => break enc,
            Err(ryg_rans_rs_parallel::ParallelError::EncodeFailed(canonical)) => {
                let bi = canonical.block_index;
                let idx = bi as usize;
                if idx >= jobs.len() {
                    return Err(format!(
                        "encode failed with out-of-range block index {} (window {})",
                        bi,
                        window.len()
                    ));
                }
                // Downgrade that block to its own model and count it.
                let EncodeBlockJob {
                    block_index,
                    data,
                    codec_policy,
                    model_policy,
                    scale_bits,
                    ..
                } = &jobs[idx];
                if matches!(model_policy, ModelPolicy::PerBlock) {
                    // A PerBlock encode failed: not a fallback case —
                    // report the underlying error.
                    return Err(format!(
                        "block {} PerBlock encode failed: {:?}",
                        block_index, canonical
                    ));
                }
                *fallbacks += 1;
                jobs[idx] = EncodeBlockJob::new(
                    *block_index,
                    data.clone(),
                    *codec_policy,
                    ModelPolicy::PerBlock,
                    *scale_bits,
                );
            }
            Err(e) => return Err(format!("window encode: {:?}", e)),
        }
    };

    // Patch the embedded header index (offset 8..16, covered by no header
    // hash) to the window-relative index so the decode batch is 0-based
    // contiguous.
    let decode_jobs: Vec<DecodeBlockJob> = encoded
        .blocks
        .into_iter()
        .enumerate()
        .map(|(j, b)| {
            let mut block = b.block;
            block[8..16].copy_from_slice(&(j as u64).to_le_bytes());
            DecodeBlockJob {
                block_index: j as u64,
                block_data: block,
            }
        })
        .collect();
    Ok((sources, decode_jobs))
}

/// Encode + decode one window of the schedule and verify byte-exactness.
/// Returns (block count, decoded bytes).  `window_base` is the schedule
/// index of the window's first block (for error messages only).
fn process_window(
    window: &[ryg_rans_rs_casefile::WorkloadBlock],
    window_base: u64,
    mode: PublicMode,
    workers: usize,
    simd_off: bool,
    cache: &std::sync::Arc<ModelArtifactCache>,
    resolver: &mut PublicSourceResolver,
    group_models: &mut std::collections::HashMap<u64, Vec<u8>>,
    distinct_sources: &[(String, String)],
    training_region: u64,
    fallbacks: &mut u64,
    cancel: Option<&std::sync::Arc<CancellationToken>>,
) -> Result<(u64, u64), String> {
    let (sources, decode_jobs) = encode_window(
        window,
        mode,
        resolver,
        group_models,
        distinct_sources,
        training_region,
        fallbacks,
    )?;
    let cfg = public_config(workers, window.len(), simd_off);
    let decoder = ParallelDecoder::with_model_cache(cfg, cache.clone());
    let decoded = match cancel {
        Some(tok) => decoder
            .decode_blocks_with_cancel(decode_jobs, Some(tok.clone()))
            .map_err(|e| format!("decode window @ {}: {:?}", window_base, e))?,
        None => decoder
            .decode_blocks(decode_jobs)
            .map_err(|e| format!("decode window @ {}: {:?}", window_base, e))?,
    };
    if decoded.blocks.len() != window.len() {
        return Err(format!(
            "window @ {}: decoded {} blocks, expected {}",
            window_base,
            decoded.blocks.len(),
            window.len()
        ));
    }
    let mut bytes = 0u64;
    for (i, db) in decoded.blocks.iter().enumerate() {
        if db.output != sources[i] {
            return Err(format!(
                "window @ {} block {}: decoded output differs from the source slice ({} vs {} bytes)",
                window_base,
                window_base + i as u64,
                db.output.len(),
                sources[i].len()
            ));
        }
        bytes += db.output.len() as u64;
    }
    Ok((window.len() as u64, bytes))
}

/// Probe that a key is not stuck behind an abandoned `Building` marker
/// (O.18 "no abandoned Building entry"): issue a lookup for `model` with a
/// watchdog.  A healthy cache returns instantly (hit or fresh build); an
/// abandoned marker would block the lookup until the watchdog fires.
///
/// The probe runs AFTER the case's metric deltas are captured, so it never
/// perturbs the measured counters.
fn probe_no_abandoned_building(
    cache: &std::sync::Arc<ModelArtifactCache>,
    codec: u16,
    scale: u8,
    model: Vec<u8>,
    timeout: std::time::Duration,
) -> Result<(), String> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let cache2 = cache.clone();
    let cancel = std::sync::Arc::new(CancellationToken::new());
    let cancel2 = cancel.clone();
    let m2 = model.clone();
    std::thread::spawn(move || {
        let r = cache2.get_or_build(codec, scale, &m2, Some(&cancel2), || {
            ryg_rans_rs_parallel::build_validated_model_artifacts(codec, scale, &m2)
        });
        let _ = tx.send(match r {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("probe build failed: {:?}", e)),
        });
    });
    match rx.recv_timeout(timeout) {
        Ok(r) => r,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(format!(
            "probe lookup did not complete within {}s — an abandoned Building marker is suspected",
            timeout.as_secs()
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            Err("probe thread died without a result".into())
        }
    }
}

/// Model bytes for a probe: encode `data` with PerBlock and extract the
/// embedded 1024-byte model (header offset 104).
fn model_bytes_for_probe(data: &[u8], codec: u16, scale: u8) -> Result<Vec<u8>, String> {
    let job = EncodeBlockJob::new(
        0,
        data.to_vec(),
        CodecPolicy::Explicit(codec),
        ModelPolicy::PerBlock,
        scale,
    );
    let enc = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(
        vec![job],
        &public_config(1, 8, false),
    )
    .map_err(|e| format!("probe model encode: {:?}", e))?;
    enc.blocks[0]
        .block
        .get(104..104 + 1024)
        .map(|m| m.to_vec())
        .ok_or_else(|| "probe: block carries no 1024-byte model".into())
}

/// Parse the public stress/soak argument set.
struct PublicRunnerArgs {
    schedule: String,
    workers: Option<usize>,
    simd_off: bool,
    modes: Vec<PublicMode>,
    window: Option<usize>,
    max_blocks: Option<u64>,
    rounds: u64,
}

impl PublicRunnerArgs {
    fn parse(args: &[String], default_schedule: &str) -> Result<Self, String> {
        let mut a = PublicRunnerArgs {
            schedule: default_schedule.to_string(),
            workers: None,
            simd_off: false,
            modes: vec![PublicMode::Natural, PublicMode::Grouped],
            window: None,
            max_blocks: None,
            rounds: std::env::var("RYG_RANS_SOAK_PUBLIC_ROUNDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
        };
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "--schedule" => {
                    i += 1;
                    a.schedule = args
                        .get(i)
                        .cloned()
                        .ok_or("--schedule requires a schedule name")?;
                }
                "--workers" => {
                    i += 1;
                    a.workers = Some(
                        args.get(i)
                            .and_then(|s| s.parse::<usize>().ok())
                            .ok_or("--workers requires a usize")?,
                    );
                }
                "--simd-off" => a.simd_off = true,
                "--mode" => {
                    i += 1;
                    a.modes = PublicMode::parse(
                        args.get(i)
                            .ok_or("--mode requires natural | grouped | both")?,
                    )?;
                }
                "--window" => {
                    i += 1;
                    a.window = Some(
                        args.get(i)
                            .and_then(|s| s.parse::<usize>().ok())
                            .ok_or("--window requires a usize")?,
                    );
                }
                "--max-blocks" => {
                    i += 1;
                    a.max_blocks = Some(
                        args.get(i)
                            .and_then(|s| s.parse::<u64>().ok())
                            .ok_or("--max-blocks requires a u64")?,
                    );
                }
                "--rounds" => {
                    i += 1;
                    a.rounds = args
                        .get(i)
                        .and_then(|s| s.parse::<u64>().ok())
                        .ok_or("--rounds requires a u64")?;
                }
                other => return Err(format!("unknown public runner option: {}", other)),
            }
            i += 1;
        }
        Ok(a)
    }
}

/// Load the manifest + schedule + resolver common to both public runners.
struct PublicRun {
    schedule: ryg_rans_rs_casefile::WorkloadSchedule,
    extracted: PathBuf,
    distinct_sources: Vec<(String, String)>,
    training_region: u64,
    cache_entries: usize,
    cache_bytes: u64,
}

fn load_public_run(spec_dir: &Path, name: &str, schedule_name: &str) -> Result<PublicRun, String> {
    let root = cache_root(name)?;
    let manifest_path = root.join("derived").join(format!("{}.manifest.json", name));
    let manifest_raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {}", manifest_path.display(), e))?;
    let manifest: ryg_rans_rs_casefile::WorkloadManifest =
        serde_json::from_str(&manifest_raw).map_err(|e| format!("parse manifest: {}", e))?;
    let schedule = select_schedule(&manifest, schedule_name)?.clone();
    if schedule.blocks.is_empty() {
        return Err(format!("schedule {} has no blocks", schedule_name));
    }
    let extracted = root.join("extracted");
    if !extracted.is_dir() {
        return Err(
            "sources not fetched — run `cargo xtask workload fetch public-rans-v1` first".into(),
        );
    }
    let (cache_entries, cache_bytes) = public_cache_budget(spec_dir)?;
    // Training region from derivation.toml [models].
    let deriv_raw = std::fs::read_to_string(spec_dir.join("derivation.toml"))
        .map_err(|e| format!("read derivation.toml: {}", e))?;
    let deriv: toml::Value =
        toml::from_str(&deriv_raw).map_err(|e| format!("parse derivation.toml: {}", e))?;
    let training_region = deriv
        .get("models")
        .and_then(|m| m.get("training_region_bytes"))
        .and_then(|v| v.as_integer())
        .unwrap_or(4096) as u64;
    let distinct_sources = distinct_source_shas(&schedule);
    Ok(PublicRun {
        schedule,
        extracted,
        distinct_sources,
        training_region,
        cache_entries,
        cache_bytes,
    })
}

/// Genuine public-corpus stress runner: `cargo xtask workload stress-public
/// public-rans-v1 [--schedule NAME] [--workers N] [--mode natural|grouped|both]
/// [--simd-off] [--window W] [--max-blocks N]`.
///
/// Consumes the derived manifest schedule: every block is
/// `source_id + source_sha256 + offset + length` resolved to the
/// hash-verified extracted source bytes, sliced, encoded with the block's
/// declared codec/scale, and decoded through the parallel engine
/// (MODEL_CACHE.WORKLOAD.2 fix).  `--schedule` actually selects the
/// executed schedule (smoke / 1g / mixed-16g / stress-64g).  Schedules are
/// processed in bounded windows (window × block size ≤ 256 MiB), so the
/// 16 GiB and 64 GiB logical schedules stream — the corpus is never
/// materialized.  Every window asserts byte-exact output; every case ends
/// with the O.18 completion invariants and a no-abandoned-marker probe.
pub fn cmd_workload_stress_public(
    spec_dir: &Path,
    name: &str,
    args: &[String],
) -> Result<(), String> {
    let a = PublicRunnerArgs::parse(args, "public-rans-smoke")?;
    let run = load_public_run(spec_dir, name, &a.schedule)?;
    println!(
        "stress-public: schedule {} ({} blocks, {} logical MiB, sha256 {})",
        run.schedule.name,
        run.schedule.block_count,
        run.schedule.logical_bytes / (1024 * 1024),
        &run.schedule.schedule_sha256[..16.min(run.schedule.schedule_sha256.len())]
    );
    println!(
        "  sources: {} distinct pinned files; cache budget {} entries / {} MiB; grouped training region {} bytes",
        run.distinct_sources.len(),
        run.cache_entries,
        run.cache_bytes / (1024 * 1024),
        run.training_region
    );

    // Worker matrix.
    let workers: Vec<usize> = match a.workers {
        Some(w) => vec![w],
        None => vec![1, 2, 4, 8, 16, 32],
    };
    let blocks: Vec<ryg_rans_rs_casefile::WorkloadBlock> = match a.max_blocks {
        Some(cap) => run
            .schedule
            .blocks
            .iter()
            .take(cap as usize)
            .cloned()
            .collect(),
        None => run.schedule.blocks.clone(),
    };
    let max_block_size = blocks.iter().map(|b| b.length).max().unwrap_or(4096);
    let window_len = a
        .window
        .unwrap_or_else(|| ((256u64 * 1024 * 1024) / max_block_size.max(1)).clamp(8, 256) as usize);
    let n_windows = blocks.len().div_ceil(window_len);
    println!(
        "  executing {} blocks in {} windows of <= {} (max block {} bytes)",
        blocks.len(),
        n_windows,
        window_len,
        max_block_size
    );

    let mut failures: Vec<String> = Vec::new();
    let mut total_cases = 0u64;
    let mut total_blocks = 0u64;
    let mut total_bytes = 0u64;
    for &w in &workers {
        for &mode in &a.modes {
            let cache = std::sync::Arc::new(ModelArtifactCache::bounded(
                run.cache_entries,
                run.cache_bytes,
            ));
            let mut resolver = PublicSourceResolver::new(&run.extracted);
            let mut group_models: std::collections::HashMap<u64, Vec<u8>> =
                std::collections::HashMap::new();
            let mut fallbacks = 0u64;
            let pre = cache.metrics();
            let mut case_blocks = 0u64;
            let mut case_bytes = 0u64;
            let mut case_err: Option<String> = None;
            for (wi, chunk) in blocks.chunks(window_len).enumerate() {
                let window_base = (wi * window_len) as u64;
                match process_window(
                    chunk,
                    window_base,
                    mode,
                    w,
                    a.simd_off,
                    &cache,
                    &mut resolver,
                    &mut group_models,
                    &run.distinct_sources,
                    run.training_region,
                    &mut fallbacks,
                    None,
                ) {
                    Ok((nb, nbytes)) => {
                        case_blocks += nb;
                        case_bytes += nbytes;
                    }
                    Err(e) => {
                        case_err = Some(e);
                        break;
                    }
                }
            }
            let post = cache.metrics();
            if let Some(e) = case_err {
                failures.push(format!(
                    "{} w={} schedule={}: {}",
                    mode.name(),
                    w,
                    a.schedule,
                    e
                ));
                println!(
                    "  stress-public {:>7} w={:>2} schedule={}: FAILED — {}",
                    mode.name(),
                    w,
                    a.schedule,
                    e
                );
                continue;
            }
            // ---- O.18 completion invariants ----------------------------------
            let builds = post.builds_started - pre.builds_started;
            let hits = post.hits - pre.hits;
            let misses = post.misses - pre.misses;
            let evictions = post.entry_evictions - pre.entry_evictions;
            let check = |ok: bool, what: &str| -> Result<(), String> {
                if ok {
                    Ok(())
                } else {
                    Err(format!(
                        "{} w={} schedule={}: invariant violated — {}",
                        mode.name(),
                        w,
                        a.schedule,
                        what
                    ))
                }
            };
            let inv = post.invariant_build_accounting();
            let inv2 = post.invariant_hit_miss_sum();
            let inv3 = post.current_entries <= run.cache_entries;
            let inv4 = post.current_bytes <= run.cache_bytes;
            if let Err(e) = check(inv, "build accounting (completed + failures > started)")
                .and_then(|_| check(inv2, "hits + misses != lookups"))
                .and_then(|_| check(inv3, "current_entries > capacity"))
                .and_then(|_| check(inv4, "current_bytes > budget"))
            {
                failures.push(e.clone());
                println!(
                    "  stress-public {} w={} schedule={}: FAILED — {}",
                    mode.name(),
                    w,
                    a.schedule,
                    e
                );
                continue;
            }
            // ---- No-abandoned-marker probe (first block's model) ------------
            let first = &blocks[0];
            let probe_data = resolver.slice(&first.source_sha256, first.offset, first.length)?;
            let probe_model = model_bytes_for_probe(&probe_data, first.codec_id, first.scale_bits)?;
            if let Err(e) = probe_no_abandoned_building(
                &cache,
                first.codec_id,
                first.scale_bits,
                probe_model,
                std::time::Duration::from_secs(120),
            ) {
                failures.push(format!("{} w={}: probe: {}", mode.name(), w, e));
                println!(
                    "  stress-public {} w={}: FAILED — probe: {}",
                    mode.name(),
                    w,
                    e
                );
                continue;
            }
            total_cases += 1;
            total_blocks += case_blocks;
            total_bytes += case_bytes;
            let mode_label = if mode == PublicMode::Grouped {
                format!("grouped fallbacks={}", fallbacks)
            } else {
                "natural".to_string()
            };
            println!(
                "  stress-public {:>7} w={:>2} schedule={}: {} blocks, {} MiB decoded, hits={} misses={} builds={} evictions={} [{}] — OK",
                mode.name(),
                w,
                a.schedule,
                case_blocks,
                case_bytes / (1024 * 1024),
                hits,
                misses,
                builds,
                evictions,
                mode_label
            );
        }
    }

    println!(
        "stress-public complete: schedule={} {} cases, {} blocks, {} MiB decoded; {} failure(s)",
        a.schedule,
        total_cases,
        total_blocks,
        total_bytes / (1024 * 1024),
        failures.len()
    );
    if !failures.is_empty() {
        return Err(format!("stress-public failures:\n{}", failures.join("\n")));
    }
    Ok(())
}

/// Genuine public-corpus soak runner: `cargo xtask workload soak-public
/// public-rans-v1 [--schedule NAME] [--workers N] [--mode natural|grouped|both]
/// [--rounds N] [--window W] [--max-blocks N]`.
///
/// Repeatedly executes the derived public schedule (default `public-rans-1g`)
/// in bounded windows against one persistent cache, checking the O.18
/// invariants periodically, with a deterministic mid-run cancellation round
/// (a cancelled decode must never return `Ok` with fewer blocks than
/// declared — the executor's completeness contract — and the cache must
/// recover with no abandoned marker).  At completion asserts the O.18 soak
/// contract:
///
/// ```text
/// current_bytes <= budget, current_entries <= capacity
/// builds_completed + build_failures <= builds_started
/// hits + misses == lookups
/// duplicate successful builds == 0 (replacements == 0)
/// decoded output == source input (asserted per window)
/// no abandoned Building entry (probe)
/// ```
pub fn cmd_workload_soak_public(
    spec_dir: &Path,
    name: &str,
    args: &[String],
) -> Result<(), String> {
    let a = PublicRunnerArgs::parse(args, "public-rans-1g")?;
    let run = load_public_run(spec_dir, name, &a.schedule)?;
    let workers = a.workers.unwrap_or(8);
    let rounds = a.rounds.max(1);
    println!(
        "soak-public: schedule {} ({} blocks, {} logical MiB, sha256 {}), workers={}, rounds={}, mode(s)={}",
        run.schedule.name,
        run.schedule.block_count,
        run.schedule.logical_bytes / (1024 * 1024),
        &run.schedule.schedule_sha256[..16.min(run.schedule.schedule_sha256.len())],
        workers,
        rounds,
        a.modes
            .iter()
            .map(|m| m.name())
            .collect::<Vec<_>>()
            .join("+")
    );

    let blocks: Vec<ryg_rans_rs_casefile::WorkloadBlock> = match a.max_blocks {
        Some(cap) => run
            .schedule
            .blocks
            .iter()
            .take(cap as usize)
            .cloned()
            .collect(),
        None => run.schedule.blocks.clone(),
    };
    let max_block_size = blocks.iter().map(|b| b.length).max().unwrap_or(4096);
    let window_len = a
        .window
        .unwrap_or_else(|| ((256u64 * 1024 * 1024) / max_block_size.max(1)).clamp(8, 256) as usize);

    let cache = std::sync::Arc::new(ModelArtifactCache::bounded(
        run.cache_entries,
        run.cache_bytes,
    ));
    let mut total_blocks = 0u64;
    let mut total_bytes = 0u64;
    let mut fallbacks = 0u64;
    let mut windows_seen = 0u64;
    let cancellation_round = rounds / 2;
    // The cancellation window is the midpoint of the cancellation round's
    // first pass (deterministic; the phase where the cache is warm).
    let cancellation_window = (blocks.len() / window_len / 2) as u64;
    let mut cancelled_ok = false;

    for round in 0..rounds {
        for &mode in &a.modes {
            let mut resolver = PublicSourceResolver::new(&run.extracted);
            let mut group_models: std::collections::HashMap<u64, Vec<u8>> =
                std::collections::HashMap::new();
            for (wi, chunk) in blocks.chunks(window_len).enumerate() {
                let window_base = (wi * window_len) as u64;
                let is_cancel_window = round == cancellation_round
                    && mode == PublicMode::Natural
                    && window_base == cancellation_window * window_len as u64;
                // The cancellation round exercises the completeness contract
                // exactly once per soak; the window is then re-run without
                // cancellation so the soak's own byte-exact accounting stays
                // complete.
                if is_cancel_window && !cancelled_ok {
                    let tok = std::sync::Arc::new(CancellationToken::new());
                    let cancel_tok = tok.clone();
                    let cancel_handle = std::thread::spawn(move || {
                        // Fire mid-flight: after the decode is under way.
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        cancel_tok.cancel();
                    });
                    match process_window(
                        chunk,
                        window_base,
                        mode,
                        workers,
                        a.simd_off,
                        &cache,
                        &mut resolver,
                        &mut group_models,
                        &run.distinct_sources,
                        run.training_region,
                        &mut fallbacks,
                        Some(&tok),
                    ) {
                        // Either the run completed fully (cancellation arrived
                        // after the last block — Ok with ALL blocks) or the
                        // executor returned the typed Cancelled error
                        // (serialized as `Cancelled { completed: ..,
                        // expected: .. }`).  A short Ok is impossible by
                        // construction; accepting only these two outcomes is
                        // the completeness check.
                        Ok(_) => {}
                        Err(e) if e.contains("Cancelled {") => {}
                        Err(e) => {
                            return Err(format!(
                                "soak cancellation round: unexpected error: {}",
                                e
                            ));
                        }
                    }
                    let _ = cancel_handle.join();
                    cancelled_ok = true;
                    println!(
                        "  soak-public round {}: cancellation round executed (complete-or-Cancelled, never short-Ok)",
                        round
                    );
                    // Re-run the window uncancelled: byte-exact accounting
                    // must stay complete.
                    match process_window(
                        chunk,
                        window_base,
                        mode,
                        workers,
                        a.simd_off,
                        &cache,
                        &mut resolver,
                        &mut group_models,
                        &run.distinct_sources,
                        run.training_region,
                        &mut fallbacks,
                        None,
                    ) {
                        Ok((nb, nbytes)) => {
                            total_blocks += nb;
                            total_bytes += nbytes;
                        }
                        Err(e) => return Err(format!("soak post-cancellation re-run: {}", e)),
                    }
                    continue;
                }
                match process_window(
                    chunk,
                    window_base,
                    mode,
                    workers,
                    a.simd_off,
                    &cache,
                    &mut resolver,
                    &mut group_models,
                    &run.distinct_sources,
                    run.training_region,
                    &mut fallbacks,
                    None,
                ) {
                    Ok((nb, nbytes)) => {
                        total_blocks += nb;
                        total_bytes += nbytes;
                    }
                    Err(e) => {
                        return Err(format!(
                            "soak round {} window @ {} ({}): {}",
                            round,
                            window_base,
                            mode.name(),
                            e
                        ));
                    }
                }
                windows_seen += 1;

                // ---- Periodic invariant checks (every 16 windows) ----------
                if windows_seen % 16 == 0 {
                    let m = cache.metrics();
                    if !m.invariant_build_accounting() {
                        return Err(format!(
                            "soak window {}: build accounting invariant violated",
                            windows_seen
                        ));
                    }
                    if !m.invariant_hit_miss_sum() {
                        return Err(format!(
                            "soak window {}: hit/miss sum invariant violated",
                            windows_seen
                        ));
                    }
                    if m.current_entries > run.cache_entries || m.current_bytes > run.cache_bytes {
                        return Err(format!(
                            "soak window {}: capacity bound violated ({} entries / {} bytes)",
                            windows_seen, m.current_entries, m.current_bytes
                        ));
                    }
                    println!(
                        "  soak-public window {:>5}: {} blocks, {} MiB, cache entries={} bytes={} hits={} builds={} evictions={}",
                        windows_seen,
                        total_blocks,
                        total_bytes / (1024 * 1024),
                        m.current_entries,
                        m.current_bytes,
                        m.hits,
                        m.builds_started,
                        m.entry_evictions,
                    );
                }
            }
        }
    }

    // ---- Completion assertions (O.18 soak contract) --------------------------
    let m = cache.metrics();
    if !m.invariant_build_accounting() {
        return Err("soak-public: build accounting invariant violated at completion".into());
    }
    if !m.invariant_hit_miss_sum() {
        return Err("soak-public: hit/miss sum invariant violated at completion".into());
    }
    if m.current_entries > run.cache_entries || m.current_bytes > run.cache_bytes {
        return Err("soak-public: capacity bound violated at completion".into());
    }
    if m.replacements != 0 {
        return Err(format!(
            "soak-public: duplicate successful builds detected ({} replacements)",
            m.replacements
        ));
    }
    // No-abandoned-marker probe on the first block's model.
    let first = &blocks[0];
    {
        let mut resolver = PublicSourceResolver::new(&run.extracted);
        resolver.resolve(&first.source_id, &first.source_sha256)?;
        let probe_data = resolver.slice(&first.source_sha256, first.offset, first.length)?;
        let probe_model = model_bytes_for_probe(&probe_data, first.codec_id, first.scale_bits)?;
        probe_no_abandoned_building(
            &cache,
            first.codec_id,
            first.scale_bits,
            probe_model,
            std::time::Duration::from_secs(120),
        )?;
    }
    println!(
        "soak-public complete: schedule={} rounds={} blocks={} MiB={} grouped-fallbacks={} hits={} builds={} evictions={} entries={} bytes={} — invariants hold",
        a.schedule,
        rounds,
        total_blocks,
        total_bytes / (1024 * 1024),
        fallbacks,
        m.hits,
        m.builds_started,
        m.entry_evictions,
        m.current_entries,
        m.current_bytes,
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Eviction-policy shadow simulation (Phase O.17)
// ---------------------------------------------------------------------------

/// A cache-resident key plus its accounted size for the shadow simulation.
/// The accounted size is the measured artifact cost (~17 KiB: 1 KiB
/// frequencies + 16 KiB packed table + overhead); using a uniform size per
/// artifact keeps entry-hit and byte-hit analyses comparable, which is
/// exactly the production situation (every admitted artifact is one built
/// packed table).
const ARTIFACT_ACCOUNTED_BYTES: u64 = 17_472;

/// Simulated cache policy outcome for one schedule at one capacity.
#[derive(Debug, Clone, Copy, Default)]
struct SimOutcome {
    hits: u64,
    misses: u64,
    evictions: u64,
    byte_hits: u64,
    byte_total: u64,
}

impl SimOutcome {
    fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    fn byte_hit_rate(&self) -> f64 {
        if self.byte_total == 0 {
            0.0
        } else {
            self.byte_hits as f64 / self.byte_total as f64
        }
    }
}

/// Deterministic FIFO cache simulation (the production policy).
fn simulate_fifo(keys: &[u64], capacity: usize) -> SimOutcome {
    let mut queue: std::collections::VecDeque<u64> = std::collections::VecDeque::new();
    let mut resident: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let mut o = SimOutcome::default();
    for &k in keys {
        if resident.contains(&k) {
            o.hits += 1;
            o.byte_hits += ARTIFACT_ACCOUNTED_BYTES;
        } else {
            o.misses += 1;
            if resident.len() >= capacity {
                if let Some(ev) = queue.pop_front() {
                    resident.remove(&ev);
                    o.evictions += 1;
                }
            }
            resident.insert(k);
            queue.push_back(k);
        }
        o.byte_total += ARTIFACT_ACCOUNTED_BYTES;
    }
    o
}

/// Deterministic LRU cache simulation (the shadow candidate).
fn simulate_lru(keys: &[u64], capacity: usize) -> SimOutcome {
    use std::collections::HashMap;
    let mut map: HashMap<u64, u64> = HashMap::new(); // key -> last-use seq
    let mut clock: u64 = 0;
    let mut o = SimOutcome::default();
    for &k in keys {
        clock += 1;
        if map.contains_key(&k) {
            o.hits += 1;
            o.byte_hits += ARTIFACT_ACCOUNTED_BYTES;
            map.insert(k, clock);
        } else {
            o.misses += 1;
            if map.len() >= capacity {
                // Evict the least-recently-used key.
                let lru = *map
                    .iter()
                    .min_by_key(|&(_, &s)| s)
                    .map(|(k, _)| k)
                    .unwrap_or(&k);
                map.remove(&lru);
                o.evictions += 1;
            }
            map.insert(k, clock);
        }
        o.byte_total += ARTIFACT_ACCOUNTED_BYTES;
    }
    o
}

/// `cargo xtask workload policy-sim public-rans-v1` — FIFO vs LRU shadow
/// simulation over the derived schedules (Phase O.17).
///
/// The production cache is FIFO (ADR-0016).  This command simulates the
/// deterministic model-key sequences of every derived schedule against
/// FIFO and LRU at several capacities and reports hit/byte-hit rates and
/// eviction counts.  The production policy changes only if the evidence
/// shows a material end-to-end benefit for another policy (ADR-0017
/// records the decision).
///
/// The model key of a block is its `(model_group, codec_id, scale_bits)`
/// identity: in grouped mode all blocks of one group share one artifact;
/// natural-mode blocks (`model_group == u64::MAX`) are unique by
/// construction and are therefore always misses under both policies (they
/// are included in the totals — honest, not hidden).
///
/// The byte dimension uses the measured uniform artifact size; entry-cap
/// capacities are chosen around the production 64-entry default.
pub fn cmd_workload_policy_sim(name: &str) -> Result<(), String> {
    let root = cache_root(name)?;
    let manifest_path = root.join("derived").join(format!("{}.manifest.json", name));
    let manifest_raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read {}: {}", manifest_path.display(), e))?;
    let manifest: ryg_rans_rs_casefile::WorkloadManifest =
        serde_json::from_str(&manifest_raw).map_err(|e| format!("parse manifest: {}", e))?;

    let capacities: [usize; 4] = [16, 64, 256, 1024];
    println!(
        "policy-sim: FIFO (production) vs LRU (candidate) over the {} derived schedules\n",
        manifest.schedules.len()
    );

    for schedule in &manifest.schedules {
        // Deterministic model-key sequence: grouped blocks share
        // (group, codec, scale); natural blocks are unique.
        let mut keys: Vec<u64> = Vec::with_capacity(schedule.blocks.len());
        let mut natural = 0u64;
        for b in &schedule.blocks {
            if b.model_group == u64::MAX {
                // A unique per-block identity: use the high bit so no two
                // natural blocks collide (each is its own model).
                keys.push(u64::MAX - b.block_index);
                natural += 1;
            } else {
                keys.push((b.model_group << 16) | (b.codec_id as u64) << 8 | b.scale_bits as u64);
            }
        }
        // Distinct grouped-model cardinality.
        let distinct: std::collections::BTreeSet<u64> =
            keys.iter().copied().filter(|k| *k != u64::MAX).collect();
        println!(
            "schedule {}: {} blocks, {} distinct grouped models, {} natural blocks",
            schedule.name,
            schedule.blocks.len(),
            distinct.len(),
            natural
        );
        println!(
            "{:<10} {:<10} {:<12} {:<12} {:<12} {:<14} {:<14}",
            "capacity", "policy", "hits", "misses", "evictions", "hit_rate", "byte_hit_rate"
        );
        for &cap in &capacities {
            for (pol, sim) in [
                ("FIFO", simulate_fifo(&keys, cap)),
                ("LRU", simulate_lru(&keys, cap)),
            ] {
                println!(
                    "{:<10} {:<10} {:<12} {:<12} {:<12} {:<14.4} {:<14.4}",
                    cap,
                    pol,
                    sim.hits,
                    sim.misses,
                    sim.evictions,
                    sim.hit_rate(),
                    sim.byte_hit_rate()
                );
            }
        }
        println!();
    }
    println!(
        "policy-sim complete — the production policy remains FIFO unless this evidence shows a material benefit for LRU (see ADR-0017)"
    );
    Ok(())
}
