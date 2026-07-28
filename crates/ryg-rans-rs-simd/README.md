# ryg-rans-rs-simd

> SSE4.1 accelerated rANS decoder kernels.

## Status

**Scaffold.** No implementation yet. This crate will provide:

- Four-lane SIMD decode (4 symbols per cycle)
- Two-decoder eight-stream interleaved decode
- 128-bit lane management for 32-bit rANS states

No operations are currently implemented. The crate exists to reserve the name and establish the API surface for when SIMD acceleration is added.
