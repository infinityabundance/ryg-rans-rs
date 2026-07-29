# ryg-rans-rs-cli

> **Production-grade rANS compression CLI — `ryg-rans`**  
> Versioned block-streaming container format with SHA-256 integrity verification.  
> Encode, decode, inspect, verify, compare, benchmark.  
> Deterministic, resource-bounded, non-panicking, corruption-detecting.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-cli)](https://crates.io/crates/ryg-rans-rs-cli)

---

## Commands

| Command | Description |
|---------|-------------|
| `encode` | Encode input into a versioned `.rygr` container |
| `decode` | Strictly decode and verify a `.rygr` container |
| `inspect` | Inspect container structure and metadata |
| `verify` | Fully verify without writing decoded output |
| `model` | Build, inspect, validate, and compare models |
| `trace` | Trace symbol/state transitions |
| `compare` | Compare arithmetic paths, backends, files, or oracle |
| `bench` | Benchmark production Rust codec backends |
| `capabilities` | Show compiled and runtime-supported codecs and backends |
| `completions` | Generate shell completion scripts |

## Container Format

RYGRANS v1 — a versioned block-streaming container:

```
┌─────────────────────┐
│ File Header (32 B)  │  ← magic, version, flags, codec, block size
├─────────────────────┤
│ Block Record 0      │  ← header (104 B) + model + payload
│   (RAW/RLE/RANS)    │     per-block SHA-256 integrity
├─────────────────────┤
│ Block Record 1      │
│   ...               │
├─────────────────────┤
│ Footer (104 B)      │  ← block count, totals, container + stream hashes
└─────────────────────┘
```

See `docs/container-format-v1.md` for the full specification.

### Codec Formats

| ID | Name | States | Scale | Renorm |
|----|------|--------|-------|--------|
| 1 | BYTE_SINGLE | 1 | 1..16 | 8-bit |
| 2 | BYTE_INTERLEAVED2 | 2 | 1..16 | 8-bit |
| 3 | R64_SINGLE | 1 | 1..31 | 32-bit |
| 4 | R64_INTERLEAVED2 | 2 | 1..31 | 32-bit |
| 5 | WORD_SINGLE | 1 | 12 | 16-bit |
| 6 | WORD_INTERLEAVED2 | 2 | 12 | 16-bit |
| 7 | WORD_INTERLEAVED8 | 8 | 12 | 16-bit |
| 8 | WORD_INTERLEAVED16 | 16 | 12 | 16-bit |
| 9 | ALIAS_SINGLE | 1 | 8..17 | 8-bit |
| 10 | ALIAS_INTERLEAVED2 | 2 | 8..17 | 8-bit |

Codec IDs identify the **stream format** (states, renormalization). Division vs reciprocal
and scalar vs SIMD are **implementation choices** that produce identical canonical bytes.

## Examples

```sh
# Encode a file
ryg-rans encode --input input.bin --output input.bin.rygr

# Decode a container
ryg-rans decode --input archive.rygr --output restored.bin

# Encode from stdin to stdout
cat input.bin | ryg-rans encode --input - --output archive.rygr

# Decode to stdout (verified spool + atomic copy)
ryg-rans decode --input archive.rygr --output -

# Inspect container structure
ryg-rans inspect --input archive.rygr --output-format json

# Verify integrity
ryg-rans verify --input archive.rygr --backend all-available

# Build a frequency model
ryg-rans model build --input input.bin --scale-bits 12 --output model.json

# Trace symbol transitions
ryg-rans trace decode --input archive.rygr --block 0 --max-symbols 128

# Bench decode throughput
ryg-rans bench decode --codec byte-interleaved2 --size 1MiB --samples 100

# Show capabilities
ryg-rans capabilities --output-format json

# Generate shell completions
ryg-rans completions bash > /etc/bash_completion.d/ryg-rans
```

## Safety

- `#![forbid(unsafe_code)]` — compile-time guarantee
- All SIMD acceleration is accessed through safe facade APIs with runtime feature detection
- Strict format validation: every field, bound, and hash is verified
- Resource limits enforced during reading, checked accumulation
- Atomic file output: no partial/corrupt output after failure
- Binary TTY protection: refuse binary data to terminal without `--force-tty`
- SHA-256 integrity: detects corruption (does not authenticate authorship)

## Architecture

```
main.rs                    → thin entry point (argument parsing + dispatch)
lib.rs                     → command routing + capability reporting
container/
  mod.rs                   → constants and re-exports
  header.rs                → file header (32 bytes)
  block.rs                 → block record (header + model + payload)
  footer.rs                → footer (104 bytes)
  codec.rs                 → codec registry and validation
  model.rs                 → canonical model normalization + serialization
  reader.rs                → streaming container parser
  writer.rs                → streaming container serializer
error.rs                   → typed errors with structured context
exit.rs                    → stable exit codes
limits.rs                  → resource limits (checked accumulation)
```

## Exit Codes

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

## Published Versions

- `0.1.15` — Phase CLI: production-grade RYGRANS container format, 10 commands
