# Glossary — exact project terminology

Every document in this repository uses these terms with exactly these
meanings.  If a document uses a term differently, that document is wrong.

## Data model

* **Block** — one independently decodable unit of a container or parallel
  job: a header, optional model data, and a payload.  A block decodes to a
  fixed number of bytes (`uncompressed_length`).
* **Stream** — the canonical byte/word sequence produced by one codec for
  one block's payload.  "8-way stream" means the interleaved 8-state format
  produced by the 8-way encoder, not a byte stream of width 8.
* **Surface** — a named, evidence-tracked capability row in the README
  Evidence Status table (e.g. "32-bit byte rANS — division + reciprocal").
  A surface aggregates many behavioural receipts and one performance
  receipt.

## Codec and backend

* **Codec** — the stream format: number of states, renormalization unit,
  scale constraint.  Identified by a stable `codec_id` (see
  `container::codec`).  A codec does *not* pick the arithmetic
  implementation.
* **Backend** — the execution engine that decodes a stream: scalar table
  lookup, SSE4.1 SIMD, AVX2, AVX-512, batch.  One codec can be decoded by
  several backends.
* **Plan** — the validated decision about how to decode one block: which
  backend, which tables, under which execution conditions.  A plan is only
  valid for the exact codec, scale, model bytes, and capability context it
  was built for.
* **Requested backend** — the backend the caller asked for (explicitly or
  via policy).  Recorded in every execution result.
* **Executed backend** — the backend that actually ran.  With the exact
  backend contract, requested == executed or the call returns a typed
  error; silent substitution is prohibited.
* **Uniform256** — the specialized table-free Uniform256 decode path for
  models where every symbol's frequency is exactly `2^(scale_bits-8)`.
* **Batch4** — the 4-stream batched AVX2 decode backend.  It requires a
  coordinator-level batch context and is therefore not reachable through
  the one-block API (which returns a typed error).

## Evidence

* **Receipt** — a SHA-256-chained JSON artifact proving one claim: a
  behavioural receipt proves one observed behaviour; a performance receipt
  proves one measured surface.  A receipt carries a canonical self-hash.
* **Manifest** — the machine-readable inventory of a receipt's inputs:
  benchmark cases (performance) or court cases (behavioural), with hashes
  of every referenced artifact.
* **Residual** — a tracked, classified, unresolved-or-accepted discrepancy
  between observed and expected behaviour.  Residuals are never deleted;
  they are resolved or explicitly accepted with justification.
* **Seal** — the state of a claim after the complete seal gate passes:
  every receipt file exists, every hash verifies, every self-hash
  recomputes, every referenced artifact exists, and the working tree was
  clean at evidence time.
* **Behaviour seal** — seal of a behavioural surface (144 receipts at the
  Phase K baseline; the Phase L courts extend this total — the number is
  generated from the evidence index, never hardcoded).
* **Performance seal** — seal of a measured performance surface (10
  receipts).  "Sealed" means the measurement and its provenance are sealed,
  not that the implementation is fastest.
* **Preflight** — the structured verification record emitted by a
  benchmark case before timing: backend requested/executed, input/output
  hashes, words consumed, final states, thread counts.  Criterion timing is
  joined to preflight by exact benchmark ID.
* **Canonical output** — the byte-exact expected output of an operation
  under the pinned upstream format, asserted by parity courts.
* **Canonical error** — the deterministic error selected when multiple
  blocks fail: the lowest failing block index.  Independent of completion
  order and thread count.
* **Strict integrity** — the default verification policy: payload hash
  must match, decode must succeed, the stored decoded hash must be nonzero
  and match.  Zero/unset decoded hash fails with `DecodedHashMissing`;
  mismatch fails with `DecodedHashMismatch`.
* **Compatibility integrity** — the explicit opt-in policy
  (`AllowLegacyUnsetDecodedHash`): zero/unset decoded hashes are reported
  as `Unset` and do not fail solely for that reason; any nonzero mismatch
  still fails.

## Concurrency

* **Worker** — one OS thread in the executor's pool, with one exclusive
  `WorkerScratch` context.
* **Task** — one block's unit of work submitted to the executor.
* **In-flight** — submitted to a worker but not yet completed.
* **Reorder buffering** — holding completed-but-out-of-order results until
  their index becomes the next expected index.
* **Committed output** — results handed to the caller in strictly
  ascending block order by the coordinator.
* **Cancellation** — cooperative: a `CancellationToken` checked between
  units of work.  Cancellation returns `ParallelError::Cancelled {
  completed, expected }` and can never return `Ok` with fewer blocks than
  declared.

## Model cache (Phase O)

* **Model artifact** — the validated, immutable decode inputs for one
  model: the 256-symbol frequency vector, the Uniform256 flag, and (SIMD
  builds) the 16 KiB packed word table.  Built by the single canonical
  constructor `build_validated_model_artifacts`.
* **ModelCacheKey** — `(model_sha256, scale_bits, codec_id)`: the byte-exact
  identity of a model artifact.  Built by `ModelCacheKey::from_model`.
* **ModelCache** — the exact-accounting FIFO core: `HashMap<Key, CacheEntry
  { value: Arc<T>, accounted_bytes }>` + a FIFO `VecDeque` in set
  equality.  `current_entries` and `current_bytes` are exact after every
  public operation; `max_entries == 0` or `max_total_bytes == 0` disables
  the cache.
* **ModelArtifactCache** — the explicitly owned, thread-safe cache with
  per-key single-flight construction (`Building` marker + condvar).  A
  builder panic is caught (`Panicked`, never a permanent `Building`
  state); a cache-internal failure bypasses to the same canonical
  constructor and is never reported as a model error.
* **Accounted bytes** — the exact per-entry byte cost tracked by the
  cache (frequencies + packed table + fixed overhead), computed by the
  canonical constructor so cached and uncached paths agree.
* **Single-flight** — N concurrent same-key cold requests perform exactly
  one construction; the N-1 waiters receive the same `Arc` artifact.
  **Builder-marker ownership** (post-v0.5.0 audit, `MODEL_CACHE.RACE.3`):
  only the builder may remove the in-flight `Building` marker; a departing
  waiter only decrements the diagnostic waiter count.
* **Design-A accounting** (post-v0.5.0 audit, `MODEL_CACHE.METRICS.2`) —
  the hit/miss classification rule: a lookup whose initial check finds no
  artifact is a **miss** whether the caller becomes the builder, a
  coalesced waiter, or a cancelled waiter; a waiter that later receives
  the published artifact is a miss, never a second hit.  This keeps
  `hits + misses == lookups` true under cancellation.
* **CacheInsertOutcome** — the typed insertion verdict (`Inserted`,
  `Replaced`, `RejectedDisabled`, `RejectedOversized { entry_bytes,
  max_total_bytes }`); oversized entries are delivered for the current
  decode but never retained, and nothing useful is evicted to find out.
* **Disabled bypass** — a zero-capacity cache serving every request by
  direct construction (the semantic baseline); counted by
  `disabled_bypasses`.
* **ModelCacheMetricsSnapshot** — the authoritative behavior counters
  (lookups/hits/misses/builds/coalesced waiters/insertions/replacements/
  evictions/oversized/disabled/fallbacks/current+peak entries and bytes)
  with the invariants `hits + misses == lookups` (Design-A accounting;
  holds under cancellation) and `builds_completed + build_failures <=
  builds_started`.
* **ModelPolicy** — the encode-side model construction policy: `PerBlock`
  (natural mode) or `External { model }` (grouped mode, Phase O.13).  The
  documented-but-inert `Uniform`/`Global` variants were removed in Phase O
  (residual `ENCODE.MODEL_POLICY.1`).
* **Synthetic-cache-stress / synthetic-cache-soak** (aliases `stress` /
  `soak`) — the Phase O.12 cache-behaviour classes on deterministic
  xorshift payloads with constant seeds, honestly labeled
  `synthetic-cache-stress-v1` (post-v0.5.0 audit, `MODEL_CACHE.WORKLOAD.2`:
  these payloads are NOT corpus-derived).
* **Stress-public / soak-public** — the genuine public-corpus runners:
  every block resolves `source_id + source_sha256 + offset + length` to
  hash-verified extracted source bytes; `--schedule` selects the executed
  derived schedule (smoke/1g/mixed-16g/stress-64g).  Only these (and the
  Criterion `model_cache/public` group) may claim corpus provenance.
* **Compiled target / runtime CPU** (post-v0.5.0 audit,
  `PERF.EVIDENCE.1`) — the typed performance metadata facts:
  `compiled_target { target_cpu, enabled_target_features, codegen_flags }`
  (codegen bound to the benchmark run's `host.json`) versus
  `runtime_cpu { detected_features }` (host capability via runtime
  detection).  `profile` is `not_applicable` where the profile dimension
  does not apply.

## Configuration

* **Worker count** — `requested_workers` (what the caller asked for) vs
  `effective_workers` (what actually ran).  `ExecutionMode::SequentialThresholdFallback`
  reports `effective_workers = 1` without spawning a pool.
* **In-flight bound** — `max_in_flight_blocks`: the job-queue capacity.
* **Input budget** — `max_buffered_input_bytes`: the bound on compressed
  input queued plus executing, enforced during submission.
* **Output budget** — `max_buffered_output_bytes`: the bound on
  completed-but-unordered output, enforced against the live reorder stage.
