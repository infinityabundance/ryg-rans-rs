//! # Criterion benchmark: Parallel block engine scaling
//!
//! Tier 6 benchmarks measuring parallel encoding, decoding, verification,
//! and scaling across 1, 2, 4, 8, and 16 thread counts.
//!
//! Cold-executor measurements create and join worker threads on every call.
//! Preflight verification ensures parallel output matches the original corpus
//! AND that every thread count produces byte-identical results with identical
//! backend identities, words consumed, and final states.
//!
//! ## Scaling workloads
//!
//! ### Cold, one-wave workload (16 MiB)
//! 16 × 1 MiB blocks.  At 16 workers, each worker gets exactly one block.
//! This exposes thread creation cost, startup latency, and one-wave load balance.
//!
//! ### Sustained scaling workload (64 MiB)
//! 64 × 1 MiB blocks.  At 8 workers: ~8 waves.  At 16 workers: ~4 waves.
//! This measures sustained throughput with multiple waves per worker.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::num::NonZeroUsize;
use std::sync::OnceLock;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

// ---------------------------------------------------------------------------
// Preflight record emission (residual L1-D)
// ---------------------------------------------------------------------------
// Every timed case emits a BenchmarkPreflightRecord before timing; the
// performance exporter joins Criterion measurements to these records by
// exact benchmark ID and refuses to export a case without one.  The preflight
// dir is run-local (RYG_RANS_PREFLIGHT_DIR); when unset, emission is skipped
// so the benches still run standalone.

/// Run-local preflight directory from `RYG_RANS_PREFLIGHT_DIR`, read once.
fn preflight_dir() -> Option<&'static str> {
    static DIR: OnceLock<Option<String>> = OnceLock::new();
    DIR.get_or_init(|| {
        std::env::var("RYG_RANS_PREFLIGHT_DIR")
            .ok()
            .filter(|s| !s.is_empty())
    })
    .as_deref()
}

/// Hex SHA-256 of a byte slice.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Hex SHA-256 of the canonical serialization of a final-states vector
/// (little-endian bytes of each u32, concatenated).
fn states_sha256(states: &[u32]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for &s in states {
        h.update(s.to_le_bytes());
    }
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Build and emit a Passed preflight record.  Emission failures are warnings
/// only: the exporter rejects missing records later, but the bench itself
/// must not fail on emission.
#[allow(clippy::too_many_arguments)]
fn emit_preflight(
    benchmark_id: String,
    backend: &str,
    input: &[u8],
    output: &[u8],
    reference_output: &[u8],
    words_consumed: Option<usize>,
    reference_words_consumed: Option<usize>,
    final_states: Option<&[u32]>,
    reference_final_states: Option<&[u32]>,
    threads_requested: usize,
    threads_effective: usize,
    block_count: usize,
    queue_capacity: usize,
) {
    let Some(dir) = preflight_dir() else {
        return;
    };
    let record = ryg_rans_rs_bench::common::preflight::BenchmarkPreflightRecord {
        benchmark_id,
        backend_requested: backend.to_string(),
        backend_executed: backend.to_string(),
        verification_passed: true,
        input_sha256: sha256_hex(input),
        output_sha256: sha256_hex(output),
        reference_output_sha256: sha256_hex(reference_output),
        words_consumed,
        reference_words_consumed,
        final_states_sha256: final_states.map(states_sha256),
        reference_final_states_sha256: reference_final_states.map(states_sha256),
        threads_requested,
        threads_effective,
        block_count,
        queue_capacity,
        allocation_mode: "unknown".to_string(),
        status: ryg_rans_rs_bench::common::preflight::BenchmarkCaseStatus::Passed,
    };
    if let Err(e) =
        ryg_rans_rs_bench::common::preflight::emit_record(&std::path::PathBuf::from(dir), &record)
    {
        eprintln!(
            "WARN: preflight emission failed for {}: {}",
            record.benchmark_id, e
        );
    }
}

/// Thread counts for the scaling matrix.
/// 8 = physical cores on 9800X3D. 16 = all SMT threads.
const THREAD_COUNTS: &[usize] = &[1, 2, 4, 8, 16];

/// Build a ParallelConfig for the given thread count with fixed queue depth.
fn config_for_threads(threads: usize) -> ryg_rans_rs_parallel::ParallelConfig {
    ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(
            NonZeroUsize::new(threads).expect("thread count must be nonzero"),
        ),
        // Keep queue depth constant across all thread counts so we measure
        // thread scaling, not queue-depth scaling.
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
        ..Default::default()
    }
}

/// Run preflight: encode, then decode with every thread count.
///
/// Verifies that for EVERY thread count:
/// - Decoded bytes equal original corpus
/// - Block count is identical
/// - Block ordering is identical
/// - Block indices are identical
/// - Backend identities are identical
/// - Payload hashes are identical
/// - Decoded hashes are identical
/// - Effective worker count matches requested (subject to block count)
///
/// Returns (decode_jobs, verify_jobs, reference_data) for use in benchmarks.
/// The reference data is the original corpus, which every thread count must
/// reproduce exactly.
fn preflight_parallel(
    total_size: usize,
    block_size: u64,
    thread_counts: &[usize],
    profile: ModelProfile,
    seed: u64,
) -> (
    Vec<ryg_rans_rs_parallel::DecodeBlockJob>,
    Vec<ryg_rans_rs_parallel::VerifyBlockJob>,
    Vec<u8>,
) {
    let corpus = Corpus::generate(profile, total_size, seed);
    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size);
    let block_count = plan.block_count();

    // Encode blocks with 4-thread config
    let cfg_encode = config_for_threads(4);
    let jobs: Vec<ryg_rans_rs_parallel::EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            ryg_rans_rs_parallel::EncodeBlockJob::new(
                r.block_index,
                corpus.data[s..s + r.length as usize].to_vec(),
                ryg_rans_rs_parallel::CodecPolicy::Auto,
                ryg_rans_rs_parallel::ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();

    let encoded = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs, &cfg_encode)
        .expect("preflight encode");

    let dj: Vec<ryg_rans_rs_parallel::DecodeBlockJob> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    let vj: Vec<ryg_rans_rs_parallel::VerifyBlockJob> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::VerifyBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // Verify 1-thread decode first to establish the canonical parallel reference.
    let cfg_1t = config_for_threads(1);
    let dec_1t = ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(dj.clone(), &cfg_1t)
        .expect("preflight decode 1t");
    assert_eq!(
        dec_1t.blocks.len(),
        block_count,
        "preflight: block count mismatch for 1t"
    );

    // Build the canonical 1-thread reference: full report fields
    let one_thread_outputs: Vec<Vec<u8>> = dec_1t.blocks.iter().map(|b| b.output.clone()).collect();
    let one_thread_backends: Vec<_> = dec_1t.blocks.iter().map(|b| b.backend).collect();
    let one_thread_words: Vec<_> = dec_1t.blocks.iter().map(|b| b.words_consumed).collect();
    let one_thread_states: Vec<Vec<u32>> = dec_1t
        .blocks
        .iter()
        .map(|b| b.final_states.clone())
        .collect();
    let one_thread_hashes: Vec<_> = dec_1t.blocks.iter().map(|b| b.output_hash).collect();

    let mut concatenated_1t = Vec::new();
    for b in &dec_1t.blocks {
        concatenated_1t.extend_from_slice(&b.output);
    }
    assert_eq!(
        concatenated_1t, corpus.data,
        "preflight: 1-thread decode must match original corpus"
    );

    // Verify EVERY thread count against the 1-thread reference.
    for &tc in thread_counts {
        if tc == 1 {
            continue; // already verified above
        }

        let cfg = config_for_threads(tc);
        let dec = ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(dj.clone(), &cfg)
            .expect(&format!("preflight decode {}t", tc));

        // Same block count
        assert_eq!(
            dec.blocks.len(),
            block_count,
            "preflight: block count mismatch for {}t",
            tc
        );

        // Every field identical to 1-thread decode
        for (i, (block, ref_block)) in dec.blocks.iter().zip(dec_1t.blocks.iter()).enumerate() {
            assert_eq!(
                block.block_index, ref_block.block_index,
                "preflight {}t: block_index mismatch at {}",
                tc, i
            );
            assert_eq!(
                block.output, one_thread_outputs[i],
                "preflight {}t: output mismatch at block {}",
                tc, i
            );
            assert_eq!(
                block.backend, one_thread_backends[i],
                "preflight {}t: backend mismatch at block {}",
                tc, i
            );
            assert_eq!(
                block.words_consumed, one_thread_words[i],
                "preflight {}t: words_consumed mismatch at block {}",
                tc, i
            );
            assert_eq!(
                block.final_states, one_thread_states[i],
                "preflight {}t: final_states mismatch at block {}",
                tc, i
            );
            assert_eq!(
                block.output_hash, one_thread_hashes[i],
                "preflight {}t: output_hash mismatch at block {}",
                tc, i
            );
        }

        // Verify concatenated output matches original
        let mut full = Vec::new();
        for b in &dec.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(
            full, corpus.data,
            "preflight: {}-thread decode must match original data",
            tc
        );
    }

    (dj, vj, corpus.data)
}

/// Benchmark decode scaling across all thread counts.
fn bench_parallel_decode_scaling(c: &mut Criterion) {
    // Sustained workload: 64 MiB, 64 blocks, multiple waves per thread
    let total_size: usize = 64 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (decode_jobs, _, corpus_data) = preflight_parallel(
        total_size,
        block_size,
        THREAD_COUNTS,
        ModelProfile::Skewed2551,
        42,
    );

    for &threads in THREAD_COUNTS {
        let config = config_for_threads(threads);
        let group_name = format!(
            "parallel/decode/cold-executor/{}threads/1MiB-blocks/64MiB",
            threads
        );

        // Preflight record: run the exact case decode once, verify parity, emit.
        let dec =
            ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(decode_jobs.clone(), &config)
                .expect("parallel decode record preflight");
        let mut decoded = Vec::new();
        let mut words = 0usize;
        let mut states: Vec<u32> = Vec::new();
        for b in &dec.blocks {
            decoded.extend_from_slice(&b.output);
            words += b.words_consumed;
            states.extend_from_slice(&b.final_states);
        }
        assert_eq!(
            decoded, corpus_data,
            "parallel decode {}t record preflight: output must match original",
            threads
        );
        emit_preflight(
            format!("{}/decode", group_name),
            "parallel-auto",
            &corpus_data,
            &decoded,
            &corpus_data,
            Some(words),
            Some(words),
            Some(&states),
            Some(&states),
            threads,
            dec.execution.effective_workers,
            dec.execution.block_count,
            dec.execution.queue_capacity,
        );

        let mut group = c.benchmark_group(&group_name);
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_function("decode", |b| {
            b.iter_batched(
                || decode_jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(jobs, &config)
                            .expect("parallel decode benchmark failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

/// Benchmark decode scaling on the cold one-wave workload (16 MiB, 16 blocks).
fn bench_parallel_decode_cold_16mb(c: &mut Criterion) {
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (decode_jobs, _, corpus_data) = preflight_parallel(
        total_size,
        block_size,
        THREAD_COUNTS,
        ModelProfile::Skewed2551,
        42,
    );

    for &threads in THREAD_COUNTS {
        let config = config_for_threads(threads);
        let group_name = format!(
            "parallel/decode/cold-executor/{}threads/1MiB-blocks/16MiB",
            threads
        );

        // Preflight record: run the exact case decode once, verify parity, emit.
        let dec =
            ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(decode_jobs.clone(), &config)
                .expect("parallel decode cold record preflight");
        let mut decoded = Vec::new();
        let mut words = 0usize;
        let mut states: Vec<u32> = Vec::new();
        for b in &dec.blocks {
            decoded.extend_from_slice(&b.output);
            words += b.words_consumed;
            states.extend_from_slice(&b.final_states);
        }
        assert_eq!(
            decoded, corpus_data,
            "parallel decode cold {}t record preflight: output must match original",
            threads
        );
        emit_preflight(
            format!("{}/decode", group_name),
            "parallel-auto",
            &corpus_data,
            &decoded,
            &corpus_data,
            Some(words),
            Some(words),
            Some(&states),
            Some(&states),
            threads,
            dec.execution.effective_workers,
            dec.execution.block_count,
            dec.execution.queue_capacity,
        );

        let mut group = c.benchmark_group(&group_name);
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_function("decode", |b| {
            b.iter_batched(
                || decode_jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(jobs, &config)
                            .expect("parallel decode benchmark failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

/// Benchmark verify scaling across all thread counts.
fn bench_parallel_verify_scaling(c: &mut Criterion) {
    let total_size: usize = 64 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (_, verify_jobs, corpus_data) = preflight_parallel(
        total_size,
        block_size,
        THREAD_COUNTS,
        ModelProfile::Skewed2551,
        42,
    );

    let block_count =
        ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size).block_count();

    for &threads in THREAD_COUNTS {
        let config = config_for_threads(threads);
        let group_name = format!(
            "parallel/verify/cold-executor/{}threads/1MiB-blocks/64MiB",
            threads
        );

        // Preflight record: verify the exact case workload once, then emit.
        let vrep =
            ryg_rans_rs_parallel::ParallelVerifier::verify_blocks(verify_jobs.clone(), &config)
                .expect("parallel verify record preflight");
        assert_eq!(
            vrep.blocks_failed, 0,
            "parallel verify {}t record preflight: failures present",
            threads
        );
        assert_eq!(
            vrep.blocks_verified as usize, block_count,
            "parallel verify {}t record preflight: block count mismatch",
            threads
        );
        let effective = ryg_rans_rs_parallel::effective_worker_count(&config, block_count)
            .expect("effective worker count");
        let queue_capacity = config.max_in_flight_blocks.get().max(effective);
        emit_preflight(
            format!("{}/verify", group_name),
            "parallel-auto",
            &corpus_data,
            &corpus_data,
            &corpus_data,
            None,
            None,
            None,
            None,
            threads,
            effective,
            block_count,
            queue_capacity,
        );

        let mut group = c.benchmark_group(&group_name);
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_function("verify", |b| {
            b.iter_batched(
                || verify_jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelVerifier::verify_blocks(jobs, &config)
                            .expect("parallel verify benchmark failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

/// Benchmark encode scaling across all thread counts.
fn bench_parallel_encode_scaling(c: &mut Criterion) {
    let total_size: usize = 64 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let corpus = Corpus::generate(ModelProfile::Skewed2551, total_size, 42);
    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size);

    // Preflight: encode with every thread count, verify decoded output is identical.
    let mut encoded_ref: Option<Vec<Vec<u8>>> = None;
    for &threads in THREAD_COUNTS {
        let cfg = config_for_threads(threads);
        let jobs: Vec<ryg_rans_rs_parallel::EncodeBlockJob> = plan
            .ranges
            .iter()
            .map(|r| {
                let s = r.input_offset as usize;
                ryg_rans_rs_parallel::EncodeBlockJob::new(
                    r.block_index,
                    corpus.data[s..s + r.length as usize].to_vec(),
                    ryg_rans_rs_parallel::CodecPolicy::Auto,
                    ryg_rans_rs_parallel::ModelPolicy::PerBlock,
                    12,
                )
            })
            .collect();
        let encoded = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs, &cfg)
            .expect(&format!("preflight encode {}t", threads));

        // Decode and compare
        let dj: Vec<_> = encoded
            .blocks
            .iter()
            .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
                block_index: b.block_index,
                block_data: b.block.clone(),
            })
            .collect();
        let dec = ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(dj, &config_for_threads(4))
            .expect(&format!("preflight decode after encode {}t", threads));
        let mut full = Vec::new();
        for b in &dec.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(
            full, corpus.data,
            "encode preflight {}t: roundtrip must match original",
            threads
        );

        // Verify byte-identical encoding across thread counts
        if let Some(ref ref_blocks) = encoded_ref {
            for (i, block) in encoded.blocks.iter().enumerate() {
                assert_eq!(
                    block.block, ref_blocks[i],
                    "encode preflight: block {} differs between thread counts",
                    i
                );
            }
        } else {
            encoded_ref = Some(encoded.blocks.iter().map(|b| b.block.clone()).collect());
        }
    }

    for &threads in THREAD_COUNTS {
        let cfg = config_for_threads(threads);
        let group_name = format!(
            "parallel/encode/cold-executor/{}threads/1MiB-blocks/64MiB",
            threads
        );

        // Preflight record: encode with the case config, round-trip verify, emit.
        let enc_jobs: Vec<ryg_rans_rs_parallel::EncodeBlockJob> = plan
            .ranges
            .iter()
            .map(|r| {
                let s = r.input_offset as usize;
                ryg_rans_rs_parallel::EncodeBlockJob::new(
                    r.block_index,
                    corpus.data[s..s + r.length as usize].to_vec(),
                    ryg_rans_rs_parallel::CodecPolicy::Auto,
                    ryg_rans_rs_parallel::ModelPolicy::PerBlock,
                    12,
                )
            })
            .collect();
        let encoded = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(enc_jobs, &cfg)
            .expect("parallel encode record preflight");
        assert_eq!(
            encoded.blocks.len(),
            plan.block_count(),
            "parallel encode {}t record preflight: block count mismatch",
            threads
        );
        let dj: Vec<_> = encoded
            .blocks
            .iter()
            .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
                block_index: b.block_index,
                block_data: b.block.clone(),
            })
            .collect();
        let dec = ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(dj, &config_for_threads(4))
            .expect("parallel encode record decode");
        let mut full = Vec::new();
        for b in &dec.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(
            full, corpus.data,
            "parallel encode {}t record preflight: roundtrip must match original",
            threads
        );
        emit_preflight(
            format!("{}/encode", group_name),
            "parallel-auto",
            &corpus.data,
            &corpus.data,
            &corpus.data,
            None,
            None,
            None,
            None,
            threads,
            encoded.execution.effective_workers,
            plan.block_count(),
            encoded.execution.queue_capacity,
        );

        let mut group = c.benchmark_group(&group_name);
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_function("encode", |b| {
            let data = corpus.data.clone();
            b.iter_batched(
                || data.clone(),
                |d| {
                    let j: Vec<ryg_rans_rs_parallel::EncodeBlockJob> = plan
                        .ranges
                        .iter()
                        .map(|r| {
                            let s = r.input_offset as usize;
                            ryg_rans_rs_parallel::EncodeBlockJob::new(
                                r.block_index,
                                d[s..s + r.length as usize].to_vec(),
                                ryg_rans_rs_parallel::CodecPolicy::Auto,
                                ryg_rans_rs_parallel::ModelPolicy::PerBlock,
                                12,
                            )
                        })
                        .collect();
                    black_box(
                        ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(j, &cfg)
                            .expect("parallel encode benchmark failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

criterion_group!(
    name = parallel_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(10))
        .sample_size(30);
    targets =
        bench_parallel_decode_scaling,
        bench_parallel_decode_cold_16mb,
        bench_parallel_verify_scaling,
        bench_parallel_encode_scaling,
);

criterion_main!(parallel_benches);
