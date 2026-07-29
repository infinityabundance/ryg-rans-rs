# ryg-rans-rs

> **Public facade for `ryg-rans-rs` — rANS entropy coding in Rust.**  
> Safe, `no_std`-compatible API. Re-exports the deterministic core, optionally adds SIMD decode kernels.

## Features

| Feature | Description | Default |
|---------|-------------|---------|
| `default` | Core re-export only | ✅ Yes |
| `simd` | Enables `ryg-rans-rs-simd` (SSE4.1 8-way interleaved word rANS decoder) | ❌ No |
| `alloc` | Adds `alloc_utils` module with convenience `encode`/`decode` using `Vec<u8>` | ❌ No |

## Modules

| Module | Source | Feature | Description |
|--------|--------|---------|-------------|
| `byte` | `ryg-rans-rs-core` | always | Complete rANS core: byte rANS, 64-bit rANS, word rANS, alias method |
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

- `0.1.13` — Current. Phase F seal: SSE4.1 SIMD decoder, 128 receipts.
- `0.1.12` — Phase F implementation (SIMD decoder, cross-courts).
- `0.1.11` — Phase E seal: alias method, 120 receipts.
- `0.1.10` — Phase E implementation (alias method, Vose table).
- `0.1.9` — Phase D seal: word rANS, Docker matrix stamp.
