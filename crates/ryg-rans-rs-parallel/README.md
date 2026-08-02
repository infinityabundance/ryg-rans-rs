# ryg-rans-rs-parallel — Deterministic Parallel Block Engine

**Version 0.3.0** (workspace) · **Phase L** · **105 tests passing** · Evidence status: **test-verified** (not sealed)

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-parallel)](https://crates.io/crates/ryg-rans-rs-parallel)

A bounded, cancellable, deterministic parallel block engine for rANS entropy
coding, built on `ryg-rans-rs-core` (portable scalar codecs) and, optionally,
`ryg-rans-rs-simd` (SIMD-accelerated decode kernels).  The crate requires
`std`.

---

## Table of Contents

1. [What This Crate Does](#what-this-crate-does)
2. [What This Crate Does NOT Do](#what-this-crate-does-not-do)
3. [Architecture](#architecture)
4. [Determinism Invariants](#determinism-invariants)
5. [Configuration](#configuration)
6. [The Bounded Executor](#the-bounded-executor)
7. [Cancellation](#cancellation)
8. [Reorder Buffering](#reorder-buffering)
9. [WorkerScratch](#workerscratch)
10. [ModelCache](#modelcache)
11. [Backend Semantics](#backend-semantics)
12. [Integrity Verification](#integrity-verification)
13. [Resource Behavior](#resource-behavior)
14. [Trust Boundaries and Input Invariants](#trust-boundaries-and-input-invariants)
15. [SIMD Requirements](#simd-requirements)
16. [Unsafe Boundaries](#unsafe-boundaries)
17. [Evidence Model](#evidence-model)
18. [Performance Methodology](#performance-methodology)
19. [Limitations](#limitations)
20. [Examples](#examples)
21. [Troubleshooting](#troubleshooting)
22. [Versioning](#versioning)
23. [Reading Order](#reading-order)

---

## What This Crate Does

`ryg-rans-rs-parallel` adds **block-level parallelism** to rANS entropy coding.
The defining invariant:

> Thread count, worker scheduling order, completion order, CPU topology, and
> the executed backend may change **performance**, but must never change
> encoded bytes, decoded bytes, block boundaries, models, hashes, stream
> hashes, canonical errors, or forensic results.

The engine provides:

- **Encode** — `ParallelEncoder::encode_blocks` / `encode_blocks_with_cancel` /
  `encode_planned`, producing `OrderedEncodedBlocks` in ascending block-index
  order.
- **Decode** — `ParallelDecoder::decode_blocks` / `decode_blocks_with_cancel` /
  `decode_streaming` / `decode_streaming_with_cancel` / `decode_with_sink`,
  producing `OrderedDecodedBlocks` (with a canonical `stream_hash`) or feeding
  a caller sink.
- **Verify** — `ParallelVerifier::verify_blocks` / `verify_blocks_with_cancel`,
  producing a `ParallelVerificationReport` without writing decoded output.
- **A bounded live executor** — producer thread + bounded job channel +
  bounded result channel + coordinator drain (`run_tasks`,
  `run_tasks_with_affinity`, `run_tasks_with_sink`, `run_tasks_sequential`).
- **Cooperative external cancellation** via `CancellationToken`.
- **Exact backend semantics** — an explicit backend request executes exactly,
  or returns a typed error; silent scalar substitution is prohibited.
- **Strict integrity by default** — zero/unset decoded hashes fail
  (`DecodedHashMissing`), mismatches fail (`DecodedHashMismatch`).

### Why rANS state streams are not split across threads

Each rANS decoder state depends on every previous symbol in its stream.
Splitting one state stream across threads would require either breaking the
state dependency chain (impossible — the state carries the accumulated entropy
of all prior symbols) or a new container format with explicit checkpoints.
Instead, the engine exploits **block-level parallelism**: the input is
partitioned into independently decodable blocks, each with its own frequency
model and compressed payload — the same strategy used by Brotli, Zstandard,
and LZMA2.

---

## What This Crate Does NOT Do

- **Does not implement rANS arithmetic.**  The codec algorithms live in
  `ryg-rans-rs-core`; SIMD kernels live in `ryg-rans-rs-simd`.  This crate
  coordinates threads, queues, cancellation, reordering, caching, and
  deterministic error selection.
- **Does not do per-symbol thread parallelism.**  Parallelism is block-level
  only; a single block is never split across workers.
- **Does not silently substitute backends.**  There is no scalar fallback for
  an explicit SIMD request.  Either the requested backend executes exactly or
  the call returns a typed error (see [Backend Semantics](#backend-semantics)).
- **Does not expose `disable_inner_batching` or an `error_policy` field.**
  Both were removed in Phase L.6 as configuration theater; canonical error
  selection is fixed to the lowest failing block index.
- **Does not implement the RYGRANS v1 CLI container.**  The block-record
  serialization here (104-byte `"RYGR"` header + fixed 1024-byte raw-frequency
  model + payload, see `block.rs`) is the parallel engine's own block format,
  shared field-layout with the container spec.  The CLI crate
  (`ryg-rans-rs-cli`) implements the full container with its own streaming
  pipeline.
- **Does not do dynamic thread scaling.**  The worker pool is fixed per run
  (`effective_workers`), clamped to `[1, min(requested, block_count)]`.
- **Does not use FFI.**  All codec work is native Rust.
- **Does not provide a CLI, benchmarking suite, or evidence tooling.**
  Those live in `ryg-rans-rs-cli`, `ryg-rans-rs-bench`, and
  `ryg-rans-rs-oracle` respectively.

---

## Architecture

```text
Input:  [ block 0 ][ block 1 ][ block 2 ]...[ block N ]
           │           │           │
           ▼           ▼           ▼
        Worker 0 ──► Worker 1 ──► Worker 2 ──► ...  (concurrent)
           │           │           │
           ▼           ▼           ▼
        ReorderBuffer (ordered commit by block_index)
           │           │           │
           ▼           ▼           ▼
Output: [ block 0 ][ block 1 ][ block 2 ]...[ block N ]
```

1. **`FixedBlockPlan`** partitions input into deterministic ranges depending
   only on input length and block size — thread-count-independent.
2. Each block independently builds its frequency model and compressed payload.
3. Workers encode/decode/verify blocks concurrently — **no shared mutable
   state between blocks** (worker-exclusive scratch, immutable model cache).
4. A `ReorderBuffer` commits results in strictly ascending block-index order
   (see [Reorder Buffering](#reorder-buffering)).

### Module layout

| Module | Responsibility |
|--------|---------------|
| `lib.rs` | Module structure, invariants, re-exports |
| `config.rs` | `ParallelConfig`, `ThreadCount`, `AffinityPolicy`, `BackendPolicy`, `BackendId`, `SmtPolicy`, `IntegrityPolicy`, `HashVerification`, `ModelPolicy`, `CodecPolicy` |
| `error.rs` | `ParallelError`, `BlockError`, `BlockErrorKind`, `CanonicalErrorTracker` |
| `executor.rs` | Bounded worker pool: `run_tasks`, `run_tasks_with_affinity`, `run_tasks_with_sink`, `run_tasks_sequential`, `ExecutorTask` trait, `ExecutorReport` |
| `cancellation.rs` | `CancellationToken` — cooperative, thread-safe, lock-free |
| `job.rs` | `EncodeBlockJob`, `DecodeBlockJob`, `VerifyBlockJob`, `EncodedBlockResult`, `DecodedBlockResult`, `VerifiedBlockResult`, `OrderedEncodedBlocks`, `OrderedDecodedBlocks`, `ExecutionMetadata` |
| `plan.rs` | `FixedBlockPlan`, `BlockRange` — thread-count-independent block boundaries |
| `reorder.rs` | `ReorderBuffer<T>`, `HasBlockIndex`, `BufferSized` — bounded ordered commit |
| `encode.rs` | `ParallelEncoder::encode_blocks`, `encode_blocks_with_cancel`, `encode_planned`, `encode_single_block`, `sha256`, `build_model_bytes` |
| `decode.rs` | `ParallelDecoder::decode_blocks`, `decode_blocks_with_cancel`, `decode_streaming`, `decode_streaming_with_cancel`, `decode_with_sink`, `decode_single_block`, `ExecutedDecode` |
| `decode_plan.rs` | `DecodePlan`, `create_decode_plan`, `plan_cache_key` — exact-backend planning |
| `verify.rs` | `ParallelVerifier::verify_blocks`, `verify_blocks_with_cancel`, `ParallelVerificationReport`, `BlockVerificationResult` |
| `cache.rs` | `ModelCache<T>`, `ModelCacheKey`, `ValidatedModelArtifacts`, `cached_model_artifacts` |
| `resource.rs` | `ParallelMemoryEstimate`, `estimate_memory`, `effective_worker_count` |
| `scratch.rs` | `WorkerScratch`, `ScratchPool` — reusable per-worker buffers |
| `affinity.rs` | `validate_affinity_policy`, `apply_worker_affinity` — Linux CPU pinning (`affinity` feature) |
| `block.rs` | Block format constants, `BlockHeaderInfo`, `parse_block_header`, `build_header` |

---

## Determinism Invariants

| Invariant | Mechanism |
|-----------|-----------|
| **Thread-count-independent output** | `FixedBlockPlan` depends only on `(input_length, block_size)`; reorder buffer commits by ascending `block_index` |
| **Canonical error** | `CanonicalErrorTracker` keeps the lowest-index failing block; same-block ties break by `BlockErrorKind` ordinal (declaration order) |
| **Canonical stream hash** | `OrderedDecodedBlocks::stream_hash` is SHA-256 over concatenated decoded output in ascending block order — independent of scheduling |
| **Worker-count-independent results** | Results are committed through `ReorderBuffer`, never in completion order |
| **Deterministic cache behavior** | `ModelCache` uses FIFO eviction (no access-order dependence) |
| **Deterministic planning** | Block boundaries are never derived from worker count |

Two runs with different `threads` values on the same input produce identical
output bytes, hashes, and canonical errors — only wall-clock time differs.

---

## Configuration

`ParallelConfig` is the single configuration type.  All fields are validated
before any worker thread spawns; invalid combinations produce
`ParallelError::Config` before any work starts (fail-fast, no partial work).

| Field | Type | Default | Meaning |
|-------|------|---------|---------|
| `threads` | `ThreadCount` | `AvailableParallelism` | `Exact(n)` or OS-detected parallelism; clamped to `[1, block_count]` |
| `max_in_flight_blocks` | `NonZeroUsize` | 16 | Job-queue capacity — the in-flight bound |
| `max_buffered_input_bytes` | `u64` | 256 MiB | Input budget: total compressed input queued plus executing, enforced during submission |
| `max_buffered_output_bytes` | `u64` | 512 MiB | Output budget: completed-but-unordered output, enforced against the live reorder stage (also bounds any single block's output allocation) |
| `parallel_threshold_bytes` | `u64` | 1 MiB | Below this total input size, execution falls back to sequential on the calling thread (`run_tasks_sequential`; metadata records `effective_workers = 1`) |
| `affinity` | `AffinityPolicy` | `None` | `None` / `Compact` / `Spread` / `Explicit(Vec<usize>)`; Linux `sched_setaffinity` + `sched_getaffinity` verification; invalid explicit lists → typed `Config` error |
| `backend_policy` | `BackendPolicy` | `Portable` | `Portable` / `ScalarPreferred` / `Auto` / `Explicit(BackendId)` / `ModelAware` |
| `worker_stack_size` | `Option<usize>` | `None` | Passed to `std::thread::Builder::stack_size` |
| `disable_simd` | `bool` | `false` | Forces scalar plans; explicit SIMD + `disable_simd` → typed config conflict |
| `smt_policy` | `SmtPolicy` | `UseAllLogical` | `UseAllLogical` / `PreferPhysicalEquivalent` / `Explicit` — topology-aware worker count adjustment |
| `integrity_policy` | `IntegrityPolicy` | `Strict` | `Strict` (default) or `AllowLegacyUnsetDecodedHash` (explicit opt-in) |

### `ThreadCount`

`Exact(NonZeroUsize)` uses exactly N workers; `AvailableParallelism` reads
`std::thread::available_parallelism()` (falling back to 1 on failure).
The effective count is clamped at executor initialisation to
`[1, min(requested, total_blocks)]` and is readable from
`ExecutionMetadata::effective_workers` / `ExecutorReport::effective_workers`.

### `AffinityPolicy`

- `None` — no pinning; the kernel scheduler decides.
- `Compact` — worker `i` pinned to CPU `i % online_cpus`.
- `Spread` — worker `i` pinned to CPU `(i * stride) % online_cpus`.
- `Explicit(cpus)` — worker `i` pinned to `cpus[i % cpus.len()]`; an empty
  list is a typed `Config` error.

Affinity never affects canonical output — only scheduling.  Policies other
than `None` require the `affinity` cargo feature on Linux; without it they
return a typed `Config` error (never silently ignored).

### `BackendPolicy` decision tree

| Policy | Behavior |
|--------|----------|
| `Portable` | Always `Scalar8` (codec 7) or `Scalar16` (codec 8).  No runtime SIMD detection. |
| `ScalarPreferred` | Currently identical to `Portable`; reserved for future dispatch heuristics. |
| `Auto` | Conservative: currently falls through to `Portable` behavior until multi-machine benchmarks establish crossover points. |
| `ModelAware` | As `Auto`, plus: a validated Uniform256 model (every frequency == 16 at scale_bits == 12) selects the real table-free kernel `Uniform256TableFree16`.  Different blocks in one run may use different backends. |
| `Explicit(BackendId)` | The specified backend is planned for every block; no feature detection, no fallback.  Incompatible formats / unavailable CPUs / missing build features → typed errors (see [Backend Semantics](#backend-semantics)). |

---

## The Bounded Executor

The executor (`executor.rs`) is a **live producer/consumer pipeline** with two
bounded channels:

```text
              ┌─────────────┐   bounded job    ┌─────────┐   bounded result   ┌────────────┐
  producer ──▶│  job_tx     │ ───────────────▶ │ workers │ ────────────────▶ │ coordinator│
  thread     │ (bounded)    │   queue        │ (N threads)│   channel        │ (drains)   │
              └─────────────┘                 └─────────┘                   └────────────┘
```

- A **producer thread** submits tasks into the bounded job channel; when the
  channel is full it blocks (backpressure), so at most `effective_queue`
  tasks are ever queued or executing.
- **Worker threads** dequeue tasks, execute them inside
  `catch_unwind(AssertUnwindSafe(..))`, and send results into the bounded
  result channel.  When the result channel is full, workers block — applying
  backpressure to the whole pipeline.
- The **coordinator** (the calling thread) drains the bounded result channel
  continuously while the producer submits.  Because the producer and the
  coordinator run concurrently, neither channel can deadlock.
- The producer thread is what makes this bounded: an earlier design that
  submitted inline and collected into a `Mutex<Vec>` accumulated every result
  before reordering; a design with inline submission plus a bounded result
  channel deadlocks (everyone blocks sending, nobody drains).  See Phase L.4
  in the gap ledger.

Public entry points:

| Function | Notes |
|----------|-------|
| `run_tasks(tasks, worker_count, max_queue, stack_size, external_cancel)` | Batch execution; delegates to `run_tasks_with_affinity` with `AffinityPolicy::None` |
| `run_tasks_with_affinity(..., affinity)` | Batch execution with worker pinning |
| `run_tasks_with_sink(..., sink)` | Identical pipeline, but the coordinator invokes `sink` per completed result in **completion order**; peak result memory is bounded by the result channel, not the task count |
| `run_tasks_sequential(tasks, external_cancel)` | No worker pool; tasks run in declaration order on the calling thread (sequential-threshold fallback) |

### `ExecutorReport`

`run_tasks*` returns `ExecutorReport<R>` with the full completeness picture:

| Field | Meaning |
|-------|---------|
| `results` | Completed results (completion order, not block-index order; empty for `run_tasks_with_sink`) |
| `worker_panics` | Number of panics caught by `catch_unwind` |
| `effective_workers` | Workers actually created (clamped) |
| `declared_tasks` / `submitted_tasks` | Tasks declared by the caller vs. actually submitted (submission stops on cancellation) |
| `started_tasks` / `completed_tasks` | Tasks that began / produced a result |
| `cancelled_tasks` | Tasks skipped because cancellation was observed before execution |
| `returned_results` | Results returned in `results` |
| `cancelled` | Whether the run was cancelled (external token or panic-triggered internal token) |

### Worker lifecycle and panic containment

- Workers are named `ryg-parallel-{i}` (producer: `ryg-parallel-producer`),
  optionally with a custom stack size.
- Every worker thread creates one exclusive `WorkerScratch` and runs tasks via
  `ExecutorTask::run(worker_index, &cancel, &mut scratch)`; the scratch is
  reset between tasks regardless of outcome.
- Panics are caught and converted to `ParallelError::WorkerPanic {
  block_index, worker_index }`, attributed via `ExecutorTask::block_index()`.
  The canonical panic is the lowest-index one (`None` treated as `u64::MAX`).
- All workers are joined in every exit path — no detached threads, no
  busy-spin or polling loops.

---

## Cancellation

`CancellationToken` (`cancellation.rs`) is a lock-free `AtomicBool` with
`SeqCst` ordering: `cancel()` from any thread, `is_cancelled()` for cheap
checks, `check() -> Result<(), ParallelError>` for `?`-style yield points.
Cancellation is **cooperative** — workers poll at defined yield points
(before each task) and the coordinator polls before each submission; in-flight
work may finish.

External cancellation is exposed through the `_with_cancel` APIs:

- `encode_blocks_with_cancel(blocks, config, external_cancel)`
- `decode_blocks_with_cancel(blocks, config, external_cancel)`
- `decode_streaming_with_cancel(blocks, config, external_cancel)`
- `decode_with_sink(blocks, config, external_cancel, sink)`
- `verify_blocks_with_cancel(blocks, config, external_cancel)`

The token argument is `Option<Arc<CancellationToken>>`; `None` creates an
internal token that is only cancelled on worker panic (panic-triggered
broadcast cancellation).

**Completeness invariant:** cancellation or incomplete execution returns
`ParallelError::Cancelled { completed, expected }` (or
`IncompleteExecution { completed, expected }` when the run was not cancelled
but results are short — an internal-bug signal).  The engine **never returns
`Ok` with fewer blocks than declared**.  The executor checks this invariant
after all workers join, before returning.

---

## Reorder Buffering

`ReorderBuffer<T>` (`reorder.rs`) serialises out-of-order completions into
ascending block-index order.  It enforces two independent bounds:

1. **Count limit** (`max_blocks`) — maximum out-of-order results stored.
2. **Byte limit** (`max_bytes`, from `BufferSized`) — maximum total buffered
   bytes.

The commit protocol is **atomic**: `insert(item) -> Result<Vec<T>, BlockError>`
returns every newly committable item (the inserted item plus every contiguous
pending block it unblocks) in strictly ascending order.  There is **no
separate drain call required after insertion**; `drain_ready()` is retained as
a final diagnostics/inspection API.  Error conditions:

- index `< next_expected` (already committed) or duplicate in-flight →
  `BlockErrorKind::OutputCommit`
- count or byte limit reached → `BlockErrorKind::ResourceLimit`

Phase L.5 replaced the earlier `Option<T>` + `drain_ready()` protocol, which
let callers forget to drain; the current protocol returns the committed chain
directly.

---

## WorkerScratch

`WorkerScratch` (`scratch.rs`) holds per-worker reusable buffers
(`input_buffer`, `output_buffer`, `model_buffer`).  Each worker owns one
exclusive scratch created at thread start and passed through
`ExecutorTask::run(worker, cancel, scratch)` — there is **no shared mutable
scratch** and no lock in the per-symbol hot path.  `reset()` clears the
buffers between tasks **without freeing capacity**, then shrinks any buffer
whose capacity exceeds `max_retain` — one adversarial oversized block cannot
permanently inflate every worker's footprint (bounded retained capacity,
Phase L.7).

`ScratchPool` (indexed per-worker buffers) remains a public utility type; the
production executor creates the worker-exclusive scratch inline rather than
via the pool.

---

## ModelCache

`ModelCache<T>` + `cached_model_artifacts` (`cache.rs`) cache validated,
immutable model-derived decode artifacts.  The cache key is
`ModelCacheKey { model_sha256, scale_bits, codec_id }` (SHA-256 of the exact
model bytes plus the two discriminators).  Properties:

- **Bounded**: 64 entries / 16 MiB for the process-global cache
  (`GLOBAL_MODEL_CACHE`, a `OnceLock<Mutex<ModelCache<ValidatedModelArtifacts>>>`).
- **FIFO eviction** — deterministic, independent of access order.
- **Correctness-independent**: a miss rebuilds the artifacts (always correct);
  the cache is a pure performance optimisation.
- **Corrupt models are never cached**: only artifacts validated
  (frequencies summing to `1 << scale_bits`, 256 entries) are inserted.
- **Backend selection happens after the lookup**, per block, so a cached
  artifact is never reused under incompatible execution conditions
  (runtime CPU features, `disable_simd`).

---

## Backend Semantics

### `BackendId` and labels

`BackendId` identifies a decode kernel or passthrough.  Its `label()` strings
are kebab-case and stable; they appear in execution telemetry
(`backend_counts`) and diagnostics:

| `BackendId` | `label()` | Width / Strategy | ISA |
|-------------|-----------|------------------|-----|
| `RawCopy` | `raw-copy` | memcpy passthrough | — |
| `RleFill` | `rle-fill` | single-symbol fill | — |
| `Scalar8` | `scalar-8way` | 8-way word rANS (codec 7 baseline) | portable |
| `Scalar16` | `scalar-16way` | 16-way word rANS (codec 8 baseline) | portable |
| `Uniform256TableFree16` | `uniform256-tablefree-16way` | table-free, `slot / 16` arithmetic | portable |
| `Sse41Interleaved8` | `sse41-interleaved-8way` | 8-way interleaved | SSE4.1 |
| `Avx512VlInterleaved8` | `avx512vl-interleaved-8way` | 8-way on 256-bit | AVX-512 VL |
| `Avx512Interleaved16` | `avx512-interleaved-16way` | 16-way on 512-bit | AVX-512 |
| `Avx512VlManualGather8` | `avx512vl-manual-gather-8way` | manual gather 8-way | AVX-512 VL |
| `Avx512ManualGather16` | `avx512-manual-gather-16way` | manual gather 16-way | AVX-512 |
| `Avx512Vl2x8` | `avx512vl-2x8` | 2×8-on-16 | AVX-512 VL |
| `Avx512Batch4` | `avx512-batch4` | 4-stream batch | AVX-512 |
| `Avx2ManualGather8` | `avx2-manual-gather-8way` | VPERMD manual gather | AVX2 |
| `Avx2HardwareGather8` | `avx2-hardware-gather-8way` | hardware gather | AVX2 |
| `Avx2TwoBy8On16` | `avx2-2x8-on16` | 2×8-on-16 | AVX2 |
| `Avx2Uniform256TableFree16` | `avx2-uniform256-tablefree-16way` | uniform256 table-free | AVX2 |
| `Avx2Batch4On16` | `avx2-batch4-on16` | 4-at-once batch on 16-way | AVX2 |

### Exact-backend contract (Phase L.9)

- An **explicit** request (`BackendPolicy::Explicit(backend)`) is never
  rewritten during planning.  Every explicit request either produces a real
  plan for the requested backend or a typed error:
  - `BackendFormatMismatch` — backend incompatible with the block's format.
  - `BackendUnavailable` — the kernel cannot execute here: missing runtime
    CPU features, missing compiled target features, or an explicit SIMD
    request combined with `disable_simd = true`.
  - `BackendRequiresBatchContext` — `Batch4` backends need coordinator-level
    grouping of four compatible jobs and are **not reachable through the
    one-block API**; the plan is rejected at planning time.
- **Format compatibility is validated before execution:**

  ```text
  8-way backend   ↔ codec 7   (canonical 8-way stream)
  16-way backend  ↔ codec 8   (canonical 16-way stream)
  Uniform256      ↔ validated Uniform256 model (all freqs == 16, scale 12)
  Batch backend   ↔ coordinator batch context (not via the one-block API)
  RAW backend     ↔ RAW block kind
  RLE backend     ↔ RLE block kind
  ```

- **Requested and executed are recorded separately.**  `DecodedBlockResult`
  carries both `plan_backend` (what the plan selected) and `backend` (what
  actually ran); on success they are equal — the Phase L.9 contract forbids
  silent substitution.  `decode_single_block` records the executed backend
  from `ExecutedDecode`, the ground-truth execution outcome (output, backend,
  `words_consumed`, `final_states`).
- Non-explicit policies (`Portable`, `ScalarPreferred`, `Auto`, `ModelAware`)
  never produce SIMD plans, so they have no fallback path either — the plan
  always executes as planned.

---

## Integrity Verification

**Strict integrity is the default** (`IntegrityPolicy::Strict`):

- Every block's **payload SHA-256** is verified against the stored hash —
  always, even for empty payloads.  Mismatch → `BlockErrorKind::PayloadHash`.
- The **decoded-output SHA-256** is recomputed and compared:
  - zero/unset stored hash → `DecodedHashMissing` (fails under `Strict`);
  - nonzero stored hash mismatch → `DecodedHashMismatch`;
  - only a matching nonzero hash passes.
- `AllowLegacyUnsetDecodedHash` (compatibility integrity, explicit opt-in):
  zero/unset hashes are reported as `HashVerification::Unset` and do not fail
  for that reason alone; any nonzero mismatch still fails.

`HashVerification` is the four-state outcome enum: `Match`, `Mismatch`,
`Unset`, `NotComputed`.

The verify pipeline (`verify.rs`) is a superset of decode: it parses the
header, verifies the payload hash, fully decodes, and compares the decoded
hash — without writing output — and aggregates `ParallelVerificationReport`
counters (`blocks_verified`, `payload_hash_ok`, `decoded_hash_ok`,
`decoded_hash_mismatch`, `decoded_hash_unset`, `decoded_hash_not_computed`,
`output_matches`, `blocks_failed`, per-block `BlockVerificationResult`s).
Any failure returns `ParallelError::VerifyFailed` with the canonical
(lowest-index) `BlockError`.

---

## Resource Behavior

The engine is bounded end-to-end at every stage:

| Stage | Bound | Enforced by |
|-------|-------|-------------|
| **Queued input budget** | `max_buffered_input_bytes` | Checked at submission, before the sequential threshold, on every path (encode, decode, verify) — exceeding it returns `ParallelError::ResourceLimit` |
| **Active task input** | `max_in_flight_blocks` (queue capacity) | Bounded job channel (`effective_queue` slots); producer blocks when full |
| **Unordered output** | bounded result channel (`result_capacity`) | Workers block when full; `run_tasks_with_sink` consumes results live |
| **Ordered committed output** | `max_buffered_output_bytes` + `max_in_flight_blocks` | `ReorderBuffer` rejects inserts that would exceed either bound (`ResourceLimit`); a single block's output allocation is also capped by `max_buffered_output_bytes` |
| **Worker scratch** | `max_retain` per buffer | `WorkerScratch::reset()` shrinks oversized buffers |
| **Model cache** | 64 entries / 16 MiB | `ModelCache` FIFO eviction |

**Streaming APIs:** `decode_with_sink` does **not materialise the whole
workload** — results stream through the bounded result channel into a live
`ReorderBuffer`, and the sink receives each block in ascending order as soon
as it becomes committable.  Peak memory is bounded by queue capacities plus
whatever the sink retains.  `decode_streaming` and
`decode_streaming_with_cancel` currently collect the job iterator into a
`Vec` before dispatch (see [Limitations](#limitations)); they still enforce
the input and output budgets and return the canonical `stream_hash`.

`estimate_memory(config, avg_block_size, worker_count)` returns a
conservative `ParallelMemoryEstimate` (fixed overhead + per-worker + in-flight
+ reorder components) using saturating arithmetic.  It is a documented
extension API for downstream capacity planning — the engine itself does not
consume it.

---

## Trust Boundaries and Input Invariants

Block data from untrusted sources is handled defensively:

- **Strict block parser** (`parse_block_header`): fixed 104-byte header,
  `"RYGR"` tag, `header_size == 104`, `block_version == 1`, reserved bytes
  zero, `codec_id` ∈ {7, 8}, `scale_bits` validated **before any shift**
  (1..=15), `state_count` must match the codec (8 for codec 7, 16 for codec
  8), `model_encoding` ∈ {0, 1, 2}, no trailing bytes, minimum payload for
  non-empty RANS blocks, `uncompressed_length` capped at 256 MiB.
- **Model validation**: model length must be 0 (synthesise uniform) or 1024
  (256 × u32 LE); frequencies must sum to exactly `1 << scale_bits`; only
  validated models are cached.
- **Checked arithmetic**: every offset and length uses `checked_add` before
  any slice is formed; output allocation is bounded by
  `max_buffered_output_bytes`.
- **No panics on malformed input**: every malformed-input path returns a typed
  `BlockError` / `ParallelError`; worker panics are contained by
  `catch_unwind` and surfaced as `WorkerPanic`.
- **Completeness**: cancellation or internal shortfall can never surface as a
  successful `Ok` with fewer blocks than declared.
- **Config fail-fast**: invalid configuration returns `ParallelError::Config`
  before any thread spawns.

---

## SIMD Requirements

- The `simd` feature (default: **on**) enables the `ryg-rans-rs-simd`
  dependency.  **Non-explicit policies never select SIMD plans** — `Portable`
  / `ScalarPreferred` / `Auto` are scalar-only by design.
- Explicit SIMD requests execute only when the build was compiled with the
  required target features **and** the host CPU supports them; otherwise the
  call returns `BackendUnavailable` (never a scalar substitution).  Build with
  e.g. `RUSTFLAGS="-C target-cpu=native"` to enable the SIMD kernels.
- `disable_simd = true` forces scalar plans for every non-explicit policy and
  makes an explicit SIMD request a typed config conflict.
- `Uniform256TableFree16` is scalar arithmetic (no vector instructions) and is
  not treated as SIMD by `disable_simd`.

---

## Unsafe Boundaries

- Default builds: `#![cfg_attr(not(feature = "affinity"), forbid(unsafe_code))]`
  in `lib.rs` — the crate is entirely safe Rust unless the `affinity` feature
  is enabled.
- The `affinity` feature (`dep:libc`, default **off**) enables two small
  unsafe libc sites in `affinity.rs` (`sched_setaffinity` / `CPU_SET`), each
  with a `SAFETY:` comment stating the exact invariants (pid 0 = calling
  thread, single-CPU set, standard macro layout).  This preserves
  `forbid(unsafe_code)` in default builds.
- Unsafe code in the underlying `ryg-rans-rs-simd` crate is machine-verified
  by `crates/ryg-rans-rs-simd/unsafe-ledger.toml` (bidirectional
  ledger↔source test + disassembly courts); every SIMD helper carries its own
  `#[target_feature]` attributes.

---

## Evidence Model

This crate's claims are evidenced at the levels it has reached; nothing here
is marked **Sealed**:

| Claim | Evidence |
|-------|----------|
| Deterministic output, bounded executor, atomic reorder commit, cancellation completeness, exact backend semantics, per-field config wiring, scratch/cache wiring | 105 unit + integration tests in `src/*.rs` and `tests/` (`phase_i_tests.rs`, `loom_tests.rs`) |
| Concurrency safety of the executor | `tests/loom_tests.rs` (Loom model under `--cfg loom`; see AGENTS.md exact commands) |
| Behavioural parity courts (receipts + manifests) | Phase L.19 courts — **OPEN** (gap ledger L19-A); no behavioural receipts yet |
| Performance | Phase K run receipt `RYG_RANS.PERF.PHASE_I.PARALLEL` under `evidence/performance/runs/phase-k-*` — **superseded** (defects L1-A…L1-S); Phase L.18 re-seals through `cargo xtask benchmark-run` + `cargo xtask performance-seal`.  No performance claim is marked Sealed until the seal gate passes. |

How to verify a claim: find the claim, find the producing code path, find the
test/court that pins it, find the receipt in `evidence/`, run the seal gate.
If any link is missing, the claim is not sealed.

---

## Performance Methodology

- The sealed measurement surface is the **Criterion suite** in
  `ryg-rans-rs-bench`: the `parallel` tier (block-level scaling, 1–16
  threads, `FixedBlockPlan` overhead) and the `container` tier (end-to-end
  encode→decode round-trip at various block sizes).
- All benchmarks use deterministic corpora (8 fixed-seed profiles) and verify
  output parity **before** timing; benchmark IDs join to preflight records
  (backend requested/executed, input/output hashes, words consumed, final
  states, thread counts).
- The `bench` subcommand of the CLI is a separate live smoke measurement, not
  part of this crate.
- Run with `RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench`.
- No throughput numbers are quoted in this README: the Phase K measurements
  are superseded, and Phase L.18 is re-sealing.

---

## Limitations

Honest, current (Phase L.15):

- **`decode_streaming` / `decode_streaming_with_cancel` materialise the job
  iterator into a `Vec` before dispatch** (noted in `decode.rs`).  They still
  enforce the input/output budgets and produce the canonical `stream_hash`.
  The truly bounded streaming path is `decode_with_sink`, which feeds results
  through a live `ReorderBuffer` without accumulating the workload.
- **Encode tasks currently ignore the scratch's `model_buffer`** —
  `encode_single_block` allocates its own model bytes; the scratch is
  accepted but not yet plumbed into the encode path.
- **The `ModelCache` is not `Sync`**; the global cache is Mutex-guarded with
  an O(N) linear scan and approximate byte accounting (documented in
  `cache.rs`).  Fine for tens-to-hundreds of unique models per process.
- **`BackendPolicy::Auto` and `ScalarPreferred` are scalar-only today**;
  `ModelAware` adds only the Uniform256 table-free kernel.  SIMD requires
  explicit opt-in.
- **`Batch4` backends are unreachable through the one-block API** (typed
  `BackendRequiresBatchContext`); no coordinator-level batch decode API exists
  yet.
- **Loom runs are not part of the default test command**; the model exists at
  `tests/loom_tests.rs` and runs under `RUSTFLAGS="--cfg loom"`.
- **`ScratchPool` is public but the executor creates scratch inline** — the
  pool is a utility, not the production path.
- **No signal-handling integration** (SIGINT/SIGTERM wiring is tracked as
  gap-ledger residual L3-D, OPEN).
- Fuzz targets for the parser/codecs and sanitizer/Miri runs are tracked in
  the gap ledger (L16-B PARTIAL, L16-D OPEN).

---

## Examples

Every API name below exists in the current source (grep `src/` to confirm).

### Encode

```rust
use ryg_rans_rs_parallel::{
    CodecPolicy, FixedBlockPlan, ModelPolicy, ParallelConfig, ParallelEncoder,
    EncodeBlockJob,
};

let data: Vec<u8> = vec![/* your input data */];

// Plan blocks — deterministic, thread-count-independent
let plan = FixedBlockPlan::new(data.len() as u64, 4096);

// Build encode jobs
let jobs: Vec<EncodeBlockJob> = plan
    .ranges
    .iter()
    .map(|r| {
        let start = r.input_offset as usize;
        EncodeBlockJob::new(
            r.block_index,
            data[start..start + r.length as usize].to_vec(),
            CodecPolicy::Auto,   // codec 8 (16-way) by default
            ModelPolicy::PerBlock,
            12,                  // scale_bits
        )
    })
    .collect();

let config = ParallelConfig::default();
let encoded = ParallelEncoder::encode_blocks(jobs, &config)?;

// Blocks are in ascending block-index order
for block in &encoded.blocks {
    println!(
        "Block {}: {} bytes -> {} bytes, backend: {}",
        block.block_index,
        block.input_length,
        block.block.len(),
        block.backend.label(),
    );
}
```

`encode_planned(plan, data, config)` builds the same jobs from a raw slice,
and `ParallelConfig::default_scale_bits()` returns the default (12).

### Decode

```rust
use ryg_rans_rs_parallel::{DecodeBlockJob, ParallelConfig, ParallelDecoder};

let blocks: Vec<DecodeBlockJob> = /* encoded block records from the encode step */;

let config = ParallelConfig::default();
let decoded = ParallelDecoder::decode_blocks(blocks, &config)?;

// Blocks are in ascending block-index order; stream_hash is canonical
for block in &decoded.blocks {
    println!(
        "Block {}: {} bytes decoded, backend: {}, words_consumed: {}",
        block.block_index,
        block.output.len(),
        block.backend.label(),
        block.words_consumed,
    );
}
```

### Verify

```rust
use ryg_rans_rs_parallel::{ParallelConfig, ParallelVerifier, VerifyBlockJob};

let verify_jobs: Vec<VerifyBlockJob> = /* block records from the encode step */;
let config = ParallelConfig::default();
let report = ParallelVerifier::verify_blocks(verify_jobs, &config)?;

println!(
    "Verified {} blocks: {} payload hashes OK, {} decoded hashes OK, {} failed",
    report.blocks_verified,
    report.payload_hash_ok,
    report.decoded_hash_ok,
    report.blocks_failed,
);
```

### External cancellation

```rust
use std::sync::Arc;
use std::time::Duration;
use ryg_rans_rs_parallel::{CancellationToken, ParallelConfig, ParallelDecoder, DecodeBlockJob};

let cancel = Arc::new(CancellationToken::new());
let cancel_worker = cancel.clone();

// Cancel from another thread after a timeout
std::thread::spawn(move || {
    std::thread::sleep(Duration::from_secs(5));
    cancel_worker.cancel();
});

// The token is checked cooperatively; cancellation returns
// ParallelError::Cancelled { completed, expected } — never Ok with fewer
// blocks than declared.
match ParallelDecoder::decode_blocks_with_cancel(blocks, &ParallelConfig::default(), Some(cancel))
{
    Ok(decoded) => println!("decoded {} blocks", decoded.blocks.len()),
    Err(ryg_rans_rs_parallel::ParallelError::Cancelled { completed, expected }) => {
        println!("cancelled after {}/{} blocks", completed, expected);
    }
    Err(e) => println!("error: {}", e),
}
```

The same `_with_cancel` pattern applies to `encode_blocks_with_cancel`,
`decode_streaming_with_cancel`, `decode_with_sink`, and
`verify_blocks_with_cancel`.

### Bounded streaming decode

```rust
use ryg_rans_rs_parallel::{DecodeBlockJob, ParallelConfig, ParallelDecoder};

let config = ParallelConfig::default();
// Peak memory is bounded by the queue capacities; each block is delivered to
// the sink in ascending block-index order as soon as it is committable.
let report = ParallelDecoder::decode_with_sink(blocks, &config, None, |decoded| {
    println!("block {} decoded", decoded.block_index);
})?;
println!("effective workers: {}", report.effective_workers);
```

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `ParallelError::ResourceLimit("max_buffered_input_bytes exceeded: ...")` | Total compressed input exceeds the input budget; raise `max_buffered_input_bytes` or split the workload |
| `BlockErrorKind::ResourceLimit` from the reorder stage | Completed-but-unordered output exceeded `max_buffered_output_bytes`, or a single block's `uncompressed_length` exceeds it; raise the output budget for large blocks |
| `ParallelError::Config("affinity policies other than None require the 'affinity' feature on Linux")` | Non-`None` `AffinityPolicy` without the `affinity` cargo feature; enable the feature on Linux or use `AffinityPolicy::None` |
| `ParallelError::Config("AffinityPolicy::Explicit requires a non-empty CPU list")` | `Explicit(vec![])` is invalid |
| `BlockErrorKind::BackendUnavailable` | An explicit SIMD request on a CPU/build without the required features, or `disable_simd` combined with an explicit SIMD policy |
| `BlockErrorKind::BackendFormatMismatch` | The requested backend does not match the block's stream format (8-way↔codec 7, 16-way↔codec 8, RAW/RLE↔block kind) |
| `BlockErrorKind::BackendRequiresBatchContext` | A `Batch4` backend was requested through the one-block API; batch backends need coordinator-level batch context |
| `ParallelError::Cancelled { completed, expected }` | Expected — cancellation was observed; `completed < expected` by design, and it is an `Err`, never a short `Ok` |
| `ParallelError::WorkerPanic { block_index, worker_index }` | A worker panicked; the panic is contained and all workers are joined.  File a bug with the block index |
| No SIMD execution with `Auto` | Expected — `Auto` is scalar-only today; use `BackendPolicy::Explicit(BackendId::...)` and build with the required target features |
| `ParallelError::IncompleteExecution` | The executor finished without cancellation but produced fewer results than declared — an internal invariant violation; report it |

---

## Versioning

- Version **0.3.0**, shared with the workspace.  The crate follows semantic
  versioning; the public API is the inventory under `docs/public-api/`
  (generated by `cargo public-api` — do not hand-edit).
- Breaking changes land only in new major versions; removed configuration
  fields (`disable_inner_batching`, single-option `error_policy`) are gone and
  must not be relied upon.
- Backend labels (`BackendId::label()`) are stable kebab-case strings; parse
  them as diagnostics only — use the enum for dispatch.

---

## Reading Order

1. `docs/glossary.md` — exact project terminology (Block, Task, In-flight,
   Reorder buffering, Committed output, Cancellation, Input/Output budget,
   In-flight bound, requested/effective workers, Strict integrity).
2. `docs/architecture.md`
3. `docs/bitstream-contract.md` — pinned upstream stream formats.
4. `docs/container-format-v1.md` — the RYGRANS v1 container.
5. This crate's `src/lib.rs` module docs, then `config.rs`, `executor.rs`,
   `decode.rs`, `encode.rs`, `verify.rs`, `reorder.rs`, `error.rs`,
   `cancellation.rs`.
6. `evidence/phase-l/gap-ledger.md` — the residual ledger (Phases L.2–L.9
   ground this crate's invariants).
7. `docs/unsafe-ledger.md` and `crates/ryg-rans-rs-simd/unsafe-ledger.toml`.
8. `docs/performance-method.md` and `docs/residual-doctrine.md`.

---

*Part of the ryg-rans-rs project. Version 0.3.0. Phase L.*
