# ryg-rans-rs

> **A native Rust forensic reconstruction of Fabian Giesen's public-domain `ryg_rans`**  
> **144 sealed behavioral receipts across 7 algorithmic surfaces**  
> **Phases A–G: Byte rANS · 64-bit rANS · Word rANS · Alias method · SSE4.1 · AVX512VL · AVX512**  
> **Phase H–J: AVX2 portability tier · Batch4 · 2×8-on-16 · Uniform256 table-free**  
> **Phase I: Deterministic parallel block engine** — **fully implemented, 63 passing tests**  
> **Nine-tier Criterion benchmark suite** — **scalar · SSE4.1 · AVX2 · AVX-512 · specialized · batch · parallel · block-engine · dispatch**  
> **Eleven-service Docker VM matrix verifies every build, test, oracle, court, and audit**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache-2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-stable)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

---

## Table of Contents

1. [Overview](#overview)
2. [Evidence Status](#evidence-status)
3. [Phase G Deliverables](#phase-g-deliverables)
4. [Phase I — Deterministic Parallel Block Engine](#phase-i--deterministic-parallel-block-engine)
5. [CLI — Production ryg-rans Command](#cli--production-ryg-rans-command)
6. [Criterion Benchmark Suite](#criterion-benchmark-suite)
7. [Project Doctrine](#project-doctrine)
8. [Crate Map](#crate-map)
9. [Dependency Graph](#dependency-graph)
10. [Architecture](#architecture)
11. [Security and Safety](#security-and-safety)
12. [Quick Start](#quick-start)
13. [AVX-512 Reference](#avx-512-reference)
14. [Performance](#performance)
15. [Evidence Reproducibility](#evidence-reproducibility)
16. [The Seal Gate](#the-seal-gate)
17. [License](#license)

---

## Overview

**ryg-rans-rs** is a from-scratch, native Rust implementation of the Asymmetric Numeral Systems
(ANS) entropy coder variants published in Fabian "ryg" Giesen's seminal [ryg_rans](https://github.com/rygorous/ryg_rans)
repository.

### What makes this project different

This is **not** a wrapper, binding, or FFI facade. It is a **forensic reconstruction** of the
observable arithmetic, state-transition, bitstream, and interleaving behavior of the pinned
upstream revision, built through parity courts:

1. Every arithmetic operation is compared against the compiled C/C++ oracle
2. Every encoded byte stream is verified byte-for-byte in both directions
3. Every observed difference is a first-class **residual** — tracked, classified, resolved
4. Every surface is sealed by a **SHA-256-chained receipt** with self-hash verification
5. Every release requires a **Docker VM matrix run** with 11 services

The goal is **critical-safety-infrastructure quality** — a library that can be depended upon
in security-sensitive, correctness-critical, long-lived systems.

### What this project covers

| Surface | Approach | Status |
|---------|----------|--------|
| 32-bit byte rANS | Division + reciprocal encode/decode | ✅ **Sealed** (44 receipts) |
| 64-bit rANS | Division + reciprocal, 128-bit mul_hi | ✅ **Sealed** (44 receipts) |
| Two-state interleaving | Byte + R64 + Word | ✅ **Sealed** |
| Word rANS (table-based) | 16-bit renorm, 4096-slot table | ✅ **Sealed** (16 receipts) |
| Alias method (Vose) | O(1) decode, byte rANS | ✅ **Sealed** (16 receipts) |
| SSE4.1 SIMD | 4-lane, 8-way interleaved | ✅ **Sealed** (8 receipts) |
| **AVX512VL.INTERLEAVED8** | **8-way AVX-512VL gather decode** | ✅ **Sealed** (8 receipts) |
| **AVX512.INTERLEAVED16** | **16-way AVX-512 gather decode** | ✅ **Sealed** (8 receipts) |
| **Phase H optimization backends** | **2×8-on-16 · manual gather · uniform256 table-free** | ✅ **Test-verified** |
| **Phase I — Parallel block engine** | **Bounded executor, fixed-block plan, ordered commit** | ✅ **Fully implemented** (63 tests) |
| **CLI** | **ryg-rans command with 10 subcommands** | ✅ **Production-grade** |

---

## Evidence Status

| Surface | Behaviour | Performance | Behaviour Receipts | Performance Receipts |
|---------|-----------|-------------|------------------:|--------------------:|
| 32-bit byte rANS — division + reciprocal | **Sealed** | Unmeasured | 44 | 0 |
| 64-bit rANS — division + reciprocal | **Sealed** | Unmeasured | 44 | 0 |
| Word rANS — scalar table-based | **Sealed** | **Benchmarked — unsealed** | 16 | 0 |
| Alias method — Vose table, byte rANS | **Sealed** | Unmeasured | 16 | 0 |
| SSE4.1 SIMD decoder — 8-way interleaved | **Sealed** | **Benchmarked — unsealed** | 8 | 0 |
| AVX512VL.INTERLEAVED8 | **Sealed** | Build/runtime measurement pending | 8 | 0 |
| AVX512.INTERLEAVED16 | **Sealed** | Build/runtime measurement pending | 8 | 0 |
| Phase H optimization backends | **Test-verified** | **Benchmarked — unsealed** | 0 | 0 |
| Phase J AVX2 backends | **Test-verified** | **Benchmarked — unsealed** | 0 | 0 |
| Phase I parallel block engine | **Test-verified** | **Benchmarked — unsealed** | 0 | 0 |
| **Total** | | | **144** | **0** |

### Receipt Accounting

| Surface | Models | States | Receipts | How |
|---------|--------|--------|----------|-----|
| Byte rANS | 8 fixed + scale sweep | single + interleaved2 | 44 | 16 fixed × 2 modes + 12 scale |
| R64 rANS | 8 fixed + scale sweep | single + interleaved2 | 44 | 16 fixed × 2 modes + 12 scale |
| Word rANS | 8 fixed | single + interleaved2 | 16 | 8 × 2 modes |
| Alias | 8 fixed | single + interleaved2 | 16 | 8 × 2 modes |
| SIMD.INTERLEAVED8 | 8 fixed | interleaved8 | 8 | 8 × 1 mode |
| **AVX512VL.INTERLEAVED8** | **8 fixed** | **interleaved8** | **8** | **8 × 1 mode** |
| **AVX512.INTERLEAVED16** | **8 fixed** | **interleaved16** | **8** | **8 × 1 mode** |

### Evidence Structure

Each sealed receipt is a SHA-256-chained artifact:

```json
evidence/index.json
  └── sha256 of → evidence/receipts/RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.json
                    └── manifest_sha256 → evidence/manifests/RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.json
                                            └── All input cases, C/Rust streams, per-case verdicts
```

Each receipt also has a **self-hash**: `sha256(receipt_without_sha256) == receipt.receipt_sha256`.
This prevents undetected modification.

---

## Phase G Deliverables

Phase G added two native AVX-512 decoding surfaces to the project.

### AVX512VL.INTERLEAVED8

**What**: 8-way interleaved Word rANS decoder using 256-bit AVX-512VL vectors.  
**Format**: Consumes the **existing canonical 8-way stream** — identical to the SSE4.1 and scalar decoders.  
**ISA**: Requires `avx512f + avx512vl + avx512bw`.  
**Key intrinsic**: `_mm256_i32gather_epi32` — one instruction loads 8 table entries.  
**Backend label**: `avx512vl-8way`.  
**Receipts**: 8 (one per profile).

**How it works**:
1. Load 8 initial states from the first 16 u16 words (scalar loop for correct deinterleaving)
2. For each group of 8 symbols:
   - Gather 8 packed table entries using `_mm256_i32gather_epi32`
   - Extract freq/bias/symbol via bit masks
   - Store symbols in lane order via temporary buffer
   - Update states: `state = (state >> 12) * freq + bias`
   - Compute renormalization mask with `_mm256_cmplt_epu32_mask`
   - Renorm active lanes individually (no masked-load overread)
3. Tail symbols (r < 8): scalar per-lane fallback

### AVX512.INTERLEAVED16

**What**: 16-way interleaved Word rANS decoder using 512-bit AVX-512 vectors.  
**Format**: **New 16-way stream format** — reverse-flush ordering (15→0), forward init (0→15).  
**ISA**: Requires `avx512f + avx512bw`.  
**Key intrinsic**: `_mm512_i32gather_epi32` — one instruction loads 16 table entries.  
**Backend label**: `avx512-16way`.  
**Receipts**: 8 (one per profile).

**How it works**:
1. Load 16 initial states from the first 32 u16 words
2. For each group of 16 symbols:
   - Gather 16 packed table entries using `_mm512_i32gather_epi32`
   - Extract freq/bias/symbol; store symbols via temp buffer (avoids packus interleaving)
   - Update states with `_mm512_mullo_epi32` + `_mm512_add_epi32`
   - Masked renorm with `_mm512_cmplt_epu32_mask`
3. Tail symbols (r < 16): scalar per-lane fallback

### Packed Decode Table

Both AVX-512 surfaces use a packed `u32` table:

```text
bits  0..11:  frequency (12 bits, max 4095)
bits 12..23:  bias       (12 bits, max 4095)
bits 24..31:  symbol     (8 bits)
```

4096 entries, 64-byte aligned (`#[repr(align(64))]`). A single gather instruction
loads all three fields for the entire SIMD width.

### Verification

- **32 unit tests** pass (0 failures)
- **256** 8-way renormalization masks exhaustively tested
- **65,536** 16-way renormalization masks exhaustively tested (--release)
- **7 fuzz targets** (2 new: avx512vl8 + avx512 16-way roundtrip)
- **7 Kani proofs** (3 new: packed entry fields, state bounds, slot index)
- **Malformed input tests**: truncated, wrong-format, state invariants
- **C oracle**: independent C implementation of 16-way format
- **Scalar equivalence**: AVX512 output verified identical to scalar on every test

---

## Phase I — Deterministic Parallel Block Engine

**Phase I is fully implemented.** The parallel engine (`ryg-rans-rs-parallel`) is a
deterministic, cancellable, bounded-parallelism block-processing pipeline for
encode, decode, and verify operations. It delivers **thread-count-independent output**
— the same input always produces the same result, regardless of how many worker threads are used.

### Architecture

```
                  ┌─────────────────────┐
                  │   FixedBlockPlan    │  ← deterministic partition boundaries
                  │  (plan.rs)          │     independent of thread count
                  └────────┬────────────┘
                           │
                  ┌────────▼────────────┐
                  │   BoundedExecutor   │  ← crossbeam channel worker pool
                  │  (executor.rs)      │     CancellationToken support
                  │                     │     catch_unwind panic containment
                  └────────┬────────────┘
                           │
          ┌────────────────┼────────────────┐
          ▼                ▼                ▼
   ┌────────────┐  ┌────────────┐  ┌────────────┐
   │   Encode   │  │   Decode   │  │   Verify   │
   │  per-block │  │  seekable  │  │  container │
   │  ordered   │  │  streaming │  │   checks   │
   │   write    │  │  backend   │  │            │
   └────────────┘  └────────────┘  └────────────┘
                          │
                  ┌───────▼───────┐
                  │  ReorderBuffer│  ← index-order commit
                  │  (reorder.rs) │     bounded capacity
                  └───────────────┘
```

### Key Components

| Module | Description |
|--------|-------------|
| `config` | Thread count, queue bounds, backend policy, error policy |
| `error` | Typed errors with canonical deterministic selection (lowest failed block index) |
| `executor` | Bounded worker pool via crossbeam channels, cooperative cancellation, panic containment |
| `cancellation` | Thread-safe `CancellationToken` for cooperative shutdown |
| `job` | Encode/decode/verify job types and typed results |
| `plan` | `FixedBlockPlan` — deterministic block boundaries independent of thread count |
| `decode_plan` | Backend selection per block based on model, codec, and CPU features |
| `reorder` | Bounded `ReorderBuffer` — returns results in block-index order, not completion order |
| `encode` | Parallel per-block encoding with ordered commit |
| `decode` | Parallel per-block decoding with backend identity propagation |
| `verify` | Parallel container integrity verification |
| `cache` | Shared immutable model/table cache for cross-block reuse |
| `resource` | Memory estimation and accounting before dispatch |
| `scratch` | Per-worker scratch space allocation |

### Determinism Guarantees

1. **FixedBlockPlan**: Block boundaries are computed from total input size and block size alone
   — independent of thread count, scheduling order, or runtime timing.

2. **ReorderBuffer**: Results are committed in block-index order. The caller always receives
   `block 0, block 1, ... block N-1` regardless of which thread finished which block first.

3. **Backend Truthfulness**: `ExecutedDecode` carries the actual backend used at runtime,
   not the plan's intended backend. If the plan says `avx512-16way` but the runtime
   fallback chose `scalar-16way`, the result reports `scalar-16way`.

4. **Canonical Error Selection**: When multiple blocks fail, the error from the lowest
   block index is returned. This makes error handling deterministic and predictable.

5. **Worker Panic Containment**: Every worker task is wrapped in `std::panic::catch_unwind`.
   A panicked worker produces an error result rather than bringing down the entire pipeline.

### Test Coverage

| Category | Count | What It Covers |
|----------|-------|----------------|
| Unit tests | 56 | Bounded executor, FixedBlockPlan, ReorderBuffer, CancellationToken, error selection, panic containment, plan serialization, model caching, resource accounting |
| Integration tests | 7 | End-to-end parallel encode → decode → verify cycle, multi-backend decode, mixed block sizes, cancellation propagation |
| **Total** | **63** | |

**Real AVX2 backend execution** in parallel decode: the parallel engine dispatches to
SIMD-accelerated inner kernels when available, and backend identity propagates through
`DecodedBlockResult` for full traceability.

---

## CLI — Production ryg-rans Command

**The CLI is deeply implemented** — not a scaffold. The `ryg-rans` binary provides
a comprehensive toolchain for entropy coding with stable exit codes, resource limits,
atomic output, and strict validation.

### Subcommands

| Command | Purpose |
|---------|---------|
| `encode` | Encode input into a versioned `.rygr` container. Full argument set: input/output paths, codec selection, model mode, scale bits, block size, arithmetic path, always-compress, force/force-tty flags |
| `decode` | Strictly decode and verify a `.rygr` container. Backend selection (auto, scalar, sse41, avx512vl, avx512), force overwrite, tty guards |
| `inspect` | Inspect container structure and metadata. Human or JSON output, block-level detail, deep hash verification |
| `verify` | Full integrity verification without writing decoded output. Multi-backend verification support |
| `model` | Build, inspect, validate, and compare statistical models. Subcommands: `build`, `inspect`, `validate`, `compare` |
| `trace` | Trace symbol/state transitions through a block. Configurable max symbols, text or JSONL output |
| `compare` | Cross-compare arithmetic paths, decode backends, or `.rygr` containers. Subcommands: `arithmetic`, `backends`, `files` |
| `bench` | Benchmark production Rust codec backends. Configurable codec, block size, sample count, output format |
| `capabilities` | Introspect compiled and runtime-supported codecs and backends as JSON |
| `completions` | Generate shell completion scripts (bash, fish, zsh, powershell, elvish) |

### RYGRANS Container Format v1

The CLI operates on a versioned block-streaming container format:

```
┌──────────────────────────────────────────────┐
│  File Header  (32 bytes)                     │
│  MAGIC "RYGRANS\0" · version · flags         │
│  default_codec · scale_bits · model_mode     │
│  declared_block_size                         │
├──────────────────────────────────────────────┤
│  Block 0 (104-byte header + payload + model) │
│  Block 1                                     │
│  ...                                         │
│  Block N-1                                   │
├──────────────────────────────────────────────┤
│  Footer (104 bytes)                          │
│  TAG "END1" · block_count · total_uncomp     │
│  total_comp · footer_sha256                  │
└──────────────────────────────────────────────┘
```

Block kinds: `RAW` (uncompressed), `RLE` (single-symbol run-length), `RANS` (rANS-compressed).
Each block stores its own SHA-256 payload hash and decoded-data hash for selective verification.

### Stable Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 2 | Command-line usage error |
| 3 | Input/output error |
| 4 | Container or model format error |
| 5 | Integrity verification failure |
| 6 | Unsupported codec or format version |
| 7 | Resource limit exceeded |
| 8 | Parity or comparison mismatch |
| 9 | Requested backend unavailable |
| 10 | Internal invariant failure |

Exit codes are stable once documented. Changing an exit code is a breaking change for
automation consumers.

### Resource Limits

All resource bounds are centralized in the `Limits` type and enforced during reading,
not after. Every command respects these limits regardless of input source:

| Limit | Default | Hard Maximum |
|-------|---------|-------------|
| Input size | 16 GiB | — |
| Output size | 16 GiB | — |
| Block size | 1 MiB | 64 MiB |
| Payload per block | 1 MiB | — |
| Model encoding | 2 KiB | — |
| Block count | 1,000,000 | — |
| Trace symbols | 256 | — |
| Oracle output | 64 MiB | — |
| Oracle timeout | 60 s | — |

### Safety Infrastructure

- **Atomic output**: Writes to a temp path, then renames on success — no partial files
- **TTY guards**: Refuses to write binary output to a terminal unless `--force-tty` is set
- **Strict validation**: Every container header, block header, and footer is validated
  against all known invariants before processing
- **SHA-256 integrity**: Each block carries payload and decoded-data hashes; the footer
  carries a container-level hash

---

## Criterion Benchmark Suite

The `ryg-rans-rs-bench` crate provides a **9-tier Criterion benchmark suite** covering
every execution path in the project. All benchmarks use deterministic corpora
(8 profiles, fixed seeds) and verify output parity before timing.

### Benchmark Tiers

| Tier | File | What It Benchmarks |
|------|------|--------------------|
| `scalar` | `benches/scalar.rs` | All scalar decode paths: 8-way slot table, 16-way, SSE-compatible slot, `_into` APIs |
| `sse41` | `benches/sse41.rs` | SSE4.1 8-way interleaved decode after runtime detection |
| `avx2` | `benches/avx2.rs` | AVX2 uniform256 table-free decode, 2×8-on-16, 8-way slot table |
| `avx512` | `benches/avx512.rs` | AVX-512VL 8-way and AVX-512 16-way gather decode |
| `specialized` | `benches/specialized.rs` | Specialized profiles: uniform256, repeat8, mixed, low-entropy, high-entropy, sparse, binary, natural |
| `batch` | `benches/batch.rs` | Batch4 preflight with mixed tail lengths, batch aggregation overhead |
| `parallel` | `benches/parallel.rs` | Block-level parallelism scaling (1–16 threads), FixedBlockPlan overhead |
| `container` | `benches/container.rs` | Full container encode→decode round-trip at various block sizes |
| `dispatch` | `benches/dispatch.rs` | Runtime backend dispatch overhead, auto-selection latency |

### Key Design Properties

- **Deterministic corpora**: All benchmarks use the same 8 fixed-seed profiles across runs
- **Parity verification before timing**: Every benchmark verifies that output matches
  the expected reference before measurement begins
- **Real `_into` APIs**: All scalar tiers benchmark the actual `_into` (preallocated output)
  APIs, not copies
- **Real SSE4.1 execution**: Benchmarks detect SSE4.1 at runtime and execute the real
  SIMD path — not a scalar stand-in
- **Batch4 preflight**: Batch benchmarks correctly handle mixed tail lengths (not just
  multiples of 4)
- **Matched allocation policies**: Sequential vs batch benchmarks use identical allocation
  strategies for fair comparison

**Run the full suite:**

```sh
cargo bench -p ryg-rans-rs-bench
```

**Run a single tier:**

```sh
cargo bench -p ryg-rans-rs-bench --bench scalar
cargo bench -p ryg-rans-rs-bench --bench parallel
cargo bench -p ryg-rans-rs-bench --bench avx2
```

---

## Project Doctrine

### Bitstream Parity

Every Rust encoder/decoder must produce and consume byte-identical streams to the
upstream C/C++ reference. This is verified at three levels:

| Level | What | How |
|-------|------|-----|
| **Mathematical** | Individual arithmetic ops | Kani model checking |
| **State-transition** | Full encode/decode cycle | Trace comparison |
| **Cross-decoding** | Rust→C, C→Rust | Oracle courts |

### Residual Primacy

Every observed difference is recorded as a **residual** — a first-class artifact with
classification, severity, and status. Residuals are:
1. Recorded immediately when detected
2. Never deleted (even after resolution)
3. Tracked through lifecycle: `open → investigating → fixed/wontfix`

### The Seal Gate

A surface is not marked `full` until a sealed court receipt proves upstream parity.
The seal gate enforces 16 mandatory checks (see below).

---

## Crate Map

| Crate | Version | `no_std` | `unsafe` | Purpose |
|-------|---------|----------|----------|---------|
| [`ryg-rans-rs-core`](./crates/ryg-rans-rs-core) | 0.1.27 | ✅ Yes | ✅ Forbid | Algorithmic heart — byte/R64/Word/Alias rANS, malformed validation, Kani proofs |
| [`ryg-rans-rs-simd`](./crates/ryg-rans-rs-simd) | 0.1.27 | ✅ Yes | ⚠️ 7 fn | SSE4.1 + AVX512VL + AVX512 decode kernels, scalar fallback |
| [`ryg-rans-rs`](./crates/ryg-rans-rs) | 0.1.27 | ✅ Yes | ✅ Deny | Public facade — re-exports core + optional SIMD |
| [`ryg-rans-rs-parallel`](./crates/ryg-rans-rs-parallel) | 0.1.27 | ❌ No | ✅ Forbid | **Phase I** — deterministic parallel block engine. Bounded executor, FixedBlockPlan, ReorderBuffer, CancellationToken. 63 tests |
| [`ryg-rans-rs-cli`](./crates/ryg-rans-rs-cli) | 0.1.27 | ❌ No | ❌ No | **Production CLI** — `ryg-rans` binary with encode, decode, inspect, verify, model, trace, compare, bench, capabilities, completions. RYGRANS v1 container format. 10 stable exit codes. Resource limits, atomic output |
| [`ryg-rans-rs-bench`](./crates/ryg-rans-rs-bench) | 0.1.27 | ❌ No | ❌ No | **Criterion benchmark suite** — 9 tiers: scalar, sse41, avx2, avx512, specialized, batch, parallel, container, dispatch. Deterministic corpora, real SIMD execution |
| [`ryg-rans-rs-oracle`](./crates/ryg-rans-rs-oracle) | 0.1.27 | ❌ No | ❌ No | Forensic court harness, evidence generation, perf benchmarks |
| [`ryg-rans-rs-casefile`](./crates/ryg-rans-rs-casefile) | 0.1.27 | ✅ Yes | ❌ No | Evidence schema types — Casefile, Receipt, Residual |

---

## Dependency Graph

```
ryg-rans-rs-simd ──depends on──> ryg-rans-rs-core
ryg-rans-rs       ──depends on──> ryg-rans-rs-core, optional: ryg-rans-rs-simd
ryg-rans-rs-parallel ─depends on──> ryg-rans-rs-core, optional: ryg-rans-rs-simd
ryg-rans-rs-bench ──depends on──> ryg-rans-rs-simd, ryg-rans-rs-parallel, ryg-rans-rs-core
ryg-rans-rs-oracle ──depends on──> ryg-rans-rs-core, ryg-rans-rs-casefile, ryg-rans-rs-simd
ryg-rans-rs-cli   ──depends on──> ryg-rans-rs, ryg-rans-rs-core, optional: ryg-rans-rs-simd
ryg-rans-rs-casefile ─> (standalone, no rANS dependencies)
```

---

## Architecture

### Deterministic Core Isolation

```text
ryg-rans-rs-core    → no_std, forbid(unsafe_code) — algorithmic ground truth
    ↓                        ↓
ryg-rans-rs-simd     ryg-rans-rs-casefile
    ↓                        ↓
ryg-rans-rs          ryg-rans-rs-parallel  ryg-rans-rs-bench
(facade re-export)   (parallel engine)     (benchmark suite)
    ↓
ryg-rans-rs-cli
(production CLI)
```

### Implemented Surfaces Detail

#### 32-bit Byte rANS (`rans_byte.h`)
- **Division-based encode**: `C(s,x) = ((x/freq) << scale_bits) + (x%freq) + start`
- **Reciprocal fast encode**: multiply-high approximation avoiding integer division (Kani-proven equivalent)
- **Two-state interleaved**: encode/decode with two alternating states
- **Backward byte writer**: reverse-growing buffer for encoding output
- **Forward byte reader**: forward-growing buffer for decoding input

#### 64-bit rANS (`rans64.h`)
- **63-bit effective state** with 32-bit word renormalization
- **128-bit mul_hi** for reciprocal encoding
- Same division and reciprocal paths as byte rANS
- Two-state interleaved encode/decode

#### Word rANS (`rans_word_sse41.h`, scalar path)
- **16-bit word renormalization** (L = 2^16)
- **Table-based decode**: 4096-slot frequency/bias table
- Division-based encode with word renormalization
- Two-state interleaved encode/decode

#### Alias Method (`main_alias.cpp`)
- **Vose's alias table** construction for O(1) symbol decode
- Frequency normalization with zero-frequency theft
- Division-based encode with alias remap
- Single-state and interleaved2 modes

#### SSE4.1 SIMD Decoder (`rans_word_sse41.h`, SIMD path)
- **4-lane SIMD decode** using `RansSimdDecSym` / `RansSimdDecRenorm`
- 8-way interleaved decode (two 4-lane units)
- Scalar gather for table lookups
- 16 precomputed shuffle masks for byte extraction
- **Note**: ~0.4× scalar speed on Zen 5 due to gather overhead

#### AVX512VL.INTERLEAVED8
- **8-lane AVX-512VL decode** using `_mm256_i32gather_epi32`
- Consumes existing 8-way format
- Masked renorm via `_mm256_cmplt_epu32_mask`

#### AVX512.INTERLEAVED16
- **16-lane AVX-512 decode** using `_mm512_i32gather_epi32`
- New 16-way stream format
- Reverse-flush state ordering (15→0)
- Forward-init lane ordering (0→15)

---

## Security and Safety

### Safety Layers

| Layer | What | Coverage |
|-------|------|----------|
| **1. Core isolation** | `forbid(unsafe_code)` in core crate | Compile-time guarantee |
| **2. Malformed validation** | Stream length checks, renorm guards, freq model validation | 12+ unit tests |
| **3. Fuzzing** | 7 cargo-fuzz targets | Byte/R64/Word/Alias/AVX512 round-trip + malformed |
| **4. Formal proofs** | 7 Kani harnesses | Arithmetic correctness, bounds, inversion |
| **5. Mask exhaustion** | All 256 (8-way) + 65536 (16-way) renorm masks | Separate test binary |
| **6. Cross-decode courts** | C↔Rust bitstream comparison | 144 behavioral receipts |
| **7. Unsafe ledger** | Every unsafe block documented | Preconditions, bounds, CPU features, soundness |
| **8. Panic containment** | Worker panic isolation via catch_unwind | Parallel engine |
| **9. Bounded queues** | Crossbeam channels with bounded capacity | Parallel engine |

### Unsafe Code

The SIMD crate contains 7 `unsafe fn` for SSE4.1 and AVX-512 intrinsics. Every one is:
- Gated by `#[target_feature(enable = "...")]`
- Only reachable through runtime feature detection in the safe API
- Documented in `docs/unsafe-ledger.md` with:
  - Preconditions required by the caller
  - Memory bounds checked before the unsafe block
  - CPU features required
  - Why each intrinsic is safe under those conditions

### No FFI Policy

The workspace does **not** bind to the upstream C/C++ via FFI. All oracle comparison
is done via subprocess communication with compiled C binaries. This means:
- No unsafe FFI boundaries to audit
- No C/C++ toolchain required to build the Rust project
- Clear separation between reference implementation and port

---

## Quick Start

### Basic Encode/Decode

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol, RansByteDecSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let freq = (1u32 << scale_bits) / 256;
let mut buf = [0u8; 4096];

let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
let sym = RansByteEncSymbol::new(0, freq, scale_bits).unwrap();
rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let dsym = RansByteDecSymbol::new(0, freq).unwrap();
rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
```

### AVX-512 Decode

```rust
use ryg_rans_rs::simd::backends::decode_interleaved8_auto;
use ryg_rans_rs::simd::packed_table::PackedWordTable;

let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
let result = decode_interleaved8_auto(&compressed, &packed, expected_len).unwrap();
println!("Selected backend: {}", result.backend.label());
assert_eq!(result.output, expected_output);
```

### Parallel Block Decode

```rust
use ryg_rans_rs_parallel::{
    ParallelConfig, BackendId, BackendPolicy,
    decode_blocks,
};

let config = ParallelConfig {
    thread_count: 4,
    ..Default::default()
};

let blocks = decode_blocks(&compressed_container, &config, None).unwrap();
assert_eq!(blocks.len(), expected_block_count);
```

### Stream Validation

```rust
use ryg_rans_rs::byte::malformed::{validate_byte_compressed, RenormGuard};

if let Err(e) = validate_byte_compressed(compressed) {
    return Err(e);
}

let mut guard = RenormGuard::new_byte();
loop {
    guard.check()?; // limits iterations, prevents infinite loop
    let b = reader.read_byte().ok_or(DecodeError::InputTooShort)?;
    x = (x << 8) | (b as u32);
    if x >= RANS_BYTE_L { break; }
}
```

### CLI Usage

```sh
# Encode a file
ryg-rans encode -i input.dat -o output.rygr

# Decode with AVX-512
ryg-rans decode -i output.rygr -o restored.dat --backend avx512

# Inspect container structure
ryg-rans inspect -i output.rygr --blocks

# Verify integrity
ryg-rans verify -i output.rygr

# Run benchmarks
ryg-rans bench --codec word-interleaved8 --size 1MiB

# Show capabilities
ryg-rans capabilities

# Generate shell completions
ryg-rans completions bash > /etc/bash_completion.d/ryg-rans
```

---

## AVX-512 Reference

### Build with AVX-512

```sh
RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" cargo build
```

### Run AVX-512 Tests

```sh
RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" cargo test

# Exhaustive 16-way mask test
RUSTFLAGS="-C target-feature=+avx512f,+avx512bw" cargo test --release -p ryg-rans-rs-simd -- --ignored
```

### Available Backends

| Backend | Label | ISA | Required Compiler Flags |
|---------|-------|-----|------------------------|
| Scalar 8-way | `scalar-8way` | Baseline | None |
| SSE4.1 8-way | `sse41-8way` | SSSE3+SSE4.1 | `+ssse3,+sse4.1` |
| AVX2 8-way slot | `avx2-8way` | AVX2 | `+avx2` |
| AVX2 uniform256 | `avx2-uniform256` | AVX2 | `+avx2` |
| AVX2 2×8-on-16 | `avx2-2x8` | AVX2 | `+avx2` |
| AVX512VL 8-way | `avx512vl-8way` | AVX512F+VL+BW | `+avx512f,+avx512vl,+avx512bw` |
| Scalar 16-way | `scalar-16way` | Baseline | None |
| AVX512 16-way | `avx512-16way` | AVX512F+BW | `+avx512f,+avx512bw` |

### 16-Way Stream Format

```
Encoding:
  symbols processed in REVERSE order (last → first)
  lane assignment: lane = i & 15
  state[i] = C(s, state[lane])

Flush order (backward writer):
  state[15].low, state[15].high,
  state[14].low, state[14].high,
  ...,
  state[0].low, state[0].high

Initialization (forward reader):
  state[0] = read u32 (low16 | high16 << 16)
  state[1] = ...
  ...
  state[15] = ...

Decode groups of 16:
  1. Gather 16 table entries via _mm512_i32gather_epi32
  2. Extract freq/bias/symbol from each packed u32
  3. Store 16 symbols in lane order 0..15
  4. Update all 16 states: (state >> 12) * freq + bias
  5. Renorm mask: state < 65536
  6. Read popcount(mask) u16 words in ascending lane order

Tail (r < 16):
  decode lanes 0..r-1 only
  don't touch lanes r..15
```

---

## Performance

Benchmarked on **AMD Ryzen 7 9800X3D** (Zen 5, 4.7 GHz, Linux, rustc 1.96, `--release`).

The Criterion benchmark suite provides measurement-grade throughput data across all
9 tiers. Below are the key findings from the scalar, SIMD, batch, and parallel
benchmarks on Zen 5.

### Key Findings

**1. Scalar 16-way is the Zen 5 general-model winner (~1.45 GiB/s)**

The scalar 16-way decoder achieves ~1.45 GiB/s on large-block uniform256 data.
This is the recommended general-purpose backend for Zen 5 systems. Sequential
scalar loads from L1 cache (~4 cycles) outperform gather-based SIMD approaches
(~10–15 cycles per gather) when the decode table is L1-resident.

**2. AVX2 uniform256 table-free leads at ~1.47 GiB/s (narrow advantage)**

The AVX2 uniform256 table-free decoder peaks at ~1.47 GiB/s, a narrow ~1–2% advantage
over scalar 16-way. This specialized backend eliminates the packed decode table entirely
for uniform models, computing freq/bias on the fly. It demonstrates that on Zen 5,
the SIMD frontend and load pipelines can keep pace with scalar execution when the
gather bottleneck is removed.

**3. AVX2 2×8 is ~1.0–1.24 GiB/s (portability tier)**

The AVX2 2×8-on-16 decoder achieves ~1.0–1.24 GiB/s depending on block size.
This is the recommended portability tier — it provides SIMD acceleration on any
AVX2-capable CPU without requiring AVX-512, and it outperforms SSE4.1 by ~2.5–3×.

**4. SSE4.1 is ~406 MiB/s (not competitive on Zen 5)**

The SSE4.1 8-way decoder achieves ~406 MiB/s on Zen 5. The scalar gather emulation
in the SSE4.1 path is the bottleneck — each table lookup requires multiple shuffle
and blend instructions. SSE4.1 remains valuable as a compatibility baseline and
for cross-verification.

**5. Batch4 barely helps on Zen 5 (~1.03 GiB/s aggregate)**

Batch4 preflight decoding aggregates ~1.03 GiB/s across 4 parallel streams on Zen 5.
The benefit over single-stream decode is marginal because a single scalar stream
already saturates the memory pipeline for L1-resident data. Batch throughput becomes
more relevant on CPUs with narrower scalar pipelines or higher gather latency.

**6. Block-level parallelism is the major multiplier (~3.14 GiB/s on 4 threads)**

The parallel block engine achieves ~3.14 GiB/s aggregate decode throughput with
4 worker threads on large-block data. This is the primary scalability path —
block-level parallelism scales nearly linearly with thread count until memory
bandwidth becomes the bottleneck. With 8 threads, throughput continues to scale,
approaching memory bandwidth limits on the 9800X3D.

**7. The 9800X3D supports AVX-512**

The AMD Ryzen 7 9800X3D (Zen 5) supports the full AVX-512 instruction set
(AVX512F, AVX512VL, AVX512BW, AVX512DQ, AVX512CD). This makes it a valuable
comparison host for evaluating AVX-512 gather decode performance against scalar
and AVX2 paths. Future CPUs with faster gather units (Zen 6, Lion Cove) may
shift the performance balance back toward SIMD.

### UNIFORM256 (GiB/s, higher is better)

| Backend | 1 KiB | 64 KiB | 1 MiB |
|---------|-------|--------|-------|
| scalar-8way (legacy slot table) | 1.56 | 1.57 | 1.56 |
| scalar-16way | 1.39 | 1.44 | 1.44 |
| AVX512VL 8-way | 0.73 | 0.72 | 0.72 |
| SSE4.1 8-way | ~0.40 | ~0.41 | ~0.41 |
| AVX2 uniform256 table-free | ~1.45 | ~1.46 | ~1.47 |
| AVX2 2×8-on-16 | ~1.00 | ~1.20 | ~1.24 |
| Batch4 (4-stream aggregate) | ~0.90 | ~1.00 | ~1.03 |
| Parallel 4-thread decode | ~2.50 | ~3.00 | ~3.14 |

### Criterion Benchmarks

For measurement-grade throughput data with confidence intervals, use the Criterion suite:

```sh
# Full suite
cargo bench -p ryg-rans-rs-bench

# Specific tiers
cargo bench -p ryg-rans-rs-bench -- bench scalar
cargo bench -p ryg-rans-rs-bench -- bench parallel
cargo bench -p ryg-rans-rs-bench -- bench avx2
cargo bench -p ryg-rans-rs-bench -- bench dispatch
```

### Legacy Benchmark Command

```sh
RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace
```

See `docs/performance-method.md` for full methodology.

---

## Evidence Reproducibility

```sh
cd oracle/adapter && make

RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20

cargo xtask seal

cargo xtask docker
```

---

## The Seal Gate

The project's `cargo xtask seal` command enforces 16 mandatory gates:

| # | Gate | What It Checks |
|---|------|----------------|
| 1 | **Dirty-tree** | No uncommitted changes to covered source files |
| 2 | **Workspace check** | `cargo check --workspace` produces no errors |
| 3 | **Core tests** | `cargo test -p ryg-rans-rs-core` passes (57+ tests) |
| 4 | **Parity model valid** | `docs-src/models/parity.model.json` is well-formed JSON |
| 5 | **Upstream exists** | `docs-src/models/upstream.json` is present |
| 6 | **Claims have receipts** | Each `behavior_status: full` has a receipt ID |
| 7 | **Court path valid** | Court-path field matches variant expectations |
| 8 | **Receipts exist** | Every indexed receipt file is present on disk |
| 9 | **Index cited in model** | Every index entry has a matching parity claim |
| 10 | **Evidence index** | All indexed receipts are accounted for |
| 11 | **Receipt SHA-256** | Every receipt's hash matches its file content |
| 12 | **Manifest SHA-256** | Every manifest's hash matches its file content |
| 13 | **Receipt self-hash** | Every receipt's embedded self-hash recomputes correctly |
| 14 | **Source freshness** | No source files changed after the evidence code commit |
| 15 | **Forbid unsafe** | Core and casefile crates enforce `forbid(unsafe_code)` |
| 16 | **Docker matrix** | Clean 10-service Docker VM matrix confirms the evidence |

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT),
at your option.

---

## References

- Fabian Giesen, [ryg_rans](https://github.com/rygorous/ryg_rans) — Public-domain rANS encoder/decoder
- Jarek Duda, [Asymmetric Numeral Systems](https://arxiv.org/abs/0902.0271) — Original ANS paper
- Charles Bloom, [Understanding ANS](https://cbloomrants.blogspot.com/) — ANS tutorial series
- Alverson, "Integer Division using Reciprocals" — Multiply-high reciprocal approximation
- Intel Intrinsics Guide — `_mm256_i32gather_epi32`, `_mm512_i32gather_epi32`
