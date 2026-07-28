# Performance Measurement Methodology

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity first, performance second. Correctness is verified by the oracle courts before any performance measurement is meaningful.

---

## Overview

Performance is measured to ensure the Rust implementation is competitive with the upstream C/C++ reference. However, performance parity is **never claimed** without first establishing bitstream parity. Measuring a wrong implementation is a waste of time.

All measurements are taken in a controlled environment to minimize noise and maximize reproducibility.

---

## Measurement Environment

### Same Container

All performance comparisons between Rust and C/C++ are run in the **same Docker container** or bare-metal environment, using the same kernel, same CPU governor, and same thermal conditions. This ensures:

- Identical CPU microarchitecture and clock speeds.
- Identical memory hierarchy and NUMA topology.
- Identical kernel scheduler and ASLR behavior.
- Identical library versions (glibc, libstdc++).

### Pinned CPU

To reduce measurement noise:

1. CPU frequency scaling is disabled (`performance` governor).
2. Turbo Boost / Intel Turbo Boost Technology is disabled (or documented if not).
3. The benchmark process is pinned to a dedicated physical core using `taskset` or `numactl`.
4. Hyperthreading siblings are excluded (logical cores sharing a physical core).
5. The benchmark is run with `nice -n -20` for maximum priority.
6. ASLR is disabled for the benchmark process (`setarch x86_64 -R`).

### Warmed Binaries

Each benchmark executable is:

1. Compiled with the same optimization level (`-O2` for C/C++, `--release` for Rust).
2. Run once as a warm-up pass (results discarded).
3. Then run multiple measurement iterations (typically 5–10).
4. The reported result is the **median** of the measurement iterations, not the mean (to reject outliers from kernel jitter).

---

## Metrics

### Cycles Per Symbol

The primary metric for algorithmic efficiency. Measured using:

- **C/C++**: `rdtsc` (via `platform.h` from upstream, or `__rdtsc()` intrinsic).
- **Rust**: `std::arch::x86_64::_rdtsc()`.

The measurement spans a tight loop of N encoding or decoding operations, and the total cycle count is divided by N. This gives a symbol-level cost that is independent of input size.

Reported as: `cycles/symbol` (lower is better).

### MiB/s (Throughput)

The secondary metric for real-world performance. Measures the rate at which input data is processed.

```
throughput (MiB/s) = (input_size_bytes / elapsed_seconds) / (1024 * 1024)
```

Elapsed seconds are measured with `CLOCK_MONOTONIC` (C/C++) or `std::time::Instant` (Rust). The measurement includes all overhead: symbol table lookups, renormalization, I/O.

### Compiler Flags

Every performance report must record the exact compiler flags used:

| Toolchain | Flags |
|---|---|
| GCC/Clang (C/C++) | `-O2 -march=x86-64-v2 -DNDEBUG` (baseline) |
| GCC/Clang (C/C++) | `-O2 -march=native -DNDEBUG` (native) |
| Rustc (release) | `--release` with default `-C target-cpu=native` |
| Rustc (release) | `--release` with explicit `-C target-cpu=x86-64-v2` |

Additional flags (LTO, codegen-units, etc.) must be documented.

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

- **Not-yet-optimized paths**: Scaffold surfaces or surfaces that have not received optimization attention are exempt from the 5% threshold. A baseline measurement is still taken.
- **Scalar fallback paths**: If the Rust implementation uses a different algorithmic strategy (e.g., pure division instead of reciprocal), no performance parity is expected.
- **I/O-heavy operations**: Operations bounded by memory bandwidth (e.g., large renormalization loops on cold cache) may show higher variance.

---

## No Integer Divide in Reciprocal Loops

The upstream `ryg_rans` uses a reciprocal-based fast encoding path that replaces integer division with multiplication and shifts. In the Rust implementation:

- The reciprocal fast path **must not** contain any integer divide instructions (`div`/`idiv`) in the hot loop.
- The reciprocal approximation must match upstream exactly: same `rcp_freq`, `rcp_shift`, `bias`, `cmpl_freq` values.
- Verification: the reciprocal → division equivalence test (`test_reciprocal_equals_division`) proves that the fast path produces the same state transitions as the division-based reference path on every input.

This constraint exists because:
1. Integer division is expensive (10–30x more latency than multiplication on modern x86_64).
2. The entire point of the reciprocal method is to avoid division.
3. A Rust reciprocal path that falls back to division would miss the performance target by definition.

---

## Measurement Protocol

```
1. Environment check
   ├─ CPU governor = performance
   ├─ Turbo Boost = disabled (document if not)
   ├─ Cores pinned (taskset -c <core>)
   └─ ASLR disabled (setarch x86_64 -R)

2. Build
   ├─ C/C++: make -C oracle/adapter/c-build release
   └─ Rust: cargo build --release

3. Warm-up (discarded)
   ├─ Run each benchmark once
   └─ Validate output is correct (cross-check with oracle)

4. Measurement (5 iterations minimum)
   ├─ Record cycles/symbol per iteration
   ├─ Record MiB/s per iteration
   └─ Compute median and IQR

5. Report
   ├─ Document compiler flags and environment
   ├─ Table of results (rust vs C, median per metric)
   └─ Any performance residuals with severity S2
```

---

## Performance Residuals

If the Rust implementation fails the 5% threshold, a **performance residual** is created:

- **Severity**: `S2` (minor — does not affect correctness).
- **Status**: `investigating` by default.
- **Resolution**: Either optimize to meet the threshold, or document as `wontfix` with justification (e.g., "Rust bounds checking adds 3% overhead, acceptable trade-off").

Performance residuals are tracked alongside correctness residuals in the same residual ledger, distinguished by their `class` and `court_id`.
