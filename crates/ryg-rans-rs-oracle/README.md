# ryg-rans-rs-oracle

> Development-only oracle harness for comparing Rust rANS output against compiled C/C++ ryg_rans

[![MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/infinityabundance/ryg-rans-rs)
[![build-status](https://img.shields.io/badge/build-dev--only-lightgrey)]()

## Overview

This crate is a **development-only oracle harness** — it is **not** a shipping dependency. Its sole
purpose is to validate the Rust rANS implementation against the canonical upstream
[rygorous/ryg_rans](https://github.com/rygorous/ryg_rans) C/C++ implementation by comparing
bit-exact encoding/decoding output.

The oracle operates as follows:

1. A **casefile** (defined in `ryg-rans-rs-casefile`) describes a test scenario: input data,
   frequency table, scale bits, interleave factor, etc.
2. The Rust implementation encodes and decodes the casefile using `ryg-rans-rs-core`.
3. An **adapter** (a compiled C/C++ binary from the upstream ryg_rans repository) processes the
   same casefile.
4. The oracle compares both outputs byte-for-byte and produces a **receipt** documenting matches,
   mismatches (residuals), and the verdict.

This crate is designed to prevent regression when porting algorithmic changes and to provide
bit-exact confidence during development of SIMD kernels and optimization passes.

## Features

- **Casefile-driven testing** — test cases are serialized as structured `Casefile` objects with
  deterministic schema versioning, SHA-256 input fingerprints, and upstream commit tracking.
- **C/C++ adapter integration** — invokes a compiled adapter binary (`oracle/adapter`) that speaks
  the same casefile protocol, enabling direct side-by-side comparison.
- **Receipt generation** — every oracle run produces a `Receipt` documenting:
  - `court_id` — identifier for the oracle run
  - `case_count` — number of casefiles tested
  - `pairs_compared` / `pairs_matched` — aggregate match statistics
  - `residual_count` — number of mismatches found
  - `verdict` — `"PASS"`, `"FAIL"`, or `"INCONCLUSIVE"`
- **Residual tracking** — each mismatch is recorded as a `Residual` with:
  - `case_id`, `court_id`, `variant` — provenance information
  - `class` — category of mismatch (e.g., `"encoding_bitdiff"`, `"decoding_bitdiff"`)
  - `severity` — `"ERROR"`, `"WARNING"`, or `"INFO"`
  - `status` — `"OPEN"`, `"RESOLVED"`, or `"WONTFIX"`
- **Deterministic commit anchoring** — the upstream commit hash is hardcoded into the casefile
  schema to ensure reproducibility.

## Building and Running

### Prerequisites

- Rust toolchain (nightly or stable with Rust edition 2024)
- A C/C++ compiler (gcc/clang) for building the oracle adapter
- The upstream ryg_rans source (cloned as a submodule or adjacent checkout)

### Build the Oracle Adapter

The oracle adapter is a C/C++ program that links against the upstream ryg_rans implementation and
speaks the casefile JSON protocol:

```sh
cd oracle/adapter
make
```

This produces an executable (e.g., `oracle_adapter`) that reads casefiles from stdin and writes
encoding/decoding results to stdout, both as newline-delimited JSON.

### Run the Oracle

```sh
cargo run --package ryg-rans-rs-oracle -- \
    --adapter ./oracle/adapter/oracle_adapter \
    --casefiles ./oracle/casefiles/
```

Or via the workspace xtask:

```sh
cargo xtask oracle --adapter ./oracle/adapter/oracle_adapter
```

### Interpreting Results

The oracle outputs a `Receipt` as JSON to stdout:

```json
{
  "schema_version": 1,
  "court_id": "oracle-v0.1.0",
  "case_count": 48,
  "verdict": "PASS",
  "upstream_commit": "c9d162d996fd600315af9ae8eb89d832576cb32d",
  "pairs_compared": 48,
  "pairs_matched": 48,
  "residual_count": 0,
  "residual_ids": [],
  "timestamp": 1715904000
}
```

A `verdict` of `"PASS"` means every casefile's Rust output matched the C/C++ output exactly.

## Cargo Features

This crate has no public features. It is always built with `serde`, `serde_json`, and `sha2`
for casefile serialization and input fingerprinting.

## Safety

This crate is **not** `#![forbid(unsafe_code)]` — it invokes external C/C++ binaries as subprocesses
via `std::process::Command` and parses their output. The oracle adapter binary itself may use unsafe
C/C++ code from the upstream ryg_rans implementation, but this has no bearing on the safety of the
Rust library crates (`ryg-rans-rs-core`, `ryg-rans-rs`).

The oracle crate's own Rust code uses `serde` for JSON deserialization, which is safe.

## Performance

Not applicable — this is a correctness verification tool, not a performance benchmark. Execution
time is dominated by the C/C++ adapter compilation and the subprocess communication overhead.

## Dependencies

| Dependency | Version | Notes |
|------------|---------|-------|
| `ryg-rans-rs-core` | `0.1.0` | Rust rANS implementation under test |
| `ryg-rans-rs-casefile` | `0.1.0` | Casefile/receipt/residual types |
| `serde` | `1` | Serialization for casefile protocol (`derive` feature) |
| `serde_json` | `1` | JSON wire format for adapter communication |
| `sha2` | `0.10` | SHA-256 input fingerprinting |
