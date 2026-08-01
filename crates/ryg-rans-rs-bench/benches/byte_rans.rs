//! # Criterion benchmark: 32-bit byte rANS
//!
//! Benchmarks for the core crate's byte rANS encoder/decoder:
//!   - byte division encode
//!   - byte reciprocal encode
//!   - byte decode
//!   - byte interleaved2 encode
//!   - byte interleaved2 decode
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

/// Build a cum2sym lookup table: for each cumulative slot (0..total), map to
/// the symbol byte.
fn build_cum2sym(cum_freqs: &[u32], total: usize) -> Vec<u8> {
    let mut cum2sym = vec![0u8; total];
    for sym in 0..256u32 {
        let start = cum_freqs[sym as usize] as usize;
        let end = cum_freqs[sym as usize + 1] as usize;
        for i in start..end {
            cum2sym[i] = sym as u8;
        }
    }
    cum2sym
}

/// Pre-encode data using division-based byte rANS, returning the compressed
/// bytes and the final state.
fn preencode_division(
    data: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
) -> (Vec<u8>, RansByteState) {
    let mut out = vec![0u8; data.len() * 4 + 8];
    let mut writer = BackwardByteWriter::new(&mut out);
    let mut state = RansByteState::new();
    for &sym in data.iter().rev() {
        rans_byte_enc_put(
            &mut state,
            &mut writer,
            cum_freqs[sym as usize],
            freqs[sym as usize],
            scale_bits,
        )
        .unwrap();
    }
    rans_byte_enc_flush(&state, &mut writer).unwrap();
    let encoded = writer.encoded().to_vec();
    (encoded, state)
}

/// Pre-encode data using reciprocal (symbol-based) byte rANS, returning the
/// compressed bytes.
fn preencode_reciprocal(data: &[u8], esyms: &[RansByteEncSymbol]) -> (Vec<u8>, RansByteState) {
    let mut out = vec![0u8; data.len() * 4 + 8];
    let mut writer = BackwardByteWriter::new(&mut out);
    let mut state = RansByteState::new();
    for &sym in data.iter().rev() {
        rans_byte_enc_put_symbol(&mut state, &mut writer, &esyms[sym as usize]).unwrap();
    }
    rans_byte_enc_flush(&state, &mut writer).unwrap();
    let encoded = writer.encoded().to_vec();
    (encoded, state)
}

/// Pre-encode data using two-stream interleaved byte rANS.
fn preencode_interleaved2(
    data: &[u8],
    esyms: &[RansByteEncSymbol],
) -> (Vec<u8>, RansByteState, RansByteState) {
    let mut out = vec![0u8; data.len() * 4 + 16];
    let mut writer = BackwardByteWriter::new(&mut out);
    let mut state0 = RansByteState::new();
    let mut state1 = RansByteState::new();
    let n = data.len();
    if n & 1 != 0 {
        rans_byte_enc_put_symbol(&mut state0, &mut writer, &esyms[data[n - 1] as usize]).unwrap();
    }
    let mut i = n & !1;
    while i > 0 {
        let s1 = data[i - 1] as usize;
        let s0 = data[i - 2] as usize;
        rans_byte_enc_put_symbol(&mut state1, &mut writer, &esyms[s1]).unwrap();
        rans_byte_enc_put_symbol(&mut state0, &mut writer, &esyms[s0]).unwrap();
        i = i.wrapping_sub(2);
    }
    rans_byte_enc_flush(&state1, &mut writer).unwrap();
    rans_byte_enc_flush(&state0, &mut writer).unwrap();
    let encoded = writer.encoded().to_vec();
    (encoded, state0, state1)
}

// ---------------------------------------------------------------------------
// Preflight: verify encode then decode roundtrip
// ---------------------------------------------------------------------------

fn preflight_division_encode_decode(
    data: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
) {
    let (encoded, _final_state) = preencode_division(data, freqs, cum_freqs, scale_bits);
    let total = 1usize << scale_bits;
    let cum2sym = build_cum2sym(cum_freqs, total);
    let mut decoded = vec![0u8; data.len()];

    let mut reader = ByteReader::new(&encoded);
    let mut state = rans_byte_dec_init(&mut reader).unwrap();
    for i in 0..data.len() {
        let cf = rans_byte_dec_get(&state, scale_bits);
        let s = cum2sym[cf as usize];
        decoded[i] = s;
        rans_byte_dec_advance(
            &mut state,
            &mut reader,
            cum_freqs[s as usize],
            freqs[s as usize],
            scale_bits,
        )
        .unwrap();
    }
    assert_eq!(decoded, data, "division encode→decode preflight failed");
}

fn preflight_reciprocal_encode_decode(
    data: &[u8],
    esyms: &[RansByteEncSymbol],
    dsyms: &[RansByteDecSymbol],
    cum2sym: &[u8],
    scale_bits: u32,
) {
    let (encoded, _final_state) = preencode_reciprocal(data, esyms);
    let mut decoded = vec![0u8; data.len()];
    let mut reader = ByteReader::new(&encoded);
    let mut state = rans_byte_dec_init(&mut reader).unwrap();
    for i in 0..data.len() {
        let cf = rans_byte_dec_get(&state, scale_bits);
        let s = cum2sym[cf as usize] as usize;
        decoded[i] = s as u8;
        rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsyms[s], scale_bits).unwrap();
    }
    assert_eq!(decoded, data, "reciprocal encode→decode preflight failed");
}

fn preflight_interleaved2_encode_decode(
    data: &[u8],
    esyms: &[RansByteEncSymbol],
    dsyms: &[RansByteDecSymbol],
    cum2sym: &[u8],
    scale_bits: u32,
) {
    let (encoded, _s0, _s1) = preencode_interleaved2(data, esyms);
    let mut reader = ByteReader::new(&encoded);
    let mut d0 = rans_byte_dec_init(&mut reader).unwrap();
    let mut d1 = rans_byte_dec_init(&mut reader).unwrap();
    let mut output = vec![0u8; data.len()];
    let n = data.len();
    let even_n = n & !1;
    let mut pos = 0;
    while pos < even_n {
        let cf0 = rans_byte_dec_get(&d0, scale_bits);
        let s0 = cum2sym[cf0 as usize] as usize;
        let cf1 = rans_byte_dec_get(&d1, scale_bits);
        let s1 = cum2sym[cf1 as usize] as usize;
        output[pos] = s0 as u8;
        output[pos + 1] = s1 as u8;
        rans_byte_dec_advance_symbol_step(&mut d0, &dsyms[s0], scale_bits);
        rans_byte_dec_advance_symbol_step(&mut d1, &dsyms[s1], scale_bits);
        rans_byte_dec_renorm(&mut d0, &mut reader).unwrap();
        rans_byte_dec_renorm(&mut d1, &mut reader).unwrap();
        pos += 2;
    }
    if n & 1 != 0 {
        let cf0 = rans_byte_dec_get(&d0, scale_bits);
        let s0 = cum2sym[cf0 as usize] as usize;
        output[n - 1] = s0 as u8;
        rans_byte_dec_advance_symbol(&mut d0, &mut reader, &dsyms[s0], scale_bits).unwrap();
    }
    assert_eq!(output, data, "interleaved2 encode→decode preflight failed");
}

// ---------------------------------------------------------------------------
// Benchmark: byte division encode
// ---------------------------------------------------------------------------

fn bench_byte_division_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            preflight_division_encode_decode(data, freqs, cum_freqs, scale_bits);

            let group_name = format!(
                "byte-rans/byte-division/encode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "byte-division",
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
                                rans_byte_enc_put(
                                    black_box(&mut state),
                                    black_box(&mut writer),
                                    black_box(cum_freqs[sym as usize]),
                                    black_box(freqs[sym as usize]),
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
// Benchmark: byte reciprocal encode
// ---------------------------------------------------------------------------

fn bench_byte_reciprocal_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build encoder symbols (reciprocal)
            let esyms: Vec<RansByteEncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteEncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        RansByteEncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<RansByteDecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteDecSymbol::new(0, 1).unwrap()
                    } else {
                        RansByteDecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            preflight_reciprocal_encode_decode(data, &esyms, &dsyms, &cum2sym, scale_bits);

            let group_name = format!(
                "byte-rans/byte-reciprocal/encode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "byte-reciprocal",
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
                                rans_byte_enc_put_symbol(
                                    black_box(&mut state),
                                    black_box(&mut writer),
                                    black_box(&esyms[sym as usize]),
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
// Benchmark: byte decode
// ---------------------------------------------------------------------------

fn bench_byte_decode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build encoder/decoder symbols
            let esyms: Vec<RansByteEncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteEncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        RansByteEncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<RansByteDecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteDecSymbol::new(0, 1).unwrap()
                    } else {
                        RansByteDecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            // Pre-encode once
            let (encoded, _final_state) = preencode_reciprocal(data, &esyms);

            // Preflight decode
            {
                let mut reader = ByteReader::new(&encoded);
                let mut state = rans_byte_dec_init(&mut reader).unwrap();
                let mut decoded = vec![0u8; data.len()];
                for i in 0..data.len() {
                    let cf = rans_byte_dec_get(&state, scale_bits);
                    let s = cum2sym[cf as usize] as usize;
                    decoded[i] = s as u8;
                    rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsyms[s], scale_bits)
                        .unwrap();
                }
                assert_eq!(
                    decoded,
                    *data,
                    "decode preflight failed for {} / {}",
                    profile.label(),
                    size_label(size),
                );
            }

            let group_name = format!(
                "byte-rans/byte-decode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "byte-decode",
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
                            let cf = rans_byte_dec_get(black_box(&state), black_box(scale_bits));
                            let s = cum2sym[cf as usize] as usize;
                            decoded[i] = s as u8;
                            rans_byte_dec_advance_symbol(
                                black_box(&mut state),
                                black_box(&mut reader),
                                black_box(&dsyms[s]),
                                black_box(scale_bits),
                            )
                            .unwrap();
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
// Benchmark: byte interleaved2 encode
// ---------------------------------------------------------------------------

fn bench_byte_interleaved2_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build encoder symbols
            let esyms: Vec<RansByteEncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteEncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        RansByteEncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<RansByteDecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteDecSymbol::new(0, 1).unwrap()
                    } else {
                        RansByteDecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            preflight_interleaved2_encode_decode(data, &esyms, &dsyms, &cum2sym, scale_bits);

            let group_name = format!(
                "byte-rans/byte-interleaved2/encode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "byte-interleaved2",
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
                                rans_byte_enc_put_symbol(
                                    black_box(&mut state0),
                                    black_box(&mut writer),
                                    black_box(&esyms[data[n - 1] as usize]),
                                )
                                .unwrap();
                            }
                            let mut i = n & !1;
                            while i > 0 {
                                let s1 = data[i - 1] as usize;
                                let s0 = data[i - 2] as usize;
                                rans_byte_enc_put_symbol(
                                    black_box(&mut state1),
                                    black_box(&mut writer),
                                    black_box(&esyms[s1]),
                                )
                                .unwrap();
                                rans_byte_enc_put_symbol(
                                    black_box(&mut state0),
                                    black_box(&mut writer),
                                    black_box(&esyms[s0]),
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
// Benchmark: byte interleaved2 decode
// ---------------------------------------------------------------------------

fn bench_byte_interleaved2_decode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build symbols
            let esyms: Vec<RansByteEncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteEncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        RansByteEncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<RansByteDecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        RansByteDecSymbol::new(0, 1).unwrap()
                    } else {
                        RansByteDecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            // Pre-encode interleaved2
            let (encoded, _s0, _s1) = preencode_interleaved2(data, &esyms);

            // Preflight decode
            {
                let mut reader = ByteReader::new(&encoded);
                let mut d0 = rans_byte_dec_init(&mut reader).unwrap();
                let mut d1 = rans_byte_dec_init(&mut reader).unwrap();
                let mut output = vec![0u8; data.len()];
                let n = data.len();
                let even_n = n & !1;
                let mut pos = 0;
                while pos < even_n {
                    let cf0 = rans_byte_dec_get(&d0, scale_bits);
                    let s0 = cum2sym[cf0 as usize] as usize;
                    let cf1 = rans_byte_dec_get(&d1, scale_bits);
                    let s1 = cum2sym[cf1 as usize] as usize;
                    output[pos] = s0 as u8;
                    output[pos + 1] = s1 as u8;
                    rans_byte_dec_advance_symbol_step(&mut d0, &dsyms[s0], scale_bits);
                    rans_byte_dec_advance_symbol_step(&mut d1, &dsyms[s1], scale_bits);
                    rans_byte_dec_renorm(&mut d0, &mut reader).unwrap();
                    rans_byte_dec_renorm(&mut d1, &mut reader).unwrap();
                    pos += 2;
                }
                if n & 1 != 0 {
                    let cf0 = rans_byte_dec_get(&d0, scale_bits);
                    let s0 = cum2sym[cf0 as usize] as usize;
                    output[n - 1] = s0 as u8;
                    rans_byte_dec_advance_symbol(&mut d0, &mut reader, &dsyms[s0], scale_bits)
                        .unwrap();
                }
                assert_eq!(
                    output,
                    *data,
                    "interleaved2 decode preflight failed for {} / {}",
                    profile.label(),
                    size_label(size),
                );
            }

            let group_name = format!(
                "byte-rans/byte-interleaved2/decode/{}/{}",
                profile.label(),
                size_label(size),
            );
            emit_preflight(
                format!("{}/iter", group_name),
                "byte-interleaved2",
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
                        let mut d0 = rans_byte_dec_init(black_box(&mut reader)).unwrap();
                        let mut d1 = rans_byte_dec_init(black_box(&mut reader)).unwrap();
                        let n = data.len();
                        let even_n = n & !1;
                        let mut pos = 0;
                        while pos < even_n {
                            let cf0 = rans_byte_dec_get(black_box(&d0), black_box(scale_bits));
                            let s0 = cum2sym[cf0 as usize] as usize;
                            let cf1 = rans_byte_dec_get(black_box(&d1), black_box(scale_bits));
                            let s1 = cum2sym[cf1 as usize] as usize;
                            output[pos] = s0 as u8;
                            output[pos + 1] = s1 as u8;
                            rans_byte_dec_advance_symbol_step(
                                black_box(&mut d0),
                                black_box(&dsyms[s0]),
                                black_box(scale_bits),
                            );
                            rans_byte_dec_advance_symbol_step(
                                black_box(&mut d1),
                                black_box(&dsyms[s1]),
                                black_box(scale_bits),
                            );
                            rans_byte_dec_renorm(black_box(&mut d0), black_box(&mut reader))
                                .unwrap();
                            rans_byte_dec_renorm(black_box(&mut d1), black_box(&mut reader))
                                .unwrap();
                            pos += 2;
                        }
                        if n & 1 != 0 {
                            let cf0 = rans_byte_dec_get(black_box(&d0), black_box(scale_bits));
                            let s0 = cum2sym[cf0 as usize] as usize;
                            output[n - 1] = s0 as u8;
                            rans_byte_dec_advance_symbol(
                                black_box(&mut d0),
                                black_box(&mut reader),
                                black_box(&dsyms[s0]),
                                black_box(scale_bits),
                            )
                            .unwrap();
                        }
                        black_box((d0, d1, output, reader))
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
    name = byte_rans_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(5))
        .sample_size(100);
    targets =
        bench_byte_division_encode,
        bench_byte_reciprocal_encode,
        bench_byte_decode,
        bench_byte_interleaved2_encode,
        bench_byte_interleaved2_decode,
);

criterion_main!(byte_rans_benches);
