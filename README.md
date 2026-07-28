# ryg-rans-rs

> **A native Rust forensic reconstruction of Fabian Giesen's public-domain `ryg_rans`**  
> **Four scalar single-state profiles sealed via bit-exact C↔Rust cross-decoding courts**  
> **Ten-service Docker VM matrix verifies every build, test, oracle, court, and audit**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-stable)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

---

## Overview

**ryg-rans-rs** is a from-scratch, native Rust implementation of the Asymmetric Numeral Systems (ANS) entropy coder variants published in Fabian "ryg" Giesen's seminal [ryg_rans](https://github.com/rygorous/ryg_rans) repository.

This is **not** a wrapper, binding, or FFI facade. It is a reconstruction of the **observable arithmetic, state-transition, bitstream, and interleaving behavior** of the pinned upstream revision, built through forensic parity courts.

### Current Evidence

| Surface | Behaviour Status | Performance Status | Receipt |
|---------|-----------------|-------------------|---------|
| 32-bit byte rANS division, single-state, uniform-256, scale=12 | **Sealed** | Unsealed | `RYG_RANS.BYTE.DIVISION.SINGLE_STATE.UNIFORM256.S12` |
| 32-bit byte rANS reciprocal, single-state, uniform-256, scale=12 | **Sealed** | Unsealed | `RYG_RANS.BYTE.RECIPROCAL.SINGLE_STATE.UNIFORM256.S12` |
| 64-bit rANS division, single-state, uniform-256, scale=12 | **Sealed** | Unsealed | `RYG_RANS.R64.DIVISION.SINGLE_STATE.UNIFORM256.S12` |
| 64-bit rANS reciprocal, single-state, uniform-256, scale=12 | **Sealed** | Unsealed | `RYG_RANS.R64.RECIPROCAL.SINGLE_STATE.UNIFORM256.S12` |
| 32-bit byte two-state interleaving | Implemented | — | No cross-decoding receipt yet |
| 64-bit rANS two-state interleaving | Partial (primitives only) | — | No cross-decoding receipt yet |
| Word-aligned scalar rANS | Scaffold | — | — |
| SSE4.1 SIMD decode | Scaffold | — | — |
| Alias method | Scaffold | — | — |

**Evidence structure**: Each sealed receipt is a SHA-256-chained artifact: a machine-readable `CaseManifest` (containing all deterministic input cases, frequency models, C and Rust compressed streams, and per-case verdicts) plus a `Receipt` (containing verdict, `code_commit`, `manifest_sha256`, `receipt_sha256` self-hash). The receipts are registered in `evidence/index.json` and verified by the 16-gate seal check.

### Current Limitations

- **All sealed profiles use a single model class** (uniform 256-symbol frequencies, scale=12). Generalization across skewed, sparse, prime-residue, and renormalization-boundary models is the next engineering phase.
- **Interleaving is unsealed.** The implementation exists but has no cross-decoding receipt.
- **Performance parity is compile-validated** (`cargo bench --no-run` passes) but no cycle-level measurements have been recorded.
- **Word, Alias, and SSE4.1 surfaces remain scaffolded.**

---

## Project Doctrine

> **Bitstream parity, state-transition parity, performance-shape parity, operational-knowledge parity.**

The implementation method is **forensic parity courts** governed by **residual primacy**:

- Every arithmetic operation is compared against the compiled C/C++ oracle via `oracle/adapter/rans_trace.cpp`.
- Every encoded byte stream is verified byte-for-byte across both implementations in both directions (C→Rust and Rust→C).
- Every observed difference is recorded as a **residual** — a first-class artifact that must be classified, understood, and either resolved or explicitly admitted.
- No surface is labelled `full` until a sealed court receipt proves upstream parity.
- No seal is accepted without a clean Docker VM matrix run producing the evidence.

### The Seal Gate

The project's `cargo xtask seal` command enforces 16 mandatory gates:

| # | Gate | Purpose |
|---|------|---------|
| 0 | **Dirty-tree** | No uncommitted source changes to covered paths |
| 1 | `cargo check --workspace` | Whole workspace compiles |
| 2 | `cargo test -p ryg-rans-rs-core` | 44 core algorithm tests pass |
| 3–4 | Model file validity | Parity model and upstream pin are well-formed |
| 5 | Full claims have receipts | Every `behavior_status: full` surface has a receipt ID |
| 6 | Receipt file existence | All cited receipt files exist on disk |
| 7–9 | SHA-256 chains | Receipt → index matches, manifest → receipt matches, self-hash recomputes |
| 10 | **Source freshness** | No covered source files changed after `code_commit` |
| 11–12 | `#![forbid(unsafe_code)]` | Core and casefile crates forbid unsafe |
| 13–15 | **Docker matrix** | Stamp exists, all 10 jobs present with `exit_code: 0`, `log_sha256` verified |
| — | Pre/post fingerprinting | Full Docker resource diff; hard-fail on any change to protected resources |

---

## Quick Start

```sh
# Run all workspace tests
cargo test --workspace

# Run core algorithm tests (44 tests, no std required)
cargo test -p ryg-rans-rs-core

# Verify all gates pass
cargo xtask check

# Full release seal (requires prior Docker matrix run)
cargo xtask seal
```

### Basic Usage

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol, RansByteDecSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_get,
    rans_byte_dec_advance_symbol,
};

// 1. Define a frequency model (scale_bits = 14, total = 16384)
let scale_bits = 14;
let total = 1u32 << scale_bits;
let freq_a = total / 3;
let freq_b = total / 3;
let freq_c = total - freq_a - freq_b;

let esym_a = RansByteEncSymbol::try_new(0, freq_a, scale_bits).unwrap();
let esym_b = RansByteEncSymbol::try_new(freq_a, freq_b, scale_bits).unwrap();
let esym_c = RansByteEncSymbol::try_new(freq_a + freq_b, freq_c, scale_bits).unwrap();

let dsym_a = RansByteDecSymbol::new(0, freq_a);
let dsym_b = RansByteDecSymbol::new(freq_a, freq_b);
let dsym_c = RansByteDecSymbol::new(freq_a + freq_b, freq_c);

// 2. Encode symbols in reverse order
let input = [0u8, 1, 2, 0, 1, 2, 0, 1, 2, 0];
let mut out = [0u8; 1024];
let mut writer = BackwardByteWriter::new(&mut out);

let mut state = RansByteState::new();
for &s in input.iter().rev() {
    let esym = match s { 0 => &esym_a, 1 => &esym_b, _ => &esym_c };
    rans_byte_enc_put_symbol(&mut state, &mut writer, esym).unwrap();
}
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

// 3. Decode in forward order
let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();

let cum2sym: Vec<u8> = (0..total as usize)
    .map(|i| if i < freq_a as usize { 0 }
         else if i < (freq_a + freq_b) as usize { 1 } else { 2 })
    .collect();

let mut output = vec![0u8; input.len()];
for i in 0..input.len() {
    let cf = rans_byte_dec_get(&dec_state, scale_bits);
    let s = cum2sym[cf as usize];
    output[i] = s;
    let dsym = match s { 0 => &dsym_a, 1 => &dsym_b, _ => &dsym_c };
    rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits).unwrap();
}

assert_eq!(output, input); // Round-trip complete!
```

---

## Implementation Architecture

```
ryg-rans-rs/
├── crates/
│   ├── ryg-rans-rs-core/          # no_std, forbid(unsafe_code) — algorithmic core
│   ├── ryg-rans-rs-simd/          # SSE4.1 kernels (scaffold)
│   ├── ryg-rans-rs/               # Public facade, optional simd+alloc features
│   ├── ryg-rans-rs-oracle/        # Cross-decoding court harness (published for reproducibility)
│   ├── ryg-rans-rs-casefile/      # Typed evidence schemas
│   └── ryg-rans-rs-cli/           # CLI tools (scaffold)
├── xtask/                          # Build automation & seal gate
├── oracle/adapter/                 # rans_trace.cpp — 22 C/C++ oracle operations
├── evidence/                       # SHA-256-chained receipts & manifests (git-tracked)
├── docker/                         # Docker Compose matrix (10 services)
└── docs-src/models/                # Parity & upstream machine-readable models
```

### Crate Details

| Crate | Version | Description |
|-------|---------|-------------|
| [`ryg-rans-rs-core`](crates/ryg-rans-rs-core/) | 0.1.3 | `#![no_std]` + `#![forbid(unsafe_code)]`. 32-bit and 64-bit rANS, byte/word I/O traits, reciprocal arithmetic, two-state interleaving. 44 unit tests. |
| [`ryg-rans-rs-simd`](crates/ryg-rans-rs-simd/) | 0.1.3 | SSE4.1 accelerated decoder kernels. Currently scaffolded (no implementation). |
| [`ryg-rans-rs`](crates/ryg-rans-rs/) | 0.1.3 | Public facade. Re-exports core types under `byte` and `r64` modules. Optional `simd` and `alloc` features. |
| [`ryg-rans-rs-oracle`](crates/ryg-rans-rs-oracle/) | 0.1.3 | Cross-decoding court harness. Runs four deterministic courts comparing C and Rust encode/decode. Accepts `RANS_EVIDENCE_DIR` and `RANS_GIT_COMMIT` environment variables. |
| [`ryg-rans-rs-casefile`](crates/ryg-rans-rs-casefile/) | 0.1.3 | Typed schemas for court evidence: `CaseManifest`, `Receipt`, `Residual`. Schema foundation — canonical serialization and validation in development. |
| [`ryg-rans-rs-cli`](crates/ryg-rans-rs-cli/) | 0.1.3 | CLI tools for encoding, decoding, inspection, and benchmarking. Currently scaffolded (no user-facing implementation). |

---

## Docker VM Test Matrix

All testing, validation, benchmarking, fuzzing, sanitization, proof execution, package inspection, oracle compilation, and cross-compilation run inside Docker containers. The matrix is defined in `docker/compose/matrix.yml` and executed by `docker/bootstrap-docker.sh`.

### Ten Services

| Service | Purpose | `cap_drop` | Source Mount | Network |
|---------|---------|------------|--------------|---------|
| `oracle-gcc` | Builds 4 C/C++ oracle binaries from pinned upstream | ALL | — (build context) | — |
| `rust-stable-tests` | `cargo test --workspace`, `--features std`, `--no-default-features` | ALL | Read-only | None |
| `rust-musl-build` | musl target build + 44 core tests | ALL | Read-only | — |
| `package-audit` | `cargo package --list` for all crates | ALL | Read-only | None |
| `cross-court` | C↔Rust cross-decoding: 4 courts, 20 cases × 5 checks each | — | Read-only | — |
| `miri` | Nightly Miri: 44 no_std tests (nightly installed at runtime) | ALL | Read-only | — |
| `msrv` | Rust 1.85 MSRV build: core, casefile, facade, CLI | ALL | Read-only | None |
| `cross-aarch64` | aarch64 cross-compilation: core, casefile, facade | ALL | Read-only | None |
| `sanitizers` | ASan-instrumented oracle build + smoke test | ALL | — | None |
| `performance` | `cargo bench --workspace --no-run` (compile validation) | ALL | Read-only | None |

### Safety Features

- **Preflight inventory**: Full resource fingerprint (containers, images with digests, volumes, networks, compose projects, buildx builders) captured before any operation.
- **Collision checks**: Proposed container, volume, image, and network names checked against existing resources before creation.
- **Per-run isolation**: Unique Compose project names, run-ID tagged images and volumes.
- **Non-interference**: Post-run fingerprint comparison. Any change to a pre-existing (non-project) resource causes a hard failure.
- **Per-job log SHA-256**: Every job's output log is hashed and recorded in the matrix stamp.
- **Fail-closed**: `set -euo pipefail`. No `|| true` suppression anywhere in the execution path.
- **Cleanup trap**: Containers and networks removed on exit; reports archived before cleanup; preserves original exit code.

### Evidence Chain

1. Bootstrap creates immutable source snapshot from current `HEAD`.
2. Docker builds all images with run-specific tags.
3. Matrix jobs execute in fixed sequence; any failure aborts.
4. Cross-court writes receipts and manifests to `/reports/evidence/` with `code_commit` from `RANS_GIT_COMMIT`.
5. Post-run fingerprint comparison verifies no protected resources changed.
6. Matrix receipt and JSON stamp (with per-job `exit_code`, `log_sha256`, timestamps) written.
7. Reports archived to `/run/media/one/toshiba4TB/docker/ryg-rans-rs/reports/`.

Run the full matrix:
```sh
./docker/bootstrap-docker.sh
```

---

## C/C++ Oracle Adapter

The directory `oracle/adapter/` contains:

- **`rans_trace.cpp`**: A standalone C++ program that emits JSON-line traces by calling the pinned upstream functions directly (not duplicating their arithmetic). Supports **22 operations** covering 32-bit byte and 64-bit rANS variants.
- **`rans_byte.h`**, **`rans64.h`**, **`platform.h`**: Pinned upstream headers from `rygorous/ryg_rans`.

Each operation outputs deterministic JSON with explicit-width fields for programmatic comparison. The cross-court harness (`ryg-rans-rs-oracle`) invokes the compiled adapter for every comparison.

Build:
```sh
cd oracle/adapter && make
```

---

## Upstream Oracle

| Property | Value |
|----------|-------|
| Repository | [rygorous/ryg_rans](https://github.com/rygorous/ryg_rans) |
| Pinned Commit | [`c9d162d9`](https://github.com/rygorous/ryg_rans/commit/c9d162d996fd600315af9ae8eb89d832576cb32d) |
| Date | 2018-11-25 |
| Host | x86_64, little-endian |

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

## Acknowledgment

- **Fabian "ryg" Giesen** — for the original public-domain `ryg_rans` implementation and extensive blog documentation of rANS design.
- **Jarek Duda** — for the ANS framework (arxiv.org/abs/1311.2540).
- **Charles Bloom**, **Yann Collet**, **Eugene Shelwien** — for the broader entropy coding and ANS literature that informed this reconstruction.

---

## Contributing

This project follows a **forensic parity methodology**. Before contributing a new surface or optimization:

1. Study the upstream C/C++ reference behavior via the oracle adapter.
2. Implement the Rust equivalent with deterministic state transitions.
3. Verify against the compiled oracle.
4. Record any residuals as first-class artifacts.
5. Submit evidence, not just code.
6. The seal gate (`cargo xtask seal`) must pass from a clean checkout.
