# ADR-0007 — Cancellation completeness enforced at the public-API boundary

Status: Accepted

## Context
The `_with_cancel` APIs document: "returns `ParallelError::Cancelled` if
cancelled before all blocks complete; never returns `Ok` with fewer
blocks than declared."  The executor enforces this internally, but the
final return paths of the high-level functions set `cancelled: true` in
the metadata and returned `Ok` regardless — the guarantee was delegated,
not enforced, and a future change to the executor or a collector could
silently return a short `Ok` (the same failure class as the original
verify.rs bug).

## Problem
Where does the completeness guarantee live?

## Alternatives considered
1. Rely on the executor's internal check (`?` propagation).
2. Re-assert the invariant at every public-API boundary with a shared
   helper.
3. Enforce it only in the doc comments.

## Rejected alternatives
- (1) was rejected after the audit: the guarantee is part of each public
  function's contract, and a contract enforced only by an internal
  implementation detail can silently regress.
- (3) was rejected: that is exactly the defect being fixed.

## Decision
`error::check_completeness(cancelled, completed, expected)` is called at
every public boundary before returning `Ok`: `decode_blocks_with_cancel`
(both sequential and parallel paths), `decode_streaming_with_cancel`,
`decode_with_sink` (via `returned_results`), `verify_blocks_with_cancel`
(`blocks_verified` now reports the actual collected count), and
`encode_blocks_with_cancel` (both paths).  Cancellation-with-short →
`Cancelled { completed, expected }`; short-without-cancellation →
`IncompleteExecution` (an internal bug, never a silent short `Ok`).

## Tradeoffs
Gained: the documented promise is enforced by code at the exact point it
is made; a regression in either layer fails loudly.  Given up: nothing
measurable — the checks are unreachable while the executor contract
holds, and their cost is one comparison.

## Evidence
`crates/ryg-rans-rs-parallel/src/error.rs`; the pre-cancelled-token tests
through all four entry points; the `RYG_RANS.L.CANCEL.COMPLETENESS`
court.

## Future implications
Any new public entry point that processes a declared block count must
call `check_completeness` before returning `Ok`; the seal's public-API
reachability court is the backstop.
