# Negative Capabilities Ledger

This document explicitly states what `ryg-rans-rs` does **not** yet claim.

## Algorithmic Surfaces

| Capability | Status | Rationale |
|---|---|---|
| Big-endian 64-bit rANS parity | Not claimed | Upstream format is native-endian `uint32_t`; Rust `to_le()` would differ |
| Word-aligned scalar rANS | Scaffold only | Not yet implemented |
| SSE4.1 SIMD decoder | Scaffold only | Not yet implemented |
| Alias method (table, encode, decode) | Scaffold only | Not yet implemented |
| Frequency normalization | Scaffold only | Not yet implemented |
| AVX2 support | Not claimed | Upstream does not include AVX2 in pinned revision |
| NEON support | Not claimed | Upstream is x86-only |
| Non-x86 SIMD parity | Not claimed | No upstream reference |

## Safety Properties

| Property | Status | Rationale |
|---|---|---|
| Undefined behavior on invalid models | Intentionally rejected | Rust API checks preconditions; upstream C assumes valid input |
| Out-of-bounds read on unpadded SIMD input | Intentionally rejected | Upstream SIMD code reads 8 bytes past end; Rust uses checked paths |
| Constant-time behavior | Not claimed | Not intended by upstream design |

## Stability

| Property | Status |
|---|---|
| Stable compressed-file container format | Not claimed |
| API stability across versions | Not yet |
| MSRV guarantee | Not yet |
| Production maturity | Not yet - pre-release |

## Performance

| Property | Status |
|---|---|
| Performance parity on unmeasured processors | Not claimed |
| Performance parity on non-x86_64 hosts | Not claimed |
| Faster-than-upstream claims | Cannot yet make - need performance courts |
