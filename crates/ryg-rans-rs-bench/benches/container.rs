//! # Criterion benchmark: Block-engine throughput scaling
//!
//! Tier 7 benchmarks measuring the parallel block engine's decode+integrity,
//! verify, and encode throughput across 1, 2, 4, 8, and 16 thread counts.
//!
//! "Decode+integrity" includes payload SHA-256, decoded SHA-256, and model
//! reconstruction as part of normal block decoding.  A true decode-only kernel
//! benchmark is in the scalar/avx2/avx512 SIMD tiers.
//!
//! ## Scaling workloads
//!
//! ### Cold one-wave (16 MiB)
//! 16 × 1 MiB blocks.  At 16 workers this is one wave.  Tests startup latency.
//!
//! ### Sustained (64 MiB)
//! 64 × 1 MiB blocks.  At 8 workers: ~8 waves.  At 16 workers: ~4 waves.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::num::NonZeroUsize;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

const THREAD_COUNTS: &[usize] = &[1, 2, 4, 8, 16];

fn config_for_threads(threads: usize) -> ryg_rans_rs_parallel::ParallelConfig {
    ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(
            NonZeroUsize::new(threads).expect("nonzero"),
        ),
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
        ..Default::default()
    }
}

/// Encode the corpus and return decode_jobs and verify_jobs.
fn preflight_block_engine(
    total_size: usize,
    block_size: u64,
    profile: ModelProfile,
    seed: u64,
) -> (
    Vec<ryg_rans_rs_parallel::DecodeBlockJob>,
    Vec<ryg_rans_rs_parallel::VerifyBlockJob>,
    Vec<ryg_rans_rs_parallel::EncodeBlockJob>,
    Corpus,
) {
    let corpus = Corpus::generate(profile, total_size, seed);
    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size);

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

    let encoded = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs.clone(), &cfg_encode)
        .expect("encode");

    let decode_jobs: Vec<ryg_rans_rs_parallel::DecodeBlockJob> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    let verify_jobs: Vec<ryg_rans_rs_parallel::VerifyBlockJob> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::VerifyBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // Preflight: every thread count must produce identical output
    for &tc in THREAD_COUNTS {
        let cfg = config_for_threads(tc);
        let dec = ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(decode_jobs.clone(), &cfg)
            .expect(&format!("preflight decode {}t", tc));
        let mut full = Vec::new();
        for b in &dec.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(
            full, corpus.data,
            "block-engine preflight {}t: decoded output must match original",
            tc
        );
    }

    (decode_jobs, verify_jobs, jobs, corpus)
}

fn bench_block_engine_decode_scaling(c: &mut Criterion) {
    let total_size: usize = 64 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (decode_jobs, _, _, _) =
        preflight_block_engine(total_size, block_size, ModelProfile::Skewed2551, 42);

    for &threads in THREAD_COUNTS {
        let cfg_decode = config_for_threads(threads);
        let group_name = format!(
            "block-engine/decode+integrity/{}threads/1MiB-blocks/64MiB",
            threads
        );

        let mut group = c.benchmark_group(&group_name);
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_function("decode", |b| {
            b.iter_batched(
                || decode_jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(jobs, &cfg_decode)
                            .expect("block-engine decode failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

fn bench_block_engine_decode_cold_16mb(c: &mut Criterion) {
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (decode_jobs, _, _, _) =
        preflight_block_engine(total_size, block_size, ModelProfile::Skewed2551, 42);

    for &threads in THREAD_COUNTS {
        let cfg_decode = config_for_threads(threads);
        let group_name = format!(
            "block-engine/decode+integrity/{}threads/1MiB-blocks/16MiB",
            threads
        );

        let mut group = c.benchmark_group(&group_name);
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_function("decode", |b| {
            b.iter_batched(
                || decode_jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(jobs, &cfg_decode)
                            .expect("block-engine decode failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

fn bench_block_engine_encode_scaling(c: &mut Criterion) {
    let total_size: usize = 64 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (_, _, _, corpus) =
        preflight_block_engine(total_size, block_size, ModelProfile::Skewed2551, 42);

    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size);

    for &threads in THREAD_COUNTS {
        let cfg = config_for_threads(threads);
        let group_name = format!("block-engine/encode/{}threads/1MiB-blocks/64MiB", threads);

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
                            .expect("block-engine encode failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

fn bench_block_engine_verify_scaling(c: &mut Criterion) {
    let total_size: usize = 64 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (_, verify_jobs, _, _) =
        preflight_block_engine(total_size, block_size, ModelProfile::Skewed2551, 42);

    for &threads in THREAD_COUNTS {
        let cfg = config_for_threads(threads);
        let group_name = format!("block-engine/verify/{}threads/1MiB-blocks/64MiB", threads);

        let mut group = c.benchmark_group(&group_name);
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_function("verify", |b| {
            b.iter_batched(
                || verify_jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelVerifier::verify_blocks(jobs, &cfg)
                            .expect("block-engine verify failed"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

criterion_group!(
    name = container_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(10))
        .sample_size(30);
    targets =
        bench_block_engine_decode_scaling,
        bench_block_engine_decode_cold_16mb,
        bench_block_engine_encode_scaling,
        bench_block_engine_verify_scaling,
);

criterion_main!(container_benches);
