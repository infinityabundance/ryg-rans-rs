# Paper 0004 — The deterministic parallel block engine

> *Layer: Algorithm/Subsystem.  Companion: `docs/papers/0005-performance-methodology.md`
> (measurement), `docs/papers/0006-evidence.md` (receipts), the parallel
> crate README (API).  Code: `crates/ryg-rans-rs-parallel/`.  The engine is
> the answer to a specific question: **how do you decode many blocks in
> parallel while keeping output order, error identity, and peak memory
> deterministic and bounded?***

## 1. The problem

Compression pipelines slice input into blocks; each block is independently
encodable/decodable, which is what makes parallelism possible.  But three
properties are easy to lose:

1. **Ordering** — the output must be in block-index order regardless of
   completion order.
2. **Error identity** — when multiple blocks fail, the reported error must
   be deterministic (the lowest block index).
3. **Boundedness** — peak memory must not scale with the workload; a
   slow block 0 must not cause the whole decoded output to accumulate.

The engine treats all three as first-class invariants, enforced by
construction and by courts.

## 2. Block planning: deterministic boundaries

`FixedBlockPlan` divides input into ranges that depend only on the input
length and the configured block size — never on thread count or schedule.
The same input always produces the same block boundaries, so the
parallel decode of a file is bit-identical to a sequential decode of the
same file.  The plan is a pure function; the executor cannot change it.

## 3. The executor: a genuinely bounded live pipeline

The executor (Phase L.4 redesign) is:

```text
producer thread ──bounded job channel──▶ K workers ──bounded result channel──▶ coordinator
     │                                                       ▲
     └── submits tasks live while the coordinator drains ────┘
                                                              │
                                        live ReorderBuffer insert (bounded by
                                        max_buffered_output_bytes) ──▶ ordered commit
```

Key properties:

* **Two bounded channels** — jobs and results — each capped at
  `max(queue capacity, worker count)`.  Neither side can grow without
  bound; a slow consumer back-pressures the producer through the channel
  itself (blocking send), not through a resource-limit error after the
  fact.
* **Live submission and consumption** — the producer submits while the
  coordinator drains; `select!`-style interleaving (realised with a
  dedicated producer thread + blocking channels) means neither side
  deadlocks.
* **Live reorder commit** — results are inserted into the reorder buffer
  as they arrive; every contiguous run commits immediately in block-index
  order.  `max_buffered_output_bytes` bounds the reorder stage, so a slow
  block 0 back-pressures the pipeline instead of allocating the entire
  output.
* **Streaming sink API** — `decode_with_sink` delivers committed blocks to
  a callback and never collects all results; peak memory is the queue
  capacities plus whatever the sink retains.

### The memory model

| Budget | Enforced against | Notes |
|--------|------------------|-------|
| `max_buffered_input_bytes` | total input bytes (queued + executing), checked before submission and at the API boundary | a pure input cap; exceeding it is a typed `ResourceLimit` |
| `max_buffered_output_bytes` | the live reorder stage and per-block output allocation | the streaming API is the true bounded path; the collect API inherently retains the final `Vec` |
| queue capacity | in-flight jobs + results | `max_in_flight_blocks`, floored at the worker count |

The stress tests pin the model: a "10 GiB-equivalent synthetic stream"
never holds 10 GiB resident; a slow block 0 with thousands of fast later
blocks back-pressures rather than exhausting memory; cancellation while
blocked on either budget returns the typed `Cancelled` error.

## 4. Determinism

* Output order: reorder buffer commits ascending block indexes only.
* Error identity: `CanonicalErrorTracker` keeps the lowest-index error;
  `ParallelError::DecodeFailed` / `EncodeFailed` / `VerifyFailed` carry it.
* Backend identity: the plan records intent, the execution records fact;
  Phase L.9 forbids divergence on success.
* Worker counts: `effective_workers` is clamped (min 1, max block count)
  and reported; the same input + config always yields the same report
  fields.

## 5. Cancellation: cooperative and complete

`CancellationToken` is a lock-free `AtomicBool` with `SeqCst` ordering.
Workers poll it before each task; the producer polls before each
submission.  Cancellation is cooperative — an in-flight block finishes —
and **complete**: the `ExecutorReport` tracks declared/submitted/started/
completed/cancelled/skipped/returned, and every public API enforces the
completeness invariant at its own boundary (`error::check_completeness`,
Phase L.3): a cancelled run that produced fewer results than declared
returns `ParallelError::Cancelled { completed, expected }`; a short run
that was not cancelled returns `ParallelError::IncompleteExecution`
(an internal bug, never a silent short `Ok`).

Priority is canonical and tested: **affinity failure > worker panic >
per-block errors (lowest index) > cancellation > completeness**.  The
panic/cancellation race courts (loom) prove no lost tasks and no wedge.

## 6. Scratch: one exclusive arena per worker

Each worker owns one `WorkerScratch`, created at startup, reset between
tasks, retained capacity bounded by configuration, and released when
oversized.  There is no shared mutable scratch and no lock in the
per-symbol hot path.  The allocation-count tests prove the parallel engine
reduces per-block allocation churn relative to per-task allocation.

## 7. Model cache: shared immutable artifacts

Phase O replaced the Phase L.8 process-global cache with the explicitly
owned `ModelArtifactCache` (ADR-0016): `ParallelDecoder` owns an
`Arc<ModelArtifactCache>` created by `ParallelDecoder::new(config)` (fresh)
or injected by `ParallelDecoder::with_model_cache(config, cache)`.  The
core `ModelCache<T>` keeps exact accounting — `current_entries` and
`current_bytes` equal the retained set after every public operation
(per-entry `accounted_bytes`; two-phase inserts plan the eviction set
before mutating; checked arithmetic).  Zero capacity disables; oversized
entries are delivered for the current decode but never retained, and
nothing useful is evicted to find out.

Per-key single-flight (`Building` marker + condvar) guarantees N
concurrent same-key cold requests perform exactly **one** construction;
the build runs outside the cache-state lock (measured flat build time in
worker count, `docs/performance/model-cache.md`).  A builder panic is
caught (`Panicked`, never a permanent `Building` state); a cache-internal
failure bypasses to the same canonical constructor and is never reported
as a model error.  The single constructor
`build_validated_model_artifacts` serves both the cached and uncached
paths, so the two cannot drift.  Eviction is FIFO — kept on measured
evidence (ADR-0017): at the production 64-entry capacity, the shadow
simulation shows FIFO and LRU are identical on every derived public
schedule.

The single-flight guarantee survived a post-v0.5.0 audit only after the
builder-marker ownership rule was made explicit (MODEL_CACHE.RACE.3,
`4389d9b`): **only the builder may remove the in-flight marker**.  A
cancelled waiter that deleted the RUNNING builder's marker let a later
arrival become a second builder — a composition failure (cancellation ×
single-flight) that no single-feature test could catch; the fix added a
deterministic three-party test, a court case, and a loom court.  Cache
metrics use Design-A accounting (MODEL_CACHE.METRICS.2): every lookup
whose initial check finds no artifact is a miss, so
`hits + misses == lookups` holds under cancellation.

The cache stores the model-derived immutable artifacts — frequencies
(Arc-shared) and the 16 KiB packed word table (Arc-shared) — and
**never** the backend choice: backend selection happens after the lookup
so a cached artifact is never reused under incompatible execution
conditions.  A corrupt model is never admitted; a miss rebuilds (always
correct); eviction is deterministic (FIFO).  This is a pure performance
optimisation with no correctness dependence.

## 8. Integrity: strict by default

Every block carries a payload SHA-256 and a decoded-output SHA-256; the
container carries a stream-level SHA-256.  Under `IntegrityPolicy::Strict`
(the default for every verify/CLI/court/evidence path) a zero/unset
decoded hash fails with `DecodedHashMissing` and a mismatch fails with
`DecodedHashMismatch` — the decoded-output hash is what catches model
corruption that payload hashing alone cannot (a block with intact payload
but corrupt model bytes decodes to wrong output while the payload hash
still matches).  `AllowLegacyUnsetDecodedHash` is an explicit opt-in for
legacy streams.

## 9. Failure handling

* Worker panics are caught by `catch_unwind`, converted to
  `ParallelError::WorkerPanic` with the block index, and broadcast to the
  remaining workers via the internal cancellation token.
* Per-block errors are collected by `CanonicalErrorTracker`; the lowest
  index is the canonical error.
* A failed `sched_setaffinity` is a typed `Config` error (never a silent
  ignore); a stack size below the platform minimum is a typed `Config`
  error before any thread spawns (never a process abort).
* Channel-based coordination has no mutex poisoning surface: task execution
  runs inside `catch_unwind` before any coordinator lock is taken, and the
  coordinator locks are held only for tiny, panic-free pushes.

## 10. Thread independence and ordering

Blocks are fully independent (each carries its own model, payload, and
hashes), so workers never communicate except through the two channels.
The only cross-worker coordination is the cancellation token (lock-free)
and the shared model cache (a short critical section per block).  Output
ordering is re-established entirely by the reorder buffer, so completion
order is free to vary — determinism does not depend on the schedule.

## 11. Architecture summary

```text
crates/ryg-rans-rs-parallel/src/
  config.rs        ParallelConfig, policies (typed; every field has a
                   production read site and an observable test)
  executor.rs      bounded live executor, ExecutorTask trait, reports
  cancellation.rs  CancellationToken (lock-free, SeqCst)
  job.rs           job/result types, ExecutionMetadata
  plan.rs          FixedBlockPlan (thread-count-independent boundaries)
  reorder.rs       ReorderBuffer: insert → Result<Vec<T>> atomic commit
  scratch.rs       WorkerScratch/ScratchPool (per-worker exclusive)
  cache.rs         ModelCache + ValidatedModelArtifacts (Arc-shared)
  decode_plan.rs   exact-backend planning + format compatibility matrix
  decode.rs        ParallelDecoder (+ _with_cancel, streaming, sink)
  encode.rs        ParallelEncoder (+ _with_cancel)
  verify.rs        ParallelVerifier (+ _with_cancel)
  error.rs         ParallelError, BlockError, check_completeness
  resource.rs      effective worker counts, estimates
  affinity.rs      Linux sched_setaffinity policies (typed)
  sync.rs          channel/Arc/Mutex abstraction for loom instrumentation
```
