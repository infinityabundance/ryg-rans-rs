# ryg-rans-rs

> **A native Rust forensic reconstruction of Fabian Giesen's public-domain `ryg_rans`**
>
> **158 sealed behavioural receipts · 10 performance receipts** across 7 algorithmic surfaces
>
> **Phases A–G:** Byte rANS · 64-bit rANS · Word rANS · Alias method · SSE4.1 · AVX512VL · AVX512
>
> **Phases H–J:** AVX2 portability tier · Batch4 · 2×8-on-16 · Uniform256 table-free
>
> **Phase I:** Deterministic parallel block engine — bounded executor, atomic reorder commit, cancellation completeness
>
> **Phase L:** strict decoded-hash integrity · live bounded pipeline · exact backend semantics · machine-verified unsafe ledger · fully wired CLI
>
> **Phase M/N:** custodian documentation · navigation · knowledge architecture · technical publications
>
> **Nine-tier Criterion benchmark suite** — scalar · SSE4.1 · AVX2 · AVX-512 · specialized · batch · parallel · block-engine · dispatch
>
> **Eleven-service Docker VM matrix** verifies every build, test, oracle, court, and audit

[![Rust](https://img.shields.io/badge/rust-1.85%2B-stable)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

---

## What This Repository Is (N.16 identity)

This is not merely "a Rust implementation of rANS".  It is five artifacts
in one tree, each claim below linking to the evidence:

| Identity | Where it lives | How it is verified |
|----------|----------------|--------------------|
| **A production-capable implementation** | the crates in `crates/`, the CLI | the workspace test suite; the Docker matrix; the seal |
| **A long-term implementation reference** | `docs/papers/`, `docs/adr/`, the module/function/section/line commentary | the documentation-inventory and documentation-links seal gates |
| **A reproducible verification corpus** | `evidence/` — 158 behavioural + 10 performance receipts, manifests, indexes | every receipt's file and canonical hash verified by the seal |
| **A benchmark corpus** | `crates/ryg-rans-rs-bench/`, `evidence/performance/` (run `phase-l-20260802e`, 800 cases × 100 samples) | the performance-evidence gates (run-manifest binding, preflight join, throughput derivation) |
| **An educational resource** | `docs/education.md`, `docs/navigation/` (guides, maps, paths) | the navigation-completeness gates (N.14/N.21) |
| **A case study in evidence-backed LLM-assisted systems engineering** | `docs/papers/0008`, `docs/llm/`, `docs/history/`, `docs/story/`, `docs/failures/` | the paper/article inventory gates |

---

## Entry Points (N.3 portal)

* **I'm completely new** → [`docs/navigation/00-first-day.md`](docs/navigation/00-first-day.md)
* **I'm evaluating the library** → [`docs/navigation/10-security-review.md`](docs/navigation/10-security-review.md) and [`docs/negative-capabilities.md`](docs/negative-capabilities.md)
* **I want maximum performance** → [`docs/navigation/03-performance-engineer.md`](docs/navigation/03-performance-engineer.md)
* **I want to understand rANS** → [`docs/papers/0001-rans-design.md`](docs/papers/0001-rans-design.md)
* **I'm modifying SIMD** → [`docs/navigation/04-simd-engineer.md`](docs/navigation/04-simd-engineer.md)
* **I'm modifying decode** → [`docs/navigation/05-parallel-engineer.md`](docs/navigation/05-parallel-engineer.md)
* **I'm modifying the CLI** → [`docs/navigation/08-cli-engineer.md`](docs/navigation/08-cli-engineer.md)
* **I'm modifying evidence generation** → [`docs/navigation/07-evidence-engineer.md`](docs/navigation/07-evidence-engineer.md)
* **I'm reviewing safety** → [`docs/navigation/10-security-review.md`](docs/navigation/10-security-review.md)
* **I'm contributing** → [`docs/contributing/`](docs/contributing/)

Every reader starts at `docs/navigation/00-first-day.md`; the guides route
from there.  The learning maps (`docs/navigation/maps/`) show dependency
structure at a glance.

---

## Table of Contents

1. [Reading Order](#reading-order)
2. [Overview](#overview)
3. [Evidence Status](#evidence-status)
4. [CLI — the `ryg-rans` Command](#cli--the-ryg-rans-command)
5. [Criterion Benchmark Suite](#criterion-benchmark-suite)
6. [Project Doctrine](#project-doctrine)
7. [Crate Map](#crate-map)
8. [Architecture](#architecture)
9. [Security and Safety](#security-and-safety)
10. [Quick Start](#quick-start)
11. [AVX-512 Reference](#avx-512-reference)
12. [Performance](#performance)
13. [Evidence Reproducibility](#evidence-reproducibility)
14. [The Seal Gate](#the-seal-gate)
15. [License](#license)

---

## Reading Order

1. This README.
2. [`docs/navigation/00-first-day.md`](docs/navigation/00-first-day.md) — the one-hour orientation.
3. [`docs/philosophy.md`](docs/philosophy.md) — the documentation constitution.
4. [`docs/architecture.md`](docs/architecture.md)
5. [`docs/layers.md`](docs/layers.md) — the layered documentation architecture.
6. [`docs/bitstream-contract.md`](docs/bitstream-contract.md) — the pinned upstream stream formats.
7. [`docs/container-format-v1.md`](docs/container-format-v1.md) — the RYGRANS v1 container.
8. [`docs/glossary.md`](docs/glossary.md) — the exact project terminology; every document uses it.
9. [`docs/unsafe-ledger.md`](docs/unsafe-ledger.md)
10. [`docs/performance-method.md`](docs/performance-method.md)
11. [`docs/residual-doctrine.md`](docs/residual-doctrine.md)
12. [`docs/negative-capabilities.md`](docs/negative-capabilities.md)
13. [`docs/oracle-method.md`](docs/oracle-method.md)
14. [`docs/papers/`](docs/papers/) — the eight long-form design papers (rANS, word coder, SIMD, parallel engine, performance methodology, evidence, proof philosophy, LLM-assisted engineering).
15. [`docs/articles/`](docs/articles/) — the standalone publishable articles.
16. [`docs/adr/`](docs/adr/) — the architecture decision records.
17. [`docs/history/`](docs/history/) — the chronological engineering record.
18. [`docs/story/`](docs/story/) — the engineering story.
19. [`docs/failures/`](docs/failures/) — the failure encyclopedia.
20. [`docs/diagrams/`](docs/diagrams/) — the architecture diagrams.
21. [`docs/atlas/`](docs/atlas/) — the architecture atlas.
22. [`docs/education.md`](docs/education.md) — reading orders and future-maintainer notes.
23. [`docs/navigation/`](docs/navigation/) — guides, maps, the knowledge graph, and search indexes.
24. [`docs/llm/`](docs/llm/) — the LLM-assisted engineering operational record.
25. [`docs/contributing/`](docs/contributing/) — contributor experience.
26. [`AGENTS.md`](AGENTS.md) — ground truth for AI agents and contributors.
27. The crate README for whichever crate you are changing.

---

## Overview

**ryg-rans-rs** is a from-scratch, native Rust implementation of the Asymmetric
Numeral Systems (ANS) entropy coder variants published in Fabian "ryg" Giesen's
[ryg_rans](https://github.com/rygorous/ryg_rans) repository.

### What makes this project different

This is **not** a wrapper, binding, or FFI facade. It is a **forensic
reconstruction** of the observable arithmetic, state-transition, bitstream,
and interleaving behavior of the pinned upstream revision, built through
parity courts:

1. Every arithmetic operation is compared against the compiled C/C++ oracle
2. Every encoded byte stream is verified byte-for-byte in both directions
3. Every observed difference is a first-class **residual** — tracked, classified, resolved
4. Every surface is sealed by a **SHA-256-chained receipt** with self-hash verification
5. Every release requires a **Docker VM matrix run** with 11 services

### What this project covers

| Surface | Approach | Status |
|---------|----------|--------|
| 32-bit byte rANS | Division + reciprocal encode/decode | ✅ **Sealed** (44 behavioural receipts) |
| 64-bit rANS | Division + reciprocal, 128-bit mul_hi | ✅ **Sealed** (44) |
| Two-state interleaving | Byte + R64 + Word | ✅ **Sealed** |
| Word rANS (table-based) | 16-bit renorm, 4096-slot table | ✅ **Sealed** (16) |
| Alias method (Vose) | O(1) decode, byte rANS | ✅ **Sealed** (16) |
| SSE4.1 SIMD | 4-lane, 8-way interleaved | ✅ **Sealed** (8) |
| **AVX512VL.INTERLEAVED8** | **8-way AVX-512VL gather decode** | ✅ **Sealed** (8) |
| **AVX512.INTERLEAVED16** | **16-way AVX-512 gather decode** | ✅ **Sealed** (8) |
| **Phase H optimization backends** | **2×8-on-16 · manual gather · uniform256 table-free** | ✅ **Test-verified** |
| **Phase J AVX2 backends** | **AVX2 portability tier** | ✅ **Test-verified** |
| **Phase I — Parallel block engine** | **Bounded executor, fixed-block plan, ordered commit** | ✅ **Test-verified** |
| **Phase O — Model artifact cache** | **Explicitly owned, exact-accounting, single-flight model cache + public-corpus workloads** | ✅ **Sealed** (9 behavioural courts + 5 performance receipts) |
| **CLI** | **`ryg-rans` with 10 wired subcommands** | ✅ **Implemented** (20 CLI tests) |

The behavioural counts above are the sealed totals: **158 receipts** (144
oracle/upstream-parity courts + 14 Phase L behavioural courts); the README
table is regenerated from the evidence index by the seal machinery (never
hand-edited).  Phase O adds **9 behavioural courts** (`RYG_RANS.O.CACHE.*`,
`RYG_RANS.O.WORKLOAD.PUBLIC_RANS_V1`) and **5 performance receipts**
(`RYG_RANS.PERF.CACHE.*`) — see the Evidence Status table.

---

## Evidence Status
| Surface | Behaviour | Performance | Behaviour Receipts | Performance Receipts |
|---------|-----------|-------------|------------------:|--------------------:|
| 32-bit byte rANS — division + reciprocal | **Sealed** | **Sealed** | 44 | 1 |
| 64-bit rANS — division + reciprocal | **Sealed** | **Sealed** | 44 | 1 |
| Word rANS — scalar table-based | **Sealed** | **Sealed** | 16 | 1 |
| Alias method — Vose table, byte rANS | **Sealed** | **Sealed** | 16 | 1 |
| SSE4.1 SIMD decoder — 8-way interleaved | **Sealed** | **Sealed** | 0 | 1 |
| AVX512VL.INTERLEAVED8 | **Sealed** | **Sealed** | 8 | 1 |
| AVX512.INTERLEAVED16 | **Sealed** | **Sealed** | 8 | 1 |
| Phase H optimization backends | **Test-verified** | **Sealed** | 0 | 1 |
| Phase J AVX2 backends | **Test-verified** | **Sealed** | 0 | 1 |
| Phase I parallel block engine | **Test-verified** | **Sealed** | 0 | 1 || Phase L behavioural courts | **Sealed** | — | 14 | 0 || Phase O cache courts | **Sealed** | **Sealed** | 9 | 5 || **Total** | | | **167** | **15** |
## CLI — the `ryg-rans` Command

The `ryg-rans` binary implements the RYGRANS v1 container format
(`docs/container-format-v1.md`) with stable exit codes, resource limits, and
strict hash verification.  All subcommands are wired and integration-tested
(17 end-to-end tests + 5 normalizer tests + 1 proptest).  Long-running
operations are cancellable from outside: SIGINT/SIGTERM and a `--timeout N`
watchdog (fractional seconds) return the typed `Cancelled` error (exit 11) at
the next block boundary instead of a hard kill.

### Subcommands

| Command | Status | Notes |
|---------|--------|-------|
| `encode` | ✅ Implemented | Streaming block encode; RLE / rANS / RAW selection per block. Codecs: `byte-single`, `byte-interleaved2` (default), `r64-single`, `word-single`. Other codecs → typed error, exit 6. |
| `decode` | ✅ Implemented | Strict integrity walk: payload hash, decoded-data hash, container hash, decoded-stream hash all verified; any mismatch → exit 5. Codecs 1, 2, 3, 5 and 7 (8-way via SIMD/scalar); 4, 6, 8, 9, 10 → typed error. |
| `inspect` | ✅ Implemented | Human or JSON metadata; `--deep` decodes and verifies every block. |
| `verify` | ✅ Implemented | Full verification without writing output; per-container summary; exit 5 on any failure. |
| `model` | ✅ Implemented | `build` (deterministic normalizer), `inspect`, `validate`, `compare` (binary or JSON). |
| `trace` | ✅ Implemented | Per-symbol state transitions for `byte-single` blocks; other codecs → typed error. |
| `compare` | ✅ Implemented | `arithmetic` (division vs reciprocal, byte-identical), `backends` (scalar vs SIMD 8-way), `files` (decoded-stream hash equality). |
| `bench` | ✅ Implemented | In-process throughput with round-trip preflight; the Criterion suite remains the sealed measurement surface. |
| `capabilities` | ✅ Implemented | Compiled and runtime codec/backend inventory as JSON. |
| `completions` | ✅ Implemented | bash, fish, zsh, powershell, elvish. |

### RYGRANS Container Format v1

The CLI operates on a versioned block-streaming container format
(full specification in [`docs/container-format-v1.md`](docs/container-format-v1.md)):

```text
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

Block kinds: `RAW` (uncompressed), `RLE` (single-symbol run-length), `RANS`
(rANS-compressed).  Each block stores its own SHA-256 payload hash and
decoded-data hash; strict integrity requires both to match (a zero/unset
decoded hash fails).  The footer carries a container-level hash and the
decoded-stream hash.

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
| 11 | Operation cancelled (signal, timeout, or caller request) |

Exit codes are stable once documented and are propagated verbatim by the
binary (including code 6, which is reachable since Phase L.15, and code 11,
which is reachable since Phase L.3-D — SIGINT/SIGTERM or `--timeout` on
`encode`, `decode`, and `verify`).

### Resource Limits

All resource bounds are centralized in the `Limits` type and enforced during
reading, not after (see `crates/ryg-rans-rs-cli/src/limits.rs`):

| Limit | Default | Hard Maximum |
|-------|---------|-------------|
| Input size | 16 GiB | — |
| Output size | 16 GiB | — |
| Block size | 1 MiB | 64 MiB |
| Payload per block | 1 MiB | — |
| Model encoding | 2 KiB | — |
| Block count | 1,000,000 | — |
| Trace symbols | 256 | — |

### Safety Infrastructure

- **Atomic output**: refuses to overwrite without `--force`; writes complete
  containers only.
- **TTY guards**: refuses binary output to a terminal unless `--force-tty`.
- **Strict validation**: every container header, block header, footer, model,
  and hash is validated before any output is produced.
- **No silent fallback**: an explicit `--backend` request or unsupported codec
  returns a typed error (exit 6/9), never a different code path.
- **Bounded I/O**: input reads are capped by the resource limits during
  reading.

---

## Criterion Benchmark Suite

The `ryg-rans-rs-bench` crate provides a **9-tier Criterion benchmark suite**
covering every execution path in the project. All benchmarks use deterministic
corpora (8 profiles, fixed seeds) and verify output parity before timing.

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
| `byte_rans` / `r64` / `alias` | legacy surfaces | Byte/R64/Alias division + reciprocal, interleaved2, model construction |
| `comparative` | `benches/comparative.rs` | Phase L.14 same-host court vs upstream C via `ryg-rans-sys` (=1.2.0) |
| `model_cache` | `benches/model_cache.rs` | Phase O model artifact cache: construction + cache-op microbenchmarks; end-to-end disabled/cold/warm/hot-set/thrash/unique decode × 6 sizes × 1–32 workers; public-corpus natural/grouped modes when the workload cache is present. Every case emits a mode-proving preflight record. |

### Workloads (Phase O)

The model-cache and cache-behavior benchmarks consume a deterministic,
versioned **rANS workload derivation** from public corpora — Canterbury,
enwik8/enwik9, and Pizza & Chili (15 pinned sources; hashes cross-validated
with the publishers).  The corpus bytes are never committed; the pinned
identity, rights record, derivation policy, and tooling are
(`workloads/public-rans-v1/`).  `cargo xtask workload
policy-sim` reproduces the FIFO-vs-LRU eviction evidence (ADR-0017).
Measured cache behavior is reported in `docs/performance/model-cache.md`.

**Two execution families (post-v0.5.0 audit, `MODEL_CACHE.WORKLOAD.2`):**
`synthetic-cache-stress` / `synthetic-cache-soak` (aliases `stress` /
`soak`) run the cache-behaviour classes on deterministic xorshift payloads
and are labeled `synthetic-cache-stress-v1`; `stress-public` /
`soak-public` execute the derived manifest schedule itself — every block
resolves `source_id + source_sha256 + offset + length` to hash-verified
extracted source bytes, with `--schedule` selecting the executed schedule
(smoke/1g/mixed-16g/stress-64g) and bounded-window streaming so the 16/64
GiB logical schedules never materialize.  Only the public family may claim
corpus provenance.

### Key Design Properties

- **Deterministic corpora**: All benchmarks use the same 8 fixed-seed profiles across runs
- **Parity verification before timing**: Every benchmark verifies that output matches
  the expected reference before measurement begins
- **Real `_into` APIs**: All scalar tiers benchmark the actual `_into` (preallocated output)
  APIs, not copies
- **Real SSE4.1/AVX2/AVX-512 execution**: Benchmarks detect ISA at runtime and execute the
  real SIMD path — not a scalar stand-in
- **Batch4 preflight**: Batch benchmarks correctly handle mixed tail lengths (not just
  multiples of 4)
- **Matched allocation policies**: Sequential vs batch benchmarks use identical allocation
  strategies for fair comparison

**Run the full suite:**

```sh
RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench
```

See [`docs/performance-method.md`](docs/performance-method.md) for methodology.

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

The Phase L.14 comparative court additionally proved byte-identical compressed
output between the Rust core and the upstream C implementation (via
`ryg-rans-sys`) for both byte and word rANS, before any timing.

### Residual Primacy

Every observed difference is recorded as a **residual** — a first-class artifact with
classification, severity, and status. Residuals are:
1. Recorded immediately when detected
2. Never deleted (even after resolution or supersession)
3. Tracked through lifecycle: `open → investigating → fixed/wontfix`

The full ledger is [`evidence/phase-l/gap-ledger.md`](evidence/phase-l/gap-ledger.md).

### The Seal Gate

A surface is not marked **Sealed** until a sealed court receipt proves upstream parity
and the complete seal gate passes (L.20: 40 gates). The seal gate enforces the checks
listed below.

---

## Crate Map

| Crate | Version | `no_std` | `unsafe` | Purpose |
|-------|---------|----------|----------|---------|
| [`ryg-rans-rs-core`](./crates/ryg-rans-rs-core) | 0.5.1 | ✅ Yes | ✅ Forbid | Algorithmic heart — byte/R64/Word/Alias rANS, malformed validation, Kani proofs |
| [`ryg-rans-rs-simd`](./crates/ryg-rans-rs-simd) | 0.5.1 | ✅ Yes | ⚠️ Ledgered | SSE4.1 + AVX512VL + AVX512 decode kernels, scalar fallback; every `unsafe fn` in `unsafe-ledger.toml` |
| [`ryg-rans-rs`](./crates/ryg-rans-rs) | 0.5.1 | ✅ Yes | ✅ Deny | Public facade — re-exports core + optional SIMD |
| [`ryg-rans-rs-parallel`](./crates/ryg-rans-rs-parallel) | 0.5.1 | ❌ No | ✅ Forbid | **Phase I** — deterministic parallel block engine: bounded live executor, atomic reorder commit, scratch, model cache, exact backend planning (105 tests) |
| [`ryg-rans-rs-cli`](./crates/ryg-rans-rs-cli) | 0.5.1 | ❌ No | ⚠️ Signals-gated | **`ryg-rans` binary** — 10 wired subcommands, RYGRANS v1 container, 11 stable exit codes, SIGINT/SIGTERM/`--timeout` cancellation, resource limits, strict integrity (23 tests) |
| [`ryg-rans-rs-bench`](./crates/ryg-rans-rs-bench) | 0.5.1 | ❌ No | ❌ No | **Criterion benchmark suite** — 9 tiers + legacy surfaces + Phase L.14 comparative court. `publish = false` |
| [`ryg-rans-rs-oracle`](./crates/ryg-rans-rs-oracle) | 0.5.1 | ❌ No | ❌ No | Forensic court harness, evidence generation |
| [`ryg-rans-rs-casefile`](./crates/ryg-rans-rs-casefile) | 0.5.1 | ✅ Yes | ❌ No | Evidence schema types — Casefile, Receipt, Residual |

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

#### Word rANS (`rans_word_sse41.h`, scalar path)
- **16-bit word renormalization** (L = 2^16)
- **Table-based decode**: 4096-slot frequency/bias table
- Division-based encode with word renormalization

#### Alias Method (`main_alias.cpp`)
- **Vose's alias table** construction for O(1) symbol decode
- Frequency normalization with zero-frequency theft
- Division-based encode with alias remap
- Single-state and interleaved2 modes

#### SSE4.1 SIMD Decoder (`rans_word_sse41.h`, SIMD path)
- **4-lane SIMD decode** using `RansSimdDecSym` / `RansSimdDecRenorm`
- 8-way interleaved decode (two 4-lane units)
- 16 precomputed shuffle masks for byte extraction
- Since Phase L.10 every SIMD helper carries its own `#[target_feature]`
  attributes and is listed in the machine-verified unsafe ledger

#### AVX512VL.INTERLEAVED8
- **8-lane AVX-512VL decode** using `_mm256_i32gather_epi32`
- Consumes the existing 8-way format
- Masked renorm via `_mm256_cmplt_epu32_mask`

#### AVX512.INTERLEAVED16
- **16-lane AVX-512 decode** using `_mm512_i32gather_epi32`
- 16-way stream format
- Reverse-flush state ordering (15→0); forward-init lane ordering (0→15)

---

## Security and Safety

### Safety Layers

| Layer | What | Coverage |
|-------|------|----------|
| **1. Core isolation** | `forbid(unsafe_code)` in core crate | Compile-time guarantee |
| **2. Malformed validation** | Stream length checks, renorm guards, freq model validation | Unit tests + fuzz targets |
| **3. Fuzzing** | cargo-fuzz targets | Byte/R64/Word/Alias/AVX512 round-trip + malformed |
| **4. Formal proofs** | Kani harnesses | Arithmetic correctness, bounds, inversion |
| **5. Mask exhaustion** | All 256 (8-way) + 65536 (16-way) renorm masks | Separate test binary |
| **6. Cross-decode courts** | C↔Rust bitstream comparison | 144 oracle + 14 Phase L behavioural receipts |
| **7. Unsafe ledger** | Every unsafe function documented | Bidirectional ledger↔source test + disassembly courts |
| **8. Panic containment** | Worker panic isolation via catch_unwind | Parallel engine |
| **9. Bounded queues** | Bounded job/result channels with live coordinator | Parallel engine |
| **10. Strict integrity** | Payload + decoded-data hashes, zero-decoded-hash fails | Verifier, CLI, courts, evidence |

### Unsafe Code

The SIMD crate's unsafe surface is machine-verified: every `unsafe fn` is listed in
`crates/ryg-rans-rs-simd/unsafe-ledger.toml` with its exact `#[target_feature]`
attributes, and the `unsafe_ledger` test fails if ledger and source disagree. Every
function has a `# Safety` section stating pointer provenance, bounds, alignment, CPU
requirements, and its caller list. Disassembly courts assert the expected ISA
instructions are present in native builds.

### FFI Policy

**Production crates use no FFI.** The workspace does not bind to upstream C in
core/simd/parallel/cli; all oracle comparison uses subprocess communication with
compiled C binaries.  The one exception is the **bench crate's Phase L.14
comparative court**, which links the maintained `ryg-rans-sys` (=1.2.0) FFI
bindings for same-host comparison against the C reference — a measurement
surface, not a production dependency (`publish = false`).

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

External cancellation is available through the `_with_cancel` APIs
(`decode_blocks_with_cancel`, `decode_streaming_with_cancel`,
`verify_blocks_with_cancel`, `encode_blocks_with_cancel`); cancellation
returns `ParallelError::Cancelled { completed, expected }` and can never
return `Ok` with fewer blocks than declared.

### CLI Usage

```sh
# Encode a file (byte-interleaved2, 1 MiB blocks)
ryg-rans encode -i input.dat -o output.rygr

# Decode and verify (strict integrity; exit 5 on any hash mismatch)
ryg-rans decode -i output.rygr -o restored.dat

# Inspect container structure (--deep decodes and verifies every block)
ryg-rans inspect -i output.rygr --deep

# Verify integrity without writing output
ryg-rans verify -i output.rygr

# Model tooling
ryg-rans model build -i input.dat -o model.bin --output-format binary
ryg-rans model validate -i model.bin

# Trace symbol/state transitions
ryg-rans trace -i output.rygr --block 0 --max-symbols 64

# Compare division vs reciprocal encoding parity
ryg-rans compare arithmetic -i input.dat

# Smoke benchmark (the Criterion suite is the sealed surface)
ryg-rans bench --samples 50

# Show capabilities / completions
ryg-rans capabilities
ryg-rans completions bash > /etc/bash_completion.d/ryg-rans
```

---

## AVX-512 Reference

### Build with AVX-512

```sh
RUSTFLAGS="-C target-cpu=native" cargo build          # host-native
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

```text
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

The Phase K Criterion measurements below are retained as historical evidence
and are **superseded** by the Phase L.18 re-seal (see
[Evidence Status](#evidence-status)).  Phase L.17 performs the regression and
component-isolation analysis; Phase L.18 regenerates and seals the ten
performance receipts through `cargo xtask benchmark-run`.

Benchmark host: **AMD Ryzen 7 9800X3D** (Zen 5, 8 cores / 16 threads, Linux,
rustc 1.96, `--release`).

### Phase K key findings (historical, superseded)

1. Scalar 16-way was the Zen 5 general-model winner (~1.45 GiB/s uniform256).
2. AVX2 uniform256 table-free led at ~1.47 GiB/s (narrow advantage).
3. AVX2 2×8 reached ~1.0–1.24 GiB/s (portability tier).
4. SSE4.1 was ~406 MiB/s on Zen 5 (gather-emulation bound).
5. Batch4 was ~1.03 GiB/s aggregate on Zen 5.
6. Block-level parallelism scaled nearly linearly: ~3.14 GiB/s on 4 threads.
7. The 9800X3D supports full AVX-512 (F/VL/BW/DQ/CD).

The Phase L.14 comparative court (same-host, identical corpus, identical
methodology vs upstream C via `ryg-rans-sys` =1.2.0) found:

| Case | Rust core | C (FFI) | Note |
|------|-----------|---------|------|
| byte encode (reciprocal) | 541.4 MiB/s | 514.8 MiB/s | parity (1.05×) |
| byte encode (division) | 349.6 MiB/s | — | reciprocal is 1.55× faster |
| byte decode | 947.3 MiB/s | 430.2 MiB/s | ~2.0 ms/MiB of that is isolated FFI cost |
| word encode | 365.0 MiB/s | 215.7 MiB/s | C compiled without `-march=native` (L14-A) |
| word decode | 486.4 MiB/s | 480.2 MiB/s | parity (1.01×) |
| FFI crossing | — | 1.09 ns/call | measured separately |

Full methodology and residuals: [`docs/performance/comparative.md`](docs/performance/comparative.md).

---

## Evidence Reproducibility

```sh
cd oracle/adapter && make

RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20

cargo xtask seal
```

Performance evidence is generated by the Phase L.18 pipeline:

```sh
cargo xtask benchmark-run --criterion-dir target/criterion \
  --implementation-commit "$(git rev-parse HEAD)"
cargo xtask performance-seal --criterion-dir target/criterion \
  --run-dir evidence/performance/runs/<run-id>
cargo xtask seal
```

---

## The Seal Gate

`cargo xtask seal` is the single authoritative final gate (Phase L.20: 40
checks).  It verifies, among others:

| # | Gate | What It Checks |
|---|------|----------------|
| 1 | **Dirty-tree** | No uncommitted changes to covered source files |
| 2 | **Workspace check** | `cargo check --workspace` produces no errors |
| 3 | **Core tests** | `cargo test -p ryg-rans-rs-core` passes |
| 4 | **Parity model valid** | `docs-src/models/parity.model.json` is well-formed JSON |
| 5 | **Upstream exists** | `docs-src/models/upstream.json` is present |
| 6 | **Claims have receipts** | Each `behavior_status: full` has a receipt ID |
| 7 | **Court path valid** | Court-path field matches variant expectations |
| 8 | **Receipts exist** | Every indexed receipt file is present on disk |
| 9 | **Index cited in model** | Every index entry has a matching parity claim |
| 10 | **Evidence index** | All indexed receipts are accounted for |
| 11 | **Receipt SHA-256** | Every receipt's hash matches its file content |
| 12 | **Manifest SHA-256** | Every manifest's hash matches its file content |
| 13 | **Receipt self-hash** | Every receipt's embedded self-hash recomputes (never skipped) |
| 14 | **Source freshness** | No source files changed after the evidence code commit |
| 15 | **Forbid unsafe** | Core and casefile crates enforce `forbid(unsafe_code)` |
| 16 | **Docker matrix** | Clean 11-service Docker VM matrix confirms the evidence |
| 17+ | **Performance evidence** | Top-level index, run index, receipt file hashes, canonical self-hashes, manifests, archive integrity, preflight records, backend/thread identity, README regeneration (Phase L.18/L.20) |

The gate fails on any warning affecting evidence validity and never prints
success for a skipped verification.

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
