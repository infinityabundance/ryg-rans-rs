# ryg-rans-rs

> **A native Rust forensic reconstruction of Fabian Giesen's public-domain `ryg_rans`**  
> **144 receipts across 7 algorithmic surfaces, sealed via bit-exact C↔Rust cross-decoding courts**  
> **Phase G: Native AVX-512 rANS — AVX512VL.INTERLEAVED8 + AVX512.INTERLEAVED16**  
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
| 32-bit byte rANS (division + reciprocal) | **Sealed** | Unsealed | 44 |
| 64-bit rANS (division + reciprocal) | **Sealed** | Unsealed | 44 |
| Word-aligned scalar rANS (division) | **Sealed** | Unsealed | 16 |
| Alias method (byte rANS + Vose alias table) | **Sealed** | Unsealed | 16 |
| SSE4.1 SIMD decoder (8-way interleaved) | **Sealed** | Unsealed | 8 |
| **AVX512VL.INTERLEAVED8** (AVX-512 8-way) | **Sealed** | Unsealed | 8 |
| **AVX512.INTERLEAVED16** (AVX-512 16-way) | **Sealed** | Unsealed | 8 |
| **Total** | | | **144** |

**Evidence structure**: Each sealed receipt is a SHA-256-chained artifact — a machine-readable `CaseManifest` (containing all deterministic input cases, frequency models, C and Rust compressed streams, and per-case verdicts) plus a `Receipt` (containing verdict, `code_commit`, `manifest_sha256`, `receipt_sha256` self-hash). The receipts are registered in `evidence/index.json` and verified by the 16-gate seal check.

### Phase G Deliverables

| Component | Status | Description |
|-----------|--------|-------------|
| **Packed table** (`u32` gather-optimized) | ✅ Done | 4096-slot, freq\|bias<<12\|sym<<24, 64-byte aligned |
| **AVX512VL.INTERLEAVED8** decoder | ✅ Done | 8-way AVX-512VL gather decode, existing format |
| **AVX512.INTERLEAVED16** format | ✅ Done | New 16-way stream: scalar encode/decode + C oracle |
| **AVX512.INTERLEAVED16** decoder | ✅ Done | 16-way AVX-512 gather decode, 512-bit |
| **Backend dispatch** | ✅ Done | Runtime detection + auto-select + explicit backends |
| **C oracle** (16-way) | ✅ Done | Independent C encode/decode for cross-verification |
| **Fuzz targets** (2 new) | ✅ Done | AVX512VL8 + AVX512 16-way roundtrip fuzz |
| **Kani proofs** | ✅ Done | Packed entry fields + state bounds + slot index |
| **Mask tests** (256 + 65536) | ✅ Done | Exhaustive renormalization mask verification |
| **Malformed input tests** | ✅ Done | Truncated, wrong-format, state invariant tests |
| **32 tests** | ✅ Done | All pass, 0 failures |
| **Unsafe ledger** | ✅ Done | 7 blocks, fully documented |
| **Publish v0.1.15** | ✅ Done | All 6 crates on crates.io |

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
| [`ryg-rans-rs-core`](./crates/ryg-rans-rs-core) | Deterministic algorithmic core | ✅ Yes | ✅ Forbid | Byte rANS, R64, Word rANS, alias, malformed, Kani proofs |
| [`ryg-rans-rs-simd`](./crates/ryg-rans-rs-simd) | SSE4.1 + AVX-512 decode kernels | ✅ Yes | ⚠️ Selective | SSE4.1 8-way, AVX512VL 8-way, AVX512 16-way, scalar fallback |
| [`ryg-rans-rs`](./crates/ryg-rans-rs) | Public facade crate | ✅ Yes | ✅ Deny | Re-exports core + optional SIMD |
| [`ryg-rans-rs-oracle`](./crates/ryg-rans-rs-oracle) | Forensic court harness | ❌ No | ❌ No | Cross-decoding courts, evidence generation, perf benchmarks |
| [`ryg-rans-rs-casefile`](./crates/ryg-rans-rs-casefile) | Typed evidence schemas | ✅ Yes | ❌ No | CaseResult, Receipt, Manifest types |
| [`ryg-rans-rs-cli`](./crates/ryg-rans-rs-cli) | CLI tools (scaffold) | ❌ No | ❌ No | Planned: encode, decode, inspect, trace, bench |

---

## Architecture

### Deterministic Core Isolation

```
ryg-rans-rs-core    → no_std, forbid(unsafe_code) — algorithmic ground truth
    ↓                        ↓
ryg-rans-rs-simd     ryg-rans-rs-casefile
    ↓                        ↓
ryg-rans-rs          ryg-rans-rs-oracle
(facade re-export)   (court harness, dev only)
```

### Phase G: AVX-512 Decode Surfaces

#### AVX512VL.INTERLEAVED8

- **8-way decode** using 256-bit AVX-512VL vectors
- Consumes the **existing canonical 8-way Word rANS stream**
- Uses `_mm256_i32gather_epi32` for packed-table gather
- Masked renormalization via `_mm256_cmplt_epu32_mask`
- Requires: `avx512f`, `avx512vl`, `avx512bw`
- Backend label: `avx512vl-8way`

#### AVX512.INTERLEAVED16

- **16-way decode** using 512-bit AVX-512 vectors
- **New stream format**: 16 states, reverse-flush (15→0), forward init (0→15)
- Uses `_mm512_i32gather_epi32` for packed-table gather
- Requires: `avx512f`, `avx512bw`
- Backend label: `avx512-16way`
- Independent C oracle for cross-verification

#### Stream Format: 16-way

```
Encoding:  symbols processed in reverse, lane = i & 15
Flush:     states 15, 14, ..., 1, 0 (each as low16, high16)
Init:      states 0, 1, ..., 15 (each from low16, high16)
Decode:    groups of 16, renorm in ascending lane order
Tail:      lanes 0..r-1 for remainder r (0..15)
```

### Packed Decode Table

All AVX-512 kernels use a packed `u32` gather table:

```text
bits  0..11   frequency
bits 12..23   bias
bits 24..31   symbol
```

4096 entries, 64-byte aligned. Equivalent to existing `RansWordSlot` representation.

---

## Safety Infrastructure

### Fuzzing (7 targets)

| Target | Format | What it verifies |
|--------|--------|------------------|
| `byte_rans_roundtrip` | Byte rANS | Division + reciprocal roundtrip |
| `r64_rans_roundtrip` | 64-bit rANS | Roundtrip + stream equivalence |
| `word_rans_roundtrip` | Word rANS | Single-state roundtrip |
| `malformed_byte` | Byte rANS | Never panics on corrupted input |
| `alias_roundtrip` | Alias method | Normalized alias roundtrip |
| `avx512vl8_roundtrip` | AVX512VL 8-way | Scalar/AVX512 equivalence on random inputs |
| `avx512_16way_roundtrip` | AVX512 16-way | Scalar/AVX512 + word consumption match |

### Kani Proofs (7 total)

- Encoder symbol init correctness
- Reciprocal = division (byte + R64)
- Encode-decode inversion
- Packed entry field extraction round-trip
- State update overflow bounds
- Slot index boundedness

### Mask Exhaustion Tests

- **256** 8-way renormalization masks verified
- **65,536** 16-way renormalization masks verified

### Malformed Input Tests (13+ tests)

- Truncated streams for both 8-way and 16-way
- Wrong-format detection (8-way → 16-way decoder)
- Final state invariant preservation
- Reader consumption parity

---

## Quick Start

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let total = 1u32 << scale_bits;
let freq = total / 256;
let mut buf = [0u8; 4096];

let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
let sym = RansByteEncSymbol::new(0, freq, scale_bits).unwrap();
rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let dsym = RansByteDecSymbol::new(0, freq).unwrap();
rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits).unwrap();
```

### AVX-512 Decode

```rust
use ryg_rans_rs::simd::backends::decode_interleaved8_auto;
use ryg_rans_rs::simd::packed_table::PackedWordTable;

let packed = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
let result = decode_interleaved8_auto(&compressed, &packed, expected_len).unwrap();
assert_eq!(result.backend.label(), "avx512vl-8way");
```

---

## Evidence Reproducibility

```sh
# Build the C oracle adapter
cd oracle/adapter && make

# Generate evidence (10+ minutes, full 144-receipt suite)
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
- Intel Intrinsics Guide — `_mm256_i32gather_epi32`, `_mm512_i32gather_epi32`
