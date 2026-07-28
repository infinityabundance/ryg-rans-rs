# ryg-rans-rs-simd

> SSE4.1 accelerated rANS decoder kernels — scaffold crate

[![#![no_std]](https://img.shields.io/badge/std-no--std-blue)](https://docs.rs/ryg-rans-rs-simd)
[![MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/infinityabundance/ryg-rans-rs)

## Overview

This crate is a **scaffold** for SSE4.1-accelerated rANS decoder kernels. It exists as part of the
`ryg-rans-rs` workspace to reserve the API surface and feature-gate pathway for SIMD optimization.

Currently, the crate exports nothing — it is a placeholder containing only a module-level comment:

```rust
// SSE4.1 accelerated rANS decoder kernels
```

The architecture is designed so that the public facade crate (`ryg-rans-rs`) can optionally enable
this crate via the `simd` cargo feature, which in turn gates compilation of the SSE4.1 intrinsics.

## Planned Architecture

When implemented, this crate will contain:

- **`decode_forward_sse41`** — Batch-decode multiple rANS symbols in a single pass using SSE4.1
  packed integer arithmetic. The decoder advance step involves `freq * (x >> scale_bits) + (x & mask) - start`,
  which maps naturally to SSE4.1 `_mm_mullo_epi32`, `_mm_srlv_epi32`, and `_mm_add_epi32` /
  `_mm_sub_epi32` operations.

- **`decode_renorm_sse41`** — Batched renormalization using SSE4.1 compare-and-blend to process
  up to 4 byte-renormalization steps simultaneously.

- **Interleaved acceleration** — The two-state interleaved decoder is a natural target for SIMD:
  both states can be advanced in parallel in a single SIMD vector lane pair.

All SIMD kernels will be wrapped in safe Rust functions. Unsafe intrinsic calls will be confined
to the kernel implementations, with safe public APIs performing bounds checking and state validation.

## Feature Gate

The crate is currently gated behind the `simd` feature in the workspace facade:

```toml
# ryg-rans-rs/Cargo.toml
[features]
default = ["simd"]
simd = ["ryg-rans-rs-simd"]
```

When the SIMD kernels are implemented, this crate will also need target-CPU gates:

```rust
// Future: gate compilation to x86-64 with SSE4.1
#[cfg(all(target_arch = "x86_64", target_feature = "sse4.1"))]
pub mod sse41;
```

## Current Status

| Component | Status |
|-----------|--------|
| SSE4.1 forward decode kernel | Not yet implemented |
| SSE4.1 renormalization kernel | Not yet implemented |
| Interleaved SIMD acceleration | Not yet implemented |
| Safe wrapper API | Not yet designed |
| `#[cfg]` target gates | Not yet added |

This crate compiles but has no runtime effect. It exists so that `ryg-rans-rs` can declare the
`simd` dependency in `Cargo.toml` and the `simd` module in `lib.rs` without feature-resolution
errors. The consuming application can safely enable the `simd` feature today; it simply enables
an empty module.

## Dependencies

| Dependency | Version | Notes |
|------------|---------|-------|
| `ryg-rans-rs-core` | `0.1.0` | Algorithmic primitives (used as building blocks for SIMD kernels) |

When SIMD kernels are implemented, additional dependencies may include:

- `core::arch::x86_64` (built-in `core`) — for `_mm_*` SSE4.1 intrinsics
