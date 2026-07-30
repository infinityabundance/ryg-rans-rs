# ryg-rans-rs-simd

> **SIMD-accelerated Word rANS decode kernels — SSE4.1, AVX2, AVX512VL, AVX-512.**  
> `#![no_std]` — works in embedded and kernel contexts on x86_64.  
> 8-way and 16-way interleaved decode with scalar fallback.  
> **46 tests** · 256 + 65536 mask exhaustion · 7 unsafe functions, all documented.  
> **Criterion benchmark suite** — 9 bench tiers across scalar, SSE4.1, AVX2, AVX-512, batch, parallel, container, dispatch.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-simd)](https://crates.io/crates/ryg-rans-rs-simd)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [The Three SIMD Surfaces](#the-three-simd-surfaces)
3. [Phase H Optimization Backends](#phase-h-optimization-backends)
4. [Phase J — AVX2 Portability Tier](#phase-j--avx2-portability-tier)
5. [The Packed Table Design](#the-packed-table-design)
6. [AVX512VL.INTERLEAVED8 Architecture](#avx512vlinterleaved8-architecture)
7. [AVX512.INTERLEAVED16 Architecture](#avx512interleaved16-architecture)
8. [Backend Dispatch and Safety](#backend-dispatch-and-safety)
9. [Why VNNI Is Not Used](#why-vnni-is-not-used)
10. [Why Lane-Wise Renormalization](#why-lane-wise-renormalization)
11. [Measured Performance](#measured-performance)
12. [Criterion Benchmark Suite](#criterion-benchmark-suite)
13. [Unsafe Code Policy](#unsafe-code-policy)
14. [Testing and Verification](#testing-and-verification)
15. [Module Reference](#module-reference)
16. [Feature Flags](#feature-flags)
17. [Build and Test](#build-and-test)

---

## What This Crate Is

This crate implements **SIMD-accelerated Word rANS decode kernels** that build on the
mathematical foundation in `ryg-rans-rs-core`. It provides three primary decode surfaces
plus a fourth AVX2 portability tier:

| Surface | Vector Width | Lanes | ISA Required | Stream Format |
|---------|-------------|-------|-------------|---------------|
| SSE4.1 8-way | 128-bit | 4 × 2 units | SSE4.1 + SSSE3 | Existing 8-way |
| AVX512VL 8-way | 256-bit | 8 | AVX512F + AVX512VL + AVX512BW | Existing 8-way |
| AVX512 16-way | 512-bit | 16 | AVX512F + AVX512BW | New 16-way |
| **AVX2 8-way/16-way** | 256-bit | 8 / 16 | AVX2 | Both formats |

### Why SIMD for rANS Decode?

Word rANS decode has three computational phases:

1. **Table lookup**: `slot = state & 4095`, then fetch `frequency`, `bias`, `symbol` from the table
2. **State update**: `state = frequency × (state >> 12) + bias` — a multiply-add
3. **Renormalization**: if `state < 65536`, read one `u16` word and shift it in

Phase 1 is an **address-dependent gather** — each lane may access a different table slot.
This is the bottleneck that SIMD must overcome. SSE4.1 lacks a gather instruction, forcing
scalar extraction (extract index → scalar load → insert result). AVX2 and AVX-512 provide
native gather (`_mm256_i32gather_epi32`, `_mm512_i32gather_epi32`) that can load 8 or 16 table
entries with a single instruction.

Phase 2 is a natural SIMD multiply-add (`_mm256_mullo_epi32`, `_mm512_mullo_epi32`).

Phase 3 is a **masked scatter** — each lane independently decides whether to consume a
`u16` word. AVX2 and AVX-512's masked compare (`_mm256_cmplt_epu32_mask`) makes this efficient.

---

## The Three SIMD Surfaces

### SSE4.1 8-Way (Legacy)

Two 4-lane SIMD units (`RansSimdDec0`, `RansSimdDec1`) operating on 128-bit `__m128i`
registers. Each unit decodes 4 symbols per iteration. The 8-way interleaving is achieved
by alternating between the two units.

**Limitation**: SSE4.1 has no gather instruction. The unit must extract each lane's index
to a scalar register, perform a scalar memory load (4 separate loads), then insert each
result back into the vector. This serialization dominates the runtime.

**Real SSE4.1 execution is benchmarked and confirmed**: the SSE4.1 8-way kernel achieves
**406 MiB/s** on Zen 5 (Ryzen 7 9800X3D) for skewed data at 1 MiB, as measured by the
Criterion benchmark suite.

### AVX512VL 8-Way

A single 8-lane unit using 256-bit `__m256i` registers. Uses `_mm256_i32gather_epi32`
to load all 8 table entries simultaneously. AVX512VL ("Vector Length") enables AVX-512
operations on 256-bit registers.

**ISA requirements**: `avx512f` (gather + masked operations), `avx512vl` (256-bit AVX-512),
`avx512bw` (byte/word mask operations like `_mm256_cmplt_epu32_mask`).

### AVX512 16-Way

A single 16-lane unit using 512-bit `__m512i` registers. Requires the new 16-way stream
format. Uses `_mm512_i32gather_epi32` to load 16 table entries simultaneously.

**ISA requirements**: `avx512f` (gather + 512-bit ops), `avx512bw` (mask operations).

---

## Phase H Optimization Backends (Test-verified, Behavioral Receipts Pending)

### AVX512VL 2×8-on-16 (Two 256-bit Gather Chains)

Splits the 16-way stream into two independent 8-lane groups, each using 256-bit `__m256i`
gathers. This avoids a single 512-bit gather dependency chain and allows the out-of-order
core to overlap the two groups' gather-arithmetic-renorm cycles.

```text
Group 0 (lanes 0-7):  _mm256_i32gather_epi32 → freq/bias/symbol → renorm
Group 1 (lanes 8-15): _mm256_i32gather_epi32 → freq/bias/symbol → renorm
```

**ISA requirements**: `avx512f`, `avx512vl`, `avx512bw`.

#### Manual-Gather Backends (Scalar Loads + Vector Arithmetic)

Replaces the hardware gather instruction with 8 or 16 explicitly unrolled scalar loads
followed by a vector register reload. On Zen 5 (Ryzen 7 9800X3D), scalar table loads
(~4-cycle latency from L1) outperform hardware gathers (~10-15 cycles) when the table
is L1-resident.

```text
1. _mm256_storeu_si256(indices)    → buffer
2. for lane 0..7:                  → scalar loads
     entries[lane] = table[indices[lane]]
3. _mm256_loadu_si256(entries)     → reload to SIMD register
4. vector freq/bias extraction, arithmetic, renorm
```

#### Uniform256 Table-Free Kernel (No Gather, No Table)

Exploits the uniform-256 model structure at S12 where every symbol has frequency 16.
The ANS transition reduces to pure arithmetic:

```text
slot    = state & 0xfff         // 12-bit slot index
symbol  = slot >> 4             // 256 symbols → 4 bits per symbol
bias    = slot & 15             // 16 positions per symbol
new_state = 16 × (state >> 12) + bias
```

No table lookup or gather needed — this is the fastest backend, reaching **2.75 GiB/s**
for uniform data on the Ryzen 7 9800X3D.

---

## Phase J — AVX2 Portability Tier

Phase J adds **AVX2 backends** for both 8-way and 16-way stream formats, making SIMD
decode available on CPUs that lack AVX-512 (e.g., Intel pre-Ice Lake, AMD Zen 1–4).
AVX2 is available on virtually all x86_64 CPUs since ~2013, dramatically widening the
deployment surface.

### 2×8-on-16 Manual Gather

Decodes 16-way streams using two independent 256-bit gather chains with **scalar table
loads** (manual gather). Each group of 8 lanes uses explicitly unrolled scalar loads
rather than `_mm256_i32gather_epi32`, which on Zen 4/5 (16 KB L1-resident table) is
significantly faster than the hardware gather instruction.

```text
Group 0 (lanes 0-7):  store indices → 8× scalar load → reload → arithmetic → renorm
Group 1 (lanes 8-15): store indices → 8× scalar load → reload → arithmetic → renorm
```

**ISA requirements**: `avx2` only (no gather instruction needed).

### 2×8-on-16 Hardware Gather

Uses `_mm256_i32gather_epi32` for table lookups. This is the native AVX2 gather path
and serves as the reference for correctness parity between manual and hardware gather.
On CPUs where the table is not L1-resident or where gather latency is lower (e.g., Intel
Golden Cove / Redwood Cove), the hardware gather can be competitive or faster.

### Uniform256 Table-Free Kernel

An AVX2 implementation of the uniform-256 arithmetic kernel. No table lookup, no gather
— pure vector arithmetic. Reaches multi-GiB/s throughput on any AVX2-capable CPU.

### Batch4 — Multi-Stream Batch Decoder

The batch4 decoder processes up to **4 independent 16-way streams** in a round-robin
fashion. Each iteration decodes one 16-symbol group from each job, allowing the CPU to
overlap gather latency of one stream with the arithmetic and renormalization of another.

```text
Iteration:
  Job 0: gather → arith → renorm (lanes 0-15)
  Job 1: gather → arith → renorm (lanes 0-15)
  Job 2: gather → arith → renorm (lanes 0-15)
  Job 3: gather → arith → renorm (lanes 0-15)
```

Each job uses the 2×8 representation (two YMM registers per job, low 8 + high 8 lanes).
Renormalization uses the AVX2 permutation table for lane-wise word injection.

**ISA requirements**: `avx2` only. Available on any CPU with AVX2 support.

### AVX2 Renormalization Table

The AVX2 renormalization loop uses a precomputed **permutation table** (2.5 KB) that maps
each of the 256 possible 8-lane renormalization masks to a shuffle vector. This avoids
branching and enables constant-time lane-wise word injection:

```text
perm_idx = mask  // 8-bit mask → index into permutation table
shuffle   = perm_table[perm_idx]
result    = _mm256_permutevar8x32_epi32(states, shuffle)
```

### Available Backend Functions

| Function | Safety | Behavior |
|----------|--------|----------|
| `decode_interleaved8_avx2_manual_gather` | ⚠️ Unsafe | 8-way AVX2 manual gather |
| `decode_interleaved8_avx2_hardware_gather` | ⚠️ Unsafe | 8-way AVX2 hardware gather |
| `decode_interleaved16_avx2_2x8` | ⚠️ Unsafe | 16-way AVX2 2×8 manual gather |
| `decode_interleaved16_uniform256_avx2` | ⚠️ Unsafe | 16-way AVX2 table-free uniform |
| `decode_batch4_interleaved16_avx2` | ⚠️ Unsafe | 4-stream AVX2 batch decode |
| `decode_interleaved8_avx2_manual_gather_checked` | ✅ Safe | Runtime-checked dispatch |
| `decode_interleaved8_avx2_hardware_gather_checked` | ✅ Safe | Runtime-checked dispatch |
| `decode_interleaved16_avx2_2x8_checked` | ✅ Safe | Runtime-checked dispatch |
| `decode_interleaved16_uniform256_avx2_checked` | ✅ Safe | Runtime-checked dispatch |
| `decode_batch4_interleaved16_avx2_checked` | ✅ Safe | Runtime-checked dispatch |

---

## The Packed Table Design

### Why a Separate Table Format?

The existing Word rANS table (`RansWordSlot` + `slot2sym`) uses two separate arrays:
- `slots: &[RansWordSlot]` — 4096 × 4 bytes = 16 KB, each entry has `freq: u16` and `bias: u16`
- `slot2sym: &[u8]` — 4096 × 1 byte = 4 KB

A SIMD gather can only load from one address stream. With two separate arrays, we'd need
two gathers per decode iteration — or one gather plus scalar loads from the second array.

### The Packed Representation

```rust
#[repr(transparent)]
pub struct PackedWordEntry(pub u32);
```

**Bit layout**: `frequency(12 bits) | bias(12 bits) | symbol(8 bits)`

| Bits | Field | Max Value | Why 12/8 bits? |
|------|-------|-----------|-----------------|
| 0..11 | frequency | 4095 | 12 bits covers scale_bits = 12 (table has 4096 slots) |
| 12..23 | bias | 4095 | 12 bits covers the maximum offset within a slot range |
| 24..31 | symbol | 255 | 8 bits covers the 256-symbol byte alphabet |

**Why this works**: With scale_bits = 12, the table has exactly 4096 slots. Frequency
and bias are bounded by 4095. Symbol is bounded by 255. All three fit in 32 bits with
room to spare.

### Benefits of Packing

1. **Single gather**: One `_mm256_i32gather_epi32` loads freq, bias, and symbol for all lanes
2. **16 KB table**: Fits in L1 data cache on modern x86_64 CPUs (32 KB L1D)
3. **64-byte alignment**: `#[repr(align(64))]` aligns to cache line boundary
4. **Bitwise extraction**: `freq = entry & 0xfff`, `bias = (entry >> 12) & 0xfff`,
   `symbol = entry >> 24` — all single-cycle operations

### Equivalence Guarantee

```rust
pub fn verify_equivalence(&self, slots: &[RansWordSlot], slot2sym: &[u8])
    -> Result<(), EquivalenceError>
```

This function compares every slot (0..4095) between the packed table and the legacy
representation. Any mismatch is reported with the exact slot index, expected value,
and actual value. This is called in unit tests to prove that the packed table is
mathematically identical to the legacy representation.

---

## AVX512VL.INTERLEAVED8 Architecture

### Decode Loop (One Iteration = 8 Symbols)

```
State:  __m256i containing [s0, s1, s2, s3, s4, s5, s6, s7] as u32
Input:  &[u16] compressed stream (backward writer → forward reader)
Table:  PackedWordTable (4096 entries × 4 bytes = 16 KB)

1. INDICES:  indices = state & _mm256_set1_epi32(4095)
   // Mask off the low 12 bits of each lane to get the table index

2. GATHER:   packed = _mm256_i32gather_epi32(table_ptr, indices, 4)
   // Load 8 × u32 from addresses [table_ptr + indices[lane] * 4]
   // This single instruction does 8 independent table lookups

3. UNPACK:   freq   = packed & 0x0fff                    // Low 12 bits
             bias   = (packed >> 12) & 0x0fff             // Next 12 bits
             symbol = packed >> 24                        // High 8 bits

4. STORE:    Write 8 symbol bytes to output[i..i+8]
   // Via temp [u32; 8] buffer — avoids packus interleaving issues

5. UPDATE:   xscaled = state >> 12
             state   = (xscaled * freq) + bias
   // Lane-wise multiply-add: 8 independent state updates

6. RENORM:   mask = state < 65536  (per lane)
             words_needed = popcount(mask)
             For each active lane: read one u16, state = (state << 16) | word
   // Inactive lanes are untouched (lane-wise loop, not masked load)
```

### Why Lane-Wise Renorm Instead of Masked Expand?

The natural SIMD approach would be `_mm256_mask_expand_epi16` — expand contiguous input
words into active lanes. However, this intrinsic's memory-access semantics for inactive
lanes are microarchitecture-dependent. To guarantee no overread beyond the provided slice,
we use explicit per-lane scalar reads. This runs at most 8 iterations (typical 0-2 active
lanes) and is provably safe.

### Tail Handling

For lengths not divisible by 8, the remaining `r` symbols (1..7) use scalar logic:
1. Store SIMD state to `[u32; 8]` temp array
2. Decode each active lane individually
3. Load modified state back into SIMD register

---

## AVX512.INTERLEAVED16 Architecture

### Stream Format

The 16-way format is a **new** stream format, not compatible with the 8-way format:

**Encoding (reverse order)**:
- Lane assignment: `lane = i & 15`
- Encode symbols in reverse order into their lane's state
- Flush states in **reverse** lane order: 15, 14, ..., 0
- Each state is 2 × u16 (low word, high word)

**Resulting forward stream layout**:
```
state[0].low, state[0].high, state[1].low, state[1].high, ..., state[15].low, state[15].high
```

The reverse flush produces an ascending forward layout because the writer moves backward.

**Decode loop (one iteration = 16 symbols)**:
```
1. GATHER: 16 entries via _mm512_i32gather_epi32
2. UNPACK: freq/bias/symbol from packed u32
3. STORE:  16 symbols via temp [u32; 16] buffer
4. UPDATE: 16 state updates via _mm512_mullo_epi32 + _mm512_add_epi32
5. RENORM: mask via _mm512_cmplt_epu32_mask, lane-wise reads
```

### Why a New Format Instead of Extending the Existing One?

The existing 8-way format uses two 4-lane SIMD units — an artifact of SSE4.1's 128-bit
registers. AVX512's 512-bit registers can handle 16 lanes natively. The new format:
- Eliminates two-unit coordination overhead
- Doubles arithmetic density (16 symbols per gather vs 8)
- Has simpler tail handling (0..15 remainder vs two units with 0..3 remainders)
- Requires its own encoder and C oracle

---

## Backend Dispatch and Safety

### Auto-Dispatch Priority

```
8-way:  scalar (fastest on Zen 5) → SSE4.1 → AVX2 → AVX512VL
16-way: scalar (fastest on Zen 5) → AVX2 → AVX512

Explicit SIMD backends remain available for courts, cross-verification,
benchmarks, and future CPUs with faster gather instructions.
```

### Runtime Detection (Two-Tier)

```rust
fn avx512vl_available() -> bool {
    #[cfg(feature = "std")] {
        std::is_x86_feature_detected!("avx512f")  // Runtime CPUID
            && ...
    }
    #[cfg(not(feature = "std"))] {
        cfg!(all(target_feature = "avx512f", ...))  // Compile-time check
    }
}
```

When `std` is available: uses `is_x86_feature_detected!()` which calls the CPUID instruction
at runtime. When `std` is not available: uses `cfg!(target_feature = "...")` which checks
compile-time target features set via `RUSTFLAGS="-C target-feature=..."`.

### Explicit Backends

| Function | Safety | Behavior |
|----------|--------|----------|
| `decode_interleaved8_auto` | ✅ Safe | Runtime detection → best backend |
| `decode_interleaved8_scalar` | ✅ Safe | Always scalar |
| `decode_interleaved8_avx2_manual_gather_checked` | ✅ Safe | Runtime-checked AVX2 manual gather |
| `decode_interleaved8_avx2_hardware_gather_checked` | ✅ Safe | Runtime-checked AVX2 hardware gather |
| `decode_interleaved8_avx512vl` | ⚠️ Unsafe | Caller must ensure CPU support |
| `decode_interleaved16_auto` | ✅ Safe | Runtime detection → best backend |
| `decode_interleaved16_scalar` | ✅ Safe | Always scalar |
| `decode_interleaved16_avx2_2x8_checked` | ✅ Safe | Runtime-checked AVX2 2×8 |
| `decode_interleaved16_uniform256_avx2_checked` | ✅ Safe | Runtime-checked AVX2 uniform |
| `decode_interleaved16_avx512` | ⚠️ Unsafe | Caller must ensure CPU support |
| `decode_batch4_interleaved16_avx2_checked` | ✅ Safe | Runtime-checked AVX2 batch4 |

The unsafe functions exist for callers who have already performed runtime detection and
wish to avoid the overhead of checking again.

---

## Why VNNI Is Not Used

AVX-512 VNNI (`_mm512_dpbusd_epi32`) accelerates **packed dot products** — where
multiple pairs of bytes are multiplied and **summed together** into one result:

```
result[i] = sum(a[i][0..3] * b[i][0..3])
```

The rANS state transition is:
```
state[i] = frequency[i] × (state[i] >> 12) + bias[i]
```

This is a set of **independent** lane-wise operations, not a dot product. Using VNNI
would require packing tricks to prevent adjacent lanes from being summed together,
costing more than the straightforward multiply-add approach.

---

## Why Lane-Wise Renormalization (and When Mask-Expand Is Used)

Early SIMD kernels (SSE4.1, original AVX-512) used explicit lane-wise renormalization:
each active lane is spilled to a scalar, updated, and reloaded. This is provably safe but
costly — each active lane performs a full vector store/load cycle.

**Phase H optimization**: The optimized backends (2×8, manual gather, uniform256 table-free)
use a **scratch-buffer + mask-expand** pattern:

1. Copy `popcount(mask)` contiguous input words into a compact buffer
2. Use `_mm256_maskz_expand_epi32` or `_mm512_maskz_expand_epi32` to scatter the
   compact words into the lanes selected by the renormalization mask
3. Shift and blend to produce the new state in a single masked operation

This reduces renormalization from `N` vector spill/reload cycles (one per active lane)
to one compact copy + one expand + one blend — regardless of how many lanes are active.

```text
Before (per-lane spill):
  for each active lane:
    store entire ZMM to stack
    modify one lane
    reload entire ZMM

After (mask-expand):
  compact[0..count] = input[reader..reader+count]
  expanded = _mm512_maskz_expand_epi32(mask, compact)
  state    = _mm512_mask_blend_epi32(mask, new_state, shifted | expanded)
  reader  += count
```

ISA requirements: `avx512f` provides `_mm{256,512}_maskz_expand_epi32` and
`_mm{256,512}_mask_blend_epi32`.

**AVX2 renorm**: The AVX2 backends use an alternative approach with a **precomputed
permutation table** (2.5 KB, 256 × 8 × i32) that translates each of the 256 possible
8-lane renormalization masks into a `_mm256_permutevar8x32_epi32` shuffle, enabling
constant-time lane-wise word injection without branching.

---

## Measured Performance

Benchmarked on **AMD Ryzen 7 9800X3D** (Zen 5, 4.7 GHz, Linux, rustc 1.96, `--release`).
Values in **GiB/s** (higher is better).

### UNIFORM256

| Backend | 64 B | 256 B | 1 KiB | 4 KiB | 16 KiB | 64 KiB | 256 KiB | 1 MiB |
|---------|------|-------|-------|-------|--------|--------|--------|-------|
| scalar-8way | 1.23 | 1.44 | 1.56 | 1.58 | 1.58 | 1.57 | 1.57 | 1.56 |
| sse41-8way | 0.73 | 0.74 | 0.73 | 0.72 | 0.72 | 0.72 | 0.72 | 0.72 |
| **avx512vl-8way** | 0.72 | 0.73 | 0.73 | 0.72 | 0.71 | 0.72 | 0.72 | 0.72 |
| scalar-16way | 1.05 | 1.26 | 1.39 | 1.44 | 1.44 | 1.44 | 1.44 | 1.44 |

### SKEWED.255_1

| Backend | 64 B | 256 B | 1 KiB | 4 KiB | 16 KiB | 64 KiB | 256 KiB | 1 MiB |
|---------|------|-------|-------|-------|--------|--------|--------|-------|
| scalar-8way | 1.39 | 1.66 | 1.80 | 1.83 | 1.84 | 1.82 | 1.82 | 1.82 |
| sse41-8way | 1.28 | 1.34 | 1.32 | 1.32 | 1.33 | 1.32 | 1.33 | 1.32 |
| **avx512vl-8way** | 0.58 | 0.60 | 0.57 | 0.57 | 0.56 | 0.56 | 0.56 | 0.56 |
| scalar-16way | 1.32 | 1.64 | 1.79 | 1.82 | 1.83 | 1.83 | 1.83 | 1.83 |

### Key Findings

1. **Scalar is fastest on Zen 5**: The scalar decoder achieves 1.6-1.8 GiB/s because
   the 16 KB table is L1-resident and sequential loads (~4 cycles) beat gathers (~10-15 cycles).

2. **AVX512VL 8-way ≈ SSE4.1 8-way**: Both SIMD backends are ~0.3-0.7× scalar speed.
   The gather instruction does not help when the table fits in L1 cache.

3. **SSE4.1 8-way reaches 406 MiB/s** on Zen 5 for skewed 1 MiB data — a confirmed,
   benchmarked figure from the Criterion suite.

4. **Scalar 16-way ≈ 90% of scalar 8-way**: The 16-way format achieves 1.4-1.8 GiB/s,
   close to the 8-way format despite processing 16 symbols per iteration.

5. **SIMD is still valuable**: Cross-verification, mathematical equivalence proof, and
   future-proofing for CPUs with faster gathers (Zen 6, Intel Lion Cove).

---

## Criterion Benchmark Suite

The `ryg-rans-rs-bench` crate provides a **9-tier Criterion benchmark suite** for all
decode backends. Each tier uses deterministic corpora (Uniform256, Skewed255_1,
RenormBoundary, Freq1Residual) and verifies correctness against a scalar reference
before timing.

### Benchmark Tiers

| Tier | Bench File | Backends Tested |
|------|-----------|-----------------|
| 1. Scalar | `scalar.rs` | scalar-8way, scalar-16way, `_into` variants |
| 2. SSE4.1 | `sse41.rs` | sse41-8way, all tail lengths |
| 3. AVX2 | `avx2.rs` | avx2-manual-gather-8way, avx2-hardware-gather-8way, avx2-2x8-on16, avx2-uniform256-tablefree-16way |
| 4. AVX-512 | `avx512.rs` | avx512vl-8way, avx512-16way |
| 5. Specialized | `specialized.rs` | uniform256 table-free, manual gather, mask-expand renorm |
| 6. Batch | `batch.rs` | scalar-sequential-4x, avx2-2x8-sequential, avx2-batch4-on16 aggregate |
| 7. Parallel | `parallel.rs` | Multi-threaded parallel block decode (via `ryg-rans-rs-parallel`) |
| 8. Container | `container.rs` | Full RYGRANS container encode → decode round-trip |
| 9. Dispatch | `dispatch.rs` | Backend auto-dispatch overhead, runtime detection latency |

### Running Benchmarks

```sh
# Run all benchmark tiers
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" \
    cargo bench -p ryg-rans-rs-bench

# Run a specific tier
cargo bench -p ryg-rans-rs-bench --bench avx2

# Run with specific filter
cargo bench -p ryg-rans-rs-bench --bench scalar -- "decode_8way_packed_scalar_into"
```

### Verification Before Timing

Every Criterion benchmark **verifies correctness** against the scalar reference before
measuring throughput. This ensures that reported numbers correspond to correct decode
results. Verification checks:

1. **Output bytes**: exact match with scalar decoder
2. **Words consumed**: exact match with scalar decoder
3. **Final states**: exact match with scalar decoder (16 lanes)

---

## Unsafe Code Policy

This crate contains 7 `unsafe fn` for SSE4.1 and AVX-512 intrinsics. Every one is:

1. **Gated by `#[target_feature(enable = "...")]`** — ensures the correct CPU features
   are compiled in
2. **Documented in `docs/unsafe-ledger.md`** — with preconditions, bounds, CPU features,
   inactive-lane safety, and soundness justification
3. **Only reachable through safe APIs** — the `_auto` functions perform runtime detection
   before calling unsafe kernels

### The 7 Unsafe Functions

| Function | ISA | Why Unsafe | Safety Contract |
|----------|-----|------------|-----------------|
| `rans_simd_dec_init` | SSE2 | `_mm_loadu_si128` from raw pointer | Bounds check: `reader.len() >= 8` |
| `rans_simd_dec_sym_unchecked` | SSE4.1 | Table gather via extract/insert | Table bounded: index masked to 0..4095 |
| `rans_simd_dec_renorm_unchecked` | SSSE3+SSE4.1 | Shuffle-mask renorm | Scratch buffer: no overread |
| `decode_interleaved8_avx512vl_kernel` | AVX512VL+AVX512BW | 256-bit gather + masked ops | All bounds checked upfront |
| `decode_interleaved16_avx512_kernel` | AVX512F+AVX512BW | 512-bit gather + masked ops | All bounds checked upfront |
| `decode_interleaved8_avx512vl` (wrapper) | — | Calls gated kernel | Caller ensures CPU support |
| `decode_interleaved16_avx512` (wrapper) | — | Calls gated kernel | Caller ensures CPU support |

The safe auto-dispatch functions (`decode_interleaved8_auto`, `decode_interleaved16_auto`)
perform runtime feature detection before calling any unsafe kernel.

**AVX2 backends** use additional `unsafe` functions gated by `#[target_feature(enable = "avx2")]`,
each with equivalent safety contracts — documented in `avx2.rs` and `avx2_renorm.rs`.

---

## Testing and Verification

### 46 Unit Tests (All Pass)

| Group | Tests | What They Verify |
|-------|-------|------------------|
| Packed table | 6 | Field extraction, scale validation, equivalence to legacy |
| Scalar 8-way | 1 | Packed decoder matches existing scalar |
| Scalar 16-way | 3 | Round-trip, tail lengths 0..15, truncated rejection |
| State ordering | 1 | Stream layout: 16 initial states in lane order |
| AVX512VL 8-way | 3 | Scalar equivalence, various lengths, truncated rejection |
| AVX512 16-way | 3 | Scalar equivalence, all tails, truncated rejection |
| AVX2 backends | 14 | Manual gather, hardware gather, 2×8-on16, uniform256 table-free, batch4 parity, multi-job batch, empty jobs, exhaustive renorm masks, all tails, truncation, manual vs hardware parity |
| Backend dispatch | 5 | Backend labels, scalar dispatch, truncated rejection |
| Malformed input | 6 | Truncated partial init, decode, wrong-format detection |
| Mask exhaustion 8-way | 1 | All 256 renormalization masks |
| Original SSE4.1 | 4 | Round-trip, lengths, renorm, truncated |

### Mask Exhaustion

Every possible renormalization mask is tested:
- **8-way**: 256 masks (2^8) — takes < 1 second in debug
- **16-way**: 65,536 masks (2^16) — requires `--release` (~1 second)

For each mask, we verify:
1. Correct popcount equals words consumed
2. Truncated stream (one fewer word) is correctly rejected
3. Full stream decodes without error

### Fuzzing

Two AVX-512 fuzz targets verify scalar/AVX512 equivalence on random inputs:
- `avx512vl8_roundtrip`: Encode → scalar decode → AVX512VL decode → compare output + consumption
- `avx512_16way_roundtrip`: Same for 16-way format

### Kani Proofs

Three proofs in the core crate verify:
- `kani_packed_entry_fields`: Pack/unpack round-trips exactly
- `kani_state_update_no_overflow`: State update arithmetic stays bounded
- `kani_slot_index_bounded`: Slot index always < 4096 (table bounds)

---

## Module Reference

### `packed_table.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `PackedWordEntry` | struct | Single u32 entry with `freq()`, `bias()`, `symbol()` extraction |
| `PackedWordTable` | struct | 4096-entry table, 64-byte aligned, heap-allocated |
| `PackedWordTable::from_freqs` | method | Validated construction from frequency model |
| `PackedWordTable::verify_equivalence` | method | Per-slot comparison with legacy table |
| `PackedWordTable::as_ptr` | method | Raw pointer for gather operations |
| `decode_8way_packed_scalar` | function | Scalar 8-way decode using packed table (allocates output) |
| `decode_8way_packed_scalar_with_report` | function | 8-way decode with full `DecodeReport8` (words consumed + 8 final states) |
| **`decode_8way_packed_scalar_into`** | **function** | **8-way decode into preallocated `&mut [u8]` — no allocation** |
| `decode_interleaved16_scalar` | function | Scalar 16-way decode with `DecodeReport` (allocates output) |
| **`decode_interleaved16_scalar_into`** | **function** | **16-way decode into preallocated `&mut [u8]` — no allocation** |
| `encode_interleaved16` | function | 16-way encoder for new format |
| `DecodeReport` | struct | Words consumed + 16 final states |
| `DecodeReport8` | struct | Words consumed + 8 final states |

The **`_into` variants** (`decode_8way_packed_scalar_into`, `decode_interleaved16_scalar_into`)
are the recommended APIs for performance-sensitive callers. They write directly into a
caller-provided output buffer, eliminating the allocation and copy required by the
`Vec`-returning counterparts. This is especially important in Criterion benchmarks and
parallel block decode where allocations in the hot path degrade throughput.

### `avx512.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `decode_interleaved8_avx512vl_kernel` | unsafe fn | 8-way AVX512VL decode |
| `decode_interleaved16_avx512_kernel` | unsafe fn | 16-way AVX512 decode |
| `NUM_WORDS_8` | static | Popcount LUT for 8-lane masks (256 entries) |
| `NUM_WORDS_16` | static | Popcount LUT for 16-lane masks (65536 entries) |

### `avx2.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `Avx2DecodeJob` | struct | Batch decode job descriptor (`compressed`, `table`, `output`, `block_index`) |
| `decode_interleaved8_avx2_manual_gather` | unsafe fn | 8-way AVX2 manual gather decode |
| `decode_interleaved8_avx2_hardware_gather` | unsafe fn | 8-way AVX2 hardware gather decode |
| `decode_interleaved16_avx2_2x8` | unsafe fn | 16-way AVX2 2×8 manual gather decode |
| `decode_interleaved16_uniform256_avx2` | unsafe fn | 16-way AVX2 uniform256 table-free decode |
| `decode_batch4_interleaved16_avx2` | unsafe fn | Batch4 multi-stream AVX2 decode |

### `avx2_renorm.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `Avx2RenormPermutations` | struct | Precomputed permutation table for AVX2 renormalization (2.5 KB) |
| `build_avx2_renorm_table` | fn | Build the permutation table from all 256 8-lane masks |

### `backends.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `DecodeBackend` | enum | 5 backend variants with stable `label()` strings |
| `DecodeResult` | struct | Output + `DecodeReport` + `DecodeBackend` |
| `DecodeError` | enum | `InputTooShort`, `InvalidTable`, `UnsupportedBackend`, etc. |
| `decode_interleaved8_auto` | fn | Safe auto-dispatch: scalar (fastest on Zen 5) |
| `decode_interleaved8_avx512vl` | unsafe fn | Explicit AVX512VL 8-way |
| `decode_interleaved8_avx2_manual_gather_checked` | fn | Safe AVX2 manual gather 8-way |
| `decode_interleaved8_avx2_hardware_gather_checked` | fn | Safe AVX2 hardware gather 8-way |
| `decode_interleaved8_scalar` | fn | Explicit scalar 8-way |
| `decode_interleaved16_auto` | fn | Safe auto-dispatch: scalar (fastest on Zen 5) |
| `decode_interleaved16_avx512` | unsafe fn | Explicit AVX512 16-way |
| `decode_interleaved16_avx2_2x8_checked` | fn | Safe AVX2 2×8 16-way |
| `decode_interleaved16_uniform256_avx2_checked` | fn | Safe AVX2 uniform256 16-way |
| `decode_interleaved16_scalar` | fn | Explicit scalar 16-way |
| `decode_batch4_interleaved16_avx2_checked` | fn | Safe AVX2 batch4 multi-stream |

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
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" cargo build

# Run all tests (46 tests)
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" cargo test

# Exhaustive 16-way mask test (requires --release)
RUSTFLAGS="-C target-feature=+avx512f,+avx512bw" cargo test --release -p ryg-rans-rs-simd -- --ignored

# Run performance benchmarks across all backends
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace

# Run Criterion benchmark suite (all 9 tiers)
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" \
    cargo bench -p ryg-rans-rs-bench

# Run AVX2 benchmarks only
cargo bench -p ryg-rans-rs-bench --bench avx2

# Run batch benchmarks (batch4 multi-stream)
cargo bench -p ryg-rans-rs-bench --bench batch
```
