//! # Criterion benchmark: Dispatch overhead
//!
//! Tier 8 benchmarks measuring the cost of runtime CPU detection,
//! decode-plan construction, model classification, and safe wrapper dispatch.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::time::Duration;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

fn bench_cpu_detection(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch/cpu-detection/overhead");
    group.measurement_time(Duration::from_secs(5));
    group.sample_size(100);

    group.bench_function("avx2-available", |b| {
        b.iter(|| {
            let result = ryg_rans_rs_simd::backends::avx2_available_checked();
            black_box(result)
        });
    });

    group.bench_function("avx512vl-available", |b| {
        b.iter(|| {
            let result = ryg_rans_rs_simd::backends::avx512vl_available_checked();
            black_box(result)
        });
    });

    group.finish();
}

fn bench_model_classification(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch/model-classification/overhead");

    // Uniform model data (easy classification)
    let mut uniform_data = Vec::with_capacity(1024);
    for _ in 0..256 {
        uniform_data.extend_from_slice(&16u32.to_le_bytes());
    }

    // Skewed model data (harder classification)
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 4096, 42);
    let mut skewed_data = Vec::with_capacity(1024);
    for &f in &corpus.freqs {
        skewed_data.extend_from_slice(&f.to_le_bytes());
    }

    group.bench_function("uniform256-detection/uniform", |b| {
        b.iter(|| {
            let result = ryg_rans_rs_simd::backends::check_uniform256(
                black_box(&uniform_data),
                black_box(12),
            );
            black_box(result)
        });
    });

    group.bench_function("uniform256-detection/skewed", |b| {
        b.iter(|| {
            let result = ryg_rans_rs_simd::backends::check_uniform256(
                black_box(&skewed_data),
                black_box(12),
            );
            black_box(result)
        });
    });

    group.finish();
}

fn bench_safe_wrapper_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatch/safe-wrapper/overhead");
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 4096, 42);
    let table = corpus.packed_table();
    let encoded = corpus.encode_16way();

    // Only register AVX2 benchmarks if AVX2 is available at runtime.
    let avx2_ok = ryg_rans_rs_simd::backends::avx2_available_checked();

    if avx2_ok {
        // Direct unsafe call — requires AVX2 runtime.
        group.bench_function("avx2-2x8-unsafe-direct", |b| {
            let perm_table = ryg_rans_rs_simd::avx2_renorm::build_avx2_renorm_table();
            b.iter(|| {
                let mut output = vec![0u8; corpus.data.len()];
                unsafe {
                    let report = ryg_rans_rs_simd::avx2::decode_interleaved16_avx2_2x8_into(
                        black_box(&encoded),
                        black_box(&table),
                        black_box(&mut output),
                        black_box(&perm_table),
                    )
                    .expect("direct");
                    black_box(report)
                }
            });
        });
    } else {
        eprintln!("UNSUPPORTED: avx2-2x8-unsafe-direct (AVX2 not available)");
    }

    // Safe checked wrapper — handles detection internally.
    group.bench_function("avx2-2x8-safe-checked", |b| {
        b.iter(|| {
            let result = ryg_rans_rs_simd::backends::decode_interleaved16_avx2_2x8_checked(
                black_box(&encoded),
                black_box(&table),
                black_box(corpus.data.len()),
            );
            black_box(result)
        });
    });

    group.finish();
}

criterion_group!(
    name = dispatch_benches;
    config = Criterion::default()
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(5))
        .sample_size(100);
    targets =
        bench_cpu_detection,
        bench_model_classification,
        bench_safe_wrapper_overhead,
);

criterion_main!(dispatch_benches);
