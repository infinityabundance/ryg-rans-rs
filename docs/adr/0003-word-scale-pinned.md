# ADR-0003 — Word coder pinned at scale 12 with a 4096-slot packed table

Status: Accepted

## Context
The word rANS surface must match `rans_word_sse41.h`, and all SIMD work
is built on its table layout.

## Problem
What scale, table size, and table representation to standardise on.

## Alternatives considered
1. `scale_bits = 12`, `M = 4096`, packed 12/12/8-bit u32 entries.
2. Higher scale (14) for better compression.
3. Unpacked slot+symbol arrays as the only representation.
4. `scale_bits = 10` for smaller tables.

## Rejected alternatives
- (2) and (4) were rejected: not upstream; changing the scale changes the
  stream format (the decoder's `& 0xfff` mask and the slot space are
  defined by scale 12).
- (3) was rejected as the *only* representation: the packed layout (one
  u32 per slot: `freq` 12 bits, `bias` 12 bits, `sym` 8 bits) is what
  makes a single 32-bit gather load an entire decode step.  The unpacked
  layout is retained as the scalar reference and for the SSE4.1 kernel.

## Decision
`RANS_WORD_SCALE_BITS = 12` is pinned (it is part of the bitstream
contract).  The packed table is 4096 × u32 = 16 KiB, 64-byte aligned,
constructed by `PackedWordTable::from_freqs` with full validation
(sum == 4096, monotonic cum, no zero-frequency symbols).

## Tradeoffs
Gained: L1-resident 16 KiB table, single-gather decode steps, exact
upstream compatibility.  Given up: the ability to tune scale per use.

## Evidence
`docs/papers/0002-word-rans.md`; the Kani `packed_entry_proof.rs`; the
report-parity courts; the bitstream contract.

## Future implications
A scale-14 surface would be a new codec with its own receipts, not a
parameter of this one.
