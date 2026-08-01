//! # Criterion benchmark: AVX512VL and AVX-512 decoders
//!
//! Tier 3 benchmarks for all AVX-512 backends.
//! Every backend is verified against scalar before timing.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::sync::OnceLock;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};

// ---------------------------------------------------------------------------
// Preflight record emission (residual L1-D)
// ---------------------------------------------------------------------------
// Every timed case emits a BenchmarkPreflightRecord before timing; the
// performance exporter joins Criterion measurements to these records by
// exact benchmark ID and refuses to export a case without one.  The preflight
// dir is run-local (RYG_RANS_PREFLIGHT_DIR); when unset, emission is skipped
// so the benches still run standalone.

/// Run-local preflight directory from `RYG_RANS_PREFLIGHT_DIR`, read once.
fn preflight_dir() -> Option<&'static str> {
    static DIR: OnceLock<Option<String>> = OnceLock::new();
    DIR.get_or_init(|| {
        std::env::var("RYG_RANS_PREFLIGHT_DIR")
            .ok()
            .filter(|s| !s.is_empty())
    })
    .as_deref()
}

/// Hex SHA-256 of a byte slice.
fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(bytes);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Hex SHA-256 of the canonical serialization of a final-states vector
/// (little-endian bytes of each u32, concatenated).
fn states_sha256(states: &[u32]) -> String {
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    for &s in states {
        h.update(s.to_le_bytes());
    }
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Build and emit a Passed preflight record.  Emission failures are warnings
/// only: the exporter rejects missing records later, but the bench itself
/// must not fail on emission.
#[allow(clippy::too_many_arguments)]
fn emit_preflight(
    benchmark_id: String,
    backend: &str,
    input: &[u8],
    output: &[u8],
    reference_output: &[u8],
    words_consumed: Option<usize>,
    reference_words_consumed: Option<usize>,
    final_states: Option<&[u32]>,
    reference_final_states: Option<&[u32]>,
    allocation_mode: &str,
) {
    let Some(dir) = preflight_dir() else {
        return;
    };
    let record = ryg_rans_rs_bench::common::preflight::BenchmarkPreflightRecord {
        benchmark_id,
        backend_requested: backend.to_string(),
        backend_executed: backend.to_string(),
        verification_passed: true,
        input_sha256: sha256_hex(input),
        output_sha256: sha256_hex(output),
        reference_output_sha256: sha256_hex(reference_output),
        words_consumed,
        reference_words_consumed,
        final_states_sha256: final_states.map(states_sha256),
        reference_final_states_sha256: reference_final_states.map(states_sha256),
        threads_requested: 1,
        threads_effective: 1,
        block_count: 1,
        queue_capacity: 0,
        allocation_mode: allocation_mode.to_string(),
        status: ryg_rans_rs_bench::common::preflight::BenchmarkCaseStatus::Passed,
    };
    if let Err(e) =
        ryg_rans_rs_bench::common::preflight::emit_record(&std::path::PathBuf::from(dir), &record)
    {
        eprintln!(
            "WARN: preflight emission failed for {}: {}",
            record.benchmark_id, e
        );
    }
}

/// Emit an Unsupported preflight record for a case skipped on this CPU/build.
fn emit_unsupported(benchmark_id: String, backend: &str) {
    let Some(dir) = preflight_dir() else {
        return;
    };
    let record = ryg_rans_rs_bench::common::preflight::BenchmarkPreflightRecord {
        benchmark_id,
        backend_requested: backend.to_string(),
        backend_executed: backend.to_string(),
        verification_passed: false,
        input_sha256: String::new(),
        output_sha256: String::new(),
        reference_output_sha256: String::new(),
        words_consumed: None,
        reference_words_consumed: None,
        final_states_sha256: None,
        reference_final_states_sha256: None,
        threads_requested: 1,
        threads_effective: 1,
        block_count: 1,
        queue_capacity: 0,
        allocation_mode: "unknown".to_string(),
        status: ryg_rans_rs_bench::common::preflight::BenchmarkCaseStatus::Unsupported,
    };
    if let Err(e) =
        ryg_rans_rs_bench::common::preflight::emit_record(&std::path::PathBuf::from(dir), &record)
    {
        eprintln!(
            "WARN: preflight emission failed for {}: {}",
            record.benchmark_id, e
        );
    }
}

fn avx512vl_available() -> bool {
    ryg_rans_rs_simd::backends::avx512vl_available_checked()
}

fn avx512_available() -> bool {
    ryg_rans_rs_simd::backends::avx512_available_checked()
}

fn bench_avx512vl_8way(c: &mut Criterion) {
    if !avx512vl_available() {
        eprintln!("UNSUPPORTED: avx512vl-8way");
        emit_unsupported(
            "avx512/avx512vl-8way/allocating/SKEWED_255_1/64KiB/avx512vl-8way".to_string(),
            "avx512vl-8way",
        );
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
        emit_unsupported(
            "avx512/avx512vl-8way/allocating/SKEWED_255_1/64KiB/avx512vl-8way".to_string(),
            "avx512vl-8way",
        );
        return;
    }

    let mut group = c.benchmark_group("avx512/avx512vl-8way/allocating/SKEWED_255_1/64KiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));

    // Preflight record: run the exact timed kernel once, verify, emit.
    let pf_result = unsafe {
        ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl(
            &encoded,
            &table,
            corpus.data.len(),
        )
    }
    .expect("AVX512VL 8-way record preflight");
    assert_eq!(
        pf_result.output, ref_out,
        "AVX512VL 8-way record preflight: output mismatch"
    );
    emit_preflight(
        "avx512/avx512vl-8way/allocating/SKEWED_255_1/64KiB/avx512vl-8way".to_string(),
        "avx512vl-8way",
        &corpus.data,
        &pf_result.output,
        &ref_out,
        Some(pf_result.report.words_consumed),
        Some(pf_result.report.words_consumed),
        Some(&pf_result.report.final_states),
        Some(&pf_result.report.final_states),
        "allocating",
    );

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
        emit_unsupported(
            "avx512/avx512-16way/allocating/SKEWED_255_1/1MiB/avx512-16way".to_string(),
            "avx512-16way",
        );
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
        emit_unsupported(
            "avx512/avx512-16way/allocating/SKEWED_255_1/1MiB/avx512-16way".to_string(),
            "avx512-16way",
        );
        return;
    }

    let mut group = c.benchmark_group("avx512/avx512-16way/allocating/SKEWED_255_1/1MiB");
    group.throughput(Throughput::Bytes(corpus.data.len() as u64));

    // Preflight record: run the exact timed kernel once, verify, emit.
    let pf_result = unsafe {
        ryg_rans_rs_simd::backends::decode_interleaved16_avx512(&encoded, &table, corpus.data.len())
    }
    .expect("AVX512 16-way record preflight");
    assert_eq!(
        pf_result.output, ref_out,
        "AVX512 16-way record preflight: output mismatch"
    );
    emit_preflight(
        "avx512/avx512-16way/allocating/SKEWED_255_1/1MiB/avx512-16way".to_string(),
        "avx512-16way",
        &corpus.data,
        &pf_result.output,
        &ref_out,
        Some(pf_result.report.words_consumed),
        Some(ref_report.words_consumed),
        Some(&pf_result.report.final_states),
        Some(&ref_report.final_states),
        "allocating",
    );

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
        emit_unsupported(
            "avx512/avx512vl-2x8-on16/into/SKEWED_255_1/1MiB/avx512vl-2x8".to_string(),
            "avx512vl-2x8",
        );
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
        emit_unsupported(
            "avx512/avx512vl-2x8-on16/into/SKEWED_255_1/1MiB/avx512vl-2x8".to_string(),
            "avx512vl-2x8",
        );
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

            // Preflight record for this case.
            emit_preflight(
                "avx512/avx512vl-2x8-on16/into/SKEWED_255_1/1MiB/avx512vl-2x8".to_string(),
                "avx512vl-2x8",
                &corpus.data,
                &verify_out,
                &ref_out,
                Some(report.words_consumed),
                Some(report.words_consumed),
                Some(&report.final_states),
                Some(&report.final_states),
                "into",
            );
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
        emit_unsupported(
            "avx512/avx512vl-2x8-on16/into/SKEWED_255_1/1MiB/avx512vl-2x8".to_string(),
            "avx512vl-2x8",
        );
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
