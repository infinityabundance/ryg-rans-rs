# ryg-rans-rs

> **A native Rust forensic reconstruction of Fabian Giesen's public-domain `ryg_rans`**  
> **128 receipts across 5 algorithmic surfaces, sealed via bit-exact C↔Rust cross-decoding courts**  
> **Phase H: Malformed-stream hardening · Fuzzing · Kani formal proofs · Performance benchmarks**  
> **Ten-service Docker VM matrix verifies every build, test, oracle, court, and audit**

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-stable)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs)](https://docs.rs/ryg-rans-rs/latest/ryg_rans_rs/)

---

## Overview

**ryg-rans-rs** is a from-scratch, native Rust implementation of the Asymmetric Numeral Systems (ANS) entropy coder variants published in Fabian "ryg" Giesen's seminal [ryg_rans](https://github.com/rygorous/ryg_rans) repository.

This is **not** a wrapper, binding, or FFI facade. It is a reconstruction of the **observable arithmetic, state-transition, bitstream, and interleaving behavior** of the pinned upstream revision, built through forensic parity courts.

Each algorithmic surface is verified by **cross-decoding courts**: the Rust implementation and the compiled C/C++ oracle encode and decode the same deterministic inputs, producing byte-identical streams and bit-exact state transitions. Every court produces a signed receipt with SHA-256 chains linking the manifest, receipt, and evidence index.

### Current Evidence

| Surface | Behaviour Status | Performance Status | Receipts |
|---------|-----------------|-------------------|----------|
| 32-bit byte rANS (division + reciprocal, single-state, uniform-256, scale=12) | **Sealed** | Unsealed | 44 |
| 32-bit byte rANS (interleaved2, division + reciprocal, uniform-256, scale=12) | **Sealed** | Unsealed | — (included above) |
| 64-bit rANS (division + reciprocal, single-state, uniform-256, scale=12) | **Sealed** | Unsealed | 44 |
| 64-bit rANS (interleaved2, division + reciprocal, uniform-256, scale=12) | **Sealed** | Unsealed | — (included above) |
| Word-aligned scalar rANS (division, single-state + interleaved2, uniform-256, scale=12) | **Sealed** | Unsealed | 16 |
| Alias method (byte rANS + Vose alias table, single-state + interleaved2, 8 profiles, scale=12) | **Sealed** | Unsealed | 16 |
| SSE4.1 SIMD decoder (8-way interleaved, 8 profiles, scale=12) | **Sealed** | Unsealed | 8 |
| **Total** | | | **128** |

**Evidence structure**: Each sealed receipt is a SHA-256-chained artifact — a machine-readable `CaseManifest` (containing all deterministic input cases, frequency models, C and Rust compressed streams, and per-case verdicts) plus a `Receipt` (containing verdict, `code_commit`, `manifest_sha256`, `receipt_sha256` self-hash). The receipts are registered in `evidence/index.json` and verified by the 16-gate seal check.

### Phase H Deliverables (Current)

| Component | Status | Description |
|-----------|--------|-------------|
| **Malformed-stream hardening** | ✅ Done | `malformed` module: pre-decode validation, renormalization guards, frequency model validation, edge-case detection |
| **Fuzzing (cargo-fuzz)** | ✅ Set up | 5 targets: byte/r64/word/alias roundtrip + malformed byte fuzz |
| **Kani formal proofs** | ✅ Done | 4 proof harnesses: symbol init, reciprocal=division (byte + r64), encode-decode inversion |
| **Performance benchmarks** | ✅ Upgraded | Multi-profile × multi-size measurement, median-based, output allocation outside timed loop |
| **Documentation** | ✅ Updated | All crate READMEs, gap ledger, negative capabilities, unsafe ledger |

### Current Limitations

- **Performance is measured** (`cargo run --release --bin perf`) but no cycle-level hardware-counter readings are sealed. The benchmark methodology is documented and reproducible.
- **Fuzzing infrastructure** is set up but has not run millions of iterations in CI. The targets exist and compile but require `cargo fuzz run` on a fuzzing host.
- **Kani proofs** verify bounded model checking and pass, but are not run in the seal gate (Kani is a large dependency).
- **SSE4.1 SIMD decoder is slower than scalar** on Ryzen 7 9800X3D (~0.41× speedup). Future AVX-512 work may use packed-table gathers to reverse this.

---

## Project Doctrine

> **Bitstream parity, state-transition parity, performance-shape parity, operational-knowledge parity.**

The implementation method is **forensic parity courts** governed by **residual primacy**:

- Every arithmetic operation is compared against the compiled C/C++ oracle via `oracle/adapter/rans_trace.cpp`.
- Every encoded byte stream is verified byte-for-byte across both implementations in both directions (C→Rust and Rust→C).
- Every observed difference is recorded as a **residual** — a first-class artifact that must be classified, understood, and either resolved or explicitly admitted.
- No surface is labelled `full` until a sealed court receipt proves upstream parity.
- No seal is accepted without a clean Docker VM matrix run producing the evidence.

### The Seal Gate

The project's `cargo xtask seal` command enforces 16 mandatory gates:

1. **Dirty-tree gate**: No uncommitted changes to covered source files.
2. **Workspace check**: `cargo check --workspace` produces no errors.
3. **Core tests**: `cargo test -p ryg-rans-rs-core` passes (57+ tests).
4. **Parity model valid JSON**: `docs-src/models/parity.model.json` is well-formed.
5. **Upstream reference exists**: `docs-src/models/upstream.json` is present.
6. **Every claim has a receipt**: Each entry in `parity.model.json` has a matching receipt file.
7. **Court path valid for variant**: The court-path field matches the variant's expectations.
8. **Receipts exist on disk**: Every indexed receipt file is present.
9. **Index receipts cited in parity model**: Every index entry has a matching claim in the model.
10. **Evidence index**: All indexed receipts are accounted for.
11. **Receipt SHA-256 hashes**: Every receipt's hash matches its file content.
12. **Manifest SHA-256 hashes**: Every manifest's hash matches its file content.
13. **Receipt SHA-256 self-hashes**: Every receipt's embedded self-hash matches.
14. **Source freshness**: No source files changed after the evidence code commit.
15. **Forbid unsafe**: Core and casefile crates enforce `forbid(unsafe_code)`.
16. **Docker matrix evidence**: A clean 10-service Docker VM matrix run confirms the evidence.

---

## Crate Map

| Crate | Description | `no_std` | `unsafe` | Key Features |
|-------|-------------|----------|----------|--------------|
| [`ryg-rans-rs-core`](./crates/ryg-rans-rs-core) | Deterministic algorithmic core | ✅ Yes | ✅ Forbid | Byte rANS, 64-bit rANS, word rANS, alias method, malformed validation, Kani proofs |
| [`ryg-rans-rs-simd`](./crates/ryg-rans-rs-simd) | SSE4.1 accelerated decode kernels | ✅ Yes | ⚠️ Selective | 8-way interleaved SIMD decode, scalar fallback, unsafe ledger documented |
| [`ryg-rans-rs`](./crates/ryg-rans-rs) | Public facade crate | ✅ Yes | ✅ Deny | Re-exports core + optional SIMD |
| [`ryg-rans-rs-oracle`](./crates/ryg-rans-rs-oracle) | Forensic court harness | ❌ No | ❌ No | Cross-decoding courts, evidence generation, perf benchmarks |
| [`ryg-rans-rs-casefile`](./crates/ryg-rans-rs-casefile) | Typed evidence schemas | ✅ Yes | ❌ No | CaseResult, Receipt, Manifest types |
| [`ryg-rans-rs-cli`](./crates/ryg-rans-rs-cli) | CLI tools (scaffold) | ❌ No | ❌ No | Planned: encode, decode, inspect, trace, bench |

---

## Architecture

### Deterministic Core Isolation

The crate hierarchy enforces a strict isolation strategy: the algorithmic heart lives in `ryg-rans-rs-core` (`no_std`, `unsafe`-free), while platform-specific acceleration builds on top without compromising the core's guarantees.

```
ryg-rans-rs-core    → no_std, forbid(unsafe_code) — algorithmic ground truth
    ↓                        ↓
ryg-rans-rs-simd     ryg-rans-rs-casefile
    ↓                        ↓
ryg-rans-rs          ryg-rans-rs-oracle
(facade re-export)   (court harness, dev only)
```

### Implemented Surfaces

#### 32-bit Byte rANS (`rans_byte.h`)
- Division-based encode: `C(s,x) = ((x/freq) << scale_bits) + (x%freq) + start`
- Reciprocal fast encode: multiply-high approximation avoiding integer division
- Two-state interleaved encode/decode
- Backward byte writer, forward byte reader I/O abstractions
- **Kani-proven**: reciprocal = division, encode ∘ decode = identity

#### 64-bit rANS (`rans64.h`)
- 63-bit effective state with 32-bit word renormalization
- 128-bit `mul_hi` for reciprocal encoding
- Same division and reciprocal paths as byte rANS
- Two-state interleaved encode/decode
- **Kani-proven**: 64-bit reciprocal = division (up to scale_bits=31)

#### Word-aligned rANS (`rans_word_sse41.h`, scalar path)
- 16-bit word renormalization (L=2^16)
- Table-based decode: 4096-slot frequency/bias table
- Division-based encode with word renormalization
- Two-state interleaved encode/decode
- **Fuzz-tested**: word rANS roundtrip

#### Alias Method (`main_alias.cpp`)
- Vose's alias table construction for O(1) symbol decode
- Frequency normalization with zero-frequency theft
- Division-based encode with alias remap
- Single-state and interleaved2 modes
- **Fuzz-tested**: alias roundtrip

#### SSE4.1 SIMD Decoder (`rans_word_sse41.h`, SIMD path)
- 4-lane SIMD decode using `RansSimdDecSym` / `RansSimdDecRenorm`
- 8-way interleaved decode (two 4-lane units)
- Scalar gather for table lookups (no AVX2 gather)
- 16 precomputed shuffle masks for byte extraction
- Sign-biased unsigned comparison for renormalization
- Scalar 8-way reference decoder for verification
- **Note**: Slower than scalar on Ryzen 7 9800X3D due to gather overhead
- **Perf-measured**: 5 profiles × 7 sizes, GiB/s and ns/symbol reported

#### Malformed-Stream Hardening (`malformed` module)
- Pre-decode validation: minimum stream length checks
- Renormalization guards: loop-bound to prevent infinite loops
- Frequency model validation: monotonic cumulative, range bounds
- Edge-case detection: dominant symbol, single symbol, freq=1
- 12 dedicated unit tests

---

## Quick Start

```rust
// Basic encode/decode with byte rANS
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol, RansByteDecSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let total = 1u32 << scale_bits;
let freq = total / 256;
let mut buf = [0u8; 4096];

// Encode
let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
let sym = RansByteEncSymbol::new(0, freq, scale_bits).unwrap();
rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

// Decode
let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let dsym = RansByteDecSymbol::new(0, freq).unwrap();
rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
```

---

## Phase H Usage

### Malformed-Stream Validation

```rust
use ryg_rans_rs::byte::malformed::{
    validate_byte_compressed, RenormGuard, validate_freq_model,
};

// Before decoding untrusted input:
if let Err(e) = validate_byte_compressed(compressed) {
    return Err(e);
}

// During renormalization of untrusted input:
let mut guard = RenormGuard::new_byte();
loop {
    guard.check()?; // limits iterations
    let b = reader.read_byte().ok_or(DecodeError::InputTooShort)?;
    x = (x << 8) | (b as u32);
    if x >= RANS_BYTE_L { break; }
}
```

### Fuzzing

```sh
# Run individual fuzz targets
cargo fuzz run byte_rans_roundtrip
cargo fuzz run malformed_byte
cargo fuzz run r64_rans_roundtrip
cargo fuzz run word_rans_roundtrip
cargo fuzz run alias_roundtrip
```

### Kani Formal Proofs

```sh
# Requires Kani (cargo install kani-verifier)
kani crates/ryg-rans-rs-core/kani/enc_symbol_new_proof.rs
kani crates/ryg-rans-rs-core/kani/reciprocal_proof.rs
kani crates/ryg-rans-rs-core/kani/r64_reciprocal_proof.rs
kani crates/ryg-rans-rs-core/kani/encode_decode_inversion_proof.rs
```

### Performance Benchmark

```sh
# Build C oracle
cd oracle/adapter && make

# Run benchmark (no SIMD)
cargo run --release --bin perf -- oracle/adapter/rans_trace

# Run benchmark with SIMD + specific size
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo run --release \
    --bin perf -- oracle/adapter/rans_trace 4096
```

---

## Evidence Reproducibility

```sh
# Build the C oracle adapter
cd oracle/adapter && make

# Generate evidence (10+ minutes, full 128-receipt suite)
RANS_EVIDENCE_STAGING=1 cargo run -p ryg-rans-rs-oracle \
    -- oracle/adapter/rans_trace 12 42 20

# Verify all gates
cargo xtask seal

# Run Docker VM matrix (2+ hours)
cargo xtask docker
```

---

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT License](LICENSE-MIT), at your option.

---

## References

- Fabian Giesen, [ryg_rans](https://github.com/rygorous/ryg_rans) — Public-domain rANS encoder/decoder
- Jarek Duda, [Asymmetric Numeral Systems](https://arxiv.org/abs/0902.0271) — Original ANS paper
- Charles Bloom, [Understanding ANS](https://cbloomrants.blogspot.com/) — ANS tutorial series
- Alverson, "Integer Division using Reciprocals" — Multiply-high reciprocal approximation
