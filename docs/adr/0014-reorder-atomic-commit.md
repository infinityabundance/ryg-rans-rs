# ADR-0014 — ReorderBuffer atomic commit batches

Status: Accepted

## Context
The original `ReorderBuffer::insert` returned `Option<T>` and required
every caller to remember to call `drain_ready()` after receiving the
next expected item.  That unenforced protocol was fragile: a caller that
forgot the drain silently lost contiguous blocks.

## Problem
How to make ordered commit safe by construction.

## Alternatives considered
1. Keep `insert → Option<T>` + mandatory `drain_ready()`.
2. `insert(item) → Result<Vec<T>>` returning everything newly committable.
3. A `CommitBatch<T>` wrapper type.

## Rejected alternatives
- (1) was rejected (L5-A): a fragile unenforced protocol.
- (3) was rejected as ceremony over the plain `Vec<T>`; the return value
  is a batch of committed blocks, and `Vec<T>` says that plainly.

## Decision
`insert(item) -> Result<Vec<T>, BlockError>` returns the newly inserted
next-expected item plus every contiguous pending item it unblocks, in
strictly ascending block-index order.  No separate drain call is required
after insertion.  A final inspection API (`drain_ready`) is retained only
for diagnostics at completion.

## Tradeoffs
Gained: atomic commit semantics enforced by the type system; no caller
can forget the drain.  Given up: the incremental `Option<T>` interface.

## Evidence
`crates/ryg-rans-rs-parallel/src/reorder.rs`; the N≤9 exhaustive
permutation test (all inputs inserted exactly once → concatenated commit
batches equal `[0..N]`); duplicate/stale/missing-gap/overflow/error-
recovery tests; the `RYG_RANS.L.REORDER.ATOMIC_COMMIT` court.

## Future implications
The permutation property is the load-bearing test; extending it to
larger N or randomized schedules must keep the same invariant.
