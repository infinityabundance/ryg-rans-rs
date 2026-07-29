# ryg-rans-rs-core

> `#![no_std]` + `#![forbid(unsafe_code)]` — deterministic rANS algorithmic core.  
> 5 surfaces, 128 receipts, bit-exact C↔Rust parity.  
> Includes: malformed-stream hardening, Kani formal proofs, cargo-fuzz targets.

## Status

All surfaces are **sealed** (behaviour status: `full`) at scale_bits=12 with 8 profiles (Uniform256, Freq1Residual, Skewed2551, Sparse2, Sparse17, PrimeResidue, RenormBoundary, LengthBoundary) plus scale sweep for byte and r64.

| Surface | Mode | Receipts | Verified |
|---------|------|----------|----------|
| Byte rANS (division + reciprocal) | Single-state + Interleaved2 + ScaleSweep | 44 | C↔Rust cross-decode |
| 64-bit rANS (division + reciprocal) | Single-state + Interleaved2 + ScaleSweep | 44 | C↔Rust cross-decode |
| Word rANS (division) | Single-state + Interleaved2 | 16 | C↔Rust cross-decode |
| Alias method | Single-state + Interleaved2 | 16 | C↔Rust cross-decode |
| SSE4.1 SIMD decoder | Interleaved8 | 8 | C↔Rust cross-decode |

## Phase H: Safety Infrastructure

### Malformed-Stream Hardening (`malformed` module)

The `malformed` sub-module provides defensive checks for untrusted input:

- **Pre-decode validation**: `validate_byte_compressed()`, `validate_r64_compressed()`, `validate_word_compressed()` — check minimum stream length before decoder init.
- **Renormalization guards**: `RenormGuard` — bounds the number of consecutive renormalization iterations, preventing infinite loops on corrupted input.
- **Frequency model validation**: `validate_freq_model()` — checks monotonic cumulative frequencies, range bounds, and zero-frequency invariance.
- **Edge-case detection**: `has_dominant_symbol()`, `is_single_symbol()`, `has_freq_one()` — classify frequency model shapes for targeted testing.
- **Error conversion**: `validation_to_decode_error()` bridges `ValidationError` to `DecodeError` for callers that don't want separate validation error handling.

### Fuzzing (cargo-fuzz)

Five fuzz targets in `fuzz/`:

| Target | Input | Exercises |
|--------|-------|-----------|
| `byte_rans_roundtrip` | Arbitrary bytes | Division + reciprocal encode, decode, cross-verification |
| `r64_rans_roundtrip` | Arbitrary bytes | 64-bit rANS encode/decode, division vs reciprocal stream match |
| `word_rans_roundtrip` | Arbitrary bytes | Word rANS table construction, single-state encode/decode |
| `malformed_byte` | Random bytes | Truncated/corrupted stream → decoder must never panic |
| `alias_roundtrip` | Arbitrary bytes | Frequency normalization, alias table construction, encode/decode |

### Kani Formal Proofs

Four Kani proof harnesses in `kani/` verify critical arithmetic under bounded model checking:

| Proof | Property | Scope |
|-------|----------|-------|
| `kani_enc_symbol_new_valid` | Valid parameters → `Ok`, invalid → correct `ModelError` | All `scale_bits 1..=16`, any `start`, `freq` |
| `kani_reciprocal_equals_division` | Reciprocal fast path = division reference | All parameters where no renorm needed |
| `kani_r64_reciprocal_equals_division` | R64 reciprocal = division | All `scale_bits 1..=31`, no-renorm region |
| `kani_byte_encode_decode_inversion` | `decode(encode(x)) = x` | Core formula for all valid parameters |

Run proofs with:
```sh
# Requires Kani installed (cargo install kani-verifier)
kani crates/ryg-rans-rs-core/kani/reciprocal_proof.rs
kani crates/ryg-rans-rs-core/kani/encode_decode_inversion_proof.rs
```

## Contents

### Byte rANS (32-bit, `rans_byte.h`)

- `RansByteState` — 31-bit effective encoder/decoder state
- `RansByteEncSymbol` — Precomputed reciprocal encoder symbol (x_max, rcp_freq, bias, cmpl_freq, rcp_shift)
- `RansByteDecSymbol` — Decoder symbol (start, freq)
- `rans_byte_enc_renorm` / `rans_byte_enc_put` — Division-based encode
- `rans_byte_enc_put_symbol` — Reciprocal fast-path encode (Alverson's method)
- `rans_byte_enc_flush` — Flush remaining state to output
- `rans_byte_dec_init` / `rans_byte_dec_get` / `rans_byte_dec_advance` — Division-based decode
- `rans_byte_dec_advance_symbol` / `rans_byte_dec_advance_step` — Symbol decode + step-only operations
- `ByteInterleavedEncoder` / `ByteInterleavedDecoder` — Two-state interleaved mode

### 64-bit rANS (`rans64.h`)

- `Rans64State` — 63-bit effective state with 32-bit word renormalization
- `Rans64EncSymbol` / `Rans64DecSymbol` — Encoder/decoder symbols for 64-bit variant
- `rans64_mul_hi` — 128-bit multiply-high for reciprocal
- `rans64_enc_renorm` / `rans64_enc_put` / `rans64_enc_put_symbol` — Encode operations
- `rans64_dec_init` / `rans64_dec_advance` / `rans64_dec_advance_symbol` — Decode operations
- `BackwardWord32Writer` / `Word32Reader` — 32-bit word I/O

### Word rANS (`rans_word_sse41.h` scalar path)

- `RansWordState` — 16-bit word renormalization (L=2^16)
- `RansWordSlot` — Table entry (freq, bias)
- `RansWordTables` — 4096-slot decode table
- `rans_word_enc_init` / `rans_word_enc_put` / `rans_word_enc_flush` — Word-based encode
- `rans_word_dec_init` / `rans_word_dec_sym` / `rans_word_dec_renorm` — Word-based decode
- Hardcoded `RANS_WORD_SCALE_BITS = 12` per upstream design

### Alias Method (`main_alias.cpp`)

- `AliasTable` — Vose's alias table (256 buckets, each with divider + 2 slots)
- `rans_byte_alias_normalize_freqs` — Frequency normalization to power-of-2 total
- `rans_byte_alias_build_table` — Vose's algorithm alias table construction
- `rans_byte_alias_enc_put` — Division-based encode with alias remap
- `rans_byte_alias_dec_get` / `rans_byte_alias_dec_advance` — O(1) alias decode
- Requires `alloc` feature (for the alias_remap table)

### Writer/Reader Abstractions

- `BackwardByteWriter` — Reverse-growing byte buffer for encoding output
- `ByteReader` — Forward byte buffer for decoding input
- `BackwardWord32Writer` / `Word32Reader` — 32-bit word variants
- `BackwardWord16Writer` / `Word16Reader` — 16-bit word variants (word rANS)
- `SliceBackwardWriter` — Convenience mutable-slice wrapper
- Trait-based: `BackwardWriter` / `ForwardReader` — zero-cost abstraction

## Design

- **Zero `unsafe`** — The `forbid(unsafe_code)` attribute is a compile-time guarantee.
- **Zero `std`** — Works in embedded, kernel, and Wasm environments.
- **Caller-provided storage** — No hidden allocation in encode/decode hot paths.
- **`alloc` feature** — Optional for alias table construction and test infrastructure.

## Feature Flags

- `default = []` — Core only, no std dependency
- `alloc` — Enables `AliasTable` construction and `Vec`-based APIs
- `std` — Enables `std::error::Error` impls for error types
