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
use ryg_rans_rs_simd::avx2_renorm::build_avx2_renorm_table;
use std::sync::OnceLock;

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
        allocation_mode: "into".to_string(),
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

/// Check AVX2 runtime support via the SIMD crate.
fn avx2_available() -> bool {
    ryg_rans_rs_simd::backends::avx2_available_checked()
}

fn bench_avx2_2x8(c: &mut Criterion) {
    if !avx2_available() {
        eprintln!("UNSUPPORTED: avx2-2x8-on16 (AVX2 not available on this host)");
        emit_unsupported(
            "avx2/avx2-2x8-on16/into/UNIFORM256/64KiB/iter".to_string(),
            "avx2-2x8",
        );
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

            // Preflight record: run the exact timed kernel once, verify, emit.
            let mut pf_out = vec![0u8; corpus.data.len()];
            let pf_report = unsafe {
                ryg_rans_rs_simd::avx2::decode_interleaved16_avx2_2x8_into(
                    &encoded,
                    &table,
                    &mut pf_out,
                    &perm_table,
                )
            }
            .expect("AVX2 2x8 record preflight");
            assert_eq!(
                pf_out, corpus.data,
                "AVX2 2x8 record preflight: output must match original"
            );
            emit_preflight(
                format!("{}/iter", group_label),
                "avx2-2x8",
                &corpus.data,
                &pf_out,
                &corpus.data,
                Some(pf_report.words_consumed),
                Some(ref_report.words_consumed),
                Some(&pf_report.final_states),
                Some(&ref_report.final_states),
            );

            group.bench_function("iter", |b| {
                let output = vec![0u8; corpus.data.len()];
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
        emit_unsupported(
            "avx2/avx2-uniform256-tablefree-16way/into/UNIFORM256/1MiB/avx2-uniform256".to_string(),
            "avx2-uniform256",
        );
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

    // Preflight record: run the exact timed kernel once, verify, emit.
    let mut pf_out = vec![0u8; corpus.data.len()];
    let pf_report = unsafe {
        ryg_rans_rs_simd::avx2::decode_interleaved16_uniform256_avx2_into(
            &encoded,
            &mut pf_out,
            &perm_table,
        )
    }
    .expect("AVX2 uniform256 record preflight");
    assert_eq!(
        pf_out, corpus.data,
        "AVX2 uniform256 record preflight: output must match original"
    );
    emit_preflight(
        "avx2/avx2-uniform256-tablefree-16way/into/UNIFORM256/1MiB/avx2-uniform256".to_string(),
        "avx2-uniform256",
        &corpus.data,
        &pf_out,
        &corpus.data,
        Some(pf_report.words_consumed),
        Some(ref_report.words_consumed),
        Some(&pf_report.final_states),
        Some(&ref_report.final_states),
    );

    group.bench_function("avx2-uniform256", |b| {
        let output = vec![0u8; corpus.data.len()];
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
        emit_unsupported(
            "avx2/avx2-manual-gather-8way/into/SKEWED_255_1/64KiB/avx2-manual-gather".to_string(),
            "avx2-manual-gather",
        );
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

    // Preflight record: run the exact timed kernel once, verify, emit.
    let mut pf_out = vec![0u8; corpus.data.len()];
    let pf_report = unsafe {
        ryg_rans_rs_simd::avx2::decode_interleaved8_avx2_manual_gather_into(
            &encoded,
            &table,
            &mut pf_out,
            &perm_table,
        )
    }
    .expect("AVX2 manual gather record preflight");
    assert_eq!(
        pf_out, corpus.data,
        "AVX2 manual gather record preflight: output must match original"
    );
    emit_preflight(
        "avx2/avx2-manual-gather-8way/into/SKEWED_255_1/64KiB/avx2-manual-gather".to_string(),
        "avx2-manual-gather",
        &corpus.data,
        &pf_out,
        &corpus.data,
        Some(pf_report.words_consumed),
        Some(ref_report8.words_consumed),
        Some(&pf_report.final_states[..8]),
        Some(&ref_report8.final_states),
    );

    group.bench_function("avx2-manual-gather", |b| {
        let output = vec![0u8; corpus.data.len()];
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
        emit_unsupported(
            "avx2/avx2-hardware-gather-8way/into/SKEWED_255_1/64KiB/avx2-hw-gather".to_string(),
            "avx2-hardware-gather",
        );
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

    // Preflight record: run the exact timed kernel once, verify, emit.
    let mut pf_out = vec![0u8; corpus.data.len()];
    let pf_report = unsafe {
        ryg_rans_rs_simd::avx2::decode_interleaved8_avx2_hardware_gather_into(
            &encoded,
            &table,
            &mut pf_out,
            &perm_table,
        )
    }
    .expect("AVX2 HW gather record preflight");
    assert_eq!(
        pf_out, corpus.data,
        "AVX2 HW gather record preflight: output must match original"
    );
    emit_preflight(
        "avx2/avx2-hardware-gather-8way/into/SKEWED_255_1/64KiB/avx2-hw-gather".to_string(),
        "avx2-hardware-gather",
        &corpus.data,
        &pf_out,
        &corpus.data,
        Some(pf_report.words_consumed),
        Some(ref_report8.words_consumed),
        Some(&pf_report.final_states[..8]),
        Some(&ref_report8.final_states),
    );

    group.bench_function("avx2-hw-gather", |b| {
        let output = vec![0u8; corpus.data.len()];
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
