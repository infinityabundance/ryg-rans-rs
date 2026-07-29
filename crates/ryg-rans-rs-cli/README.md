# ryg-rans-rs-cli

> **Production-grade rANS compression CLI — `ryg-rans`**  
> Versioned block-streaming container format (RYGRANS v1).  
> SHA-256 integrity verification. Resource-bounded, deterministic, non-panicking.  
> Encode, decode, inspect, verify, compare, benchmark, trace.  
> 10 codec formats · 6 decode backends · 10 stable exit codes · 5 shell completions.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-cli)](https://crates.io/crates/ryg-rans-rs-cli)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [Commands](#commands)
3. [Container Format](#container-format)
4. [Codec vs Backend Distinction](#codec-vs-backend-distinction)
5. [Safety Guarantees](#safety-guarantees)
6. [Exit Codes](#exit-codes)
7. [Architecture](#architecture)
8. [Examples](#examples)

---

## What This Crate Is

This crate provides the `ryg-rans` command-line tool for rANS entropy coding. It is
a **production-grade** implementation built on the repository's sealed rANS primitives.

### Design Principles

1. **`#![forbid(unsafe_code)]`** — the CLI crate is entirely safe Rust. All SIMD
   acceleration is accessed through the facade crate's safe APIs with runtime feature
   detection.
2. **Versioned container format** — RYGRANS v1 is a block-streaming format with explicit
   fields, bounds checking, and SHA-256 integrity verification at both the block and
   stream level.
3. **Resource limits** — every command enforces configurable limits on input size, output
   size, block size, block count, and memory. Limits are checked during reading, not after.
4. **Deterministic output** — identical input + options → byte-identical container.
   No timestamps, random identifiers, or host-dependent values.
5. **Strict validation** — every field, bound, and hash is verified. Unknown format
   versions, unsupported codecs, and trailing data are all rejected.

---

## Commands

| Command | Description | Status |
|---------|-------------|--------|
| `encode` | Encode input into a versioned `.rygr` container | Foundation built |
| `decode` | Strictly decode and verify a `.rygr` container | Foundation built |
| `inspect` | Inspect container structure and metadata | Foundation built |
| `verify` | Fully verify without writing decoded output | Foundation built |
| `model` | Build, inspect, validate, and compare models | Implemented |
| `trace` | Trace symbol/state transitions | Foundation built |
| `compare` | Compare arith paths, backends, files, or oracle | Foundation built |
| `bench` | Benchmark production Rust codec backends | Foundation built |
| `capabilities` | Show compiled and runtime-supported codecs and backends | ✅ Implemented |
| `completions` | Generate shell completion scripts | ✅ Implemented |

---

## Container Format

RYGRANS v1 — a versioned block-streaming container:

```text
┌─────────────────────┐
│ File Header (32 B)  │  ← magic "RYGRANS\0", version, flags, codec, block size
├─────────────────────┤
│ Block Record 0      │  ← header (104 B) + model + payload
│   (RAW/RLE/RANS)    │     per-block SHA-256 integrity
├─────────────────────┤
│ Block Record 1      │
│   ...               │
├─────────────────────┤
│ Footer (104 B)      │  ← block count, totals, container + stream SHA-256
└─────────────────────┘
```

### Block Kinds

| Kind | Use | Payload |
|------|-----|---------|
| RAW | Uncompressed data | Raw bytes (payload == decoded) |
| RLE | Single-symbol run | 1 byte (the repeated symbol) |
| RANS | rANS-compressed | Canonical rANS stream + model |

Block selection is deterministic: RANS is used only if it's strictly smaller than RAW.

See `docs/container-format-v1.md` for the full specification.

---

## Codec vs Backend Distinction

### Codec IDs (Stream Format)

Codec IDs identify the **stream format** — the number of states, renormalization unit,
and scale constraint:

| ID | Name | States | Renorm Unit | Scale | Use |
|----|------|--------|-------------|-------|-----|
| 1 | `BYTE_SINGLE` | 1 | 8-bit | 1..=16 | Single-state byte rANS |
| 2 | `BYTE_INTERLEAVED2` | 2 | 8-bit | 1..=16 | Two-state interleaved byte |
| 3 | `R64_SINGLE` | 1 | 32-bit | 1..=31 | Single-state 64-bit |
| 4 | `R64_INTERLEAVED2` | 2 | 32-bit | 1..=31 | Two-state interleaved 64-bit |
| 5 | `WORD_SINGLE` | 1 | 16-bit | 12 | Single-state Word |
| 6 | `WORD_INTERLEAVED2` | 2 | 16-bit | 12 | Two-state Word |
| 7 | `WORD_INTERLEAVED8` | 8 | 16-bit | 12 | Eight-way Word |
| 8 | `WORD_INTERLEAVED16` | 16 | 16-bit | 12 | Sixteen-way Word |
| 9 | `ALIAS_SINGLE` | 1 | 8-bit | 8..=17 | Single-state alias |
| 10 | `ALIAS_INTERLEAVED2` | 2 | 8-bit | 8..=17 | Two-state alias |

### Backends (Implementation Choice)

Backends are **not** codec IDs. Division vs reciprocal are arithmetic implementations.
SSE4.1 vs AVX-512 are decode backends. When two implementations produce the same
canonical stream, that distinction belongs in execution metadata, not the format ID.

---

## Safety Guarantees

| Property | How It's Enforced |
|----------|-------------------|
| No unsafe code | `#![forbid(unsafe_code)]` — compile-time guarantee |
| No CPU feature misuse | Runtime detection via `is_x86_feature_detected!` before calling SIMD kernels |
| No decompression bombs | Limits on block size (64 MiB max), output size (configurable), block count |
| No integer overflow | Checked arithmetic (`checked_add`) on all length accumulations |
| No overread | Pre-declared payload lengths, bounds-checked reads |
| No partial output | Atomic file output with temporary file + rename + sync |
| No binary data to TTY | Refused unless `--force-tty` is supplied |
| No shell injection | Oracle comparison uses direct exec, not shell |
| No panic | Production paths handle all errors via typed `AppError` |
| No trailing data | Decoder rejects bytes after footer |
| No duplicate blocks | Block index must increase by exactly 1 |
| No unknown formats | Unsupported major versions are rejected |

---

## Exit Codes

| Code | Meaning | Triggered By |
|------|---------|-------------|
| 0 | Success | All commands on success |
| 2 | Usage error | Invalid arguments (Clap) |
| 3 | I/O error | File not found, permission denied, broken pipe |
| 4 | Format error | Invalid magic, truncated stream, bad model |
| 5 | Integrity failure | SHA-256 mismatch (payload, decoded, container) |
| 6 | Unsupported | Unknown codec, unsupported format version |
| 7 | Resource limit | Input/output/block size exceeds limit |
| 8 | Comparison mismatch | Arithmetic paths diverge, backends disagree |
| 9 | Backend unavailable | Requested SIMD backend not supported by CPU |
| 10 | Internal error | Invariant violation (bug) |

---

## Architecture

```
main.rs                    → thin entry point (parse args → call lib → return ExitCode)
lib.rs                     → command routing + capabilities + completions
container/
  mod.rs                   → constants (magic, sizes, block kinds)
  header.rs                → FileHeader: 32-byte fixed-size header
  block.rs                 → Block: 104-byte header + model + payload
  footer.rs                → FileFooter: 104-byte footer + SHA-256 hashes
  codec.rs                 → Codec registry: 10 IDs, scale validation, state counts
  model.rs                 → FrequencyModel: canonical normalization (integer only)
  reader.rs                → ContainerReader: streaming parser with validation
  writer.rs                → ContainerWriter: streaming serializer with hashing
error.rs                   → AppError: 10 typed variants with structured context
exit.rs                    → 10 stable exit codes with documentation
limits.rs                  → Limits: central resource bounds, size parsing
```

---

## Examples

```sh
# Encode a file
ryg-rans encode --input input.bin --output input.bin.rygr

# Decode a container
ryg-rans decode --input archive.rygr --output restored.bin

# Encode from stdin to stdout
cat input.bin | ryg-rans encode --input - --output archive.rygr

# Decode to stdout (verified spool — no output until verification passes)
ryg-rans decode --input archive.rygr --output -

# Inspect container structure
ryg-rans inspect --input archive.rygr --output-format json

# Verify integrity with all available backends
ryg-rans verify --input archive.rygr --backend all-available

# Build a frequency model
ryg-rans model build --input input.bin --scale-bits 12 --output model.json

# Show capabilities in JSON
ryg-rans capabilities --output-format json

# Generate bash completions
ryg-rans completions bash > /etc/bash_completion.d/ryg-rans
```
