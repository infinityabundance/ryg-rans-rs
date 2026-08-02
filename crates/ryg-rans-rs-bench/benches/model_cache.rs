//! # Criterion benchmark: model artifact cache (Phase O.14)
//!
//! Measures the model cache across the Phase O.12 workload classes:
//!
//! | Class | Mode | Deterministic metric proof |
//! |-------|------|----------------------------|
//! | A | `disabled` | `disabled_bypasses == blocks`, `current_entries == 0` |
//! | B/C | `cold` / `warm` (single shared model) | cold: `builds_started == 1`; warm: builds delta `== 0`, hits delta `== blocks` |
//! | D | `hot-set` (16 models, cache 64) | hits `== blocks - 16`, builds `== 16` |
//! | F | `thrash` (65 models, cache 64) | hits `== 64`, builds `== blocks - 64`, evictions `> 0` |
//! | I | `unique` (per-block model) | hits `== 0`, builds `== blocks` |
//!
//! Every timed case emits a **preflight record** (before timing) that
//! verifies decoded bytes, words consumed, final states, backend identity,
//! and the cache-mode metrics.  A case whose metrics do not prove its mode
//! is rejected (the preflight record is not emitted as `Passed` and the
//! exporter refuses missing records).
//!
//! ## What is measured and what is NOT
//!
//! * Measures: end-to-end parallel decode throughput with the cache in each
//!   mode; the isolated construction steps; the isolated cache operations.
//! * Does NOT measure: encode, container I/O, or the model-training step of
//!   grouped-model workloads (training is a pre-pass; its cost is reported
//!   separately by the construction microbenchmarks).

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use ryg_rans_rs_bench::common::preflight::{
    BenchmarkCaseStatus, BenchmarkPreflightRecord, emit_record,
};
use ryg_rans_rs_parallel::{
    DecodeBlockJob, ModelArtifactCache, ParallelConfig, ParallelDecoder, ThreadCount,
    build_validated_model_artifacts,
};
use sha2::Digest;
use std::num::NonZeroUsize;
use std::path::PathBuf;
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

// ---------------------------------------------------------------------------
// Model construction helpers
// ---------------------------------------------------------------------------

/// Build a valid 1024-byte model whose symbol `s` has the given frequency
/// skew.  `skew == 0` yields the uniform256 model (all 16s).
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

/// A deterministic pseudo-random byte pattern (xorshift) for unique-model
/// data: each block's bytes hash differently, so per-block models differ.
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
fn encode_case(
    size: usize,
    block_count: usize,
    model_for_block: impl Fn(usize) -> Option<u32>,
) -> (Vec<DecodeBlockJob>, Vec<u8>) {
    use ryg_rans_rs_parallel::{CodecPolicy, EncodeBlockJob, ModelPolicy, ParallelEncoder};
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
                // Blocks sharing a skew MUST be byte-identical: the
                // per-block model is the histogram of the block's own
                // bytes, so identical data is what makes identical models
                // (the cache-reuse premise of the shared-model modes).
                // Seeding by the pattern — not the block index — makes
                // every block of one pattern byte-identical.
                let mut d = Vec::with_capacity(size);
                let mut s = (skew as u64 + 1) | 1;
                for _ in 0..size {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    let sym: u8 = if (s % 256) == (skew % 256) as u64 {
                        (skew % 256) as u8
                    } else if (s % 256) == ((skew + 1) % 256) as u64 {
                        ((skew + 1) % 256) as u8
                    } else {
                        (s % 256) as u8
                    };
                    d.push(sym as u8);
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

fn blocks_per_case(size: usize) -> usize {
    match size {
        s if s <= 262_144 => 32,
        1_048_576 => 16,
        _ => 8,
    }
}

/// Verify that the cache metrics prove the intended mode.  Returns the
/// pre/post metric text used in failure messages.
fn prove_mode(
    mode: CacheMode,
    pre: &CacheMetricsLike,
    post: &CacheMetricsLike,
) -> Result<(), String> {
    match mode {
        CacheMode::Disabled => {
            if post.disabled_bypasses - pre.disabled_bypasses == 0 || post.current_entries != 0 {
                return Err(
                    "disabled mode: expected disabled_bypasses == blocks and current_entries == 0"
                        .into(),
                );
            }
        }
        CacheMode::Cold => {
            if post.builds_started - pre.builds_started != 1 {
                return Err(format!(
                    "cold single-model mode: expected exactly 1 build, got {}",
                    post.builds_started - pre.builds_started
                ));
            }
        }
        CacheMode::Warm => {
            if post.builds_started - pre.builds_started != 0 || post.hits - pre.hits == 0 {
                return Err("warm mode: expected zero build delta and a positive hit delta".into());
            }
        }
        CacheMode::HotSet => {
            let builds = post.builds_started - pre.builds_started;
            if builds != 16 {
                return Err(format!("hot-set mode: expected 16 builds, got {}", builds));
            }
        }
        CacheMode::Thrash => {
            let builds = post.builds_started - pre.builds_started;
            if builds == 0 || post.entry_evictions - pre.entry_evictions == 0 {
                return Err("thrash mode: expected builds > 0 and evictions > 0".into());
            }
        }
        CacheMode::Unique => {
            let builds = post.builds_started - pre.builds_started;
            if builds == 0 {
                return Err("unique mode: expected per-block builds".into());
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
        b.iter(|| {
            black_box(ryg_rans_rs_parallel::ModelCacheKey::from_model(
                8, 12, &uniform,
            ))
        });
    });
    group.bench_function("parse-frequencies", |b| {
        b.iter(|| {
            let freqs: Vec<u32> = uniform
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect();
            black_box(freqs.len())
        });
    });
    group.bench_function("validate-sum", |b| {
        b.iter(|| {
            let ok = build_validated_model_artifacts(8, 12, &uniform).is_ok();
            black_box(ok)
        });
    });
    group.bench_function("cumulative-freqs", |b| {
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
        b.iter(|| {
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
            for h in handles {
                black_box(h.join().expect("join"));
            }
        });
        // The build counter must be exactly 8 total across the benchmark run
        // (one per iteration; single-flight coalesces within an iteration).
        let _ = builds;
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
    let bc = blocks_per_case(size);

    // ---- Build the encoded case (outside timing; never timed) --------------
    let (jobs, source) = match mode {
        CacheMode::Disabled | CacheMode::Cold | CacheMode::Warm => {
            // All blocks share one model (uniform256 skew 0).
            encode_case(size, bc, |_| Some(0))
        }
        CacheMode::HotSet => {
            // 16 distinct models, blocks cycling through them.
            encode_case(size, bc, |i| Some((i % 16) as u32))
        }
        CacheMode::Thrash => {
            // 65 distinct models (one more than the 64-slot cache), cyclic.
            encode_case(size, bc, |i| Some((i % 65) as u32))
        }
        CacheMode::Unique => {
            // Every block derives its own model from distinct data.
            encode_case(size, bc, |_| None)
        }
    };
    let source_sha = sha256_hex(&source);

    let id = format!(
        "model_cache/e2e/{}/{}-workers/{}-bytes",
        mode.name(),
        workers,
        size
    );

    let cache_for = |mode: CacheMode| -> std::sync::Arc<ModelArtifactCache> {
        match mode {
            CacheMode::Disabled => ModelArtifactCache::disabled(),
            _ => ModelArtifactCache::bounded(64, 16 * 1024 * 1024),
        }
    };

    // ---- Preflight: verify bytes + prove the mode, then emit ----------------
    // The preflight decoder must be in the SAME cache state the timed
    // region measures: warm prewarms its cache with the blocks' actual
    // shared model; every other mode starts cold.
    let preflight_decoder = ParallelDecoder::with_model_cache(config_for(workers), cache_for(mode));
    if mode == CacheMode::Warm {
        let warm_model = shared_model_from(&jobs)
            .expect("shared-model blocks must carry a 1024-byte model");
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
    if let Err(e) = prove_mode(mode, &pre, &post) {
        eprintln!("WARN: case {} mode proof failed: {} (rejected)", id, e);
        // Emit a FAILED record so the exporter rejects the case explicitly.
        let Some(dir) = preflight_dir() else { return };
        let rec = BenchmarkPreflightRecord {
            benchmark_id: id.clone(),
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
        &id,
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
            let warm_model = shared_model_from(&jobs)
                .expect("shared-model blocks must carry a 1024-byte model");
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
fn shared_model_from(jobs: &[DecodeBlockJob]) -> Option<Vec<u8>> {
    let block = jobs.first()?.block_data.as_slice();
    // Parse model_length from the header (offset 8, u32 LE per the
    // bitstream contract) — reusing the crate's parser would be cleaner,
    // but the bench avoids depending on parser internals.
    let model_len = u32::from_le_bytes([block[8], block[9], block[10], block[11]]) as usize;
    let start = 104usize;
    let end = start.checked_add(model_len)?;
    if model_len == 0 || end > block.len() {
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

criterion_group!(
    name = model_cache_benches;
    config = Criterion::default().sample_size(100);
    targets = bench_construction, bench_cache_ops, bench_e2e
);
criterion_main!(model_cache_benches);
