# ADR-0015 — Per-worker exclusive scratch (no shared mutable state)

Status: Accepted

## Context
`WorkerScratch` and `ScratchPool` were public and documented as the
solution to per-block allocation churn, but the parallel engine did not
use them — a dead public subsystem.  Each block allocated its own working
memory, and any integration had to avoid introducing a lock into the
per-symbol hot path.

## Problem
How to make scratch real without shared mutable state or hot-path locks.

## Alternatives considered
1. A shared `ScratchPool` with a mutex, workers borrow buffers from it.
2. One exclusive scratch context per worker, created at startup, reset
   between tasks, bounded retention.
3. Leave the subsystem inert and document it as "extension API".

## Rejected alternatives
- (1) was rejected: a mutex in the per-block path would serialize workers
  and defeat the purpose.
- (3) was rejected: an inert public subsystem is prohibited by the
  dead-wiring doctrine (L.7).

## Decision
The `ExecutorTask` trait is `run(self, worker: WorkerIndex, cancel:
&CancellationToken, scratch: &mut WorkerScratch)`.  Each worker owns one
exclusive scratch, created at startup, reset between tasks regardless of
outcome (success, error, or panic), with retained capacity bounded by
configuration and oversized allocations released per policy.  No shared
mutable scratch exists; no lock is in the hot path.

## Tradeoffs
Gained: genuine allocation-churn reduction with provable absence of
contention.  Given up: sharing buffers across workers (which the
boundedness doctrine would not allow anyway).

## Evidence
`crates/ryg-rans-rs-parallel/src/scratch.rs`, `executor.rs`; the
allocation-count instrumentation tests; the `RYG_RANS.L.SCRATCH.INTEGRATION`
court.

## Future implications
If per-worker scratch ever needs more capacity than configured, the
policy (retain vs release) is the knob; the reset-between-tasks invariant
must survive any such change.
