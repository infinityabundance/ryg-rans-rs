# ryg-rans-rs-simd

> **SSE4.1 + AVX-512 accelerated rANS decoder kernels.**  
> 8-way interleaved Word rANS decode with AVX512VL, SSE4.1, and scalar backends.  
> 16-way interleaved Word rANS decode with AVX-512 and scalar backends.  
> `#![no_std]` — works in embedded and kernel contexts on x86_64.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-simd)](https://crates.io/crates/ryg-rans-rs-simd)

## Status

**Fully implemented and sealed** — 16 new AVX-512 behavioral receipts  
(8 × `AVX512VL.INTERLEAVED8`, 8 × `AVX512.INTERLEAVED16`).

| Surface | Backends | Stream Format | ISA Required |
|---------|----------|---------------|--------------|
| 8-way interleaved | scalar, SSE4.1, AVX512VL | Existing canonical 8-way | `avx512f+avx512vl+avx512bw` |
| 16-way interleaved | scalar, AVX-512 | New 16-way format | `avx512f+avx512bw` |

## Backends

| Backend | Label | Available | Description |
|---------|-------|-----------|-------------|
| Scalar 8-way | `scalar-8way` | Always | Pure-Rust scalar reference |
| SSE4.1 8-way | `sse41-8way` | `target_feature("sse4.1")` | 4-lane SIMD decode |
| AVX512VL 8-way | `avx512vl-8way` | `target_feature("avx512f,avx512vl,avx512bw")` | 8-lane AVX-512 gather decode |
| Scalar 16-way | `scalar-16way` | Always | Pure-Rust 16-state scalar |
| AVX-512 16-way | `avx512-16way` | `target_feature("avx512f,avx512bw")` | 16-lane AVX-512 gather decode |

## Performance

Performance characteristics are documented separately. Initial measurements on Ryzen 7 9800X3D:
- Scalar 8-way is ~2.5× faster than SSE4.1 (known gather overhead)
- AVX512VL 8-way and AVX512 16-way performance is pending benchmark receipts

## AVX512VL.INTERLEAVED8

Decodes the **existing canonical 8-way Word rANS stream format** using AVX-512VL 256-bit vectors. Compatible with all existing encoders and decoders.

### ISA requirements
- `avx512f` — gathers, masked operations  
- `avx512vl` — 256-bit AVX-512 operations  
- `avx512bw` — byte/word operations  

### Key intrinsics
- `_mm256_i32gather_epi32` — packed table gather
- `_mm256_cmplt_epu32_mask` — renormalization mask
- `_mm256_mullo_epi32` — state update multiply

### Safe API
```rust
pub fn decode_interleaved8_auto(...) -> Result<DecodeResult, DecodeError>;
pub fn decode_interleaved8_avx512vl(...) -> Result<DecodeResult, DecodeError>;
pub fn decode_interleaved8_scalar(...) -> Result<DecodeResult, DecodeError>;
```

## AVX512.INTERLEAVED16

A new 16-state interleaved Word rANS format with explicit stream specification.

### Stream format
- 16 independent 32-bit rANS states
- Word renormalization (L = 2^16)
- Fixed `scale_bits = 12`
- Reverse-flush ordering: states flushed 15 → 0
- Forward init ordering: states loaded 0 → 15
- Lane assignment: `lane = i & 15`

### ISA requirements
- `avx512f` — 512-bit gathers, masked operations  
- `avx512bw` — byte/word operations  

### Key intrinsics
- `_mm512_i32gather_epi32` — packed table gather
- `_mm512_cmplt_epu32_mask` — renormalization mask
- `_mm512_mullo_epi32` — state update multiply

### Safe API
```rust
pub fn decode_interleaved16_auto(...) -> Result<DecodeResult, DecodeError>;
pub fn decode_interleaved16_avx512(...) -> Result<DecodeResult, DecodeError>;
pub fn decode_interleaved16_scalar(...) -> Result<DecodeResult, DecodeError>;
```

## Packed Decode Table

All AVX-512 kernels use a packed `u32` decode table:

```text
bits  0..11   frequency  (12 bits, max 4095)
bits 12..23   bias       (12 bits, max 4095)
bits 24..31   symbol     (8 bits)
```

4096 entries, 64-byte aligned, heap-allocated. Equivalent to the existing `RansWordSlot + slot2sym` representation.

## Unsafe Code

This crate contains 7 `unsafe fn` for SSE4.1 and AVX-512 intrinsics. All are:
- Gated by `#[target_feature(enable = "...")]`
- Documented with preconditions, bounds, CPU features, and soundness justification
- Listed in `docs/unsafe-ledger.md`

The safe auto-dispatch APIs perform runtime feature detection before calling unsafe kernels.

## Mask Exhaustion Tests

- **8-way**: all 256 renormalization masks verified
- **16-way**: all 65,536 renormalization masks verified (ignored by default, run with `--release`)

## Malformed Input Tests

All decoders reject:
- Truncated streams (empty, partial init, missing renorm words)
- Wrong-format streams (8-way → 16-way decoder, 16-way → 8-way decoder)
- State invariants are preserved across all tested cases

## Feature Flags

- `default = []` — scalar backends only
- `std` — enables runtime CPU feature detection for auto-dispatch

## Build

```sh
# Build with AVX-512 and SSE4.1
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" cargo build

# Run all tests (32 tests)
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" cargo test

# Run exhaustive 16-way mask test (requires --release)
RUSTFLAGS="-C target-feature=+avx512f,+avx512bw" cargo test --release -p ryg-rans-rs-simd -- --ignored
```
