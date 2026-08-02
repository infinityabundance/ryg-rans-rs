# ryg-rans-rs-casefile

> **Typed evidence schema foundation for rANS forensic court proceedings.**
> `#![no_std]` + `#![forbid(unsafe_code)]` — portable to embedded and Wasm targets.
> Schema version 1 (`CASEFILE_SCHEMA_VERSION`). Zero rANS arithmetic.

**Version: 0.3.0** · Schema v1 · 3 core types (+ std-gated performance types) · 1 test (doc test)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [What This Crate Does NOT Do](#what-this-crate-does-not-do)
3. [Why a Separate Schema Crate](#why-a-separate-schema-crate)
4. [Core Types](#core-types)
5. [Evidence SHA-256 Chain](#evidence-sha-256-chain)
6. [Performance Evidence Types (`std` feature)](#performance-evidence-types-std-feature)
7. [Field Reference](#field-reference)
8. [Serialization](#serialization)
9. [Feature Flags and Dependencies](#feature-flags-and-dependencies)
10. [Trust Boundaries and Input Invariants](#trust-boundaries-and-input-invariants)
11. [Resource Behavior](#resource-behavior)
12. [Usage](#usage)
13. [Troubleshooting](#troubleshooting)
14. [Versioning and Reading Order](#versioning-and-reading-order)

---

## What This Crate Is

This crate defines the **data model** for the ryg-rans-rs forensic evidence
system. The types model the artifacts of the court-of-record architecture:
**Casefile** (a deterministic test case), **Receipt** (a court verdict), and
**Residual** (a tracked discrepancy). With the `std` feature it also defines the
performance-evidence types (`PerformanceManifest`, `PerformanceReceipt`,
`PerformanceIndex`, ...) that `xtask`'s `performance-seal` command builds its
artifacts from.

The crate is `#![no_std]` with `#![forbid(unsafe_code)]`. It has **zero knowledge
of rANS arithmetic, SIMD, or any other implementation detail** — it is a pure data
model crate, usable independently by downstream consumers, auditors, and
researchers.

---

## What This Crate Does NOT Do

This crate does **not** contain:

- Any rANS encoding or decoding logic
- Any oracle or court orchestration code (that is `ryg-rans-rs-oracle`)
- Any SIMD intrinsics or unsafe code
- Any I/O, file-system, or network operations
- Any serialization of the core types — the core types (`Casefile`, `Receipt`,
  `Residual`) do **not** derive `serde` traits. Only the `std`-gated performance
  types do. Evidence JSON is written by the oracle harness (behavioural) and by
  `xtask performance-seal` (performance), which define their own serialization.

---

## Why a Separate Schema Crate

1. **Independent verification**: downstream consumers, auditors, and researchers
   can model evidence artifacts without depending on the oracle harness or the
   rANS crates.
2. **Schema stability**: the types are frozen at schema version 1
   (`CASEFILE_SCHEMA_VERSION = 1`). Changing a field name or adding a variant is a
   breaking change that requires a schema version bump.
3. **No rANS dependency**: the crate depends only on `sha2` (unconditional) and,
   behind the `std` feature, `serde`/`serde_json`.
4. **Separation of concerns**: the evidence schema spans oracle generation,
   sealing verification, and downstream consumption; isolating it prevents
   circular dependencies.

---

## Core Types

### `Casefile` — A Single Deterministic Test Case

```rust
pub struct Casefile {
    pub schema_version: u32,             // CASEFILE_SCHEMA_VERSION
    pub case_id: &'static str,           // e.g. "RYG_RANS.BYTE.BITSTREAM.000001"
    pub upstream_commit: &'static str,   // pinned upstream C revision (c9d162d9...)
    pub variant: &'static str,           // e.g. "byte32", "r64", "word"
    pub operation: &'static str,         // e.g. "encode_decode"
    pub seed: u64,                       // deterministic input seed
    pub input_sha256: Option<[u8; 32]>,  // optional content hash of the input
    pub input: Vec<u8>,                  // input bytes
    pub scale_bits: u32,                 // entropy scale (14 for byte rANS, 12 for word rANS)
    pub frequencies: Vec<u32>,           // per-symbol frequencies
    pub cumulative_frequencies: Vec<u32>,// cumulative frequencies
    pub interleave: u32,                 // interleave factor: 1, 2, 8, or 16
}
```

`Casefile::new(case_id, variant)` fills defaults (schema version, pinned upstream
commit, `operation: "encode_decode"`, `scale_bits: 14`, `interleave: 1`) and is
the constructor used in the doc example.

**Validation invariants** (guaranteed by construction in the oracle, not enforced
by the type system):

1. `frequencies` sums to `1 << scale_bits`
2. `cumulative_frequencies[0] == 0`, `cumulative_frequencies[len-1] == 1 << scale_bits`
3. `cumulative_frequencies` is non-decreasing
4. `scale_bits` is valid for the variant (12 for word rANS, 14 for byte rANS)
5. `interleave` is 1, 2, 8, or 16

### `Receipt` — Court Verdict

```rust
pub struct Receipt {
    pub schema_version: u32,
    pub court_id: &'static str,          // e.g. "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12"
    pub case_count: u32,
    pub verdict: &'static str,           // "admitted_match" | "admitted_partial"
    pub upstream_commit: &'static str,
    pub rust_commit: Option<&'static str>,
    pub pairs_compared: u64,
    pub pairs_matched: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<&'static str>,
    pub timestamp: Option<u64>,          // Unix timestamp (optional)
}
```

The `verdict` field is the primary pass/fail indicator:

- **`admitted_match`**: every comparison passed on every case; the surface can be
  marked fully verified.
- **`admitted_partial`**: some comparisons failed; residuals exist; the surface
  cannot be marked `full` until the residuals are resolved.

> **Note on hash fields**: this `Receipt` has **no** `receipt_sha256` or
> `manifest_sha256` field. The SHA-256 chain fields live on the oracle harness's
> own receipt type (`ryg_rans_rs_oracle::Receipt`), which is what is actually
> written to `evidence/receipts/`. The casefile `Receipt` is the schema model; the
> produced artifacts extend it. Do not look for self-hash fields on this type —
> they are not there.

### `Residual` — A Documented Difference

```rust
pub struct Residual {
    pub case_id: &'static str,
    pub court_id: &'static str,
    pub variant: &'static str,
    pub upstream_commit: &'static str,
    pub class: &'static str,             // "implementation" | "oracle" | "casefile" | "design"
    pub severity: &'static str,          // "S0".."S3"
    pub status: &'static str,            // "open" | "investigating" | "fixed" | "wontfix"
}
```

Residuals are **never deleted** — even after resolution, the record remains with
`status: "fixed"` for auditability.

**Display implementation** (exact, from source — note it prints `court_id`, not
`variant`):

```rust
impl fmt::Display for Residual {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}): {} [{}] - {}",
            self.case_id, self.court_id, self.class, self.severity, self.status
        )
    }
}
```

Example output:

```
RYG_RANS.BYTE.DIVISION.UNIFORM256.S14.0000 (RYG_RANS.BYTE.DIVISION.UNIFORM256.S14): implementation [S1] - fixed
```

---

## Evidence SHA-256 Chain

The produced evidence forms a three-level hash chain (the chain fields exist on
the oracle harness's receipt type and on the xtask performance types; the casefile
types model the same shape):

```text
evidence/index.json
  └── sha256 of → evidence/receipts/receipt-<court_id>.json
                    └── receipt.manifest_sha256 → evidence/manifests/manifest-<court_id>.json
                                                    └── all cases, streams, per-case verdicts
```

### Level 1: Index (`evidence/index.json`)

Produced by the oracle harness (`src/main.rs`) and consumed by the seal gate:

```json
{
  "schema_version": 1,
  "code_commit": "edbf2bb0a924725673c43fad29274e651c77d9f5",
  "receipts": [
    { "court_id": "RYG_RANS.BYTE.DIVISION.SINGLE_STATE.UNIFORM256.S12",
      "sha256": "c40244cb4c5c2bb253da1df158069552eff4b546897c312feb04e6c6fcd8cd5e" }
  ]
}
```

### Level 2: Receipt (`evidence/receipts/receipt-<court_id>.json`)

The oracle harness's receipt carries `court_id`, `court_path`, `variant`,
`profile`, `scale_bits`, `seed`, `num_cases`, `verdict`, `upstream_commit`,
`code_commit`, `pairs_compared`, `pairs_matched`, `residual_count`,
`residual_ids`, `manifest_sha256`, `receipt_sha256`, `reproduction_command`, and
`oracle_compiler`. The **manifest SHA-256 is verified by the seal gate** (gate 12);
behavioural **receipt self-hash verification is currently skipped** by the seal
gate — tracked as residual **L1-R / L20-A** in
[`evidence/phase-l/gap-ledger.md`](../../evidence/phase-l/gap-ledger.md).

### Level 3: Manifest (`evidence/manifests/manifest-<court_id>.json`)

Contains `schema_version`, `court_id`, `court_path`, `variant`, `profile`,
`scale_bits`, `seed`, and `cases` — each case carrying the input hex, the C and
Rust compressed streams, and the per-check booleans (`compressed_match`,
`c_self_decode`, `rust_self_decode`, `c_to_rust`, `rust_to_c`, and for SIMD
courts `rust_scalar_self_decode`, `rust_simd_self_decode`, `c_to_rust_scalar`,
`c_to_rust_simd`, `simd_scalar_agree`, plus the backend fields).

### Security Properties

1. **No receipt can be modified** without changing its hash in the index.
2. **No manifest can be modified** without changing its hash in the receipt.
3. **No receipt can be forged** without recomputing the self-hash (which requires
   knowing the exact serialization format).
4. **The evidence directory is self-authenticating** — index, receipt, and
   manifest hashes are verified by the seal gate (`cargo xtask seal`).

**Receipt counts are never hardcoded in project documentation.** The authoritative
inventory is `evidence/index.json`; the Phase K baseline was 144 behavioural
receipts, and the Phase L courts extend the total (glossary:
[`docs/glossary.md`](../../docs/glossary.md)).

---

## Performance Evidence Types (`std` feature)

Gated behind the `std` feature (which also enables the `serde`/`serde_json`
dependencies). These types are consumed by `cargo xtask performance-seal`:

| Type | Purpose |
|------|---------|
| `CpuMetadata` | CPU model, features, microcode, SMT, governor |
| `OsMetadata` | kernel, OS, memory |
| `PerformanceArtifactHashes` | SHA-256 of criterion archive, results.json, results.csv, host metadata, commands log |
| `PerformanceCase` | One benchmark case (backend requested/executed, timings, CI, hashes, status) |
| `PerformanceManifest` | One performance sealing run for one surface |
| `PerformanceReceipt` | Seals a performance manifest (verdict, counts, all artifact hashes, self-hash) |
| `PerformanceIndexEntry` / `PerformanceIndex` | Run-level inventory of performance receipts |

`PERF_SCHEMA_VERSION = 1` is the schema version for performance evidence. These
types do derive `Serialize`/`Deserialize`.

---

## Field Reference

### `Casefile`

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `schema_version` | `u32` | `CASEFILE_SCHEMA_VERSION` (1) | Schema version |
| `case_id` | `&'static str` | Unique | Case identifier |
| `upstream_commit` | `&'static str` | 40 hex chars | Upstream C commit hash |
| `variant` | `&'static str` | Defined variants | Algorithmic variant |
| `operation` | `&'static str` | e.g. `encode_decode` | What is being tested |
| `seed` | `u64` | Any | PRNG seed |
| `input_sha256` | `Option<[u8; 32]>` | — | Optional input content hash |
| `input` | `Vec<u8>` | Any | Input data |
| `scale_bits` | `u32` | 12/14 (variant-dependent) | Entropy scale |
| `frequencies` | `Vec<u32>` | sums to `1 << scale_bits` | Symbol frequencies |
| `cumulative_frequencies` | `Vec<u32>` | monotonic | Cumulative frequencies |
| `interleave` | `u32` | 1, 2, 8, or 16 | Interleave lanes |

### `Receipt`

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `schema_version` | `u32` | 1 | Schema version |
| `court_id` | `&'static str` | Unique | Court surface ID |
| `case_count` | `u32` | > 0 | Number of cases |
| `verdict` | `&'static str` | `admitted_match` / `admitted_partial` | Pass/fail |
| `upstream_commit` | `&'static str` | 40 hex chars | C commit hash |
| `rust_commit` | `Option<&'static str>` | — | Rust commit hash (optional) |
| `pairs_compared` | `u64` | ≥ 0 | Total comparisons |
| `pairs_matched` | `u64` | ≤ pairs_compared | Matching comparisons |
| `residual_count` | `u32` | ≥ 0 | Number of residuals |
| `residual_ids` | `Vec<&'static str>` | — | Residual references |
| `timestamp` | `Option<u64>` | — | Unix timestamp (optional) |

### `Residual`

| Field | Type | Constraints | Description |
|-------|------|-------------|-------------|
| `case_id` | `&'static str` | References a case | Originating case |
| `court_id` | `&'static str` | References a court | Court surface |
| `variant` | `&'static str` | Defined variants | Algorithmic variant |
| `upstream_commit` | `&'static str` | 40 hex chars | C commit hash |
| `class` | `&'static str` | `implementation`, `oracle`, `casefile`, `design` | Classification |
| `severity` | `&'static str` | `S0`–`S3` | Severity level |
| `status` | `&'static str` | `open`, `investigating`, `fixed`, `wontfix` | Resolution status |

---

## Serialization

- **Core types** (`Casefile`, `Receipt`, `Residual`) derive only `Clone, Debug`.
  They are **not** `serde` types — you cannot serialize them with
  `serde_json` directly. JSON serialization of behavioural evidence is performed
  by the oracle harness, which defines its own serializable receipt/manifest
  types.
- **Performance types** (behind `std`) derive
  `serde::Serialize`/`serde::Deserialize` and are serialized by `xtask
  performance-seal` as pretty-printed JSON.

---

## Feature Flags and Dependencies

| Feature | Enables | Notes |
|---------|---------|-------|
| (default) | — | `no_std` core types only; `sha2` is a dependency but no type uses it yet |
| `std` | `serde`, `serde_json` + all `Performance*` types | Required by `xtask` for `performance-seal` |

| Dependency | Purpose | Optional? |
|------------|---------|-----------|
| `sha2` | SHA-256 (declared; used by downstream consumers) | no |
| `serde` (derive) | `Serialize`/`Deserialize` for performance types | yes (`std`) |
| `serde_json` | JSON round-trip for performance types | yes (`std`) |

---

## Trust Boundaries and Input Invariants

- **`no_std` + `forbid(unsafe_code)`**: this crate cannot execute unsafe code by
  construction; its types carry no behavior that can corrupt memory.
- **Static-str fields**: `case_id`, `court_id`, `variant`, etc. are `&'static str`
  — evidence producers embed identifiers at compile time, so identifiers cannot be
  dynamically forged at runtime.
- **Content addressing**: `input_sha256` exists so large payloads can be
  referenced by hash and stored separately from the casefile manifest.
- **Schema versioning**: every artifact type carries a schema version; consumers
  must reject unknown versions rather than guess.

---

## Resource Behavior

- No heap allocation beyond what `Vec` requires (via `extern crate alloc`).
- No I/O, no filesystem access, no subprocesses — the crate is pure data
  modeling; nothing here can block or consume significant resources.
- `std` feature adds no runtime cost beyond `serde` derive availability.

---

## Usage

```rust
use ryg_rans_rs_casefile::*;

let case = Casefile::new("RYG_RANS.BYTE.BITSTREAM.000001", "byte32");
println!("Case: {} (variant: {})", case.case_id, case.variant);

let residual = Residual {
    case_id: "RYG_RANS.BYTE.BITSTREAM.000001",
    court_id: "RYG_RANS.BYTE.BITSTREAM",
    variant: "byte32",
    upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
    class: "byte_mismatch",
    severity: "S1",
    status: "open",
};
println!("{}", residual);
```

Building a `Receipt`:

```rust
use ryg_rans_rs_casefile::Receipt;

let receipt = Receipt {
    schema_version: 1,
    court_id: "RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12",
    case_count: 8,
    verdict: "admitted_match",
    upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
    rust_commit: None,
    pairs_compared: 64,
    pairs_matched: 64,
    residual_count: 0,
    residual_ids: vec![],
    timestamp: None,
};
```

With the `std` feature, performance artifacts can be serialized:

```rust
#[cfg(feature = "std")]
{
    use ryg_rans_rs_casefile::PerformanceIndex;
    // ... construct a PerformanceIndex, then serde_json::to_string_pretty(&index)
}
```

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `serde_json`/`Serialize` not found | The `std` feature is off; the core types are intentionally not serializable. Enable `features = ["std"]`. |
| `Performance*` types not found | Same — they are gated behind `std`. |
| "receipt_sha256 field does not exist" | Correct — the casefile `Receipt` has no hash fields; the oracle harness's receipt type carries them. |
| Doc-test failures | The doctest requires `std`-linkable doctest environment (`cargo test` links std for doctests). |

---

## Versioning and Reading Order

- **Version**: 0.3.0 (workspace crates). Schema version 1.
- **Consumers**: `ryg-rans-rs-oracle` declares this crate as a dependency (its
  behavioural evidence uses harness-local receipt types); `xtask` uses the `std`
  performance types in `performance-seal`.
- **Reading order**: root [`README.md`](../../README.md) →
  [`docs/glossary.md`](../../docs/glossary.md) →
  [`docs/oracle-method.md`](../../docs/oracle-method.md) → this README →
  [`crates/ryg-rans-rs-oracle/README.md`](../ryg-rans-rs-oracle/README.md) →
  [`xtask/README.md`](../../xtask/README.md).
- **Ground-truth ledger**: [`evidence/phase-l/gap-ledger.md`](../../evidence/phase-l/gap-ledger.md).

---


---

**Further reading:** `docs/papers/0006-evidence.md` (the evidence system these types model), `docs/glossary.md` (receipt, manifest, residual, seal).

*Part of the ryg-rans-rs project. Version 0.3.0. Phase M custodian documentation pass.*
