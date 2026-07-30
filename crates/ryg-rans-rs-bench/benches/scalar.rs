//! # Criterion benchmark: Portable scalar reference decoders
//!
//! Tier 0 benchmarks for scalar-8way-legacy, scalar-8way-packed, scalar-16way,
//! and uniform256-scalar-specialized decoding.  These are the correctness and
//! portability baselines that all SIMD backends must match.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::vec::Vec;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_bench::common::verification;

// ---------------------------------------------------------------------------
// Benchmark helpers
// ---------------------------------------------------------------------------

fn bench_decode_8way_packed(c: &mut Criterion) {
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 64 * 1024, 42); // 64 KiB for speed
    let table = corpus.packed_table();

    // 8-way encoded data
    let encoded_8way =
        ryg_rans_rs_simd::encode_8way_for_test(&corpus.data, &corpus.freqs, &corpus.cum_freqs);

    // Preflight: verify correctness against original data
    let (slots, slot2sym) =
        ryg_rans_rs_simd::build_word_tables(&corpus.freqs, &corpus.cum_freqs, corpus.scale_bits);
    let ref_tables = ryg_rans_rs_simd::RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };
    let scalar_8way_ref =
        ryg_rans_rs_simd::decode_8way_scalar(&encoded_8way, &ref_tables, corpus.data.len())
            .expect("scalar 8-way reference");
    assert_eq!(
        scalar_8way_ref, corpus.data,
        "scalar 8-way preflight: output must match original data"
    );

    // Also verify packed table decoder matches
    let packed_ref = ryg_rans_rs_simd::backends::decode_interleaved8_scalar(
        &encoded_8way,
        &table,
        corpus.data.len(),
    )
    .expect("packed scalar 8-way");
    assert_eq!(
        packed_ref.output, corpus.data,
        "packed scalar 8-way preflight: output must match original"
    );

    let mut group = c.benchmark_group("scalar/scalar-8way-packed/allocating");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));

    group.bench_function("SKEWED_255_1/64KiB", |b| {
        let words: Vec<u16> = encoded_8way.clone();
        b.iter(|| {
            let result = ryg_rans_rs_simd::backends::decode_interleaved8_scalar(
                black_box(&words),
                black_box(&table),
                black_box(corpus.data.len()),
            );
            black_box(result)
        });
    });
    group.finish();

    // True _into benchmark using preallocated buffer and _into API
    let mut into_group = c.benchmark_group("scalar/scalar-8way-packed/into");
    into_group.throughput(Throughput::Bytes(corpus.data.len() as u64));
    into_group.bench_function("SKEWED_255_1/64KiB", |b| {
        let words: Vec<u16> = encoded_8way.clone();
        b.iter_batched(
            || vec![0u8; corpus.data.len()],
            |mut out| {
                let report = ryg_rans_rs_simd::packed_table::decode_8way_packed_scalar_into(
                    black_box(&words),
                    black_box(&table),
                    black_box(&mut out),
                )
                .expect("scalar 8-way into");
                black_box((out, report))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    into_group.finish();
}

fn bench_decode_16way_scalar(c: &mut Criterion) {
    for profile in &[
        ModelProfile::Uniform256,
        ModelProfile::Skewed2551,
        ModelProfile::RenormBoundary,
        ModelProfile::Freq1Residual,
        ModelProfile::Sparse2,
        ModelProfile::IncompressibleLike,
    ] {
        for size in &[65536u64, 262144, 1048576] {
            let corpus = Corpus::generate(*profile, *size as usize, 42);
            let table = corpus.packed_table();
            let encoded = corpus.encode_16way();
            let words: Vec<u16> = encoded.clone();

            // Verify before timing: output must match original corpus
            let (ref_out, ref_report) =
                ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
                    &words,
                    &table,
                    corpus.data.len(),
                )
                .expect("scalar decode");

            assert_eq!(
                ref_out,
                corpus.data,
                "scalar 16-way preflight: output must match original data (profile={}, size={})",
                profile.label(),
                size
            );

            // Self-verification of report consistency
            let verification = verification::verify_16way(
                "scalar-16way",
                &ref_out,
                ref_report.words_consumed,
                &ref_report.final_states,
                &ref_out,
                ref_report.words_consumed,
                &ref_report.final_states,
            );
            verification::assert_verified(&verification);

            let mut group = c.benchmark_group(format!(
                "scalar/scalar-16way/allocating/{}/{}",
                profile.label(),
                format_size(*size),
            ));
            group.throughput(Throughput::Bytes(corpus.data.len() as u64));

            group.bench_function("iter", |b| {
                b.iter(|| {
                    let result = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
                        black_box(&words),
                        black_box(&table),
                        black_box(corpus.data.len()),
                    );
                    black_box(result)
                });
            });
            group.finish();

            // True _into benchmark using preallocated buffer and _into API.
            let mut into_group = c.benchmark_group(format!(
                "scalar/scalar-16way/into/{}/{}",
                profile.label(),
                format_size(*size),
            ));
            into_group.throughput(Throughput::Bytes(corpus.data.len() as u64));
            into_group.bench_function("iter", |b| {
                b.iter_batched(
                    || vec![0u8; corpus.data.len()],
                    |mut out| {
                        let report =
                            ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar_into(
                                black_box(&words),
                                black_box(&table),
                                black_box(&mut out),
                            )
                            .expect("decode_interleaved16_scalar_into");
                        black_box((out, report))
                    },
                    criterion::BatchSize::SmallInput,
                );
            });
            into_group.finish();
        }
    }
}

fn bench_decode_uniform256_scalar(c: &mut Criterion) {
    let corpus = Corpus::generate(ModelProfile::Uniform256, 1024 * 1024, 42);
    let table = corpus.packed_table();
    let encoded = corpus.encode_16way();
    let words: Vec<u16> = encoded.clone();

    // Reference verification
    let (ref_out, ref_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
        &words,
        &table,
        corpus.data.len(),
    )
    .expect("scalar decode");

    let verify = verification::verify_16way(
        "scalar-16way-uniform256",
        &ref_out,
        ref_report.words_consumed,
        &ref_report.final_states,
        &ref_out,
        ref_report.words_consumed,
        &ref_report.final_states,
    );
    verification::assert_verified(&verify);

    let mut group =
        c.benchmark_group("scalar/uniform256-scalar-specialized/allocating/UNIFORM256/1MiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));
    group.bench_function("uniform256-scalar", |b| {
        b.iter(|| {
            let result = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
                black_box(&words),
                black_box(&table),
                black_box(corpus.data.len()),
            );
            black_box(result)
        });
    });
    group.finish();
}

fn format_size(bytes: u64) -> &'static str {
    match bytes {
        65536 => "64KiB",
        262144 => "256KiB",
        1048576 => "1MiB",
        _ => "other",
    }
}

criterion_group!(
    name = scalar_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(5))
        .sample_size(100);
    targets =
        bench_decode_8way_packed,
        bench_decode_16way_scalar,
        bench_decode_uniform256_scalar,
);

criterion_main!(scalar_benches);
