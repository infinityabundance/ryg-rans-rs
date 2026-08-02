# ADR-0013 — Configuration discipline: every public field has an observable effect

Status: Accepted

## Context
The Phase L.6 field-by-field audit found public `ParallelConfig` fields
that were documented but inert: `max_buffered_input_bytes` was never
enforced, `disable_inner_batching` had no distinct executable path, and
`error_policy` was a single-option enum — configuration theatre.

## Problem
What to do with a public configuration field that has no observable
effect?

## Alternatives considered
1. Keep the field and document it as "reserved".
2. Implement the missing behaviour.
3. Remove the field.

## Rejected alternatives
- (1) was rejected: a documented field with no effect is a lie to the
  caller ("no inert public field" is a frozen invariant).
- For `disable_inner_batching` and single-option `error_policy`, (2) was
  rejected as fabrication: inventing a second policy just to justify a
  field is the same defect in new clothing.

## Decision
Every remaining field has a production read site, an observable effect,
and a test that changes only that field and observes the effect
(`max_buffered_input_bytes`, `parallel_threshold_bytes`,
`max_buffered_output_bytes`, `max_in_flight_blocks`, `threads`,
`affinity`, `smt_policy`, `worker_stack_size`, `disable_simd`,
`backend_policy`, `integrity_policy`).  Inert fields were removed.  The
seal gate fails when a public `ParallelConfig` field has no non-test
production read site unless explicitly allowlisted with a reason.

## Tradeoffs
Gained: honest configuration; no dead knobs.  Given up: the API surface
of the removed fields (a deliberate, documented removal).

## Evidence
`crates/ryg-rans-rs-parallel/src/config.rs` and the single-field tests;
the `RYG_RANS.L.CONFIG.WIRING` court; `docs/papers/0004-parallel-engine.md`.

## Future implications
New configuration fields must arrive with their observable effect and
test in the same change, or they will be removed again.
