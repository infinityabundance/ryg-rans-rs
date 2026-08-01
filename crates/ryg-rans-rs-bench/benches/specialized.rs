//! # Criterion benchmark: Model-specialized decode paths
//!
//! Tier 4 benchmarks measuring model-specific kernels:
//! - Uniform256 scalar, AVX2, AVX-512
//! - Future: dominant-symbol, sparse-model, binary-model

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_bench::common::preflight::{
    BenchmarkCaseStatus, BenchmarkPreflightRecord, emit_record,
};
use ryg_rans_rs_bench::common::verification;
use sha2::Digest;
use std::path::PathBuf;
use std::sync::OnceLock;

/// Run-local preflight directory from `RYG_RANS_PREFLIGHT_DIR`, read once.
static PREFLIGHT_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

fn preflight_dir() -> Option<PathBuf> {
    PREFLIGHT_DIR
        .get_or_init(|| {
            std::env::var("RYG_RANS_PREFLIGHT_DIR")
                .ok()
                .map(PathBuf::from)
        })
        .clone()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = sha2::Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Emit the preflight record for one case (before timing).  Failures are
/// warnings only — the exporter rejects missing records later.
fn emit(
    benchmark_id: &str,
    backend: &str,
    input: &[u8],
    output: &[u8],
    reference: &[u8],
    words: Option<usize>,
    ref_words: Option<usize>,
    states: Option<&[u32]>,
    ref_states: Option<&[u32]>,
) {
    let Some(dir) = preflight_dir() else { return };
    let final_states_sha256 = states.map(|s| sha256_hex(&states_le_bytes(s)));
    let reference_final_states_sha256 = ref_states.map(|s| sha256_hex(&states_le_bytes(s)));
    let record = BenchmarkPreflightRecord {
        benchmark_id: benchmark_id.to_string(),
        backend_requested: backend.to_string(),
        backend_executed: backend.to_string(),
        verification_passed: true,
        input_sha256: sha256_hex(input),
        output_sha256: sha256_hex(output),
        reference_output_sha256: sha256_hex(reference),
        words_consumed: words,
        reference_words_consumed: ref_words,
        final_states_sha256,
        reference_final_states_sha256,
        threads_requested: 1,
        threads_effective: 1,
        block_count: 1,
        queue_capacity: 0,
        allocation_mode: "into".to_string(),
        status: BenchmarkCaseStatus::Passed,
    };
    if let Err(e) = emit_record(&dir, &record) {
        eprintln!("WARN: preflight emit {}: {}", benchmark_id, e);
    }
}

fn states_le_bytes(states: &[u32]) -> Vec<u8> {
    states.iter().flat_map(|s| s.to_le_bytes()).collect()
}

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
    emit(
        "specialized/uniform256/scalar/into/UNIFORM256/1MiB/scalar-16way-uniform256-into",
        "scalar-16way-uniform256",
        &corpus.data,
        &ref_out,
        &corpus.data,
        Some(_ref_report.words_consumed),
        Some(_ref_report.words_consumed),
        Some(&_ref_report.final_states),
        Some(&_ref_report.final_states),
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
        emit(
            "specialized/uniform256/avx2/into/UNIFORM256/1MiB/avx2-tablefree-uniform256",
            "avx2-uniform256",
            &corpus.data,
            &v_out,
            &ref_out,
            Some(v_rep.words_consumed),
            Some(ref_report.words_consumed),
            Some(&v_rep.final_states),
            Some(&ref_report.final_states),
        );
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
