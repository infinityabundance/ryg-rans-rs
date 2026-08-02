# Designing a Deterministic Parallel Entropy Coding Pipeline

*An engineering article.  This is the story of the parallel block engine —
what "deterministic" means for entropy coding, why it is hard, and how the
design enforces it by construction.  The full evidence trail is in
`docs/papers/0004-parallel-engine.md` and the ADRs.*

## Abstract

Parallel entropy decoding of a block-streaming container must preserve
three properties that naively parallel designs lose: byte-identical output
order, deterministic error identity, and configuration-bounded peak
memory.  This article derives a pipeline — producer thread, two bounded
channels, live reorder commit — in which all three are enforced by
construction rather than by convention, and reports the failure modes that
motivated each design choice.

## 1. Why block engines

Entropy coding is serial at the symbol level: each decoded symbol depends
on the previous state.  The escape hatch is the *block*: split the input
into independently decodable units, each with its own model, payload, and
integrity hashes.  Blocks are embarrassingly parallel; the pipeline
problem is everything *around* the blocks.

## 2. Determinism is three separate promises

A pipeline is deterministic when all three hold:

1. **Output determinism** — the concatenated result is in block-index
   order regardless of completion order.
2. **Error determinism** — when several blocks fail, the reported error is
   the same on every run (the lowest block index), not the first error
   observed.
3. **Resource determinism** — peak memory is a function of configuration,
   not of schedule or workload.

Each promise needs a mechanism.  Output determinism needs a reorder stage.
Error determinism needs a canonical selection rule.  Resource determinism
needs bounded channels and a live commit stage.

## 3. The pipeline

```
producer ──▶ bounded job channel ──▶ K workers ──▶ bounded result channel ──▶ coordinator
     └────────────────────── coordinator drains while producer submits ────▶ reorder commit
```

The producer thread is the subtle piece.  A naive design submits tasks
inline from the coordinator and collects results in a `Mutex<Vec>`.  Peak
memory is O(N) results regardless of queue capacity — the documented
`max_buffered_output_bytes` budget is a lie under that architecture.  A
design with inline submission and a bounded result channel deadlocks: the
coordinator blocks sending into the full job channel while every worker
blocks sending results into the full result channel, and nobody drains.
The producer thread decouples submission from draining, so neither channel
can deadlock and both stay bounded.

## 4. Reordering without unbounded buffering

The reorder buffer holds completed-but-out-of-order results until their
index becomes the next expected index.  Two bounds apply: a count bound
and a byte bound.  The byte bound is the load-bearing one — a slow block 0
with thousands of fast later blocks must back-pressure the producer rather
than accumulate the whole output.  The atomic-commit API
(`insert → Result<Vec<T>>`) makes correctness un-forgettable: a caller
cannot forget to drain, because insertion returns everything newly
committable.

## 5. Cancellation that cannot truncate

Cancellation is cooperative (workers poll between tasks) and complete (the
report counts declared/submitted/started/completed/cancelled/skipped/
returned).  The completeness invariant is enforced twice: inside the
executor, and again at every public API boundary.  The second enforcement
is not redundancy; it is the documented guarantee living where the
documentation makes it.  A cancelled run that produced fewer results than
declared returns `Cancelled { completed, expected }`; a short run that was
not cancelled returns `IncompleteExecution` — an internal bug surfaced
loudly, never a silent short `Ok`.

## 6. Failure handling and priority

The canonical priority, tested by courts: affinity failure > worker panic
> per-block error (lowest index) > cancellation > completeness.  Worker
panics are caught, converted to typed errors with the block index, and
broadcast to the other workers.  Per-block errors are selected by lowest
index.  Configuration errors (affinity, stack size) are typed errors
before any thread spawns — a worker stack below the platform minimum
would otherwise abort the process on Linux.

## 7. Memory bounds

| Budget | Bound | Enforced against |
|--------|-------|------------------|
| input | `max_buffered_input_bytes` | total input, at the API boundary |
| output | `max_buffered_output_bytes` | the live reorder stage and per-block output |
| in-flight | `max_in_flight_blocks` | both channels (floored at worker count) |

The streaming sink API is the truly bounded path: committed blocks are
handed to a callback, never collected.  The collect API inherently retains
its final `Vec` — an honest, documented exception.

## 8. Alternatives rejected

* Work-stealing schedulers — rejected: they complicate deterministic error
  selection and boundedness proofs.
* Shared scratch behind a mutex — rejected: a lock in the per-block path
  serialises workers.
* Caching whole decode plans — rejected: plans depend on runtime backend
  conditions; only model-derived artifacts are cacheable.
* Reorder-then-sort — rejected: unbounded buffering.

## 9. Lessons learned

1. A documented memory bound is a promise; the architecture must make it
   true by construction.
2. A guarantee documented at the API belongs at the API boundary.
3. Determinism is three separate promises with three separate mechanisms.
4. The producer thread exists because its absence deadlocks — and that
   fact is only discoverable by modelling the schedules (loom courts
   caught the missed-wakeup race).

## References

`docs/papers/0004-parallel-engine.md`; ADRs 0004, 0005, 0007, 0013, 0014,
0015; the loom courts; the stress tests; the courts
`RYG_RANS.L.EXECUTOR.BOUNDED`, `RYG_RANS.L.CANCEL.COMPLETENESS`,
`RYG_RANS.L.REORDER.ATOMIC_COMMIT`.
