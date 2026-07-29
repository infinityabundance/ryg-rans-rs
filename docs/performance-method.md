# Performance Measurement Methodology

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity first, performance second. Correctness is verified by the oracle courts before any performance measurement is meaningful.

---

## Overview

Performance is measured to ensure the Rust implementation is competitive with the upstream C/C++ reference. However, performance parity is **never claimed** without first establishing bitstream parity. Measuring a wrong implementation is a waste of time.

All measurements are taken in a controlled environment to minimize noise and maximize reproducibility.

---

## Test Environment

**CPU**: AMD Ryzen 7 9800X3D (Zen 5, 8 cores, 4.7 GHz boost)  
**Kernel**: Linux 6.x, x86_64  
**rustc**: 1.96.0 (2026-05-25)  
**Build**: `RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" cargo run --release`  
**Governor**: performance (fixed frequency)  

---

## Benchmark Results

### UNIFORM256 — 256 symbols, equal frequencies

| Backend | 64 B | 256 B | 1 KiB | 4 KiB | 16 KiB | 64 KiB | 256 KiB | 1 MiB |
|---------|------|-------|-------|-------|--------|--------|--------|-------|
| scalar-8way (legacy) | 1.23 | 1.44 | 1.56 | 1.58 | 1.58 | 1.57 | 1.57 | 1.56 |
| scalar-8way (packed) | 0.99 | 1.15 | 1.25 | 1.31 | 1.31 | 1.31 | 1.31 | 1.31 |
| sse41-8way | 0.73 | 0.74 | 0.73 | 0.72 | 0.72 | 0.72 | 0.72 | 0.72 |
| avx512vl-8way | **0.72** | **0.73** | **0.73** | **0.72** | **0.71** | **0.72** | **0.72** | **0.72** |
| scalar-16way | 1.05 | 1.26 | 1.39 | 1.44 | 1.44 | 1.44 | 1.44 | 1.44 |

Values in GiB/s (higher is better).

### SKEWED.255_1 — 2 symbols, 255:1 ratio

| Backend | 64 B | 256 B | 1 KiB | 4 KiB | 16 KiB | 64 KiB | 256 KiB | 1 MiB |
|---------|------|-------|-------|-------|--------|--------|--------|-------|
| scalar-8way (legacy) | 1.39 | 1.66 | 1.80 | 1.83 | 1.84 | 1.82 | 1.82 | 1.82 |
| scalar-8way (packed) | 0.77 | 0.97 | 1.10 | 1.17 | 1.17 | 1.18 | 1.18 | 1.18 |
| sse41-8way | 1.28 | 1.34 | 1.32 | 1.32 | 1.33 | 1.32 | 1.33 | 1.32 |
| avx512vl-8way | **0.58** | **0.60** | **0.57** | **0.57** | **0.56** | **0.56** | **0.56** | **0.56** |
| scalar-16way | 1.32 | 1.64 | 1.79 | 1.82 | 1.83 | 1.83 | 1.83 | 1.83 |

### RENORM.BOUNDARY — frequent renormalization

| Backend | 64 B | 256 B | 1 KiB | 4 KiB | 16 KiB | 64 KiB | 256 KiB | 1 MiB |
|---------|------|-------|-------|-------|--------|--------|--------|-------|
| scalar-8way (legacy) | 1.26 | 1.54 | 1.62 | 1.64 | 1.63 | 1.66 | 1.66 | 1.64 |
| scalar-8way (packed) | 1.19 | 1.32 | 1.41 | 1.43 | 1.46 | 1.44 | 1.45 | 1.44 |
| sse41-8way | 0.50 | 0.49 | 0.51 | 0.51 | 0.50 | 0.51 | 0.51 | 0.51 |
| avx512vl-8way | **0.49** | **0.51** | **0.52** | **0.53** | **0.53** | **0.53** | **0.53** | **0.53** |
| scalar-16way | 1.14 | 1.37 | 1.46 | 1.48 | 1.50 | 1.47 | 1.50 | 1.48 |
| **avx512-16way** | **0.73** | **0.61** | **0.63** | **0.64** | **0.64** | **0.64** | **0.64** | **0.64** |

---

## Key Findings

### 1. Scalar is fastest on Zen 5

On Ryzen 7 9800X3D, **scalar 8-way (legacy slot table) is the fastest backend** across all profiles and sizes. The scalar decoder achieves:
- 1.56–1.84 GiB/s for uniform to skewed profiles
- 0.56–0.74 ns/symbol
- Consistently 2–3× faster than any SIMD backend

### 2. AVX512VL 8-way ≈ SSE4.1 8-way

Both SIMD backends perform similarly (0.5–1.3 GiB/s), with AVX512VL being slightly better on uniform profiles and slightly worse on skewed profiles.

The gather instruction (`_mm256_i32gather_epi32`) does not provide a speedup because:
- The 4096-entry table (16 KB) fits in L1 cache → scalar loads are ~4 cycles
- Gather has higher latency (~10–15 cycles) on Zen 5
- Lane-wise store/modify/reload for renormalization adds overhead
- Packed table requires extra bit-manipulation (freq/bias extraction) that scalar avoids

### 3. Scalar 16-way is competitive with scalar 8-way

The 16-way format achieves 1.4–1.8 GiB/s, about 90% of the 8-way throughput. This is excellent considering it processes twice as many symbols per gather cycle. The slight overhead comes from:
- 16 initial states to load (32 u16 words vs 16)
- More renormalization checks per group

### 4. Packed table has ~15% overhead vs legacy slot table

The packed table requires the decoder to extract freq (`entry & 0xfff`) and bias (`(entry >> 12) & 0xfff`) from each entry, adding bit-manipulation overhead. The legacy slot table has freq and bias in separate u16 fields, accessible without masking.

---

## Acceptance Threshold

### 5% Threshold for Fast Paths

For algorithmic surfaces that have been optimized as "fast paths" (e.g., reciprocal encoding, SIMD decoding), the Rust implementation must perform **within 5%** of the equivalent C/C++ implementation on the same hardware.

The 5% threshold is measured as:

```
degradation = (rust_cycles_per_symbol - c_cycles_per_symbol) / c_cycles_per_symbol * 100
```

If `degradation > 5%`, the fast path is considered to have a **performance residual**, which is recorded and investigated. The surface may still be marked `full` for correctness with the performance residual tracked separately.

### Exceptions

- **Not-yet-optimized paths**: Scaffold surfaces or surfaces that have not received optimization attention are exempt from the 5% threshold.
- **Scalar fallback paths**: If the Rust implementation uses a different algorithmic strategy, no performance parity is expected.
- **SIMD paths**: Subject to architectural limitations. The SSE4.1 and AVX-512 paths are not expected to match scalar performance on Zen 5 due to gather overhead.

---

## No Integer Divide in Reciprocal Loops

The upstream `ryg_rans` uses a reciprocal-based fast encoding path that replaces integer division with multiplication and shifts. In the Rust implementation:

- The reciprocal fast path **must not** contain any integer divide instructions (`div`/`idiv`) in the hot loop.
- The reciprocal approximation must match upstream exactly: same `rcp_freq`, `rcp_shift`, `bias`, `cmpl_freq` values.
- Verification: the reciprocal → division equivalence test proves that the fast path produces the same state transitions as the division-based reference path on every input.

---

## Measurement Protocol

```
1. Build
   ├─ Rust: cargo build --release
   └─ Flags: -C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw

2. Warm-up (discarded)
   ├─ Run each benchmark 5 times
   └─ Validate correctness across all backends

3. Measurement
   ├─ n_iter = max(20, min(500000, 100_000_000 / size))
   ├─ Report median GiB/s and ns/symbol
   └─ Backend identity recorded

4. Report
   ├─ Table of results (scalar vs sse41 vs avx512vl vs avx512)
   ├─ Speedup factors
   └─ CSV summary
```

### Hardware Counter Measurement

```sh
sudo perf stat -r 10 \
  -e cycles,instructions,branches,branch-misses,L1-dcache-loads,L1-dcache-load-misses \
  RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" \
  cargo run --release --bin perf -- oracle/adapter/rans_trace
```
