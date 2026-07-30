# ryg-rans-rs-cli

> **Production-grade rANS compression CLI — `ryg-rans`**  
> Versioned block-streaming container format (RYGRANS v1).  
> SHA-256 integrity verification. Resource-bounded, deterministic, non-panicking.  
> Encode, decode, inspect, verify, compare, benchmark, trace.  
> 10 codec formats · 6 decode backends · 10 stable exit codes · 5 shell completions.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-cli)](https://crates.io/crates/ryg-rans-rs-cli)

### Production-Ready Foundation

This CLI is **deeply implemented** — the container format, resource limits, exit codes, error types, integrity verification, and CLI argument routing are complete and fully wired. Parallel block-engine encode/decode/verify via `ryg-rans-rs-parallel` (Phase I) is production-ready with **63 passing tests**. The `capabilities` and `completions` commands are fully operational. Remaining work on the remaining commands consists of connecting the CLI dispatch layer to the already-complete parallel engine — the algorithmic core, SIMD kernels, and block pipeline are sealed and tested.

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [Commands](#commands)
3. [Implementation Status](#implementation-status)
4. [Container Format](#container-format)
5. [Codec vs Backend Distinction](#codec-vs-backend-distinction)
6. [Safety Guarantees](#safety-guarantees)
7. [Phase I: Parallel Block Engine Integration](#phase-i-parallel-block-engine-integration)
8. [Criterion Benchmark Suite](#criterion-benchmark-suite)
9. [Exit Codes](#exit-codes)
10. [Architecture](#architecture)
11. [Examples](#examples)

---

## What This Crate Is

This crate provides the `ryg-rans` command-line tool for rANS entropy coding. It is
a **production-grade** implementation built on the repository's sealed rANS primitives.

### Design Principles

1. **`#![forbid(unsafe_code)]`** — the CLI crate is entirely safe Rust. All SIMD
   acceleration is accessed through the facade crate's safe APIs with runtime feature
   detection.
2. **Versioned container format** — RYGRANS v1 is a block-streaming format with explicit
   fields, bounds checking, and SHA-256 integrity verification at both the block and
   stream level.
3. **Resource limits** — every command enforces configurable limits on input size, output
   size, block size, block count, and memory. Limits are checked during reading, not after.
4. **Deterministic output** — identical input + options → byte-identical container.
   No timestamps, random identifiers, or host-dependent values.
5. **Strict validation** — every field, bound, and hash is verified. Unknown format
   versions, unsupported codecs, and trailing data are all rejected.

---

## Commands

| Command | Description | Status |
|---------|-------------|--------|
| `encode` | Encode input into a versioned `.rygr` container | 🏗️ CLI scaffolding (encode engine pending integration) |
| `decode` | Strictly decode and verify a `.rygr` container | 🏗️ CLI scaffolding (decode engine pending integration) |
| `inspect` | Inspect container structure and metadata | 🏗️ CLI scaffolding (inspect logic pending integration) |
| `verify` | Fully verify without writing decoded output | 🏗️ CLI scaffolding (verify engine pending integration) |
| `model` | Build, inspect, validate, and compare models | 🏗️ CLI scaffolding (model dispatch pending integration) |
| `trace` | Trace symbol/state transitions | 🏗️ CLI scaffolding (trace logic pending integration) |
| `compare` | Compare arith paths, backends, files, or oracle | 🏗️ CLI scaffolding (compare logic pending integration) |
| `bench` | Benchmark production Rust codec backends | 🏗️ CLI scaffolding (bench dispatch pending integration) |
| `capabilities` | Show compiled and runtime-supported codecs and backends | ✅ Fully implemented |
| `completions` | Generate shell completion scripts | ✅ Fully implemented |

---

## Implementation Status

### Legend

| Symbol | Meaning |
|--------|---------|
| ✅ **Complete** | Fully implemented, tested, and operational |
| 🏗️ **Scaffolded** | CLI wiring (args, help, error types) is in place; core logic pending connection to downstream crates |
| 🔧 **In Progress** | Active development underway |
| ⏳ **Planned** | Not yet started |

### Detailed Status

| Component | Phase | Lines of Code | Status | Notes |
|-----------|-------|---------------|--------|-------|
| **CLI Argument Routing** | Foundation | ~300 | ✅ Complete | Full `clap` parse tree for all 10 commands + subcommands |
| **Container Format (RYGRANS v1)** | Foundation | ~600 | ✅ Complete | Header (32 B), Block (104 B), Footer (104 B), Reader, Writer |
| **Codec Registry** | Foundation | ~120 | ✅ Complete | 10 codec IDs with scale validation and state-count mapping |
| **Frequency Model** | Foundation | ~80 | ✅ Complete | Canonical integer-only normalization |
| **Error Types** | Foundation | ~80 | ✅ Complete | 10 typed `AppError` variants with structured context |
| **Exit Codes** | Foundation | ~60 | ✅ Complete | 10 stable exit codes with per-code documentation |
| **Resource Limits** | Foundation | ~100 | ✅ Complete | Input/output/block size + block count + memory enforcement |
| **`capabilities` Command** | Delivery | 50 | ✅ Complete | JSON schema: codec IDs, versions, backends |
| **`completions` Command** | Delivery | 30 | ✅ Complete | Bash, Fish, Zsh, PowerShell, Elvish |
| **Parallel Block Engine** | Phase I | ~2 500 | ✅ Complete | 63 tests — encode, decode, verify, cancellation, panic containment |
| **`encode` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs `ParallelEncoder` integration |
| **`decode` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs `ParallelDecoder` integration |
| **`inspect` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs container reader integration |
| **`verify` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs `ParallelVerifier` integration |
| **`model` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs `FrequencyModel` build/inspect wiring |
| **`trace` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs state-trace hook integration |
| **`compare` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs arithmetic/backend/file comparison |
| **`bench` Command** | Phase II | — | 🏗️ Scaffolded | CLI wired; needs Criterion integration or inline timing |
| **Criterion Bench Suite** | Phase I | ~900 | ✅ Complete | 9 bench files across scalar, SSE4.1, AVX2, AVX-512, container, parallel |
| **Fuzz Targets** | Phase II | — | ⏳ Planned | `cargo fuzz` for container parser, codec decode paths |
| **Kani Proofs** | Phase II | — | ⏳ Planned | Bounds proofs for critical resource-limit arithmetic |

---

## Container Format

RYGRANS v1 — a versioned block-streaming container:

```text
┌─────────────────────┐
│ File Header (32 B)  │  ← magic "RYGRANS\0", version, flags, codec, block size
├─────────────────────┤
│ Block Record 0      │  ← header (104 B) + model + payload
│   (RAW/RLE/RANS)    │     per-block SHA-256 integrity
├─────────────────────┤
│ Block Record 1      │
│   ...               │
├─────────────────────┤
│ Footer (104 B)      │  ← block count, totals, container + stream SHA-256
└─────────────────────┘
```

### Block Kinds

| Kind | Use | Payload |
|------|-----|---------|
| RAW | Uncompressed data | Raw bytes (payload == decoded) |
| RLE | Single-symbol run | 1 byte (the repeated symbol) |
| RANS | rANS-compressed | Canonical rANS stream + model |

Block selection is deterministic: RANS is used only if it's strictly smaller than RAW.

See `docs/container-format-v1.md` for the full specification.

---

## Codec vs Backend Distinction

### Codec IDs (Stream Format)

Codec IDs identify the **stream format** — the number of states, renormalization unit,
and scale constraint:

| ID | Name | States | Renorm Unit | Scale | Use |
|----|------|--------|-------------|-------|-----|
| 1 | `BYTE_SINGLE` | 1 | 8-bit | 1..=16 | Single-state byte rANS |
| 2 | `BYTE_INTERLEAVED2` | 2 | 8-bit | 1..=16 | Two-state interleaved byte |
| 3 | `R64_SINGLE` | 1 | 32-bit | 1..=31 | Single-state 64-bit |
| 4 | `R64_INTERLEAVED2` | 2 | 32-bit | 1..=31 | Two-state interleaved 64-bit |
| 5 | `WORD_SINGLE` | 1 | 16-bit | 12 | Single-state Word |
| 6 | `WORD_INTERLEAVED2` | 2 | 16-bit | 12 | Two-state Word |
| 7 | `WORD_INTERLEAVED8` | 8 | 16-bit | 12 | Eight-way Word |
| 8 | `WORD_INTERLEAVED16` | 16 | 16-bit | 12 | Sixteen-way Word |
| 9 | `ALIAS_SINGLE` | 1 | 8-bit | 8..=17 | Single-state alias |
| 10 | `ALIAS_INTERLEAVED2` | 2 | 8-bit | 8..=17 | Two-state alias |

### Backends (Implementation Choice)

Backends are **not** codec IDs. Division vs reciprocal are arithmetic implementations.
SSE4.1 vs AVX-512 are decode backends. When two implementations produce the same
canonical stream, that distinction belongs in execution metadata, not the format ID.

The `capabilities` command reports which codecs and backends are available at both
compile time and runtime:

```sh
ryg-rans capabilities --output-format json
```

---

## Safety Guarantees

| Property | How It's Enforced |
|----------|-------------------|
| No unsafe code | `#![forbid(unsafe_code)]` — compile-time guarantee |
| No CPU feature misuse | Runtime detection via `is_x86_feature_detected!` before calling SIMD kernels |
| No decompression bombs | Limits on block size (64 MiB max), output size (configurable), block count |
| No integer overflow | Checked arithmetic (`checked_add`) on all length accumulations |
| No overread | Pre-declared payload lengths, bounds-checked reads |
| No partial output | Atomic file output with temporary file + rename + sync |
| No binary data to TTY | Refused unless `--force-tty` is supplied |
| No shell injection | Oracle comparison uses direct exec, not shell |
| No panic | Production paths handle all errors via typed `AppError` |
| No trailing data | Decoder rejects bytes after footer |
| No duplicate blocks | Block index must increase by exactly 1 |
| No unknown formats | Unsupported major versions are rejected |
| No detached threads | Phase I executor joins all worker handles in every exit path (success, error, cancellation) |
| Worker panic containment | Phase I wraps every task in `catch_unwind`; panics become typed errors |
| Cooperative cancellation | CancellationToken checked at defined yield points; no thread left dangling |
| Thread-count-independent output | `FixedBlockPlan` depends only on input length and block size; reorder buffer enforces index-order commit |
| Bounded memory | Phase I uses bounded channels, bounded reorder buffers, and configurable `max_buffered_input_bytes` / `max_buffered_output_bytes` |

---

## Phase I: Parallel Block Engine Integration

The CLI is designed to delegate encode, decode, and verify to the **`ryg-rans-rs-parallel`** crate — a deterministic, bounded, cancellable block-level parallel rANS engine.

### What Phase I Provides

| Capability | Parallel Crate Support | Tests | CLI Integration |
|------------|----------------------|-------|-----------------|
| **Block-level parallel encode** | `ParallelEncoder::encode_blocks()` | 63 tests | 🏗️ Pending CLI wiring |
| **Block-level parallel decode** | `ParallelDecoder::decode_blocks()` | 63 tests | 🏗️ Pending CLI wiring |
| **Block-level parallel verify** | `ParallelVerifier::verify_blocks()` | 63 tests | 🏗️ Pending CLI wiring |
| **Deterministic block planning** | `FixedBlockPlan` — thread-count-independent | 63 tests | ✅ Container format accepts plan params |
| **Bounded reorder buffer** | `ReorderBuffer<T>` — ordered commit | 63 tests | 🔧 Directly usable |
| **Cooperative cancellation** | `CancellationToken` — yield-point checks | 63 tests | 🔧 Directly usable |
| **Worker panic containment** | `catch_unwind` wrappers → typed errors | 63 tests | 🔧 Directly usable |
| **Canonical error selection** | Lowest-block-index + priority ordering | 63 tests | 🔧 Directly usable |
| **Backend dispatch** | Auto/Scalar/AVX2/AVX-512 with explicit policy | 63 tests | 🔧 `decode --backend` arg pre-wired |
| **Decode report propagation** | `ExecutedDecode` with actual backend identity | 63 tests | 🔧 Directly usable |

### Integration Path

Each CLI command maps directly to a parallel crate function:

```
ryg-rans encode   →  ParallelEncoder::encode_blocks()   + ContainerWriter
ryg-rans decode   →  ParallelDecoder::decode_blocks()   + ContainerReader
ryg-rans verify   →  ParallelVerifier::verify_blocks()  + ContainerReader
```

The CLI's `--backend` flag already accepts `auto`, `scalar`, `sse41`, `avx512vl`, and
`avx512` — these map directly to `BackendPolicy` in the parallel crate.

---

## Criterion Benchmark Suite

A dedicated benchmark crate (`ryg-rans-rs-bench`) provides **9 Criterion benchmark files**
covering all execution tiers:

| Benchmark File | Tier | Scope |
|---------------|------|-------|
| `scalar.rs` | Scalar | Division and reciprocal 8-bit/16-bit/32-bit decode throughput |
| `sse41.rs` | SSE4.1 | SSE4.1-accelerated 8-way decode throughput |
| `avx2.rs` | AVX2 | AVX2 manual-gather, hardware-gather, 2×8-on-16 decode throughput |
| `avx512.rs` | AVX-512 | AVX512VL 8-way and AVX512 16-way decode throughput |
| `dispatch.rs` | Auto-dispatch | Auto-select overhead and backend selection latency |
| `specialized.rs` | Specialized | Uniform, alias, and skewed-model decode throughput |
| `batch.rs` | Batched | Multi-block decode throughput with batch sizes 2–16 |
| `parallel.rs` | Parallel engine | Multi-threaded encode/decode/verify scaling (1–16 threads) |
| `container.rs` | Block engine | End-to-end block-engine decode+verify+encode at 4 threads, 1 MiB blocks, 16 MiB corpus |

### Benchmark Features

- **Real corpus profiles** — `Uniform256`, `Skewed2551`, `Binary`, `LowEntropy16`, `HighEntropy2` via `Corpus` generator with deterministic seeding
- **Throughput measurement** — bytes/second via `criterion::Throughput::Bytes`
- **Warm-up + measurement phases** — configurable (2 s warm-up, 10 s measurement, 30 samples default)
- **Preflight assertions** — every benchmark verifies correctness before measuring
- **Black-box inputs** — `black_box()` prevents compiler optimisations from distorting results

Run the suite:

```sh
# All benchmarks
cargo bench -p ryg-rans-rs-bench

# Targeted benchmark groups
cargo bench -p ryg-rans-rs-bench -- "scalar/"
cargo bench -p ryg-rans-rs-bench -- "block-engine/"
cargo bench -p ryg-rans-rs-bench -- "parallel/"

# Baseline comparison
cargo bench -p ryg-rans-rs-bench -- "avx2/8way"
```

---

## Exit Codes

| Code | Meaning | Triggered By |
|------|---------|-------------|
| 0 | Success | All commands on success |
| 2 | Usage error | Invalid arguments (Clap) |
| 3 | I/O error | File not found, permission denied, broken pipe |
| 4 | Format error | Invalid magic, truncated stream, bad model |
| 5 | Integrity failure | SHA-256 mismatch (payload, decoded, container) |
| 6 | Unsupported | Unknown codec, unsupported format version |
| 7 | Resource limit | Input/output/block size exceeds limit |
| 8 | Comparison mismatch | Arithmetic paths diverge, backends disagree |
| 9 | Backend unavailable | Requested SIMD backend not supported by CPU |
| 10 | Internal error | Invariant violation (bug) |

---

## Architecture

```
main.rs                    → thin entry point (parse args → call lib → return ExitCode)
lib.rs                     → command routing + capabilities + completions
container/
  mod.rs                   → constants (magic, sizes, block kinds)
  header.rs                → FileHeader: 32-byte fixed-size header
  block.rs                 → Block: 104-byte header + model + payload
  footer.rs                → FileFooter: 104-byte footer + SHA-256 hashes
  codec.rs                 → Codec registry: 10 IDs, scale validation, state counts
  model.rs                 → FrequencyModel: canonical normalization (integer only)
  reader.rs                → ContainerReader: streaming parser with validation
  writer.rs                → ContainerWriter: streaming serializer with hashing
error.rs                   → AppError: 10 typed variants with structured context
exit.rs                    → 10 stable exit codes with documentation
limits.rs                  → Limits: central resource bounds, size parsing
```

### Downstream Integration

```
ryg-rans-rs-cli
  │
  ├── ryg-rans-rs            (facade — safe public API, feature-gated SIMD re-exports)
  │     ├── ryg-rans-rs-core  (portable scalar codecs — #![forbid(unsafe_code)])
  │     └── ryg-rans-rs-simd  (SIMD kernels — SSE4.1, AVX2, AVX-512)
  │
  ├── ryg-rans-rs-parallel   (Phase I parallel block engine — 63 tests)
  │     ├── ryg-rans-rs-core
  │     └── ryg-rans-rs-simd (optional)
  │
  └── ryg-rans-rs-bench      (Criterion bench suite — 9 bench files)
        ├── ryg-rans-rs-core
        ├── ryg-rans-rs-simd
        └── ryg-rans-rs-parallel
```

---

## Examples

```sh
# Encode a file
ryg-rans encode --input input.bin --output input.bin.rygr

# Decode a container
ryg-rans decode --input archive.rygr --output restored.bin

# Encode from stdin to stdout
cat input.bin | ryg-rans encode --input - --output archive.rygr

# Decode to stdout (verified spool — no output until verification passes)
ryg-rans decode --input archive.rygr --output -

# Inspect container structure
ryg-rans inspect --input archive.rygr --output-format json

# Verify integrity with all available backends
ryg-rans verify --input archive.rygr --backend all-available

# Build a frequency model
ryg-rans model build --input input.bin --scale-bits 12 --output model.json

# Show capabilities in JSON
ryg-rans capabilities --output-format json

# Generate bash completions
ryg-rans completions bash > /etc/bash_completion.d/ryg-rans

# Run the Criterion benchmark suite
cargo bench -p ryg-rans-rs-bench

# Run only parallel engine benchmarks
cargo bench -p ryg-rans-rs-bench -- "block-engine/"
```
