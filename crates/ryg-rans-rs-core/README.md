# ryg-rans-rs-core

> **The mathematical heart of rANS entropy coding — pure safe Rust.**  
> `#![no_std]` + `#![forbid(unsafe_code)]` — works in embedded, kernel, and Wasm.  
> 7 algorithmic surfaces · 144 behavioral receipts · Kani-proven arithmetic · Malformed-stream hardened.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-core)](https://crates.io/crates/ryg-rans-rs-core)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs-core)](https://docs.rs/ryg-rans-rs-core/latest/ryg_rans_rs_core/)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [Understanding rANS Arithmetic](#understanding-rans-arithmetic)
3. [The Seven Algorithmic Surfaces](#the-seven-algorithmic-surfaces)
4. [Module Architecture](#module-architecture)
5. [I/O Abstraction Design](#io-abstraction-design)
6. [Interleaved Decoding Pattern](#interleaved-decoding-pattern)
7. [Malformed-Stream Hardening](#malformed-stream-hardening)
8. [Kani Formal Proofs](#kani-formal-proofs)
9. [Error Types](#error-types)
10. [Feature Flags](#feature-flags)
11. [Testing Strategy](#testing-strategy)
12. [Performance Characteristics](#performance-characteristics)

---

## What This Crate Is

This crate is a **from-scratch, native Rust reconstruction** of the Asymmetric Numeral
Systems (ANS) entropy coder variants published in Fabian Giesen's seminal
[ryg_rans](https://github.com/rygorous/ryg_rans) repository. It is **not** a wrapper,
binding, or FFI facade — every arithmetic operation is independently implemented and
verified against the compiled C/C++ reference.

### Design Constraints

| Constraint | Why It Matters |
|------------|----------------|
| `#![no_std]` | Works in embedded, kernel, and Wasm environments where the standard library is unavailable |
| `#![forbid(unsafe_code)]` | The algorithmic core must be provably safe — no UB can arise from the encode/decode logic itself |
| Zero allocation in hot paths | Encoder/decoder functions use caller-provided storage; no hidden `Vec` or `Box` allocations |
| `alloc` feature gated | Alias table construction requires heap allocation, but this is gated behind a feature flag |
| `std` feature gated | `std::error::Error` impls are only enabled when the standard library is available |

### What This Crate Provides

Every function in this crate implements an exact mathematical formula from the upstream
C headers. The crate is organized around **algorithmic surfaces** — each surface represents
a distinct combination of state size, renormalization unit, and encoding strategy:

| Surface | Upstream File | State Bits | Renorm Unit | Scale Bits | Encoding |
|---------|---------------|------------|-------------|------------|----------|
| Byte rANS (division) | `rans_byte.h` | 32-bit (31 effective) | 8-bit byte | 1..=16 | Division |
| Byte rANS (reciprocal) | `rans_byte.h` | 32-bit (31 effective) | 8-bit byte | 1..=16 | Multiply-high |
| R64 (division) | `rans64.h` | 64-bit (63 effective) | 32-bit word | 1..=31 | Division |
| R64 (reciprocal) | `rans64.h` | 64-bit (63 effective) | 32-bit word | 1..=31 | `mul_hi` |
| Word rANS | `rans_word_sse41.h` | 32-bit | 16-bit word | 12 (fixed) | Division |
| Alias method | `main_alias.cpp` | 32-bit | 8-bit byte | 8..=17 | Division + alias |
| 16-way scalar | (new format) | 32-bit | 16-bit word | 12 (fixed) | Division |

---

## Understanding rANS Arithmetic

### The Core Insight: Compressing States Instead of Bits

Traditional entropy coders (Huffman, arithmetic coding) encode symbols as sequences of bits.
rANS (range Asymmetric Numeral Systems) takes a different approach: it encodes symbols as
**state transitions**. A single integer `x` represents the entire encoding state, and each
symbol updates this state via a carefully designed mathematical function.

The key property is that the state `x` grows logarithmically with the number of encoded
symbols — specifically, a symbol with probability `p` ≈ `freq / total` adds approximately
`-log2(p)` bits to the state. This makes rANS a **near-optimal entropy coder** with
compression efficiency approaching the Shannon limit, but with the computational simplicity
of integer arithmetic.

### Encoding: C(s, x)

The encoding transition maps a current state `x` and symbol `s` to a new state:

```
C(s, x) = floor(x / freq_s) × M + (x % freq_s) + start_s
```

**Why this formula?** The state space `[0, ∞)` is partitioned into `M` equal-sized "slots,"
where `M = 1 << scale_bits` (typically 4096 for scale_bits=12). Each symbol `s` owns
`freq_s` consecutive slots, starting at position `start_s` (the cumulative frequency of
all symbols before `s`). The numerator `x` represents the current position in the state
space. The division `x / freq_s` gives the "row," the remainder `x % freq_s` gives the
position within the symbol's block, and `start_s` offsets to the correct symbol's region.

### Decoding: The Inverse

Decoding recovers the previous state and the encoded symbol:

```
slot = x & (M - 1)          // Extract the slot — which part of the state space?
symbol = table[slot]         // Which symbol owns this slot?
x' = freq_s × (x >> M) + (slot - start_s)  // Reverse the encoding
```

**Why this works**: The encoding formula maps a pair `(x, s)` to a new state `C(s, x)`.
The decoding formula maps `C(s, x)` back to `(x, s)`. Substitution shows they are
mathematical inverses:

```
decode(C(s, x)):
  slot = C(s, x) & (M - 1) = (x % freq_s) + start_s
  symbol = table[slot] = s
  x' = freq_s × (C(s, x) >> M) + (slot - start_s)
     = freq_s × (x / freq_s) + (x % freq_s)
     = x
```

### Renormalization

The state `x` must stay within a valid range to prevent overflow and to ensure that
encoding/decoding operations have enough "headroom." Renormalization is the process of
transferring bits between the state and the compressed stream.

**During encoding**: If `state >= x_max` (where `x_max = ((L >> scale_bits) << 8) × freq`),
the state has grown too large. We emit the low 8/16/32 bits and shift right, reducing the
state. This is like "flushing" the low-order bits to the output stream.

**During decoding**: If `state < L`, the state has shrunk too small. We read bits from the
compressed stream and shift left, increasing the state. This is like "refilling" the state
from the input stream.

The exact unit of renormalization defines the three main variants:
- **Byte rANS (32-bit)**: `L = 2^23`, renormalization unit = 1 byte. Emit/ingest 8 bits at a time.
- **R64 (64-bit)**: `L = 2^31`, renormalization unit = 1 u32 word. Emit/ingest 32 bits at a time.
- **Word rANS (32-bit)**: `L = 2^16`, renormalization unit = 1 u16 word. Emit/ingest 16 bits at a time.

### Reciprocal Multiplication: Avoiding Division

Integer division (`idiv`) is expensive — typically 10-30 cycles on modern x86_64. The
reciprocal fast path replaces `x / freq` with a multiplication and shift:

```
q = mul_hi(x, rcp_freq) >> rcp_shift
new_state = x + bias + q × cmpl_freq
```

This is Alverson's "Integer Division using Reciprocals" technique:
- `rcp_freq` is a fixed-point approximation of `1/freq`, scaled by `2^(shift + 31)`
- `mul_hi(x, rcp_freq)` computes the high 32 bits of the 64-bit product `x × rcp_freq`,
  which gives `(x × approx(1/freq))` in the high bits
- Shifting by `rcp_shift` adjusts the decimal point to get the quotient

**Kani proof**: `kani_reciprocal_equals_division` proves that for every input where no
renormalization is needed, the reciprocal path produces the exact same state as the
division path. This is a formal verification that the approximation is exact for all
valid inputs, not just a statistical claim.

---

## The Seven Algorithmic Surfaces

### 1. Byte rANS (Division Path)

The reference implementation — uses actual integer division:

```rust
pub fn rans_byte_enc_put(state, writer, start, freq, scale_bits) -> Result<(), EncodeError>
```

This function first renormalizes (if `state >= x_max`, emit bytes), then computes the
new state using division: `((x / freq) << scale_bits) + (x % freq) + start`.

### 2. Byte rANS (Reciprocal Path)

The fast path — uses precomputed reciprocal to avoid division:

```rust
pub fn rans_byte_enc_put_symbol(state, writer, sym) -> Result<(), EncodeError>
```

The `RansByteEncSymbol` is precomputed from `(start, freq, scale_bits)`:
- `x_max`: threshold for renormalization
- `rcp_freq`: fixed-point reciprocal of frequency
- `rcp_shift`: shift for decimal point adjustment
- `bias`: start value (or `start + M - 1` for freq=1)
- `cmpl_freq`: complement of frequency = `M - freq`

### 3. 64-bit rANS (Division Path)

Extends byte rANS to 64-bit state with 32-bit word renormalization:

```rust
pub fn rans64_enc_put(state, writer, start, freq, scale_bits) -> Result<(), EncodeError>
```

The state is `u64`, and renormalization emits/consumes `u32` words instead of bytes.
The scale_bits range extends to 31 (since we have more state space).

### 4. 64-bit rANS (Reciprocal Path)

Uses 128-bit multiply-high for the reciprocal:

```rust
pub fn rans64_mul_hi(a: u64, b: u64) -> u64
```

This computes `(a × b) >> 64` using Rust's `u128` type, which the compiler lowers to
a single `mul` instruction (or `mulx` on modern x86_64).

### 5. Word rANS

Fixed scale_bits = 12, 16-bit word renormalization, table-based decode:

```rust
pub fn rans_word_dec_sym(state, tables, scale_bits) -> u8
pub fn rans_word_dec_renorm(state, reader) -> Result<(), DecodeError>
```

Uses the `RansWordSlot` struct (frequency + bias) and `RansWordTables` (slots + slot→symbol mapping).

### 6. Alias Method

Vose's alias table for O(1) symbol lookup — no binary search needed:

```rust
pub fn rans_byte_alias_dec_advance(state, reader, table, scale_bits) -> Result<u8, DecodeError>
```

The alias table converts the frequency distribution into 256 equal-sized buckets, each
containing at most 2 symbols. Decoding is constant-time: `bucket = state & 0xFF`,
check if `state < divider[bucket]`, pick the appropriate symbol.

### 7. 16-Way Scalar

The 16-way interleaved Word rANS format (used by AVX512.INTERLEAVED16):

```rust
pub fn encode_interleaved16(symbols, freqs, cum, scale_bits) -> Vec<u16>
pub fn decode_interleaved16_scalar(compressed, table, expected_len) -> Result<(Vec<u8>, DecodeReport), &str>
```

This is defined in `ryg-rans-rs-simd` but uses the core's constants and data types.

---

## Module Architecture

```
src/
  lib.rs              — Types, encode/decode functions, I/O traits, 57+ unit tests
  malformed.rs        — Stream validation, renormalization guards, frequency model checks
  kani/               — Kani proof harnesses (7 total)
```

### `lib.rs` — Types and Core Functions

The file is organized into clearly labeled sections:

1. **Error types** (`EncodeError`, `DecodeError`, `ModelError`) — exact error variants
2. **Constants** (`RANS_BYTE_L`, `RANS64_L`, `RANS_WORD_L`) — upstream normalization bounds
3. **State types** (`RansByteState`, `Rans64State`, `RansWordState`) — newtype wrappers
4. **I/O types** (`BackwardByteWriter`, `ByteReader`, etc.) — buffer management
5. **Encoder/decoder symbols** — precomputed parameters for fast encoding
6. **Encode functions** — renormalization, put, flush for each variant
7. **Decode functions** — init, get, advance, renorm for each variant
8. **Interleaved types** — `ByteInterleavedEncoder`/`Decoder` for two-state interleaving
9. **64-bit rANS** — all the R64 functions (wider state, word renormalization)
10. **Word rANS** — table-based encode/decode with 16-bit renormalization
11. **Alias method** — Vose's alias table construction and operations
12. **Tests** — 57+ unit tests covering every function

### `malformed.rs` — Defensive Validation

This module provides safety checks for untrusted compressed streams:

- **Pre-decode validation**: Ensure minimum stream length before attempting to decode
- **Renormalization guards**: Prevent infinite loops on corrupted input by bounding iterations
- **Frequency model validation**: Check cumulative frequencies are monotonically non-decreasing
- **Edge-case detection**: Classify models by their statistical properties

### `kani/` — Formal Proofs

Seven Kani harnesses that use bounded model checking to verify arithmetic properties:

| Harness | What It Proves | Why It Matters |
|---------|---------------|----------------|
| `kani_enc_symbol_new_valid` | Valid parameters → `Ok`, invalid → correct error variant | Encoder symbol construction is sound |
| `kani_reciprocal_equals_division` | Reciprocal path = division path for all valid inputs | Fast path is correct, not just fast |
| `kani_r64_reciprocal_equals_division` | Same for 64-bit variant | Widens the proof to higher scale_bits |
| `kani_byte_encode_decode_inversion` | `decode(encode(x)) = x` | The fundamental ANS identity holds |
| `kani_packed_entry_fields` | Pack/unpack round-trips exactly | Packed table has no ambiguity |
| `kani_state_update_no_overflow` | State update arithmetic stays bounded | No silent overflow in hot path |
| `kani_slot_index_bounded` | Slot index always < 4096 | Table access is always in bounds |

---

## I/O Abstraction Design

The encode/decode functions use **trait-based I/O** for maximum flexibility:

```rust
pub trait BackwardWriter {
    fn write_byte(&mut self, b: u8) -> Result<(), ()>;     // Write one byte
    fn write_u32_le(&mut self, v: u32) -> Result<(), ()>;  // Write 4 bytes LE
}

pub trait ForwardReader {
    fn read_byte(&mut self) -> Option<u8>;     // Read one byte
    fn read_u32_le(&mut self) -> Option<u32>;  // Read 4 bytes LE
}
```

This trait-based approach means the same encode/decode functions work with any storage
backend — byte slices, word buffers, file streams, network buffers, or custom allocators.

### Why Traits Instead of Concrete Types?

Zero-cost abstraction: the compiler monomorphizes the encode/decode functions for each
concrete writer/reader type, eliminating virtual dispatch overhead. A `BackwardByteWriter`
wrapping a `&mut [u8]` compiles to the same machine code as direct slice manipulation.

### Concrete Implementations

| Type | Width | Direction | Use Case |
|------|-------|-----------|----------|
| `BackwardByteWriter` | 8-bit | Encoding | Byte rANS output |
| `ByteReader` | 8-bit | Decoding | Byte rANS input |
| `BackwardWord32Writer` | 32-bit | Encoding | R64 output |
| `Word32Reader` | 32-bit | Decoding | R64 input |
| `BackwardWord16Writer` | 16-bit | Encoding | Word rANS output |
| `Word16Reader` | 16-bit | Decoding | Word rANS input |
| `SliceBackwardWriter` | 8-bit | Encoding | Convenient `&mut [u8]` wrapper |

### Why Backward Writers?

rANS encoding proceeds in **reverse order** — the last symbol is encoded first. The output
buffer grows backward from the end toward the beginning. This means the encoded stream is
produced in the correct forward-reading order: the first byte written (from the last symbol
encoded) ends up at the end of the buffer, and the last byte written (from the first symbol
encoded) ends up at the beginning.

The backward writer pattern is:
1. Start with `pos = buf.len()` (end of buffer)
2. To write a byte: `pos -= 1; buf[pos] = byte`
3. The encoded region is `buf[pos..]`

This avoids the need to know the encoded size in advance and allows in-place encoding
when the buffer is sized appropriately.

---

## Interleaved Decoding Pattern

### Two-State Interleaving

The two-state interleaving pattern maintains two independent rANS states and alternates
between them during both encoding and decoding:

**Encoding** (reverse order):
```
For symbols in reverse order:
  Encode symbol into state0
  Encode next symbol into state1
Flush: state1 first, then state0
```

**Decoding** (forward order):
```
For each pair of symbols:
  Decode from state0 → symbol0
  Decode from state1 → symbol1
  Renorm state0
  Renorm state1
  Output (symbol0, symbol1)
```

This doubles decode throughput by allowing the two renormalizations to overlap in the
CPU pipeline. The `ByteInterleavedEncoder` and `ByteInterleavedDecoder` structs implement
this pattern for byte rANS.

### 8-Way and 16-Way Interleaving

The 8-way format extends this to 8 states, decoded by two 4-lane SIMD units (SSE4.1)
or one 8-lane SIMD unit (AVX512VL). The 16-way format extends to 16 states decoded by
one 16-lane SIMD unit (AVX512). The mathematical pattern is the same — only the lane
count changes.

---

## Malformed-Stream Hardening

This module is critical for any production system that accepts compressed data from
untrusted sources. It provides:

### Pre-decode Validation

```rust
pub fn validate_byte_compressed(compressed: &[u8]) -> Result<(), ValidationError>
pub fn validate_r64_compressed(compressed: &[u8]) -> Result<(), ValidationError>
pub fn validate_word_compressed(compressed: &[u16]) -> Result<(), ValidationError>
```

Each function checks that the compressed stream has at least enough bytes/words for
decoder initialization (4 bytes for byte rANS, 8 bytes for R64, 2 words for word rANS).
This catches trivially truncated streams before any decoder state is touched.

### Renormalization Guards

```rust
pub struct RenormGuard { remaining: u32 }
```

The guard bounds the number of consecutive renormalization iterations. A corrupted stream
could otherwise cause the renormalization loop to spin indefinitely:

- Byte rANS: max 16 iterations (31-bit state / 8 bits per byte, with safety margin)
- R64: max 8 iterations (63-bit state / 32 bits per word, with safety margin)
- Word rANS: max 8 iterations

### Frequency Model Validation

```rust
pub fn validate_freq_model(cum_freqs, freqs, scale_bits) -> Result<(), ValidationError>
```

Checks:
1. Cumulative frequencies are monotonically non-decreasing
2. No symbol's frequency exceeds the allowed range
3. Total frequency matches `1 << scale_bits`
4. Scale bits is within valid range for the variant

### Edge-Case Detection

```rust
pub fn has_dominant_symbol(freqs, total) -> bool    // Any symbol > 50% of total?
pub fn is_single_symbol(freqs) -> bool               // Only one active symbol?
pub fn has_freq_one(freqs) -> bool                   // Any symbol with freq = 1?
```

These classifiers enable the CLI and higher-level code to make intelligent block-type
decisions (e.g., single-symbol blocks can use RLE instead of rANS).

---

## Kani Formal Proofs

The Kani proof harnesses use **bounded model checking** to verify that critical arithmetic
properties hold for **all** valid inputs — not just the test cases we thought of.

### How Kani Works

Kani symbolically explores all possible execution paths within specified bounds. For each
harness, it:
1. Treats function inputs as symbolic variables (any value in their type range)
2. Adds `kani::assume()` constraints to restrict to valid inputs
3. Explores all execution paths
4. Checks that `assert!()` and other properties hold for every path

### Proof: Reciprocal = Division

The most important proof verifies that the reciprocal fast path produces the same result
as division for every valid input:

```rust
fn div_put(x: u32, start: u32, freq: u32, scale_bits: u32) -> u32 {
    ((x / freq) << scale_bits) + (x % freq) + start
}

// Kani proves: for all valid (x, start, freq, scale_bits) where no renorm is needed,
// reciprocal_result == div_put(x, start, freq, scale_bits)
```

This proof is significant because the reciprocal approximation uses finite-precision
arithmetic (`u32` for `rcp_freq`, `u32` for the multiply-high result). The proof shows
that despite this finite precision, the result is **exact** for all valid inputs.

---

## Error Types

```rust
pub enum EncodeError { OutputTooSmall }         // Buffer too small for encoded data
pub enum DecodeError { InputTooShort }           // Truncated compressed stream

pub enum ModelError {
    EmptyInput,              // No data to model
    ZeroTotal,               // All frequencies are zero
    InvalidScaleBits,        // scale_bits outside valid range
    ZeroFrequency,           // A symbol has freq = 0 (not allowed in encoder)
    FrequencyOutOfRange,     // freq exceeds allowed range for scale_bits/start
    StartOutOfRange,         // start exceeds (1 << scale_bits)
    TotalMismatch,           // Accumulated total doesn't match declared total
    WorkspaceTooSmall,       // Buffer too small for table construction
}

pub enum ValidationError {
    TruncatedStream,             // Not enough data for init
    ExcessiveRenormalization,    // Too many consecutive renorm iterations
    ZeroFrequency,               // Model has zero-frequency symbol
    CumulativeOverflow,          // Cumulative freqs not monotonically increasing
    InvalidScaleBits,            // scale_bits out of range
    RangeOverflow,               // start + freq > target total
    TrailingData,                // Extra data after complete decode
}
```

All error types implement `Display`. With the `std` feature, they also implement
`std::error::Error`.

---

## Feature Flags

```toml
[features]
default = []    # No features by default — pure no_std, no alloc
alloc = []      # Enables AliasTable construction (requires alloc::vec::Vec)
std = []        # Enables std::error::Error impls for error types
```

- **default**: The crate is usable in any `no_std` environment. Encode/decode functions
  work with caller-provided buffers. No heap allocation needed.
- **alloc**: Enables the alias method table construction which requires `Vec`. Also enables
  `Vec`-based test infrastructure.
- **std**: Implements `std::error::Error` for the error types, making them compatible with
  `anyhow`, `eyre`, and other error-handling frameworks.

---

## Testing Strategy

Run tests: `cargo test -p ryg-rans-rs-core`

### 57+ Unit Tests Cover:

| Category | Tests | What They Verify |
|----------|-------|------------------|
| State initialization | 2 | States initialized to correct `L` value |
| Writer/reader basics | 6 | Byte/word ordering, position tracking, exhaustion |
| Encoder symbol construction | 5 | freq=1 case, max freq, scale bits bounds, reciprocal params |
| Decoder symbol construction | 2 | start/freq bounds |
| Single-symbol round-trip | 2 | Division and reciprocal paths |
| Multi-symbol round-trip | 3 | Two symbols, slice traits, uniform model |
| Reciprocal = division | 3 | Byte, freq=1, R64 — mathematical equivalence |
| Interleaved round-trip | 2 | Two-state interleaving |
| 64-bit specific | 12 | mul_hi, large scale_bits, step operations, state transitions |
| Word rANS | 4 | Init, sym, renorm, round-trip |
| Alias method | 4 | Normalization, table build, encode, decode |
| Malformed input | 12 | Truncation detection, scale validation, freq model checks |
| Renormalization guards | 2 | Byte and R64 iteration limits |
| Edge-case detection | 3 | Dominant symbol, single symbol, freq=1 |
| Slice-based I/O | 3 | Forward/backward trait implementations |

### Test Philosophy

Every test uses deterministic inputs — no randomness that could make tests flaky.
Round-trip tests verify that `decode(encode(input)) == input` for a range of inputs.
Equivalence tests verify that different algorithmic paths produce identical results.
Malformed tests verify that invalid inputs produce errors (not panics or UB).
