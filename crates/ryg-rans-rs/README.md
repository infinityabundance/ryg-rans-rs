# ryg-rans-rs

> **Public facade for `ryg-rans-rs` — rANS entropy coding in Rust.**  
> Safe, `no_std`-compatible API. Re-exports the deterministic core, optionally adds SIMD decode kernels.  
> 128 behavioural receipts across 5 algorithmic surfaces, sealed via bit-exact C↔Rust cross-decoding courts.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `default` | Core re-export only | ✅ Yes |
| `simd` | Enables `ryg-rans-rs-simd` (SSE4.1 8-way interleaved word rANS decoder) | ❌ No |
| `alloc` | Adds `alloc_utils` module with convenience `encode`/`decode` using `Vec<u8>` | ❌ No |

## Modules

| Module | Source | Feature | Description |
|--------|--------|---------|-------------|
| `byte` | `ryg-rans-rs-core` | always | Complete rANS core: byte rANS, 64-bit rANS, word rANS, alias method, malformed-stream validation |
| `simd` | `ryg-rans-rs-simd` | `simd` | SSE4.1 8-way interleaved SIMD word rANS decoder |
| `alloc_utils` | this crate | `alloc` | Convenience encode/decode with `Vec<u8>` |

## SIMD Module

The `simd` module (behind the `simd` feature) provides:

- `decode_simd_8way` — Safe 8-way word rANS decode (auto-selects SIMD or scalar)
- `decode_simd_8way_unchecked` — Unsafe SSE4.1+SSSE3 path (requires runtime feature check)
- `decode_8way_scalar` — Pure-Rust scalar 8-way reference decoder
- `build_word_tables` — Build 4096-slot frequency/bias decode tables
- `RansSimdDec` — 4-lane SIMD decoder state

**Note**: On the tested architecture (Ryzen 7 9800X3D), the scalar 8-way decoder outperforms the SSE4.1 decoder by ~2.5× due to gather overhead in the upstream algorithm design. The SIMD decoder is provided for cross-decoding verification and as a baseline for future AVX-512 work.

## Malformed-Stream Hardening

The `byte` module includes a `malformed` sub-module providing:

- **Pre-decode validation**: Check compressed stream integrity before entering decoder hot paths.
- **Renormalization guards**: Loop-bounded renormalization to prevent infinite loops on corrupted input.
- **Frequency model validation**: Verify cumulative frequencies are monotonically non-decreasing and within range.
- **Edge-case detection**: Identify dominant-symbol, single-symbol, and freq=1 models.

## Fuzzing

The workspace includes 5 `cargo-fuzz` targets for continuous security testing:

- `byte_rans_roundtrip` — Division and reciprocal byte rANS round-trip
- `r64_rans_roundtrip` — 64-bit rANS round-trip (division + reciprocal)
- `word_rans_roundtrip` — Word rANS single-state round-trip
- `malformed_byte` — Randomly truncated/corrupted byte rANS streams
- `alias_roundtrip` — Alias method round-trip

## Formal Proofs (Kani)

The core crate includes Kani proof harnesses for critical arithmetic properties:

- **Encoder symbol init correctness**: Valid parameters always produce `Ok`.
- **Reciprocal = division equivalence**: The fast reciprocal path matches the division-based reference on every valid input.
- **Encode-decode inversion**: `decode(encode(x)) = x` for the core formula.
- **R64 reciprocal = division**: 64-bit variant of the reciprocal proof.

All proofs pass under bounded model checking with Kani.

## Quick Start

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let total = 1u32 << scale_bits;
let freq = total / 256;
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
rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits).unwrap();
```

## Published Versions

- `0.1.14` — Current. Phase H: malformed-stream hardening, fuzzing, Kani proofs, performance benchmarks.
- `0.1.13` — Phase F seal: SSE4.1 SIMD decoder, 128 receipts.
- `0.1.12` — Phase F implementation (SIMD decoder, cross-courts).
- `0.1.11` — Phase E seal: alias method, 120 receipts.
- `0.1.10` — Phase E implementation (alias method, Vose table).
- `0.1.9` — Phase D seal: word rANS, Docker matrix stamp.
