# ryg-rans-rs-casefile

> Typed schemas for ryg-rans-rs forensic court evidence.

## Types

- `CaseResult` — Per-case comparison result: input, frequencies, C and Rust streams, five check booleans (C self-decode, Rust self-decode, compressed match, C→Rust decode, Rust→C decode).
- `CaseManifest` — Complete collection of cases for one court, with metadata (court ID, variant, scale_bits, seed).
- `Receipt` — Verdict, counts, SHA-256 chains, upstream and code commits, reproduction command.

## Purpose

This crate provides the data model for the project's forensic evidence system. It is published for transparency and reproducibility — downstream consumers can parse and verify the evidence artifacts independently.

## Status

**Schema foundation.** The types are defined and used by `ryg-rans-rs-oracle`. Canonical serialization, hash computation, and validation are implemented in the oracle harness.
