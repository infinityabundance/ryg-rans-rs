# ADR-0001 — Byte-exact reconstruction of the pinned upstream `ryg_rans`

Status: Accepted

## Context
The repository's purpose is a native Rust rANS codec that interoperates
with Fabian Giesen's public-domain `ryg_rans` — the de-facto reference for
rANS in the wild.  Interoperability means a stream encoded by this library
must decode on the original C and vice versa.

## Problem
Which standard defines the stream format: the upstream bytes, or our
documentation?

## Alternatives considered
1. Reimplement "in the spirit of" rANS with our own conventions.
2. Reconstruct the exact upstream stream formats, byte for byte.
3. Write our own format and provide a converter.

## Rejected alternatives
- (1) was rejected: a codec whose stream is not cross-decodable with the
  reference is not rANS for practical purposes; every consumer of the
  reference would be incompatible.
- (3) was rejected: a converter is a second codec to maintain and a second
  source of defects.

## Decision
The stream formats are pinned to the upstream bytes, recorded in
`docs/bitstream-contract.md` with the exact upstream revision.  Every
surface must produce byte-identical output to the pinned reference,
proven by oracle courts.  Any change that alters an encoded stream is a
breaking format change that invalidates every receipt — this is the first
frozen invariant in `AGENTS.md`.

## Tradeoffs
Gained: true interoperability, a byte-exact oracle as the strongest
semantic test, and no format-ownership burden.  Given up: the freedom to
"improve" the stream format (e.g. different renormalization bounds).

## Evidence
The oracle receipts `RYG_RANS.BYTE.*`, `RYG_RANS.R64.*`,
`RYG_RANS.WORD.*`, `RYG_RANS.ALIAS.*`, `RYG_RANS.SIMD.INTERLEAVED8.*`,
`RYG_RANS.AVX512VL.*`, `RYG_RANS.AVX512.*` in `evidence/receipts/`;
`docs/oracle-method.md`.

## Future implications
A new codec surface (e.g. 128-bit rANS) would be a new format with its own
contract and receipts.  Changing the pinned upstream revision is a
format-level decision requiring full evidence regeneration.
