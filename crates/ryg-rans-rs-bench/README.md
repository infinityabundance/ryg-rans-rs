# ryg-rans-rs-bench

> **Criterion benchmark suite for the ryg-rans-rs execution tiers.**
> 9 tiers (scalar, SSE4.1, AVX2, AVX-512, specialized, batch, parallel, container, dispatch)
> plus the legacy byte/R64/alias surfaces and the Phase L.14 comparative court.
> Deterministic corpora (8 model profiles, fixed seeds) and a verification-before-timing policy.

**Version: 0.3.0** · `publish = false` (workspace-internal crate) · 16 tests

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [What This Crate Does NOT Do](#what-this-crate-does-not-do)
3. [Benchmark Targets](#benchmark-targets)
4. [Deterministic Corpora](#deterministic-corpora)
5. [Verification-Before-Timing Policy](#verification-before-timing-policy)
6. [Backend Semantics and SIMD Requirements](#backend-semantics-and-simd-requirements)
7. [Unsafe Boundaries](#unsafe-boundaries)
8. [How to Run Benchmarks](#how-to-run-benchmarks)
9. [Structured Export](#structured-export)
10. [Interpreting Results](#interpreting-results)
11. [Evidence Model](#evidence-model)
12. [Module Reference](#module-reference)
13. [Limitations (honest)](#limitations-honest)
14. [Troubleshooting](#troubleshooting)
15. [Versioning and Reading Order](#versioning-and-reading-order)

---

## What This Crate Is

This crate is the **measurement surface** for every ryg-rans-rs execution path. It uses
[Criterion.rs](https://github.com/bheisler/criterion.rs) for statistically-grounded
throughput measurement. Every benchmark binary:

1. Generates a **deterministic corpus** from a known `ModelProfile` + seed
2. **Verifies correctness** against a scalar reference **before** any timing begins
3. Measures **bytes/second** throughput via `criterion::Throughput::Bytes`
4. Collects **median, mean, standard deviation, and 95% confidence intervals**
5. Captures **host metadata** (CPU model, rustc version, target features, git commit)

The crate is `publish = false` — workspace-internal, never published to crates.io.

---

## What This Crate Does NOT Do

- **It does not seal performance evidence.** This crate produces Criterion
  measurements (raw trees under `target/criterion/`). Turning those measurements
  into sealed performance receipts is the job of `cargo xtask performance-seal`
  followed by the authoritative `cargo xtask seal` gate.
- **Its current exporter output is not sealed evidence.** The exporter
  (`src/exporter.rs`) fabricated sample counts, hardcoded verification flags, and
  produced empty hashes in the Phase K run — residuals **L1-A … L1-N** in
  [`evidence/phase-l/gap-ledger.md`](../../evidence/phase-l/gap-ledger.md). The
  exporter is being **rewritten in Phase L.18**, and Phase L.18 regenerates the
  performance evidence through the new `cargo xtask benchmark-run` wrapper. Until
  the regenerated run passes the full seal gate, **no performance claim from this
  crate is marked Sealed**.
- **It has no standalone export binary.** There is no `export-results` or `export`
  binary in this crate. Structured export is a **library API**
  (`exporter::load_criterion_estimates` + `exporter::export_summary`) consumed by
  `cargo xtask performance-seal`.
- **It does not seal behavioural evidence.** Behavioural parity receipts come from
  the oracle crate (`ryg-rans-rs-oracle`) and are verified by the seal gate.
- **The comparative court is not one of the ten sealed performance surfaces.**
  `benches/comparative.rs` is a separate methodological same-host comparison against
  upstream C (Phase L.14), documented in
  [`docs/performance/comparative.md`](../../docs/performance/comparative.md).

---

## Benchmark Targets

`benches/` contains 13 Criterion binaries. The first 9 are the tier suite; the next
3 are the legacy surfaces; the last is the Phase L.14 comparative court.

| # | Target | File | What It Benchmarks |
|---|--------|------|--------------------|
| 1 | `scalar` | `benches/scalar.rs` | Scalar reference decoders: 8-way packed (`allocating` + `into`), 16-way (`allocating`/`into` across 6 profiles), uniform256 scalar-specialized. |
| 2 | `sse41` | `benches/sse41.rs` | `sse41-8way` interleaved8 decode after runtime detection. |
| 3 | `avx2` | `benches/avx2.rs` | `avx2-2x8-on16`, `avx2-uniform256-tablefree-16way`, `avx2-manual-gather-8way`, `avx2-hardware-gather-8way`. |
| 4 | `avx512` | `benches/avx512.rs` | `avx512vl-8way`, `avx512-16way`, `avx512vl-2x8-on16`. |
| 5 | `specialized` | `benches/specialized.rs` | Profile-specialized kernels: uniform256 scalar and uniform256 AVX2 (table-free) `into` paths. |
| 6 | `batch` | `benches/batch.rs` | Batch4 preflight with **mixed tail lengths** (0/15/9/1 mod 16), batch aggregation overhead vs sequential `into` with matched allocation. |
| 7 | `parallel` | `benches/parallel.rs` | Block-level parallelism scaling (1/2/4/8/16 threads, fixed queue depth 64), 64 MiB and cold-16 MiB decode + verify + encode via `ryg-rans-rs-parallel`. |
| 8 | `container` | `benches/container.rs` | Block-engine decode+integrity / encode / verify scaling (1 MiB blocks, 16/64 MiB totals). |
| 9 | `dispatch` | `benches/dispatch.rs` | Runtime dispatch overhead: CPU-feature detection (`avx2_available_checked`), model classification, safe-wrapper vs direct unsafe dispatch. |
| 10 | `byte_rans` | `benches/byte_rans.rs` | Legacy 32-bit byte rANS: division + reciprocal encode, decode, interleaved2 encode/decode. |
| 11 | `r64` | `benches/r64.rs` | Legacy 64-bit rANS: division + reciprocal encode, decode, interleaved2. |
| 12 | `alias` | `benches/alias.rs` | Legacy alias method: table construction, single + interleaved2 encode/decode. |
| 13 | `comparative` | `benches/comparative.rs` | Phase L.14 court: Rust core vs upstream C via `ryg-rans-sys =1.2.0` FFI, same corpus/model/size; word comparison behind the `comparative-word-sse41` feature. |

---

## Deterministic Corpora

Every benchmark uses a `Corpus` generated from a known `ModelProfile` and seed. The
same profile + seed always produces identical bytes, frequencies, and compressed
streams — required for Criterion measurements to be comparable across runs.

### Model Profiles

| Profile | Description |
|---------|-------------|
| `Uniform256` | Every symbol 0..=255 appears exactly 16 times per 4096-byte block — perfect uniformity |
| `Freq1Residual` | 99.9% symbol 0, 0.1% random residuals — near-zero entropy, `cmpl_freq = M - 1` path |
| `Skewed2551` | 255/256 probability of symbol 0 — strong skew, 255:1 frequency disparity |
| `Sparse2` | Only symbols 0 and 1, 50/50 — minimal alphabet |
| `Sparse17` | 17 symbols, uniform — odd-sized alphabet, remainder distribution |
| `PrimeResidue` | Lehmer RNG modulo 257, mapped through `& 0xFF` — deterministic chaotic non-uniform data |
| `RenormBoundary` | Alternating runs of 0x00/0xFF every 16 bytes — frequent renormalisation |
| `IncompressibleLike` | Fresh uniform random bytes from a seeded RNG — worst-case expansion |

### Corpus Sizes

Sizes are chosen per target in the source; the common patterns are:

| Bench | Sizes |
|-------|-------|
| `byte_rans`, `r64`, `alias` | 64 B, 256 B, 1 KiB, 4 KiB, 64 KiB, 1 MiB |
| `scalar` 16-way, `avx2` | 64 KiB, 256 KiB, 1 MiB (8-way paths at 64 KiB) |
| `avx512` | 64 KiB (8-way), 1 MiB (16-way, 2×8) |
| `batch` | 4 × 1 MiB with mixed tail lengths |
| `parallel`, `container` | 16 MiB (cold) and 64 MiB (sustained), 1 MiB blocks |
| `comparative` | exactly 1 MiB, `Skewed2551`, seed 42 |

### Corpus Construction

```rust
let corpus = Corpus::generate(ModelProfile::Uniform256, 1_048_576, 42);
let compressed = corpus.encode_16way();   // 16-way interleaved Word rANS stream
let packed_table = corpus.packed_table(); // 4096-slot packed decode table
```

All three are deterministic functions of the `(profile, length, seed)` tuple
(`StdRng::seed_from_u64`).

---

## Verification-Before-Timing Policy

**Critical invariant**: no benchmark reports a timing result unless the backend has
been verified against a scalar reference first.

### Verification Checks

Every benchmark calls `verify_16way` or `verify_8way` before entering the timing
loop:

```rust
let report = verify_16way(
    "avx512-16way",
    &output, &words_consumed, &final_states,
    &reference_output, &reference_words, &reference_states,
);
assert_verified(&report); // Panics if any check fails
```

| Check | What It Verifies |
|-------|-----------------|
| Output bytes match | Decoded bytes are byte-identical to the reference |
| Words consumed match | Both backends consumed the same number of u16 words from the compressed stream |
| Final states match | All 16 (8-way: first 8 of 16) final rANS states are identical |

All three must pass. `assert_verified` **panics** on failure — the process exits
non-zero and the faulty backend is never timed. A failure means the backend is
miscompiled or incorrectly ported; silence would produce wrong numbers.

### Multi-threaded Preflight

`parallel` and `container` additionally run a **block-engine preflight**: encode
once (4-thread config), then decode/verify with every thread count in
{1, 2, 4, 8, 16} and assert byte-identical outputs and equal block counts against
the 1-thread reference **before** any timing. This pins the parallel determinism
invariant (same input → same output, independent of worker count).

---

## Backend Semantics and SIMD Requirements

- **Exact backend labels** follow the project glossary: `scalar-8way`,
  `sse41-8way`, `avx2-manual-gather`, `avx2-hardware-gather`, `avx2-2x8`,
  `avx2-uniform256`, `avx2-batch4`, `avx512vl-8way`, `avx512-16way`.
- **Runtime ISA detection**: SIMD benches call the simd crate's runtime checks
  (`avx2_available`, `avx512vl_available`, `avx512_available`). If the required
  feature is unavailable at runtime, the bench prints `UNSUPPORTED: <backend>` and
  returns **without timing that backend** — no silent scalar substitution is
  measured or claimed.
- **Compile-time features are required too**: the SIMD kernels carry
  `#[target_feature]` attributes, so build with
  `RUSTFLAGS="-C target-cpu=native"` (or the explicit feature list) to get real
  SIMD execution.
- **Batch4** benches exercise the batched AVX2 decode with mixed tail lengths
  (not just multiples of 4) and matched allocation policies between sequential and
  batch paths so the comparison is fair.

---

## Unsafe Boundaries

- The bench crate itself defines no `unsafe fn`; it does contain `unsafe` **call
  sites** (e.g. `benches/avx512.rs` calls `decode_interleaved8_avx512vl`) that
  invoke the SIMD crate's ledgered kernels. Those kernels live in
  `ryg-rans-rs-simd`, carry their own exact `#[target_feature]` attributes, and
  are inventoried in `crates/ryg-rans-rs-simd/unsafe-ledger.toml`.
- `benches/comparative.rs` is the **only** FFI surface in the workspace
  (`ryg-rans-sys = "=1.2.0"`), pinned to the exact upstream-C binding version and
  gated by the `comparative-word-sse41` feature for the word surface. It is a
  measurement surface (`publish = false`), not a production dependency.

---

## How to Run Benchmarks

### Prerequisites

- Rust toolchain (workspace edition 2024)
- For SIMD tiers: a CPU with the relevant features and
  `RUSTFLAGS="-C target-cpu=native"` (or explicit `-C target-feature=...`)
- For parallel/container tiers: a multi-core CPU

### Basic Usage

```sh
cargo bench -p ryg-rans-rs-bench                 # full suite
cargo bench -p ryg-rans-rs-bench --bench scalar  # one tier
cargo bench -p ryg-rans-rs-bench --bench avx2    # AVX2 tier
```

### Native SIMD

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench
```

Or with explicit features:

```sh
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" \
    cargo bench -p ryg-rans-rs-bench
```

### Filter Specific Benchmarks

```sh
cargo bench -p ryg-rans-rs-bench -- "decode"     # matches function names
cargo bench -p ryg-rans-rs-bench -- "UNIFORM256"
cargo bench -p ryg-rans-rs-bench -- "1MiB"
```

### Save and Compare Baselines

```sh
cargo bench -p ryg-rans-rs-bench --bench avx2 -- --save-baseline phase-j-avx2
cargo bench -p ryg-rans-rs-bench --bench avx2 -- --baseline phase-j-avx2
```

### Phase L.14 Comparative Court

```sh
RUSTFLAGS="-C target-cpu=native" \
  cargo bench -p ryg-rans-rs-bench --bench comparative \
  --features comparative-word-sse41 \
  -- --save-baseline phase-l-comparative-final
```

The word comparison needs the `comparative-word-sse41` feature (its C surface
requires SSE4.1 compiled in). The default build excludes the word comparison.

---

## Structured Export

### The Library API

There is no export binary. The exporter is a library:

- `exporter::load_criterion_estimates(&criterion_dir, &metadata) -> Result<Vec<BenchRecord>, String>`
  — walks `target/criterion`, parses every `estimates.json`, and validates the
  records (dirty-tree rejection, NaN/infinity/negative rejection, zero-sample
  rejection, commit-mismatch rejection, duplicate-ID rejection).
- `exporter::export_summary(&records, &output_dir) -> Result<(json_path, csv_path, json_sha, csv_sha), String>`
  — writes canonical `results.json` (compact, lexicographically sorted keys,
  records sorted by `benchmark_id`) and `results.csv`, returning SHA-256 hashes of
  both.

The consumer is `cargo xtask performance-seal`, which calls both functions when
generating per-surface evidence.

### `BenchRecord` JSON Schema

```json
{
  "benchmark_id": "avx512/avx512-16way/allocating/INCOMPRESSIBLE_LIKE/1MiB",
  "tier": "avx512",
  "backend_requested": "avx512-16way",
  "backend_executed": "avx512-16way",
  "api": "allocating",
  "profile": "INCOMPRESSIBLE_LIKE",
  "bytes": 1048576,
  "threads_requested": 1,
  "threads_effective": 1,
  "median_ns": 1234567.89,
  "mean_ns": 1245678.90,
  "stddev_ns": 12345.67,
  "confidence_low_ns": 1220000.0,
  "confidence_high_ns": 1270000.0,
  "sample_count": 100,
  "throughput_gib_s": 7.89,
  "implementation_commit": "abc123def456",
  "rustc": "rustc 1.96.0 (...)",
  "cpu": "AMD Ryzen 7 9800X3D 8-Core Processor",
  "target_features": ["avx512f", "avx512bw"],
  "runtime_features": ["avx512f", "avx512vl", "avx512bw"],
  "verification_passed": true,
  "output_hash": "e3b0c44298fc1c149afbf4c8996fb924...",
  "words_consumed_hash": "e3b0c44298fc1c149afbf4c8996fb924...",
  "final_states_hash": "e3b0c44298fc1c149afbf4c8996fb924...",
  "status": "pass"
}
```

CSV columns:
`benchmark_id,tier,backend_requested,backend_executed,api,profile,bytes,threads_requested,threads_effective,median_ns,mean_ns,stddev_ns,confidence_low_ns,confidence_high_ns,sample_count,throughput_gib_s,commit,status`

> **Status caveat (Phase L.18):** the current exporter reconstructed identity from
> sanitized directory names and defaulted `sample_count`/`verification_passed`/
> `output_hash` when the preflight channel was absent (residuals L1-A…L1-N). It is
> under rewrite. Do not treat its current output as sealed evidence.

---

## Interpreting Results

Criterion reports per-iteration time and throughput:

```
scalar-16way/into/INCOMPRESSIBLE_LIKE/1MiB
  time:   [10.234 ms 10.345 ms 10.456 ms]
  thrpt:  [95.67 MiB/s 96.72 MiB/s 97.81 MiB/s]
```

- `time`: lower bound, median, upper bound of per-iteration time
- `thrpt`: throughput in MiB/s, inverted from time (higher is better)

### Key Metrics

| Metric | Meaning |
|--------|---------|
| Median time | Typical iteration time (50th percentile) |
| Mean / std dev | Average and stability of measurement |
| Throughput (GiB/s) | `(bytes / median_ns) * 1e9 / (1024³)` |
| Confidence interval | 95% CI on the mean, from Criterion |
| Scaling efficiency | Parallel speedup relative to 1 thread |

### Phase K Scaling Matrix (historical, superseded)

The Phase K parallel-engine measurements below are **retained as historical
evidence only**. The Phase L.18 pipeline regenerates the performance receipts; the
Phase K run is superseded (residuals L1-A…L1-S) and is **not** sealed evidence.
Host was an AMD Ryzen 7 9800X3D (8 cores / 16 threads), 64 MiB block-engine
decode+integrity workload (64 × 1 MiB blocks, fixed queue depth 64):

```
Threads:   1        2         4         8         16
GiB/s:    0.90     1.69      3.24      5.25      6.36
Speedup:  1.00×    1.87×     3.60×     5.83×     7.07×
Efficiency: 100%  93.9%     90.0%     72.9%     44.2%
```

Efficiency = (speedup / thread count) × 100%. 1–4 threads scaled near-linearly;
SMT (8→16) added ~21%. These numbers will be regenerated and re-sealed in Phase
L.18.

### Phase L.14 Comparative Court (vs upstream C)

Same-host, identical corpus (`Skewed2551`, seed 42, exactly 1 MiB), identical
frequency model passed to both sides, `RUSTFLAGS="-C target-cpu=native"`,
Criterion 0.5.1 (warm-up 2 s, measurement 8 s, 50 samples). Full methodology and
residuals: [`docs/performance/comparative.md`](../../docs/performance/comparative.md).

**Preflight**: the compressed output of the Rust core and upstream C is
**byte-identical** for both byte rANS (`rans_byte_enc_put_symbol` vs
`rans_enc_put_symbol`) and word rANS (`rans_word_enc_put` LE bytes vs C u16 words
flattened to LE bytes).

| Case | Rust core | C (via `ryg-rans-sys` FFI) | Ratio |
|------|-----------|----------------------------|-------|
| byte encode (reciprocal) | 541.4 MiB/s | 514.8 MiB/s | **1.05×** |
| byte encode (division, reference path) | 349.6 MiB/s | — | reciprocal is 1.55× faster |
| byte decode | 947.3 MiB/s | 430.2 MiB/s | **2.20×** end-to-end |
| word encode | 365.0 MiB/s | 215.7 MiB/s | **1.69×** |
| word decode | 486.4 MiB/s | 480.2 MiB/s | **1.01×** (parity) |
| FFI crossing (isolated) | — | 1.09 ns/call; ≈2.0 ms/MiB at 2 calls/byte | — |

The byte-decode 2.20× is **end-to-end**: ≈2.0 ms/MiB of the C-side time is the
isolated cost of the mandatory two FFI crossings per byte (measured separately);
the Rust path pays zero crossings. The court claims no general "faster than C"
result — it records measurements, separations, and residuals. Residuals: **L14-A**
(C compiled by `cc` without `-march=native`, so non-vectorised C paths favour
Rust) and **L14-B** (`rans` 0.4.0 excluded — different API/format, not
byte-comparable).

---

## Evidence Model

- This crate is the **measurement surface**; Criterion raw trees land in
  `target/criterion/`.
- A **preflight** record (backend requested/executed, output hash,
  words-consumed hash, final-states hash, verification verdict) is what the Phase
  L.18 exporter rewrite joins to Criterion timing by exact benchmark ID — this
  channel does **not** exist yet, which is residual L1-D.
- Performance **receipts** (one per surface, 10 surfaces), **manifests**, and the
  run **index** are produced by `cargo xtask performance-seal` into
  `evidence/performance/runs/<run-id>/` and sealed by `cargo xtask seal`.
- The Phase K run is archived at `evidence/performance/runs/phase-k-20260731-004044/`
  as **superseded** evidence (never deleted).
- The Phase L.14 court artifacts are archived under `evidence/phase-l/comparative/criterion/`.

---

## Module Reference

### `lib.rs`

```rust
pub mod common;    // Shared benchmark infrastructure
pub mod exporter;  // Criterion structured summary export (JSON + CSV)
```

### `common/corpus.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `ModelProfile` | enum | 8 deterministic model profiles |
| `Corpus` | struct | Data + frequencies + cumulative frequencies + scale model |
| `Corpus::generate(profile, length, seed)` | fn | Create a deterministic corpus |
| `Corpus::encode_16way()` | fn | Encode into a 16-way interleaved Word rANS stream (`Vec<u16>`) |
| `Corpus::packed_table()` | fn | Build the `PackedWordTable` for this corpus |
| `ModelProfile::label()` | fn | Canonical profile label (e.g. `"UNIFORM256"`) |

### `common/models.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `build_freqs(data, total)` | fn | Build a normalized frequency model summing to `total` |
| `is_uniform256(freqs)` | fn | True if every symbol has frequency 16 |

### `common/verification.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `VerificationReport` | struct | `output_matches`, `words_consumed_match`, `final_states_match`, `all_ok` |
| `verify_16way(...)` | fn | Compare against the scalar 16-way reference (all 16 states) |
| `verify_8way(...)` | fn | Compare against the scalar 8-way reference (first 8 of 16 slots) |
| `assert_verified(&report)` | fn | Panic if `all_ok` is false |

### `common/metadata.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `BenchMetadata` | struct | rustc version, target features, CPU model, OS, git commit, dirty-tree flag, CPU count |
| `BenchMetadata::collect()` | fn | Gather host metadata |
| `BenchMetadata::to_map()` | fn | `HashMap<String, String>` form for export |

### `exporter.rs`

| Symbol | Kind | Description |
|--------|------|-------------|
| `BenchRecord` | struct | One validated benchmark record (fields above) |
| `export_summary(records, dir)` | fn | Write canonical `results.json` + `results.csv`, return SHA-256 hashes |
| `load_criterion_estimates(dir, metadata)` | fn | Parse + validate the Criterion tree into `Vec<BenchRecord>` |

---

## Limitations (honest)

1. **Exporter under repair (L.18).** The Phase K exporter fabricated
   `sample_count`, hardcoded `verification_passed`, and left hashes empty
   (L1-A…L1-N); the rewrite is Phase L.18 work. Current exporter output is not
   sealed evidence.
2. **Phase K measurements are superseded.** Any historical number in this README
   or the root README that is not from the Phase L.14 court is Phase K data,
   retained for the record, regenerated in L.18.
3. **Word comparison is opt-in.** The comparative court's word surface requires
   the `comparative-word-sse41` feature and native RUSTFLAGS; default builds
   exclude it.
4. **Unavailable ISA backends are not timed.** A bench that prints
   `UNSUPPORTED:` for a backend leaves that backend with zero measurements on that
   host — a real signal, not an error.
5. **The comparative court is methodological, not a sealed surface.** Its
   numbers are not part of the ten performance receipts.
6. No claim of general superiority over C is made anywhere in this crate's
   documentation; the comparative court records measurements, separations, and
   residuals.

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `UNSUPPORTED: avx512vl-8way` printed | CPU or build lacks the ISA. Rebuild with `RUSTFLAGS="-C target-cpu=native"` (or the explicit feature list) on capable hardware. |
| `Backend '...' verification FAILED` | A backend produced wrong output/words/states vs the scalar reference. This is a real defect — the process aborts before timing, by design. |
| Criterion reports 0 measurements for a group | The backend failed verification (see above) or the ISA is unavailable. |
| `refusing to export: working tree is dirty` | `load_criterion_estimates` rejects dirty trees — benchmark provenance requires a clean checkout. |
| Word comparative functions missing | Build without the `comparative-word-sse41` feature; that surface needs it plus SSE4.1 compiled in. |
| `no matching package named ryg-rans-sys` | The workspace cannot resolve the pinned `=1.2.0` dependency offline; run with network access. |

---

## Versioning and Reading Order

- **Version**: 0.3.0 (workspace crates); `publish = false` — never published.
- **Reading order**: root [`README.md`](../../README.md) →
  [`docs/architecture.md`](../../docs/architecture.md) →
  [`docs/performance-method.md`](../../docs/performance-method.md) →
  [`docs/performance/comparative.md`](../../docs/performance/comparative.md) →
  [`docs/glossary.md`](../../docs/glossary.md) → this README →
  [`xtask/README.md`](../../xtask/README.md).
- **Evidence status**: see the Evidence Status table in the root README; the
  performance column reads **Re-sealing (L.18)** until the regenerated run passes
  the seal gate.

---

*Part of the ryg-rans-rs project. Version 0.3.0. Phase L.15 documentation pass.*
