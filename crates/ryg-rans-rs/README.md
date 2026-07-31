# ryg-rans-rs

> **Public facade for ryg-rans-rs — rANS entropy coding in Rust.**  
> Safe, `no_std`-compatible.  Re-exports the deterministic core from
> `ryg-rans-rs-core`, optionally adds SSE4.1-accelerated decode kernels from
> `ryg-rans-rs-simd`, and optionally adds heap-allocated convenience wrappers.

**Version: 0.1.30** (workspace) · **Phase L** · 2 tests (doc tests)

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [What This Crate Is NOT](#what-this-crate-is-not)
3. [Module Reference](#module-reference)
4. [Feature Matrix](#feature-matrix)
5. [Quick Start](#quick-start)
6. [Safety Boundaries](#safety-boundaries)
7. [Evidence Model](#evidence-model)
8. [Versioning](#versioning)
9. [Troubleshooting](#troubleshooting)
10. [Reading Order](#reading-order)

---

## What This Crate Is

This is the **single public entry point** for the ryg-rans-rs project.  It is
a **pure re-export and feature-gating layer** — it adds no algorithmic logic.
Two internal crates provide all functionality:

```
ryg-rans-rs-core  →  the algorithmic heart (no_std, forbid(unsafe_code))
ryg-rans-rs-simd  →  SIMD acceleration (no_std, ledgered unsafe under #[target_feature])
```

The facade (`src/lib.rs`) is `#![no_std]` and `#![deny(unsafe_code)]` —
note **`deny`, not `forbid`**: the crate contains no unsafe code, and the
attribute is set so any future accidental `unsafe` is a hard compile error.
All SIMD intrinsics live behind the `simd` feature in `ryg-rans-rs-simd`,
never in this crate.

### Re-export architecture

- **`byte` module** (always available): re-exports every public item from
  `ryg-rans-rs-core` (`pub use ryg_rans_rs_core::*`), including the
  `malformed` submodule.  Types: `RansByteState`, `Rans64State`,
  `RansWordState`, `RansByteEncSymbol`, `RansByteDecSymbol`, `Rans64EncSymbol`,
  `Rans64DecSymbol`, `BackwardByteWriter`, `ByteReader`, `SliceBackwardWriter`,
  `ByteInterleavedEncoder`, `ByteInterleavedDecoder`, `BackwardWord16Writer`,
  `BackwardWord32Writer`, `Word16Reader`, `Word32Reader`, `RansWordSlot`,
  `RansWordTables`, `AliasTable`, and the `rans_byte_*`, `rans64_*`,
  `rans_word_*`, `rans_byte_alias_*` function families.
- **`simd` module** (`simd` feature): re-exports a fixed, documented subset of
  `ryg-rans-rs-simd` (see [Module Reference](#module-reference)).
- **`alloc_utils` module** (`alloc` feature): two `Vec`-based convenience
  functions, `encode` and `decode` (see [Module Reference](#module-reference)).

---

## What This Crate Is NOT

- **Not an implementation.**  It performs no arithmetic, hashing, or stream
  parsing of its own; all behavior comes from the re-exported crates.
- **Not the parallel engine.**  Block-level parallelism lives in the separate
  `ryg-rans-rs-parallel` crate.
- **Not the CLI.**  The `ryg-rans` binary and the RYGRANS v1 container live in
  the separate `ryg-rans-rs-cli` crate.
- **Not a full SIMD surface.**  The `simd` module re-exports only the
  documented subset below.  The complete SIMD API (packed tables, explicit
  AVX2/AVX-512 `_checked` dispatchers, `DecodeBackend`/`DecodeResult`, and
  the `backends` / `packed_table` modules) lives in `ryg-rans-rs-simd` and is
  consumed directly by `ryg-rans-rs-parallel`; it is **not** re-exported here.

---

## Module Reference

### `byte` (always available)

Re-exports everything from `ryg-rans-rs-core`.  Representative items:

| Category | Items |
|----------|-------|
| State | `RansByteState`, `Rans64State`, `RansWordState` |
| Encode symbols | `RansByteEncSymbol`, `Rans64EncSymbol` |
| Decode symbols | `RansByteDecSymbol`, `Rans64DecSymbol` |
| Encode (32-bit) | `rans_byte_enc_put`, `rans_byte_enc_put_symbol`, `rans_byte_enc_flush` |
| Decode (32-bit) | `rans_byte_dec_init`, `rans_byte_dec_get`, `rans_byte_dec_advance_symbol`, `rans_byte_dec_renorm` |
| Encode (64-bit) | `rans64_enc_put`, `rans64_enc_put_symbol`, `rans64_enc_flush` |
| Decode (64-bit) | `rans64_dec_init`, `rans64_dec_get`, `rans64_dec_advance_symbol`, `rans64_dec_renorm` |
| Word rANS | `rans_word_enc_init`, `rans_word_enc_put`, `rans_word_enc_flush`, `rans_word_dec_init`, `rans_word_dec_sym`, `rans_word_dec_renorm` |
| I/O | `BackwardByteWriter`, `ByteReader`, `SliceBackwardWriter`, `BackwardWord16Writer`, `Word16Reader`, `BackwardWord32Writer`, `Word32Reader` |
| Interleaved | `ByteInterleavedEncoder`, `ByteInterleavedDecoder` |
| Alias | `AliasTable`, `rans_byte_alias_normalize_freqs`, `rans_byte_alias_build_table`, `rans_byte_alias_enc_put`, `rans_byte_alias_dec_get`, `rans_byte_alias_dec_renorm`, `rans_byte_alias_dec_advance` |
| Constants | `RANS_BYTE_L`, `RANS64_L`, `RANS_WORD_L` |
| Malformed-stream validation | `malformed::{validate_byte_compressed, validate_r64_compressed, validate_word_compressed, validate_freq_model, RenormGuard, ValidationError, has_dominant_symbol, ...}` |

### `simd` (`simd` feature)

Re-exports exactly:

| Item | Notes |
|------|-------|
| `RANS_WORD_L`, `RANS_WORD_M`, `RANS_WORD_SCALE_BITS` | Word-rANS constants |
| `RansWordSlot`, `RansWordTables` | Slot / table types for the 8-way kernels |
| `build_word_tables(freqs, cum_freqs, scale_bits)` | Build `(slots, slot2sym)` |
| `rans_word_tables_init_symbol(...)` | Fill one symbol's slots |
| `decode_8way_scalar(compressed, tables, expected_len)` | Portable scalar 8-way decode |
| `decode_simd_8way(compressed, tables, expected_len)` | **Safe**: SSE4.1 8-way decode when compiled with `sse4.1`, scalar fallback otherwise (compile-time `cfg`, no runtime detection) |
| `decode_simd_8way_unchecked(...)` | **`unsafe fn`** with `#[target_feature(enable = "ssse3,sse4.1")]`; the caller must guarantee runtime SSSE3 + SSE4.1 |
| `RansSimdDec` | SIMD decoder state (4 × 32-bit lanes) |
| `rans_simd_dec_init(...)` | Initialize a `RansSimdDec` |

The `unsafe fn` is listed in the machine-verified ledger
(`crates/ryg-rans-rs-simd/unsafe-ledger.toml`) and carries its own
`#[target_feature]` attributes and a `# Safety` section — callers are not
relied on for target features.  Every other item here is safe.

### `alloc_utils` (`alloc` feature)

| Function | Signature behavior |
|----------|--------------------|
| `encode(symbols, esyms, scale_bits) -> Vec<u8>` | Encodes a byte slice with precomputed `RansByteEncSymbol`s into a heap-allocated buffer |
| `decode(encoded, cum2sym, dsyms, scale_bits, num_symbols) -> Vec<u8>` | Decodes into a heap-allocated buffer |

Both use the core's manual APIs internally and **may panic** on malformed or
truncated input (documented in `src/lib.rs`); for controlled environments use
the manual API with `BackwardByteWriter` / `ByteReader` and typed
`EncodeError` / `DecodeError`.

---

## Feature Matrix

| Feature | Default | What It Enables | `no_std` |
|---------|---------|-----------------|----------|
| (none) | ✅ | `byte` module only | ✅ |
| `simd` | — | `simd` module (re-exports from `ryg-rans-rs-simd`) | ✅ (with `alloc` where the kernels allocate; the crate itself is `#![no_std]`) |
| `alloc` | — | `alloc_utils` module + `ryg-rans-rs-core/alloc` | ✅ (with a global allocator) |

Composition: `simd` and `alloc` are independent.  `alloc` + `simd` enables
everything this facade exposes.  The facade declares no `std` dependency in
any feature combination.

---

## Quick Start

### Basic byte rANS encode/decode (no features)

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol, RansByteDecSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_get,
    rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let freq = (1u32 << scale_bits) / 256; // uniform 256-symbol model
let mut buf = [0u8; 4096];

let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
let sym = RansByteEncSymbol::new(0, freq, scale_bits).unwrap();
rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let dsym = RansByteDecSymbol::new(0, freq).unwrap();
let cf = rans_byte_dec_get(&dec_state, scale_bits);
rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
```

### Convenience API (`alloc` feature)

```rust
# #[cfg(feature = "alloc")] {
use ryg_rans_rs::byte::RansByteEncSymbol;
use ryg_rans_rs::alloc_utils;

let scale_bits = 14;
let total = 1u32 << scale_bits;
let base_freq = total / 256;

let esyms: Vec<_> = (0..256)
    .map(|i| RansByteEncSymbol::new(i * base_freq, base_freq, scale_bits).unwrap())
    .collect();

let data = b"Hello, rANS!";
let encoded = alloc_utils::encode(data, &esyms, scale_bits);

let cum2sym: Vec<u8> = (0..total as usize)
    .map(|i| (i / base_freq as usize) as u8)
    .collect();

let dsyms: Vec<_> = (0..256)
    .map(|i| ryg_rans_rs::byte::RansByteDecSymbol::new(i * base_freq, base_freq).unwrap())
    .collect();

let decoded = alloc_utils::decode(&encoded, &cum2sym, &dsyms, scale_bits, data.len());
assert_eq!(&decoded, data);
# }
```

### Malformed-input validation

```rust
use ryg_rans_rs::byte::malformed::{
    validate_byte_compressed,
    RenormGuard,
    ValidationError,
};

if let Err(e) = validate_byte_compressed(untrusted_input) {
    // e: ValidationError (TruncatedStream, ExcessiveRenormalization, ...)
}
```

---

## Safety Boundaries

| Layer | Attribute | Unsafe code |
|-------|-----------|-------------|
| `ryg-rans-rs-core` | `#![forbid(unsafe_code)]` | none |
| **`ryg-rans-rs` (this crate)** | `#![deny(unsafe_code)]`, `#![no_std]` | none |
| `ryg-rans-rs-simd` | ledgered | every `unsafe fn` listed in `unsafe-ledger.toml`, each with its own `#[target_feature]` and a `# Safety` section |

The `simd` module's safe entry points are compile-time-gated (`cfg!` /
`#[cfg(target_feature)]`); the one `unsafe fn` re-exported here
(`decode_simd_8way_unchecked`) requires the caller to guarantee runtime
SSSE3 + SSE4.1 support, exactly as its documentation states.

---

## Evidence Model

- The facade **adds no algorithmic surface of its own**, so it has no
  behaviour or performance receipts.  Its own test surface is the two doc
  tests shown above (`cargo test -p ryg-rans-rs`).
- The underlying surfaces carry the project evidence: the Phase K baseline of
  **144 behavioural receipts** across the core/SIMD surfaces (byte, R64,
  word, alias, SSE4.1, AVX512VL.INTERLEAVED8, AVX512.INTERLEAVED16), with the
  Phase L courts extending the total.  Performance receipts are being
  re-sealed in Phase L.18 (the Phase K run is superseded — gap ledger
  L1-A…L1-S); no performance claim is marked **Sealed** until the seal gate
  passes.
- Claim-check path: claim → producing code path (in `ryg-rans-rs-core` or
  `ryg-rans-rs-simd`) → court/test → receipt in `evidence/` → `cargo xtask
  seal`.

---

## Versioning

- Version **0.1.30**, shared with the workspace.  The facade is a re-export
  layer, so its API surface tracks the versions of `ryg-rans-rs-core` and
  `ryg-rans-rs-simd` (both 0.1.30).
- The public API inventory is generated by `cargo public-api` into
  `docs/public-api/` — do not hand-edit those files.

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `use ryg_rans_rs::simd::decode_interleaved8_auto;` fails to compile | The full SIMD API (`backends`, `packed_table`, `PackedWordTable`, `DecodeBackend`, `DecodeResult`) is **not** re-exported by the facade; depend on `ryg-rans-rs-simd` directly or use the re-exported subset (`decode_simd_8way`, `decode_8way_scalar`, ...) |
| `use ryg_rans_rs::alloc_utils::encode_byte` fails | The alloc helpers are named `encode` and `decode`, not `encode_byte` / `decode_byte` |
| SIMD decode falls back to scalar | `decode_simd_8way` uses the scalar path when the build lacks `sse4.1`; compile with `RUSTFLAGS="-C target-cpu=native"` (or the equivalent feature flags) for the SIMD path |
| `alloc_utils` panics on short input | Documented behavior; the convenience wrappers use `expect` internally.  Use the manual API for untrusted input |

---

## Reading Order

1. `docs/glossary.md` — exact project terminology.
2. `docs/architecture.md` and the root `README.md` (crate map, evidence
   status).
3. `crates/ryg-rans-rs-core/src/lib.rs` — the re-exported surface.
4. `crates/ryg-rans-rs-simd/src/lib.rs` + `unsafe-ledger.toml` — the SIMD
   surface.
5. `docs/bitstream-contract.md` and `docs/container-format-v1.md`.
6. `evidence/phase-l/gap-ledger.md`.

---

*Part of the ryg-rans-rs project. Version 0.1.30. Phase L.*
