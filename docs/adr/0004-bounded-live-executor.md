# ADR-0004 — The bounded live executor (producer + coordinator pipeline)

Status: Accepted

## Context
The original parallel executor materialised all tasks, accumulated all
results in a `Mutex<Vec>`, joined every worker, and reordered only
afterwards.  `max_buffered_output_bytes` could not bound peak decoded
memory: a slow block 0 meant every later result accumulated in memory
before reordering could commit anything.

## Problem
How to bound peak memory end-to-end while keeping determinism.

## Alternatives considered
1. Keep the materialise-then-reorder architecture and accept the memory
   bound violation.
2. A live pipeline: producer thread + bounded job channel + bounded
   result channel + coordinator that drains results and commits through a
   live reorder stage.
3. A fully lock-free work-stealing scheduler.

## Rejected alternatives
- (1) was rejected: the documented `max_buffered_output_bytes` budget was
  a lie under that architecture (residual L4-B).
- (3) was rejected: work-stealing complicates deterministic error
  selection and boundedness guarantees; the channel pipeline provides
  both with far less machinery.

## Decision
The executor is a live pipeline (Phase L.4): a producer thread submits
tasks into a bounded job channel while K workers drain it and push
results into a bounded result channel; the coordinator drains results and
inserts them into a live `ReorderBuffer` whose byte budget is
`max_buffered_output_bytes`.  A slow consumer back-pressures the producer
through the bounded channels (blocking send), never through a
post-allocation resource-limit error.  Streaming sink APIs
(`decode_with_sink`) never collect all results.

## Tradeoffs
Gained: genuine end-to-end boundedness; live ordered commit; backpressure
instead of OOM.  Given up: the simplicity of a materialise-and-sort
implementation, and the ability to return unordered results cheaply.

## Evidence
`docs/papers/0004-parallel-engine.md`; the stress tests (10 GiB-equivalent
synthetic stream with bounded RSS, slow block 0, budget-limit
cancellation); the loom courts; the sealed performance run.

## Future implications
A true streaming input path (feeding blocks one at a time from a
non-seekable source) would build on this architecture; the current
`decode_streaming_with_cancel` still materialises jobs (documented
limitation).
