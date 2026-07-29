# Negative Capabilities Ledger

> This document explicitly states what `ryg-rans-rs` does **not** yet claim.

## Algorithmic Surfaces

| Capability | Status | Rationale |
|---|---|---|
| Big-endian 64-bit rANS parity | Not claimed | Upstream format is native-endian `uint32_t`; Rust `to_le()` would differ on BE hosts |
| AVX-512 decode | Not claimed | Not yet implemented. Future plan: packed-table gather + masked renormalization |
| NEON support | Not claimed | Upstream is x86-only; no oracle reference |
| Non-x86 SIMD parity | Not claimed | No upstream reference to compare against |
| Interleaved4 or Interleaved16 streams | Not claimed | Only 2-way (byte/r64) and 8-way (SIMD word) interleaving are implemented |

## Safety Properties

| Property | Status | Rationale |
|---|---|---|
| Malformed-stream fuzzing coverage | **Fuzzing set up** (5 targets) | cargo-fuzz targets exist but have not run millions of iterations in CI yet |
| Constant-time behavior | Not claimed | Not intended by upstream design |

## Stability

| Property | Status |
|---|---|
| Stable compressed-file container format | Not claimed |
| MSRV guarantee | Not yet |
| Production maturity | Not yet — pre-release |

## Performance

| Property | Status |
|---|---|
| Performance parity on unmeasured processors | Not claimed |
| Performance parity on non-x86_64 hosts | Not claimed |
| Faster-than-upstream claims | Cannot yet make — SSE4.1 is ~2.5× slower than scalar on 9800X3D |
| AVX-512 throughput comparison | Not yet measured |
| Hardware counter analysis (cycles, cache misses) | Not yet run — `perf stat` methodology documented but counters not recorded |
