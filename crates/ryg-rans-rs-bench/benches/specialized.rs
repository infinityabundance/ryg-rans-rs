//! # Criterion benchmark: Model-specialized decode paths
//!
//! Tier 4 benchmarks measuring model-specific kernels:
//! - Uniform256 scalar, AVX2, AVX-512
//! - Future: dominant-symbol, sparse-model, binary-model

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_bench::common::verification;

fn avx2_available() -> bool {
    ryg_rans_rs_simd::backends::avx2_available_checked()
}

fn bench_uniform256_scalar(c: &mut Criterion) {
    let corpus = Corpus::generate(ModelProfile::Uniform256, 1048576, 42);
    let table = corpus.packed_table();
    let encoded = corpus.encode_16way();

    let mut group = c.benchmark_group("specialized/uniform256/scalar/into/UNIFORM256/1MiB");
    group.throughput(Throughput::Bytes(1048576));

    // Preflight: verify using the allocating API
    let (ref_out, _ref_report) =
        ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(&encoded, &table, 1048576)
            .expect("scalar decode preflight");
    assert_eq!(
        ref_out, corpus.data,
        "specialized uniform256 scalar preflight: output must match original"
    );

    group.bench_function("scalar-16way-uniform256-into", |b| {
        let output = vec![0u8; 1048576];
        b.iter_batched(
            || output.clone(),
            |mut out| {
                let report = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar_into(
                    black_box(&encoded),
                    black_box(&table),
                    black_box(&mut out),
                )
                .expect("scalar decode into");
                black_box((out, report))
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_uniform256_avx2(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: uniform256/avx2");
        return;
    }
    let corpus = Corpus::generate(ModelProfile::Uniform256, 1048576, 42);
    let encoded = corpus.encode_16way();
    let pt = ryg_rans_rs_simd::avx2_renorm::build_avx2_renorm_table();

    // Verify against scalar
    let (ref_out, ref_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
        &encoded,
        &corpus.packed_table(),
        corpus.data.len(),
    )
    .expect("scalar ref");

    unsafe {
        let mut v_out = vec![0u8; 1048576];
        let v_rep = ryg_rans_rs_simd::avx2::decode_interleaved16_uniform256_avx2_into(
            &encoded, &mut v_out, &pt,
        )
        .expect("AVX2 uniform256 verify");
        let vr = verification::verify_16way(
            "avx2-uniform256",
            &v_out,
            v_rep.words_consumed,
            &v_rep.final_states,
            &ref_out,
            ref_report.words_consumed,
            &ref_report.final_states,
        );
        verification::assert_verified(&vr);
    }

    let mut group = c.benchmark_group("specialized/uniform256/avx2/into/UNIFORM256/1MiB");
    group.throughput(Throughput::Bytes(1048576));

    group.bench_function("avx2-tablefree-uniform256", |b| {
        let output = vec![0u8; 1048576];
        b.iter_batched(
            || output.clone(),
            |mut out| unsafe {
                let report = ryg_rans_rs_simd::avx2::decode_interleaved16_uniform256_avx2_into(
                    black_box(&encoded),
                    black_box(&mut out),
                    black_box(&pt),
                )
                .expect("AVX2 uniform256");
                black_box(report)
            },
            criterion::BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    name = specialized_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(8))
        .sample_size(50);
    targets =
        bench_uniform256_scalar,
        bench_uniform256_avx2,
);

criterion_main!(specialized_benches);
