# ryg-rans-rs-core

> `#![no_std]` + `#![forbid(unsafe_code)]` — deterministic rANS algorithmic core.  
> 7 surfaces, 144 receipts, bit-exact C↔Rust parity.  
> Includes: malformed-stream hardening, Kani formal proofs, packed table reference.

## Status

| Surface | Receipts | Verified |
|---------|----------|----------|
| Byte rANS (division + reciprocal) | 44 | C↔Rust cross-decode |
| 64-bit rANS (division + reciprocal) | 44 | C↔Rust cross-decode |
| Word rANS (division) | 16 | C↔Rust cross-decode |
| Alias method | 16 | C↔Rust cross-decode |
| SSE4.1 SIMD decoder | 8 | C↔Rust cross-decode |
| AVX512VL.INTERLEAVED8 | 8 | C↔Rust cross-decode |
| AVX512.INTERLEAVED16 | 8 | C↔Rust cross-decode |

## Phase G Contributions

The core crate provides the **packed table reference** implementation and the **16-way scalar encoder/decoder** that AVX-512 kernels are verified against.

- `PackedWordTable` construction validated in core via equivalence tests
- 16-way scalar encode/decode operates entirely in safe `no_std` Rust

## Feature Flags

- `default = []` — Core only, no std dependency
- `alloc` — Enables `AliasTable` construction and `Vec`-based APIs
- `std` — Enables `std::error::Error` impls for error types
