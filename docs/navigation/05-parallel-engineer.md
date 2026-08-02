# 05 — Parallel Engineer

**Purpose:** understand and extend the deterministic parallel engine:
executor, reorder, cancellation, scratch, cache, config.

**Prerequisites:** `01-first-week.md`.

**Required papers:** 0004, 0006 §4.

**Required ADRs:** 0004, 0005, 0007, 0009, 0013, 0014, 0015.

**Required source modules:** every module in `crates/ryg-rans-rs-parallel/`:
`executor.rs`, `reorder.rs`, `cancellation.rs`, `job.rs`, `plan.rs`,
`config.rs`, `error.rs`, `scratch.rs`, `cache.rs`, `decode_plan.rs`,
`decode.rs`, `encode.rs`, `verify.rs`, `sync.rs`.

**Recommended reading order:**
1. `docs/papers/0004-parallel-engine.md` — the architecture.
2. The module docs in the order listed above (each has the M.6 section
   set).
3. The ADRs above — the decisions behind the shape.
4. The loom courts and the stress tests.
5. `docs/navigation/reading-paths.md` — the parallel path with hours.

**Expected understanding:** the live bounded pipeline (producer, two
channels, coordinator, reorder commit); the completeness invariant and
where it is enforced; the memory model (input budget, output budget,
queue capacity); deterministic error selection; how scratch and the model
cache plug in.

**Estimated reading time:** 12–20 hours.

**Exercises:**
1. Explain why the producer thread exists (what deadlocks without it).
2. Trace `check_completeness` at every API boundary.
3. Explain what `max_buffered_output_bytes` bounds and what it cannot
   bound (the collect API's final Vec).

**Common misconceptions:**
- "The reorder bound is the queue capacity." It is
  `effective_queue + workers` (L.17-B fixed exactly this).
- "Cancellation kills in-flight work." It is cooperative.
- "The cache can be disabled freely." It is a pure optimisation — correct
  either way — but disabling it costs throughput.

**Related evidence:** `RYG_RANS.L.CANCEL.COMPLETENESS`,
`RYG_RANS.L.EXECUTOR.BOUNDED`, `RYG_RANS.L.REORDER.ATOMIC_COMMIT`,
`RYG_RANS.L.CONFIG.WIRING`, `RYG_RANS.L.SCRATCH.INTEGRATION`,
`RYG_RANS.L.MODEL_CACHE.INTEGRATION`; the parallel performance receipt.

**Future reading:** `03-performance-engineer.md`, `02-maintainer-path.md`.
