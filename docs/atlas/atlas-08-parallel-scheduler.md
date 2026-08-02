# Atlas 8 — Parallel Scheduler

**Purpose:** the live bounded executor in detail.

```mermaid
sequenceDiagram
    participant P as Producer thread
    participant J as bounded job channel (queue capacity)
    participant W as K workers (exclusive scratch each)
    participant R as bounded result channel
    participant C as Coordinator
    participant B as ReorderBuffer (output budget)
    P->>J: submit (blocking send = backpressure)
    J->>W: recv
    W->>W: poll cancel; catch_unwind; run(task, scratch); reset scratch
    W->>R: send result (blocking send)
    R->>C: recv result
    C->>B: insert -> commit batch (ascending)
    Note over P,C: producer submits while coordinator drains — no deadlock
```

Memory model: input budget (`max_buffered_input_bytes`) enforced at the
API boundary; output budget (`max_buffered_output_bytes`) enforced against
the live reorder stage; queue capacity = `max_in_flight_blocks` floored at
the worker count.  Cancellation is cooperative and complete; the
completeness invariant is re-asserted at every public boundary
(`error::check_completeness`).

**Related:** paper 0004; ADR-0004, ADR-0005, ADR-0007, ADR-0013, ADR-0014,
ADR-0015; parallel `executor.rs`, `reorder.rs`; courts
`RYG_RANS.L.EXECUTOR.BOUNDED`, `RYG_RANS.L.CANCEL.COMPLETENESS`,
`RYG_RANS.L.REORDER.ATOMIC_COMMIT`.
