# Performance Measurement Methodology

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity first, performance second. Correctness is verified by the oracle courts before any performance measurement is meaningful.

---

## Overview

Performance is measured to ensure the Rust implementation is competitive with the upstream C/C++ reference. However, performance parity is **never claimed** without first establishing bitstream parity. Measuring a wrong implementation is a waste of time.

All measurements are taken in a controlled environment to minimize noise and maximize reproducibility.

---

## Phase H: Benchmark Implementation

The `perf` binary in `crates/ryg-rans-rs-oracle/src/bin/perf.rs` implements the measurement methodology.

### What is Measured

The benchmark measures **decode throughput** for the word rANS 8-way decoders (scalar 8-way, SSE4.1 SIMD, and C oracle):

| Metric | Unit | How |
|--------|------|-----|
| Throughput | GiB/s | (symbols × 1 byte) / elapsed seconds, normalized to GiB |
| Latency | ns/symbol | Total elapsed nanoseconds / total symbols decoded |
| Speedup | Ratio | SIMD GiB/s / Scalar GiB/s |

### Profiles

Five frequency-model profiles exercise different decoder behaviours:

| Profile | Description | Renorm Stress | Table Stress |
|---------|-------------|---------------|--------------|
| UNIFORM256 | 256 symbols, equal frequencies | Low | Uniform |
| FREQ1_RESIDUAL | 255 symbols, one freq=1 | Medium | Sparse |
| SKEWED.255_1 | 2 symbols: one occupies 255/256 of range | High | Very sparse |
| SPARSE.17 | 17 evenly-distributed symbols | Medium | Medium |
| RENORM.BOUNDARY | 50% in one symbol, rest distributed | High | Dense |

### Sizes

| Size | Bytes | Purpose |
|------|-------|---------|
| 64 | 64 | Tiny: dispatch + tail overhead dominates |
| 256 | 256 | Small: still dominated by init/finalize |
| 1024 | 1 KiB | Transition range |
| 4096 | 4 KiB | Moderate block |
| 16384 | 16 KiB | Typical block |
| 65536 | 64 KiB | Large block |
| 262144 | 256 KiB | Sustained throughput |
| 1048576 | 1 MiB | Large sustained throughput |

### Measurement Protocol

1. **Input generation**: Symbols are generated from the frequency distribution (not uniform random), matching the statistical properties the decoder expects.
2. **Compression**: The 8-way word rANS encoder produces the compressed stream.
3. **Correctness check**: Every profile×size combination is verified for bit-exact decode before measurement.
4. **Warmup**: 5 iterations, discarded.
5. **Measurement**: Multiple iterations (aiming for ~200ms total), timed with `std::time::Instant`.
6. **Reporting**: Median-based throughput in GiB/s and ns/symbol.

### What is NOT in the Timed Loop

- Output buffer allocation (pre-allocated before timing)
- Table construction (done once)
- Input generation (done once)
- Correctness verification (done once before measurement)
- Feature detection (done once before measurement)

### Measurement Environment

```
1. Environment check
   ├─ CPU model recorded from /proc/cpuinfo
   ├─ rustc version recorded
   └─ SIMD availability detected

2. Build
   ├─ C/C++: make -C oracle/adapter (with -msse4.1 for SIMD oracle)
   └─ Rust: cargo build --release
       └─ RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" for SIMD benchmark

3. Warm-up (discarded)
   ├─ Run each benchmark 5 times
   └─ Validate output is correct (cross-check with oracle)

4. Measurement
   ├─ n_iter = max(20, min(500000, 100_000_000 / size))
   └─ Report median GiB/s and ns/symbol

5. Report
   ├─ Table of results (scalar vs SIMD per profile + size)
   ├─ Speedup factor
   └─ CSV summary
```

### Hardware Counter Measurement

For authoritative cycle-level analysis:

```sh
sudo perf stat -r 10 \
  -e cycles,instructions,branches,branch-misses,L1-dcache-loads,L1-dcache-load-misses \
  RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" taskset -c <isolated-core> \
  cargo run --release --bin perf -- oracle/adapter/rans_trace
```

This requires `perf` and a pinned CPU core to minimize measurement noise. Results are informative but not yet part of the seal gate.

---

## Known Results

On the tested architecture (Ryzen 7 9800X3D, Rust 1.96, GCC 14):

| Implementation | Relative Throughput |
|----------------|-------------------|
| Scalar 8-way (Rust) | 1.00× (baseline) |
| SSE4.1 SIMD (Rust) | ~0.41× (2.5× slower) |
| C upstream (oracle, subprocess) | Not comparable (process overhead) |

The SSE4.1 decoder's slower performance is a known architectural limitation:
- Each SIMD decode round extracts lane indices to scalar registers via `_mm_extract_epi32`.
- Table lookups are scalar (plain array indexing).
- Results are reconstructed into vectors via `_mm_insert_epi32`.
- Only the final multiply-add and renormalization benefit from SIMD.

This is not a failure of the Rust implementation — the upstream design uses the same approach.
