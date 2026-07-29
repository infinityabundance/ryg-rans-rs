# ryg-rans-rs-oracle

> **Forensic cross-decoding court harness — the verification engine for ryg-rans-rs.**  
> Generates SHA-256-chained evidence receipts by comparing every Rust encode/decode
> operation against a compiled C/C++ oracle.  
> 144 behavioral receipts across 7 algorithmic surfaces.  
> 10+ checks per case, backend assertions, SHA-256 chains.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-oracle)](https://crates.io/crates/ryg-rans-rs-oracle)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [Court Surfaces](#court-surfaces)
3. [How Courts Work](#how-courts-work)
4. [Per-Case Checks (AVX512VL.INTERLEAVED8)](#per-case-checks-avx512vlinterleaved8)
5. [Backend Assertion](#backend-assertion)
6. [The C Oracle Adapter](#the-c-oracle-adapter)
7. [Evidence Output](#evidence-output)
8. [Usage](#usage)
9. [Performance Benchmark](#performance-benchmark)

---

## What This Crate Is

This crate is the **verification engine** for ryg-rans-rs. It does not contain any
encoding or decoding logic itself. Instead, it orchestrates deterministic comparisons
between the Rust implementation and the compiled C/C++ oracle (`rans_trace.cpp`).

### Why Process-Level Comparison Instead of FFI?

The project uses process-level communication (subprocess invocation with JSON I/O)
instead of FFI for several reasons:

1. **No unsafe FFI boundaries**: The oracle crate is entirely safe Rust. No `extern "C"`,
   no raw pointer manipulation, no lifetime annotations on foreign types.
2. **No C/C++ build dependency**: The workspace can be compiled without a C/C++ compiler.
   The oracle is only needed for verification runs.
3. **Clear separation**: The Rust implementation is completely independent of the upstream
   C. Process-level comparison proves that two independently compiled programs produce
   identical results.
4. **Auditability**: Anyone can compile the upstream C themselves and run the oracle
   without trusting the prebuilt binary.

---

## Court Surfaces

### Original Courts (128 receipts)

| Surface | Court ID Pattern | Receipts | Checks Per Case |
|---------|-----------------|----------|-----------------|
| Byte rANS division | `BYTE.DIVISION.*` | 44 | 5 |
| Byte rANS reciprocal | `BYTE.RECIPROCAL.*` | (included) | 5 |
| R64 division | `R64.DIVISION.*` | 44 | 5 |
| R64 reciprocal | `R64.RECIPROCAL.*` | (included) | 5 |
| Word rANS | `WORD.DIVISION.*` | 16 | 5 |
| Alias method | `ALIAS.*` | 16 | 5 |
| SSE4.1 SIMD | `SIMD.INTERLEAVED8.*` | 8 | 6 |

### Phase G: AVX-512 Courts (16 new receipts)

| Surface | Court ID Pattern | Receipts | Backend Assertion |
|---------|-----------------|----------|-------------------|
| AVX512VL 8-way | `AVX512VL.INTERLEAVED8.*` | 8 | `avx512vl-8way` |
| AVX512 16-way | `AVX512.INTERLEAVED16.*` | 8 | `avx512-16way` |

### Total: 144 behavioral receipts

---

## How Courts Work

### The Court Lifecycle

```
1. Generate deterministic input (fixed seed + model profile)
2. Encode with C oracle → C compressed stream
3. Encode with Rust → Rust compressed stream
4. Self-decode C stream with C oracle
5. Self-decode Rust stream with Rust (scalar + SIMD backends)
6. Cross-decode: C stream → Rust decoder (scalar + SIMD)
7. Cross-decode: Rust stream → C oracle
8. Compare compressed streams byte-for-byte
9. Compare decoded outputs byte-for-byte
10. Compare final states (where available)
11. Compare reader word consumption (where available)
12. Assert backend identity
```

Every step produces a boolean check result. All booleans must be true for a case
to pass. Any false boolean generates a residual.

---

## Per-Case Checks (AVX512VL.INTERLEAVED8)

| # | Check | What It Proves |
|---|-------|----------------|
| 1 | `c_self_decode` | C oracle can decode its own output (C oracle works) |
| 2 | `rust_scalar_self_decode` | Rust scalar can decode Rust output (scalar works) |
| 3 | `rust_simd_self_decode` | Rust SIMD can decode Rust output (SIMD works) |
| 4 | `compressed_match` | C and Rust compressed streams are byte-identical (implementations agree) |
| 5 | `c_to_rust_scalar` | Rust scalar can decode C's output (cross-decode) |
| 6 | `c_to_rust_simd` | Rust SIMD can decode C's output (cross-decode, SIMD) |
| 7 | `rust_to_c` | C can decode Rust's output (reverse cross-decode) |
| 8 | `simd_scalar_agree` | SIMD and scalar produce identical output (internal consistency) |

### Backend Assertion

Every SIMD court case records the `rust_backend` field. If the backend is not the
expected value (`avx512vl-8way` or `avx512-16way`), a `BACKEND.*` residual is recorded.
This prevents SIMD courts from silently passing via scalar fallback.

---

## The C Oracle Adapter

The C oracle (`oracle/adapter/rans_trace.cpp`) is compiled from the pinned upstream
`ryg_rans` headers (commit `c9d162d9`). It supports:

### Stream Operations

| Operation | Inputs | Outputs |
|-----------|--------|---------|
| `enc-stream-byte[-div]` | scale_bits, freq_csv, input_hex | compressed_hex, decode_ok |
| `dec-stream-byte[-div]` | scale_bits, freq_csv, compressed_hex, num_symbols | decoded_hex |
| `enc-stream-r64[-div]` | scale_bits, freq_csv, input_hex | compressed_hex, decode_ok |
| `dec-stream-r64[-div]` | scale_bits, freq_csv, compressed_hex, num_symbols | decoded_hex |
| `enc-stream-word` | scale_bits, freq_csv, input_hex | compressed_hex, decode_ok |
| `dec-stream-word` | scale_bits, freq_csv, compressed_hex, num_symbols | decoded_hex |
| `enc-stream-word-interleaved2` | scale_bits, freq_csv, input_hex | compressed_hex, decode_ok |
| `dec-stream-word-interleaved2` | scale_bits, freq_csv, compressed_hex, num_symbols | decoded_hex |
| `enc-stream-simd` | scale_bits, freq_csv, input_hex | compressed_hex, decode_ok |
| `dec-stream-simd` | scale_bits, freq_csv, compressed_hex, num_symbols | decoded_hex |
| `enc-stream-word-interleaved16` | scale_bits, freq_csv, input_hex | compressed_hex, decode_ok |
| `dec-stream-word-interleaved16` | scale_bits, freq_csv, compressed_hex, num_symbols | decoded_hex |
| `trace-alias-table` | scale_bits, freq_csv | alias table JSON |
| `enc-stream-alias[-interleaved2]` | scale_bits, freq_csv, input_hex | compressed_hex, decode_ok |
| `dec-stream-alias[-interleaved2]` | scale_bits, freq_csv, compressed_hex, num_symbols | decoded_hex |

### Build

```sh
cd oracle/adapter && make
```

This produces the `rans_trace` binary.

---

## Evidence Output

### Directory Structure

```text
evidence.staging/<timestamp>/
├── index.json              ← SHA-256 index of all receipts
├── receipts/
│   ├── RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.json
│   ├── RYG_RANS.AVX512.INTERLEAVED16.UNIFORM256.S12.json
│   └── ... (144 total)
└── manifests/
    ├── RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.json
    ├── RYG_RANS.AVX512.INTERLEAVED16.UNIFORM256.S12.json
    └── ... (144 total)
```

### SHA-256 Chain

```
index.json
  └── sha256 of → receipt.json
                    └── receipt.receipt_sha256 == sha256(receipt without receipt_sha256)
                    └── receipt.manifest_sha256 → manifest.json
                                                    └── All cases, streams, verdicts
```

### Staging and Atomic Promotion

Evidence is generated into `evidence.staging/<run-id>/`. On success (all 144 courts
pass, all verdicts `admitted_match`, zero residuals), the staging directory is
atomically swapped into `evidence/`. On failure, the existing canonical evidence
is preserved and the staging directory path is printed.

---

## Usage

### Generate Full Evidence

```sh
cd oracle/adapter && make
RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20
```

### Verify

```sh
cargo xtask seal
```

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `RANS_EVIDENCE_DIR` | `evidence/` | Output directory |
| `RANS_GIT_COMMIT` | `git rev-parse HEAD` | Commit hash in evidence |
| `RANS_EVIDENCE_STAGING` | (unset) | Enable staging + atomic swap |

---

## Performance Benchmark

```sh
# All backends: scalar, SSE4.1, AVX512VL 8-way, AVX512 16-way
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace

# With hardware counters
sudo perf stat -r 10 -e cycles,instructions,branches,branch-misses \
    RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace
```
