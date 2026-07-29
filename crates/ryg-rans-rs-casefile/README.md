# ryg-rans-rs-casefile

> **Typed evidence schemas for rANS forensic court proceedings.**  
> `#![no_std]` + `#![forbid(unsafe_code)]` — portable to embedded and Wasm targets.  
> Schema version 1, stable since v0.1.6.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-casefile)](https://crates.io/crates/ryg-rans-rs-casefile)

---

## What This Crate Is

This crate defines the **data model** for the ryg-rans-rs forensic evidence system.
The types in this crate are the foundation of the project's **court-of-record**
architecture — every algorithmic surface is verified by cross-decoding courts that
produce receipts and manifests serialized using these types.

### Why a Separate Schema Crate?

1. **Independent verification**: Downstream consumers, security auditors, and researchers
   can parse and verify evidence artifacts without depending on the full oracle harness
   or the rANS algorithmic crates.
2. **Schema stability**: The types are frozen at schema version 1. Changing a field name
   or adding a variant is a breaking change that requires a schema version bump.
3. **No rANS dependency**: This crate has no dependency on any rANS crate. It only depends
   on `serde` for serialization and `sha2` for hash computation.

---

## Core Types

### `Casefile` — A Single Deterministic Test Case

```rust
pub struct Casefile {
    pub schema_version: u32,          // Always 1
    pub case_id: &'static str,        // e.g. "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.0000"
    pub upstream_commit: &'static str, // Pinned upstream C commit
    pub variant: &'static str,        // e.g. "byte32", "avx512vl-interleaved8"
    pub operation: &'static str,      // e.g. "encode_decode"
    pub seed: u64,                     // PRNG seed for deterministic input generation
    pub input_sha256: Option<[u8; 32]>, // Hash of the input data
    pub input: Vec<u8>,               // The input byte stream to encode
    pub scale_bits: u32,              // Entropy scale (14 for byte, 12 for word)
    pub frequencies: Vec<u32>,        // Per-symbol frequencies (256 entries)
    pub cumulative_frequencies: Vec<u32>, // Cumulative frequencies (257 entries)
    pub interleave: u32,              // 1, 2, 8, or 16 lanes
}
```

A `Casefile` contains everything needed to reproduce a specific test: the input data,
the frequency model, the scale bits, the interleave factor, and the upstream commit
that defines the reference behavior.

### `Receipt` — Court Verdict

```rust
pub struct Receipt {
    pub schema_version: u32,         // Always 1
    pub court_id: &'static str,      // e.g. "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12"
    pub case_count: u32,             // Number of cases in this court
    pub verdict: &'static str,       // "admitted_match" or "admitted_partial"
    pub upstream_commit: &'static str, // c9d162d9...
    pub rust_commit: Option<&'static str>, // Git commit of the Rust implementation
    pub pairs_compared: u64,         // Total comparisons made
    pub pairs_matched: u64,          // Comparisons that matched
    pub residual_count: u32,         // Number of residuals
    pub residual_ids: Vec<&'static str>, // Links to residual records
    pub timestamp: Option<u64>,      // Unix timestamp (optional)
}
```

The `verdict` field is the primary pass/fail indicator:
- **`admitted_match`**: Every comparison passed. The Rust implementation matches the
  upstream C oracle exactly, on every case, for every check.
- **`admitted_partial`**: Some comparisons failed. Residuals exist. The surface cannot
  be marked `full` until all residuals are resolved.

### `Residual` — A Documented Difference

```rust
pub struct Residual {
    pub case_id: &'static str,
    pub court_id: &'static str,
    pub variant: &'static str,
    pub upstream_commit: &'static str,
    pub class: &'static str,     // "implementation", "oracle", "casefile", "design"
    pub severity: &'static str,  // "S0" (critical) through "S3" (informational)
    pub status: &'static str,   // "open", "investigating", "fixed", "wontfix"
}
```

Residuals are **never deleted** — even after resolution, the record remains with
`status: "fixed"` for auditability.

---

## Evidence SHA-256 Chain

The evidence system uses a three-level hash chain to guarantee integrity:

```
evidence/index.json
  └── contains sha256_of(receipt.json) for every receipt
        └── receipt.json contains sha256_of(manifest.json)
              └── manifest.json contains all case data

Self-hash: receipt.json.receipt_sha256 == sha256(receipt.json without receipt_sha256)
```

This means:
1. **No receipt can be modified** without changing its hash in the index
2. **No manifest can be modified** without changing its hash in the receipt
3. **No receipt can be forged** without recomputing the self-hash (which requires
   knowing the exact serialization format)

---

## Usage

```rust
use ryg_rans_rs_casefile::*;

let case = Casefile::new(
    "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.0000",
    "avx512vl-interleaved8",
);
case.seed = 42;
case.scale_bits = 12;

let residual = Residual {
    case_id: "...",
    court_id: "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12",
    variant: "avx512vl-interleaved8",
    upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
    class: "implementation",
    severity: "S1",
    status: "fixed",
};
println!("{}", residual);
// Output: "... (avx512vl-interleaved8): implementation [S1] - fixed"
```

---

## Status

**Schema foundation** — stable since v0.1.6. Used by:
- `ryg-rans-rs-oracle` for evidence generation
- `evidence/index.json`, `evidence/receipts/*.json`, `evidence/manifests/*.json`
- `cargo xtask seal` for hash-verification gates (gates 11-13)

### Feature Flags

None. The crate is `#![no_std]` with `extern crate alloc` for `Vec`.
