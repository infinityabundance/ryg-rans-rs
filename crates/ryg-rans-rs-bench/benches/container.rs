//! # Criterion benchmark: Block-engine throughput
//!
//! Measures the parallel block engine's decode, verify, and encode throughput
//! with explicit thread counts.  "Decode-only" includes integrity hashing
//! (payload SHA-256, decoded SHA-256) which is part of normal block decoding.
//! A true decode-only kernel benchmark is in the scalar/avx2/avx512 tiers.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::num::NonZeroUsize;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

fn bench_block_engine_decode(c: &mut Criterion) {
    let mut group = c.benchmark_group("block-engine/decode+integrity/4threads/1MiB-blocks/16MiB");
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let corpus = Corpus::generate(ModelProfile::Skewed2551, total_size, 42);
    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size);
    let cfg_encode = ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };
    let cfg_decode = ryg_rans_rs_parallel::ParallelConfig {
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

    let encoded =
        ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs, &cfg_encode).expect("encode");

    let decode_jobs: Vec<ryg_rans_rs_parallel::DecodeBlockJob> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // Preflight: decode must match original
    let decoded =
        ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(decode_jobs.clone(), &cfg_decode)
            .expect("preflight decode");
    let mut full = Vec::new();
    for b in &decoded.blocks {
        full.extend_from_slice(&b.output);
    }
    assert_eq!(
        full, corpus.data,
        "block-engine decode preflight: must match original"
    );

    group.throughput(Throughput::Bytes(total_size as u64));
    group.bench_function("decode", |b| {
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
                    jobs,
                    &cfg_decode,
                ))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_block_engine_encode(c: &mut Criterion) {
    let mut group = c.benchmark_group("block-engine/encode/4threads/1MiB-blocks/16MiB");
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let corpus = Corpus::generate(ModelProfile::Skewed2551, total_size, 42);
    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size);
    let cfg = ryg_rans_rs_parallel::ParallelConfig {
        threads: ryg_rans_rs_parallel::ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };

    // Preflight: encode then decode must match original
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
    let encoded =
        ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs, &cfg).expect("preflight encode");
    let dj: Vec<_> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();
    let decoded =
        ryg_rans_rs_parallel::ParallelDecoder::decode_blocks(dj, &cfg).expect("preflight decode");
    let mut full = Vec::new();
    for b in &decoded.blocks {
        full.extend_from_slice(&b.output);
    }
    assert_eq!(
        full, corpus.data,
        "encode preflight: roundtrip must match original"
    );

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
                black_box(ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(
                    j, &cfg,
                ))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_block_engine_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("block-engine/verify/4threads/1MiB-blocks/16MiB");
    let total_size: usize = 16 * 1024 * 1024;
    let block_size: u64 = 1024 * 1024;

    let corpus = Corpus::generate(ModelProfile::Skewed2551, total_size, 42);
    let plan = ryg_rans_rs_parallel::FixedBlockPlan::new(total_size as u64, block_size);
    let cfg = ryg_rans_rs_parallel::ParallelConfig {
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
    let encoded =
        ryg_rans_rs_parallel::ParallelEncoder::encode_blocks(jobs, &cfg).expect("preflight encode");
    let vj: Vec<ryg_rans_rs_parallel::VerifyBlockJob> = encoded
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::VerifyBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // Preflight: verify must succeed
    let report = ryg_rans_rs_parallel::ParallelVerifier::verify_blocks(vj.clone(), &cfg)
        .expect("preflight verify");
    assert_eq!(
        report.blocks_failed, 0,
        "verify preflight: all blocks must pass"
    );

    group.throughput(Throughput::Bytes(total_size as u64));
    group.bench_function("verify", |b| {
        let vj2: Vec<_> = vj
            .iter()
            .map(|j| ryg_rans_rs_parallel::VerifyBlockJob {
                block_index: j.block_index,
                block_data: j.block_data.clone(),
            })
            .collect();
        b.iter_batched(
            || vj2.clone(),
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
    name = container_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(10))
        .sample_size(30);
    targets =
        bench_block_engine_decode,
        bench_block_engine_encode,
        bench_block_engine_verify,
);

criterion_main!(container_benches);
