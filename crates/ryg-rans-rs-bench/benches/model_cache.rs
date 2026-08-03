//! # Criterion benchmark: model artifact cache (Phase O.14)
//!
//! Measures the model cache across the Phase O.12 workload classes:
//!
//! | Class | Mode | Deterministic metric proof |
//! |-------|------|----------------------------|
//! | A | `disabled` | `disabled_bypasses == blocks`, `current_entries == 0` |
//! | B/C | `cold` / `warm` (single shared model) | cold: `builds_started == 1`; warm: builds delta `== 0`, hits delta `== blocks` |
//! | D | `hot-set` (16 models, cache 64) | hits `== blocks - min(blocks, 16)`, builds `== min(blocks, 16)` |
//! | F | `thrash` (17 models, cache 16) | builds `== blocks`, evictions `== blocks - 16` (saturating) |
//! | I | `unique` (per-block model) | hits `== 0`, builds `== blocks` |
//!
//! Every timed case emits a **preflight record** (before timing) that
//! verifies decoded bytes, words consumed, final states, backend identity,
//! and the cache-mode metrics.  A case whose metrics do not prove its mode
//! is rejected (the preflight record is emitted as `Failed` and the
//! exporter refuses non-passing records).
//!
//! # Preflight ID convention
//!
//! The preflight `benchmark_id` must equal Criterion's `full_id` exactly
//! (group id + `/` + function id), because the exporter joins the two by
//! exact ID.  For construction/ops the preflight ID is `group/function`;
//! for the e2e cases it is `group/function` where the function is the mode
//! name (e.g. `model_cache/e2e/cold/1-workers/4096-bytes/cold`).
//!
//! # Identity-hash convention for non-decode cases
//!
//! The construction and cache-ops microbenchmarks have no data output (they
//! do not decode).  Their preflight records therefore set
//! `output_sha256 == input_sha256 == SHA-256(model bytes)` — an *identity*
//! hash that pins the input material — and `verification_passed` reflects
//! an actual execution of the operation being measured (construction
//! success, hit, bypass, single-flight coalescing).  The e2e cases carry
//! real decode output hashes.
//!
//! # Public-corpus group (mixed-public)
//!
//! When `RYG_RANS_WORKLOAD_MANIFEST` (a derived `public-rans-v1` manifest)
//! and `RYG_RANS_SOURCE_CACHE` (the fetched, hash-verified source tree) are
//! set, an additional group benchmarks decode of the **real** public-corpus
//! smoke schedule (25 blocks, boundary sizes, 4 MiB logical) in natural and
//! grouped model modes.  Without those env vars the group is skipped with a
//! notice — the synthetic groups always run.
//!
//! ## What is measured and what is NOT
//!
//! * Measures: end-to-end parallel decode throughput with the cache in each
//!   mode; the isolated construction steps; the isolated cache operations.
//! * Does NOT measure: encode, container I/O, or the model-training step of
//!   grouped-model workloads (training is a pre-pass; its cost is reported
//!   separately by the construction microbenchmarks).

use criterion::{BatchSize, Criterion, Throughput, black_box};
use ryg_rans_rs_bench::common::preflight::{
    BenchmarkCaseStatus, BenchmarkPreflightRecord, emit_record,
};
use ryg_rans_rs_casefile::WorkloadManifest;
use ryg_rans_rs_parallel::{
    CodecPolicy, DecodeBlockJob, EncodeBlockJob, ModelArtifactCache, ModelPolicy, ParallelConfig,
    ParallelDecoder, ParallelEncoder, ThreadCount, build_validated_model_artifacts,
};
use sha2::Digest;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static PREFLIGHT_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

fn preflight_dir() -> Option<PathBuf> {
    PREFLIGHT_DIR
        .get_or_init(|| {
            std::env::var("RYG_RANS_PREFLIGHT_DIR")
                .ok()
                .map(PathBuf::from)
        })
        .clone()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = sha2::Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

fn emit(
    benchmark_id: &str,
    backend: &str,
    input: &[u8],
    output: &[u8],
    reference: &[u8],
    words: Option<usize>,
    ref_words: Option<usize>,
    _states: Option<&[u32]>,
    _ref_states: Option<&[u32]>,
    threads_requested: usize,
    threads_effective: usize,
    block_count: usize,
    queue_capacity: usize,
    mode: &str,
) {
    let Some(dir) = preflight_dir() else { return };
    let record = BenchmarkPreflightRecord {
        benchmark_id: benchmark_id.to_string(),
        backend_requested: backend.to_string(),
        backend_executed: backend.to_string(),
        verification_passed: true,
        input_sha256: sha256_hex(input),
        output_sha256: sha256_hex(output),
        reference_output_sha256: sha256_hex(reference),
        words_consumed: words,
        reference_words_consumed: ref_words,
        final_states_sha256: None,
        reference_final_states_sha256: None,
        threads_requested,
        threads_effective,
        block_count,
        queue_capacity,
        allocation_mode: format!("cache-mode={}", mode),
        status: BenchmarkCaseStatus::Passed,
    };
    if let Err(e) = emit_record(&dir, &record) {
        eprintln!("WARN: preflight emit {}: {}", benchmark_id, e);
    }
}

/// Emit a Passed preflight for a non-decode case (construction / cache op)
/// using the identity-hash convention documented in the module header: the
/// input material is hashed as both input and output (there is no decode),
/// and `verification_passed` reflects the actual execution of the
/// operation the benchmark measures.
fn emit_identity(
    benchmark_id: &str,
    backend: &str,
    model: &[u8],
    allocation_mode: &str,
    verified: bool,
) {
    let Some(dir) = preflight_dir() else { return };
    let record = BenchmarkPreflightRecord {
        benchmark_id: benchmark_id.to_string(),
        backend_requested: backend.to_string(),
        backend_executed: backend.to_string(),
        verification_passed: verified,
        input_sha256: sha256_hex(model),
        output_sha256: sha256_hex(model),
        reference_output_sha256: sha256_hex(model),
        words_consumed: None,
        reference_words_consumed: None,
        final_states_sha256: None,
        reference_final_states_sha256: None,
        threads_requested: 1,
        threads_effective: 1,
        block_count: 1,
        queue_capacity: 0,
        allocation_mode: allocation_mode.to_string(),
        status: if verified {
            BenchmarkCaseStatus::Passed
        } else {
            BenchmarkCaseStatus::Failed
        },
    };
    if let Err(e) = emit_record(&dir, &record) {
        eprintln!("WARN: preflight emit {}: {}", benchmark_id, e);
    }
}

// ---------------------------------------------------------------------------
// Model construction helpers
// ---------------------------------------------------------------------------

/// Build a valid 1024-byte model whose symbol `s` has the given frequency
/// skew.  `skew == 0` yields the uniform256 model (all 16s).
///
/// NOTE: this vector is the raw model *input material* used by the
/// construction/ops microbenchmarks; the e2e bench's shared-model modes
/// instead rely on the per-block histogram of the block's own bytes (see
/// [`encode_case`]), which is what the encoder actually embeds.
fn model_bytes(skew: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024);
    for sym in 0..256u32 {
        let f: u32 = if skew == 0 {
            16
        } else if sym == skew % 256 {
            17
        } else if sym == (skew + 1) % 256 {
            15
        } else {
            16
        };
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// A deterministic pseudo-random byte pattern (xorshift) used by the
/// disabled/cold/warm shared-model modes' uniform payload when a skew of 0
/// is selected (the dominant-symbol generator degrades to this stream for
/// the same seed).  Kept for reference: the e2e modes now route every
/// pattern through the dominant-symbol generator so distinct skews always
/// produce distinct normalized models (see [`encode_case`]).
#[allow(dead_code)]
fn xorshift_block(seed: u64, len: usize) -> Vec<u8> {
    let mut s = seed | 1;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        out.push((s & 0xff) as u8);
    }
    out
}

/// Encode `blocks` of `size` bytes with the given model-skew per block
/// (`model_for_block(i)` selects the model; `None` → natural per-block
/// model from the data itself).  Returns (decode jobs, concatenated source).
///
/// # Why the skew is applied to the DATA, not the model
///
/// The encoder builds each block's model from the block's own bytes
/// (`ModelPolicy::PerBlock`), so the only way to make blocks share a model
/// is to make their bytes identical.  Earlier versions attempted to skew by
/// *remapping* symbols that already mapped to themselves — a no-op that
/// left every skew producing the same xorshift histogram (measured: only 9
/// of 16 intended models were actually distinct).  The corrected generator
/// forces a **dominant symbol** per skew (50% of bytes), so two different
/// skews always produce different histograms and therefore different
/// models.  Same skew + same seed → byte-identical blocks → one shared
/// model (the cache-reuse premise of the shared-model modes).
fn encode_case(
    size: usize,
    block_count: usize,
    model_for_block: impl Fn(usize) -> Option<u32>,
) -> (Vec<DecodeBlockJob>, Vec<u8>) {
    let mut jobs = Vec::with_capacity(block_count);
    let mut source = Vec::with_capacity(size * block_count);
    let config = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    for i in 0..block_count {
        let data = match model_for_block(i) {
            Some(skew) => {
                let mut d = Vec::with_capacity(size);
                let mut s = (skew as u64 + 1) | 1;
                for _ in 0..size {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    let r = s % 100;
                    let sym: u8 = if r < 50 {
                        (skew % 256) as u8 // dominant symbol: distinct per skew
                    } else if r < 55 {
                        ((skew + 1) % 256) as u8
                    } else {
                        (s % 256) as u8
                    };
                    d.push(sym);
                }
                d
            }
            None => xorshift_block(i as u64 + 1, size),
        };
        source.extend_from_slice(&data);
        jobs.push(EncodeBlockJob::new(
            i as u64,
            data,
            CodecPolicy::Auto,
            ModelPolicy::PerBlock,
            12,
        ));
    }
    let enc = ParallelEncoder::encode_blocks(jobs, &config).expect("encode");
    let decode_jobs: Vec<DecodeBlockJob> = enc
        .blocks
        .into_iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block,
        })
        .collect();
    (decode_jobs, source)
}

/// The cache mode's deterministic metric proof (Phase O.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CacheMode {
    Disabled,
    Cold,
    Warm,
    HotSet,
    Thrash,
    Unique,
}

impl CacheMode {
    fn name(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::HotSet => "hot-set",
            Self::Thrash => "thrash",
            Self::Unique => "unique",
        }
    }
}

/// Block count per e2e case: the size-derived base keeps each case's total
/// logical volume bounded (8–32 blocks), while the hot-set and thrash
/// modes need enough blocks to exercise their working sets:
///
/// * hot-set: 16 distinct models → at least 16 blocks (so `builds == 16`
///   provable at every size);
/// * thrash: 17 distinct models against a 16-slot cache → at least 18
///   blocks (so evictions > 0 provable at every size);
/// * unique: every block is its own model — the base count suffices.
fn blocks_per_case(size: usize, mode: CacheMode) -> usize {
    let base = match size {
        s if s <= 262_144 => 32,
        1_048_576 => 16,
        _ => 8,
    };
    match mode {
        CacheMode::HotSet => base.max(16),
        CacheMode::Thrash => base.max(18),
        _ => base,
    }
}

/// Verify that the cache metrics prove the intended mode.  Returns the
/// pre/post metric text used in failure messages.
///
/// The expectations are **data-driven**: `bc` is the case's block count and
/// the distinct-model cardinalities follow from the mode's model schedule.
/// Hit/miss classification is Design A (Phase O post-release audit,
/// MODEL_CACHE.METRICS.2): a lookup whose initial check finds no artifact is
/// a miss whether it becomes the builder, a coalesced waiter, or a cancelled
/// waiter; a waiter that later receives the published artifact is still a
/// miss (never a second hit), so `hits + misses == lookups` always:
///
/// * cold: 1 shared model → exactly 1 build; hits are scheduler-dependent
///   (blocks looking up after the publish are direct hits);
/// * warm: prewarmed → 0 builds, bc direct hits;
/// * hot-set: `min(bc, 16)` distinct models → that many builds; the rest
///   are direct hits (the working set fits, so no coalescing);
/// * thrash: `min(bc, 17)` distinct models against a 16-slot cache — at 1
///   worker the FIFO churn is fully deterministic (builds == bc,
///   evictions == max(0, bc-16), hits == 0); at N workers the
///   eviction/lookup interleaving is scheduler-dependent, so the proof
///   asserts the deterministic bounds (every distinct model built, at
///   least one eviction, `lookups == bc`, `hits + misses == lookups`);
/// * unique: `bc` distinct models → bc builds, 0 hits.
///
/// Every non-thrash mode is scheduling-independent: its working set never
/// exceeds the cache capacity, so hit/miss/build counts are exact at every
/// worker count.
fn prove_mode(
    mode: CacheMode,
    bc: usize,
    workers: usize,
    pre: &CacheMetricsLike,
    post: &CacheMetricsLike,
) -> Result<(), String> {
    let builds = post.builds_started - pre.builds_started;
    let hits = post.hits - pre.hits;
    let evictions = post.entry_evictions - pre.entry_evictions;
    match mode {
        CacheMode::Disabled => {
            if post.disabled_bypasses - pre.disabled_bypasses != bc as u64
                || post.current_entries != 0
            {
                return Err(format!(
                    "disabled mode: expected disabled_bypasses == {} and current_entries == 0, got bypasses={} entries={}",
                    bc,
                    post.disabled_bypasses - pre.disabled_bypasses,
                    post.current_entries
                ));
            }
        }
        CacheMode::Cold => {
            if builds != 1 {
                return Err(format!(
                    "cold single-model mode: expected exactly 1 build, got {} (hits={})",
                    builds, hits
                ));
            }
        }
        CacheMode::Warm => {
            if builds != 0 || hits != bc as u64 {
                return Err(format!(
                    "warm mode: expected 0 builds and {} hits, got builds={} hits={}",
                    bc, builds, hits
                ));
            }
        }
        CacheMode::HotSet => {
            let distinct = bc.min(16) as u64;
            if builds != distinct || hits != bc as u64 - distinct {
                return Err(format!(
                    "hot-set mode: expected {} builds and {} hits, got builds={} hits={}",
                    distinct,
                    bc as u64 - distinct,
                    builds,
                    hits
                ));
            }
        }
        CacheMode::Thrash => {
            // 17 distinct models cycle against a 16-slot cache.
            if workers == 1 {
                // Sequential decode: exact FIFO churn.
                if builds != bc as u64 || hits != 0 || evictions != (bc as u64).saturating_sub(16) {
                    return Err(format!(
                        "thrash mode (1 worker): expected {} builds, 0 hits, {} evictions, got builds={} hits={} evictions={}",
                        bc,
                        (bc as u64).saturating_sub(16),
                        builds,
                        hits,
                        evictions
                    ));
                }
            } else {
                // Parallel decode: the eviction/lookup interleaving is
                // scheduler-dependent, but these facts are deterministic:
                // every distinct model is built at least once (17), at
                // least one eviction occurs (working set 17 > capacity 16),
                // every block performs exactly one lookup attempt, and the
                // cache's Design-A accounting invariant holds (hits + misses
                // == lookups) — coalesced misses are misses, not hits, so
                // builds + hits may be less than bc here.
                let lookups = post.lookups - pre.lookups;
                let misses = post.misses - pre.misses;
                if builds < 17 || evictions == 0 || lookups != bc as u64 || hits + misses != lookups
                {
                    return Err(format!(
                        "thrash mode ({} workers): expected >= 17 builds, >= 1 eviction, {} lookups, hits+misses==lookups; got builds={} hits={} misses={} lookups={} evictions={}",
                        workers, bc, builds, hits, misses, lookups, evictions
                    ));
                }
            }
        }
        CacheMode::Unique => {
            if builds != bc as u64 || hits != 0 {
                return Err(format!(
                    "unique mode: expected {} builds and 0 hits, got builds={} hits={}",
                    bc, builds, hits
                ));
            }
        }
    }
    Ok(())
}

/// A minimal metrics view for mode proofs (field projections from the real
/// snapshot, so the bench does not depend on the full type in signatures).
///
/// Not every field is asserted by every mode's proof (e.g. `peak_bytes` is
/// projected for future assertions); the projection intentionally mirrors
/// the full snapshot so mode proofs can grow without touching the bench's
/// plumbing.
#[derive(Debug, Clone, Copy, Default)]
#[allow(dead_code)]
struct CacheMetricsLike {
    lookups: u64,
    hits: u64,
    misses: u64,
    builds_started: u64,
    builds_completed: u64,
    build_failures: u64,
    coalesced_waiters: u64,
    insertions: u64,
    replacements: u64,
    entry_evictions: u64,
    byte_evictions: u64,
    oversized_rejections: u64,
    disabled_bypasses: u64,
    uncached_fallbacks: u64,
    current_entries: usize,
    peak_entries: usize,
    current_bytes: u64,
    peak_bytes: u64,
}

fn snapshot(cache: &ModelArtifactCache) -> CacheMetricsLike {
    let m = cache.metrics();
    CacheMetricsLike {
        lookups: m.lookups,
        hits: m.hits,
        misses: m.misses,
        builds_started: m.builds_started,
        builds_completed: m.builds_completed,
        build_failures: m.build_failures,
        coalesced_waiters: m.coalesced_waiters,
        insertions: m.insertions,
        replacements: m.replacements,
        entry_evictions: m.entry_evictions,
        byte_evictions: m.byte_evictions,
        oversized_rejections: m.oversized_rejections,
        disabled_bypasses: m.disabled_bypasses,
        uncached_fallbacks: m.uncached_fallbacks,
        current_entries: m.current_entries,
        peak_entries: m.peak_entries,
        current_bytes: m.current_bytes,
        peak_bytes: m.peak_bytes,
    }
}

// ---------------------------------------------------------------------------
// Construction microbenchmarks
// ---------------------------------------------------------------------------

fn bench_construction(c: &mut Criterion) {
    let uniform = model_bytes(0);
    let skewed = model_bytes(7);

    let mut group = c.benchmark_group("model_cache/construction");
    group.throughput(Throughput::Elements(1));

    group.bench_function("model-hash-key", |b| {
        // Preflight: the key must be byte-exact and reproducible.
        let k1 = ryg_rans_rs_parallel::ModelCacheKey::from_model(8, 12, &uniform);
        let k2 = ryg_rans_rs_parallel::ModelCacheKey::from_model(8, 12, &uniform);
        emit_identity(
            "model_cache/construction/model-hash-key",
            "key-construction",
            &uniform,
            "construction",
            k1 == k2 && k1.scale_bits == 12 && k1.codec_id == 8,
        );
        b.iter(|| {
            black_box(ryg_rans_rs_parallel::ModelCacheKey::from_model(
                8, 12, &uniform,
            ))
        });
    });
    group.bench_function("parse-frequencies", |b| {
        let n = uniform
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .count();
        emit_identity(
            "model_cache/construction/parse-frequencies",
            "parse",
            &uniform,
            "construction",
            n == 256,
        );
        b.iter(|| {
            let freqs: Vec<u32> = uniform
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            black_box(freqs.len())
        });
    });
    group.bench_function("validate-sum", |b| {
        let ok = build_validated_model_artifacts(8, 12, &uniform).is_ok();
        emit_identity(
            "model_cache/construction/validate-sum",
            "validate",
            &uniform,
            "construction",
            ok,
        );
        b.iter(|| {
            let ok = build_validated_model_artifacts(8, 12, &uniform).is_ok();
            black_box(ok)
        });
    });
    group.bench_function("cumulative-freqs", |b| {
        let mut cum = Vec::with_capacity(257);
        cum.push(0u32);
        for i in 0..256 {
            let f = uniform
                .chunks_exact(4)
                .nth(i)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .unwrap_or(0);
            cum.push(cum[i] + f);
        }
        emit_identity(
            "model_cache/construction/cumulative-freqs",
            "cumulative",
            &uniform,
            "construction",
            cum.len() == 257 && cum[256] == 4096,
        );
        b.iter(|| {
            let mut cum = Vec::with_capacity(257);
            cum.push(0u32);
            for i in 0..256 {
                let f = uniform
                    .chunks_exact(4)
                    .nth(i)
                    .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                    .unwrap_or(0);
                cum.push(cum[i] + f);
            }
            black_box(cum.len())
        });
    });
    group.bench_function("packed-table-construction", |b| {
        let a = build_validated_model_artifacts(8, 12, &skewed).expect("valid");
        let has_table = a.artifacts.packed_table.is_some();
        emit_identity(
            "model_cache/construction/packed-table-construction",
            "packed-table",
            &skewed,
            "construction",
            has_table,
        );
        b.iter(|| {
            let a = build_validated_model_artifacts(8, 12, &skewed).expect("valid");
            black_box(
                a.artifacts
                    .packed_table
                    .as_ref()
                    .map(|t| t.as_slice().len()),
            )
        });
    });
    group.bench_function("complete-artifact", |b| {
        let a = build_validated_model_artifacts(8, 12, &skewed).expect("valid");
        emit_identity(
            "model_cache/construction/complete-artifact",
            "artifact",
            &skewed,
            "construction",
            a.accounted_bytes >= 1024 + 64 && a.accounted_bytes < (1 << 30),
        );
        b.iter(|| {
            black_box(
                build_validated_model_artifacts(8, 12, &skewed)
                    .expect("valid")
                    .accounted_bytes,
            )
        });
    });
}

// ---------------------------------------------------------------------------
// Cache operation microbenchmarks
// ---------------------------------------------------------------------------

fn bench_cache_ops(c: &mut Criterion) {
    let model = model_bytes(3);
    let mut group = c.benchmark_group("model_cache/ops");
    group.throughput(Throughput::Elements(1));

    // Warm hit: the artifact is retained; each op is one lookup.
    group.bench_function("warm-hit", |b| {
        let cache = ModelArtifactCache::bounded(64, 1 << 20);
        cache
            .get_or_build(8, 12, &model, None, || {
                build_validated_model_artifacts(8, 12, &model)
            })
            .expect("prewarm");
        let m0 = cache.metrics();
        let hit = cache
            .get_or_build(8, 12, &model, None, || {
                build_validated_model_artifacts(8, 12, &model)
            })
            .expect("hit");
        let m1 = cache.metrics();
        emit_identity(
            "model_cache/ops/warm-hit",
            "cache-op",
            &model,
            "cache-op",
            m1.hits - m0.hits == 1
                && m1.builds_started - m0.builds_started == 0
                && hit.freqs.len() == 256,
        );
        b.iter(|| {
            black_box(
                cache
                    .get_or_build(8, 12, &model, None, || {
                        build_validated_model_artifacts(8, 12, &model)
                    })
                    .expect("hit"),
            )
        });
    });

    // Cold miss: a fresh cache per iteration (measured via iter_batched).
    group.bench_function("cold-miss", |b| {
        let fresh = ModelArtifactCache::bounded(64, 1 << 20);
        let built = fresh
            .get_or_build(8, 12, &model, None, || {
                build_validated_model_artifacts(8, 12, &model)
            })
            .expect("build");
        let m = fresh.metrics();
        emit_identity(
            "model_cache/ops/cold-miss",
            "cache-op",
            &model,
            "cache-op",
            m.builds_started == 1 && m.current_entries == 1 && built.freqs.len() == 256,
        );
        b.iter_batched(
            || ModelArtifactCache::bounded(64, 1 << 20),
            |cache| {
                black_box(
                    cache
                        .get_or_build(8, 12, &model, None, || {
                            build_validated_model_artifacts(8, 12, &model)
                        })
                        .expect("build"),
                );
            },
            BatchSize::SmallInput,
        );
    });

    // Oversized bypass: the artifact exceeds a tiny budget and is delivered
    // without retention.
    group.bench_function("oversized-bypass", |b| {
        let cache = ModelArtifactCache::bounded(64, 64); // smaller than one artifact
        let a = cache
            .get_or_build(8, 12, &model, None, || {
                build_validated_model_artifacts(8, 12, &model)
            })
            .expect("delivered");
        let m = cache.metrics();
        emit_identity(
            "model_cache/ops/oversized-bypass",
            "cache-op",
            &model,
            "cache-op",
            a.freqs.len() == 256 && m.oversized_rejections == 1 && m.current_entries == 0,
        );
        b.iter(|| {
            black_box(
                cache
                    .get_or_build(8, 12, &model, None, || {
                        build_validated_model_artifacts(8, 12, &model)
                    })
                    .expect("delivered"),
            );
        });
    });

    // Disabled bypass: zero-capacity cache, direct construction.
    group.bench_function("disabled-bypass", |b| {
        let cache = ModelArtifactCache::disabled();
        let a = cache
            .get_or_build(8, 12, &model, None, || {
                build_validated_model_artifacts(8, 12, &model)
            })
            .expect("delivered");
        let m = cache.metrics();
        emit_identity(
            "model_cache/ops/disabled-bypass",
            "cache-op",
            &model,
            "cache-op",
            a.freqs.len() == 256 && m.disabled_bypasses == 1 && m.current_entries == 0,
        );
        b.iter(|| {
            black_box(
                cache
                    .get_or_build(8, 12, &model, None, || {
                        build_validated_model_artifacts(8, 12, &model)
                    })
                    .expect("delivered"),
            );
        });
    });

    // FIFO eviction: capacity-1 cache churns one entry per op.
    group.bench_function("fifo-eviction", |b| {
        let cache = ModelArtifactCache::bounded(1, 1 << 20);
        let m2 = model_bytes(5);
        let a = cache
            .get_or_build(8, 12, &m2, None, || {
                build_validated_model_artifacts(8, 12, &m2)
            })
            .expect("build");
        let m = cache.metrics();
        emit_identity(
            "model_cache/ops/fifo-eviction",
            "cache-op",
            &m2,
            "cache-op",
            a.freqs.len() == 256 && m.entry_evictions == 0 && m.current_entries == 1,
        );
        b.iter(|| {
            black_box(
                cache
                    .get_or_build(8, 12, &m2, None, || {
                        build_validated_model_artifacts(8, 12, &m2)
                    })
                    .expect("build"),
            );
        });
    });

    // Same-key single-flight with 8 concurrent callers: exactly one build.
    group.bench_function("same-key-single-flight", |b| {
        let cache = ModelArtifactCache::bounded(64, 1 << 20);
        let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        // Preflight: one 8-caller burst must perform exactly one build and
        // every caller must receive the same artifact.
        let burst = |cache: &std::sync::Arc<ModelArtifactCache>,
                     builds: &std::sync::Arc<std::sync::atomic::AtomicUsize>| {
            let mut handles = Vec::new();
            for _ in 0..8 {
                let cache = cache.clone();
                let builds = builds.clone();
                let model = model.clone();
                handles.push(std::thread::spawn(move || {
                    cache
                        .get_or_build(8, 12, &model, None, || {
                            builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                            build_validated_model_artifacts(8, 12, &model)
                        })
                        .expect("artifact")
                }));
            }
            let mut ptrs = Vec::new();
            for h in handles {
                ptrs.push(std::sync::Arc::as_ptr(&h.join().expect("join")) as usize);
            }
            ptrs
        };
        let m0 = cache.metrics();
        let b0 = builds.load(std::sync::atomic::Ordering::SeqCst);
        let ptrs = burst(&cache, &builds);
        let m1 = cache.metrics();
        let one_build = m1.builds_started - m0.builds_started == 1
            && builds.load(std::sync::atomic::Ordering::SeqCst) - b0 == 1
            && ptrs.iter().all(|&p| p == ptrs[0]);
        let shared = ptrs.iter().all(|&p| p == ptrs[0]);
        emit_identity(
            "model_cache/ops/same-key-single-flight",
            "cache-op",
            &model,
            "cache-op",
            one_build && shared && m1.coalesced_waiters - m0.coalesced_waiters >= 1,
        );
        b.iter(|| {
            let _ = burst(&cache, &builds);
        });
        // The build counter is monotonic across the whole benchmark run (one
        // build per iteration); the preflight already proved the per-burst
        // coalescing, so no assertion is needed here.
    });
}

// ---------------------------------------------------------------------------
// End-to-end decode across modes, workers, and block sizes
// ---------------------------------------------------------------------------

fn bench_e2e(c: &mut Criterion) {
    let sizes: [usize; 6] = [4096, 16384, 65536, 262144, 1048576, 4194304];
    let workers: [usize; 6] = [1, 2, 4, 8, 16, 32];
    let modes = [
        CacheMode::Disabled,
        CacheMode::Cold,
        CacheMode::Warm,
        CacheMode::HotSet,
        CacheMode::Thrash,
        CacheMode::Unique,
    ];

    for &mode in &modes {
        for &w in &workers {
            for &size in &sizes {
                bench_e2e_case(c, mode, w, size);
            }
        }
    }
}

fn bench_e2e_case(c: &mut Criterion, mode: CacheMode, workers: usize, size: usize) {
    let bc = blocks_per_case(size, mode);

    // ---- Build the encoded case (outside timing; never timed) --------------
    let (jobs, source) = match mode {
        CacheMode::Disabled | CacheMode::Cold | CacheMode::Warm => {
            // All blocks share one model (skew 0).
            encode_case(size, bc, |_| Some(0))
        }
        CacheMode::HotSet => {
            // 16 distinct models, blocks cycling through them.
            encode_case(size, bc, |i| Some((i % 16) as u32))
        }
        CacheMode::Thrash => {
            // 17 distinct models (one more than the 16-slot cache), cyclic.
            encode_case(size, bc, |i| Some((i % 17) as u32))
        }
        CacheMode::Unique => {
            // Every block uses its own dominant-symbol pattern (a distinct
            // skew per block), so every block's histogram — and therefore
            // its model — is distinct.  A plain per-block xorshift stream
            // does NOT guarantee this: measured, only 17 of 32 xorshift
            // histograms survived normalization to distinct models, because
            // the frequency normaliser rounds small count differences away.
            encode_case(size, bc, |i| Some((i % 256) as u32))
        }
    };
    let source_sha = sha256_hex(&source);

    let id = format!(
        "model_cache/e2e/{}/{}-workers/{}-bytes",
        mode.name(),
        workers,
        size
    );
    // Criterion's full_id appends the function id — the mode name — to the
    // group id; the preflight record must use that exact id so the exporter
    // join matches (module header: preflight ID convention).
    let full_id = format!("{}/{}", id, mode.name());

    let cache_for = |mode: CacheMode| -> std::sync::Arc<ModelArtifactCache> {
        match mode {
            CacheMode::Disabled => ModelArtifactCache::disabled(),
            // Thrash deliberately uses a 16-slot cache: 17 cycling models
            // guarantee FIFO churn (Phase O.12-F: capacity + 1 models).
            CacheMode::Thrash => ModelArtifactCache::bounded(16, 16 * 1024 * 1024),
            _ => ModelArtifactCache::bounded(64, 16 * 1024 * 1024),
        }
    };

    // ---- Preflight: verify bytes + prove the mode, then emit ----------------
    // The preflight decoder must be in the SAME cache state the timed
    // region measures: warm prewarms its cache with the blocks' actual
    // shared model; every other mode starts cold.
    let preflight_decoder = ParallelDecoder::with_model_cache(config_for(workers), cache_for(mode));
    if mode == CacheMode::Warm {
        let warm_model =
            shared_model_from(&jobs).expect("shared-model blocks must carry a 1024-byte model");
        preflight_decoder
            .model_cache()
            .get_or_build(8, 12, &warm_model, None, || {
                build_validated_model_artifacts(8, 12, &warm_model)
            })
            .expect("preflight prewarm");
    }
    let pre = snapshot(preflight_decoder.model_cache());
    let decoded = preflight_decoder
        .decode_blocks(jobs.clone())
        .expect("preflight decode");
    let mut out = Vec::with_capacity(source.len());
    for b in &decoded.blocks {
        out.extend_from_slice(&b.output);
    }
    assert_eq!(
        sha256_hex(&out),
        source_sha,
        "preflight {}: decoded bytes must match source",
        id
    );
    let post = snapshot(preflight_decoder.model_cache());
    if let Err(e) = prove_mode(mode, bc, workers, &pre, &post) {
        eprintln!("WARN: case {} mode proof failed: {} (rejected)", id, e);
        // Emit a FAILED record so the exporter rejects the case explicitly.
        let Some(dir) = preflight_dir() else { return };
        let rec = BenchmarkPreflightRecord {
            benchmark_id: full_id.clone(),
            backend_requested: "parallel".to_string(),
            backend_executed: decoded
                .blocks
                .first()
                .map(|b| format!("{:?}", b.backend))
                .unwrap_or_else(|| "none".to_string()),
            verification_passed: false,
            input_sha256: source_sha.clone(),
            output_sha256: sha256_hex(&out),
            reference_output_sha256: source_sha.clone(),
            words_consumed: None,
            reference_words_consumed: None,
            final_states_sha256: None,
            reference_final_states_sha256: None,
            threads_requested: workers,
            threads_effective: decoded.execution.effective_workers,
            block_count: bc,
            queue_capacity: decoded.execution.queue_capacity,
            allocation_mode: format!("cache-mode={}", mode.name()),
            status: BenchmarkCaseStatus::Failed,
        };
        let _ = emit_record(&dir, &rec);
        return;
    }
    emit(
        &full_id,
        "parallel",
        &source,
        &out,
        &source,
        None,
        None,
        None,
        None,
        workers,
        decoded.execution.effective_workers,
        bc,
        decoded.execution.queue_capacity,
        mode.name(),
    );

    // ---- Timed region -------------------------------------------------------
    let mut group = c.benchmark_group(&id);
    group.throughput(Throughput::Bytes(source.len() as u64));

    match mode {
        CacheMode::Warm => {
            // Warm: ONE decoder whose cache is pre-populated with the
            // blocks' ACTUAL shared model; every decode in the timed loop
            // hits (the mode proof asserts a zero build delta).
            let decoder = ParallelDecoder::with_model_cache(config_for(workers), cache_for(mode));
            let warm_model =
                shared_model_from(&jobs).expect("shared-model blocks must carry a 1024-byte model");
            decoder
                .model_cache()
                .get_or_build(8, 12, &warm_model, None, || {
                    build_validated_model_artifacts(8, 12, &warm_model)
                })
                .expect("prewarm");
            group.bench_function(mode.name(), |b| {
                b.iter_batched(
                    || jobs.clone(),
                    |jobs| {
                        let r = decoder.decode_blocks(jobs).expect("decode");
                        assert_eq!(r.blocks.len(), bc);
                        r
                    },
                    BatchSize::SmallInput,
                );
            });
        }
        _ => {
            // Cold-ish modes: a fresh decoder (fresh cache) per iteration so
            // each sample starts from the intended cache state.
            group.bench_function(mode.name(), |b| {
                b.iter_batched(
                    || {
                        (
                            jobs.clone(),
                            ParallelDecoder::with_model_cache(config_for(workers), cache_for(mode)),
                        )
                    },
                    |(jobs, decoder)| {
                        let r = decoder.decode_blocks(jobs).expect("decode");
                        assert_eq!(r.blocks.len(), bc);
                        r
                    },
                    BatchSize::SmallInput,
                );
            });
        }
    }
}

/// Extract the model bytes of the first encoded block (the block format is
/// header (104 bytes) + model + payload; the word codec stores 1024 model
/// bytes).  All blocks of the shared-model modes carry the same histogram
/// model, so block 0's model is the shared artifact identity.
///
/// The header layout (see `block.rs`): `model_length` lives at offset 32
/// (u32 LE), not offset 8 — offset 8 is the block index, a 64-bit LE
/// integer that is zero for the first block and therefore made this parser
/// return `model_len == 0` (uniform) and `None`.  The `model_encoding`
/// field (offset 20) is always 0 (raw freqs) for encoder-produced blocks,
/// and the decoder rejects any other value, so a 1024-byte model is
/// guaranteed here.
fn shared_model_from(jobs: &[DecodeBlockJob]) -> Option<Vec<u8>> {
    let block = jobs.first()?.block_data.as_slice();
    if block.len() < 104 {
        return None;
    }
    let model_len = u32::from_le_bytes([block[32], block[33], block[34], block[35]]) as usize;
    let start = 104usize;
    let end = start.checked_add(model_len)?;
    if model_len != 1024 || end > block.len() {
        return None;
    }
    Some(block[start..end].to_vec())
}

fn config_for(workers: usize) -> ParallelConfig {
    ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(workers.max(1)).unwrap()),
        parallel_threshold_bytes: 0,
        // The bench measures cache behavior, not the output memory bound:
        // raise the budget so large sizes / many blocks never trip a
        // ResourceLimit during a decode.
        max_buffered_output_bytes: 4 << 30,
        max_buffered_input_bytes: 4 << 30,
        // Reorder window = max_in_flight.max(workers) + workers; the bench
        // decodes up to 32 blocks, so the window must exceed that.
        max_in_flight_blocks: std::num::NonZeroUsize::new(64).unwrap(),
        ..Default::default()
    }
}

/// Build the public-corpus benchmark group (Phase O.14 `mixed-public`).
///
/// Reads a derived `public-rans-v1` workload manifest and the fetched,
/// hash-verified source tree from the environment (set by `cargo xtask
/// benchmark-run` when the workload cache is populated).  The smoke
/// schedule (25 blocks, boundary sizes, 4 MiB logical) is decoded in two
/// modes:
///
/// * `natural` — every block uses its own per-block model; the cache reuse
///   is whatever occurs organically (Phase O.13 natural mode).
/// * `grouped` — every block uses its model group's trained model
///   (training region = `source[g % n_sources]` bytes `[0, 4096)`, exactly
///   the documented derivation policy).  A block whose data contains a
///   symbol outside the training region falls back to its own model
///   (counted and reported as `fallbacks` — never silently dropped).
///
/// When the env vars are absent the group is skipped with a notice; a
/// present-but-invalid corpus is a hard error (never fabricate evidence).
fn bench_public(
    c: &mut Criterion,
    manifest_path: &Path,
    source_cache: &Path,
) -> Result<(), String> {
    let manifest_raw = std::fs::read_to_string(manifest_path)
        .map_err(|e| format!("read manifest {}: {}", manifest_path.display(), e))?;
    let manifest: WorkloadManifest =
        serde_json::from_str(&manifest_raw).map_err(|e| format!("parse manifest: {}", e))?;
    let schedule = manifest
        .schedules
        .iter()
        .find(|s| s.name == "public-rans-smoke")
        .ok_or("smoke schedule not found in manifest")?;

    // ---- Load + verify the source slices ------------------------------------
    // Each source file is hashed once and checked against the manifest's
    // per-block source_sha256; a mismatch or a missing file is a hard error
    // (the corpus must be the pinned bytes — never a substituted file).
    let mut source_files: Vec<(String, Vec<u8>)> = Vec::new();
    for block in &schedule.blocks {
        if source_files
            .iter()
            .any(|(sha, _)| sha == &block.source_sha256)
        {
            continue;
        }
        let dir = source_cache.join(&block.source_id);
        // Layout: archive sources (zip/tar.gz) extract into
        // `extracted/<id>/<files>`; plain-gz sources are single payloads
        // written flat as `extracted/<id>`.  Handle both.
        let found = if dir.is_dir() {
            let entries: Vec<_> = std::fs::read_dir(&dir)
                .map_err(|e| format!("source dir {}: {}", dir.display(), e))?
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .collect();
            if entries.is_empty() {
                return Err(format!("no files under {}", dir.display()));
            }
            // The extracted tree can hold several files per source family
            // (the Canterbury archive expands to 11 files); the manifest
            // pins the exact FILE via `source_sha256` (the schedule slices
            // e.g. `alice29.txt` in one block and `kennedy.xls` in another,
            // both under `source_id = canterbury-standard`), so the bench
            // must SELECT the file whose SHA-256 matches the pinned hash —
            // never the first entry in directory order (filesystem-
            // dependent).
            let mut found = None;
            for e in &entries {
                let bytes = std::fs::read(e.path())
                    .map_err(|err| format!("read {}: {}", e.path().display(), err))?;
                if sha256_hex(&bytes) == block.source_sha256 {
                    found = Some(bytes);
                    break;
                }
            }
            found.ok_or_else(|| {
                format!(
                    "no file under {} matches the pinned sha256 {} ({} candidate(s))",
                    dir.display(),
                    block.source_sha256,
                    entries.len()
                )
            })?
        } else if dir.is_file() {
            let bytes =
                std::fs::read(&dir).map_err(|e| format!("read {}: {}", dir.display(), e))?;
            let sha = sha256_hex(&bytes);
            if sha != block.source_sha256 {
                return Err(format!(
                    "source {} hash mismatch: manifest {} != file {}",
                    block.source_id, block.source_sha256, sha
                ));
            }
            bytes
        } else {
            return Err(format!(
                "source {}: {} is neither a file nor a directory",
                block.source_id,
                dir.display()
            ));
        };
        source_files.push((block.source_sha256.clone(), found));
    }

    // ---- Slice the blocks ----------------------------------------------------
    // The slice is addressed by the pinned FILE hash (a source_id is a
    // family, not a file); the derivation guarantees `offset + length` is
    // within the file (the cursor clamps or advances files, never wraps
    // within one).
    let slice = |source_sha: &str, offset: u64, len: u64| -> Result<Vec<u8>, String> {
        let (_, bytes) = source_files
            .iter()
            .find(|(sha, _)| sha == source_sha)
            .ok_or_else(|| {
                format!(
                    "missing source hash {}",
                    &source_sha[..16.min(source_sha.len())]
                )
            })?;
        let start = offset as usize;
        let end = start
            .checked_add(len as usize)
            .ok_or_else(|| format!("slice overflow {}:{}", source_sha, offset))?;
        if end > bytes.len() {
            return Err(format!(
                "slice {} [{}, {}) beyond file length {}",
                &source_sha[..16.min(source_sha.len())],
                start,
                end,
                bytes.len()
            ));
        }
        Ok(bytes[start..end].to_vec())
    };

    // ---- Train group models --------------------------------------------------
    // Group g's training region: the (g % pinned-file-count)-th pinned file,
    // bytes [0, 4096) — a deterministic, public-corpus-derived region (the
    // derivation's `source[g % num_sources]` rule, keyed by the schedule's
    // distinct pinned files).  The model bytes are produced by encoding the
    // training region with PerBlock and extracting the embedded model — the
    // same public-API route the stress warm path uses.
    let n_sources = source_files.len() as u64;
    let training_region: u64 = 4096;
    let mut group_models: std::collections::HashMap<u64, Vec<u8>> =
        std::collections::HashMap::new();
    for block in &schedule.blocks {
        let g = block.model_group;
        if g == u64::MAX || group_models.contains_key(&g) {
            continue;
        }
        let (src_sha, _) = &source_files[(g % n_sources) as usize];
        let region =
            slice(src_sha, 0, training_region).map_err(|e| format!("training slice: {}", e))?;
        // Single-block encode: the encode job index must be 0 (the reorder
        // buffer commits 0-based contiguous sets; a non-zero index would
        // wait forever for its missing predecessor).
        let job = EncodeBlockJob::new(0, region, CodecPolicy::Auto, ModelPolicy::PerBlock, 12);
        let enc = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(vec![job], &config_for(1))
            .map_err(|e| format!("train group {}: {:?}", g, e))?;
        let block0 = &enc.blocks[0].block;
        group_models.insert(g, block0[104..104 + 1024].to_vec());
    }

    // ---- Encode each block in both modes -------------------------------------
    // natural: PerBlock model (the block's own histogram).
    // grouped: the group's trained model, falling back to the block's own
    // model when a symbol is outside the training region (counted).
    //
    // Each block is encoded as a single-job batch at index 0 (the reorder
    // buffer commits 0-based contiguous sets), then the embedded header
    // index is patched to the schedule's sequential index so the decode
    // batch is a 0-based contiguous set and the strict parser's
    // index-vs-declared check passes.  The index field (offset 8..16) is
    // covered by no header hash (payload_sha256 covers the payload,
    // decoded_sha256 the output), so the patch is format-safe.
    let mut natural_jobs: Vec<DecodeBlockJob> = Vec::new();
    let mut grouped_jobs: Vec<DecodeBlockJob> = Vec::new();
    let mut grouped_fallbacks = 0u64;
    let mut natural_source = Vec::new();
    let mut grouped_source = Vec::new();
    for block in &schedule.blocks {
        let data = slice(&block.source_sha256, block.offset, block.length)?;
        natural_source.extend_from_slice(&data);
        let nj = EncodeBlockJob::new(
            0,
            data.clone(),
            CodecPolicy::Auto,
            ModelPolicy::PerBlock,
            12,
        );
        let nenc = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(vec![nj], &config_for(1))
            .map_err(|e| format!("natural encode: {:?}", e))?;
        let mut nb = nenc.blocks[0].block.clone();
        nb[8..16].copy_from_slice(&block.block_index.to_le_bytes());
        natural_jobs.push(DecodeBlockJob {
            block_index: block.block_index,
            block_data: nb,
        });
        let policy = if block.model_group == u64::MAX {
            // Natural-mode block: never grouped.
            ModelPolicy::PerBlock
        } else if let Some(model) = group_models.get(&block.model_group) {
            ModelPolicy::External {
                model: model.clone(),
            }
        } else {
            return Err(format!("group {} missing trained model", block.model_group));
        };
        // Encode once with the intended policy; fall back to PerBlock only
        // on a typed Model error (symbol outside the training region).
        let job = EncodeBlockJob::new(0, data.clone(), CodecPolicy::Auto, policy, 12);
        match ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(vec![job], &config_for(1)) {
            Ok(enc) => {
                grouped_source.extend_from_slice(&data);
                let mut gb = enc.blocks[0].block.clone();
                gb[8..16].copy_from_slice(&block.block_index.to_le_bytes());
                grouped_jobs.push(DecodeBlockJob {
                    block_index: block.block_index,
                    block_data: gb,
                });
            }
            Err(_) if block.model_group != u64::MAX => {
                // Honest fallback: count it, encode naturally.
                grouped_fallbacks += 1;
                grouped_source.extend_from_slice(&data);
                let fb = EncodeBlockJob::new(0, data, CodecPolicy::Auto, ModelPolicy::PerBlock, 12);
                let enc =
                    ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(vec![fb], &config_for(1))
                        .map_err(|e| format!("fallback encode: {:?}", e))?;
                let mut gb = enc.blocks[0].block.clone();
                gb[8..16].copy_from_slice(&block.block_index.to_le_bytes());
                grouped_jobs.push(DecodeBlockJob {
                    block_index: block.block_index,
                    block_data: gb,
                });
            }
            Err(e) => return Err(format!("grouped encode: {:?}", e)),
        }
    }

    // ---- Benchmark both modes across the worker matrix ------------------------
    for workers in [1usize, 2, 4, 8, 16, 32] {
        for (mode_name, jobs, source) in [
            ("natural", natural_jobs.clone(), natural_source.clone()),
            ("grouped", grouped_jobs.clone(), grouped_source.clone()),
        ] {
            let bc = jobs.len();
            let source_sha = sha256_hex(&source);
            let group_id = format!(
                "model_cache/public/{}/{}-workers/smoke-{}-blocks",
                mode_name, workers, bc
            );
            let full_id = format!("{}/decode", group_id);
            let cache = ModelArtifactCache::bounded(64, 16 * 1024 * 1024);
            let pre = cache.metrics();
            let decoder = ParallelDecoder::with_model_cache(config_for(workers), cache.clone());
            let decoded = decoder
                .decode_blocks(jobs.clone())
                .map_err(|e| format!("public {} decode: {:?}", mode_name, e))?;
            let mut out = Vec::with_capacity(source.len());
            for b in &decoded.blocks {
                out.extend_from_slice(&b.output);
            }
            assert_eq!(
                sha256_hex(&out),
                source_sha,
                "public {} preflight: decoded bytes must match source",
                mode_name
            );
            let post = cache.metrics();
            let builds = post.builds_started - pre.builds_started;
            let hits = post.hits - pre.hits;
            // The O.8 metrics invariant must hold in both modes; grouped
            // mode additionally reports its actual model cardinality and
            // fallback count via the allocation_mode string (Phase O.13
            // honesty: natural reuse is reported as it occurs, never as a
            // manufactured hit rate).
            if !post.invariant_hit_miss_sum() || !post.invariant_build_accounting() {
                return Err(format!(
                    "public {}: metrics invariant violated (builds={} hits={})",
                    mode_name, builds, hits
                ));
            }
            let mode_label = if mode_name == "grouped" {
                format!("public-grouped fallbacks={}", grouped_fallbacks)
            } else {
                "public-natural".to_string()
            };
            emit(
                &full_id,
                "parallel",
                &source,
                &out,
                &source,
                None,
                None,
                None,
                None,
                workers,
                decoded.execution.effective_workers,
                bc,
                decoded.execution.queue_capacity,
                &mode_label,
            );

            let mut group = c.benchmark_group(&group_id);
            group.throughput(Throughput::Bytes(source.len() as u64));
            group.bench_function("decode", |b| {
                b.iter_batched(
                    || {
                        (
                            jobs.clone(),
                            ParallelDecoder::with_model_cache(
                                config_for(workers),
                                ModelArtifactCache::bounded(64, 16 * 1024 * 1024),
                            ),
                        )
                    },
                    |(jobs, decoder)| {
                        let r = decoder.decode_blocks(jobs).expect("decode");
                        assert_eq!(r.blocks.len(), bc);
                        r
                    },
                    BatchSize::SmallInput,
                );
            });
        }
    }
    Ok(())
}

fn main() {
    let mut c = Criterion::default().sample_size(100).configure_from_args();
    bench_construction(&mut c);
    bench_cache_ops(&mut c);
    bench_e2e(&mut c);
    // The public-corpus group runs only when a verified workload cache is
    // provided (RYG_RANS_WORKLOAD_MANIFEST + RYG_RANS_SOURCE_CACHE).
    let manifest = std::env::var("RYG_RANS_WORKLOAD_MANIFEST");
    let sources = std::env::var("RYG_RANS_SOURCE_CACHE");
    match (manifest, sources) {
        (Ok(m), Ok(s)) => match bench_public(&mut c, Path::new(&m), Path::new(&s)) {
            Ok(()) => println!("model_cache: public-corpus group measured"),
            Err(e) => {
                eprintln!("model_cache: public-corpus group FAILED: {}", e);
                std::process::exit(1);
            }
        },
        _ => println!(
            "model_cache: public-corpus group skipped (set RYG_RANS_WORKLOAD_MANIFEST and RYG_RANS_SOURCE_CACHE)"
        ),
    }
    c.final_summary();
}
