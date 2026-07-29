# ryg-rans-rs

> **Public facade for `ryg-rans-rs` — rANS entropy coding in Rust.**  
> Safe, `no_std`-compatible API. Re-exports the deterministic core, optionally adds SSE4.1 and AVX-512 decode kernels.  
> 144 behavioral receipts across 7 algorithmic surfaces, sealed via bit-exact C↔Rust cross-decoding courts.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

---

## Architecture Overview

This is the **single public entry point** for the ryg-rans-rs project. It re-exports
functionality from two internal crates:

```text
ryg-rans-rs-core  →  algorithmic heart (no_std, no unsafe)
    └── byte module: Byte rANS, R64, Word rANS, Alias, malformed validation

ryg-rans-rs-simd  →  SIMD acceleration (no_std, selective unsafe)
    └── simd module: SSE4.1 8-way, AVX512VL 8-way, AVX512 16-way decoders
```

The facade adds no algorithmic logic — it is purely a re-export and feature-gating layer.
This means:
- Users get exactly one crate to depend on: `ryg-rans-rs`
- The core remains independent of SIMD concerns
- The SIMD module is entirely optional

---

## API Surfaces

### `byte` Module (Always Available)

The `byte` module re-exports all types from `ryg-rans-rs-core`:

| Sub-module | Contents | When to Use |
|------------|----------|-------------|
| (top-level) | `RansByteState`, `RansByteEncSymbol`, `RansByteDecSymbol` | General rANS work |
| (top-level) | `rans_byte_enc_put`, `rans_byte_enc_put_symbol` | Encoding |
| (top-level) | `rans_byte_dec_init`, `rans_byte_dec_advance_symbol` | Decoding |
| (top-level) | `BackwardByteWriter`, `ByteReader`, `SliceBackwardWriter` | I/O |
| (top-level) | `ByteInterleavedEncoder`, `ByteInterleavedDecoder` | Two-state interleaved |
| (top-level) | `RANS_BYTE_L`, `RANS64_L`, `RANS_WORD_L` | Constants |
| `malformed` | `validate_byte_compressed`, `RenormGuard`, `validate_freq_model` | Stream validation |
| (r64 types) | `Rans64State`, `Rans64EncSymbol`, `Rans64DecSymbol` | 64-bit rANS |
| (word types) | `RansWordState`, `RansWordSlot`, `RansWordTables` | Word rANS |
| (alias types) | `AliasTable`, `rans_byte_alias_*` | Alias method |

### `simd` Module (Behind `simd` Feature)

The `simd` module provides three decode surfaces + packed table:

| API | Surface | Description |
|-----|---------|-------------|
| `decode_interleaved8_auto` | 8-way auto | Selects AVX512VL → SSE4.1 → scalar |
| `decode_interleaved8_scalar` | 8-way scalar | Pure-Rust reference |
| `decode_interleaved8_avx512vl` | 8-way AVX512VL | Explicit AVX512VL kernel |
| `decode_interleaved16_auto` | 16-way auto | Selects AVX512 → scalar |
| `decode_interleaved16_scalar` | 16-way scalar | Pure-Rust reference |
| `decode_interleaved16_avx512` | 16-way AVX512 | Explicit AVX512 kernel |
| `encode_interleaved16` | 16-way encoder | Produces the new 16-way format |
| `PackedWordTable` | Table | 4096-slot u32 packed table for gathers |
| `DecodeBackend` | Enum | Backend identification with stable labels |

### `alloc_utils` Module (Behind `alloc` Feature)

Convenience functions that wrap the core primitives with `Vec<u8>` allocation:

```rust
pub fn encode_byte(...) -> Result<Vec<u8>, EncodeError>;
pub fn decode_byte(...) -> Result<Vec<u8>, DecodeError>;
```

---

## Feature Matrix

| Feature | What It Enables | no_std Compatible | Use Case |
|---------|----------------|-------------------|----------|
| (default) | Core re-export only | ✅ Yes | Minimal builds |
| `simd` | `ryg-rans-rs-simd` (all backends) | ✅ Yes (cfg dispatch) | SIMD acceleration |
| `alloc` | `alloc_utils` + alias table | ✅ Yes (extern alloc) | Vec-based APIs |

---

## Quick Start

### Basic Encode/Decode

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let freq = (1u32 << scale_bits) / 256;
let mut buf = [0u8; 4096];

// Encode a single symbol
let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
let sym = RansByteEncSymbol::new(0, freq, scale_bits).unwrap();
rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

// Decode it back
let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let dsym = ryg_rans_rs::byte::RansByteDecSymbol::new(0, freq).unwrap();
rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
```

### AVX-512 Decode

```rust
#[cfg(feature = "simd")]
{
    use ryg_rans_rs::simd::backends::decode_interleaved8_auto;
    use ryg_rans_rs::simd::packed_table::PackedWordTable;

    let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
    let result = decode_interleaved8_auto(&compressed, &packed, expected_len).unwrap();
    println!("Backend: {}", result.backend.label());
    assert_eq!(result.output, expected_output);
}
```

### Malformed Input Validation

```rust
use ryg_rans_rs::byte::malformed::validate_byte_compressed;

if let Err(e) = validate_byte_compressed(compressed) {
    return Err(e);  // "compressed stream is truncated"
}
```

---

## ISA Feature Requirements for SIMD

| Backend | Required `target_feature` Flags | CPU Support |
|---------|-------------------------------|-------------|
| SSE4.1 8-way | `+ssse3,+sse4.1` | Intel Core 2+ (2008+), AMD Bulldozer+ (2011+) |
| AVX512VL 8-way | `+avx512f,+avx512vl,+avx512bw` | Intel Ice Lake+ (2019+), AMD Zen 4+ (2022+) |
| AVX512 16-way | `+avx512f,+avx512bw` | Intel Ice Lake+ (2019+), AMD Zen 4+ (2022+) |

Build with: `RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" cargo build`

---

## Published Versions

| Version | Phase | Key Changes |
|---------|-------|-------------|
| **0.1.15** | **G** | **AVX512VL 8-way + AVX512 16-way decode kernels** |
| 0.1.14 | H | Malformed-stream hardening, fuzzing, Kani proofs |
| 0.1.13 | F | SSE4.1 SIMD decoder, 128 receipts |
| 0.1.12 | F | SIMD implementation, cross-courts |
| 0.1.11 | E | Alias method seal, 120 receipts |
| 0.1.10 | E | Alias implementation |
| 0.1.9 | D | Word rANS seal, Docker stamp |

---

## Safety

- Core crate: `#![forbid(unsafe_code)]` — compile-time guarantee
- Facade crate: `#![deny(unsafe_code)]` — compile-time guarantee  
- SIMD crate: 7 `unsafe fn`, all `#[target_feature]`-gated and documented in `docs/unsafe-ledger.md`
- Safe auto-dispatch functions perform runtime feature detection before calling SIMD kernels
- No `unsafe` code can execute on a CPU that doesn't support it through the safe API
