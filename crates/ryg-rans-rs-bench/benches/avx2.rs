//! # Criterion benchmark: AVX2 decoders
//!
//! Tier 2 benchmarks for all AVX2 backends:
//! - avx2-manual-gather-8way
//! - avx2-hardware-gather-8way
//! - avx2-2x8-on16
//! - avx2-uniform256-tablefree-16way
//! - avx2-batch4-on16
//!
//! Every backend is verified against its scalar reference before timing.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use ryg_rans_rs_simd::avx2::Avx2DecodeJob;
use ryg_rans_rs_simd::avx2_renorm::{Avx2RenormPermutations, build_avx2_renorm_table};
use std::vec::Vec;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_bench::common::verification;

/// Check AVX2 runtime support via the SIMD crate.
fn avx2_available() -> bool {
    ryg_rans_rs_simd::backends::avx2_available_checked()
}

fn bench_avx2_2x8(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: avx2-2x8-on16 (AVX2 not available on this host)");
        return;
    }
    let perm_table = build_avx2_renorm_table();

    for profile in &[
        ModelProfile::Uniform256,
        ModelProfile::Skewed2551,
        ModelProfile::RenormBoundary,
        ModelProfile::Freq1Residual,
    ] {
        for size in &[65536u64, 262144, 1048576] {
            let corpus = Corpus::generate(*profile, *size as usize, 42);
            let table = corpus.packed_table();
            let encoded = corpus.encode_16way();

            // Scalar reference for verification
            let (ref_out, ref_report) =
                ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
                    &encoded,
                    &table,
                    corpus.data.len(),
                )
                .expect("scalar decode");

            // Verify AVX2 2x8 before timing
            unsafe {
                let mut verify_out = vec![0u8; corpus.data.len()];
                let verify_report = ryg_rans_rs_simd::avx2::decode_interleaved16_avx2_2x8_into(
                    &encoded,
                    &table,
                    &mut verify_out,
                    &perm_table,
                )
                .expect("AVX2 2x8 verify");

                let vr = verification::verify_16way(
                    "avx2-2x8-on16",
                    &verify_out,
                    verify_report.words_consumed,
                    &verify_report.final_states,
                    &ref_out,
                    ref_report.words_consumed,
                    &ref_report.final_states,
                );
                verification::assert_verified(&vr);
            }

            let group_label = format!(
                "avx2/avx2-2x8-on16/into/{}/{}",
                profile.label(),
                format_size(*size)
            );

            let mut group = c.benchmark_group(&group_label);
            group.throughput(Throughput::Bytes(corpus.data.len() as u64));

            group.bench_function("iter", |b| {
                let mut output = vec![0u8; corpus.data.len()];
                b.iter_batched(
                    || output.clone(),
                    |mut out| unsafe {
                        let report = ryg_rans_rs_simd::avx2::decode_interleaved16_avx2_2x8_into(
                            black_box(&encoded),
                            black_box(&table),
                            black_box(&mut out),
                            black_box(&perm_table),
                        )
                        .expect("AVX2 2x8");
                        black_box(report)
                    },
                    criterion::BatchSize::SmallInput,
                );
            });
            group.finish();
        }
    }
}

fn bench_avx2_uniform256(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: avx2-uniform256-tablefree-16way (AVX2 not available)");
        return;
    }
    let perm_table = build_avx2_renorm_table();
    let corpus = Corpus::generate(ModelProfile::Uniform256, 1048576, 42);
    let encoded = corpus.encode_16way();

    // Scalar reference
    let (ref_out, ref_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
        &encoded,
        &corpus.packed_table(),
        corpus.data.len(),
    )
    .expect("scalar decode");

    // Verify AVX2 uniform256
    unsafe {
        let mut verify_out = vec![0u8; corpus.data.len()];
        let verify_report = ryg_rans_rs_simd::avx2::decode_interleaved16_uniform256_avx2_into(
            &encoded,
            &mut verify_out,
            &perm_table,
        )
        .expect("AVX2 uniform256 verify");

        let vr = verification::verify_16way(
            "avx2-uniform256-tablefree-16way",
            &verify_out,
            verify_report.words_consumed,
            &verify_report.final_states,
            &ref_out,
            ref_report.words_consumed,
            &ref_report.final_states,
        );
        verification::assert_verified(&vr);
    }

    let mut group = c.benchmark_group("avx2/avx2-uniform256-tablefree-16way/into/UNIFORM256/1MiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));
    group.bench_function("avx2-uniform256", |b| {
        let mut output = vec![0u8; corpus.data.len()];
        b.iter_batched(
            || output.clone(),
            |mut out| unsafe {
                let report = ryg_rans_rs_simd::avx2::decode_interleaved16_uniform256_avx2_into(
                    black_box(&encoded),
                    black_box(&mut out),
                    black_box(&perm_table),
                )
                .expect("AVX2 uniform256");
                black_box(report)
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_avx2_manual_gather(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: avx2-manual-gather-8way (AVX2 not available)");
        return;
    }
    // 8-way benchmark uses 8-way encoded data
    let perm_table = build_avx2_renorm_table();
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 65536, 42);
    let table = corpus.packed_table();
    let encoded =
        ryg_rans_rs_simd::encode_8way_for_test(&corpus.data, &corpus.freqs, &corpus.cum_freqs);

    // Scalar 8-way reference with full report (output + words consumed + final states)
    let (ref_out, ref_report8) =
        ryg_rans_rs_simd::packed_table::decode_8way_packed_scalar_with_report(
            &encoded,
            &table,
            corpus.data.len(),
        )
        .expect("scalar 8-way packed reference");

    // Build a 16-element final_states from the 8-element ref for verify_8way compatibility
    let mut ref_final_states = [0u32; 16];
    for i in 0..8 {
        ref_final_states[i] = ref_report8.final_states[i];
    }

    // Verify manual gather: output, words consumed, AND final states
    unsafe {
        let mut verify_out = vec![0u8; corpus.data.len()];
        let verify_report = ryg_rans_rs_simd::avx2::decode_interleaved8_avx2_manual_gather_into(
            &encoded,
            &table,
            &mut verify_out,
            &perm_table,
        )
        .expect("AVX2 manual gather verify");

        let vr = verification::verify_8way(
            "avx2-manual-gather-8way",
            &verify_out,
            verify_report.words_consumed,
            &verify_report.final_states,
            &ref_out,
            ref_report8.words_consumed,
            &ref_final_states[..8].try_into().unwrap(),
        );
        verification::assert_verified(&vr);
    }

    let mut group = c.benchmark_group("avx2/avx2-manual-gather-8way/into/SKEWED_255_1/64KiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));
    group.bench_function("avx2-manual-gather", |b| {
        let mut output = vec![0u8; corpus.data.len()];
        b.iter_batched(
            || output.clone(),
            |mut out| unsafe {
                let report = ryg_rans_rs_simd::avx2::decode_interleaved8_avx2_manual_gather_into(
                    black_box(&encoded),
                    black_box(&table),
                    black_box(&mut out),
                    black_box(&perm_table),
                )
                .expect("AVX2 manual gather");
                black_box(report)
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_avx2_hardware_gather(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: avx2-hardware-gather-8way (AVX2 not available)");
        return;
    }
    let perm_table = build_avx2_renorm_table();
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 65536, 42);
    let table = corpus.packed_table();
    let encoded =
        ryg_rans_rs_simd::encode_8way_for_test(&corpus.data, &corpus.freqs, &corpus.cum_freqs);

    // Scalar 8-way reference with full report
    let (ref_out, ref_report8) =
        ryg_rans_rs_simd::packed_table::decode_8way_packed_scalar_with_report(
            &encoded,
            &table,
            corpus.data.len(),
        )
        .expect("scalar 8-way packed reference");

    let mut ref_final_states = [0u32; 16];
    for i in 0..8 {
        ref_final_states[i] = ref_report8.final_states[i];
    }

    // Verify hardware gather: output, words consumed, AND final states
    unsafe {
        let mut verify_out = vec![0u8; corpus.data.len()];
        let verify_report = ryg_rans_rs_simd::avx2::decode_interleaved8_avx2_hardware_gather_into(
            &encoded,
            &table,
            &mut verify_out,
            &perm_table,
        )
        .expect("AVX2 HW gather verify");

        let vr = verification::verify_8way(
            "avx2-hardware-gather-8way",
            &verify_out,
            verify_report.words_consumed,
            &verify_report.final_states,
            &ref_out,
            ref_report8.words_consumed,
            &ref_final_states[..8].try_into().unwrap(),
        );
        verification::assert_verified(&vr);
    }

    let mut group = c.benchmark_group("avx2/avx2-hardware-gather-8way/into/SKEWED_255_1/64KiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));
    group.bench_function("avx2-hw-gather", |b| {
        let mut output = vec![0u8; corpus.data.len()];
        b.iter_batched(
            || output.clone(),
            |mut out| unsafe {
                let report = ryg_rans_rs_simd::avx2::decode_interleaved8_avx2_hardware_gather_into(
                    black_box(&encoded),
                    black_box(&table),
                    black_box(&mut out),
                    black_box(&perm_table),
                )
                .expect("AVX2 HW gather");
                black_box(report)
            },
            criterion::BatchSize::SmallInput,
        );
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
    name = avx2_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(8))
        .sample_size(50);
    targets =
        bench_avx2_2x8,
        bench_avx2_uniform256,
        bench_avx2_manual_gather,
        bench_avx2_hardware_gather,
);

criterion_main!(avx2_benches);
