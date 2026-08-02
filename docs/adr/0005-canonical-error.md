# ADR-0005 — Deterministic error selection (lowest block index)

Status: Accepted

## Context
In a parallel engine, several blocks can fail simultaneously.  The
reported error must be deterministic — independent of completion order and
worker count — or the same input can report different errors on different
runs.

## Problem
Which error is canonical when multiple blocks fail?

## Alternatives considered
1. First error observed (schedule-dependent).
2. Lowest block index (schedule-independent).
3. Aggregate of all errors.

## Rejected alternatives
- (1) was rejected: it breaks the determinism invariant.
- (3) was rejected: aggregate error reporting is complex and rarely useful
  at the API boundary; the canonical error identifies the first bad block,
  which is what a caller acts on.

## Decision
`CanonicalErrorTracker` keeps the error with the lowest block index
(breaking ties by error-kind ordinal).  `ParallelError::DecodeFailed` /
`EncodeFailed` / `VerifyFailed` carry the canonical error.  Priority
across *classes* is also canonical and tested: affinity failure >
worker panic > per-block errors > cancellation > completeness.

## Tradeoffs
Gained: deterministic, debuggable failures.  Given up: the ability to
surface every failure in one call (per-block results are still available
in the verification report).

## Evidence
`crates/ryg-rans-rs-parallel/src/error.rs`; the cancellation/panic
priority courts; the executor courts that fail on schedule-dependent
errors.

## Future implications
If an aggregate error API is ever added, the canonical error must remain
the lowest-index one for backwards compatibility.
