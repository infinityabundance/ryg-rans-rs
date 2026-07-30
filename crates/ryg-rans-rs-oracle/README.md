# ryg-rans-rs-oracle

> **Forensic cross-decoding court harness — the verification engine for ryg-rans-rs.**  
> Generates SHA-256-chained evidence receipts by comparing every Rust encode/decode
> operation against a compiled C/C++ oracle.  
> 144 behavioral receipts across 7 algorithmic surfaces.  
> 10+ checks per case, backend assertions, SHA-256 chains.  
> Docker matrix for reproducible evidence generation across toolchains.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-oracle)](https://crates.io/crates/ryg-rans-rs-oracle)

**Version: 0.1.27** · 144 behavioral receipts · 10+ checks per case · 7 court surfaces

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [Court Surfaces](#court-surfaces)
3. [How Courts Work](#how-courts-work)
4. [Court Architecture](#court-architecture)
5. [Per-Case Checks](#per-case-checks)
6. [Backend Assertion](#backend-assertion)
7. [The C Oracle Adapter](#the-c-oracle-adapter)
8. [Evidence Output](#evidence-output)
9. [Docker Matrix](#docker-matrix)
10. [Usage](#usage)
11. [Performance Benchmark](#performance-benchmark)

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

```text
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

### Court ID Convention

Each court is identified by a unique dot-separated ID:

```
RYG_RANS.<SURFACE>.<PROFILE>.S<SCALE_BITS>.<CASE_NUMBER>
```

For example:

- `RYG_RANS.BYTE.DIVISION.UNIFORM256.S14.0000`
- `RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.0000`
- `RYG_RANS.AVX512.INTERLEAVED16.SKEWED_255_1.S12.0003`

### Case Generation

Each court runs multiple cases (typically 8 per surface). Cases vary by:

- **Model profile**: Uniform256, Skewed255_1, Sparse2, etc.
- **Scale bits**: S12 (12-bit), S14 (14-bit), etc.
- **Seed**: Deterministic PRNG seed for input generation
- **Length**: Input length in bytes

---

## Court Architecture

### Court Trait

Each court surface implements a `Court` trait:

```rust
trait Court {
    fn id(&self) -> &str;
    fn operation(&self) -> &str;
    fn generate_input(&self, profile: &str, seed: u64) -> Vec<u8>;
    fn encode_c(&self, input: &[u8], profile: &str) -> Result<Vec<u8>, CourtError>;
    fn encode_rust(&self, input: &[u8], profile: &str) -> Result<Vec<u8>, CourtError>;
    fn decode_c(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CourtError>;
    fn decode_rust_scalar(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CourtError>;
    fn decode_rust_simd(&self, compressed: &[u8], expected_len: usize) -> Result<Vec<u8>, CourtError>;
    fn expected_backend(&self) -> Option<&str>;
}
```

### CaseLifecycle

The `CaseLifecycle` orchestrates the full court proceeding for a single case:

```rust
struct CaseLifecycle {
    case_id: String,
    court_id: String,
    profile: String,
    scale_bits: u32,
    input: Vec<u8>,
    c_compressed: Option<Vec<u8>>,
    rust_compressed: Option<Vec<u8>>,
    c_decoded: Option<Vec<u8>>,
    rust_scalar_decoded: Option<Vec<u8>>,
    rust_simd_decoded: Option<Vec<u8>>,
    checks: Vec<CheckResult>,
    expected_backend: Option<String>,
}
```

### CheckResult

Each check produces a structured result:

```rust
struct CheckResult {
    check_id: String,
    description: String,
    passed: bool,
    expected: String,
    actual: String,
}
```

### Residual Generation

When a check fails, a `Residual` is generated with:

- `case_id`: The specific case that failed
- `court_id`: The court surface
- `variant`: Algorithmic variant name
- `class`: Classification (`"implementation"`, `"oracle"`, `"casefile"`, `"design"`)
- `severity`: S0 (critical) through S3 (informational)
- `status`: `"open"`, `"investigating"`, `"fixed"`, `"wontfix"`

Residuals are **never deleted** — even after resolution, the record remains with
`status: "fixed"` for auditability.

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

### Per-Case Checks (Original Byte/R64 Courts)

| # | Check | What It Proves |
|---|-------|----------------|
| 1 | `c_self_decode` | C oracle works |
| 2 | `rust_self_decode` | Rust works |
| 3 | `compressed_match` | Implementations agree on encode |
| 4 | `c_to_rust` | Rust can decode C's output |
| 5 | `rust_to_c` | C can decode Rust's output |

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

### Communication Protocol

The oracle uses a simple JSON-line protocol:

1. Rust writes an operation line to the oracle's stdin: `{"op": "enc-stream-byte", "scale_bits": 14, "freq_csv": "0,16384,...", "input_hex": "414142..."}`
2. Oracle reads, processes, and writes a result line to stdout: `{"compressed_hex": "abcd...", "decode_ok": true}`
3. Rust parses the JSON response and verifies the result

### Build

```sh
cd oracle/adapter && make
```

This produces the `rans_trace` binary. The Makefile compiles with `g++ -std=c++11 -O2`
and statically links against the upstream `ryg_rans` headers.

---

## Evidence Output

### Directory Structure

```text
evidence/<run-id>/
├── index.json              ← SHA-256 index of all receipts
├── receipts/
│   ├── RYG_RANS.BYTE.DIVISION.UNIFORM256.S14.json
│   ├── RYG_RANS.AVX512VL.INTERLEAVED8.UNIFORM256.S12.json
│   ├── RYG_RANS.AVX512.INTERLEAVED16.UNIFORM256.S12.json
│   └── ... (144 total)
└── manifests/
    ├── RYG_RANS.BYTE.DIVISION.UNIFORM256.S14.json
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

The three-level chain provides:

1. **Index integrity**: No receipt can be modified without changing its hash in the index
2. **Receipt integrity**: No manifest can be modified without changing its hash in the receipt
3. **Self-authentication**: No receipt can be forged without recomputing the self-hash

### Staging and Atomic Promotion

Evidence is generated into `evidence.staging/<run-id>/`. On success (all 144 courts
pass, all verdicts `admitted_match`, zero residuals), the staging directory is
atomically swapped into `evidence/`. On failure, the existing canonical evidence
is preserved and the staging directory path is printed.

The atomic swap is performed by:

1. Generate all evidence into `evidence.staging/<timestamp>/`
2. Verify all 144 courts pass
3. Create a hardlink farm from `evidence/<run-id>/` pointing to the staging files
4. Verify the hardlink count matches (no partial state)

This guarantees that `evidence/` never contains partial or corrupted evidence.

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `RANS_EVIDENCE_DIR` | `evidence/` | Output directory |
| `RANS_GIT_COMMIT` | `git rev-parse HEAD` | Commit hash in evidence |
| `RANS_EVIDENCE_STAGING` | (unset) | Enable staging + atomic swap |
| `RANS_ORACLE_TIMEOUT_MS` | 30000 | Oracle subprocess timeout |

---

## Docker Matrix

The oracle can be run inside Docker containers to verify reproducibility across
different toolchains, operating systems, and CPU architectures.

### Docker Images

| Image | Compiler | OS | Purpose |
|-------|----------|----|---------|
| `ryg-rans-oracle:gcc-12` | GCC 12 | Ubuntu 22.04 | Primary reference |
| `ryg-rans-oracle:gcc-13` | GCC 13 | Ubuntu 24.04 | Compiler-independent verification |
| `ryg-rans-oracle:clang-16` | Clang 16 | Ubuntu 24.04 | Clang parity verification |
| `ryg-rans-oracle:alpine` | GCC (musl) | Alpine 3.19 | Musl libc verification |

### Running in Docker

```sh
# Build all oracle Docker images
docker compose -f oracle/docker/docker-compose.yml build

# Run courts inside Docker (GCC 12 reference)
docker compose -f oracle/docker/docker-compose.yml run oracle-gcc-12

# Verify with Clang
docker compose -f oracle/docker/docker-compose.yml run oracle-clang-16

# Run with specific evidence directory
docker compose -f oracle/docker/docker-compose.yml run \
    -e RANS_EVIDENCE_DIR=/evidence \
    -v $(pwd)/evidence:/evidence \
    oracle-gcc-12
```

### Docker Build

```dockerfile
FROM ubuntu:22.04 AS builder
RUN apt-get update && apt-get install -y build-essential git
COPY oracle/adapter/ /oracle/adapter/
RUN cd /oracle/adapter && make

FROM ubuntu:22.04
COPY --from=builder /oracle/adapter/rans_trace /usr/local/bin/
COPY crates/ryg-rans-rs-oracle/ /oracle/
WORKDIR /oracle
CMD ["cargo", "run", "--release", "--", "rans_trace", "12", "42", "20"]
```

---

## Usage

### Generate Full Evidence

```sh
cd oracle/adapter && make
RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20
```

The arguments are:

1. `oracle/adapter/rans_trace` — path to compiled C oracle binary
2. `12` — default scale bits (overridden per court)
3. `42` — default seed (overridden per court)
4. `20` — random seed trials for statistical coverage

### Verify

```sh
cargo xtask seal
```

The `xtask seal` command:

1. Reads the canonical `evidence/` directory
2. Verifies the SHA-256 chain (index → receipts → manifests)
3. Checks all 144 receipts have verdict `admitted_match`
4. Checks zero residuals in any court
5. Exits with code 0 on success, non-zero on failure

### Partial Court Run

```sh
# Run only AVX-512 courts
cargo run -p ryg-rans-rs-oracle -- \
    oracle/adapter/rans_trace 12 42 20 \
    --filter "AVX512"

# Run only byte rANS courts
cargo run -p ryg-rans-rs-oracle -- \
    oracle/adapter/rans_trace 14 42 20 \
    --filter "BYTE"
```

---

## Performance Benchmark

```sh
# All backends: scalar, SSE4.1, AVX512VL 8-way, AVX512 16-way
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace

# With hardware counters
sudo perf stat -r 10 -e cycles,instructions,branches,branch-misses \
    RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
    cargo run --release --bin perf -- oracle/adapter/rans_trace
```

---

## Dependency Graph

```
ryg-rans-rs-oracle
  ├── ryg-rans-rs-core (alloc)     — Reference encode/decode
  ├── ryg-rans-rs-casefile          — Evidence schema types
  └── ryg-rans-rs-simd             — SIMD decode backends
```

The casefile crate has **no rANS dependency** — it is a pure schema crate that can be
used by downstream consumers, auditors, and researchers independently.

---

*Part of the ryg-rans-rs project. Version 0.1.27. Phase G/H.*
