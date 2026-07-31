//! # Criterion benchmark: AVX512VL and AVX-512 decoders
//!
//! Tier 3 benchmarks for all AVX-512 backends.
//! Every backend is verified against scalar before timing.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

fn avx512vl_available() -> bool {
    ryg_rans_rs_simd::backends::avx512vl_available_checked()
}

fn avx512_available() -> bool {
    ryg_rans_rs_simd::backends::avx512_available_checked()
}

fn bench_avx512vl_8way(c: &mut Criterion) {
    if !avx512vl_available() {
        eprintln!("UNSUPPORTED: avx512vl-8way");
        return;
    }
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 65536, 42);
    let table = corpus.packed_table();
    let encoded =
        ryg_rans_rs_simd::encode_8way_for_test(&corpus.data, &corpus.freqs, &corpus.cum_freqs);

    // Scalar 8-way reference
    let (slots, slot2sym) =
        ryg_rans_rs_simd::build_word_tables(&corpus.freqs, &corpus.cum_freqs, corpus.scale_bits);
    let ref_tables = ryg_rans_rs_simd::RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };
    let ref_out = ryg_rans_rs_simd::decode_8way_scalar(&encoded, &ref_tables, corpus.data.len())
        .expect("scalar 8-way");

    // Verify AVX512VL 8-way
    let avx512vl_ok = match unsafe {
        ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl(
            &encoded,
            &table,
            corpus.data.len(),
        )
    } {
        Ok(result) => {
            assert_eq!(result.output, ref_out, "AVX512VL 8-way verification failed");
            true
        }
        Err(ryg_rans_rs_simd::backends::DecodeError::UnsupportedBackend) => {
            eprintln!("UNSUPPORTED: avx512vl-8way (not compiled with avx512bw)");
            false
        }
        Err(e) => panic!("AVX512VL verify failed: {:?}", e),
    };

    if !avx512vl_ok {
        return;
    }

    let mut group = c.benchmark_group("avx512/avx512vl-8way/allocating/SKEWED_255_1/64KiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));
    group.bench_function("avx512vl-8way", |b| {
        b.iter(|| unsafe {
            let result = ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl(
                black_box(&encoded),
                black_box(&table),
                black_box(corpus.data.len()),
            );
            black_box(result)
        });
    });
    group.finish();
}

fn bench_avx512_16way(c: &mut Criterion) {
    if !avx512_available() {
        eprintln!("UNSUPPORTED: avx512-16way");
        return;
    }
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 1048576, 42);
    let table = corpus.packed_table();
    let encoded = corpus.encode_16way();

    // Scalar reference
    let (ref_out, ref_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
        &encoded,
        &table,
        corpus.data.len(),
    )
    .expect("scalar decode");

    // Verify AVX512 16-way
    let avx512_16way_ok = match unsafe {
        ryg_rans_rs_simd::backends::decode_interleaved16_avx512(&encoded, &table, corpus.data.len())
    } {
        Ok(result) => {
            assert_eq!(result.output, ref_out, "AVX512 16-way verification failed");
            assert_eq!(
                result.report.words_consumed, ref_report.words_consumed,
                "AVX512 16-way words consumed mismatch"
            );
            true
        }
        Err(ryg_rans_rs_simd::backends::DecodeError::UnsupportedBackend) => {
            eprintln!("UNSUPPORTED: avx512-16way (not compiled with avx512bw)");
            false
        }
        Err(e) => panic!("AVX512 16-way verify failed: {:?}", e),
    };

    if !avx512_16way_ok {
        return;
    }

    let mut group = c.benchmark_group("avx512/avx512-16way/allocating/SKEWED_255_1/1MiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));
    group.bench_function("avx512-16way", |b| {
        b.iter(|| unsafe {
            let result = ryg_rans_rs_simd::backends::decode_interleaved16_avx512(
                black_box(&encoded),
                black_box(&table),
                black_box(corpus.data.len()),
            );
            black_box(result)
        });
    });
    group.finish();
}

fn bench_avx512vl_2x8(_c: &mut Criterion) {
    if !avx512vl_available() {
        eprintln!("UNSUPPORTED: avx512vl-2x8-on16");
        return;
    }
    let corpus = Corpus::generate(ModelProfile::Skewed2551, 1048576, 42);
    let table = corpus.packed_table();
    let encoded = corpus.encode_16way();

    // Scalar reference
    let (ref_out, _ref_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
        &encoded,
        &table,
        corpus.data.len(),
    )
    .expect("scalar decode");

    // Check AVX512 availability at compile+time
    let avx512_ok = match unsafe {
        ryg_rans_rs_simd::backends::decode_interleaved16_avx512(&encoded, &table, corpus.data.len())
    } {
        Ok(result) => {
            assert_eq!(result.output, ref_out, "AVX512 16-way verification failed");
            true
        }
        Err(ryg_rans_rs_simd::backends::DecodeError::UnsupportedBackend) => {
            eprintln!("UNSUPPORTED: avx512-16way (not compiled with avx512bw)");
            false
        }
        Err(e) => panic!("AVX512 verify failed: {:?}", e),
    };

    if !avx512_ok {
        return;
    }

    // Verify AVX512VL 2x8 (only if compiled with avx512bw)
    #[cfg(target_feature = "avx512bw")]
    {
        // The criterion handle is only used in this cfg-gated section; bind
        // it here so the default build has no unused-parameter warning.
        let c = _c;
        unsafe {
            let mut verify_out = vec![0u8; corpus.data.len()];
            let report = ryg_rans_rs_simd::avx512::decode_interleaved16_2x8_into(
                &encoded,
                &table,
                &mut verify_out,
            )
            .expect("AVX512VL 2x8 verify");
            assert_eq!(verify_out, ref_out, "AVX512VL 2x8 verification failed");
            let _ = report;
        }

        let mut group = c.benchmark_group("avx512/avx512vl-2x8-on16/into/SKEWED_255_1/1MiB");
        group.throughput(Throughput::Bytes(corpus.data.len() as u64));
        group.bench_function("avx512vl-2x8", |b| {
            let output = vec![0u8; corpus.data.len()];
            b.iter_batched(
                || output.clone(),
                |mut out| unsafe {
                    let report = ryg_rans_rs_simd::avx512::decode_interleaved16_2x8_into(
                        black_box(&encoded),
                        black_box(&table),
                        black_box(&mut out),
                    )
                    .expect("AVX512VL 2x8");
                    black_box(report)
                },
                criterion::BatchSize::SmallInput,
            );
        });
        group.finish();
    }
    #[cfg(not(target_feature = "avx512bw"))]
    {
        eprintln!("UNSUPPORTED: avx512vl-2x8-on16 (not compiled with avx512bw target feature)");
    }
}

criterion_group!(
    name = avx512_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(8))
        .sample_size(50);
    targets =
        bench_avx512vl_8way,
        bench_avx512_16way,
        bench_avx512vl_2x8,
);

criterion_main!(avx512_benches);
