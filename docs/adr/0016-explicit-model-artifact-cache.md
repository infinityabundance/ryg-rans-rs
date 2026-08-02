# ADR-0016 — Explicit model-artifact cache ownership with single-flight construction

Status: Accepted

## Context

Before Phase O, the model artifact cache was a **process-global implicit
cache**: `static GLOBAL_MODEL_CACHE: OnceLock<Mutex<ModelCache<...>>>` in
`crates/ryg-rans-rs-parallel/src/cache.rs`, consulted through the
`cached_model_artifacts` wrapper from `decode_single_block`.  This made:

* **cold/warm benchmarking ambiguous** — a "cold" benchmark could not prove
  it started cold, because the process-global may have been warmed by an
  earlier test or benchmark case;
* **test contamination possible** — one test's insertions changed another
  test's hit/miss behavior;
* **lifetime hidden** — nothing owned the cache; it lived for the process;
* **per-decoder policy impossible** — tenants or workloads could not have
  different budgets or disabled caches.

The Phase O residuals `MODEL_CACHE.RACE.1` (concurrent cold misses duplicate
construction), `MODEL_CACHE.RACE.2` (duplicate keys), `MODEL_CACHE.BOUND.1-3`
(approximate byte accounting, oversized retention, zero-capacity admission),
`MODEL_CACHE.AVAILABILITY.1` (poisoned lock mapped to a false model error),
`MODEL_CACHE.METRICS.1`, `MODEL_CACHE.CONTENTION.1`, and
`MODEL_CACHE.PERF.1` documented the concrete defects.

## Problem

Phase O.4 required: no hidden global state in correctness or benchmark
paths; cold runs create a fresh cache; warm runs reuse a known instance;
tests inject tiny budgets; applications isolate tenants; cache lifetime is
explicit; the default stays ergonomic.  The same phase required (O.5)
single-flight construction: N concurrent cold requests for one key perform
exactly one build.

## Alternatives considered

1. **Process-global cache, repaired in place.**  Keep the
   `OnceLock<Mutex<ModelCache>>` and fix the accounting/duplicate-key
   defects.  Rejected: it cannot satisfy the cold/warm isolation
   requirement (O.4) — the benchmark and test contamination problems are
   structural, not bugs.
2. **`ParallelDecoder` owns an `Arc<ModelArtifactCache>`** (chosen).  The
   decoder is constructed with `ParallelDecoder::new(config)` (fresh cache)
   or `ParallelDecoder::with_model_cache(config, cache)` (caller-owned).
   The per-block entry point `decode_single_block(job, config, cache,
   cancel)` takes the cache explicitly.  Cold = new decoder; warm = reused
   decoder; tenant isolation = per-tenant cache instance; budget control =
   `ModelArtifactCache::bounded(entries, bytes)`.
3. **A `DecodeContext` struct threading config + cache through every
   call.**  More explicit than (2), but it duplicates the existing
   `ParallelConfig` argument that already flows everywhere; (2) achieves the
   same ownership clarity with less churn.

Single-flight design candidates (O.16 later measures them):

* **One global mutex + per-key `Building` marker + condvar** (chosen): the
  cache-state lock is held only for state transitions (check, register,
  publish); the expensive build runs outside it; waiters block on a condvar
  polled for cancellation.  This is the "per-key single-flight plus short
  global metadata lock" design the mission names.
* Read/write lock: rejected — the hit path already takes the mutex only for
  a HashMap lookup; an RwLock adds complexity without measured benefit.
* Sharded cache: rejected for now — O.16 must first measure whether global
  lock contention is material; the decision is revisit-able with data.

## Rejected alternatives

* **Keep `cached_model_artifacts` and add an explicit `&Cache` parameter.**
  Rejected: the wrapper was the carrier of the hidden-global design; an
  explicit owner makes the global impossible.
* **LRU eviction.**  Rejected in this ADR's scope: FIFO is retained until
  shadow simulation (O.17, ADR-0017) shows another policy's value; this ADR
  fixes ownership and single-flight, not policy.
* **Caching whole `DecodePlan`s.**  Rejected in ADR-0009 and reaffirmed:
  plans depend on runtime backend conditions; only model-derived immutable
  artifacts are cacheable.

## Decision

1. Remove the process-global cache.  `ModelArtifactCache` is the explicit,
   thread-safe owner, constructed by `ModelArtifactCache::bounded(max_entries,
   max_total_bytes)` or `ModelArtifactCache::disabled()`.
2. `ParallelDecoder` (and `ParallelVerifier`) become structs holding
   `config: ParallelConfig` and `model_cache: Arc<ModelArtifactCache>`.
   `ParallelDecoder::new(config)` creates the default bounded cache
   (64 entries, 16 MiB — the pre-Phase-O global defaults, now explicit);
   `with_model_cache` accepts a caller-owned cache.
3. `decode_single_block(job, config, model_cache, cancel)` takes the cache
   and the worker's cancellation token explicitly.  The decode path is
   `get_or_build` → single-flight → `build_validated_model_artifacts`
   (the one canonical constructor, Phase O.7).
4. Single-flight: a per-key `Building { waiters }` marker under the cache
   mutex; exactly one caller builds; waiters block on a condvar
   (`wait_timeout` polling for cancellation).  Failed builds remove the
   marker, notify all, and are retryable.  Panics are caught and converted
   to `ModelArtifactBuildError::Panicked`; no permanent `Building` state is
   possible.
5. Cache-internal failures (poisoned lock, accounting invariant violation)
   record `uncached_fallbacks` and bypass to a direct construction with the
   same canonical constructor — never a false model error (O.6).
6. This is a **breaking public API change** (the pre-1.0 semver rule maps
   it to a minor bump; the release decision is recorded in the gap ledger).

## Tradeoffs

* **Given up:** the zero-argument ergonomics of the old stateless API
  (`ParallelDecoder::decode_blocks(blocks, &config)`); callers now
  construct a decoder.  The `new(config)` constructor keeps the common case
  one line.
* **Given up:** an O(1) replacement path in the FIFO core (queue `retain`
  is O(N) on the rare replacement path).  Single-flight makes replacements
  rare (an explicit re-insert); lookups stay O(1) via the `HashMap`, and
  the hit path never scans the queue.
* **Gained:** provable cold/warm isolation; test isolation; per-tenant
  budgets; explicit lifetime; exact byte/count accounting; single-flight
  (one build per concurrent cold burst); observable metrics; cache failure
  transparency.

## Evidence

* Code: `crates/ryg-rans-rs-parallel/src/cache.rs` (`ModelArtifactCache`,
  `ModelCache`, `CacheInsertOutcome`, `ModelCacheError`,
  `ModelCacheMetricsSnapshot`, `build_validated_model_artifacts`);
  `decode.rs` (`ParallelDecoder`, `decode_single_block`); `verify.rs`
  (`ParallelVerifier`); `sync.rs` (`Condvar`, `MutexGuard`, `wait_timeout`).
* Tests: `cache.rs` unit tests (exact accounting, zero capacity, oversized,
  duplicate keys, overflow-boundary, panic, disabled equivalence, Arc
  sharing); `cache_proptests` (shadow-model property test);
  real-thread concurrency courts (same-key burst, different keys, failure
  retry, cancelled builder/waiter, eviction-vs-hit); loom courts in
  `tests/loom_tests.rs` (single-flight, failure/panic waiter release, no
  deadlock).
* Receipts (Phase O.20): `RYG_RANS.O.CACHE.EXACT_BYTES`,
  `RYG_RANS.O.CACHE.ZERO_CAPACITY`, `RYG_RANS.O.CACHE.OVERSIZED`,
  `RYG_RANS.O.CACHE.UNIQUE_KEYS`, `RYG_RANS.O.CACHE.SINGLE_FLIGHT`,
  `RYG_RANS.O.CACHE.FAILURE_EQUIVALENCE`, `RYG_RANS.O.CACHE.CANCELLATION`,
  `RYG_RANS.O.CACHE.METRICS`.
* Residuals resolved: `MODEL_CACHE.BOUND.1-3`, `MODEL_CACHE.RACE.1-2`,
  `MODEL_CACHE.AVAILABILITY.1`, `MODEL_CACHE.METRICS.1`,
  `MODEL_CACHE.CONTENTION.1`, `MODEL_CACHE.PERF.1` (see
  `evidence/phase-l/gap-ledger.md` Phase O section).

## Future implications

* A measured material global-lock contention finding (O.16) would justify a
  sharded or lock-free design; this ADR's single-flight + short-metadata-
  lock structure is the baseline the measurement compares against.
* An eviction-policy change (LRU etc.) would be a separate ADR (0017)
  driven by the O.17 shadow simulation; the ownership model is
  policy-agnostic.
* Stabilising the public API (cache construction, decoder ownership) is a
  candidate for the 1.0 compatibility boundary.
