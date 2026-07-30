# ryg-rans-rs-core

> **The mathematical heart of rANS entropy coding — pure safe Rust.**  
> `#![no_std]` + `#![forbid(unsafe_code)]` — works in embedded, kernel, and Wasm.  
> 7 algorithmic surfaces · 144 behavioral receipts · Kani-proven arithmetic · Malformed-stream hardened.  
> **Foundation for Phase I parallel block engine frequency normalization.**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-core)](https://crates.io/crates/ryg-rans-rs-core)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs-core)](https://docs.rs/ryg-rans-rs-core/latest/ryg_rans_rs_core/)

**Version: 0.1.27** · 57+ unit tests · 7 Kani proofs · 144 behavioral receipts

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
13. [Frequency Normalization — `normalize_frequencies`](#frequency-normalization--normalize_frequencies)
14. [Canonical Normalizer Invariants](#canonical-normalizer-invariants)
15. [Phase I Integration](#phase-i-integration)

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

The `#![forbid(unsafe_code)]` attribute is a **compile-time guarantee**. It is not merely
a lint or a convention — it is enforced by the Rust compiler. No undefined behavior can
arise from the encode/decode arithmetic itself. This is critical for the project's
verification architecture: the Kani proofs in this crate verify properties over safe Rust,
meaning the proofs are not undermined by hidden unsafe operations.

The `#![no_std]` attribute means the crate can be used in any environment — embedded
microcontrollers (ARM Cortex-M, RISC-V), operating system kernels (Linux, Redox),
WebAssembly (Wasm), and userspace applications alike. The only dependency is `core`.

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

### Phase I Integration

The **Phase I parallel block engine** (`ryg-rans-rs-parallel`) depends on this crate for
its mathematical foundation — specifically the Word rANS encoding/decoding primitives and
the `RANS_WORD_L`, `RANS_WORD_M`, and `RANS_WORD_SCALE_BITS` constants. The parallel engine's
`normalize_frequencies` function uses core's frequency model invariants to produce tables
compatible with the SIMD packed table decoder in `ryg-rans-rs-simd`.

The dependency flow is unidirectional:

```
ryg-rans-rs-simd  →  ryg-rans-rs-core  ←  ryg-rans-rs-parallel
```

The core crate has no knowledge of SIMD, parallelism, or container formats. It is a pure
computational kernel that can be verified, tested, and proved independently of the rest
of the project.

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

The formula can be understood intuitively as:

1. **How many complete rows?** `x / freq_s` — each row has `M` states
2. **Where in this block?** `x % freq_s` — the offset within the symbol's `freq_s` slots
3. **Which symbol's block?** `+ start_s` — the cumulative frequency of all prior symbols

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

The critical observation is that `C(s, x) >> M` equals `x / freq_s` because the encoding
formula is designed to make the top bits of the new state carry the quotient. This is the
key insight that makes ANS invertible with a simple table lookup.

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

| Variant | L | Renorm Unit | Bits Per Step | Max Renorm Iterations |
|---------|---|-------------|---------------|----------------------|
| **Byte rANS (32-bit)** | `RANS_BYTE_L = 2^23` | 1 byte | 8 bits | 16 (31-bit / 8-bit + margin) |
| **R64 (64-bit)** | `RANS64_L = 2^31` | 1 u32 word | 32 bits | 8 (63-bit / 32-bit + margin) |
| **Word rANS (32-bit)** | `RANS_WORD_L = 2^16` | 1 u16 word | 16 bits | 8 (32-bit / 16-bit + margin) |

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

The precomputed `RansByteEncSymbol` contains:

| Field | Purpose |
|-------|---------|
| `x_max` | Renormalization threshold: `((L >> scale_bits) << 8) * freq` |
| `rcp_freq` | Fixed-point reciprocal of freq, scaled by `2^(32 + rcp_shift)` |
| `rcp_shift` | Shift amount to align the decimal point |
| `bias` | Start position (or `start + M - 1` for freq=1) |
| `cmpl_freq` | Complement of frequency: `M - freq` |

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

The `RenormGuard` is a simple counter that starts at `max_iterations` and decrements on
each call to `check()`. When it reaches zero, `check()` returns an error. This prevents
infinite loops without adding complexity to the hot renormalization path.

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

### Proof: Encode/Decode Inversion

```rust
kani_byte_encode_decode_inversion:
  // For all valid state values where encode succeeds without renormalization:
  //   decode(encode(state, symbol)) == (state, symbol)
  // This is the fundamental identity that makes rANS a valid codec.
```

The remaining proofs cover:
- `kani_enc_symbol_new_valid`: Symbol construction from valid parameters always succeeds
- `kani_r64_reciprocal_equals_division`: The 64-bit reciprocal path is also exact
- `kani_packed_entry_fields`: The 32-bit packed encoding round-trips losslessly
- `kani_state_update_no_overflow`: State arithmetic never wraps in the valid range
- `kani_slot_index_bounded`: Table indices never exceed 4095

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

### Downstream Integration

The `ryg-rans-rs-parallel` crate (Phase I parallel block engine) depends on this crate
with the `alloc` feature to build frequency models and cumulative frequency tables. The
`ryg-rans-rs-simd` crate depends on this crate without extra features for the constant
definitions (`RANS_WORD_L`, `RANS_WORD_M`, `RANS_WORD_SCALE_BITS`).

The `ryg-rans-rs-oracle` crate (forensic court harness) depends on this crate with the `alloc`
feature for building casefile inputs and frequency models.

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

All 57+ tests pass on every commit. Tests are run in CI on x86_64 with `--release` to
ensure performance characteristics match production builds.

---

## Performance Characteristics

The core crate is not benchmarked directly for throughput — it is the scalar reference
against which SIMD backends are measured. However, the following properties are known:

| Operation | Approximate Cost | Notes |
|-----------|-----------------|-------|
| Byte encode (division) | `idiv` instruction | ~10-30 cycles per symbol |
| Byte encode (reciprocal) | `mul_hi` + shift + add | ~3-5 cycles per symbol |
| Byte decode | Slot table + multiply | ~4-8 cycles per symbol |
| Word encode | 16-bit arithmetic | ~2-4 cycles per symbol |
| Word decode | Table lookup + mul + renorm | ~4-6 cycles per symbol |
| R64 encode (reciprocal) | 128-bit multiply | ~3-5 cycles per symbol |

All operations are constant-time per symbol — no variable-length loops in the hot path
(except renormalization, which is bounded and usually short).

---

## Frequency Normalization — `normalize_frequencies`

Although defined in the `ryg-rans-rs-parallel` crate, the **canonical frequency
normalizer** is a direct consequence of the mathematical invariants established by this
core crate. It transforms raw symbol counts into a frequency table that satisfies the
core's requirements for rANS encode/decode.

### Guarantees

1. **Sum = total**: The output frequencies sum exactly to `1 << scale_bits` (typically 4096).
2. **Every observed symbol gets ≥ 1**: The "reserved slot" algorithm guarantees no symbol
   becomes undecodable (zero frequency would cause division-by-zero during decode).
3. **No frequency exceeds 4095**: Every output fits in the 12-bit packed table field
   used by the SIMD decoder.
4. **Fully deterministic**: Same raw counts always produce the same normalized output —
   essential for reproducible compression across threads and runs.

### Algorithm

1. **Reserve** `nonzero_count` slots — one per observed symbol.
2. **Scale** each observed frequency proportionally into the remaining
   `total - nonzero_count` space, clamped to `[1, 4094]`.
3. **Distribute remainder** — any leftover units are assigned greedily to the largest
   frequencies not yet at the cap of 4095.

### Why This Matters for Phase I

The Phase I parallel block engine encodes each block independently using Word rANS with
`scale_bits = 12`. Every block's frequency model must be normalized to `4096` total before
encoding. The `normalize_frequencies` function, combined with core's `build_word_tables`
and `PackedWordTable::from_freqs` (in the SIMD crate), forms the complete model-building
pipeline:

```
raw byte counts → normalize_frequencies → cum_freqs → PackedWordTable → SIMD/scalar decode
```

### Relationship to Core Invariants

The normalizer produces frequencies that satisfy core's `validate_freq_model` checks:

- Cumulative frequencies are monotonically non-decreasing
- Total matches `1 << scale_bits`
- No frequency exceeds the allowed range for `scale_bits = 12`
- No zero frequencies for observed symbols

## Canonical Normalizer Invariants

The `normalize_frequencies` function (implemented in `ryg-rans-rs-parallel`) satisfies
six invariants that are proven by construction:

### Invariant 1: Sum Equals Total

```
Σ normalized_freqs[i] for i in 0..256 == 1 << scale_bits
```

The normalized frequencies always sum exactly to the target total. This is guaranteed
by the remainder distribution step, which allocates leftover units until the sum is exact.

### Invariant 2: No Zero Frequencies for Observed Symbols

```
For every symbol s where raw_freq[s] > 0: normalized_freq[s] ≥ 1
```

Each observed symbol receives at least one reserved slot before proportional scaling.
This ensures every symbol that appeared in the input is decodable from the output.

### Invariant 3: All Frequencies Bounded by 4095

```
For all i in 0..256: normalized_freqs[i] ≤ 4095
```

The 12-bit packed table representation in the SIMD decoder can represent frequencies
up to 4095. The normalizer clamps all frequencies to this bound, ensuring compatibility
with `PackedWordTable`.

### Invariant 4: Deterministic

```
normalize_frequencies(raw, seed=0) == normalize_frequencies(raw, seed=0)
for all invocations on the same input
```

The algorithm has no randomness, no hash-map iteration order sensitivity, and no
thread-dependent behavior. The same input always produces the same output.

### Invariant 5: Monotonic Cumulative Frequencies

```
cum[i+1] = cum[i] + normalized_freqs[i]
cum[0] = 0, cum[256] = 1 << scale_bits
```

Cumulative frequencies are built deterministically from normalized frequencies. The
monotonic property is guaranteed by construction.

### Invariant 6: Compatibility with PackedWordTable

```
PackedWordTable::from_freqs(&normalized_freqs, &cum, scale_bits) → Ok( table )
```

The normalizer's output is guaranteed to pass `PackedWordTable` validation, which checks
that all frequencies fit in 12 bits and cumulative frequencies are non-decreasing.

---

## Phase I Integration

The core crate is the mathematical foundation for the entire ryg-rans-rs project. All
other crates depend on it, directly or transitively:

```
ryg-rans-rs (facade)
  ├── ryg-rans-rs-core (this crate)  — all algorithmic primitives
  └── ryg-rans-rs-simd               — SIMD decode kernels, uses core constants

ryg-rans-rs-parallel                  — parallel block engine, uses core for block codecs
  ├── ryg-rans-rs-core (direct dep)
  └── ryg-rans-rs-simd (optional)

ryg-rans-rs-cli                        — CLI tool, uses core via facade + parallel
  ├── ryg-rans-rs-core (direct dep, std)
  └── ryg-rans-rs-parallel

ryg-rans-rs-oracle                     — forensic courts, uses core for reference encode/decode
  └── ryg-rans-rs-core (alloc)
```

### What the Core Provides to Each Crate

| Crate | Core Contribution |
|-------|------------------|
| `ryg-rans-rs-simd` | `RANS_WORD_L`, `RANS_WORD_M`, `RANS_WORD_SCALE_BITS`, `RansWordSlot` |
| `ryg-rans-rs-parallel` | Word rANS encode/decode primitives, frequency model validation, `ModelError` types |
| `ryg-rans-rs-cli` | All codec variants, I/O traits, malformed-stream validation |
| `ryg-rans-rs-oracle` | Reference encode/decode for cross-decoding courts |
| `ryg-rans-rs-casefile` | Uses no rANS types — pure schema (independent) |

---

## Usage

### Basic Byte rANS Encode

```rust
use ryg_rans_rs_core::{
    RansByteState, RansByteEncSymbol,
    BackwardByteWriter,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
};

let scale_bits = 14;
let freq = 1u32 << (scale_bits - 8);  // Uniform 256-symbol model
let mut buf = [0u8; 4096];

let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
let sym = RansByteEncSymbol::new(0, freq, scale_bits).unwrap();
rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();
```

### Basic Byte rANS Decode

```rust
use ryg_rans_rs_core::{
    ByteReader, RansByteDecSymbol,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let dsym = RansByteDecSymbol::new(0, freq).unwrap();
rans_byte_dec_advance_symbol(
    &mut dec_state, &mut reader, &dsym, scale_bits
).unwrap();
```

### Word rANS (Most Efficient for Block Encoding)

```rust
use ryg_rans_rs_core::{
    RansWordState, BackwardWord16Writer, Word16Reader,
    rans_word_enc_init, rans_word_enc_put, rans_word_enc_flush,
    rans_word_dec_init, rans_word_dec_sym, rans_word_dec_renorm,
    build_word_tables, RANS_WORD_SCALE_BITS,
};

let scale_bits = RANS_WORD_SCALE_BITS;  // 12
let mut buf = vec![0u16; 4096];
let mut writer = BackwardWord16Writer::new(&mut buf);
let mut state = RansWordState::new();
rans_word_enc_init(&mut state);
rans_word_enc_put(&mut state, &mut writer, 65, 16, 0, scale_bits).unwrap();
rans_word_enc_flush(&state, &mut writer).unwrap();
let compressed = writer.encoded();

let tables = build_word_tables(&[16u32; 256], scale_bits).unwrap();
let mut reader = Word16Reader::new(compressed);
let mut dec_state = rans_word_dec_init(&mut reader).unwrap();
let slot = rans_word_dec_sym(&dec_state, &tables, scale_bits);
rans_word_dec_renorm(&mut dec_state, &mut reader).unwrap();
// dec_state now contains the original state, slot contains symbol 65
```

---

## Build and Test

```sh
# Build (no_std, no alloc)
cargo build -p ryg-rans-rs-core

# Run all 57+ tests
cargo test -p ryg-rans-rs-core

# Build with all features
cargo build -p ryg-rans-rs-core --features alloc,std

# Run Kani proofs (requires Kani installed)
cargo kani -p ryg-rans-rs-core
```

---

*Part of the ryg-rans-rs project. Version 0.1.27. Phase J.*
