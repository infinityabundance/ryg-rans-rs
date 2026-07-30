# ryg-rans-rs

> **Public facade for ryg-rans-rs — rANS entropy coding in Rust.**  
> Safe, `no_std`-compatible API. Re-exports the deterministic core, optionally adds SSE4.1 and AVX-512 decode kernels.  
> 144 behavioral receipts across 7 algorithmic surfaces, sealed via bit-exact C↔Rust cross-decoding courts.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [Architecture Overview](#architecture-overview)
3. [Module Reference](#module-reference)
4. [SIMD Module](#simd-module)
5. [Feature Matrix](#feature-matrix)
6. [Quick Start](#quick-start)
7. [Safety Guarantees](#safety-guarantees)
8. [Published Versions](#published-versions)

---

## What This Crate Is

This is the **single public entry point** for the ryg-rans-rs project. It re-exports
functionality from two internal crates:

```
ryg-rans-rs-core  →  the algorithmic heart (no_std, no unsafe)
ryg-rans-rs-simd  →  SIMD acceleration (no_std, selective unsafe under #[target_feature])
```

The facade adds **no algorithmic logic** — it is purely a re-export and feature-gating
layer. This means users get exactly one crate to depend on (`ryg-rans-rs`), while the
core and SIMD crates remain independently versioned and tested.

### Why a Facade Crate?

1. **Single dependency**: Users depend on one crate instead of three
2. **Feature isolation**: SIMD code is compiled only when the `simd` feature is enabled
3. **Versioning flexibility**: Core and SIMD can version independently
4. **Safety boundary**: The facade is `#![deny(unsafe_code)]` — users never directly
   interact with unsafe SIMD intrinsics

---

## Architecture Overview

### Internal structure

```
ryg-rans-rs (facade)
├── byte module (always available)
│   ├── Types: RansByteState, RansByteEncSymbol, RansByteDecSymbol
│   ├── Encode: rans_byte_enc_put, rans_byte_enc_put_symbol
│   ├── Decode: rans_byte_dec_init, rans_byte_dec_advance_symbol
│   ├── I/O: BackwardByteWriter, ByteReader, SliceBackwardWriter
│   ├── Interleaved: ByteInterleavedEncoder, ByteInterleavedDecoder
│   ├── Constants: RANS_BYTE_L, RANS64_L, RANS_WORD_L
│   ├── 64-bit: Rans64State, Rans64EncSymbol, Rans64DecSymbol
│   ├── Word: RansWordState, RansWordSlot, RansWordTables
│   └── Alias: AliasTable, rans_byte_alias_*
│
├── simd module (behind "simd" feature)
│   ├── 8-way: decode_interleaved8_auto, decode_interleaved8_scalar
│   ├── 16-way: decode_interleaved16_auto, decode_interleaved16_scalar
│   ├── Backends: DecodeBackend enum with stable labels
│   └── Table: PackedWordTable, PackedWordEntry
│
└── alloc_utils module (behind "alloc" feature)
    └── Convenience: encode_byte, decode_byte (Vec-based)
```

### Dependency flow

```
ryg-rans-rs (facade, #![deny(unsafe_code)])
    ↓ imports from ↓
ryg-rans-rs-core (#![forbid(unsafe_code)])     ryg-rans-rs-simd (unsafe fn, #[target_feature])
    ↓ depends on ↓
(no dependencies)                                 ryg-rans-rs-core
```

The core has no knowledge of SIMD. The SIMD crate builds on the core. The facade
provides unified access.

---

## Module Reference

### `byte` Module (Always Available)

The `byte` module re-exports every public type and function from `ryg-rans-rs-core`.
This includes:

| Category | Types/Functions | Description |
|----------|----------------|-------------|
| **State** | `RansByteState`, `Rans64State`, `RansWordState` | Encoder/decoder state wrappers |
| **Encode symbols** | `RansByteEncSymbol`, `Rans64EncSymbol` | Precomputed reciprocal params |
| **Decode symbols** | `RansByteDecSymbol`, `Rans64DecSymbol` | Decoder frequency/start pairs |
| **Encode** | `rans_byte_enc_put`, `rans_byte_enc_put_symbol`, `rans_byte_enc_flush` | Division + reciprocal encode |
| **Decode** | `rans_byte_dec_init`, `rans_byte_dec_get`, `rans_byte_dec_advance_symbol` | Division decode |
| **R64 encode** | `rans64_enc_put`, `rans64_enc_put_symbol`, `rans64_enc_flush` | 64-bit encode |
| **R64 decode** | `rans64_dec_init`, `rans64_dec_advance_symbol`, `rans64_dec_renorm` | 64-bit decode |
| **Word encode** | `rans_word_enc_init`, `rans_word_enc_put`, `rans_word_enc_flush` | Table-based word encode |
| **Word decode** | `rans_word_dec_init`, `rans_word_dec_sym`, `rans_word_dec_renorm` | Table-based word decode |
| **I/O** | `BackwardByteWriter`, `ByteReader`, `SliceBackwardWriter` | Buffer management |
| **R64 I/O** | `BackwardWord32Writer`, `Word32Reader` | Word-based I/O |
| **Word I/O** | `BackwardWord16Writer`, `Word16Reader` | 16-bit word I/O |
| **Interleaved** | `ByteInterleavedEncoder`, `ByteInterleavedDecoder` | Two-state interleaved |
| **Alias** | `AliasTable`, `rans_byte_alias_*` | Vose alias table + operations |
| **Constants** | `RANS_BYTE_L`, `RANS64_L`, `RANS_WORD_L`, `RANS_WORD_SCALE_BITS` | Normalization bounds |
| **Malformed** | `malformed::validate_byte_compressed`, `malformed::RenormGuard`, etc. | Stream validation |

### `malformed` Sub-Module

The `malformed` sub-module provides defensive stream validation. It is always available
because it lives in `ryg-rans-rs-core`:

```rust
use ryg_rans_rs::byte::malformed::{
    validate_byte_compressed,
    RenormGuard,
    validate_freq_model,
    has_dominant_symbol,
};
```

---

## SIMD Module

The `simd` module is enabled with `features = ["simd"]` in your `Cargo.toml`.

### Available APIs

#### 8-Way Interleaved (Existing Format)

| Function | Backend | Safety | Description |
|----------|---------|--------|-------------|
| `decode_interleaved8_auto` | Auto-select | ✅ Safe | Scalar (fastest on Zen 5) |
| `decode_interleaved8_scalar` | Scalar | ✅ Safe | Always scalar |
| `decode_interleaved8_avx512vl` | AVX512VL | ⚠️ Unsafe | Requires CPU support |

#### 16-Way Interleaved (New Format)

| Function | Backend | Safety | Description |
|----------|---------|--------|-------------|
| `decode_interleaved16_auto` | Auto-select | ✅ Safe | Scalar (fastest on Zen 5) |
| `decode_interleaved16_scalar` | Scalar | ✅ Safe | Always scalar |
| `decode_interleaved16_avx512` | AVX512 | ⚠️ Unsafe | Requires CPU support |
| `encode_interleaved16` | Scalar | ✅ Safe | 16-way encoder |

#### Packed Table

| Type | Description |
|------|-------------|
| `PackedWordTable` | 4096-slot u32 packed table (freq\|bias<<12\|sym<<24) |
| `PackedWordEntry` | Single entry with freq/bias/symbol extraction |
| `DecodeBackend` | Enum with stable labels: `scalar-8way`, `avx512vl-8way`, etc. |
| `DecodeResult` | Output + report + backend identity |

### ISA Requirements

| Backend | Required `target_feature` | First CPU Support |
|---------|--------------------------|-------------------|
| SSE4.1 8-way | `+ssse3,+sse4.1` | Intel Core 2 (2008), AMD Bulldozer (2011) |
| AVX512VL 8-way | `+avx512f,+avx512vl,+avx512bw` | Intel Ice Lake (2019), AMD Zen 4 (2022) |
| AVX512 16-way | `+avx512f,+avx512bw` | Intel Ice Lake (2019), AMD Zen 4 (2022) |

---

## Feature Matrix

| Feature | What It Enables | `no_std` Compatible | Typical Use Case |
|---------|----------------|-------------------|------------------|
| (default) | Core re-export only | ✅ Yes | Embedded, kernel, Wasm |
| `simd` | `ryg-rans-rs-simd` (SSE4.1 + AVX-512) | ✅ Yes (cfg dispatch) | Performance-sensitive decoding |
| `alloc` | `alloc_utils` + alias table | ✅ Yes (extern alloc) | Heap-allocated decode |

---

## Quick Start

### Basic Byte rANS Encode/Decode

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let freq = (1u32 << scale_bits) / 256;  // Uniform 256-symbol model
let mut buf = [0u8; 4096];               // Output buffer

// Encode a single symbol 'A' (byte value 65)
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
// dec_state now contains the original state (RANS_BYTE_L)
```

### AVX-512 Decode (with SIMD feature)

```rust
#[cfg(feature = "simd")]
{
    use ryg_rans_rs::simd::backends::decode_interleaved8_auto;
    use ryg_rans_rs::simd::packed_table::PackedWordTable;

    let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
    let result = decode_interleaved8_auto(&compressed, &packed, expected_len).unwrap();
    println!("Decoded {} bytes using {}", result.output.len(), result.backend.label());
    assert_eq!(result.output, original_input);
}
```

### Malformed Input Validation

```rust
use ryg_rans_rs::byte::malformed::{
    validate_byte_compressed,
    RenormGuard,
    ValidationError,
};

// Before decoding an untrusted stream:
if let Err(e) = validate_byte_compressed(untrusted_input) {
    return Err(e.into());
}

// During renormalization with untrusted input:
let mut guard = RenormGuard::new_byte();
loop {
    guard.check()?;  // Fails after 16 iterations on corrupted input
    let b = reader.read_byte().ok_or(DecodeError::InputTooShort)?;
    x = (x << 8) | (b as u32);
    if x >= RANS_BYTE_L { break; }
}
```

---

## Safety Guarantees

| Layer | What | Enforcement |
|-------|------|-------------|
| **Core crate** | All arithmetic | `#![forbid(unsafe_code)]` — compile-time |
| **Facade crate** | Re-exports only | `#![deny(unsafe_code)]` — compile-time |
| **SIMD crate** | Intrinsics | 7 `unsafe fn`, all `#[target_feature]`-gated |
| **SIMD dispatch** | Runtime detection | Safe `_auto` functions check CPU features |
| **No overread** | Input bounds | Every decoder checks length before reading |
| **No overflow** | Arithmetic bounds | Kani proofs for critical formulas |

---

## Published Versions

| Version | Phase | Key Changes |
|---------|-------|-------------|
| **0.1.27** | **J** | **Criterion all-tier benchmark suite, 8/16-thread scaling matrix, strict block parser, ultra-thorough documentation** |
| **0.1.26** | **J** | **AVX2 portability tier, Batch4, real SSE execution, backend truthfulness, Phase I CLI integration** |
| **0.1.25** | **I** | **Phase I parallel block engine: bounded executor, FixedBlockPlan, ReorderBuffer, CancellationToken, 63 tests. Published all 7 workspace crates.** |
| **0.1.15** | **G** | **AVX512VL 8-way + AVX512 16-way decode kernels** |
| 0.1.14 | H | Malformed-stream hardening, fuzzing, Kani proofs |
| 0.1.13 | F | SSE4.1 SIMD decoder, 128 receipts |
| 0.1.12 | F | SIMD implementation, cross-courts |
| 0.1.11 | E | Alias method seal, 120 receipts |
| 0.1.10 | E | Alias implementation |
| 0.1.9 | D | Word rANS seal, Docker stamp |
