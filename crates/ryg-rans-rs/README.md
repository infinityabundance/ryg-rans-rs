# ryg-rans-rs

> Public facade for ryg-rans-rs forensic rANS implementation.

## Features

- `default`: Core re-export only.
- `simd`: Enables `ryg-rans-rs-simd` (SSE4.1 kernels, currently scaffolded).
- `alloc`: Enables convenience `encode`/`decode` wrappers that use `Vec<u8>`.

## Modules

- `byte` — 32-bit byte-aligned rANS types and functions (re-exported from core).
- `r64` — 64-bit rANS types and functions (re-exported from core).
- `simd` — SSE4.1 accelerated kernels (behind `simd` feature, scaffolded).
- `alloc_utils` — Convenience encode/decode with `Vec<u8>` (behind `alloc` feature).

## Published Versions

- `0.1.3` — Current. Scalar single-state profiles sealed.
- `0.1.2` — Phase A seal complete.
- `0.1.1` — Reseal with manifest hash chain fix.
- `0.1.0` — Initial publication.
