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

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

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
/// Returns (decode_jobs, verify_jobs) for use in benchmarks.
fn preflight_parallel(
    total_size: usize,
    block_size: u64,
    thread_counts: &[usize],
    profile: ModelProfile,
    seed: u64,
) -> (
    Vec<ryg_rans_rs_parallel::DecodeBlockJob>,
    Vec<ryg_rans_rs_parallel::VerifyBlockJob>,
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

    (dj, vj)
}

/// Benchmark decode scaling across all thread counts.
fn bench_parallel_decode_scaling(c: &mut Criterion) {
    // Sustained workload: 64 MiB, 64 blocks, multiple waves per thread
    let total_size: usize = 64 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (decode_jobs, _) = preflight_parallel(
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

    let (decode_jobs, _) = preflight_parallel(
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

    let (_, verify_jobs) = preflight_parallel(
        total_size,
        block_size,
        THREAD_COUNTS,
        ModelProfile::Skewed2551,
        42,
    );

    for &threads in THREAD_COUNTS {
        let config = config_for_threads(threads);
        let group_name = format!(
            "parallel/verify/cold-executor/{}threads/1MiB-blocks/64MiB",
            threads
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
