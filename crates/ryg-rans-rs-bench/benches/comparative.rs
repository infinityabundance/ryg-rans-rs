//! # Phase L.14 — Comparative benchmark court
//!
//! Same-host, identical-corpus comparison of `ryg-rans-rs` against credible
//! alternatives, pinned to exact versions:
//!
//! * `ryg-rans-sys = "=1.2.0"` — maintained FFI bindings to upstream
//!   `ryg_rans` (https://github.com/m4tx/ryg-rans-sys), i.e. the C
//!   reference implementation this project is derived from.
//!
//! ## Methodology (identical across sides)
//!
//! * Same corpus (same seed, same model profile, same byte content).
//! * Same frequency model — the same `freqs`/`cum` arrays are passed to both
//!   implementations, so the comparison isolates codec throughput and never
//!   conflates normalisation differences.
//! * Same sizes (1 MiB), same operation (encode then decode), same compiler
//!   flags (`RUSTFLAGS` apply to the whole workspace), same warmup/sample
//!   settings, same host, same affinity (none).
//!
//! ## Separated cost components
//!
//! * Core codec throughput (encode, decode).
//! * FFI-crossing overhead: the cost of one (and of two, per byte) call into
//!   the C library is measured separately so FFI cost is not silently
//!   attributed to the codec.  The C-side timed loops hoist all symbol
//!   construction out of the timed region, leaving exactly the mandatory
//!   crossings: 1 per byte (encode) and 2 per byte (decode) for the byte
//!   surface, and 2 per byte for the word surface.
//!
//! ## Methodological residuals
//!
//! * `ryg-rans-sys`'s C wrappers are compiled by the `cc` crate with its own
//!   flags; `-C target-cpu=native` applies to Rust codegen only, and the C
//!   byte/word surfaces are plain C (no explicit `-march=native`).  Where
//!   the C side lacks auto-vectorisation the comparison favours Rust; this
//!   is recorded as residual L14-A in `evidence/phase-l/gap-ledger.md`.
//! * The word-SSE4.1 surface requires SSE4.1 compiled in
//!   (`comparative-word-sse41` bench feature + native RUSTFLAGS).
//! * The `rans` 0.4.0 crate (m4tx) was considered but exposes a different
//!   API/format; comparing it byte-for-byte is not possible without
//!   format adaptation — recorded as residual L14-B.
//!
//! This bench is **not** part of the ten sealed performance surfaces — it is
//! a separate, methodological, same-host comparison (Phase L.14).

use criterion::{Criterion, Throughput, black_box, criterion_group, criterion_main};
use ryg_rans_rs_bench::common::corpus::{Corpus, ModelProfile};
use ryg_rans_rs_core::{
    BackwardByteWriter, ByteReader, RansByteDecSymbol, RansByteEncSymbol, RansByteState,
    rans_byte_dec_advance_symbol, rans_byte_dec_init, rans_byte_enc_flush,
    rans_byte_enc_put_symbol,
};
#[cfg(feature = "comparative-word-sse41")]
use ryg_rans_rs_core::{
    BackwardWord16Writer, RansWordSlot, RansWordState, RansWordTables, Word16Reader,
};

const SIZE: usize = 1 << 20; // 1 MiB
const SCALE_BITS: u32 = 12;

/// Measure FFI crossing overhead: trivial calls into the C library that do
/// negligible work, reported both per single call and per byte at the exact
/// crossing rate of the byte-rANS decode path (2 calls per byte).
fn ffi_overhead(c: &mut Criterion) {
    let mut group = c.benchmark_group("comparative/ffi-overhead");
    group.throughput(Throughput::Bytes(SIZE as u64));
    group.bench_function("empty-extern-call", |b| {
        b.iter(|| {
            // SAFETY: rans_dec_get reads a u32 state by value; no memory
            // dereference beyond the stack local.
            unsafe {
                let mut st: u32 = 0x12345678;
                black_box(ryg_rans_sys::rans_byte::rans_dec_get(&mut st, SCALE_BITS))
            }
        });
    });
    group.bench_function("two-calls-per-byte", |b| {
        b.iter(|| {
            // SAFETY: as above; both calls are pure value transforms on a
            // stack-local state.  SIZE iterations model the byte-decode
            // path's 2 crossings per byte (get + advance).
            unsafe {
                let mut st: u32 = 0x12345678;
                for _ in 0..SIZE {
                    black_box(ryg_rans_sys::rans_byte::rans_dec_get(&mut st, SCALE_BITS));
                    black_box(ryg_rans_sys::rans_byte::rans_dec_get(&mut st, SCALE_BITS));
                }
            }
        });
    });
    group.finish();
}

/// Byte rANS (division path): Rust core vs upstream C via FFI.
///
/// Identical model arrays, identical data, identical 1 MiB workload.
/// A preflight asserts both sides produce byte-identical compressed output.
/// Both timed loops hoist symbol construction out of the timed region; the
/// C side retains exactly the mandatory crossings (1/byte encode,
/// 2/byte decode).
fn byte_rans_comparative(c: &mut Criterion) {
    let corpus = Corpus::generate(ModelProfile::Skewed2551, SIZE, 42);
    let freqs = &corpus.freqs;
    let cum = &corpus.cum_freqs;
    let data = &corpus.data;

    // C-side symbols are pure functions of (cum, freq, symbol); the data is
    // known in advance, so precompute the arrays once and hoist them out of
    // the timed region.  This removes symbol-init FFI crossings from the
    // measurement, leaving only the mandatory per-byte crossings.
    // SAFETY: zeroed memory is a valid `RansEncSymbol`/`RansDecSymbol`
    // (plain POD structs of u32 fields); every element is initialised by
    // the init functions before use.
    let c_enc_syms: Vec<ryg_rans_sys::rans_byte::RansEncSymbol> = unsafe {
        let mut syms: Vec<ryg_rans_sys::rans_byte::RansEncSymbol> =
            (0..SIZE).map(|_| std::mem::zeroed()).collect();
        for (i, &s) in data.iter().rev().enumerate() {
            ryg_rans_sys::rans_byte::rans_enc_symbol_init(
                &mut syms[i],
                cum[s as usize],
                freqs[s as usize],
                SCALE_BITS,
            );
        }
        syms
    };
    let c_dec_syms: Vec<ryg_rans_sys::rans_byte::RansDecSymbol> = unsafe {
        let mut syms: Vec<ryg_rans_sys::rans_byte::RansDecSymbol> =
            (0..SIZE).map(|_| std::mem::zeroed()).collect();
        for (i, &s) in data.iter().enumerate() {
            ryg_rans_sys::rans_byte::rans_dec_symbol_init(
                &mut syms[i],
                cum[s as usize],
                freqs[s as usize],
            );
        }
        syms
    };

    // Rust-side symbols, precomputed exactly like the C side so both timed
    // loops measure the same operation: encode with a ready reciprocal
    // symbol (C's `RansEncPutSymbol` is the same multiply-high path).  The
    // division-based reference path is measured separately below.
    let rust_enc_syms: Vec<RansByteEncSymbol> = data
        .iter()
        .rev()
        .map(|&s| {
            RansByteEncSymbol::new(cum[s as usize], freqs[s as usize], SCALE_BITS).expect("enc sym")
        })
        .collect();
    let rust_dec_syms: Vec<RansByteDecSymbol> = data
        .iter()
        .map(|&s| RansByteDecSymbol::new(cum[s as usize], freqs[s as usize]).expect("dec sym"))
        .collect();

    // ---- Preflight: byte-identical compressed output ----
    let rust_comp = {
        let mut buf = vec![0u8; SIZE * 4 + 64];
        let mut writer = BackwardByteWriter::new(&mut buf);
        let mut state = RansByteState::new();
        for &s in data.iter().rev() {
            let sym = RansByteEncSymbol::new(cum[s as usize], freqs[s as usize], SCALE_BITS)
                .expect("sym");
            rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).expect("enc");
        }
        rans_byte_enc_flush(&state, &mut writer).expect("flush");
        writer.encoded().to_vec()
    };
    let c_comp = {
        // SAFETY: pptr walks the buffer backward exactly as the C API
        // expects; we allocate SIZE*4+64 and the encoder cannot exceed it
        // (same bounds as the Rust side's writer).
        unsafe {
            let mut buf = vec![0u8; SIZE * 4 + 64];
            let mut enc: u32 = 0;
            ryg_rans_sys::rans_byte::rans_enc_init(&mut enc);
            let mut wptr = buf.as_mut_ptr().add(buf.len());
            for sym in &c_enc_syms {
                ryg_rans_sys::rans_byte::rans_enc_put_symbol(&mut enc, &mut wptr, sym);
            }
            ryg_rans_sys::rans_byte::rans_enc_flush(&mut enc, &mut wptr);
            let used = buf.len() - (buf.as_ptr().add(buf.len()) as usize - wptr as usize);
            buf[used..].to_vec()
        }
    };
    assert_eq!(
        rust_comp, c_comp,
        "byte rANS: Rust core and upstream C must produce identical compressed bytes"
    );

    // ---- Encode ----
    let mut group = c.benchmark_group("comparative/byte-rans/encode/1MiB/SKEWED_255_1");
    group.throughput(Throughput::Bytes(SIZE as u64));
    group.bench_function("rust-core-reciprocal", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE * 4 + 64];
            let mut writer = BackwardByteWriter::new(&mut buf);
            let mut state = RansByteState::new();
            for sym in &rust_enc_syms {
                rans_byte_enc_put_symbol(&mut state, &mut writer, sym).expect("enc");
            }
            rans_byte_enc_flush(&state, &mut writer).expect("flush");
            black_box(writer.encoded().len())
        });
    });
    group.bench_function("rust-core-division", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE * 4 + 64];
            let mut writer = BackwardByteWriter::new(&mut buf);
            let mut state = RansByteState::new();
            for &s in black_box(data).iter().rev() {
                ryg_rans_rs_core::rans_byte_enc_put(
                    &mut state,
                    &mut writer,
                    cum[s as usize],
                    freqs[s as usize],
                    SCALE_BITS,
                )
                .expect("enc");
            }
            rans_byte_enc_flush(&state, &mut writer).expect("flush");
            black_box(writer.encoded().len())
        });
    });
    group.bench_function("ryg-rans-sys-c", |b| {
        b.iter(|| {
            // SAFETY: as preflight; symbols are precomputed, so the timed
            // region holds exactly one crossing per byte.
            unsafe {
                let mut buf = vec![0u8; SIZE * 4 + 64];
                let mut enc: u32 = 0;
                ryg_rans_sys::rans_byte::rans_enc_init(&mut enc);
                let mut wptr = buf.as_mut_ptr().add(buf.len());
                for sym in &c_enc_syms {
                    ryg_rans_sys::rans_byte::rans_enc_put_symbol(&mut enc, &mut wptr, sym);
                }
                ryg_rans_sys::rans_byte::rans_enc_flush(&mut enc, &mut wptr);
                black_box(wptr)
            }
        });
    });
    group.finish();

    // ---- Decode ----
    let mut group = c.benchmark_group("comparative/byte-rans/decode/1MiB/SKEWED_255_1");
    group.throughput(Throughput::Bytes(SIZE as u64));
    group.bench_function("rust-core-reciprocal", |b| {
        let comp = rust_comp.clone();
        b.iter(|| {
            let mut reader = ByteReader::new(black_box(&comp));
            let mut state = rans_byte_dec_init(&mut reader).expect("init");
            let mut out = vec![0u8; SIZE];
            for (i, o) in out.iter_mut().enumerate() {
                // Upstream API: get the symbol, then advance.
                *o = ryg_rans_rs_core::rans_byte_dec_get(&state, SCALE_BITS) as u8;
                rans_byte_dec_advance_symbol(
                    &mut state,
                    &mut reader,
                    &rust_dec_syms[i],
                    SCALE_BITS,
                )
                .expect("dec");
            }
            black_box(out.len())
        });
    });
    group.bench_function("ryg-rans-sys-c", |b| {
        let comp = rust_comp.clone();
        b.iter(|| {
            // SAFETY: rptr walks the compressed buffer forward exactly as
            // the C API expects; preflight proved the buffer is well-formed.
            // Symbols are precomputed; the timed region holds the two
            // mandatory crossings per byte (get + advance).
            unsafe {
                let mut rptr = comp.as_ptr() as *mut u8;
                let mut state: u32 = 0;
                ryg_rans_sys::rans_byte::rans_dec_init(&mut state, &mut rptr);
                let mut out = vec![0u8; SIZE];
                for (i, o) in out.iter_mut().enumerate() {
                    *o = ryg_rans_sys::rans_byte::rans_dec_get(&mut state, SCALE_BITS) as u8;
                    ryg_rans_sys::rans_byte::rans_dec_advance_symbol(
                        &mut state,
                        &mut rptr,
                        &c_dec_syms[i],
                        SCALE_BITS,
                    );
                }
                black_box(out.len())
            }
        });
    });
    group.finish();
}

/// Word rANS (single-state): Rust core vs upstream C `rans_word_sse41`.
///
/// Requires the `comparative-word-sse41` bench feature AND SSE4.1 compiled
/// in (run under `RUSTFLAGS="-C target-cpu=native"`).
#[cfg(feature = "comparative-word-sse41")]
fn word_rans_comparative(c: &mut Criterion) {
    let corpus = Corpus::generate(ModelProfile::Skewed2551, SIZE, 42);
    let freqs = &corpus.freqs;
    let cum = &corpus.cum_freqs;
    let data = &corpus.data;
    let m = 1usize << SCALE_BITS;

    // Rust tables (slot + slot2sym) for the core decoder.
    let mut r_slots = vec![RansWordSlot { freq: 0, bias: 0 }; m];
    let mut r_slot2sym = vec![0u8; m];
    for s in 0..256usize {
        let f = freqs[s] as usize;
        let start = cum[s] as usize;
        for i in 0..f {
            let slot = start + i;
            if slot < m {
                r_slots[slot] = RansWordSlot {
                    freq: f as u16,
                    bias: i as u16,
                };
                r_slot2sym[slot] = s as u8;
            }
        }
    }
    let r_tables = RansWordTables {
        slots: &r_slots,
        slot2sym: &r_slot2sym,
    };

    // C-side tables: the upstream type is an inline struct of arrays
    // (20480 bytes) built with `rans_word_tables_init_symbol`, which fills
    // both `slots[slot]` and `slot2sym[slot]` for the symbol's range.
    let mut c_tables: ryg_rans_sys::rans_word_sse41::RansWordTables = unsafe {
        // SAFETY: plain-old-data struct of fixed arrays; zeroing yields a
        // valid empty table (every slot unused until initialized below).
        std::mem::zeroed()
    };
    for s in 0..256usize {
        // SAFETY: `c_tables` is a valid, writable table for the lifetime of
        // the call; `s < 256` and `cum[s] + freqs[s] <= 4096` (normalized
        // model invariant, asserted by `Corpus::generate`).
        unsafe {
            ryg_rans_sys::rans_word_sse41::rans_word_tables_init_symbol(
                &mut c_tables,
                s as u8,
                cum[s],
                freqs[s],
            );
        }
    }

    // Preflight: identical compressed bytes (Rust writes LE bytes; the C
    // side writes u16 words — flatten the C words to LE bytes for the
    // byte-identical comparison).
    let rust_comp = {
        let mut buf = vec![0u8; SIZE * 4 + 64];
        let mut writer = BackwardWord16Writer::new(&mut buf);
        let mut state = RansWordState::new();
        for &s in data.iter().rev() {
            ryg_rans_rs_core::rans_word_enc_put(
                &mut state,
                &mut writer,
                cum[s as usize],
                freqs[s as usize],
                SCALE_BITS,
            )
            .expect("enc");
        }
        ryg_rans_rs_core::rans_word_enc_flush(&mut state, &mut writer).expect("flush");
        writer.encoded().to_vec()
    };
    let c_comp_words = unsafe {
        // SAFETY: pptr walks the u16 buffer backward per the upstream API.
        let mut buf = vec![0u16; SIZE * 2 + 64];
        let mut enc: u32 = ryg_rans_sys::rans_word_sse41::rans_word_enc_init();
        let mut wptr = buf.as_mut_ptr().add(buf.len());
        for &s in data.iter().rev() {
            ryg_rans_sys::rans_word_sse41::rans_word_enc_put(
                &mut enc,
                &mut wptr,
                cum[s as usize],
                freqs[s as usize],
            );
        }
        ryg_rans_sys::rans_word_sse41::rans_word_enc_flush(&mut enc, &mut wptr);
        // `wptr` moves by one u16 (2 bytes) per word written, so the byte
        // difference must be halved to recover the element index.
        let used = buf.len() - ((buf.as_ptr().add(buf.len()) as usize - wptr as usize) / 2);
        buf[used..].to_vec()
    };
    let c_comp: Vec<u8> = c_comp_words.iter().flat_map(|w| w.to_le_bytes()).collect();
    assert_eq!(
        rust_comp, c_comp,
        "word rANS: Rust core and upstream C must produce identical compressed bytes"
    );

    // ---- Encode ----
    let mut group = c.benchmark_group("comparative/word-rans/encode/1MiB/SKEWED_255_1");
    group.throughput(Throughput::Bytes(SIZE as u64));
    group.bench_function("rust-core", |b| {
        b.iter(|| {
            let mut buf = vec![0u8; SIZE * 4 + 64];
            let mut writer = BackwardWord16Writer::new(&mut buf);
            let mut state = RansWordState::new();
            for &s in black_box(data).iter().rev() {
                ryg_rans_rs_core::rans_word_enc_put(
                    &mut state,
                    &mut writer,
                    cum[s as usize],
                    freqs[s as usize],
                    SCALE_BITS,
                )
                .expect("enc");
            }
            ryg_rans_rs_core::rans_word_enc_flush(&mut state, &mut writer).expect("flush");
            black_box(writer.encoded().len())
        });
    });
    group.bench_function("ryg-rans-sys-c", |b| {
        b.iter(|| {
            // SAFETY: as preflight; the timed region holds one crossing per
            // byte (the word encoder takes no per-symbol init).
            unsafe {
                let mut buf = vec![0u16; SIZE * 2 + 64];
                let mut enc: u32 = ryg_rans_sys::rans_word_sse41::rans_word_enc_init();
                let mut wptr = buf.as_mut_ptr().add(buf.len());
                for &s in black_box(data).iter().rev() {
                    ryg_rans_sys::rans_word_sse41::rans_word_enc_put(
                        &mut enc,
                        &mut wptr,
                        cum[s as usize],
                        freqs[s as usize],
                    );
                }
                ryg_rans_sys::rans_word_sse41::rans_word_enc_flush(&mut enc, &mut wptr);
                black_box(wptr)
            }
        });
    });
    group.finish();

    // ---- Decode ----
    let mut group = c.benchmark_group("comparative/word-rans/decode/1MiB/SKEWED_255_1");
    group.throughput(Throughput::Bytes(SIZE as u64));
    group.bench_function("rust-core", |b| {
        let comp = rust_comp.clone();
        b.iter(|| {
            let mut reader = Word16Reader::new(black_box(&comp));
            let mut state = ryg_rans_rs_core::rans_word_dec_init(&mut reader).expect("init");
            let mut out = vec![0u8; SIZE];
            for o in out.iter_mut() {
                *o = ryg_rans_rs_core::rans_word_dec_sym(&mut state, &r_tables, SCALE_BITS);
                ryg_rans_rs_core::rans_word_dec_renorm(&mut state, &mut reader).expect("renorm");
            }
            black_box(out.len())
        });
    });
    group.bench_function("ryg-rans-sys-c", |b| {
        let comp = rust_comp.clone();
        b.iter(|| {
            // SAFETY: as preflight; the timed region holds the two
            // mandatory crossings per byte (sym + renorm).
            unsafe {
                let mut rptr = comp.as_ptr() as *mut u16;
                let mut state: u32 = 0;
                ryg_rans_sys::rans_word_sse41::rans_word_dec_init(&mut state, &mut rptr);
                let mut out = vec![0u8; SIZE];
                for o in out.iter_mut() {
                    *o = ryg_rans_sys::rans_word_sse41::rans_word_dec_sym(&mut state, &c_tables);
                    ryg_rans_sys::rans_word_sse41::rans_word_dec_renorm(&mut state, &mut rptr);
                }
                black_box(out.len())
            }
        });
    });
    group.finish();
}

#[cfg(feature = "comparative-word-sse41")]
criterion_group!(
    name = comparative_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(8))
        .sample_size(50);
    targets = ffi_overhead, byte_rans_comparative, word_rans_comparative
);

#[cfg(not(feature = "comparative-word-sse41"))]
criterion_group!(
    name = comparative_benches;
    config = Criterion::default()
        .warm_up_time(std::time::Duration::from_secs(2))
        .measurement_time(std::time::Duration::from_secs(8))
        .sample_size(50);
    targets = ffi_overhead, byte_rans_comparative
);
criterion_main!(comparative_benches);
