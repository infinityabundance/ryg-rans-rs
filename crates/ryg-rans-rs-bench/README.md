# ryg-rans-rs-bench

> **Criterion benchmark suite for all ryg-rans-rs execution tiers.**  
> 9 benchmark tiers across scalar, SSE4.1, AVX2, AVX-512, batch, parallel, container, and dispatch.  
> Deterministic corpora with 8 model profiles. Verification-before-timing policy.  
> JSON/CSV structured export with full host metadata.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)

**Version: 0.1.27** · `publish = false` (workspace-internal crate)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [The 9 Benchmark Tiers](#the-9-benchmark-tiers)
3. [Deterministic Corpora](#deterministic-corpora)
4. [Verification-Before-Timing Policy](#verification-before-timing-policy)
5. [How to Run Benchmarks](#how-to-run-benchmarks)
6. [Interpreting Results](#interpreting-results)
7. [Structured Export](#structured-export)
8. [Module Reference](#module-reference)
9. [Benchmark Architecture](#benchmark-architecture)
10. [Adding a New Benchmark](#adding-a-new-benchmark)
11. [Feature Flags](#feature-flags)

---

## What This Crate Is

This crate provides the **canonical benchmark suite** for all ryg-rans-rs decode
backends and execution tiers. It uses [Criterion.rs](https://github.com/bheisler/criterion.rs)
for precise throughput measurement with statistical rigor.

Every benchmark:

1. Generates a **deterministic corpus** from a known model profile and seed
2. **Verifies correctness** against the scalar reference decoder before timing
3. Measures **bytes/second** throughput via `criterion::Throughput::Bytes`
4. Collects **median, mean, and standard deviation** across multiple samples
5. Captures **full host metadata** (CPU model, rustc version, target features, git commit)

The crate is marked `publish = false` — it is workspace-internal and not published to
crates.io.

---

## The 9 Benchmark Tiers

### Tier 1: Scalar (`benches/scalar.rs`)

The baseline — all scalar decode backends:

- `decode_8way_packed_scalar` — 8-way scalar decode (allocates output)
- `decode_8way_packed_scalar_into` — 8-way scalar decode into preallocated buffer
- `decode_interleaved16_scalar` — 16-way scalar decode (allocates output)
- `decode_interleaved16_scalar_into` — 16-way scalar decode into preallocated buffer

Measured at: 64 B, 256 B, 1 KiB, 4 KiB, 16 KiB, 64 KiB, 256 KiB, 1 MiB.

### Tier 2: SSE4.1 (`benches/sse41.rs`)

Legacy SIMD backend:

- `sse41-8way` — SSE4.1-accelerated 8-way decode
- All tail lengths (1..7)

### Tier 3: AVX2 (`benches/avx2.rs`)

Phase J AVX2 portability tier:

- `avx2-manual-gather-8way` — 8-way AVX2 manual gather
- `avx2-hardware-gather-8way` — 8-way AVX2 hardware gather
- `avx2-2x8-on16` — 16-way AVX2 2×8 manual gather
- `avx2-uniform256-tablefree-16way` — 16-way AVX2 uniform256 table-free

### Tier 4: AVX-512 (`benches/avx512.rs`)

Full-width SIMD backends:

- `avx512vl-8way` — AVX512VL 8-way decode
- `avx512-16way` — AVX512 16-way decode

### Tier 5: Specialized (`benches/specialized.rs`)

Optimized backend variants:

- `uniform256-tablefree` — Table-free uniform kernel
- `manual-gather` — Scalar-load manual gather
- `mask-expand-renorm` — Mask-expand renormalization optimization

### Tier 6: Batch (`benches/batch.rs`)

Multi-stream batch decoders:

- `scalar-sequential-4x` — Scalar: 4 streams sequentially
- `avx2-2x8-sequential` — AVX2: 2×8-on16 sequentially
- `avx2-batch4-on16` — AVX2: batch4 multi-stream aggregate

### Tier 7: Parallel (`benches/parallel.rs`)

Multi-threaded parallel block engine (via `ryg-rans-rs-parallel`):

- 1, 2, 4, 6, 8, 12, 16 threads
- Scalability matrix: encodes and decodes with varying block sizes
- Scaling efficiency (speedup relative to 1 thread)

### Tier 8: Container (`benches/container.rs`)

End-to-end container round-trip:

- Full RYGRANS v1 container encode → decode cycle
- 4 threads, 1 MiB blocks, 16 MiB corpus
- Includes SHA-256 hashing overhead

### Tier 9: Dispatch (`benches/dispatch.rs`)

Backend dispatch overhead:

- Auto-dispatch latency (runtime `is_x86_feature_detected!` calls)
- Backend selection overhead
- `_checked` vs direct `unsafe` dispatch comparison

---

## Deterministic Corpora

Every benchmark uses a `Corpus` generated from a known `ModelProfile` and seed. The
same profile + seed always produces identical bytes, frequencies, and compressed streams.

### Model Profiles

| Profile | Description | Entropy |
|---------|-------------|---------|
| `UNIFORM256` | Exactly 16 occurrences of each of 256 symbols | 8.00 bits/sym |
| `SKEWED_255_1` | Symbol 0 appears 255× more often than any other | ~1.03 bits/sym |
| `FREQ1_RESIDUAL` | Symbol 0 dominates; 0.1% of symbols are random residuals | ~0.02 bits/sym |
| `SPARSE_2` | Only symbols 0 and 1, 50/50 | 1.00 bits/sym |
| `SPARSE_17` | Only 17 distinct symbols, uniform distribution | ~4.09 bits/sym |
| `PRIME_RESIDUE` | Multiplicative congruential generator (prime modulus 257) | ~8.00 bits/sym |
| `RENORM_BOUNDARY` | Alternating 0/255 blocks at 16-symbol granularity | ~1.00 bits/sym |
| `INCOMPRESSIBLE_LIKE` | Full 8-bit random via seeded RNG | ~8.00 bits/sym |

### Corpus Sizes

Benchmarks are measured at 8 sizes per tier:

64 B → 256 B → 1 KiB → 4 KiB → 16 KiB → 64 KiB → 256 KiB → 1 MiB

This spans the range from tiny (L1 cache fits all data) to moderate (L2/L3 cache).
Larger corpora (4 MiB+) stress memory bandwidth and are available for the parallel
engine benchmarks.

### Corpus Construction

```rust
let corpus = Corpus::generate(ModelProfile::Uniform256, 1_048_576, 42);
let compressed = corpus.encode_16way();       // Pre-encode
let packed_table = corpus.packed_table();     // Build packed table
```

---

## Verification-Before-Timing Policy

**Critical invariant**: No benchmark reports a timing result unless the backend has
been verified to produce correct output.

### Verification Checks

Every Criterion benchmark calls `verify_16way` or `verify_8way` before entering the
timing loop:

```rust
// Verify 16-way decode
let report = verify_16way(
    "avx512-16way",
    &output, &words_consumed, &final_states,
    &reference_output, &reference_words, &reference_states,
);
assert_verified(&report);  // Panics if any check fails
```

The verification function checks:

| Check | What It Verifies |
|-------|-----------------|
| Output bytes match | `output == reference_output` — decoded data is byte-identical |
| Words consumed match | `words_consumed == reference_words` — same number of u16 words read |
| Final states match | `final_states == reference_states` — all 16 (or 8) final rANS states match |

All three must pass. A failure in any dimension causes a panic with a detailed message
identifying the backend, the failing check, and the expected vs actual values.

### Why Three Checks?

1. **Output bytes**: The most important — the decoded data must match the original input.
   This catches algorithmic errors in the state update or table lookup.

2. **Words consumed**: Ensures the renormalization loop consumed exactly the right number
   of words. A mismatch means the decoder read too few (leaving buffered state) or too many
   (overreading the stream).

3. **Final states**: Ensures the decoder's internal state after processing all symbols is
   correct. This is the most sensitive check — even a single symbol's state update error
   is caught here.

### Enforcement in Benchmarks

```rust
fn bench_avx2_8way(c: &mut Criterion) {
    let corpus = Corpus::generate(ModelProfile::Skewed255_1, 65536, 42);
    let compressed = corpus.encode_16way();
    let table = corpus.packed_table();

    // Reference decode
    let reference = decode_interleaved16_scalar_into(
        &compressed, &table, corpus.data.len()
    ).unwrap();

    // Verify before benchmarking
    let test = decode_interleaved16_avx2_2x8_checked(
        &compressed, &table, corpus.data.len()
    ).unwrap();
    assert_eq!(test.output, reference.output);
    assert_eq!(test.report.words_consumed, reference.report.words_consumed);
    assert_eq!(test.report.final_states, reference.report.final_states);

    // Now benchmark
    c.bench_function("avx2-2x8-on16/SKEWED_255_1/64KiB", |b| {
        b.iter(|| {
            black_box(decode_interleaved16_avx2_2x8_checked(
                black_box(&compressed),
                black_box(&table),
                black_box(corpus.data.len()),
            ))
        })
    });
}
```

---

## How to Run Benchmarks

### Prerequisites

- Rust toolchain (edition 2024 or later)
- For SIMD backends: appropriate CPU features (see below)
- For parallel benchmarks: multi-core CPU

### Basic Usage

```sh
# Run all benchmarks (selects available backends)
cargo bench -p ryg-rans-rs-bench
```

### Run Specific Tiers

```sh
# Scalar only
cargo bench -p ryg-rans-rs-bench --bench scalar

# AVX2 only
cargo bench -p ryg-rans-rs-bench --bench avx2

# Parallel engine scaling
cargo bench -p ryg-rans-rs-bench --bench parallel

# Container round-trip
cargo bench -p ryg-rans-rs-bench --bench container

# Batch decoder
cargo bench -p ryg-rans-rs-bench --bench batch
```

### Run with Full SIMD Support

To measure all backends including SSE4.1, AVX2, and AVX-512:

```sh
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" \
    cargo bench -p ryg-rans-rs-bench
```

### Run with Native CPU Features

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench --bench avx2
```

### Filter Specific Benchmarks

```sh
# Run only benchmarks matching "decode_8way"
cargo bench -p ryg-rans-rs-bench -- "decode_8way"

# Run only uniform256 profiles
cargo bench -p ryg-rans-rs-bench -- "UNIFORM256"

# Run only 1 MiB sizes
cargo bench -p ryg-rans-rs-bench -- "1MiB"
```

### Save and Compare Baselines

```sh
# Save a baseline
cargo bench -p ryg-rans-rs-bench --bench avx2 -- --save-baseline phase-j-avx2

# Compare against a saved baseline
cargo bench -p ryg-rans-rs-bench --bench avx2 -- --baseline phase-j-avx2
```

### JSON/CSV Export

```sh
# Run benchmarks, then export results
cargo bench -p ryg-rans-rs-bench
cargo run -p ryg-rans-rs-bench --bin export-results -- target/criterion output/results
```

---

## Interpreting Results

### Output Format

Criterion produces output like:

```
decode_8way_packed_scalar_into/UNIFORM256/1MiB
  time:   [10.234 ms 10.345 ms 10.456 ms]
  thrpt:  [95.67 MiB/s 96.72 MiB/s 97.81 MiB/s]
```

- `time`: Median, mean, and upper bound of per-iteration time
- `thrpt`: Throughput in MiB/s (higher is better), inverted from time

### Key Metrics

| Metric | Meaning |
|--------|---------|
| Median time | The typical iteration time (50th percentile) |
| Mean time | Average iteration time |
| Std dev | Stability of measurement (lower = more consistent) |
| Throughput (GiB/s) | Bytes processed per second (higher = better) |
| Scaling efficiency | Parallel speedup relative to single-threaded |

### Comparing Backends

To compare backends at the same corpus size and profile:

```sh
cargo bench -p ryg-rans-rs-bench -- "scalar/UNIFORM256/1MiB"
cargo bench -p ryg-rans-rs-bench -- "avx2/UNIFORM256/1MiB"
```

Or use Criterion's baseline comparison feature for statistical significance.

### Scaling Matrix

The parallel engine benchmarks produce a scaling matrix showing throughput at
1, 2, 4, 6, 8, 12, and 16 threads:

```
Threads:   1      2      4      6      8      12     16
GiB/s:    1.56   3.02   5.89   8.45   10.12  12.34  13.01
Efficiency: 100%   97%    94%    90%    81%    66%    52%
```

Efficiency = (speedup / thread count) × 100%. Perfect linear scaling = 100%.

---

## Structured Export

### JSON Export Schema

```json
{
  "benchmark_id": "scalar/UNIFORM256/1MiB",
  "tier": "scalar",
  "backend": "scalar-8way",
  "api": "decode_8way_packed_scalar_into",
  "profile": "UNIFORM256",
  "bytes": 1048576,
  "threads": 1,
  "median_ns": 10345000.0,
  "mean_ns": 10456000.0,
  "stddev_ns": 123000.0,
  "throughput_gib_s": 0.9672,
  "implementation_commit": "a1b2c3d4e5f6...",
  "rustc": "rustc 1.84.0 (9fc6b4312 2025-01-07)",
  "cpu": "AMD Ryzen 7 9800X3D",
  "target_features": ["avx2", "avx512f", "sse4.1"]
}
```

### CSV Export

```csv
benchmark_id,tier,backend,api,profile,bytes,threads,median_ns,mean_ns,stddev_ns,throughput_gib_s,commit
scalar/UNIFORM256/1MiB,scalar,scalar-8way,decode_8way_packed_scalar_into,UNIFORM256,1048576,1,10345000.0,10456000.0,123000.0,0.9672,a1b2c3d4
```

### Export Script

```sh
cargo run -p ryg-rans-rs-bench --bin export-results -- <criterion_dir> <output_dir>
```

This walks the Criterion output tree, extracts estimates, and writes `results.json` and
`results.csv` with a SHA-256 hash of the JSON file for integrity.

---

## Module Reference

### `lib.rs`

Re-exports the public modules:

```rust
pub mod common;    // Shared benchmark infrastructure
pub mod exporter;  // JSON/CSV result export
```

### `common/corpus.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `ModelProfile` | enum | 8 deterministic model profiles |
| `Corpus` | struct | Generated data + frequencies + model + packed table |
| `Corpus::generate` | fn | Create corpus from profile + length + seed |
| `Corpus::encode_16way` | fn | Encode corpus into 16-way compressed stream |
| `Corpus::packed_table` | fn | Build packed decode table for this corpus |

### `common/models.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `build_freqs` | fn | Build normalized frequency model from data |
| `is_uniform256` | fn | Check if model is the uniform-256 distribution |

### `common/verification.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `VerificationReport` | struct | Three-dimensional verification result |
| `verify_16way` | fn | Verify 16-way decode against scalar reference |
| `verify_8way` | fn | Verify 8-way decode against scalar reference |
| `assert_verified` | fn | Panic on verification failure |

### `common/metadata.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `BenchMetadata` | struct | Host metadata collector |
| `BenchMetadata::collect` | fn | Gather CPU, rustc, features, git commit |
| `BenchMetadata::to_map` | fn | Convert to HashMap for export |

### `exporter.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `BenchRecord` | struct | Single benchmark result record |
| `export_summary` | fn | Write JSON and CSV exports |
| `load_criterion_estimates` | fn | Parse Criterion output tree |

---

## Benchmark Architecture

### Directory Structure

```
benches/
  scalar.rs          — Tier 1: Scalar decode backends
  sse41.rs           — Tier 2: SSE4.1 decode backends
  avx2.rs            — Tier 3: AVX2 decode backends
  avx512.rs          — Tier 4: AVX-512 decode backends
  specialized.rs     — Tier 5: Specialized algorithm variants
  batch.rs           — Tier 6: Multi-stream batch decoders
  parallel.rs        — Tier 7: Multi-threaded parallel engine
  container.rs       — Tier 8: Container round-trip
  dispatch.rs        — Tier 9: Dispatch overhead

src/
  lib.rs             — Public module exports
  common/
    corpus.rs        — Deterministic corpus generation
    models.rs        — Frequency model construction
    verification.rs  — Backend verification helpers
    metadata.rs      — Host metadata collection
  exporter.rs        — JSON/CSV result export
```

### Data Flow

```
1. Corpus::generate(profile, length, seed)
   │
   ├─► data: Vec<u8>           (raw input bytes)
   ├─► freqs: Vec<u32>         (normalized frequency table)
   ├─► cum_freqs: Vec<u32>     (cumulative frequencies)
   │
   ├─► corpus.encode_16way()
   │     └─► compressed: Vec<u16>  (16-way Word rANS stream)
   │
   └─► corpus.packed_table()
         └─► table: PackedWordTable  (4096-slot packed table)
               │
               ▼
2. Reference decode (scalar)
   │
   ▼
3. Verify backend (output, words, states)
   │
   ▼
4. Benchmark (Criterion timing loop with black_box)
   │
   ▼
5. Export (JSON + CSV with host metadata)
```

---

## Adding a New Benchmark

To add a new benchmark tier:

1. Create `benches/new_tier.rs`
2. Define a Criterion benchmark group:
   ```rust
   use criterion::{Criterion, Throughput, black_box};
   use ryg_rans_rs_bench::common::{corpus::*, verification::*};

   fn bench_new_backend(c: &mut Criterion) {
       let mut group = c.benchmark_group("new-backend");
       for size in [64, 256, 1024, 4096] {
           let corpus = Corpus::generate(ModelProfile::Uniform256, size, 42);
           let compressed = corpus.encode_16way();
           let table = corpus.packed_table();

           // Verify
           let reference = decode_interleaved16_scalar_into(
               &compressed, &table, corpus.data.len()
           ).unwrap();
           let test = /* your backend */;
           assert_verified(&verify_16way(/* ... */));

           group.throughput(Throughput::Bytes(size as u64));
           group.bench_with_input(
               format!("UNIFORM256/{}B", size),
               &(&compressed, &table, corpus.data.len()),
               |b, (comp, tbl, len)| {
                   b.iter(|| black_box(/* your backend */))
               },
           );
       }
       group.finish();
   }
   ```
3. Register in `Cargo.toml`:
   ```toml
   [[bench]]
   name = "new_tier"
   harness = false
   ```
4. Run: `cargo bench -p ryg-rans-rs-bench --bench new_tier`

---

## Feature Flags

This crate has no public features. It always depends on `ryg-rans-rs-core`,
`ryg-rans-rs-simd`, and optionally `ryg-rans-rs-parallel` (for Tiers 7 and 8).

### Dependencies

| Dependency | Purpose |
|------------|---------|
| `ryg-rans-rs-core` | Scalar reference decoder (verification baseline) |
| `ryg-rans-rs-simd` | SSE4.1, AVX2, AVX-512 decode kernels (Tiers 2-6) |
| `ryg-rans-rs-parallel` | Parallel block engine (Tiers 7-8) |
| `criterion` | Benchmark harness + statistical analysis |
| `serde` / `serde_json` | JSON export formatting |
| `sha2` | SHA-256 hash of export files |
| `rand` | Seeded RNG for corpus generation |

---

*Part of the ryg-rans-rs project. Version 0.1.27. Phase J.*
