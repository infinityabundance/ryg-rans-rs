# Workspace Architecture

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity — every Rust encoder/decoder must produce and consume byte-identical streams to the upstream C/C++ reference. Residual primacy — every observed diff is a first-class artifact until resolved.

---

## Workspace Overview

The workspace is organized as a monorepo with six crates and one automation package (`xtask`). The architecture enforces a **deterministic core isolation** strategy: the algorithmic heart lives in a `no_std`, `unsafe`-free crate, while platform-specific acceleration, developer tooling, and public API layers build on top of it without compromising the core's guarantees.

```
ryg-rans-rs/
├── crates/
│   ├── ryg-rans-core/        # no_std, no_unsafe algorithmic core
│   ├── ryg-rans-simd/        # SSE4.1 accelerated decoder kernels
│   ├── ryg-rans/             # Public facade crate
│   ├── ryg-rans-oracle/      # Development-only oracle harness
│   ├── ryg-rans-casefile/    # Typed schemas for deterministic testing
│   └── ryg-rans-cli/         # CLI tools
├── xtask/                    # Build system automation
├── oracle/                   # Oracle adapter & scripts (C/C++ compilation)
├── cases/                    # Deterministic test case payloads
├── tests/                    # Integration test suites
├── reports/                  # Oracle court receipts & residual records
└── docs/                     # Project documentation
```

---

## Deterministic Core Isolation

The core isolation principle is:

1. **ryg-rans-core** contains all rANS arithmetic — state transitions, renormalization, reciprocal encoding, symbol table construction, interleaved encoding/decoding — in pure safe Rust.
2. Everything above the core may use platform intrinsics, `std`, or `alloc`, but must **never alter the arithmetic results**. The core's state machine is the ground truth.
3. The facade crate (`ryg-rans`) re-exports core types and conditionally adds SIMD acceleration, but the SIMD paths must produce the same state transitions as the scalar core on every input.

This means every accelerated kernel has an equivalent scalar reference in `ryg-rans-core` that it can be cross-checked against.

---

## Crate Duties

### `crates/ryg-rans-core` — Algorithmic Core

| Property | Value |
|---|---|
| `#![no_std]` | Yes |
| `#![forbid(unsafe_code)]` | Yes |
| `alloc` feature | Optional (behind feature flag) |
| Upstream file | `rans_byte.h`, `rans64.h` |

Provides:
- 32-bit byte-aligned rANS: state, encoder symbols, decoder symbols, renormalization, division-based put, reciprocal fast put, flush, decoder init/get/advance, step operations, interleaved two-state mode.
- 64-bit rANS: 64-bit state, 32-bit word renormalization, 128-bit `mul_hi` for reciprocal.
- Backward byte/word writers and forward byte/word readers as trait-based abstractions.
- Trait-based I/O (`BackwardWriter`, `ForwardReader`) for zero-cost abstraction over buffer types.
- Slice-based writer/reader implementations for immediate use.

No unsafe code is permitted. All arithmetic is verified against the upstream C via the oracle courts.

### `crates/ryg-rans-simd` — SSE4.1 Decoder Kernels

| Property | Value |
|---|---|
| `#![no_std]` | Yes |
| `unsafe` | Yes (intrinsics) |
| Target feature | SSE4.1 (`sse4.1`) |
| Upstream file | `rans_word_sse41.h` |

Provides:
- SSE4.1 accelerated four-lane word-aligned decoder.
- Shuffle-based byte extraction and sign-biased unsigned comparison.
- Renormalization with 16 precomputed masks.

Each `unsafe` block must be documented with its preconditions, alignment assumptions, bounds, CPU feature requirements, and a soundness justification. The SIMD decoder is cross-checked against the scalar core via the oracle courts before being marked `full`.

### `crates/ryg-rans` — Public Facade

| Property | Value |
|---|---|
| `#![no_std]` | Yes |
| `#![deny(unsafe_code)]` | Yes |
| Default feature | `simd` |

This is the crate users depend on. It re-exports `ryg-rans-core` as `ryg_rans::byte` and conditionally re-exports `ryg-rans-simd` as `ryg_rans::simd`. It adds:
- Convenience `alloc`-based encode/decode functions that wrap the core primitives.
- No new algorithmic logic — purely a re-export and convenience layer.

The facade is safe code. It is the single public entry point for consumers.

### `crates/ryg-rans-casefile` — Schemas

| Property | Value |
|---|---|
| `#![no_std]` | Yes (extern crate alloc) |
| `unsafe` | No |
| Dependencies | `serde`, `sha2` |

Provides typed schemas for:
- **`Casefile`**: A deterministic test case — variant, seed, input data, frequency model, scale bits, interleave factor. Includes the pinned upstream commit hash.
- **`Receipt`**: A court verdict — how many pairs were compared, how many matched, residual count.
- **`Residual`**: A documented difference — case ID, court ID, variant, class, severity, status.

Casefiles are the serializable ground truth for deterministic testing across Rust and C/C++.

### `crates/ryg-rans-oracle` — Oracle Harness (dev-only)

| Property | Value |
|---|---|
| `#![no_std]` | No (uses `std`) |
| `unsafe` | No |
| Dependencies | `ryg-rans-core`, `ryg-rans-casefile`, `serde_json`, `sha2` |

This crate orchestrates oracle courts. It:
- Spawns and communicates with compiled upstream C/C++ binaries.
- Loads deterministic casefiles, feeds them to both Rust and C code.
- Compares state transitions, bitstreams, and decoded output.
- Produces receipts and residuals.

This crate is **never** shipped to consumers. It lives in the workspace for CI and local verification.

### `crates/ryg-rans-cli` — CLI Tools

| Property | Value |
|---|---|
| `unsafe` | No |
| Dependencies | `ryg-rans`, `ryg-rans-core`, `clap` |

Subcommands: `encode`, `decode`, `inspect`, `trace`, `compare`, `bench`. Used for manual testing, debugging, and performance measurement. Not intended for production use.

### `xtask` — Automation

| Property | Value |
|---|---|
| `unsafe` | No |
| Dependencies | `pico-args`, `serde_json` |

Commands:
- `bootstrap` — Initialize workspace (Docker images, git submodules, oracle binaries).
- `gen` — Generate documentation from `docs-src/`.
- `check` — Verify all gates pass (tests, oracle courts, residuals tracked).
- `seal` — Run release-critical gates.
- `court <id>` / `court --all` — Run oracle courts.
- `cases generate / verify` — Manage deterministic casefiles.
- `residuals list / verify / reproduce / minimize` — Manage residual lifecycle.
- `no-ffi` — Assert zero FFI dependencies.
- `no-upstream-source` — Assert no upstream C/C++ source in the workspace tree.
- `package-audit` — Pre-release package validation.

---

## Dependency Graph

```
ryg-rans-simd ──depends on──> ryg-rans-core
ryg-rans       ──depends on──> ryg-rans-core, optional: ryg-rans-simd
ryg-rans-oracle ──depends on──> ryg-rans-core, ryg-rans-casefile
ryg-rans-cli   ──depends on──> ryg-rans, ryg-rans-core
ryg-rans-casefile ─> (standalone, no rANS dependencies)
xtask          ─> (standalone, no rANS dependencies)
```

The dependency direction ensures:
- The core has no knowledge of SIMD, CLI, or oracle logic.
- The facade is the only crate consumers import.
- Oracle and casefile are independently usable.

---

## No FFI Policy

The workspace does **not** bind to the upstream C/C++ via FFI. All oracle comparison is done via process-level communication (compiled binaries). This ensures:
- No unsafe FFI boundaries to audit.
- No C/C++ toolchain required to build the Rust project.
- Clear separation between reference implementation and port.
