//! # Criterion benchmark: Independent-stream batching
//!
//! Tier 5 benchmarks measuring aggregate and per-stream throughput
//! for batch decode of multiple independent 16-way streams.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_bench::common::verification;

fn avx2_available() -> bool {
    ryg_rans_rs_simd::backends::avx2_available_checked()
}

fn bench_batch_scalar_sequential(c: &mut Criterion) {
    // Compare sequential decode of 4 streams vs batched decode.
    let profiles = [ModelProfile::Uniform256, ModelProfile::Skewed2551];

    for &profile in &profiles {
        let mut group = c.benchmark_group(format!(
            "batch/scalar-sequential/decode/{}/4x1MiB",
            profile.label(),
        ));

        let corpora: Vec<Corpus> = (0..4)
            .map(|i| Corpus::generate(profile, 1048576, 42 + i as u64))
            .collect();

        let encoded: Vec<Vec<u16>> = corpora.iter().map(|c| c.encode_16way()).collect();
        let tables: Vec<_> = corpora.iter().map(|c| c.packed_table()).collect();

        group.throughput(Throughput::Bytes(4 * 1048576));

        group.bench_function("sequential-scalar-4x", |b| {
            b.iter(|| {
                let mut total_bytes = 0usize;
                for (j, corpus) in corpora.iter().enumerate() {
                    let (out, _) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
                        black_box(&encoded[j]),
                        black_box(&tables[j]),
                        black_box(corpus.data.len()),
                    )
                    .expect("scalar decode");
                    total_bytes += out.len();
                }
                black_box(total_bytes);
            });
        });
        group.finish();
    }
}

fn bench_batch_avx2_2x8_sequential(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: batch/avx2-2x8");
        return;
    }

    let perm_table = ryg_rans_rs_simd::avx2_renorm::build_avx2_renorm_table();
    let profile = ModelProfile::Skewed2551;

    let corpora: Vec<Corpus> = (0..4)
        .map(|i| Corpus::generate(profile, 1048576, 42 + i as u64))
        .collect();
    let encoded: Vec<Vec<u16>> = corpora.iter().map(|c| c.encode_16way()).collect();
    let tables: Vec<_> = corpora.iter().map(|c| c.packed_table()).collect();

    // Verify one stream
    let (ref_out, ref_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
        &encoded[0],
        &tables[0],
        1048576,
    )
    .expect("scalar ref");

    unsafe {
        let mut v_out = vec![0u8; 1048576];
        let v_rep = ryg_rans_rs_simd::avx2::decode_interleaved16_avx2_2x8_into(
            &encoded[0],
            &tables[0],
            &mut v_out,
            &perm_table,
        )
        .expect("AVX2 2x8 verify");
        let vr = verification::verify_16way(
            "avx2-2x8-batch",
            &v_out,
            v_rep.words_consumed,
            &v_rep.final_states,
            &ref_out,
            ref_report.words_consumed,
            &ref_report.final_states,
        );
        verification::assert_verified(&vr);
    }

    let mut group = c.benchmark_group("batch/avx2-seq-2x8/decode/SKEWED_255_1/4x1MiB");
    group.throughput(Throughput::Bytes(4 * 1048576));

    group.bench_function("avx2-2x8-sequential-4x", |b| {
        b.iter(|| {
            let mut total = 0usize;
            for j in 0..4 {
                let mut buf = vec![0u8; 1048576];
                unsafe {
                    let report = ryg_rans_rs_simd::avx2::decode_interleaved16_avx2_2x8_into(
                        black_box(&encoded[j]),
                        black_box(&tables[j]),
                        black_box(&mut buf),
                        black_box(&perm_table),
                    )
                    .expect("AVX2 2x8");
                    total += report.words_consumed;
                }
            }
            black_box(total);
        });
    });
    group.finish();
}

fn bench_batch_avx2_2x8_batch4_aggregate(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: batch/avx2-batch4");
        return;
    }

    let perm_table = ryg_rans_rs_simd::avx2_renorm::build_avx2_renorm_table();
    let profile = ModelProfile::Skewed2551;

    // Use mixed tail lengths to exercise all tail classes
    let sizes: [usize; 4] = [1048576, 1048575, 1048569, 1048561]; // 0, 15, 9, 1 mod 16

    let corpora: Vec<Corpus> = (0..4)
        .map(|i| Corpus::generate(profile, sizes[i], 42 + i as u64))
        .collect();
    let encoded: Vec<Vec<u16>> = corpora.iter().map(|c| c.encode_16way()).collect();
    let tables: Vec<_> = corpora.iter().map(|c| c.packed_table()).collect();

    // Preflight: verify Batch4 against scalar reference for ALL 4 streams
    // including output, words consumed, and final states.
    for j in 0..4 {
        let (ref_out, _ref_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
            &encoded[j],
            &tables[j],
            corpora[j].data.len(),
        )
        .expect("scalar ref");

        assert_eq!(
            ref_out, corpora[j].data,
            "batch preflight: scalar ref output must match original for stream {}",
            j
        );
    }

    // Run Batch4 and verify all 4 outputs, words consumed, and final states
    {
        let mut out0 = vec![0u8; sizes[0]];
        let mut out1 = vec![0u8; sizes[1]];
        let mut out2 = vec![0u8; sizes[2]];
        let mut out3 = vec![0u8; sizes[3]];
        let mut jobs: [ryg_rans_rs_simd::avx2::Avx2DecodeJob; 4] = [
            ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                compressed: &encoded[0],
                table: &tables[0],
                output: &mut out0,
                block_index: 0,
            },
            ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                compressed: &encoded[1],
                table: &tables[1],
                output: &mut out1,
                block_index: 1,
            },
            ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                compressed: &encoded[2],
                table: &tables[2],
                output: &mut out2,
                block_index: 2,
            },
            ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                compressed: &encoded[3],
                table: &tables[3],
                output: &mut out3,
                block_index: 3,
            },
        ];
        let reports = unsafe {
            ryg_rans_rs_simd::avx2::decode_batch4_interleaved16_avx2(&mut jobs, &perm_table)
                .expect("avx2 batch4 preflight")
        };

        for j in 0..4 {
            let (ref_out, ref_report) =
                ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
                    &encoded[j],
                    &tables[j],
                    corpora[j].data.len(),
                )
                .expect("scalar ref for batch4 verify");

            let batch_output = match j {
                0 => &out0,
                1 => &out1,
                2 => &out2,
                3 => &out3,
                _ => unreachable!(),
            };

            assert_eq!(
                batch_output, &ref_out,
                "Batch4 output mismatch for stream {} (size {})",
                j, sizes[j]
            );
            assert_eq!(
                reports[j].words_consumed, ref_report.words_consumed,
                "Batch4 words consumed mismatch for stream {}",
                j
            );
            assert_eq!(
                &reports[j].final_states[..],
                &ref_report.final_states[..],
                "Batch4 final states mismatch for stream {}",
                j
            );
        }
    }

    // Benchmark: allocation policy matched for both sequential and batch4
    let mut group = c.benchmark_group("batch/avx2-batch4-on16/aggregate/SKEWED_255_1/4x1MiB");
    group.throughput(Throughput::Bytes(sizes.iter().sum::<usize>() as u64));

    group.bench_function("avx2-batch4-into", |b| {
        b.iter_batched(
            || {
                // Reset buffers (reuse allocation but zero content not needed
                // since batch4 decoder overwrites all bytes)
                (
                    vec![0u8; sizes[0]],
                    vec![0u8; sizes[1]],
                    vec![0u8; sizes[2]],
                    vec![0u8; sizes[3]],
                )
            },
            |(mut o0, mut o1, mut o2, mut o3)| {
                let mut jobs: [ryg_rans_rs_simd::avx2::Avx2DecodeJob; 4] = [
                    ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                        compressed: &encoded[0],
                        table: &tables[0],
                        output: &mut o0,
                        block_index: 0,
                    },
                    ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                        compressed: &encoded[1],
                        table: &tables[1],
                        output: &mut o1,
                        block_index: 1,
                    },
                    ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                        compressed: &encoded[2],
                        table: &tables[2],
                        output: &mut o2,
                        block_index: 2,
                    },
                    ryg_rans_rs_simd::avx2::Avx2DecodeJob {
                        compressed: &encoded[3],
                        table: &tables[3],
                        output: &mut o3,
                        block_index: 3,
                    },
                ];
                unsafe {
                    let reports = ryg_rans_rs_simd::avx2::decode_batch4_interleaved16_avx2(
                        &mut jobs,
                        &perm_table,
                    )
                    .expect("avx2 batch4");
                    black_box(reports)
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();

    // Sequential 2x8 _into benchmark with PREALLOCATED buffers (no allocation inside timed region)
    let mut seq_group = c.benchmark_group("batch/avx2-seq-2x8/into/SKEWED_255_1/4x1MiB");
    seq_group.throughput(Throughput::Bytes(sizes.iter().sum::<usize>() as u64));
    seq_group.bench_function("avx2-2x8-sequential-into-4x", |b| {
        b.iter_batched(
            || {
                (
                    vec![0u8; sizes[0]],
                    vec![0u8; sizes[1]],
                    vec![0u8; sizes[2]],
                    vec![0u8; sizes[3]],
                )
            },
            |(mut o0, mut o1, mut o2, mut o3)| unsafe {
                let parts: [(
                    &[u16],
                    &ryg_rans_rs_simd::packed_table::PackedWordTable,
                    &mut [u8],
                ); 4] = [
                    (&encoded[0], &tables[0], &mut o0),
                    (&encoded[1], &tables[1], &mut o1),
                    (&encoded[2], &tables[2], &mut o2),
                    (&encoded[3], &tables[3], &mut o3),
                ];
                for (compressed, table, buf) in parts {
                    let report = ryg_rans_rs_simd::avx2::decode_interleaved16_avx2_2x8_into(
                        black_box(compressed),
                        black_box(table),
                        black_box(buf),
                        black_box(&perm_table),
                    )
                    .expect("AVX2 2x8 seq");
                    black_box(report);
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
    seq_group.finish();
}

criterion_group!(
    name = batch_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(10))
        .sample_size(50);
    targets =
        bench_batch_scalar_sequential,
        bench_batch_avx2_2x8_sequential,
        bench_batch_avx2_2x8_batch4_aggregate,
);

criterion_main!(batch_benches);
