# Phase O — Baseline (O.0)

Frozen state before any Phase O code change.

- commit: 5cae5bd55f6a6ff0170946b6c9bd4be725f9aead
- tree:   6c549ff026ca046b2b30a295a0a8ee048623c297
- Cargo.lock SHA-256: 09daf043fcccddcd39f42012b3840413c9370164e013ad5a3cbee582b9f8a3d1
- rustc 1.96.0 (ac68faa20 2026-05-25); cargo 1.96.0
- working tree clean at freeze time
- release v0.4.1 (tag v0.4.1 @ b932143), evidence sealed at implementation
  commit 6264b53 (run phase-l-20260802f, 800x100, 158 behavioural receipts,
  Docker ci-20260802-6264b53 11/11)

## Current cache implementation (pre-Phase-O)

- `crates/ryg-rans-rs-parallel/src/cache.rs` (527 lines)
- `ModelCache<T>`: `VecDeque<(ModelCacheKey, T)>` FIFO, `max_entries`,
  `max_total_bytes`, `current_bytes` (approximate; eviction subtracts the
  *new* entry's size — MODEL_CACHE.BOUND.1)
- `ModelCacheKey::from_model(codec_id, scale_bits, model_data)` — SHA-256 of
  exact model bytes + scale + codec
- `ValidatedModelArtifacts { freqs: Arc<Vec<u32>>, uniform256,
  packed_table: Option<Arc<PackedWordTable>> }` (simd feature)
- Process-global `GLOBAL_MODEL_CACHE: OnceLock<Mutex<ModelCache<...>>>`
  (64 entries, 16 MiB) — the implicit global owner (MODEL_CACHE.CONTENTION.1)
- `cached_model_artifacts(codec_id, scale, model_data, build)` wrapper:
  lock().ok()? lookup → build outside lock (duplicate builds possible,
  MODEL_CACHE.RACE.1) → lock().ok() insert (lock failure silently skipped;
  lock().ok()? maps poisoned lock to None → false Model error,
  MODEL_CACHE.AVAILABILITY.1)
- No metrics (MODEL_CACHE.METRICS.1); insert() never dedups keys
  (MODEL_CACHE.RACE.2); max_entries==0 admits an entry
  (MODEL_CACHE.BOUND.3); oversized entries retained (MODEL_CACHE.BOUND.2)

## Production consumption path (verified, O.0 audit record)

decode_single_block (decode.rs:281)
→ cached_model_artifacts (cache.rs:311)
→ GLOBAL_MODEL_CACHE lookup → miss construction (decode.rs:339)
→ insertion (cache.rs:337) → execute_decode_plan → borrowed Arc<PackedWordTable>

## Baseline artifacts archived below

- workspace-check.txt
- workspace-test.txt
- parallel-test.txt
- native-simd-test.txt
- seal.txt
