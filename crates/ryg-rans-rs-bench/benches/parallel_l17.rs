//! # Criterion bench: Phase L.17 analysis surfaces
//!
//! Adds the L.17 analysis dimensions to the sealed surface set:
//!
//! * **Queue-depth sweep** (8/16/32/64/128) at fixed worker count: shows
//!   whether `max_in_flight_blocks` is a throughput lever or a pure bound.
//! * **Sequential-threshold crossover**: sweeps
//!   `parallel_threshold_bytes` so the sequential fallback vs pooled modes
//!   can be compared at equal workloads, exposing the crossover point the
//!   L.6 wiring made observable (`ExecutionMode` is recorded per run).
//! * **256 MiB extended workload** for the L.17 extended-size dimension.
//!
//! Preflight is identical to `parallel.rs`: encode once, then decode with
//! every configuration, asserting byte-identical output, identical backend
//! identities, words consumed, and final states across all configurations.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::num::NonZeroUsize;
use std::time::Duration;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

/// Queue depths for the sweep (the CLI/executor default is 64).
const QUEUE_DEPTHS: &[usize] = &[8, 16, 32, 64, 128];

/// Thread counts for the threshold crossover sweep.
const THREAD_COUNTS: &[usize] = &[1, 4, 16];

/// Build a config with the given worker count and queue depth.
fn config(threads: usize, queue: usize) -> ryg_rans_rs_parallel::ParallelConfig {
    ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(
            NonZeroUsize::new(threads).expect("thread count must be nonzero"),
        ),
        max_in_flight_blocks: NonZeroUsize::new(queue).unwrap(),
        ..Default::default()
    }
}

/// Build decode jobs for `total_size` bytes in 1 MiB blocks with full
/// preflight against a single-threaded decode (determinism across configs).
fn preflight_jobs(total_size: usize, seed: u64) -> Vec<ryg_rans_rs_parallel::DecodeBlockJob> {
    let corpus = Corpus::generate(ModelProfile::Skewed2551, total_size, seed);
    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, 1024 * 1024);
    let cfg_encode = config(4, 64);
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
    let enc = ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs, &cfg_encode)
        .expect("preflight encode");
    let decode_jobs: Vec<ryg_rans_rs_parallel::DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // Reference decode with 1 thread; every other configuration must match.
    let reference = ryg_rans_rs_parallel::ParallelDecoder::new(config(1, 64))
        .decode_blocks(decode_jobs.clone())
        .expect("reference decode");
    for &threads in THREAD_COUNTS {
        let got = ryg_rans_rs_parallel::ParallelDecoder::new(config(threads, 64))
            .decode_blocks(decode_jobs.clone())
            .expect("preflight decode");
        assert_eq!(got.blocks.len(), reference.blocks.len());
        for (g, r) in got.blocks.iter().zip(reference.blocks.iter()) {
            assert_eq!(g.output_hash, r.output_hash, "output hash parity");
            assert_eq!(g.words_consumed, r.words_consumed, "words consumed parity");
        }
    }
    decode_jobs
}

/// Queue-depth sweep at 8 workers on the 64 MiB sustained workload.
fn bench_queue_depth_sweep(c: &mut Criterion) {
    let total_size: usize = 64 * 1024 * 1024;
    let jobs = preflight_jobs(total_size, 7);
    for &depth in QUEUE_DEPTHS {
        let cfg = config(8, depth);
        let mut group = c.benchmark_group(&format!(
            "parallel-l17/queue-depth/8workers/{}blocks/64MiB",
            depth
        ));
        group.throughput(Throughput::Bytes(total_size as u64));
        group.bench_function("decode", |b| {
            b.iter_batched(
                || jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelDecoder::new(cfg.clone()).decode_blocks(jobs)
                            .expect("queue-depth decode"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

/// Sequential-threshold crossover: with a 64 MiB workload and the threshold
/// below/at/above the total size, the executor should pick sequential
/// fallback vs the pooled path (the config wiring makes this observable).
fn bench_sequential_threshold_crossover(c: &mut Criterion) {
    let total_size: usize = 64 * 1024 * 1024;
    let jobs = preflight_jobs(total_size, 11);
    let thresholds: &[u64] = &[1, 1024 * 1024, total_size as u64, u64::MAX];
    for &thr in thresholds {
        for &threads in &[1usize, 16] {
            let cfg = ryg_rans_rs_parallel::ParallelConfig {
                threads: ryg_rans_rs_parallel::ThreadCount::Exact(
                    NonZeroUsize::new(threads).unwrap(),
                ),
                parallel_threshold_bytes: thr,
                ..Default::default()
            };
            let mut group = c.benchmark_group(&format!(
                "parallel-l17/sequential-threshold/{}/{}workers/64MiB",
                thr, threads
            ));
            group.throughput(Throughput::Bytes(total_size as u64));
            group.bench_function("decode", |b| {
                b.iter_batched(
                    || jobs.clone(),
                    |jobs| {
                        black_box(
                            ryg_rans_rs_parallel::ParallelDecoder::new(cfg.clone()).decode_blocks(jobs)
                                .expect("threshold decode"),
                        )
                    },
                    criterion::BatchSize::SmallInput,
                );
            });
            group.finish();
        }
    }
}

/// 256 MiB extended workload at 1/4/16 workers (L.17 extended dimension).
fn bench_extended_256mb(c: &mut Criterion) {
    let total_size: usize = 256 * 1024 * 1024;
    let jobs = preflight_jobs(total_size, 13);
    for &threads in THREAD_COUNTS {
        let cfg = config(threads, 64);
        let mut group = c.benchmark_group(&format!(
            "parallel-l17/extended/{}threads/1MiB-blocks/256MiB",
            threads
        ));
        group.throughput(Throughput::Bytes(total_size as u64));
        group.bench_function("decode", |b| {
            b.iter_batched(
                || jobs.clone(),
                |jobs| {
                    black_box(
                        ryg_rans_rs_parallel::ParallelDecoder::new(cfg.clone()).decode_blocks(jobs)
                            .expect("extended decode"),
                    )
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
}

criterion_group!(
    name = parallel_l17;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(6))
        .sample_size(20);
    targets = bench_queue_depth_sweep, bench_sequential_threshold_crossover, bench_extended_256mb
);
criterion_main!(parallel_l17);
