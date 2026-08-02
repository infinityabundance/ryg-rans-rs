# ADR-0008 — Exact backend semantics: no silent fallback

Status: Accepted

## Context
An explicitly requested backend (SSE4.1, AVX2, AVX-512, Uniform256, batch)
must either execute exactly or fail with a typed error.  Historically, an
unavailable explicit backend could be silently rewritten to scalar during
planning — violating the caller's explicit request.

## Problem
What happens when a caller requests a backend that cannot execute on this
build or CPU?

## Alternatives considered
1. Silently substitute a scalar fallback.
2. Return a typed error (`BackendUnavailable`, `BackendFormatMismatch`,
   `BackendRequiresBatchContext`).
3. Panic.

## Rejected alternatives
- (1) was rejected: silent substitution breaks the caller's contract
  (throughput expectations, thermal/power behaviour, or a mandatory
  hardware path).  "No silent fallback" is a frozen invariant.
- (3) was rejected: a config/format problem is not a panic.

## Decision
The planner (`create_decode_plan`) validates format compatibility
(8-way ↔ codec 7, 16-way ↔ codec 8, Uniform256 ↔ validated model,
RAW/RLE ↔ block kind, batch ↔ coordinator context) and produces either the
exact requested plan or a typed error.  Execution capability (runtime CPU
features, compiled target features) is checked at execution time by the
kernel's own safe wrapper, which returns `BackendUnavailable` when absent.
Every execution result carries requested backend, selected plan, actual
backend, words consumed, and final states.

## Tradeoffs
Gained: exact, observable backend semantics; no hidden behaviour.  Given
up: the convenience of "just decode it with anything".

## Evidence
`crates/ryg-rans-rs-parallel/src/decode_plan.rs`; the
`RYG_RANS.L.BACKEND.EXPLICIT` court; the format-compatibility matrix
tests; the preflight backend-identity checks in the benchmark pipeline.

## Future implications
New backends must be added to the compatibility matrix, the plan enum,
the SIMD crate's dispatch, and the backend-identity mapping together —
the courts require the set to stay in sync.
