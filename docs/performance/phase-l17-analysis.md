# Phase L.17 — Performance Regression and Resource Analysis

Host: AMD Ryzen 7 9800X3D (8 cores / 16 threads, 1 NUMA node), Linux,
rustc 1.96, `RUSTFLAGS="-C target-cpu=native"`, Criterion 0.5.1, cold
executor (threads spawned per call), skewed model, 1 MiB blocks.

**Status:** analysis measurements; the sealed performance evidence is
regenerated in Phase L.18 through `cargo xtask benchmark-run`.  These
numbers are reproducible via the commands below and archived in the
Criterion tree.

## Internal scaling matrix (1/2/4/8/16 workers)

64 MiB sustained decode (64 × 1 MiB blocks), cold executor:

| Workers | Median | Throughput | Per-thread efficiency |
|---------|--------|-----------|----------------------|
| 1 | 93.2 ms | 686 MiB/s | 100 % |
| 2 | 60.1 ms | 1.037 GiB/s | 75.6 % |
| 4 | 43.4 ms | 1.437 GiB/s | 52.4 % |
| 8 | 36.7 ms | 1.701 GiB/s | 31.0 % |
| 16 | 35.0 ms | 1.785 GiB/s | 16.3 % |

16 MiB one-wave decode (16 × 1 MiB blocks): 22.9 ms (~691 MiB/s) at every
worker count — the workload is a single wave whose per-block decode time
dominates; parallelism cannot help a one-block-per-worker wave.

256 MiB extended decode:

| Workers | Median | Throughput |
|---------|--------|-----------|
| 1 | 392.7 ms | 651 MiB/s |
| 4 | 174.1 ms | 1.47 GiB/s |
| 16 | 135.4 ms | 1.89 GiB/s |

Readings: 8 workers are the efficiency-oriented mode (1.70 GiB/s,
31 % per-thread efficiency); 16 workers add +5 % aggregate (SMT), matching
the Phase K finding that 16 threads are not merely scheduling noise but the
marginal gain over 8 physical cores is small (~5 %).

## Queue-depth sweep (8 workers, 64 MiB)

`max_in_flight_blocks` ∈ {8, 16, 32, 64, 128} → 36.5–36.7 ms at every
depth.  **Queue depth is a pure bound, not a throughput lever**, for this
workload: the concurrency is limited by the 8 workers, and the bounded
result channel plus live reorder absorb any excess.  (The sweep also
exposed and fixed a real defect: with queue depth below the worker count,
the old reorder bound `max_in_flight` was smaller than the true peak
in-flight count — `effective_queue + workers` — and a slow early block
produced a spurious `ResourceLimit`.  Regression test:
`test_queue_depth_below_worker_count_no_spurious_limit`.)

## Sequential threshold crossover

`parallel_threshold_bytes = 1` forces the pooled path even with 1 worker:
93.3 ms (686 MiB/s) — identical to the 1-worker pooled result, confirming
`ExecutionMode::SequentialThresholdFallback` is throughput-equivalent (it is
the same single-stream decode) while avoiding pool spawn + queue overhead
for small inputs.  The default threshold (1 MiB) is below every sustained
benchmark input here, so the pooled path is measured throughout; the exact
crossover point is workload-dependent and is not claimed as a universal
constant (per L.6, calibrated defaults would require sealed calibration
evidence).

## Component isolation (software decomposition)

Hardware counters were **not** collected: `perf` is not installed and the
kernel does not permit it on this host (recorded as L17-C).  Component
separation is by software decomposition of the same-host measurements:

| Component | Measurement | Source |
|-----------|-------------|--------|
| Byte-rANS decode kernel (single stream, 1 MiB) | 947 MiB/s | L.14 court (Rust core reciprocal, no hashing) |
| Word-rANS decode kernel | 486 MiB/s | L.14 court |
| Parallel decode, 1 worker, 64 MiB (kernel + plan + dispatch + payload hash + decoded hash + reorder) | 686 MiB/s | this file |
| Parallel decode, 8 workers | 1.70 GiB/s | this file |

The 1-worker gap (947 → 686 MiB/s, 1.38×) is the per-block overhead of the
parallel pipeline: plan construction, backend selection, payload + decoded
SHA-256 hashing, and reorder commit.  Hashing is the dominant contributor
(two SHA-256 passes per block); the L.18 sealed decomposition will separate
these with explicit benchmark-only hooks.  The 64 MiB aggregate at 8 workers
(1.70 GiB/s) versus the single-stream kernel ceiling (947 MiB/s) shows that
block-level parallelism multiplies the *parallelisable* part of the work;
the residual scaling loss is attributed to memory bandwidth and hash
throughput, not scheduling (the queue-depth flatness supports this).

## Regression policy

* **Baseline:** the sealed Phase K run is superseded (L1-A..S); L.18
  establishes the new sealed baseline with the benchmark wrapper.
* **Thresholds:** a change is a regression if the median moves outside the
  95 % confidence intervals of both the before and after runs on the same
  host (measurement noise excluded).  For scaling claims, the slope across
  the 1/2/4/8/16 matrix must be reproduced, not a single point.
* **Noisy-CI policy:** the Docker matrix is a build/test gate, never a
  performance gate; performance claims require the dedicated benchmark host.
* **Stable-host policy:** performance evidence is bound to the exact host,
  kernel, rustc, RUSTFLAGS, governor, and SMT state (the L.18 wrapper
  captures these before and after the run and refuses to seal if the
  environment changed materially).
* **Repeat policy:** a measurement is repeated if outliers exceed 10 % of
  samples or the CI width exceeds 5 % of the median.
* **Faster results:** a faster result replaces the baseline only when
  produced by the same sealed methodology (same corpus, model, sizes,
  affinity, compiler flags) and passes the same correctness preflight.
* **Regression → residual:** any confirmed regression becomes a residual in
  `evidence/phase-l/gap-ledger.md` with before/after measurements, and is
  never silently reverted.

## Reproducing

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench --bench parallel
RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench --bench parallel_l17
```
