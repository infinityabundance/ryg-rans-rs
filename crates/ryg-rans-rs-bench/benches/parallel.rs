//! # Criterion benchmark: Parallel block engine
//!
//! Tier 6 benchmarks measuring parallel encoding, decoding, verification,
//! and scaling across thread counts.  Cold-executor measurements create
//! and join worker threads on every call.  Preflight verification ensures
//! parallel output matches original data before any timing.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::num::NonZeroUsize;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

/// Run preflight: encode, decode with 1 thread, decode with N threads,
/// verify all match original corpus.
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

    // Encode blocks
    let cfg_encode = ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };
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

    // Verify 1-thread decode matches original
    for &tc in thread_counts {
        let cfg = ryg_rans_rs_parallel::ParallelConfig {
            threads: ryg_rans_rs_parallel::ThreadCount::Exact(NonZeroUsize::new(tc).unwrap()),
            ..Default::default()
        };
        let decoded = ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(dj.clone(), &cfg)
            .expect(&format!("preflight decode {}t", tc));
        let mut full = Vec::new();
        for b in &decoded.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(
            full, corpus.data,
            "preflight: {}-thread decode must match original data",
            tc
        );
    }

    let vj: Vec<ryg_rans_rs_parallel::VerifyBlockJob> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::VerifyBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    (dj, vj)
}

fn bench_parallel_decode_1_thread(c: &mut Criterion) {
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (decode_jobs, _) = preflight_parallel(
        total_size,
        block_size,
        &[1, 4],
        ModelProfile::Skewed2551,
        42,
    );

    let cfg = ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        ..Default::default()
    };

    let mut group = c.benchmark_group("parallel/decode/cold-executor/1thread/1MiB-blocks/16MiB");
    group.throughput(Throughput::Bytes(total_size as u64));

    group.bench_function("decode-1t", |b| {
        let dj: Vec<_> = decode_jobs
            .iter()
            .map(|j| ryg_rans_rs_parallel::DecodeBlockJob {
                block_index: j.block_index,
                block_data: j.block_data.clone(),
            })
            .collect();
        b.iter_batched(
            || dj.clone(),
            |jobs| {
                black_box(ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(
                    jobs, &cfg,
                ))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_parallel_decode_4_threads(c: &mut Criterion) {
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (decode_jobs, _) = preflight_parallel(
        total_size,
        block_size,
        &[1, 4],
        ModelProfile::Skewed2551,
        42,
    );

    let cfg = ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };

    let mut group = c.benchmark_group("parallel/decode/cold-executor/4threads/1MiB-blocks/16MiB");
    group.throughput(Throughput::Bytes(total_size as u64));

    group.bench_function("decode-4t", |b| {
        let dj: Vec<_> = decode_jobs
            .iter()
            .map(|j| ryg_rans_rs_parallel::DecodeBlockJob {
                block_index: j.block_index,
                block_data: j.block_data.clone(),
            })
            .collect();
        b.iter_batched(
            || dj.clone(),
            |jobs| {
                black_box(ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(
                    jobs, &cfg,
                ))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_parallel_verify_4_threads(c: &mut Criterion) {
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let (_, verify_jobs) = preflight_parallel(
        total_size,
        block_size,
        &[1, 4],
        ModelProfile::Skewed2551,
        42,
    );

    let cfg = ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };

    let mut group = c.benchmark_group("parallel/verify/cold-executor/4threads/1MiB-blocks/16MiB");
    group.throughput(Throughput::Bytes(total_size as u64));

    group.bench_function("verify-4t", |b| {
        let vj: Vec<_> = verify_jobs
            .iter()
            .map(|j| ryg_rans_rs_parallel::VerifyBlockJob {
                block_index: j.block_index,
                block_data: j.block_data.clone(),
            })
            .collect();
        b.iter_batched(
            || vj.clone(),
            |jobs| {
                black_box(ryg_rans_rs_parallel::ParallelVerifier::verify_blocks(
                    jobs, &cfg,
                ))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    name = parallel_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(10))
        .sample_size(30);
    targets =
        bench_parallel_decode_1_thread,
        bench_parallel_decode_4_threads,
        bench_parallel_verify_4_threads,
);

criterion_main!(parallel_benches);
