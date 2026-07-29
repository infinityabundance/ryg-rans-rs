# ryg-rans-rs-parallel — Deterministic Parallel Block Engine

## Overview

Phase I of `ryg-rans-rs` adds a **deterministic parallel block engine** that preserves all existing sealed single-core codecs while adding bounded, reproducible block-level concurrency around them.

The defining invariant:

> Thread count, worker scheduling order, completion order, CPU topology, and selected compatible execution backend may change **performance**, but must never change encoded bytes, decoded bytes, block boundaries, models, hashes, footer values, canonical errors, or forensic results.

## Architecture

### Why rANS state streams are not split across threads

Each rANS decoder state depends on every previous symbol in the stream. Splitting one state stream across threads would require either:

1. Breaking the state dependency chain (impossible — the ANS state carries the accumulated entropy of all prior symbols).
2. Introducing a new container format with explicit checkpoints (a future format-level feature).

Instead, Phase I exploits **block-level parallelism**: the input is partitioned into independently decodable blocks, each with its own model and compressed payload. This is the same strategy used by block-based compression formats (Brotli, Zstandard, LZMA2) and independently decodable rANS block engines.

### Independent blocks

1. A fixed block plan partitions the input into deterministic ranges.
2. Each block independently builds its frequency model and compressed payload.
3. Workers encode and decode blocks concurrently — no shared state between blocks.
4. An ordered reorder buffer ensures blocks are committed in ascending index order.

### Seekable versus streaming architecture

- **Seekable**: A planning pass validates the complete container structure, then workers decode disjoint block ranges into disjoint output regions.
- **Streaming**: A sequential container reader feeds a bounded queue. Workers decode concurrently. The reorder buffer emits blocks in order as they become available.

### Resource bounds

All queues, buffers, and data structures are bounded. Configuration includes limits for:

- In-flight blocks (count-based backpressure)
- Buffered input bytes (memory-based backpressure)
- Buffered output bytes (memory-based backpressure)
- Per-worker scratch buffer retention

### Cancellation

A cooperative `CancellationToken` allows signalling cancellation from any thread.
Workers check cancellation at defined yield points:
- Before beginning a block
- After expensive model construction
- Before encoding or decoding
- Before hashing

Once cancelled, no new expensive work begins, in-progress work may abort at checkpoints, and all worker threads are still joined.

### Worker panic behavior

Every worker task is surrounded by `catch_unwind`. Panics are converted to typed `ParallelError::WorkerPanic` errors. The executor continues to shut down cleanly, joining all workers before returning.

### Canonical lowest-block error

Parallel discovery order is nondeterministic. The canonical error selection returns the failure associated with the lowest block index, using a stable same-block priority:
1. Format validation
2. Resource limit
3. Payload integrity
4. Model validation
5. Codec failure
6. Decoded integrity
7. Worker panic
8. Output commit

### Model-aware backend selection

A `DecodePlan` is created from validated block metadata, runtime CPU capabilities, and the configured backend policy. The planner selects the optimal inner decode kernel per block:
- `Scalar16`: portable word rANS 16-way scalar
- `Avx512Vl2x8`: two 256-bit gather chains on 16-way format
- `Avx512Batch4`: batched decode of 4 independent streams
- `Uniform256TableFree16`: no-table uniform256 decode

## Project Status

| Step | Description | Status |
|------|-------------|--------|
| 1-7 | Foundation: crate, config, errors, executor, cancellation, block planner | Complete |
| 8-14 | Core implementation: encode, decode, reorder buffer, roundtrip | Complete |
| 15-19 | Extended: verify, decode plans, scratch reuse, reports | Complete |
| 20-22 | Testing: scheduling injection, cancellation/panic tests, mixed blocks | Complete |
| 23 | Loom concurrency tests | Scaffolded |
| 24 | Fuzz targets | Pending |
| 25-26 | Kani proofs, sanitizers | Pending |
| 27-40 | Courts, Docker, benchmarks, sealing, publishing | Pending |

## Crate Map

| Path | Responsibility |
|------|---------------|
| `src/lib.rs` | Module structure, invariants, threat model |
| `src/config.rs` | `ParallelConfig`, `ThreadCount`, `BackendPolicy`, `ErrorPolicy` |
| `src/error.rs` | `ParallelError`, `CanonicalErrorTracker`, `BlockError` |
| `src/executor.rs` | `run_tasks()`, `ExecutorTask`, `ExecutorReport` |
| `src/cancellation.rs` | `CancellationToken` |
| `src/job.rs` | `EncodeBlockJob`, `DecodedBlockResult`, `VerifyBlockJob` |
| `src/plan.rs` | `FixedBlockPlan`, `BlockRange` |
| `src/reorder.rs` | `ReorderBuffer<T>`, `HasBlockIndex`, `BufferSized` |
| `src/encode.rs` | `ParallelEncoder::encode_blocks`, `encode_single_block` |
| `src/decode.rs` | `ParallelDecoder::decode_blocks`, `decode_single_block` |
| `src/decode_plan.rs` | `DecodePlan`, `create_decode_plan`, `plan_cache_key` |
| `src/verify.rs` | `ParallelVerifier::verify_blocks` |
| `src/cache.rs` | `ModelCache<T>`, `ModelCacheKey` |
| `src/resource.rs` | `ParallelMemoryEstimate`, `effective_worker_count` |
| `src/scratch.rs` | `WorkerScratch`, `ScratchPool` |
| `src/schedule.rs` | `DelaySchedule`, `DeterministicScheduler` |
| `src/report.rs` | `ParallelExecutionReport`, `ParallelBlockReport` |
