# ryg-rans-rs-oracle

> Cross-decoding court harness for ryg-rans-rs forensic parity.  
> Generates SHA-256-chained evidence receipts by comparing Rust rANS against compiled C/C++ oracles.  
> Includes performance benchmarking binary.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-oracle)](https://crates.io/crates/ryg-rans-rs-oracle)

## Courts

The harness runs deterministic courts that compare Rust rANS encoding and decoding against a compiled C/C++ oracle binary (`rans_trace.cpp`).

| Court | Surface | Receipts |
|-------|---------|----------|
| `BYTE.DIVISION` | 32-bit byte rANS division path | 44 |
| `BYTE.RECIPROCAL` | 32-bit byte rANS reciprocal path | (included above) |
| `R64.DIVISION` | 64-bit rANS division path | 44 |
| `R64.RECIPROCAL` | 64-bit rANS reciprocal path | (included above) |
| `WORD.DIVISION` | Word-aligned rANS (scalar) | 16 |
| `ALIAS` | Alias method | 16 |
| `SIMD.INTERLEAVED8` | SSE4.1 SIMD decoder | 8 |
| **Total** | | **128** |

Each court produces:
- A `Receipt` JSON file containing verdict, counts, SHA-256 chains, reproduction command.
- A `CaseManifest` JSON file containing all input cases, frequency models, C and Rust compressed streams, and per-case check results.
- An `index.json` accumulating all receipt references.

The evidence is SHA-256-chained: manifest hash is embedded in the receipt, receipt hash is embedded in the index. Self-hashes prevent undetected modification.

## Usage

```sh
# Build the oracle adapter
cd oracle/adapter && make

# Generate full evidence (10+ minutes for 128 courts)
RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20

# Verify all gates
cargo xtask seal
```

### Environment Variables

- `RANS_EVIDENCE_DIR` — Output directory for receipts and manifests (default: `evidence/`).
- `RANS_GIT_COMMIT` — Commit hash to embed in evidence (default: `git rev-parse HEAD`).
- `RANS_EVIDENCE_STAGING` — Use `evidence.staging/<timestamp>/` with atomic swap on success.

## Performance Benchmark

The crate includes a `perf` binary for measuring decode throughput:

```sh
# With SIMD backend
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo run --release \
    --bin perf -- oracle/adapter/rans_trace [only-size]
```

Measures across 5 profiles (Uniform256, Freq1Residual, Skewed.255_1, Sparse.17, Renorm.Boundary) and 7 sizes (64 B – 1 MiB). Reports GiB/s and ns/symbol for scalar and SIMD backends.

For hardware counter measurement:
```sh
sudo perf stat -r 5 -e cycles,instructions,branches,branch-misses \
    RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo run --release \
    --bin perf -- oracle/adapter/rans_trace
```
