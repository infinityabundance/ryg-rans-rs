# ryg-rans-rs

> **Public facade for `ryg-rans-rs` — rANS entropy coding in Rust.**  
> Safe, `no_std`-compatible API. Re-exports the deterministic core, optionally adds SSE4.1 and AVX-512 decode kernels.  
> 144 behavioral receipts across 7 algorithmic surfaces, sealed via bit-exact C↔Rust cross-decoding courts.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `default` | Core re-export only | ✅ Yes |
| `simd` | Enables `ryg-rans-rs-simd` (SSE4.1 + AVX-512 decode kernels) | ❌ No |
| `alloc` | Adds `alloc_utils` module with convenience `encode`/`decode` using `Vec<u8>` | ❌ No |

## Modules

| Module | Source | Feature | Description |
|--------|--------|---------|-------------|
| `byte` | `ryg-rans-rs-core` | always | Complete rANS core: byte rANS, 64-bit rANS, word rANS, alias method, malformed-stream validation |
| `simd` | `ryg-rans-rs-simd` | `simd` | SSE4.1 8-way, AVX512VL 8-way, AVX512 16-way interleaved Word rANS decoders |

## SIMD Module

The `simd` module (behind the `simd` feature) provides three decode surfaces:

### 8-way Interleaved (existing format)
- `decode_interleaved8_auto` — Auto-selects AVX512VL → SSE4.1 → scalar
- `decode_interleaved8_scalar` — Pure-Rust scalar 8-way reference
- `decode_interleaved8_avx512vl` — Explicit AVX512VL kernel

### 16-way Interleaved (new format)
- `decode_interleaved16_auto` — Auto-selects AVX512 → scalar
- `decode_interleaved16_scalar` — Pure-Rust scalar 16-way reference
- `decode_interleaved16_avx512` — Explicit AVX-512 kernel
- `encode_interleaved16` — 16-way encoder for the new format

### Packed Table
- `PackedWordTable` — u32-packed 4096-slot decode table for gather operations
- `PackedWordEntry` — Single entry with freq/bias/symbol extraction

## AVX-512 ISA Requirements

| Surface | Required Features | Stream Format |
|---------|-------------------|---------------|
| `AVX512VL.INTERLEAVED8` | `avx512f, avx512vl, avx512bw` | Existing 8-way (compatible) |
| `AVX512.INTERLEAVED16` | `avx512f, avx512bw` | New 16-way format |

## Published Versions

- `0.1.15` — Current. Phase G: AVX512VL + AVX512 decode kernels.
- `0.1.14` — Phase H: malformed-stream hardening, fuzzing, Kani proofs.
- `0.1.13` — Phase F seal: SSE4.1 SIMD decoder, 128 receipts.
- `0.1.12` — Phase F implementation (SIMD decoder, cross-courts).
- `0.1.11` — Phase E seal: alias method, 120 receipts.
- `0.1.10` — Phase E implementation (alias method, Vose table).
- `0.1.9` — Phase D seal: word rANS, Docker matrix stamp.
