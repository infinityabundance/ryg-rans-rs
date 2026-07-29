# ryg-rans-rs-oracle

> **Cross-decoding court harness — forensic verification of rANS bitstream parity.**  
> Generates SHA-256-chained evidence receipts by comparing every Rust encode/decode operation  
> against a compiled C/C++ oracle. Includes performance benchmarking binary.  
> 144 behavioral receipts across 7 algorithmic surfaces.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-oracle)](https://crates.io/crates/ryg-rans-rs-oracle)

---

## Purpose

This crate is the **verification engine** for ryg-rans-rs. It does not contain any
encoding or decoding logic itself. Instead, it:

1. **Generates deterministic test cases**: known inputs, frequency models, and seeds
2. **Feeds identical inputs** to the Rust implementation and the compiled C/C++ oracle
3. **Compares outputs** at multiple levels: compressed streams, decoded symbols, state transitions
4. **Produces receipts**: SHA-256-chained JSON documents recording the verdict
5. **Produces manifests**: complete case-by-case results with input/output hex dumps
6. **Produces residuals**: documented differences when implementations diverge

The result is a **forensic audit trail** — any future change to the code must produce
identical receipts, or the seal gate rejects it.

---

## Court Surfaces

The harness runs independent courts for each algorithmic surface:

### Original Courts (128 receipts)

| Surface | Court Path | Receipts | What It Compares |
|---------|-----------|----------|------------------|
| Byte rANS division | `BYTE.DIVISION` | 44 | Division-based encode/decode |
| Byte rANS reciprocal | `BYTE.RECIPROCAL` | (included) | Reciprocal fast path |
| R64 division | `R64.DIVISION` | 44 | 64-bit division path |
| R64 reciprocal | `R64.RECIPROCAL` | (included) | 64-bit reciprocal path |
| Word rANS | `WORD.DIVISION` | 16 | Table-based word rANS |
| Alias method | `ALIAS` | 16 | Vose alias table |
| SIMD 8-way | `SIMD.INTERLEAVED8` | 8 | SSE4.1 SIMD decoder |

### Phase G: AVX-512 Courts (16 new receipts)

| Surface | Court Path | Receipts | Backend Assertion |
|---------|-----------|----------|-------------------|
| AVX512VL 8-way | `AVX512VL.INTERLEAVED8` | 8 | `avx512vl-8way` |
| AVX512 16-way | `AVX512.INTERLEAVED16` | 8 | `avx512-16way` |

### Total: 144 behavioral receipts

---

## How Courts Work

### Per-Case Checks (AVX512VL.INTERLEAVED8)

For each case, 8 booleans are recorded:

| Check | Description | What It Proves |
|-------|-------------|----------------|
| `c_self_decode` | C oracle decodes its own stream | C oracle works |
| `rust_scalar_self_decode` | Rust scalar decodes Rust stream | Rust scalar works |
| `rust_simd_self_decode` | Rust AVX512VL decodes Rust stream | Rust SIMD works |
| `compressed_match` | C and Rust compressed streams match byte-for-byte | Implementations agree |
| `c_to_rust_scalar` | Rust scalar decodes C oracle's stream | Cross-decode (scalar) |
| `c_to_rust_simd` | Rust AVX512VL decodes C oracle's stream | Cross-decode (SIMD) |
| `rust_to_c` | C oracle decodes Rust's stream | Reverse cross-decode |
| `simd_scalar_agree` | SIMD output == scalar output | Internal consistency |

### Per-Case Checks (AVX512.INTERLEAVED16)

10 checks — same pattern, plus word consumption and final state comparison.

### Backend Assertion

Every SIMD court case records the `rust_backend` field. If the backend is not
`avx512vl-8way` or `avx512-16way` respectively, a `BACKEND.*` residual is recorded,
causing the court to fail. This prevents SIMD courts from silently passing via
scalar fallback.

### Verdict

- **`admitted_match`**: all checks pass, zero residuals
- **`admitted_partial`**: some checks failed or residuals exist

A surface is not considered `full` until all its courts produce `admitted_match`.

---

## The C Oracle Adapter

The C oracle (`oracle/adapter/rans_trace.cpp`) is compiled from the pinned upstream
`ryg_rans` headers plus our new 16-way format operations.

### Operations

| Operation | Purpose |
|-----------|---------|
| `enc-stream-byte[-div]` | Byte rANS encode |
| `dec-stream-byte[-div]` | Byte rANS decode |
| `enc-stream-r64[-div]` | 64-bit rANS encode |
| `dec-stream-r64[-div]` | 64-bit rANS decode |
| `enc-stream-word` | Word rANS encode |
| `dec-stream-word` | Word rANS decode |
| `enc-stream-word-interleaved2` | Two-state interleaved word rANS encode |
| `dec-stream-word-interleaved2` | Two-state interleaved word rANS decode |
| `enc-stream-simd` | 8-way SIMD encode |
| `dec-stream-simd` | 8-way SIMD decode |
| `enc-stream-word-interleaved16` | **16-way encode (Phase G)** |
| `dec-stream-word-interleaved16` | **16-way decode (Phase G)** |
| `trace-alias-table` | Alias table construction |
| `enc-stream-alias[-interleaved2]` | Alias encode |
| `dec-stream-alias[-interleaved2]` | Alias decode |

---

## Usage

### Build the Oracle

```sh
cd oracle/adapter && make
```

### Generate Full Evidence

```sh
RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20
```

This generates into `evidence.staging/<timestamp>/` and promotes on success
(all 144 courts pass, all verdicts `admitted_match`).

### Verify

```sh
cargo xtask seal
```

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `RANS_EVIDENCE_DIR` | `evidence/` | Output directory for receipts/manifests |
| `RANS_GIT_COMMIT` | `git rev-parse HEAD` | Commit hash to embed in evidence |
| `RANS_EVIDENCE_STAGING` | (unset) | Use `evidence.staging/<ts>/` with atomic swap |

---

## Evidence Output Structure

```text
evidence.staging/<timestamp>/
├── index.json              ← registers all receipts with SHA-256 hashes
├── receipts/
│   ├── RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.json
│   ├── RYG_RANS.AVX512.INTERLEAVED16.UNIFORM256.S12.json
│   ├── ... (144 total)
└── manifests/
    ├── RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.json
    ├── RYG_RANS.AVX512.INTERLEAVED16.UNIFORM256.S12.json
    └── ... (144 total)
```

The SHA-256 chain is:
```
index.json → receipt.json → manifest_sha256 → manifest.json
```

Each receipt contains a `receipt_sha256` self-hash, preventing undetected modification.

---

## Performance Benchmark

The crate includes a `perf` binary:

```sh
# With AVX-512 + SSE4.1
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace

# With hardware counters
sudo perf stat -r 10 -e cycles,instructions,branches,branch-misses \
    RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace
```

---

## Related Crates

- **[`ryg-rans-rs-core`](https://crates.io/crates/ryg-rans-rs-core)** — Algorithmic core
- **[`ryg-rans-rs-simd`](https://crates.io/crates/ryg-rans-rs-simd)** — SIMD accelerate kernels
- **[`ryg-rans-rs`](https://crates.io/crates/ryg-rans-rs)** — Public facade
- **[`ryg-rans-rs-casefile`](https://crates.io/crates/ryg-rans-rs-casefile)** — Evidence schema types
- **[`ryg-rans-rs-cli`](https://crates.io/crates/ryg-rans-rs-cli)** — CLI tools (scaffold)
