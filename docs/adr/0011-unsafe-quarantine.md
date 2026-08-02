# ADR-0011 — Unsafe quarantine: local `#[target_feature]` + a machine-checked ledger

Status: Accepted

## Context
The SSE helper functions relied on the target-feature context of their
callers, while the AVX2/AVX-512 kernels carried their own attributes.  A
helper compiled without `ssse3,sse4.1` but called from a feature-gated
context is undefined behaviour if the caller's context is the only thing
making it legal — and the caller's context is not an enforceable contract.

## Problem
How to make every unsafe function locally safe-by-construction.

## Alternatives considered
1. Keep caller-context reliance (the broken design).
2. Give every `unsafe fn` its own exact `#[target_feature]` attributes and
   a complete `# Safety` section.
3. Eliminate all unsafe (not possible: intrinsics require it).

## Rejected alternatives
- (1) was rejected (L10-A): a hidden caller obligation that could be
  encoded in a safe type is prohibited.
- (3) was rejected: SIMD intrinsics are unsafe by language definition.

## Decision
Every unsafe function carries its own exact `#[target_feature]`
attributes; a `# Safety` section states pointer provenance, bounds,
alignment, CPU-feature requirements, and the caller list; no hidden
caller obligation exists that could be encoded in a safe type.  The
ledger (`unsafe-ledger.toml`) inventories every unsafe function, and a
bidirectional test fails if the ledger and the source inventory disagree.
Disassembly courts prove the expected instructions are present in native
builds (no silent scalarization).

## Tradeoffs
Gained: locally enforced safety; a machine-checked inventory; compiler
output verification.  Given up: the convenience of sharing a caller's
feature context.

## Evidence
`crates/ryg-rans-rs-simd/src/lib.rs`; `unsafe-ledger.toml`; the
`unsafe_ledger` test; the disassembly courts; `docs/unsafe-ledger.md`.

## Future implications
New kernels must register in the ledger and carry their own attributes;
the bidirectional test enforces the discipline mechanically.
