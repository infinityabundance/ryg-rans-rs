//! # Criterion benchmark: 64-bit rANS
//!
//! Benchmarks for the core crate's 64-bit rANS encoder/decoder:
//!   - R64 division encode
//!   - R64 reciprocal encode
//!   - R64 decode
//!   - R64 interleaved2 encode
//!   - R64 interleaved2 decode
//!
//! Profiles: UNIFORM256, FREQ1_RESIDUAL, SKEWED_255_1, SPARSE_2, SPARSE_17,
//!           PRIME_RESIDUE, RENORM_BOUNDARY, INCOMPRESSIBLE_LIKE
//! Sizes: 64 B, 256 B, 1 KiB, 4 KiB, 64 KiB, 1 MiB

use criterion::{BatchSize, Criterion, Throughput, black_box, criterion_group, criterion_main};
use std::vec::Vec;

use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_core::*;

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

/// Build a cum2sym lookup table.
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

/// Pre-encode data using division-based 64-bit rANS, returning compressed bytes.
fn r64_preencode_division(
    data: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
) -> Vec<u8> {
    let buf_len = (data.len() * 8 + 16).next_multiple_of(4);
    let mut out = vec![0u8; buf_len];
    let mut writer = BackwardWord32Writer::new(&mut out);
    let mut state = Rans64State::new();
    for &sym in data.iter().rev() {
        rans64_enc_put(
            &mut state,
            &mut writer,
            cum_freqs[sym as usize],
            freqs[sym as usize],
            scale_bits,
        )
        .unwrap();
    }
    rans64_enc_flush(&state, &mut writer).unwrap();
    writer.encoded().to_vec()
}

/// Pre-encode data using reciprocal 64-bit rANS, returning compressed bytes.
fn r64_preencode_reciprocal(data: &[u8], esyms: &[Rans64EncSymbol]) -> Vec<u8> {
    let buf_len = (data.len() * 8 + 16).next_multiple_of(4);
    let mut out = vec![0u8; buf_len];
    let mut writer = BackwardWord32Writer::new(&mut out);
    let mut state = Rans64State::new();
    for &sym in data.iter().rev() {
        rans64_enc_put_symbol(&mut state, &mut writer, &esyms[sym as usize]).unwrap();
    }
    rans64_enc_flush(&state, &mut writer).unwrap();
    writer.encoded().to_vec()
}

/// Pre-encode data using two-stream interleaved 64-bit rANS.
fn r64_preencode_interleaved2(data: &[u8], esyms: &[Rans64EncSymbol]) -> Vec<u8> {
    let buf_len = (data.len() * 8 + 16).next_multiple_of(4);
    let mut out = vec![0u8; buf_len];
    let mut writer = BackwardWord32Writer::new(&mut out);
    let mut state0 = Rans64State::new();
    let mut state1 = Rans64State::new();
    let n = data.len();
    if n & 1 != 0 {
        rans64_enc_put_symbol(&mut state0, &mut writer, &esyms[data[n - 1] as usize]).unwrap();
    }
    let mut i = n & !1;
    while i > 0 {
        let s1 = data[i - 1] as usize;
        let s0 = data[i - 2] as usize;
        rans64_enc_put_symbol(&mut state1, &mut writer, &esyms[s1]).unwrap();
        rans64_enc_put_symbol(&mut state0, &mut writer, &esyms[s0]).unwrap();
        i = i.wrapping_sub(2);
    }
    rans64_enc_flush(&state1, &mut writer).unwrap();
    rans64_enc_flush(&state0, &mut writer).unwrap();
    writer.encoded().to_vec()
}

// ---------------------------------------------------------------------------
// Preflight: verify encode then decode roundtrip
// ---------------------------------------------------------------------------

fn r64_preflight_division_encode_decode(
    data: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    cum2sym: &[u8],
    scale_bits: u32,
) {
    let encoded = r64_preencode_division(data, freqs, cum_freqs, scale_bits);
    let mut reader = Word32Reader::new(&encoded);
    let mut state = rans64_dec_init(&mut reader).unwrap();
    let mut decoded = vec![0u8; data.len()];
    for i in 0..data.len() {
        let cf = rans64_dec_get(&state, scale_bits);
        let s = cum2sym[cf as usize];
        decoded[i] = s;
        rans64_dec_advance(
            &mut state,
            &mut reader,
            cum_freqs[s as usize],
            freqs[s as usize],
            scale_bits,
        )
        .unwrap();
    }
    assert_eq!(decoded, data, "R64 division encode→decode preflight failed");
}

fn r64_preflight_reciprocal_encode_decode(
    data: &[u8],
    esyms: &[Rans64EncSymbol],
    dsyms: &[Rans64DecSymbol],
    cum2sym: &[u8],
    scale_bits: u32,
) {
    let encoded = r64_preencode_reciprocal(data, esyms);
    let mut reader = Word32Reader::new(&encoded);
    let mut state = rans64_dec_init(&mut reader).unwrap();
    let mut decoded = vec![0u8; data.len()];
    for i in 0..data.len() {
        let cf = rans64_dec_get(&state, scale_bits);
        let s = cum2sym[cf as usize] as usize;
        decoded[i] = s as u8;
        rans64_dec_advance_symbol(&mut state, &mut reader, &dsyms[s], scale_bits).unwrap();
    }
    assert_eq!(
        decoded, data,
        "R64 reciprocal encode→decode preflight failed"
    );
}

fn r64_preflight_interleaved2_encode_decode(
    data: &[u8],
    esyms: &[Rans64EncSymbol],
    dsyms: &[Rans64DecSymbol],
    cum2sym: &[u8],
    scale_bits: u32,
) {
    let encoded = r64_preencode_interleaved2(data, esyms);
    let mut reader = Word32Reader::new(&encoded);
    let mut d0 = rans64_dec_init(&mut reader).unwrap();
    let mut d1 = rans64_dec_init(&mut reader).unwrap();
    let mut output = vec![0u8; data.len()];
    let n = data.len();
    let even_n = n & !1;
    let mut pos = 0;
    while pos < even_n {
        let cf0 = rans64_dec_get(&d0, scale_bits);
        let s0 = cum2sym[cf0 as usize] as usize;
        let cf1 = rans64_dec_get(&d1, scale_bits);
        let s1 = cum2sym[cf1 as usize] as usize;
        output[pos] = s0 as u8;
        output[pos + 1] = s1 as u8;
        rans64_dec_advance_symbol_step(&mut d0, &dsyms[s0], scale_bits);
        rans64_dec_advance_symbol_step(&mut d1, &dsyms[s1], scale_bits);
        rans64_dec_renorm(&mut d0, &mut reader).unwrap();
        rans64_dec_renorm(&mut d1, &mut reader).unwrap();
        pos += 2;
    }
    if n & 1 != 0 {
        let cf0 = rans64_dec_get(&d0, scale_bits);
        let s0 = cum2sym[cf0 as usize] as usize;
        output[n - 1] = s0 as u8;
        rans64_dec_advance_symbol(&mut d0, &mut reader, &dsyms[s0], scale_bits).unwrap();
    }
    assert_eq!(
        output, data,
        "R64 interleaved2 encode→decode preflight failed"
    );
}

// ---------------------------------------------------------------------------
// Benchmark: R64 division encode
// ---------------------------------------------------------------------------

fn bench_r64_division_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            r64_preflight_division_encode_decode(data, freqs, cum_freqs, &cum2sym, scale_bits);

            let buf_len = (data.len() * 8 + 16).next_multiple_of(4);
            let mut group = c.benchmark_group(format!(
                "r64/r64-division/encode/{}/{}",
                profile.label(),
                size_label(size),
            ));
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                b.iter_batched(
                    || vec![0u8; buf_len],
                    |mut out| {
                        let mut state = Rans64State::new();
                        {
                            let mut writer = BackwardWord32Writer::new(&mut out);
                            for &sym in data.iter().rev() {
                                rans64_enc_put(
                                    black_box(&mut state),
                                    black_box(&mut writer),
                                    black_box(cum_freqs[sym as usize]),
                                    black_box(freqs[sym as usize]),
                                    black_box(scale_bits),
                                )
                                .unwrap();
                            }
                            rans64_enc_flush(black_box(&state), black_box(&mut writer)).unwrap();
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
// Benchmark: R64 reciprocal encode
// ---------------------------------------------------------------------------

fn bench_r64_reciprocal_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build encoder symbols
            let esyms: Vec<Rans64EncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64EncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        Rans64EncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<Rans64DecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64DecSymbol::new(0, 1).unwrap()
                    } else {
                        Rans64DecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            r64_preflight_reciprocal_encode_decode(data, &esyms, &dsyms, &cum2sym, scale_bits);

            let buf_len = (data.len() * 8 + 16).next_multiple_of(4);
            let mut group = c.benchmark_group(format!(
                "r64/r64-reciprocal/encode/{}/{}",
                profile.label(),
                size_label(size),
            ));
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                b.iter_batched(
                    || vec![0u8; buf_len],
                    |mut out| {
                        let mut state = Rans64State::new();
                        {
                            let mut writer = BackwardWord32Writer::new(&mut out);
                            for &sym in data.iter().rev() {
                                rans64_enc_put_symbol(
                                    black_box(&mut state),
                                    black_box(&mut writer),
                                    black_box(&esyms[sym as usize]),
                                )
                                .unwrap();
                            }
                            rans64_enc_flush(black_box(&state), black_box(&mut writer)).unwrap();
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
// Benchmark: R64 decode
// ---------------------------------------------------------------------------

fn bench_r64_decode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build symbols
            let esyms: Vec<Rans64EncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64EncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        Rans64EncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<Rans64DecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64DecSymbol::new(0, 1).unwrap()
                    } else {
                        Rans64DecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            // Pre-encode
            let encoded = r64_preencode_reciprocal(data, &esyms);

            // Preflight decode
            {
                let mut reader = Word32Reader::new(&encoded);
                let mut state = rans64_dec_init(&mut reader).unwrap();
                let mut decoded = vec![0u8; data.len()];
                for i in 0..data.len() {
                    let cf = rans64_dec_get(&state, scale_bits);
                    let s = cum2sym[cf as usize] as usize;
                    decoded[i] = s as u8;
                    rans64_dec_advance_symbol(&mut state, &mut reader, &dsyms[s], scale_bits)
                        .unwrap();
                }
                assert_eq!(
                    decoded,
                    *data,
                    "R64 decode preflight failed for {} / {}",
                    profile.label(),
                    size_label(size),
                );
            }

            let mut group = c.benchmark_group(format!(
                "r64/r64-decode/{}/{}",
                profile.label(),
                size_label(size),
            ));
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                let enc = encoded.clone();
                b.iter_batched(
                    || {
                        let reader = Word32Reader::new(&enc);
                        (reader, vec![0u8; data.len()])
                    },
                    |(mut reader, mut decoded)| {
                        let mut state = rans64_dec_init(black_box(&mut reader)).unwrap();
                        for i in 0..data.len() {
                            let cf = rans64_dec_get(black_box(&state), black_box(scale_bits));
                            let s = cum2sym[cf as usize] as usize;
                            decoded[i] = s as u8;
                            rans64_dec_advance_symbol(
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
// Benchmark: R64 interleaved2 encode
// ---------------------------------------------------------------------------

fn bench_r64_interleaved2_encode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build encoder symbols
            let esyms: Vec<Rans64EncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64EncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        Rans64EncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<Rans64DecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64DecSymbol::new(0, 1).unwrap()
                    } else {
                        Rans64DecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            r64_preflight_interleaved2_encode_decode(data, &esyms, &dsyms, &cum2sym, scale_bits);

            let buf_len = (data.len() * 8 + 16).next_multiple_of(4);
            let mut group = c.benchmark_group(format!(
                "r64/r64-interleaved2/encode/{}/{}",
                profile.label(),
                size_label(size),
            ));
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                b.iter_batched(
                    || vec![0u8; buf_len],
                    |mut out| {
                        let mut state0 = Rans64State::new();
                        let mut state1 = Rans64State::new();
                        {
                            let mut writer = BackwardWord32Writer::new(&mut out);
                            let n = data.len();
                            if n & 1 != 0 {
                                rans64_enc_put_symbol(
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
                                rans64_enc_put_symbol(
                                    black_box(&mut state1),
                                    black_box(&mut writer),
                                    black_box(&esyms[s1]),
                                )
                                .unwrap();
                                rans64_enc_put_symbol(
                                    black_box(&mut state0),
                                    black_box(&mut writer),
                                    black_box(&esyms[s0]),
                                )
                                .unwrap();
                                i = i.wrapping_sub(2);
                            }
                            rans64_enc_flush(black_box(&state1), black_box(&mut writer)).unwrap();
                            rans64_enc_flush(black_box(&state0), black_box(&mut writer)).unwrap();
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
// Benchmark: R64 interleaved2 decode
// ---------------------------------------------------------------------------

fn bench_r64_interleaved2_decode(c: &mut Criterion) {
    for &profile in PROFILES {
        for &size in SIZES {
            let corpus = Corpus::generate(profile, size, 42);
            let data = &corpus.data;
            let freqs = &corpus.freqs;
            let cum_freqs = &corpus.cum_freqs;
            let scale_bits = corpus.scale_bits;

            // Pre-build symbols
            let esyms: Vec<Rans64EncSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64EncSymbol::new(0, 1, scale_bits).unwrap()
                    } else {
                        Rans64EncSymbol::new(start, freq, scale_bits).unwrap()
                    }
                })
                .collect();

            let total = 1usize << scale_bits;
            let cum2sym = build_cum2sym(cum_freqs, total);
            let dsyms: Vec<Rans64DecSymbol> = (0..256)
                .map(|s| {
                    let start = cum_freqs[s];
                    let freq = freqs[s];
                    if freq == 0 {
                        Rans64DecSymbol::new(0, 1).unwrap()
                    } else {
                        Rans64DecSymbol::new(start, freq).unwrap()
                    }
                })
                .collect();

            // Pre-encode interleaved2
            let encoded = r64_preencode_interleaved2(data, &esyms);

            // Preflight decode
            {
                let mut reader = Word32Reader::new(&encoded);
                let mut d0 = rans64_dec_init(&mut reader).unwrap();
                let mut d1 = rans64_dec_init(&mut reader).unwrap();
                let mut output = vec![0u8; data.len()];
                let n = data.len();
                let even_n = n & !1;
                let mut pos = 0;
                while pos < even_n {
                    let cf0 = rans64_dec_get(&d0, scale_bits);
                    let s0 = cum2sym[cf0 as usize] as usize;
                    let cf1 = rans64_dec_get(&d1, scale_bits);
                    let s1 = cum2sym[cf1 as usize] as usize;
                    output[pos] = s0 as u8;
                    output[pos + 1] = s1 as u8;
                    rans64_dec_advance_symbol_step(&mut d0, &dsyms[s0], scale_bits);
                    rans64_dec_advance_symbol_step(&mut d1, &dsyms[s1], scale_bits);
                    rans64_dec_renorm(&mut d0, &mut reader).unwrap();
                    rans64_dec_renorm(&mut d1, &mut reader).unwrap();
                    pos += 2;
                }
                if n & 1 != 0 {
                    let cf0 = rans64_dec_get(&d0, scale_bits);
                    let s0 = cum2sym[cf0 as usize] as usize;
                    output[n - 1] = s0 as u8;
                    rans64_dec_advance_symbol(&mut d0, &mut reader, &dsyms[s0], scale_bits)
                        .unwrap();
                }
                assert_eq!(
                    output,
                    *data,
                    "R64 interleaved2 decode preflight failed for {} / {}",
                    profile.label(),
                    size_label(size),
                );
            }

            let mut group = c.benchmark_group(format!(
                "r64/r64-interleaved2/decode/{}/{}",
                profile.label(),
                size_label(size),
            ));
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_function("iter", |b| {
                let enc = encoded.clone();
                b.iter_batched(
                    || {
                        let reader = Word32Reader::new(&enc);
                        (reader, vec![0u8; data.len()])
                    },
                    |(mut reader, mut output)| {
                        let mut d0 = rans64_dec_init(black_box(&mut reader)).unwrap();
                        let mut d1 = rans64_dec_init(black_box(&mut reader)).unwrap();
                        let n = data.len();
                        let even_n = n & !1;
                        let mut pos = 0;
                        while pos < even_n {
                            let cf0 = rans64_dec_get(black_box(&d0), black_box(scale_bits));
                            let s0 = cum2sym[cf0 as usize] as usize;
                            let cf1 = rans64_dec_get(black_box(&d1), black_box(scale_bits));
                            let s1 = cum2sym[cf1 as usize] as usize;
                            output[pos] = s0 as u8;
                            output[pos + 1] = s1 as u8;
                            rans64_dec_advance_symbol_step(
                                black_box(&mut d0),
                                black_box(&dsyms[s0]),
                                black_box(scale_bits),
                            );
                            rans64_dec_advance_symbol_step(
                                black_box(&mut d1),
                                black_box(&dsyms[s1]),
                                black_box(scale_bits),
                            );
                            rans64_dec_renorm(black_box(&mut d0), black_box(&mut reader)).unwrap();
                            rans64_dec_renorm(black_box(&mut d1), black_box(&mut reader)).unwrap();
                            pos += 2;
                        }
                        if n & 1 != 0 {
                            let cf0 = rans64_dec_get(black_box(&d0), black_box(scale_bits));
                            let s0 = cum2sym[cf0 as usize] as usize;
                            output[n - 1] = s0 as u8;
                            rans64_dec_advance_symbol(
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
    name = r64_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(5))
        .sample_size(100);
    targets =
        bench_r64_division_encode,
        bench_r64_reciprocal_encode,
        bench_r64_decode,
        bench_r64_interleaved2_encode,
        bench_r64_interleaved2_decode,
);

criterion_main!(r64_benches);
