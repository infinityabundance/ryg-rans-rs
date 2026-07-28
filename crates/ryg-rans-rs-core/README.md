# ryg-rans-rs-core

> `#![no_std]` + `#![forbid(unsafe_code)]` — deterministic rANS algorithmic core.

## Status

**Sealed profiles** (single-state, uniform-256, scale=12):
- 32-bit byte rANS division
- 32-bit byte rANS reciprocal
- 64-bit rANS division
- 64-bit rANS reciprocal

**Implemented, unsealed**:
- 32-bit byte two-state interleaving

**Partial**:
- 64-bit two-state interleaving (primitives only)

## Contents

- **32-bit byte-aligned rANS**: State init, renormalization, division-based `RansEncPut`, reciprocal `RansEncPutSymbol`, flush, decoder init, `RansDecAdvance`, `RansDecAdvanceSymbol`, step-only operations, decoder renormalization.
- **64-bit rANS**: 64-bit state with 32-bit word renormalization, `u128` multiply-high reciprocal, `Rans64EncPut`, `Rans64EncPutSymbol`, `Rans64DecAdvance`, `Rans64DecAdvanceSymbol`.
- **Two-state interleaving**: `ByteInterleavedEncoder`, `ByteInterleavedDecoder` with correct reverse pair ordering, dual flush, and step+renorm decode sequence.
- **Writer/reader abstractions**: `BackwardByteWriter`, `ByteReader`, `BackwardWord32Writer`, `Word32Reader`.

## Design

- Zero `unsafe` — the `forbid(unsafe_code)` attribute is a compile-time guarantee.
- Zero `std` — works in embedded, kernel, and Wasm environments.
- Caller-provided storage — no hidden allocation in encode/decode hot paths.
- Explicit `Result` — `EncodeError` and `DecodeError` cover output exhaustion and malformed input.
- Checked constructors (`try_new`) — frequency-zero, overflow, and invalid scale_bits are rejected at construction time.

## Tests

```sh
cargo test -p ryg-rans-rs-core
# 44 tests covering encode/decode round-trips, reciprocal identity,
# freq=1 special case, large-scale 64-bit reciprocal, state transitions,
# renormalization, writer/reader exhaustion, interleaving.
```
