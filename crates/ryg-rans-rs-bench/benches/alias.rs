//! # Criterion benchmark: Alias method byte rANS
//!
//! Benchmarks for the core crate's alias-method rANS encoder/decoder:
//!   - alias table construction (model build, separate from kernel timing)
//!   - alias encode
//!   - alias decode
//!   - alias interleaved2 encode
//!   - alias interleaved2 decode
//!
//! Profiles: UNIFORM256, FREQ1_RESIDUAL, SKEWED_255_1, SPARSE_2, SPARSE_17,
//!           PRIME_RESIDUE, RENORM_BOUNDARY, INCOMPRESSIBLE_LIKE
//! Sizes: 64 B, 256 B, 1 KiB, 4 KiB, 64 KiB, 1 MiB

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::sync::OnceLock;
use std::vec::Vec;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_core::*;

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

/// Build and emit a Passed preflight record.  Emission failures are warnings
/// only: the exporter rejects missing records later, but the bench itself
/// must not fail on emission.
fn emit_preflight(
    benchmark_id: String,
    backend: &str,
    input: &[u8],
    output: &[u8],
    reference_output: &[u8],
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
        words_consumed: None,
        reference_words_consumed: None,
        final_states_sha256: None,
        reference_final_states_sha256: None,
        threads_requested: 1,
        threads_effective: 1,
        block_count: 1,
        queue_capacity: 0,
        allocation_mode: "unknown".to_string(),
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
// Constants
// ---------------------------------------------------------------------------

const PROFILES: &[ModelProfile] = &[
    ModelProfile::Uniform256,
    ModelProfile::Freq1Residual,
    ModelProfile::Skewed2551,
    ModelProfile::Sparse2,
    ModelProfile::Sparse17,
    ModelProfile::PrimeResidue,
    ModelProfile::RenormBoundary,
    ModelProfile::IncompressibleLike,
];

const SIZES: &[usize] = &[64, 256, 1024, 4096, 65536, 1048576];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn size_label(bytes: usize) -> &'static str {
    match bytes {
        64 => "64B",
        256 => "256B",
        1024 => "1KiB",
        4096 => "4KiB",
        65536 => "64KiB",
        1048576 => "1MiB",
        _ => "other",
    }
}

/// Build an alias table from a Corpus.
fn build_alias_table_from_corpus(corpus: &Corpus) -> AliasTable {
    let target_total = 1u32 << corpus.scale_bits;
    let (norm_freqs, norm_cum) =
        rans_byte_alias_normalize_freqs(&corpus.freqs, 256, target_total).unwrap();
    rans_byte_alias_build_table(&norm_freqs, &norm_cum, corpus.scale_bits)
}

/// Pre-encode data using alias method, returning compressed bytes.
fn alias_preencode(data: &[u8], table: &AliasTable, scale_bits: u32) -> Vec<u8> {
    let mut out = vec![0u8; data.len() * 4 + 8];
    let mut writer = BackwardByteWriter::new(&mut out);
    let mut state = RansByteState::new();
    for &sym in data.iter().rev() {
        rans_byte_alias_enc_put(&mut state, &mut writer, table, black_box(sym), scale_bits)
            .unwrap();
    }
    rans_byte_enc_flush(&state, &mut writer).unwrap();
    writer.encoded().to_vec()
}

/// Pre-encode data using alias method with 2-way interleaving.
fn alias_preencode_interleaved2(data: &[u8], table: &AliasTable, scale_bits: u32) -> Vec<u8> {
    let mut out = vec![0u8; data.len() * 4 + 16];
    let mut writer = BackwardByteWriter::new(&mut out);
    let mut state0 = RansByteState::new();
    let mut state1 = RansByteState::new();
    let n = data.len();
    if n & 1 != 0 {
        rans_byte_alias_enc_put(&mut state0, &mut writer, table, data[n - 1], scale_bits).unwrap();
    }
    let mut i = n & !1;
    while i > 0 {
        rans_byte_alias_enc_put(&mut state1, &mut writer, table, data[i - 1], scale_bits).unwrap();
        rans_byte_alias_enc_put(&mut state0, &mut writer, table, data[i - 2], scale_bits).unwrap();
        i = i.wrapping_sub(2);
    }
    rans_byte_enc_flush(&state1, &mut writer).unwrap();
    rans_byte_enc_flush(&state0, &mut writer).unwrap();
    writer.encoded().to_vec()
}

// ---------------------------------------------------------------------------
// Preflight: verify alias encode→decode roundtrip
// ---------------------------------------------------------------------------

fn alias_preflight_encode_decode(data: &[u8], table: &AliasTable, scale_bits: u32) {
    let encoded = alias_preencode(data, table, scale_bits);
    let mut reader = ByteReader::new(&encoded);
    let mut state = rans_byte_dec_init(&mut reader).unwrap();
    let mut decoded = vec![0u8; data.len()];
    for i in 0..data.len() {
        let s = rans_byte_alias_dec_advance(&mut state, &mut reader, table, scale_bits).unwrap();
        decoded[i] = s;
    }
    assert_eq!(decoded, data, "alias encode→decode preflight failed");
}

fn alias_preflight_interleaved2_encode_decode(data: &[u8], table: &AliasTable, scale_bits: u32) {
    let encoded = alias_preencode_interleaved2(data, table, scale_bits);
    let mut reader = ByteReader::new(&encoded);
    let mut state0 = rans_byte_dec_init(&mut reader).unwrap();
    let mut state1 = rans_byte_dec_init(&mut reader).unwrap();
    let mut output = vec![0u8; data.len()];
    let n = data.len();
    let even_n = n & !1;
    let mut pos = 0;
    while pos < even_n {
        let s0 = rans_byte_alias_dec_get(state0, table, scale_bits);
        state0 = s0.1;
        let s1 = rans_byte_alias_dec_get(state1, table, scale_bits);
        state1 = s1.1;
        output[pos] = s0.0;
        output[pos + 1] = s1.0;
        rans_byte_alias_dec_renorm(&mut state0, &mut reader).unwrap();
        rans_byte_alias_dec_renorm(&mut state1, &mut reader).unwrap();
        pos += 2;
    }
    if n & 1 != 0 {
        let s = rans_byte_alias_dec_advance(&mut state0, &mut reader, table, scale_bits).unwrap();
        output[n - 1] = s;
    }
    assert_eq!(
        output, data,
        "alias interleaved2 encode→decode preflight failed"
    );
}

// ---------------------------------------------------------------------------
// Benchmark: alias table construction (model build)
// ---------------------------------------------------------------------------

fn bench_alias_table_construct(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let freqs = &corpus.freqs;
            let scale_bits = corpus.scale_bits;
            let target_total = 1u32 << scale_bits;

            // Preflight: verify we can build a valid alias table
            let table = build_alias_table_from_corpus(&corpus);
            alias_preflight_encode_decode(&corpus.data, &table, scale_bits);

            let group_name = format!(
                "alias/alias-table/construct/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "alias-table-construct",
                &corpus.data,
                &corpus.data,
                &corpus.data,
            );

            let mut group = c.benchmark_group(group_name);
            group.throughput(Throughput::Bytes(corpus.data.len() as u64));

            group.bench_function("iter", |b| {
                b.iter(|| {
                    let (norm_freqs, norm_cum) = rans_byte_alias_normalize_freqs(
                        black_box(freqs),
                        black_box(256),
                        black_box(target_total),
                    )
                    .unwrap();
                    let table = rans_byte_alias_build_table(
                        black_box(&norm_freqs),
                        black_box(&norm_cum),
                        black_box(scale_bits),
                    );
                    black_box(table)
                });
            });
            group.finish();
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark: alias encode
// ---------------------------------------------------------------------------

fn bench_alias_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let scale_bits = corpus.scale_bits;

            let table = build_alias_table_from_corpus(&corpus);
            alias_preflight_encode_decode(data, &table, scale_bits);

            let group_name = format!(
                "alias/alias-encode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "alias-encode",
                data,
                data,
                data,
            );

            let mut group = c.benchmark_group(group_name);
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                b.iter_batched(
                    || vec![0u8; data.len() * 4 + 8],
                    |mut out| {
                        let mut state = RansByteState::new();
                        {
                            let mut writer = BackwardByteWriter::new(&mut out);
                            for &sym in data.iter().rev() {
                                rans_byte_alias_enc_put(
                                    black_box(&mut state),
                                    black_box(&mut writer),
                                    black_box(&table),
                                    black_box(sym),
                                    black_box(scale_bits),
                                )
                                .unwrap();
                            }
                            rans_byte_enc_flush(black_box(&state), black_box(&mut writer)).unwrap();
                        }
                        black_box((state, out))
                    },
                    BatchSize::SmallInput,
                )
            });
            group.finish();
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark: alias decode
// ---------------------------------------------------------------------------

fn bench_alias_decode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let scale_bits = corpus.scale_bits;

            let table = build_alias_table_from_corpus(&corpus);

            // Pre-encode
            let encoded = alias_preencode(data, &table, scale_bits);

            // Preflight decode
            {
                let mut reader = ByteReader::new(&encoded);
                let mut state = rans_byte_dec_init(&mut reader).unwrap();
                let mut decoded = vec![0u8; data.len()];
                for i in 0..data.len() {
                    let s =
                        rans_byte_alias_dec_advance(&mut state, &mut reader, &table, scale_bits)
                            .unwrap();
                    decoded[i] = s;
                }
                assert_eq!(
                    decoded,
                    *data,
                    "alias decode preflight failed for {} / {}",
                    profile.label(),
                    size_label(size),
                );
            }

            let group_name = format!(
                "alias/alias-decode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "alias-decode",
                data,
                data,
                data,
            );

            let mut group = c.benchmark_group(group_name);
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                let enc = encoded.clone();
                b.iter_batched(
                    || {
                        let reader = ByteReader::new(&enc);
                        (reader, vec![0u8; data.len()])
                    },
                    |(mut reader, mut decoded)| {
                        let mut state = rans_byte_dec_init(black_box(&mut reader)).unwrap();
                        for i in 0..data.len() {
                            let s = rans_byte_alias_dec_advance(
                                black_box(&mut state),
                                black_box(&mut reader),
                                black_box(&table),
                                black_box(scale_bits),
                            )
                            .unwrap();
                            decoded[i] = s;
                        }
                        black_box((state, decoded, reader))
                    },
                    BatchSize::SmallInput,
                )
            });
            group.finish();
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark: alias interleaved2 encode
// ---------------------------------------------------------------------------

fn bench_alias_interleaved2_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let scale_bits = corpus.scale_bits;

            let table = build_alias_table_from_corpus(&corpus);
            alias_preflight_interleaved2_encode_decode(data, &table, scale_bits);

            let group_name = format!(
                "alias/alias-interleaved2/encode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "alias-interleaved2",
                data,
                data,
                data,
            );

            let mut group = c.benchmark_group(group_name);
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                b.iter_batched(
                    || vec![0u8; data.len() * 4 + 16],
                    |mut out| {
                        let mut state0 = RansByteState::new();
                        let mut state1 = RansByteState::new();
                        {
                            let mut writer = BackwardByteWriter::new(&mut out);
                            let n = data.len();
                            if n & 1 != 0 {
                                rans_byte_alias_enc_put(
                                    black_box(&mut state0),
                                    black_box(&mut writer),
                                    black_box(&table),
                                    black_box(data[n - 1]),
                                    black_box(scale_bits),
                                )
                                .unwrap();
                            }
                            let mut i = n & !1;
                            while i > 0 {
                                rans_byte_alias_enc_put(
                                    black_box(&mut state1),
                                    black_box(&mut writer),
                                    black_box(&table),
                                    black_box(data[i - 1]),
                                    black_box(scale_bits),
                                )
                                .unwrap();
                                rans_byte_alias_enc_put(
                                    black_box(&mut state0),
                                    black_box(&mut writer),
                                    black_box(&table),
                                    black_box(data[i - 2]),
                                    black_box(scale_bits),
                                )
                                .unwrap();
                                i = i.wrapping_sub(2);
                            }
                            rans_byte_enc_flush(black_box(&state1), black_box(&mut writer))
                                .unwrap();
                            rans_byte_enc_flush(black_box(&state0), black_box(&mut writer))
                                .unwrap();
                        }
                        black_box((state0, state1, out))
                    },
                    BatchSize::SmallInput,
                )
            });
            group.finish();
        }
    }
}

// ---------------------------------------------------------------------------
// Benchmark: alias interleaved2 decode
// ---------------------------------------------------------------------------

fn bench_alias_interleaved2_decode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let scale_bits = corpus.scale_bits;

            let table = build_alias_table_from_corpus(&corpus);

            // Pre-encode interleaved2
            let encoded = alias_preencode_interleaved2(data, &table, scale_bits);

            // Preflight decode
            {
                let mut reader = ByteReader::new(&encoded);
                let mut state0 = rans_byte_dec_init(&mut reader).unwrap();
                let mut state1 = rans_byte_dec_init(&mut reader).unwrap();
                let mut output = vec![0u8; data.len()];
                let n = data.len();
                let even_n = n & !1;
                let mut pos = 0;
                while pos < even_n {
                    let s0 = rans_byte_alias_dec_get(state0, &table, scale_bits);
                    state0 = s0.1;
                    let s1 = rans_byte_alias_dec_get(state1, &table, scale_bits);
                    state1 = s1.1;
                    output[pos] = s0.0;
                    output[pos + 1] = s1.0;
                    rans_byte_alias_dec_renorm(&mut state0, &mut reader).unwrap();
                    rans_byte_alias_dec_renorm(&mut state1, &mut reader).unwrap();
                    pos += 2;
                }
                if n & 1 != 0 {
                    let s =
                        rans_byte_alias_dec_advance(&mut state0, &mut reader, &table, scale_bits)
                            .unwrap();
                    output[n - 1] = s;
                }
                assert_eq!(
                    output,
                    *data,
                    "alias interleaved2 decode preflight failed for {} / {}",
                    profile.label(),
                    size_label(size),
                );
            }

            let group_name = format!(
                "alias/alias-interleaved2/decode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "alias-interleaved2",
                data,
                data,
                data,
            );

            let mut group = c.benchmark_group(group_name);
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                let enc = encoded.clone();
                b.iter_batched(
                    || {
                        let reader = ByteReader::new(&enc);
                        (reader, vec![0u8; data.len()])
                    },
                    |(mut reader, mut output)| {
                        let mut state0 = rans_byte_dec_init(black_box(&mut reader)).unwrap();
                        let mut state1 = rans_byte_dec_init(black_box(&mut reader)).unwrap();
                        let n = data.len();
                        let even_n = n & !1;
                        let mut pos = 0;
                        while pos < even_n {
                            let s0 = rans_byte_alias_dec_get(
                                black_box(state0),
                                black_box(&table),
                                black_box(scale_bits),
                            );
                            state0 = s0.1;
                            let s1 = rans_byte_alias_dec_get(
                                black_box(state1),
                                black_box(&table),
                                black_box(scale_bits),
                            );
                            state1 = s1.1;
                            output[pos] = s0.0;
                            output[pos + 1] = s1.0;
                            rans_byte_alias_dec_renorm(
                                black_box(&mut state0),
                                black_box(&mut reader),
                            )
                            .unwrap();
                            rans_byte_alias_dec_renorm(
                                black_box(&mut state1),
                                black_box(&mut reader),
                            )
                            .unwrap();
                            pos += 2;
                        }
                        if n & 1 != 0 {
                            let s = rans_byte_alias_dec_advance(
                                black_box(&mut state0),
                                black_box(&mut reader),
                                black_box(&table),
                                black_box(scale_bits),
                            )
                            .unwrap();
                            output[n - 1] = s;
                        }
                        black_box((state0, state1, output, reader))
                    },
                    BatchSize::SmallInput,
                )
            });
            group.finish();
        }
    }
}

// ---------------------------------------------------------------------------
// Criterion registration
// ---------------------------------------------------------------------------

criterion_group!(
    name = alias_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(5))
        .sample_size(100);
    targets =
        bench_alias_table_construct,
        bench_alias_encode,
        bench_alias_decode,
        bench_alias_interleaved2_encode,
        bench_alias_interleaved2_decode,
);

criterion_main!(alias_benches);
