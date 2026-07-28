# ryg-rans-rs-core

> Deterministic algorithmic core of rANS entropy coding — `#![no_std]`, `#![forbid(unsafe_code)]`

[![#![no_std]](https://img.shields.io/badge/std-no--std-blue)](https://docs.rs/ryg-rans-rs-core)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success)](https://github.com/rust-secure-code/safety-dance/)
[![MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/infinityabundance/ryg-rans-rs)
[![Edition](https://img.shields.io/badge/edition-2024-purple)](https://doc.rust-lang.org/edition-guide/editions/2024-edition.html)

## Overview

This crate is the algorithmic heart of the `ryg-rans-rs` workspace. It faithfully reconstructs the
[`rans_byte.h`](https://github.com/rygorous/ryg_rans) and [`rans64.h`](https://github.com/rygorous/ryg_rans)
reference implementations from Fabian "ryg" Giesen's canonical ryg_rans repository, ported to safe,
idiomatic Rust.

The crate implements **asymmetric numeral systems (ANS)** entropy coding at the byte/word level —
specifically the range-based variant (rANS). It provides both 32-bit byte-aligned and 64-bit
word-aligned variants, each with a division-based reference path and a reciprocal-multiply fast
path. Two-state interleaved encoding/decoding is included for higher throughput on modern
processors.

This is a `#![no_std]` crate with zero unsafe code. Every function is a direct semantic port of the
upstream C, verified against 44 unit tests (including an oracle reciprocal-parameter verification
test and full roundtrip checks for both byte and 64-bit variants).

## Features

- **32-bit byte rANS** (`rans_byte.h` semantics):
  - Division-based reference encode/decode (`rans_byte_enc_put`, `rans_byte_dec_advance`)
  - Reciprocal-multiply fast-path encode (`rans_byte_enc_put_symbol`)
  - Byte-aligned renormalization (8-bit emit/consume)
  - 23-bit state-space lower bound (`RANS_BYTE_L = 1 << 23`)
  - Reverse-order encoding with backward-growing output
- **64-bit word rANS** (`rans64.h` semantics):
  - Division-based reference encode/decode (`rans64_enc_put`, `rans64_dec_advance`)
  - Reciprocal-multiply fast-path encode via `rans64_mul_hi` (`rans64_enc_put_symbol`)
  - 32-bit word-aligned renormalization (32-bit emit/consume)
  - 63-bit effective state space (`RANS64_L = 1 << 31`)
  - Reverse-order encoding with backward-growing word output
- **Two-state interleaving** (byte variant):
  - `ByteInterleavedEncoder` — encodes symbols into two interleaved rANS streams
  - `ByteInterleavedDecoder` — decodes two interleaved rANS streams back
  - Handles odd-length symbol arrays with a single trailing symbol
- **I/O abstraction traits**:
  - `BackwardWriter` — trait for backward-growing byte output
  - `ForwardReader` — trait for forward-growing byte input
  - `BackwardByteWriter`, `SliceBackwardWriter`, `ByteReader` — concrete implementations
  - `BackwardWord32Writer`, `Word32Reader` — word-aligned I/O for 64-bit variant
- **Symbol precomputation**:
  - `RansByteEncSymbol` / `RansByteDecSymbol` — encoder/decoder symbol tables for byte rANS
  - `Rans64EncSymbol` / `Rans64DecSymbol` — encoder/decoder symbol tables for 64-bit rANS
  - Reciprocal parameters computed with oracle-verified precision
- **Error handling**:
  - `EncodeError` — `OutputTooSmall` when the output buffer is exhausted
  - `DecodeError` — `InputTooShort` or `InvalidState` on decoding failure
- **Zero dependencies** in default configuration
- Optional `alloc` feature for test infrastructure

## Usage Example

```rust
use ryg_rans_rs_core::*;

// Precompute an encoding symbol for frequency 3 in a 14-bit scale
let esym = RansByteEncSymbol::new(3, 14).unwrap();
let dsym = RansByteDecSymbol::new(3, 3, 14);

// Allocate an output buffer (worst-case: 4 bytes per symbol + flush)
let mut buf = vec![0u8; 64];
let mut writer = BackwardByteWriter::new(&mut buf);

// Encode: start with initial state, encode symbol, flush
let mut state = RansByteState::new();
rans_byte_enc_put_symbol(&mut state, &mut writer, &esym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();

let encoded = writer.encoded();

// Decode: read initial state, advance, renormalize
let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let cf = rans_byte_dec_get(&dec_state, 14);
// cf == 3 — the cumulative frequency of our symbol
rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, 14).unwrap();
assert_eq!(dec_state, RansByteState::new());
```

### Two-state interleaved usage

```rust
use ryg_rans_rs_core::*;

let symbols = [2, 5, 1, 7, 3];
let scale_bits = 14;

// Precompute symbols for an alphabet with frequencies summing to 2^14
let esyms: Vec<_> = (0..8).map(|freq| {
    RansByteEncSymbol::new(freq.max(1), scale_bits).unwrap()
}).collect();

let mut buf = vec![0u8; 128];
let mut writer = BackwardByteWriter::new(&mut buf);

let mut encoder = ByteInterleavedEncoder::new(&mut writer, scale_bits);
encoder.finalize(&symbols, &esyms).unwrap();

let encoded = writer.encoded();

let mut reader = ByteReader::new(encoded);
let mut decoder = ByteInterleavedDecoder::new(&mut reader, scale_bits).unwrap();

let dsyms: Vec<_> = (0..8).map(|freq| {
    RansByteDecSymbol::new(freq.max(1), freq.max(1), scale_bits)
}).collect();

let cum2sym: Vec<u8> = (0..256).map(|i| (i % 8) as u8).collect();
let mut output = vec![0u8; symbols.len()];
decoder.decode(&mut output, &cum2sym, &dsyms).unwrap();

assert_eq!(&output, &symbols);
```

## Cargo Features

| Feature | Default | Description |
|---------|---------|-------------|
| `alloc` | No | Enable `alloc`-dependent test infrastructure (used by `#[cfg(test)]` and the facade crate) |

No other features are defined. The crate is fully functional with `default-features = false`.

## Safety

**No unsafe code.** The crate uses `#![forbid(unsafe_code)]` at the crate root. Every algorithmic
primitive is implemented in safe Rust using only `u32`, `u64`, `u128`, slices, and iterator
patterns. There are no raw pointers, no `union` access, no `core::mem::transmute`, and no FFI calls.

Reciprocal multiply parameters are verified against the upstream C implementation via an
oracle test (`test_oracle_reciprocal_parameters`) that recomputes division results and asserts
bit-exact equivalence.

## Performance

The crate provides two encoding paths:

1. **Division-based** (`rans_byte_enc_put` / `rans64_enc_put`): Uses hardware `DIV` instructions.
   Suitable for validation, prototyping, and platforms where division is fast.

2. **Reciprocal-multiply** (`rans_byte_enc_put_symbol` / `rans64_enc_put_symbol`): Replaces
   division with a fixed-point multiplication + shift sequence. On modern x86-64 and ARM64 this
   is ~3–5× faster than the division path.

The decoder always uses division in the advance step (reconstructing the state from frequency and
cumulative frequency). This matches the upstream C behavior.

The two-state interleaved encoder/decoder processes symbols in pairs, halving the number of
renormalization checks per symbol and enabling better instruction-level parallelism.

## Dependencies

| Dependency | Version | Feature | Notes |
|------------|---------|---------|-------|
| `core` (built-in) | — | — | `#![no_std]` — only `core` and `fmt` |

No crates.io runtime dependencies.
