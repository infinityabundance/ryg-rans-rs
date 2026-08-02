# ADR-0017 — Eviction policy: FIFO retained, LRU rejected on measured evidence

Status: Accepted

## Context

The model artifact cache (ADR-0016) evicts with a deterministic FIFO queue
(`VecDeque<ModelCacheKey>` front = oldest).  Phase O.17 required that the
eviction policy be a measured decision, not an assumption: capture
deterministic cache-key access traces from the public-corpus derived
schedules and simulate FIFO against LRU (and consider CLOCK only if
justified) before changing anything.

## Problem

"LRU sounds more sophisticated than FIFO" is not a reason to adopt it.
LRU carries real costs — a per-access timestamp update on every hit (a
write to the cache-state structure on the hot hit path) and more complex
eviction bookkeeping — while FIFO is a single `pop_front` with no
per-access mutation.  The question is whether the access patterns of the
actual workloads make LRU's extra complexity pay for itself.

## Evidence

`cargo xtask workload policy-sim public-rans-v1` simulates the derived
model-key sequences (grouped-model keys share `(model_group, codec_id,
scale_bits)`; natural-mode blocks are unique and therefore always miss
under either policy) at capacities 16/64/256/1024:

| Schedule | Blocks | Distinct groups | Cap | FIFO hit rate | LRU hit rate |
|---|---|---|---|---|---|
| smoke | 25 | 9 | any | 0.640 | 0.640 |
| 1g | 772 | 28 | 16 | 0.731 | **0.950** |
| 1g | 772 | 28 | 64+ | 0.964 | 0.964 |
| mixed-16g | 65536 | 32 | 64+ | 0.9995 | 0.9995 |
| stress-64g | 65536 | 64 | 64+ | 0.9990 | 0.9990 |
| mixed-16g / stress-64g | — | — | 16 | 0.000 | 0.000 |

Findings:

1. **At the production 64-entry capacity, FIFO and LRU are identical on
   every derived schedule** (same hit counts, byte-hit rates, eviction
   counts).  The schedules' grouped-model cycling fits inside 64 entries,
   so the eviction policy never discriminates.
2. LRU wins only at capacity 16 on the 1g schedule (0.950 vs 0.731) — a
   capacity four times below production, where the 28-group cycle exceeds
   the residency.  FIFO thrashes on the cold tail of the cycle; LRU keeps
   the recently-referenced groups.
3. At capacity 16 with a 32/64-group perfectly-cycling access stream, both
   policies score 0%: no replacement policy can retain a working set
   larger than the capacity when every access is to a different key.
4. The byte-hit analysis is identical to the entry-hit analysis because
   every admitted artifact has the same accounted size (measured 17,472
   bytes: 1 KiB frequencies + 16 KiB packed table + overhead) — there is
   no size-skew the policies could exploit.

## Decision

**FIFO remains the production eviction policy.**  The measured evidence
shows no material end-to-end benefit for LRU at the production capacity on
any derived schedule; LRU's only advantage appears at a capacity that the
production default never uses, and even there the absolute hit-rate
difference (0.95 vs 0.73) would need to be weighed against LRU's
per-access write cost on the hit path (which the Phase O.16 contention
evidence shows is already the dominant synchronization cost at high worker
counts — see `docs/performance/model-cache.md`).

## Tradeoffs

- FIFO: O(1) eviction, zero per-access mutation, deterministic insertion
  order (which is also what makes replacement in `ModelCache::insert`
  exact — the queue and the map are kept in set equality by construction).
- LRU: better retention of a hot tail when the working set exceeds a small
  capacity, at the cost of a timestamp write per hit and a scan (or a
  second index) for the victim.  The shadow simulation shows the hot-tail
  advantage only below the production capacity.

## Alternatives rejected

1. **LRU** — rejected on the evidence above.
2. **CLOCK (second-chance)** — a classic middle ground (approximate LRU
   with a reference bit, no scan on hit).  The simulation shows no policy
   change is justified at the production capacity; CLOCK's reference-bit
   write per hit has the same hot-path cost profile as LRU with none of
   the measured upside.  Not implemented.
3. **Size-aware policies (LFU / most-expensive-first)** — irrelevant: every
   artifact has the same accounted size, so size-aware admission/eviction
   degenerates to FIFO/LRU on the entry dimension.

## Evidence

- Shadow simulation command: `cargo xtask workload policy-sim public-rans-v1`
  (deterministic; re-runnable from the derived manifest).
- Contention evidence: `cargo run -p ryg-rans-rs-bench --bin model_cache_contention`.
- Performance receipts: `RYG_RANS.PERF.CACHE.THRASH` (FIFO churn measured),
  `RYG_RANS.PERF.CACHE.COLD_WARM`, `RYG_RANS.PERF.CACHE.CONCURRENCY`.

## Future implications

If a future workload shows a working set that exceeds the configured
capacity with a skewed reference distribution, re-run the shadow
simulation before changing the policy.  The simulation is deterministic
and lives in the workload tooling, so the comparison can be regenerated
for any new schedule without touching the production cache.
