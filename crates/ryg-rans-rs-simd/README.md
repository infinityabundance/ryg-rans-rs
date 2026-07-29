# ryg-rans-rs-simd

> **SSE4.1 accelerated rANS decoder kernels.**  
> 8-way interleaved word rANS decode with scalar fallback.

## Status

**Fully implemented and sealed** via 8 cross-decoding court receipts (`RYG_RANS.SIMD.INTERLEAVED8.*`).

| Backend | Description | Availability |
|---------|-------------|--------------|
| `decode_8way_scalar` | Pure-Rust scalar 8-way decode | Always |
| `decode_simd_8way` | Auto-selects SIMD at compile time when `sse4.1` target feature is enabled | `#[cfg(target_feature = "sse4.1")]` |
| `decode_simd_8way_unchecked` | Unsafe SSE4.1+SSSE3 path | Requires runtime feature check by caller |

## Performance

On the tested architecture (Ryzen 7 9800X3D), the scalar 8-way decoder is **~2.5× faster** than the SSE4.1 decoder. This is a known characteristic of the upstream gather-based design: the SSE4.1 path extracts lane indices into scalar registers, performs scalar table lookups, reconstructs vectors with insert instructions, and only then performs SIMD multiply-add. The scalar path avoids this gather overhead entirely.

The SIMD decoder remains valuable as:
- A faithful upstream-compatible implementation
- A cross-decoding and portability verification surface
- A baseline for future AVX-512 experiments
- Proof that "SIMD" is not automatically synonymous with "faster"

## Contents

### Scalar Decoders

- `RansWordDec` — Single 32-bit word rANS decoder state
- `rans_word_dec_init` / `rans_word_dec_sym` / `rans_word_dec_renorm` — Single-state word decode
- `decode_8way_scalar` — 8-state interleaved scalar decode (reference for SIMD comparison)

### SIMD Decoder

- `RansSimdDec` — 4-lane SSE4.1 decoder state wrapping `__m128i`
- `rans_simd_dec_init` — Load 4×32-bit states from 8×u16 words via `_mm_loadu_si128`
- `rans_simd_dec_sym_unchecked` — Decode 4 symbols: extract lane indices → scalar gather → `_mm_mullo_epi32` + `_mm_add_epi32`
- `rans_simd_dec_renorm_unchecked` — Sign-biased unsigned comparison → shuffle-mask byte extraction → `_mm_blendv_epi8`
- `decode_simd_8way_unchecked` — 8-way interleaved via two SIMD units (requires `#[target_feature(enable = "ssse3,sse4.1")]`)
- `decode_simd_8way` — Safe wrapper: dispatch to SIMD at compile time when SSE4.1 is enabled, scalar otherwise

### Tables

- `RansWordSlot` — Packed (bias << 16 | freq) u32 table entry
- `RansWordTables` — Slice references to slots and symbol tables
- `build_word_tables` — Build tables from frequency model
- `rans_word_tables_init_symbol` — Initialize table entries for one symbol

## Safety

The crate uses `unsafe` for SSE4.1 intrinsics in three `unsafe fn`:
- `rans_simd_dec_init` — Unaligned 128-bit load
- `rans_simd_dec_sym_unchecked` — SSE2 + SSE4.1 intrinsics for gather and arithmetic
- `rans_simd_dec_renorm_unchecked` — SSE2 + SSSE3 + SSE4.1 intrinsics for renorm

Each function is `#[target_feature(enable = "ssse3,sse4.1")]` gated. The safe `decode_simd_8way` uses compile-time `#[cfg(target_feature = "sse4.1")]` for dispatch. The `decode_simd_8way_unchecked` is `unsafe` and requires the caller to ensure CPU features.

## Feature Flags

- `default = []` — Core + scalar decoders only
- No optional features; SSE4.1 selection is at compile time via `target_feature`

## Build

```sh
# Build with SSE4.1 enabled
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo build

# Build with SSE4.1 + run tests
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo test
```
