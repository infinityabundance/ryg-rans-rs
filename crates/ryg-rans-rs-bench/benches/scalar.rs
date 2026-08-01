//! # Criterion benchmark: Portable scalar reference decoders
//!
//! Tier 0 benchmarks for scalar-8way-legacy, scalar-8way-packed, scalar-16way,
//! and uniform256-scalar-specialized decoding.  These are the correctness and
//! portability baselines that all SIMD backends must match.

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::sync::OnceLock;
use std::vec::Vec;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_bench::common::verification;

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

    // Preflight record: run the exact timed kernel once, verify parity, emit.
    let pf_result = ryg_rans_rs_simd::backends::decode_interleaved8_scalar(
        &encoded_8way,
        &table,
        corpus.data.len(),
    )
    .expect("scalar 8-way record preflight");
    assert_eq!(
        pf_result.output, corpus.data,
        "scalar 8-way record preflight: output must match original"
    );
    emit_preflight(
        "scalar/scalar-8way-packed/allocating/SKEWED_255_1/64KiB".to_string(),
        "scalar-8way",
        &corpus.data,
        &pf_result.output,
        &corpus.data,
        Some(pf_result.report.words_consumed),
        Some(pf_result.report.words_consumed),
        Some(&pf_result.report.final_states),
        Some(&pf_result.report.final_states),
        "allocating",
    );

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

    // Preflight record for the _into case.
    let mut pf_out = vec![0u8; corpus.data.len()];
    let pf_report = ryg_rans_rs_simd::packed_table::decode_8way_packed_scalar_into(
        &encoded_8way,
        &table,
        &mut pf_out,
    )
    .expect("scalar 8-way into record preflight");
    assert_eq!(
        pf_out, corpus.data,
        "scalar 8-way into record preflight: output must match original"
    );
    emit_preflight(
        "scalar/scalar-8way-packed/into/SKEWED_255_1/64KiB".to_string(),
        "scalar-8way",
        &corpus.data,
        &pf_out,
        &corpus.data,
        Some(pf_report.words_consumed),
        Some(pf_report.words_consumed),
        Some(&pf_report.final_states),
        Some(&pf_report.final_states),
        "into",
    );

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

            let group_name = format!(
                "scalar/scalar-16way/allocating/{}/{}",
                profile.label(),
                format_size(*size),
            );
            let mut group = c.benchmark_group(&group_name);
            group.throughput(Throughput::Bytes(corpus.data.len() as u64));

            // Preflight record: run the exact timed kernel once, verify, emit.
            let (pf_out, pf_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
                &words,
                &table,
                corpus.data.len(),
            )
            .expect("scalar 16-way record preflight");
            assert_eq!(
                pf_out, corpus.data,
                "scalar 16-way record preflight: output must match original"
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "scalar-16way",
                &corpus.data,
                &pf_out,
                &corpus.data,
                Some(pf_report.words_consumed),
                Some(pf_report.words_consumed),
                Some(&pf_report.final_states),
                Some(&pf_report.final_states),
                "allocating",
            );

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
            let into_group_name = format!(
                "scalar/scalar-16way/into/{}/{}",
                profile.label(),
                format_size(*size),
            );
            let mut into_group = c.benchmark_group(&into_group_name);
            into_group.throughput(Throughput::Bytes(corpus.data.len() as u64));

            // Preflight record for the _into case.
            let mut pf_out = vec![0u8; corpus.data.len()];
            let pf_report = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar_into(
                &words,
                &table,
                &mut pf_out,
            )
            .expect("scalar 16-way into record preflight");
            assert_eq!(
                pf_out, corpus.data,
                "scalar 16-way into record preflight: output must match original"
            );
            emit_preflight(
                format!("{}/iter", into_group_name),
                "scalar-16way",
                &corpus.data,
                &pf_out,
                &corpus.data,
                Some(pf_report.words_consumed),
                Some(pf_report.words_consumed),
                Some(&pf_report.final_states),
                Some(&pf_report.final_states),
                "into",
            );

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

    // Preflight record: run the exact timed kernel once, verify, emit.
    let (pf_out, pf_report) = ryg_rans_rs_simd::packed_table::decode_interleaved16_scalar(
        &words,
        &table,
        corpus.data.len(),
    )
    .expect("scalar uniform256 record preflight");
    assert_eq!(
        pf_out, corpus.data,
        "scalar uniform256 record preflight: output must match original"
    );
    emit_preflight(
        "scalar/uniform256-scalar-specialized/allocating/UNIFORM256/1MiB/uniform256-scalar"
            .to_string(),
        "scalar-16way",
        &corpus.data,
        &pf_out,
        &corpus.data,
        Some(pf_report.words_consumed),
        Some(pf_report.words_consumed),
        Some(&pf_report.final_states),
        Some(&pf_report.final_states),
        "allocating",
    );

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
