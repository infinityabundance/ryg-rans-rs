# ryg-rans-rs-oracle

> Cross-decoding court harness for ryg-rans-rs forensic parity.  
> Generates SHA-256-chained evidence receipts by comparing Rust rANS against compiled C/C++ oracles.  
> Includes performance benchmarking binary.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-oracle)](https://crates.io/crates/ryg-rans-rs-oracle)

## Courts

The harness runs deterministic courts that compare Rust rANS encoding and decoding against a compiled C/C++ oracle binary (`rans_trace.cpp`).

| Court Surface | Receipts | Description |
|---------------|----------|-------------|
| Byte rANS (division + reciprocal) | 44 | 32-bit byte-aligned rANS |
| R64 (division + reciprocal) | 44 | 64-bit rANS |
| Word rANS (scalar) | 16 | Word-aligned rANS |
| Alias method | 16 | Vose alias table + byte rANS |
| SIMD.INTERLEAVED8 | 8 | SSE4.1 SIMD decoder |
| **AVX512VL.INTERLEAVED8** | **8** | AVX-512VL 8-way decoder |
| **AVX512.INTERLEAVED16** | **8** | AVX-512 16-way decoder |
| **Total** | **144** | |

### Phase G: AVX-512 Courts

Two new court surfaces in `phase_g.rs`:

#### `AVX512VL.INTERLEAVED8`
- 8 checks per case: C self-decode, Rust scalar/SIMD self-decode, compressed match, C→Rust scalar/SIMD, Rust→C, SIMD/scalar agree, backend assertion
- Rejects scalar fallback with `BACKEND.*` residual

#### `AVX512.INTERLEAVED16`
- 8 checks per case including cross-language decode
- Backend assertion `avx512-16way` required
- Independent C oracle for new 16-way format

## Usage

```sh
# Build the oracle adapter
cd oracle/adapter && make

# Run all courts (144 receipts)
RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20

# Verify all gates
cargo xtask seal
```

### Environment Variables

- `RANS_EVIDENCE_DIR` — Output directory (default: `evidence/`).
- `RANS_GIT_COMMIT` — Commit hash for evidence (default: `git rev-parse HEAD`).
- `RANS_EVIDENCE_STAGING` — Use staging directory with atomic swap on success.

## C Oracle Operations

| Operation | Purpose |
|-----------|---------|
| `enc-stream-simd` | 8-way SIMD Word rANS encode |
| `dec-stream-simd` | 8-way SIMD Word rANS decode |
| `enc-stream-word-interleaved16` | 16-way Word rANS encode |
| `dec-stream-word-interleaved16` | 16-way Word rANS decode |
| Plus all byte/R64/alias/interleaved2 operations | |

## Performance Benchmark

```sh
# With SIMD + AVX-512
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" cargo run --release \
    --bin perf -- oracle/adapter/rans_trace
```
