# ryg-rans-rs-core

> `#![no_std]` + `#![forbid(unsafe_code)]` — deterministic rANS algorithmic core.  
> 7 algorithmic surfaces, 144 behavioral receipts, bit-exact C↔Rust parity.  
> Malformed-stream hardening, Kani formal proofs, packed table reference implementation.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-core)](https://crates.io/crates/ryg-rans-rs-core)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs-core)](https://docs.rs/ryg-rans-rs-core/latest/ryg_rans_rs_core/)

---

## Why This Crate Exists

This is the **mathematical heart** of ryg-rans-rs. It implements the exact rANS encoding and
decoding formulas from Fabian Giesen's `ryg_rans` repository as pure safe Rust. Every
arithmetic operation in this crate is:

1. **Bit-exact** — produces identical results to the upstream C on every input (verified
   by cross-decoding courts)
2. **Unsafe-free** — guaranteed by `#![forbid(unsafe_code)]`
3. **No_std** — works in embedded, kernel, and Wasm environments without an allocator
4. **Tested** — 57+ unit tests covering edge cases, round-trips, and malformed inputs
5. **Proven** — Kani model-checking proofs for critical arithmetic properties

The crate provides the **seven** algorithmic surfaces that higher-level crates build on:

| Surface | Upstream File | Key Formulas | Verification |
|---------|---------------|--------------|--------------|
| Byte rANS division | `rans_byte.h` | `C(s,x) = ((x/freq) << M) + (x%freq) + start` | C↔Rust court |
| Byte rANS reciprocal | `rans_byte.h` | Multiply-high reciprocal approximation | C↔Rust court + Kani |
| 64-bit rANS division | `rans64.h` | 63-bit state, 32-bit word renorm | C↔Rust court |
| 64-bit rANS reciprocal | `rans64.h` | 128-bit mul_hi for reciprocal | C↔Rust court + Kani |
| Word rANS | `rans_word_sse41.h` | 16-bit word renorm, table-based decode | C↔Rust court |
| Alias method | `main_alias.cpp` | Vose alias table, O(1) decode | C↔Rust court |
| **16-way scalar** | (new format) | 16-state interleaved, reverse-flush | Cross-verified with C oracle |

---

## rANS Arithmetic Explained

### The ANS Encoding Function

The core ANS encoding transition `C(s, x)` maps a current state `x` and symbol `s` to a new state:

```
C(s, x) = floor(x / freq_s) × M + (x % freq_s) + start_s
```

Where:
- `M = 1 << scale_bits` is the total frequency range (e.g., 4096 for scale_bits=12)
- `freq_s` is the frequency of symbol `s`
- `start_s` is the cumulative frequency of all symbols before `s`
- The result replaces the "slot" portion of the state with the symbol's encoding

### The ANS Decoding Function

Decoding is the inverse:

```
slot = x & (M - 1)         // extract the "slot" — identifies the symbol
symbol = table[slot]        // look up which symbol owns this slot
x' = freq_s × (x >> M) + (slot - start_s)  // recover the previous state
```

After decoding, the state may be below the normalization bound `L`, requiring
renormalization: read one or more bits/bytes/words from the stream and shift them
into the state until `x' >= L`.

### Renormalization

Renormalization keeps the state in a fixed range `[L, (L × M) - 1]`:

- **Encoding**: if `state >= x_max` (where `x_max = ((L >> M) << 8) × freq`),
  emit the low 8/16/32 bits and shift right
- **Decoding**: if `state < L`, read more bits/bytes/words and shift left

The exact unit of renormalization defines the three variants:
- **Byte rANS (32-bit)**: 8-bit byte renormalization, `L = 2^23`
- **R64 (64-bit)**: 32-bit word renormalization, `L = 2^31`
- **Word rANS (32-bit)**: 16-bit word renormalization, `L = 2^16`

### Reciprocal Multiplication (Fast Path)

Integer division is expensive (~10-30 cycles). The reciprocal fast path replaces
`x / freq` with:

```
q = mul_hi(x, rcp_freq) >> rcp_shift
new_state = x + bias + q × cmpl_freq
```

Where `rcp_freq = ((1 << (shift + 31)) + freq - 1) / freq` approximates `1/freq`
as a fixed-point number. This is Alverson's "Integer Division using Reciprocals".

**Kani proof**: `kani_reciprocal_equals_division` proves this approximation is exact
for every valid input where no renormalization is needed.

---

## Architecture

### Module Organization

```text
src/
  lib.rs              — Types, encode/decode functions, I/O traits, tests (57+)
  malformed.rs        — Stream validation, renormalization guards, freq model checks
  kani/               — Kani proof harnesses (7 total)
```

### I/O Abstraction

The encode/decode functions operate on generic I/O traits:

```rust
pub trait BackwardWriter {
    fn write_byte(&mut self, b: u8) -> Result<(), ()>;     // single byte
    fn write_u32_le(&mut self, v: u32) -> Result<(), ()>;  // 4 bytes LE
}

pub trait ForwardReader {
    fn read_byte(&mut self) -> Option<u8>;     // single byte
    fn read_u32_le(&mut self) -> Option<u32>;  // 4 bytes LE
}
```

Concrete implementations:
- `BackwardByteWriter` / `ByteReader` — for byte rANS
- `BackwardWord32Writer` / `Word32Reader` — for 64-bit rANS
- `BackwardWord16Writer` / `Word16Reader` — for word rANS
- `SliceBackwardWriter` — convenient `&mut [u8]` wrapper

### Interleaved Decoding

Two-state interleaving doubles decode throughput by maintaining two independent states
and alternating between them:

```text
Step 0: decode state0 → renorm state0
Step 1: decode state1 → renorm state1
Step 2: output pair (symbol0, symbol1)
```

The `ByteInterleavedEncoder` and `ByteInterleavedDecoder` implement this pattern.
The 8-way and 16-way decoders extend this to 8 and 16 states respectively.

---

## The Malformed-Stream Module (`malformed.rs`)

This module provides defensive checks for untrusted input — critical for any
production system that accepts compressed data from external sources.

### Pre-decode Validation

```rust
pub fn validate_byte_compressed(compressed: &[u8]) -> Result<(), ValidationError>
pub fn validate_r64_compressed(compressed: &[u8]) -> Result<(), ValidationError>
pub fn validate_word_compressed(compressed: &[u16]) -> Result<(), ValidationError>
```

Each function checks that the compressed stream has enough data for decoder
initialization (4 bytes for byte rANS, 8 bytes for R64, 2 words for word rANS).

### Renormalization Guards

```rust
pub struct RenormGuard { remaining: u32 }
```

The `RenormGuard` bounds the number of consecutive renormalization iterations,
preventing infinite loops on corrupted input:

```rust
let mut guard = RenormGuard::new_byte();
loop {
    guard.check()?;  // returns Err after 16 iterations
    let b = reader.read_byte().ok_or(DecodeError::InputTooShort)?;
    x = (x << 8) | (b as u32);
    if x >= RANS_BYTE_L { break; }
}
```

### Frequency Model Validation

```rust
pub fn validate_freq_model(cum_freqs: &[u32], freqs: &[u32], scale_bits: u32)
    -> Result<(), ValidationError>
```

Checks:
- Cumulative frequencies are monotonically non-decreasing
- No frequency's range exceeds the total
- Scale bits is within valid range

---

## Kani Formal Proofs

Seven proof harnesses verify critical arithmetic properties under bounded model checking:

| Proof | Property | Scope |
|-------|----------|-------|
| `kani_enc_symbol_new_valid` | Valid params → Ok, invalid → correct Err | All scale_bits 1..=16 |
| `kani_reciprocal_equals_division` | Byte reciprocal = division | No-renorm region |
| `kani_r64_reciprocal_equals_division` | R64 reciprocal = division | Scale_bits 1..=31 |
| `kani_byte_encode_decode_inversion` | `decode(encode(x)) = x` | Core formula, all valid params |
| `kani_packed_entry_fields` | Pack/unpack round-trip | All valid freq/bias/symbol |
| `kani_state_update_no_overflow` | State update arithmetic bounds | All valid state values |
| `kani_slot_index_bounded` | Slot index in 0..4096 | Any u32 state |

To run: `kani crates/ryg-rans-rs-core/kani/<proof>.rs`

---

## Error Types

```rust
pub enum EncodeError { OutputTooSmall }
pub enum DecodeError { InputTooShort }
pub enum ModelError {
    EmptyInput, ZeroTotal, InvalidScaleBits, ZeroFrequency,
    FrequencyOutOfRange, StartOutOfRange, TotalMismatch, WorkspaceTooSmall,
}
pub enum ValidationError {
    TruncatedStream, ExcessiveRenormalization, ZeroFrequency,
    CumulativeOverflow, InvalidScaleBits, RangeOverflow, TrailingData,
}
```

All error types implement `Display`, and with `std` feature enabled, they also
implement `std::error::Error`.

---

## Feature Flags

```toml
[features]
default = []
alloc = []   # Enables AliasTable construction + Vec-based APIs
std = []     # Enables std::error::Error impls
```

---

## Tests

Run with: `cargo test -p ryg-rans-rs-core`

57+ tests cover:
- Symbol construction edge cases (freq=1, max freq, invalid params)
- Single-symbol and multi-symbol round-trips
- Reciprocal = division equivalence
- Interleaved round-trips
- Writer/reader exhaustion
- 64-bit specific: mul_hi, large scale_bits, step operations
- Malformed input: truncation, renormalization bounds, freq model validation
- Slice-based I/O
