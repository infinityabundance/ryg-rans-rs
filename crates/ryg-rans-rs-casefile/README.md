# ryg-rans-rs-casefile

> Typed schemas for ryg-rans-rs forensic court evidence.  
> `#![no_std]` — portable to embedded and Wasm targets.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-casefile)](https://crates.io/crates/ryg-rans-rs-casefile)

## Types

- **`CaseResult`** — Per-case comparison result: input, frequencies, C and Rust streams, six check booleans (C self-decode, Rust self-decode, compressed match, C→Rust decode, Rust→C decode, SIMD-scalar agree).
- **`CaseManifest`** — Complete collection of cases for one court, with metadata (court ID, variant, scale_bits, seed).
- **`Receipt`** — Verdict, counts, SHA-256 chains, upstream and code commits, reproduction command.
- **`CourtConfig`** — Court configuration: court path, variant, model profile, scale bits, number of cases.
- **`ModelProfile`** — Frequency model profile data for deterministic case generation.

## Purpose

This crate provides the data model for the project's forensic evidence system. It is published for transparency and reproducibility:

- **Downstream consumers** can parse and verify evidence artifacts independently.
- **Security auditors** can verify the SHA-256 chains from receipt → manifest → index.
- **Researchers** can reproduce the exact court conditions.

## Evidence Structure

Each sealed receipt is a SHA-256-chained artifact:

```
evidence/index.json          ← registers all receipts with their SHA-256
  └─ evidence/receipts/*.json  ← verdict, code_commit, manifest_sha256, self-hash
      └─ evidence/manifests/*.json  ← all input cases, streams, per-case checks
```

The hash chain is: index → receipt hash → receipt manifest hash → manifest content.

## Status

**Schema foundation.** The types are defined and used by `ryg-rans-rs-oracle`. Canonical serialization, hash computation, and validation are implemented in the oracle harness. The schema has been stable since v0.1.6 with 128 indexed receipts across 5 algorithmic surfaces.
