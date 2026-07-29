# ryg-rans-rs-simd

> **SSE4.1 + AVX-512 accelerated rANS decoder kernels.**  
> 8-way interleaved Word rANS decode with AVX512VL, SSE4.1, and scalar backends.  
> 16-way interleaved Word rANS decode with AVX-512 and scalar backends.  
> `#![no_std]` — works in embedded and kernel contexts on x86_64.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-simd)](https://crates.io/crates/ryg-rans-rs-simd)

---

## Why This Crate Exists

Word rANS decoding is fundamentally a **table-lookup-and-arithmetic** loop. For each symbol:

1. **Slot lookup**: `slot = state & 4095` — mask off the low 12 bits
2. **Table gather**: read `frequency`, `bias`, and `symbol` from the 4096-slot table
3. **State update**: `state = frequency * (state >> 12) + bias` — the core ANS transition
4. **Renormalization**: if `state < 65536`, read one 16-bit word from the stream and shift it in

These steps are individually simple, but their composition makes SIMD acceleration challenging:
- The table lookup is an **address-dependent gather** — each lane may access a different table slot
- Renormalization is **masked and data-dependent** — only some lanes consume input words
- The output symbols must be collected in **exact lane order**

This crate implements three strategies for accelerating the decode loop:
- **SSE4.1 4-lane**: scalar gather, SIMD arithmetic (legacy, ~0.4× scalar speed)
- **AVX512VL 8-lane**: vector gather with `_mm256_i32gather_epi32` (~1.0× scalar or better)
- **AVX512 16-lane**: native 16-way format with `_mm512_i32gather_epi32` (new format, higher throughput)

The key insight enabling AVX-512 wins is the **packed table** — a single `u32` per slot containing `freq|bias<<12|sym<<24`. A single gather instruction loads all three fields for 8 or 16 lanes simultaneously, eliminating the scalar extraction overhead that limited the SSE4.1 path.

---

## Backend Selection Guide

| Backend | Label | ISA | Format | When to Use |
|---------|-------|-----|--------|-------------|
| Scalar 8-way | `scalar-8way` | Baseline x86_64 | 8-way | Always available; best for tiny blocks |
| SSE4.1 8-way | `sse41-8way` | SSSE3+SSE4.1 | 8-way | Explicit compat; slower than scalar on Zen 4/5 |
| **AVX512VL 8-way** | `avx512vl-8way` | AVX512F+VL+BW | 8-way | **Best for AVX512 CPUs** on existing 8-way streams |
| Scalar 16-way | `scalar-16way` | Baseline x86_64 | 16-way | Always available; reference for new format |
| **AVX512 16-way** | `avx512-16way` | AVX512F+BW | 16-way | **Highest throughput** on AVX512 CPUs |

The auto-dispatch functions select the fastest available backend at runtime:

```text
8-way dispatch:  AVX512VL → SSE4.1 → scalar
16-way dispatch: AVX512 → scalar
```

The automatic backends never execute an unsupported instruction — runtime CPU feature detection
is performed before calling any `#[target_feature]`-gated kernel.

---

## Architectural Deep Dive

### Packed Decode Table (`packed_table.rs`)

The 4096-slot Word rANS decode table is the central data structure. Each slot maps a
cumulative-frequency value (0..4095) to the corresponding symbol and its decode parameters.

#### Legacy representation (v0.1.13 and earlier)

```rust
pub struct RansWordSlot { pub freq: u16, pub bias: u16 }  // 4 bytes
pub struct RansWordTables {                                 // 12 KB total
    pub slots: &[RansWordSlot],     // 4096 × 4 bytes = 16 KB
    pub slot2sym: &[u8],           // 4096 × 1 byte = 4 KB
}
```

Two separate structures → two cache lines touched per decode → scatter loads.

#### Packed representation (Phase G)

```rust
#[repr(transparent)]
pub struct PackedWordEntry(pub u32);  // 4 bytes per slot
```

**Bit layout**: `freq(12 bits) | bias(12 bits) | symbol(8 bits)`

A single `u32` per slot means:
1. **One cache line** contains 16 entries (64 bytes / 4 bytes)
2. **One gather instruction** loads freq, bias, and symbol for all lanes
3. **64-byte alignment** (`#[repr(align(64))]`) ensures the entire table fits in one cache line at the hot-start offset

#### Table construction

```rust
pub fn from_freqs(freqs: &[u32], cum_freqs: &[u32], scale_bits: u32)
    -> Result<PackedWordTable, ModelError>
```

Construction validates:
- `scale_bits == 12` (upstream constant enforcement)
- Cumulative frequencies are monotonically non-decreasing
- Total frequency sums to `1 << 12`
- Every slot's frequency and bias fit in 12 bits (max 4095)

For each slot `s` (0..4095):
1. Search cumulative frequencies to determine which symbol owns slot `s`
2. Extract that symbol's `frequency` and compute `bias = slot - cumulative_start`
3. `packed = freq | (bias << 12) | (symbol << 24)`

**Kani proof**: `kani_packed_entry_fields()` proves that packing and unpacking are exact
round-trips for all valid freq/bias/symbol values within their bit-widths.

#### Equivalence verification

```rust
pub fn verify_equivalence(&self, slots: &[RansWordSlot], slot2sym: &[u8])
    -> Result<(), EquivalenceError>
```

Called in unit tests to prove that the packed table produces the exact same symbols,
frequencies, and biases as the legacy representation. Every slot is compared.

---

### AVX512VL.INTERLEAVED8 (`avx512.rs`)

This decoder consumes the **existing canonical 8-way Word rANS stream format** — the same
format used by the scalar 8-way, SSE4.1, and C oracle decoders. No format change required.

#### Why AVX512VL instead of full AVX512?

AVX512VL ("Vector Length") allows 256-bit AVX-512 operations. This is important because:
- The 8-way format has exactly 8 states — a perfect fit for 8 × u32 in a `__m256i`
- AVX512VL provides **masked operations** (like `_mm256_cmplt_epu32_mask`) that SSE lacks
- AVX512VL provides **gather** (`_mm256_i32gather_epi32`) that SSE4.1 doesn't have
- Full 512-bit registers would be wasteful for 8 lanes

#### Decode loop (one iteration = 8 symbols)

```text
1. GATHER:  indices = state & 4095
            packed  = _mm256_i32gather_epi32(table_ptr, indices, 4)
            // One instruction loads 8 × u32 from 8 different table slots

2. UNPACK:  freq   = packed & 0x0fff
            bias   = (packed >> 12) & 0x0fff
            symbol = packed >> 24
            // Bitwise extraction — no serialization

3. STORE:   temp[0..7] = symbols[0..7]  (via temporary buffer, preserving lane order)
            output[i..i+8] = temp

4. UPDATE:  xscaled   = state >> 12
            state     = (xscaled * freq) + bias
            // Lane-wise multiply-add — the core ANS transition

5. RENORM:  renorm_mask = state < 65536  (per lane, via _mm256_cmplt_epu32_mask)
            words_needed = popcount(renorm_mask)
            // For each active lane, read one u16 and shift into state
            // Inactive lanes are untouched — no masked load overread
```

#### Why lane-wise renormalization instead of masked expand?

Upstream `rans_word_sse41.h` uses a 16-entry shuffle table for byte extraction.
The AVX512VL kernel uses a simpler approach: iterate lanes 0..7, check each lane's
mask bit, and read one u16 per active lane. This is correct because:

1. The scalar decoder does exactly this — ascending lane order
2. No masked-load overread issues — only the exact words needed are read
3. The performance cost is negligible (at most 8 iterations, typically 1-2)

A future optimization could use `_mm256_mask_expand_epi16` for a single-instruction
expand-load, but the lane-wise approach is simpler and provably correct.

#### Tail handling

When the output length is not divisible by 8, the remaining `r` symbols (1..7) are
decoded using scalar per-lane logic. The SIMD state is stored to a temporary array,
each active lane is decoded individually, and the state is reloaded.

Each tail length is tested in `test_avx512vl8_various_lengths`.

---

### AVX512.INTERLEAVED16 (`avx512.rs`)

This is a **new 16-way stream format** with higher throughput potential. Unlike the 8-way
format which has two 4-lane SIMD units, the 16-way format has a single 16-lane SIMD unit
that decodes 16 symbols per iteration.

#### Stream format specification

**Encoding** (performed by `encode_interleaved16` in `packed_table.rs`):

```
Input:  symbol[0], symbol[1], ..., symbol[N-1]
Lanes:  lane 0 gets symbols 0, 16, 32, ...
        lane 1 gets symbols 1, 17, 33, ...
        ...
        lane 15 gets symbols 15, 31, 47, ...

Processing: symbols are encoded in REVERSE order (last to first)
            each symbol is assigned to lane = i & 15

Flush order: states are written in REVERSE lane order
             state[15].low, state[15].high,
             state[14].low, state[14].high,
             ...
             state[0].low, state[0].high

Result: the forward stream contains initial states in ASCENDING lane order
        0, 1, 2, ..., 15
```

**Decoding** (performed by `decode_interleaved16_scalar` and `decode_interleaved16_avx512_kernel`):

```
Init: read 32 u16 words → 16 states in lane order 0..15

Decode groups of 16:
  1. Decode all 16 lanes (gather + unpack + update)
  2. Store 16 symbols in lane order 0..15
  3. Renorm active lanes in ascending order
  4. Advance reader by popcount(mask)

Tail (r symbols, 0 < r < 16):
  1. Decode lanes 0..r-1 only
  2. Lanes r..15 are untouched
```

#### Why a new format?

The existing 8-way format uses two 4-lane SIMD units (dec0, dec1) to achieve 8-way
interleaving. This was a pragmatic choice for SSE4.1 where 128-bit registers limit
each unit to 4 lanes.

With AVX-512 512-bit registers (16 × u32), a single unit can decode 16 lanes in parallel.
The new format:
- Eliminates the two-unit coordination overhead
- Doubles the arithmetic density per gather (16 slots vs 8)
- Has a simpler tail-handling model (0..r-1 instead of two separate units)
- Requires its own scalable encoder and C oracle

#### 512-bit gather performance characteristics

The `_mm512_i32gather_epi32` instruction loads 16 × u32 from potentially 16 different
cache lines. Performance depends on:
- **Table locality**: the packed table fits in 16 KB (L1 cache), so gathers typically hit L1
- **Lane distribution**: uniform models spread evenly; skewed models may cluster
- **Port pressure**: gathers use port 5 on modern Intel/AMD; the surrounding arithmetic
  can overlap with other ports

The 16-way format is expected to excel on:
- Large blocks (>4 KB) where initialization overhead is amortized
- Uniform or near-uniform models where gather patterns are predictable
- CPUs with efficient gather implementation (Zen 4/5, Ice Lake and later)

---

## Safety Architecture

### Unsafe code isolation

All `unsafe` code is confined to `#[target_feature]`-gated kernel functions.
The safe public API uses runtime detection before calling these kernels.

The 7 unsafe functions are:

| Function | File | ISA | Risk |
|----------|------|-----|------|
| `rans_simd_dec_init` | lib.rs | SSE2 | Unaligned 128-bit load (bounds-checked) |
| `rans_simd_dec_sym_unchecked` | lib.rs | SSE4.1 | Table gather via scalar extraction |
| `rans_simd_dec_renorm_unchecked` | lib.rs | SSSE3+SSE4.1 | Shuffle-mask renorm (scratch buffer) |
| `decode_interleaved8_avx512vl_kernel` | avx512.rs | AVX512VL | Gather + masked renorm |
| `decode_interleaved16_avx512_kernel` | avx512.rs | AVX512F+BW | 512-bit gather + masked renorm |
| `decode_simd_8way_unchecked` | lib.rs | SSE4.1 | Entry point wrapper |
| `decode_interleaved8_avx512vl` (wrapper) | backends.rs | — | Calls gated kernel |
| `decode_interleaved16_avx512` (wrapper) | backends.rs | — | Calls gated kernel |

### Inactive-lane safety

The renormalization loop processes lanes **individually** (not via masked load), so
inactive lanes never read input. This eliminates the most common SIMD memory-safety bug.

### Input bounds

Every decoder checks at entry:
- `compressed.len() >= N_INIT_WORDS` (16 for 8-way, 32 for 16-way)
- Before each renormalization: `reader.remaining >= popcount(mask)`
- Tail renorm: `reader_pos < compressed.len()` per lane

### Table bounds

Slot indices are always masked: `slot = state & 4095`. The table is guaranteed to have
exactly 4096 entries by the `PackedWordTable` type invariant (construction validates this).

---

## Runtime Feature Detection

The safe auto-dispatch functions use `cfg!(target_feature = "...")` for compile-time
detection when `std` is unavailable, and `std::is_x86_feature_detected!()` when `std`
is available:

```rust
fn avx512vl_available() -> bool {
    #[cfg(feature = "std")] {
        std::is_x86_feature_detected!("avx512f")
            && std::is_x86_feature_detected!("avx512vl")
            && std::is_x86_feature_detected!("avx512bw")
    }
    #[cfg(not(feature = "std"))] {
        cfg!(all(target_feature = "avx512f", target_feature = "avx512vl", target_feature = "avx512bw"))
    }
}
```

This means:
- When compiled with `RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw"`, the
  SIMD path is used even without `std`
- When compiled without those flags but with `std`, runtime CPUID detects available features
- Without `std` or target features, the scalar fallback is always used

---

## Verification

### Tests (32 pass, 0 fail)

| Test Group | Count | What It Verifies |
|------------|-------|------------------|
| Packed table | 6 | Field extraction, scale validation, equivalence to legacy, round-trip |
| Scalar 8-way | 1 | Packed decoder matches existing scalar decoder |
| Scalar 16-way | 3 | Round-trip, tail lengths 0..15, truncated rejection |
| State ordering | 1 | Stream layout: 16 initial states in lane order |
| AVX512VL 8-way | 3 | Scalar equivalence, various lengths, truncated rejection |
| AVX512 16-way | 3 | Scalar equivalence, all tails, truncated rejection |
| Backend dispatch | 5 | Backend labels, scalar dispatch, truncated rejection |
| Malformed input | 6 | Truncated partial init, decode, wrong-format detection, state invariants |
| Mask exhaustion 8-way | 1 | All 256 renormalization masks |
| Original SIMD tests | 4 | SSE4.1 round-trip, lengths, renorm, truncated |

### Mask exhaustion

Every possible renormalization mask is tested:

- **8-way**: 256 masks (2^8) — fast enough for debug mode
- **16-way**: 65,536 masks (2^16) — requires `--release` (~1 second)

For each mask, we verify:
1. Correct popcount = words consumed
2. Truncated stream (one fewer word) is correctly rejected
3. Full stream decodes without error

### Fuzzing

Two new fuzz targets (`avx512vl8_roundtrip` and `avx512_16way_roundtrip`) test:
- Arbitrary byte sequences → model construction → encode → scalar decode → AVX decode
- Output equivalence assertion
- Word consumption equality assertion
- Error consistency (both backends must agree on success/failure)

### Kani proofs

Three proofs in `crates/ryg-rans-rs-core/kani/packed_entry_proof.rs`:
- `kani_packed_entry_fields`: pack/unpack round-trip for all valid field values
- `kani_state_update_no_overflow`: state update arithmetic bounds
- `kani_slot_index_bounded`: slot index always within 0..4096

---

## Module Reference

### `packed_table.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `PackedWordEntry` | struct | Single u32 entry with freq/bias/symbol extraction |
| `PackedWordTable` | struct | 4096-entry table with 64-byte alignment |
| `PackedWordTable::from_freqs` | method | Validated construction from freq model |
| `PackedWordTable::verify_equivalence` | method | Per-slot comparison with legacy table |
| `PackedWordTable::as_ptr` | method | Raw pointer for gather operations |
| `decode_8way_packed_scalar` | function | Scalar 8-way decode using packed table |
| `encode_interleaved16` | function | 16-way encoder for new format |
| `decode_interleaved16_scalar` | function | Scalar 16-way decode with DecodeReport |
| `DecodeReport` | struct | Words consumed + 16 final states |
| `EquivalenceError` | struct | Slot-by-slot mismatch details |

### `avx512.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `decode_interleaved8_avx512vl_kernel` | unsafe fn | 8-way AVX512VL decode (requires avx512f+vl+bw) |
| `decode_interleaved16_avx512_kernel` | unsafe fn | 16-way AVX512 decode (requires avx512f+bw) |
| `NUM_WORDS_8` | static | Popcount lookup table for 8-lane masks |
| `NUM_WORDS_16` | static | Popcount lookup table for 16-lane masks |

### `backends.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `DecodeBackend` | enum | 5 backend variants with stable `label()` strings |
| `DecodeResult` | struct | Output + DecodeReport + DecodeBackend |
| `DecodeError` | enum | Error types: InputTooShort, InvalidTable, etc. |
| `decode_interleaved8_auto` | fn | Safe auto-dispatch: AVX512VL → scalar |
| `decode_interleaved8_avx512vl` | unsafe fn | Explicit AVX512VL 8-way |
| `decode_interleaved8_scalar` | fn | Explicit scalar 8-way |
| `decode_interleaved16_auto` | fn | Safe auto-dispatch: AVX512 → scalar |
| `decode_interleaved16_avx512` | unsafe fn | Explicit AVX512 16-way |
| `decode_interleaved16_scalar` | fn | Explicit scalar 16-way |

---

## Performance Expectations

The AVX-512 decoders are designed to address the bottleneck that limited the SSE4.1 path:
**scalar gather**. The SSE4.1 decoder extracts four lane indices to scalar registers,
performs four separate table lookups, and reconstructs the vector with insert instructions.
The AVX-512 decoder performs all lookups with a single gather instruction.

Expected characteristics (measured on Ryzen 7 9800X3D, Zen 5 architecture):

- **AVX512VL 8-way vs scalar 8-way**: expected improvement from the single gather replacing
  8 scalar lookups. The exact crossover point depends on block size and model distribution.
- **AVX512 16-way vs scalar 16-way**: higher arithmetic density (16 lanes per gather vs 8)
  but requires the new format.

Performance receipts are created separately from behavioral receipts. See `docs/performance-method.md`.

---

## Feature Flags

```toml
[features]
default = []
std = []     # Enables std::is_x86_feature_detected! for runtime backend detection
```

---

## Build and Test

```sh
# Build with all SIMD backends enabled
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" cargo build

# Run all tests (32 tests)
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" cargo test

# Exhaustive 16-way mask test (requires --release, ~1 second)
RUSTFLAGS="-C target-feature=+avx512f,+avx512bw" cargo test --release -p ryg-rans-rs-simd -- --ignored
```

---

## Unsafe Code

All 7 `unsafe fn` are documented in `docs/unsafe-ledger.md` with:
- Preconditions
- Alignment assumptions
- Bounds checks performed
- CPU features required
- Inactive-lane safety analysis
- Soundness justification
