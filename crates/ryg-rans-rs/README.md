# ryg-rans-rs

> Public facade for rANS entropy coding with safe Rust API

[![#![no_std]](https://img.shields.io/badge/std-no--std-blue)](https://docs.rs/ryg-rans-rs)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success)](https://github.com/rust-secure-code/safety-dance/)
[![MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/infinityabundance/ryg-rans-rs)
[![Edition](https://img.shields.io/badge/edition-2024-purple)](https://doc.rust-lang.org/edition-guide/editions/2024-edition.html)

## Overview

**ryg-rans-rs** is the public-facing entry point for the ryg-rans-rs workspace. It re-exports the
entire `ryg-rans-rs-core` algorithmic crate as the `byte` module, optionally enables SSE4.1
accelerated decode kernels via the `simd` feature, and provides allocation-based convenience APIs
via the `alloc` feature.

This crate serves as the single dependency for downstream users who want to perform rANS entropy
coding in Rust. Rather than depending on `ryg-rans-rs-core` directly (which exposes every
low-level primitive), consume this crate and choose your desired level of abstraction:

- **Manual API** — direct access to all core functions and types through the `byte` module
- **Convenience API** — one-shot `encode`/`decode` functions (with `alloc` feature)

The crate is `#![no_std]` and `#![deny(unsafe_code)]`. When the `alloc` feature is enabled, it
requires an allocator.

## Features

- **Re-exports `ryg-rans-rs-core`** as the `byte` module — all 32-bit and 64-bit rANS primitives,
  writers, readers, symbol tables, error types, traits, and interleaved encoders/decoders are
  available under `ryg_rans_rs::byte::*`.
- **Optional `simd` feature** (enabled by default) — reserves the `simd` module for future SSE4.1
  accelerated decoder kernels. Currently an empty module.
- **Optional `alloc` feature** — provides one-shot `encode` and `decode` convenience functions
  in `alloc_utils` that manage buffer allocation internally.
- **Zero-cost abstraction** — the `byte` module is a direct re-export with no wrapping overhead.
  The compiler sees the same code as a direct dependency on `ryg-rans-rs-core`.

## Usage Example

### Manual API (no allocator required)

```rust
use ryg_rans_rs::byte::*;

let esym = RansByteEncSymbol::new(3, 14).unwrap();
let dsym = RansByteDecSymbol::new(3, 3, 14);

let mut buf = [0u8; 64];
let mut writer = BackwardByteWriter::new(&mut buf);

let mut state = RansByteState::new();
rans_byte_enc_put_symbol(&mut state, &mut writer, &esym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();

let encoded = writer.encoded();

let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let cf = rans_byte_dec_get(&dec_state, 14);
rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, 14).unwrap();
assert_eq!(dec_state, RansByteState::new());
```

### Convenience API (requires `alloc`)

```rust
use ryg_rans_rs::byte::*;

// Precompute encoding symbols (e.g. for an alphabet with frequencies summing to 2^14)
let scale_bits = 14;
let esyms: Vec<_> = (0..8).map(|f| RansByteEncSymbol::new(f.max(1), scale_bits).unwrap()).collect();
let dsyms: Vec<_> = (0..8).map(|f| RansByteDecSymbol::new(f.max(1), f.max(1), scale_bits)).collect();
let cum2sym: Vec<u8> = (0..(1 << scale_bits)).map(|i| (i % 8) as u8).collect();

let input = [1, 0, 3, 2, 5, 4, 7, 6];

let encoded = ryg_rans_rs::alloc_utils::encode(&input, &esyms, scale_bits);
let decoded = ryg_rans_rs::alloc_utils::decode(&encoded, &cum2sym, &dsyms, scale_bits, input.len());

assert_eq!(decoded, input);
```

### Two-state interleaved

```rust
use ryg_rans_rs::byte::*;

let symbols = [2, 5, 1, 7, 3];
let scale_bits = 14;

let esyms: Vec<_> = (0..8).map(|f| RansByteEncSymbol::new(f.max(1), scale_bits).unwrap()).collect();
let dsyms: Vec<_> = (0..8).map(|f| RansByteDecSymbol::new(f.max(1), f.max(1), scale_bits)).collect();
let cum2sym: Vec<u8> = (0..(1 << scale_bits)).map(|i| (i % 8) as u8).collect();

let mut buf = vec![0u8; 128];
let mut writer = BackwardByteWriter::new(&mut buf);
let mut encoder = ByteInterleavedEncoder::new(&mut writer, scale_bits);
encoder.finalize(&symbols, &esyms).unwrap();
let encoded = writer.encoded();

let mut reader = ByteReader::new(encoded);
let mut decoder = ByteInterleavedDecoder::new(&mut reader, scale_bits).unwrap();
let mut output = vec![0u8; symbols.len()];
decoder.decode(&mut output, &cum2sym, &dsyms).unwrap();

assert_eq!(&output, &symbols);
```

## Cargo Features

| Feature | Default | Description |
|---------|---------|-------------|
| `simd`  | **Yes** | Enable SSE4.1 accelerated decode kernels (currently a scaffold — empty module) |
| `alloc` | No      | Enable allocation-based `encode`/`decode` convenience functions |

**Important note about `simd`:** This feature is enabled by default purely for forward-compatibility.
When the SIMD kernels are implemented, existing code that already has `simd` enabled will
automatically gain the acceleration. If you need guaranteed `#![no_std]` behavior at all times,
disable default features:

```toml
ryg-rans-rs = { version = "0.1", default-features = false, features = ["alloc"] }
```

## Module Structure

```
ryg_rans_rs
├── byte              # Re-export of ryg-rans-rs-core
│   ├── RansByteState / Rans64State
│   ├── BackwardByteWriter / ByteReader
│   ├── BackwardWord32Writer / Word32Reader
│   ├── RansByteEncSymbol / RansByteDecSymbol / Rans64EncSymbol / Rans64DecSymbol
│   ├── rans_byte_enc_* / rans_byte_dec_* functions
│   ├── rans64_enc_* / rans64_dec_* functions
│   ├── BackwardWriter / ForwardReader traits
│   ├── ByteInterleavedEncoder / ByteInterleavedDecoder
│   ├── EncodeError / DecodeError
│   └── …
├── simd              # [cfg(feature = "simd")] — SSE4.1 kernels (empty placeholder)
└── alloc_utils       # [cfg(feature = "alloc")] — convenience encode/decode
```

## Safety

This crate uses `#![deny(unsafe_code)]` at the crate root. All re-exported code from
`ryg-rans-rs-core` is `#![forbid(unsafe_code)]`. No unsafe code appears anywhere in the dependency
graph of this crate.

## Performance

The performance of this crate is identical to using `ryg-rans-rs-core` directly — the `byte` module
is a zero-cost re-export. For performance characteristics of the algorithmic primitives, see
the [ryg-rans-rs-core README](../ryg-rans-rs-core/README.md#performance).

## Dependencies

| Dependency | Version | Feature | Notes |
|------------|---------|---------|-------|
| `ryg-rans-rs-core` | `0.1.0` | — | Algorithmic core (always required) |
| `ryg-rans-rs-simd` | `0.1.0` | optional | SIMD kernels (scaffold, gated by `simd` feature) |
| `alloc` (built-in) | — | optional | Required by the `alloc` feature |
