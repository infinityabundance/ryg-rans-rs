//! # Criterion benchmark: SSE4.1 decoder
//!
//! Tier 1 benchmarks for SSE4.1 8-way interleaved decode.
//! Verified against scalar 8-way reference before timing.
//! Uses the REAL target-feature-gated function after runtime detection,
//! so actual SSE4.1 instructions execute even on portable builds.

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
        allocation_mode: "allocating".to_string(),
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

fn sse41_available() -> bool {
    // The real SSE4.1 kernel requires both SSSE3 and SSE4.1 at runtime.
    // The bench crate always has std, so we always use runtime detection.
    std::is_x86_feature_detected!("ssse3") && std::is_x86_feature_detected!("sse4.1")
}

fn bench_sse41_8way(c: &mut Criterion) {
    if !sse41_available() {
        eprintln!("UNSUPPORTED: sse41-8way (SSSE3+SSE4.1 not available)");
        emit_unsupported(
            "sse41/sse41-8way/allocating/SKEWED_255_1/256KiB/sse41-8way".to_string(),
            "sse41-8way",
        );
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

    // Preflight record: run the exact timed kernel once, verify, emit.
    let (pf_out, pf_words, pf_states) = unsafe {
        ryg_rans_rs_simd::decode_simd_8way_unchecked_with_report(
            &encoded,
            &tables,
            corpus.data.len(),
        )
    }
    .expect("SSE4.1 record preflight");
    assert_eq!(
        pf_out, corpus.data,
        "SSE4.1 record preflight: output must match original"
    );
    emit_preflight(
        "sse41/sse41-8way/allocating/SKEWED_255_1/256KiB/sse41-8way".to_string(),
        "sse41-8way",
        &corpus.data,
        &pf_out,
        &corpus.data,
        Some(pf_words),
        Some(pf_words),
        Some(&pf_states),
        Some(&pf_states),
    );

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
