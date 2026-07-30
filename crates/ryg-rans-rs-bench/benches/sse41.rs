//! # Criterion benchmark: SSE4.1 decoder
//!
//! Tier 1 benchmarks for SSE4.1 8-way interleaved decode.
//! Verified against scalar 8-way reference before timing.
//! Uses the REAL target-feature-gated function after runtime detection,
//! so actual SSE4.1 instructions execute even on portable builds.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

fn sse41_available() -> bool {
    // The real SSE4.1 kernel requires both SSSE3 and SSE4.1 at runtime.
    // The bench crate always has std, so we always use runtime detection.
    std::is_x86_feature_detected!("ssse3") && std::is_x86_feature_detected!("sse4.1")
}

fn bench_sse41_8way(c: &mut Criterion) {
    if !sse41_available() {
        eprintln!("UNSUPPORTED: sse41-8way (SSSE3+SSE4.1 not available)");
        return;
    }

    let corpus = Corpus::generate(ModelProfile::Skewed2551, 262144, 42);
    let encoded =
        ryg_rans_rs_simd::encode_8way_for_test(&corpus.data, &corpus.freqs, &corpus.cum_freqs);

    // Build slot-based word tables (SSE4.1 uses RansWordTables, not PackedWordTable)
    let (slots, slot2sym) =
        ryg_rans_rs_simd::build_word_tables(&corpus.freqs, &corpus.cum_freqs, corpus.scale_bits);
    let tables = ryg_rans_rs_simd::RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };

    // Scalar reference
    let ref_out = ryg_rans_rs_simd::decode_8way_scalar(&encoded, &tables, corpus.data.len())
        .expect("scalar 8-way reference");
    assert_eq!(
        ref_out, corpus.data,
        "SSE4.1 preflight: scalar reference must match original"
    );

    // SSE4.1 verification — call the target-feature-gated function AFTER runtime check.
    // decode_simd_8way_unchecked has #[target_feature(enable = "ssse3,sse4.1")] so it
    // executes real SSE4.1 instructions even on a portable binary.
    let simd_out = unsafe {
        ryg_rans_rs_simd::decode_simd_8way_unchecked(&encoded, &tables, corpus.data.len())
    }
    .expect("SSE4.1 verify");
    assert_eq!(
        simd_out, corpus.data,
        "SSE4.1 vs original: output must match"
    );

    let mut group = c.benchmark_group("sse41/sse41-8way/allocating/SKEWED_255_1/256KiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));

    group.bench_function("sse41-8way", |b| {
        let words: Vec<u16> = encoded.clone();
        b.iter(|| unsafe {
            let result = ryg_rans_rs_simd::decode_simd_8way_unchecked(
                black_box(&words),
                black_box(&tables),
                black_box(corpus.data.len()),
            );
            black_box(result)
        });
    });
    group.finish();
}

criterion_group!(
    name = sse41_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(5))
        .sample_size(100);
    targets = bench_sse41_8way,
);

criterion_main!(sse41_benches);
