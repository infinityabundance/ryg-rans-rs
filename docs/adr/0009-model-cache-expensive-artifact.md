# ADR-0009 — Model cache caches the expensive artifact, not the trivial one

Status: Accepted (revised)

## Context
Phase L.8 wired `ModelCache` into the decode path, but a later audit
found it cached only the cheap 1 KiB frequency parse.  The expensive
per-block work — constructing the 16 KiB `PackedWordTable`
(O(4096 × symbols)) — was still done on every block, so repeated models
(Uniform/Global policy) gained nothing: the cache was consulted but
inert.

## Problem
What should the cache store so repeated models genuinely avoid work?

## Alternatives considered
1. Store only `freqs` + `uniform256` (the original, inert design).
2. Store the full `DecodePlan` (backend-inclusive).
3. Store model-derived immutable artifacts including the packed table
   (Arc-shared), and select the backend after the lookup.

## Rejected alternatives
- (1) was rejected by the audit: no observable effect means no fix.
- (2) was rejected: a `DecodePlan` depends on the backend policy, runtime
  CPU capabilities, build features, and `disable_simd` — caching it would
  risk reusing a plan under incompatible execution conditions.  The L.8
  doctrine requires separating model-derived artifacts from runtime
  backend selection.

## Decision
`ValidatedModelArtifacts` holds `freqs: Arc<Vec<u32>>`, `uniform256`, and
(SIMD builds) `packed_table: Option<Arc<PackedWordTable>>`.  The table is
built once per unique model at cache-miss and Arc-shared on hit;
`execute_decode_plan` borrows it (`Cow::Borrowed`) instead of rebuilding.
A model the cache admitted without a table fails `PackedWordTable::from_freqs`
identically to the pre-cache path, so caching never changes error
identity.  `ModelCacheKey::from_model` is the single key constructor;
`plan_cache_key` delegates to it.  The cache remains bounded (64 entries,
16 MiB), FIFO-evicted, mutex-guarded, and correctness-independent.

## Tradeoffs
Gained: the cache now actually eliminates per-block table construction for
repeated models.  Given up: the trivial-artifact design's simplicity; the
cache entry is now ~17.5 KiB, and the byte budget must count the table.

## Evidence
`crates/ryg-rans-rs-parallel/src/cache.rs` (hit test asserts
`Arc::ptr_eq` on freqs AND table); the strengthened court CASE.009;
`docs/papers/0004-parallel-engine.md` §7.

## Future implications
If the SSE4.1 `RansWordTables` (slot + slot2sym) ever becomes a hot
explicit path, it can join the cached artifacts under the same key;
today it is explicit-only and rebuilt (documented in the code).
