# ryg-rans-rs

> **A native Rust forensic reconstruction of Fabian Giesen's public-domain `ryg_rans`**  
> Verified through bit-exact cross-decoding, state-transition courts, deterministic residual analysis, and matched performance measurements.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.85%2B-stable)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0.html)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs)](https://crates.io/crates/ryg-rans-rs)

---

## Overview

**ryg-rans-rs** is a from-scratch, native Rust implementation of the Asymmetric Numeral Systems (ANS) entropy coder variants published in Fabian "ryg" Giesen's seminal [ryg_rans](https://github.com/rygorous/ryg_rans) repository.

This is **not** a wrapper, binding, FFI facade, or syntax-level translation of the C++ reference. It is a reconstruction of the **observable arithmetic, state-transition, bitstream, table-generation, interleaving, endian, and performance behavior** of the admitted upstream revision, built through forensic parity courts.

### What's Implemented

| Surface | Crate | Status | Details |
|---------|-------|--------|---------|
| 32-bit byte-aligned rANS (division) | `ryg-rans-rs-core` | ✅ `implemented_unsealed` | Complete reference path |
| 32-bit byte-aligned rANS (reciprocal) | `ryg-rans-rs-core` | ✅ `implemented_unsealed` | Multiply-high fast path, freq=1 special case |
| 32-bit byte two-state interleaving | `ryg-rans-rs-core` | ✅ `implemented_unsealed` | Reverse pair ordering, exact flush order |
| 64-bit rANS (division) | `ryg-rans-rs-core` | ✅ `implemented_unsealed` | 64-bit state, 32-bit word renormalization |
| 64-bit rANS (reciprocal) | `ryg-rans-rs-core` | 🔶 `partial` | 128-bit mul_hi, cmpl_freq at full width |
| 64-bit rANS two-state interleaving | `ryg-rans-rs-core` | 🔶 `partial` | Primitives only |
| Word-aligned scalar rANS | — | 📋 `scaffold` | Not yet implemented |
| SSE4.1 SIMD decoder | — | 📋 `scaffold` | Not yet implemented |
| Alias method | — | 📋 `scaffold` | Not yet implemented |

### Project Doctrine

> **Bitstream parity, state-transition parity, performance-shape parity, operational-knowledge parity.**

The implementation method is **forensic parity courts** governed by **residual primacy**:

- Every arithmetic operation is compared against the compiled C/C++ oracle.
- Every encoded byte stream is verified byte-for-byte across both implementations.
- Every observed difference is recorded as a **residual** — a first-class artifact that must be classified, understood, and either resolved or explicitly admitted.
- No surface is labelled `full` until a sealed court receipt proves upstream parity.

### Implementation Architecture

```
ryg-rans-rs/
├── crates/
│   ├── ryg-rans-rs-core/      # no_std, forbid(unsafe) — deterministic algorithmic core
│   ├── ryg-rans-rs-simd/      # SSE4.1 accelerated kernels (future)
│   ├── ryg-rans-rs/           # Public facade with safe API, optional simd+alloc
│   ├── ryg-rans-rs-oracle/    # Dev-only oracle harness (never shipped)
│   ├── ryg-rans-rs-casefile/  # Typed schemas for court evidence
│   └── ryg-rans-rs-cli/       # CLI tools
├── xtask/                      # Build system automation & gate verification
├── oracle/                     # C/C++ oracle adapters
│   └── adapter/                # rans_trace — JSON-trace emitter (18 operations)
├── docs/                       # Architecture, contracts, methodology
├── docs-src/models/            # Upstream & parity machine-readable models
└── docker/                     # Docker Compose matrix definitions
```

### Key Design Decisions

- **`#![no_std]`** — The core encoding and decoding algorithms operate without the standard library.
- **`#![forbid(unsafe_code)]`** — The core crate guarantees no undefined behavior through Rust's type system.
- **Caller-provided storage** — No hidden allocation in encode/decode hot paths.
- **Explicit `Result` returns** — Encoding returns `Result<(), EncodeError>`, decoding returns `Result<(), DecodeError>`. No silent truncation or panics.
- **Oracle-gated development** — All claims require evidence from compiled C/C++ comparison, not just Rust round-trips.

---

## Quick Start

```sh
# Run all workspace tests
cargo test --workspace

# Run core algorithm tests (44 tests, no std required)
cargo test -p ryg-rans-rs-core

# Verify gates pass
cargo xtask check
```

### Basic Usage

```rust
use ryg_rans_rs::byte::{
    RansByteState, RansByteEncSymbol, RansByteDecSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_get,
    rans_byte_dec_advance_symbol,
};

// 1. Define a frequency model (scale_bits = 14, total = 16384)
let scale_bits = 14;
let total = 1u32 << scale_bits;
let freq_a = total / 3;
let freq_b = total / 3;
let freq_c = total - freq_a - freq_b;

let esym_a = RansByteEncSymbol::new(0, freq_a, scale_bits);
let esym_b = RansByteEncSymbol::new(freq_a, freq_b, scale_bits);
let esym_c = RansByteEncSymbol::new(freq_a + freq_b, freq_c, scale_bits);

let dsym_a = RansByteDecSymbol::new(0, freq_a);
let dsym_b = RansByteDecSymbol::new(freq_a, freq_b);
let dsym_c = RansByteDecSymbol::new(freq_a + freq_b, freq_c);

// 2. Encode symbols in reverse order
let input = [0u8, 1, 2, 0, 1, 2, 0, 1, 2, 0];
let mut out = [0u8; 1024];
let mut writer = BackwardByteWriter::new(&mut out);

let mut state = RansByteState::new();
for &s in input.iter().rev() {
    let esym = match s { 0 => &esym_a, 1 => &esym_b, _ => &esym_c };
    rans_byte_enc_put_symbol(&mut state, &mut writer, esym).unwrap();
}
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

// 3. Decode in forward order
let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();

let cum2sym: Vec<u8> = (0..total as usize)
    .map(|i| if i < freq_a as usize { 0 }
         else if i < (freq_a + freq_b) as usize { 1 } else { 2 })
    .collect();

let mut output = vec![0u8; input.len()];
for i in 0..input.len() {
    let cf = rans_byte_dec_get(&dec_state, scale_bits);
    let s = cum2sym[cf as usize];
    output[i] = s;
    let dsym = match s { 0 => &dsym_a, 1 => &dsym_b, _ => &dsym_c };
    rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits).unwrap();
}

assert_eq!(output, input); // Round-trip complete!
```

### Using the Convenience API (with `alloc` feature)

```rust
use ryg_rans_rs::byte::RansByteEncSymbol;
use ryg_rans_rs::alloc_utils;

let scale_bits = 14;
let total = 1u32 << scale_bits;
let base_freq = total / 256;

let esyms: Vec<_> = (0..256)
    .map(|i| RansByteEncSymbol::new(i * base_freq, base_freq, scale_bits))
    .collect();

let dsyms: Vec<_> = (0..256)
    .map(|i| ryg_rans_rs::byte::RansByteDecSymbol::new(i * base_freq, base_freq))
    .collect();

let data = b"Hello, rANS entropy coding!";
let encoded = alloc_utils::encode(data, &esyms, scale_bits);

let cum2sym: Vec<u8> = (0..total as usize)
    .map(|i| (i / base_freq as usize) as u8)
    .collect();

let decoded = alloc_utils::decode(&encoded, &cum2sym, &dsyms, scale_bits, data.len());
assert_eq!(&decoded, data);
```

---

## Crate Overview

### [`ryg-rans-rs-core`](crates/ryg-rans-rs-core/)

The deterministic algorithmic heart of the project. Contains:

- **32-bit byte-aligned rANS**: State init, renormalization, division-based put, reciprocal fast put, flush, decoder init/get/advance, step-only operations, decoder renormalization.
- **64-bit rANS**: 64-bit state with 32-bit word renormalization, `u128` multiply-high reciprocal, two-part 128-bit reciprocal setup.
- **Two-state interleaving**: Complete byte interleaved encoder/decoder with correct pair ordering and flush sequence.
- **Writer and reader abstractions**: `BackwardByteWriter`, `ByteReader`, `BackwardWord32Writer`, `Word32Reader` with trait abstractions.

Zero `unsafe`, zero `std` dependency, caller-provided storage.

### [`ryg-rans-rs-simd`](crates/ryg-rans-rs-simd/)

Scaffold for SSE4.1 accelerated decoder kernels. Will provide four-lane SIMD decode and two-decoder eight-stream orchestration when implemented.

### [`ryg-rans-rs`](crates/ryg-rans-rs/)

Public facade re-exporting `ryg-rans-rs-core`. Provides:

- `byte` module: direct re-exports of all core types and functions.
- `simd` module: (behind feature flag) SSE4.1 wrappers.
- `alloc_utils` module: (behind `alloc` feature) convenience encode/decode with `Vec<u8>`.

### [`ryg-rans-rs-oracle`](crates/ryg-rans-rs-oracle/)

Development-only harness for comparing Rust output against compiled C/C++ oracles. Not intended for shipping as a dependency.

### [`ryg-rans-rs-casefile`](crates/ryg-rans-rs-casefile/)

Typed schemas for court evidence: casefiles, receipts, residuals. Provides deterministic serialization for the forensic testing infrastructure.

### [`ryg-rans-rs-cli`](crates/ryg-rans-rs-cli/)

Command-line tools for encoding, decoding, inspection, and benchmarking with rANS.

### [`xtask`](xtask/)

Build system automation. Provides commands for gate verification (`cargo xtask check`), release sealing (`cargo xtask seal`), and documentation generation.

---

## C/C++ Oracle Adapter

The directory `oracle/adapter/` contains:

- **`rans_trace.cpp`**: A standalone C++ program (uses pinned upstream headers) that emits JSON-line traces of rANS operations.
- **`rans_byte.h`**, **`rans64.h`**, **`platform.h`**: Pinned upstream headers from `rygorous/ryg_rans`.

The adapter supports 18 operations covering both 32-bit byte and 64-bit rANS variants. Each operation outputs deterministic JSON with explicit-width fields for programmatic comparison.

Build:
```sh
cd oracle/adapter && make
```

Test:
```sh
# 32-bit reciprocal parameter initialization (matches Rust output)
./rans_trace enc-symbol-init 0 10 14

# 64-bit multiply-high
./rans_trace r64-mul-hi 1000000 5000000000
```

---

## Docker Matrix

All testing, validation, and benchmarking must run inside Docker containers created under the project's dedicated Docker root:

```text
/run/media/one/toshiba4TB/docker/ryg-rans-rs/
```

The Docker Compose matrix (`docker/compose/matrix.yml`) provides isolated, security-hardened jobs:

- `oracle-gcc`: Builds the C/C++ oracle binaries
- `rust-stable-tests`: Runs full Rust test suite
- `rust-musl-build`: Verifies musl target compilation
- `package-audit`: Verifies crate packaging

Containers use:
- Unprivileged execution with dropped capabilities
- tmpfs for temporary files
- Read-only source mounts
- Project-labelled named volumes
- Unique run-identified Compose project names

---

## Upstream Oracle

| Property | Value |
|----------|-------|
| Repository | [rygorous/ryg_rans](https://github.com/rygorous/ryg_rans) |
| Pinned Commit | [`c9d162d9`](https://github.com/rygorous/ryg_rans/commit/c9d162d996fd600315af9ae8eb89d832576cb32d) |
| Date | 2018-11-25 |
| Host | x86_64, little-endian |

---

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or https://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or https://opensource.org/licenses/MIT)

at your option.

## Acknowledgment

- **Fabian "ryg" Giesen** — for the original public-domain `ryg_rans` implementation and extensive blog documentation of rANS design.
- **Jarek Duda** — for the ANS framework (arxiv.org/abs/1311.2540).
- **Charles Bloom**, **Yann Collet**, **Eugene Shelwien** — for the broader entropy coding and ANS literature that informed this reconstruction.

---

## Contributing

This project follows a **forensic parity methodology**. Before contributing a new surface or optimization:

1. Study the upstream C/C++ reference behavior via the oracle adapter.
2. Implement the Rust equivalent with deterministic state transitions.
3. Verify against the compiled oracle.
4. Record any residuals as first-class artifacts.
5. Submit evidence, not just code.
