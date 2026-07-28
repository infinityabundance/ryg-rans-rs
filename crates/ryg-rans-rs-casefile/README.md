# ryg-rans-rs-casefile

> Typed schemas and deterministic serialization for rANS testing and oracle comparison

[![#![no_std]](https://img.shields.io/badge/std-no--std-blue)](https://docs.rs/ryg-rans-rs-casefile)
[![unsafe forbidden](https://img.shields.io/badge/unsafe-forbidden-success)](https://github.com/rust-secure-code/safety-dance/)
[![MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/infinityabundance/ryg-rans-rs)

## Overview

This crate defines the **typed schemas** used to represent test cases and results in the
ryg-rans-rs workspace. It provides three core data types — `Casefile`, `Receipt`, and `Residual` —
that form the basis of the deterministic testing and oracle comparison infrastructure.

The crate is `#![no_std]` with `#![forbid(unsafe_code)]` and uses the `alloc` crate for `Vec`
storage. Its types are serializable via `serde` for transport across process boundaries (e.g.,
between the Rust oracle harness and the compiled C/C++ adapter binary).

## Types

### `Casefile` — A deterministic test case

```rust
pub struct Casefile {
    pub schema_version: u32,
    pub case_id: &'static str,
    pub upstream_commit: &'static str,
    pub variant: &'static str,
    pub operation: &'static str,
    pub seed: u64,
    pub input_sha256: Option<[u8; 32]>,
    pub input: Vec<u8>,
    pub scale_bits: u32,
    pub frequencies: Vec<u32>,
    pub cumulative_frequencies: Vec<u32>,
    pub interleave: u32,
}
```

A `Casefile` fully describes an rANS encode/decode scenario:

| Field | Description |
|-------|-------------|
| `schema_version` | Schema version (`CASEFILE_SCHEMA_VERSION = 1`) for forward compatibility |
| `case_id` | Unique human-readable identifier (e.g. `"byte_div_roundtrip_single"`) |
| `upstream_commit` | Git commit hash of the ryg_rans reference implementation used for comparison |
| `variant` | Algorithm variant: `"byte"`, `"byte_interleaved"`, or `"rans64"` |
| `operation` | Operation under test: `"encode_decode"` |
| `seed` | Random seed for stochastic test generation |
| `input_sha256` | Optional SHA-256 digest of `input` for integrity verification |
| `input` | Raw input bytes to encode |
| `scale_bits` | Scale bits (number of bits for the cumulative frequency distribution) |
| `frequencies` | Symbol frequency table (length = `1 << scale_bits`) |
| `cumulative_frequencies` | Cumulative frequency table (for decoder lookup) |
| `interleave` | Interleave count (`1` for single-stream, `2` for two-state interleaved) |

### `Receipt` — A court receipt documenting oracle results

```rust
pub struct Receipt {
    pub schema_version: u32,
    pub court_id: &'static str,
    pub case_count: u32,
    pub verdict: &'static str,
    pub upstream_commit: &'static str,
    pub rust_commit: Option<&'static str>,
    pub pairs_compared: u64,
    pub pairs_matched: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<&'static str>,
    pub timestamp: Option<u64>,
}
```

A `Receipt` summarizes the outcome of an oracle comparison run. The `verdict` field is `"PASS"`,
`"FAIL"`, or `"INCONCLUSIVE"`.

### `Residual` — An observed difference between implementations

```rust
pub struct Residual {
    pub case_id: &'static str,
    pub court_id: &'static str,
    pub variant: &'static str,
    pub upstream_commit: &'static str,
    pub class: &'static str,
    pub severity: &'static str,
    pub status: &'static str,
}
```

Each `Residual` documents a single mismatch. `class` categorizes the difference (e.g.,
`"encoding_bitdiff"`), `severity` indicates impact (`"ERROR"`, `"WARNING"`, `"INFO"`), and `status`
tracks resolution state (`"OPEN"`, `"RESOLVED"`, `"WONTFIX"`).

`Residual` implements `fmt::Display` for readable output:

```
byte_div_roundtrip_single (oracle-v0.1.0): encoding_bitdiff [ERROR] - OPEN
```

## Usage Example

```rust
use ryg_rans_rs_casefile::{Casefile, Receipt, Residual, CASEFILE_SCHEMA_VERSION};

// Create a casefile
let mut case = Casefile::new("byte_recip_roundtrip_basic", "byte");
case.input = vec![1, 2, 3, 4, 5];
case.frequencies = vec![100, 50, 30, 20, 10];  // must sum to 2^scale_bits
case.scale_bits = 14;

// Later, create a receipt from oracle results
let receipt = Receipt {
    schema_version: CASEFILE_SCHEMA_VERSION,
    court_id: "oracle-v0.1.0",
    case_count: 1,
    verdict: "PASS",
    upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
    rust_commit: Some("abc123def456"),
    pairs_compared: 1,
    pairs_matched: 1,
    residual_count: 0,
    residual_ids: Vec::new(),
    timestamp: Some(1715904000),
};

// Or if a mismatch is found, create a residual
let residual = Residual {
    case_id: "byte_recip_roundtrip_basic",
    court_id: "oracle-v0.1.0",
    variant: "byte",
    upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
    class: "encoding_bitdiff",
    severity: "ERROR",
    status: "OPEN",
};
```

## Serialization

All three types are `#[derive(Serialize, Deserialize)]` for use with `serde_json` or any other
serde-compatible serializer. The JSON representation is stable within a schema version but may
change between schema versions.

Example JSON for a `Casefile`:

```json
{
  "schema_version": 1,
  "case_id": "byte_recip_roundtrip_basic",
  "upstream_commit": "c9d162d996fd600315af9ae8eb89d832576cb32d",
  "variant": "byte",
  "operation": "encode_decode",
  "seed": 0,
  "input_sha256": null,
  "input": [1, 2, 3, 4, 5],
  "scale_bits": 14,
  "frequencies": [],
  "cumulative_frequencies": [],
  "interleave": 1
}
```

## Cargo Features

This crate defines no optional features. `serde`, `serde_json`, and `sha2` are always enabled.

## Safety

**No unsafe code.** Uses `#![forbid(unsafe_code)]` at the crate root. All types are plain data
containers with no unsafe initialization, no pointer manipulation, and no FFI.

## Dependencies

| Dependency | Version | Notes |
|------------|---------|-------|
| `serde` | `1` | Derive macros for `Serialize`/`Deserialize` (with `derive` feature) |
| `serde_json` | `1` | JSON serialization format |
| `sha2` | `0.10` | SHA-256 hashing for input integrity verification |
