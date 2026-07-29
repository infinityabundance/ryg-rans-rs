# ryg-rans-rs-casefile

> **Typed evidence schemas for rANS forensic court proceedings.**  
> `#![no_std]` + `#![forbid(unsafe_code)]` — portable to embedded and Wasm targets.  
> Schema version 1, stable since v0.1.6.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-casefile)](https://crates.io/crates/ryg-rans-rs-casefile)

---

## Purpose

This crate defines the **data model** for the ryg-rans-rs forensic evidence system.
It exists so that downstream consumers, security auditors, and researchers can
independently parse and verify the evidence artifacts without depending on the
full oracle harness.

---

## Types

### `Casefile` — A single deterministic test case

```rust
pub struct Casefile {
    pub schema_version: u32,        // Always 1
    pub case_id: &'static str,      // e.g. "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.0000"
    pub upstream_commit: &'static str, // Pinned c9d162d9
    pub variant: &'static str,      // e.g. "byte32", "r64", "avx512vl-interleaved8"
    pub operation: &'static str,    // e.g. "encode_decode"
    pub seed: u64,                  // PRNG seed for deterministic generation
    pub input_sha256: Option<[u8; 32]>, // Hash of input data
    pub input: Vec<u8>,             // The input symbols to encode
    pub scale_bits: u32,            // Frequency scale (14 for byte, 12 for word)
    pub frequencies: Vec<u32>,      // Symbol frequencies (256 entries)
    pub cumulative_frequencies: Vec<u32>, // Cumulative frequencies (257 entries)
    pub interleave: u32,            // 1, 2, 8, or 16
}
```

### `Receipt` — Court verdict

```rust
pub struct Receipt {
    pub schema_version: u32,        // Always 1
    pub court_id: &'static str,     // e.g. "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12"
    pub case_count: u32,            // Number of cases in this court
    pub verdict: &'static str,      // "admitted_match" or "admitted_partial"
    pub upstream_commit: &'static str,
    pub rust_commit: Option<&'static str>,
    pub pairs_compared: u64,        // Total comparisons made
    pub pairs_matched: u64,         // Comparisons that matched
    pub residual_count: u32,
    pub residual_ids: Vec<&'static str>,
    pub timestamp: Option<u64>,
}
```

### `Residual` — A documented difference

```rust
pub struct Residual {
    pub case_id: &'static str,
    pub court_id: &'static str,
    pub variant: &'static str,
    pub upstream_commit: &'static str,
    pub class: &'static str,     // "implementation", "oracle", "casefile", etc.
    pub severity: &'static str,  // "S0" (critical) through "S3" (informational)
    pub status: &'static str,   // "open", "investigating", "fixed", "wontfix"
}
```

---

## Evidence SHA-256 Chain

The evidence system uses a three-level hash chain:

```
index.json
  └── sha256_of(receipt.json) → receipt.json
        └── sha256_of(manifest.json) → manifest.json
              └── (all case data, streams, verdicts)
```

Each receipt also has a **self-hash** (`receipt_sha256`) that prevents
undetected modification:
- Anyone can verify: `sha256(receipt.json_without_receipt_sha256) == receipt_sha256`
- A modified receipt won't match its own self-hash
- A replaced receipt won't match the index hash

This crate provides the type definitions. The hash computation and serialization
are implemented in `ryg-rans-rs-oracle`.

---

## Usage

```rust
use ryg_rans_rs_casefile::*;

// Create a test case
let case = Casefile::new(
    "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.0000",
    "avx512vl-interleaved8",
);
case.seed = 42;
case.scale_bits = 12;

// Create a residual
let residual = Residual {
    case_id: "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.0000",
    court_id: "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12",
    variant: "avx512vl-interleaved8",
    upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
    class: "implementation",
    severity: "S1",
    status: "fixed",
};
println!("{}", residual);
// Output: "RYG_RANS... (avx512vl-interleaved8): implementation [S1] - fixed"
```

---

## Status

**Schema foundation** — stable since v0.1.6. The types are used by:
- `ryg-rans-rs-oracle` for evidence generation
- `evidence/index.json`, `evidence/receipts/*.json`, `evidence/manifests/*.json`
- `cargo xtask seal` for hash-verification gates

---

## Feature Flags

None. The crate is `#![no_std]` with `extern crate alloc` for `Vec`.
