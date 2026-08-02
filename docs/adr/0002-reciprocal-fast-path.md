# ADR-0002 — Reciprocal-multiply fast path with the exact upstream bias

Status: Accepted

## Context
The encode transition needs `q = x / freq` and `r = x mod freq` per
symbol.  Integer division dominates the encode hot loop.

## Problem
How to replace division without changing the stream, which is defined by
the upstream bytes.

## Alternatives considered
1. Keep division only (correct, slow).
2. Replace division with Alverson's reciprocal multiply-high
   approximation, with a carefully chosen shift and bias.
3. Replace division with a per-symbol lookup table.

## Rejected alternatives
- (3) was rejected: the table for every possible frequency is large, and
  the multiply-high path is exact with the right bias.
- A "good enough" reciprocal with an off-by-one risk was rejected: the
  stream format is defined by the upstream bytes, and a decoder that uses
  a different reciprocal convention produces different bytes.

## Decision
The reciprocal path is implemented exactly as upstream: precompute
`recip_freq = ceil(2^shift / freq)` with the upstream shift budget and
the `(M - 1) << shift` error bias, and compute `q` by multiply-high.  The
division path is retained as the reference and proven equal.  The
`freq == 1` case is special-cased because the general reciprocal cannot
represent it within the shift budget.

## Tradeoffs
Gained: a 3–5× faster encode path.  Given up: none on correctness — the
identity is proven; but the path is now *pinned*: the bias is part of the
format contract and cannot be tuned without breaking cross-decoding.

## Evidence
Kani proofs `kani/reciprocal_proof.rs` (byte, freq {1,2,3,7,16,255,4095})
and `kani/r64_reciprocal_proof.rs`; oracle receipts; the CLI
`compare arithmetic` court; the fuzz round-trip targets.

## Future implications
A different upstream revision with a different bias would change this ADR.
The Kani proof instances are the accepted boundary (L16-E): the two
fully-symbolic instances are not bit-blastable within practical time.
