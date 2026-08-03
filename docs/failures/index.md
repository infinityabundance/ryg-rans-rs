# Failure Encyclopedia (N.10)

> Every important bug the repository has lived through: the original
> assumption, the observed failure, the evidence, the root cause, the fix,
> the invariant introduced, and how the failure class is prevented from
> recurring.  This is one of the repository's most valuable assets: the
> failure classes *recur in new clothing*, and recognising the class is
> the first step of the fix.

## How to use this file

Each entry is a failure class.  When a new defect appears, classify it
first: which class does it belong to?  The prevention column names the
tooling that exists to catch the class.

---

## F-01 — The decoded-hash aggregate bug

* **Original assumption:** a block passes when the payload hash matches
  and decode returns success.
* **Observed failure:** a block with an intact payload hash and corrupted
  model bytes decoded to wrong output and *passed* verification.
* **Evidence:** the L.2 court reproduces it; the payload hash matched
  while the decoded output was wrong.
* **Root cause:** the verifier computed `decoded_hash_ok` but the
  aggregate failure condition ignored it.
* **Fix:** the decoded-hash verdict is load-bearing; strict integrity is
  the default.
* **Invariant introduced:** a decoded-output hash is required; payload
  hashing cannot catch model corruption.
* **Future prevention:** `RYG_RANS.L.VERIFY.DECODED_HASH` and
  `RYG_RANS.L.INTEGRITY.STRICT` courts; the 15-combination test matrix.
* **Code affected:** `crates/ryg-rans-rs-parallel/src/{verify,decode}.rs`,
  `config.rs`.
* **Receipts affected:** `RYG_RANS.L.VERIFY.DECODED_HASH`,
  `RYG_RANS.L.INTEGRITY.STRICT`.

## F-02 — The Phase K fabricated evidence

* **Original assumption:** the exporter's 831 records described the
  measurements.
* **Observed failure:** `sample_count = 1` everywhere, hardcoded
  `verification_passed`, empty hashes, zero throughput for 798 records,
  truncated archive paths, a tautological commit binding, an empty command
  log, and a seal printing "verified" after skipping.
* **Evidence:** the superseded run
  `evidence/performance/superseded/phase-k-20260731-004044/`.
* **Root cause:** values were defaults and file presence was mistaken for
  evidence.
* **Fix:** the exporter reads Criterion metadata; preflight records join
  timing; the run wrapper captures provenance; dual-hash receipts; the
  honest-seal rule.
* **Invariant introduced:** values come from execution, not defaults; a
  check that cannot fail is not a check; never print "verified" after
  skipping.
* **Future prevention:** residuals L1-A..L1-S; the performance-evidence
  seal gates.
* **Receipts affected:** all performance receipts (re-sealed).

## F-03 — The evidence-destroying promote

* **Original assumption:** the oracle's atomic-promote safely publishes
  evidence.
* **Observed failure:** the promote renamed the entire `evidence/` tree
  and deleted the backup, destroying unrelated evidence (a full-precision
  performance run, the gap ledger, the docker matrix).
* **Evidence:** residual L19-B; the destroyed run was re-executed.
* **Root cause:** local optimisation (publish the oracle's output) without
  a global view of what else lives in the tree.
* **Fix:** promote merges (upsert by court_id), preserving everything it
  did not generate.
* **Invariant introduced:** evidence is never deleted; it is superseded
  with a reason.
* **Future prevention:** the merge-only promote; the residual ledger.
* **Code affected:** `crates/ryg-rans-rs-oracle/src/main.rs`.

## F-04 — The reorder bound bug

* **Original assumption:** `max_in_flight_blocks` bounds the reorder
  stage.
* **Observed failure:** a slow early block stalled the pipeline; the
  queue-depth sweep (8–128) showed a flat, bound-dominated curve.
* **Evidence:** residual L17-B; the L.17 queue-depth sweep.
* **Root cause:** the reorder bound was `effective_queue` but needed
  `effective_queue + workers`.
* **Fix:** `reorder_block_bound(config, effective_workers)`.
* **Invariant introduced:** the reorder bound is `effective_queue +
  workers`.
* **Future prevention:** the stress tests; the loom courts.
* **Code affected:** `crates/ryg-rans-rs-parallel/src/decode.rs`.

## F-05 — The missed wakeup

* **Original assumption:** the channel layer was race-free.
* **Observed failure:** a blocked producer could sleep forever under
  certain schedules.
* **Evidence:** the loom model (L.16-C) found it; the sender count lived
  outside the mutex.
* **Root cause:** the wakeup condition was updated outside the critical
  section.
* **Fix:** move the sender count under the mutex.
* **Invariant introduced:** no lost wakeup; no deadlock.
* **Future prevention:** the loom executor courts.
* **Code affected:** `crates/ryg-rans-rs-parallel/src/sync.rs`.

## F-06 — The inert model cache

* **Original assumption:** "the cache is wired" (L.8 resolved).
* **Observed failure:** the cache stored only the cheap 1 KiB frequency
  parse; the expensive 16 KiB packed table was rebuilt per block — no
  throughput gain.
* **Evidence:** residual L8-REOPEN; the audit asked "what does this
  actually buy?"
* **Root cause:** a wiring claim was accepted without an observable
  effect.
* **Fix:** Arc-shared `PackedWordTable` in `ValidatedModelArtifacts`;
  `Arc::ptr_eq` hit tests.
* **Invariant introduced:** a wiring claim needs an observable effect.
* **Future prevention:** the strengthened court CASE.009;
  `RYG_RANS.L.MODEL_CACHE.INTEGRATION`.
* **Code affected:** `crates/ryg-rans-rs-parallel/src/cache.rs`,
  `decode.rs`.

## F-07 — The unenforced cancellation promise

* **Original assumption:** the `_with_cancel` doc comments described the
  behaviour.
* **Observed failure:** the final return paths returned `Ok` regardless,
  delegating the guarantee to the executor.
* **Evidence:** residual L3-REOPEN; the audit traced both functions to
  their returns.
* **Root cause:** a doc comment described a guarantee the code did not
  enforce.
* **Fix:** `error::check_completeness` at every public boundary.
* **Invariant introduced:** the guarantee belongs to the API boundary;
  cancellation never returns a short `Ok`.
* **Future prevention:** pre-cancelled-token tests through every entry
  point; `RYG_RANS.L.CANCEL.COMPLETENESS`.
* **Code affected:** `crates/ryg-rans-rs-parallel/src/{decode,encode,verify}.rs`,
  `error.rs`.

## F-08 — The fuzz-found defects

* **Original assumption:** the parsers and codecs handled malformed input.
* **Observed failure:** out-of-bounds model reads, a single-symbol u32
  threshold overflow, a short-slice unwrap, and a 1 GiB-per-iteration
  allocation in the R64 target.
* **Evidence:** residual L16-B; the fuzz corpus.
* **Root cause:** unchecked indexing and arithmetic on adversarial input.
* **Fix:** bounds checks, checked arithmetic, typed errors.
* **Invariant introduced:** no untrusted input may panic; malformed input
  returns typed errors.
* **Future prevention:** the nine fuzz targets; the malformed-input tests.
* **Code affected:** core parser paths, the word rANS threshold, the
  parallel block plan.

## F-09 — The SSE4.1 report-parity defect

* **Original assumption:** all backends report identically.
* **Observed failure:** the SSE4.1 kernel's report diverged from the
  scalar reference (L11-A).
* **Root cause:** a report-construction path did not match the kernel's
  actual execution.
* **Fix:** the report path was corrected to match what ran.
* **Invariant introduced:** the report is a property of the stream, not
  the kernel; every executable backend must agree.
* **Future prevention:** the report-parity courts (40 trials × both
  codecs × every executable backend).
* **Code affected:** `crates/ryg-rans-rs-simd/src/backends.rs`.

## F-10 — The evidence-metadata gaps

* **Original assumption:** the run artifacts described the run.
* **Observed failure:** an empty command log was hashed; host metadata
  was hashed in memory but not stored; `RUSTFLAGS` was recorded empty
  despite `-C target-cpu=native`; CPU features came from compile-time
  `cfg` on the sealer binary (empty set).
* **Evidence:** residuals L1-G, L1-H, L1-I, L1-J.
* **Root cause:** provenance captured at seal time, not benchmark time.
* **Fix:** the run wrapper captures everything before the run; host.json,
  commands.log, rustc-vV.txt, environment.json are stored and hashed as
  files; runtime/compiled/executed features are recorded separately.
* **Invariant introduced:** provenance describes the run, not the sealer.
* **Future prevention:** the run-manifest binding; the L1 gate set.

## F-11 — The archive path truncation

* **Original assumption:** the custom tar writer preserved paths.
* **Observed failure:** file names truncated at 99 bytes, including
  truncated `.json` extensions.
* **Evidence:** residual L1-K; the archived Phase K tree.
* **Root cause:** the custom writer lacked PAX/GNU long-name support.
* **Fix:** the maintained `tar` crate with deterministic mode; round-trip
  and corruption tests.
* **Invariant introduced:** no silent path truncation; archives must
  round-trip.
* **Future prevention:** the archive round-trip seal gate.

## F-12 — The eviction byte-accounting drift

* **Original assumption:** subtracting the *incoming* entry's size for
  every eviction keeps `current_bytes` correct.
* **Observed failure:** `current_bytes` drifts whenever entry sizes differ
  (mixed-size insertions, evictions) — the counter silently diverged from
  the retained set.
* **Evidence:** residual `MODEL_CACHE.BOUND.1`; the mixed-size unit tests;
  the shadow-model proptest.
* **Root cause:** eviction subtracted the wrong entry's size; the counter
  was approximate, not derived from the retained set.
* **Fix:** exact per-entry `accounted_bytes`; eviction subtracts the
  evicted entry's exact bytes; a two-phase insert plans the eviction set
  before mutating; checked arithmetic everywhere.
* **Invariant introduced:** `current_bytes == sum(retained accounted
  sizes)` after every public operation, recomputed by an independent
  verifier.
* **Future prevention:** `invariant_check`, the proptest shadow model, and
  the `RYG_RANS.O.CACHE.EXACT_BYTES` court.

## F-13 — The no-op skew generator

* **Original assumption:** a symbol-remap that maps a value to itself
  skews the distribution.
* **Observed failure:** the model-cache bench's hot-set mode produced only
  9 distinct models from 16 intended skews; the mode proofs failed.
* **Evidence:** the bench mode-proof failures; the probe counting distinct
  embedded models.
* **Root cause:** the remap condition `if s%256 == skew { skew }` was
  identity — the data was pure xorshift for every skew, and the frequency
  normalizer collapsed many histograms to the same table.
* **Fix:** a dominant-symbol generator (50% of bytes) guarantees distinct
  histograms per skew; unique mode uses per-block skews (plain xorshift
  collapsed 32 streams to 17 models).
* **Invariant introduced:** distinct skew ⇒ distinct normalized model;
  same skew + same seed ⇒ byte-identical blocks.
* **Future prevention:** the mode-proof preflight rejects any case whose
  metrics do not prove its intended model cardinality.

## F-14 — The inert ModelPolicy variants

* **Original assumption:** documented enum variants (`Uniform`,
  `External`, `Global`) were future features.
* **Observed failure:** the encoder never read `model_policy`; `External`
  could not even be expressed (no payload), `Uniform` referenced an index
  the API cannot supply, `Global` needs cross-block coordination a
  per-job field cannot express.  Every call site used `PerBlock`.
* **Evidence:** residual `ENCODE.MODEL_POLICY.1`.
* **Root cause:** documentation outran implementation; nothing forced the
  connection.
* **Fix:** redesigned to `PerBlock` + `External { model }` (implemented
  and validated: length/sum/scale checks, zero-frequency-symbol
  rejection); `Uniform`/`Global` removed as unimplementable.
* **Invariant introduced:** every documented policy has a production call
  path and an observable effect (Phase L.13 reachability doctrine).
* **Future prevention:** the public-API reachability court and the
  observable-effect doctrine.

## F-15 — The reorder index trap

* **Original assumption:** callers would always pass 0-based contiguous
  block indices because "the planner assigns them".
* **Observed failure:** a single-block encode at index 5 (and the
  public-corpus bench's per-block encodes) surfaced as a misleading
  `IncompleteExecution { completed: 0 }` — indistinguishable from an
  internal bug.
* **Evidence:** the probe reproducing the failure; the bench's training
  encode at group-id indices.
* **Root cause:** the reorder buffer commits ascending from index 0; a
  missing predecessor is buffered forever; the completeness check then
  reported an internal-bug error for a caller-contract violation.
* **Fix:** both public boundaries validate that job indices are exactly
  `0..bc` and return a typed `Config` error otherwise; the bench encodes
  single blocks at index 0 and patches the header index (offset 8..16,
  covered by no hash) to the schedule index.
* **Invariant introduced:** a caller-obligation violation is a typed
  error, never an internal-bug error or a hang.
* **Future prevention:** the boundary validation plus the bench's
  header-index patch.

## F-16 — The scheduler-dependent mode proof

* **Original assumption:** the cache's hit/miss/build counts for a
  concurrent thrash case are deterministic.
* **Observed failure:** multi-worker thrash proofs failed: eviction
  interleaving between FIFO evictions and concurrent lookups makes the
  counts scheduler-dependent.
* **Evidence:** the bench mode-proof failures at 2+ workers.
* **Root cause:** the proof asserted exact counts that only hold for
  sequential (1-worker) execution.
* **Fix:** exact proofs at 1 worker; deterministic bounds at N workers
  (every distinct model built, ≥ 1 eviction, hits + builds == blocks at
  the time); the report documents which numbers are exact and which are
  scheduler-dependent ranges.  **Later correction** (post-v0.5.0 audit,
  `MODEL_CACHE.METRICS.2`): Design-A accounting made coalesced misses
  *misses*, so the multi-worker bound became `lookups == blocks` and
  `hits + misses == lookups` — the exact-count assertion on builds was
  itself scheduler-dependent in a way the original fix had not fully
  named.
* **Invariant introduced:** output determinism is the invariant; cache
  *metric* determinism under concurrency is not claimed.
* **Future prevention:** mode proofs are data-driven and worker-aware.

## F-17 — The cancelled-waiter marker deletion (composition failure)

* **Original assumption:** a test proving cancellation works and another
  test proving single-flight works together prove that cancellation and
  single-flight compose.
* **Observed failure:** `finish_wait` removed the whole in-flight entry
  when the last waiter left; a cancelled waiter therefore deleted the
  RUNNING builder's marker, and a caller arriving between cancellation and
  publication registered as a second builder and duplicated the expensive
  construction (single-flight violated, `builds_started` inflated, cache
  churn).  The post-v0.5.0 audit found this by composing the two features;
  the cancellation test alone (one builder + one cancelling waiter, no
  third arrival) never exercised the window.
* **Evidence:** residual `MODEL_CACHE.RACE.3`; the three-party schedule
  (sleeping builder + cancelling waiter + late arrival) reproduces it; the
  mutation test (restoring the old `finish_wait`) fails the new tests.
* **Root cause:** the waiter path and the builder path both mutated the
  in-flight marker; ownership of the marker was not assigned.
* **Fix:** only the builder may remove the marker (publication, typed
  failure, caught panic, pre-build cancellation, shutdown); `finish_wait`
  only decrements the diagnostic waiter count (`4389d9b`).
* **Invariant introduced:** the builder is the sole owner of the in-flight
  marker; a departing waiter cannot affect single-flight.
* **Future prevention:** deterministic three-party court
  (`RYG_RANS.O.CACHE.CANCELLATION` CASE.004) and loom court
  `loom_cache_cancelled_waiter_keeps_builder_marker`; audit rule: prove
  every *pairwise* feature composition, not each feature alone.

## F-18 — The synthetic stress wearing a public-corpus label

* **Original assumption:** requiring the fetched corpus to exist and
  passing the derived manifest into the stress runner means the stress
  executes the public corpus.
* **Observed failure:** `run_stress_case` took `_manifest` and
  `_schedule_filter` as unused parameters; the payloads were xorshift
  expansions of constant seeds; the extracted source tree was a presence
  gate, not an input.  The completion claims "public-corpus stress
  executed" were false, and the `--schedule` flag was inert.
* **Evidence:** residual `MODEL_CACHE.WORKLOAD.2`; the audit traced the
  function parameters and the seed constants.
* **Root cause:** naming and presence were mistaken for provenance; the
  synthetic cache-behaviour classes (which are inherently synthetic) were
  conflated with corpus execution.
* **Fix:** two honestly separated families — `synthetic-cache-stress` /
  `synthetic-cache-soak` (labeled `synthetic-cache-stress-v1`, constant
  seeds, no corpus requirement) and `stress-public` / `soak-public`
  (every block resolves `source_id + source_sha256 + offset + length` to
  hash-verified extracted bytes; `--schedule` selects the executed
  schedule) (`0510ca0`).
* **Invariant introduced:** a result may claim public-corpus provenance
  only when the executed bytes are derived from the pinned sources;
  synthetic results carry the synthetic label.
* **Future prevention:** the honest labels in the tool output; the
  workloads README's identity-honesty note; audit rule: trace what data
  actually flows into the timed/measured region.

---

| Class | Examples | Prevention |
|-------|----------|------------|
| Aggregate condition ignores a computed verdict | F-01 | courts that reproduce the class |
| Values fabricated or defaulted | F-02, F-10 | value-provenance gates |
| Local optimisation destroys global state | F-03 | merge-only tooling; "what else does this touch" review |
| Bound accounting wrong by a constant | F-04 | stress tests at the boundary |
| Race in coordination, not kernels | F-05 | loom modelling |
| Wiring without observable effect | F-06 | observable-effect doctrine |
| Doc promises code does not enforce | F-07 | trace-to-return review |
| Untrusted input reaches unchecked code | F-08 | fuzz + typed errors |
| Report diverges from execution | F-09 | report-parity courts |
| Provenance describes the wrong time/place | F-10 | benchmark-time capture |
| Serializer loses fidelity silently | F-11 | round-trip gates |
| Accounting derived from the wrong value | F-12 | exact per-entry sizes + independent recompute |
| "Skew" that is identity | F-13 | mode-proof preflights reject wrong cardinalities |
| Documented API with no production path | F-14 | reachability doctrine + observable-effect doctrine |
| Caller contract enforced as an internal bug | F-15 | typed boundary validation |
| Exact assertions on scheduler-dependent metrics | F-16 | worker-aware, data-driven proofs |
| Feature composition unproven (cancellation × single-flight) | F-17 | three-party courts + loom; pairwise-composition audits |
| Synthetic workload presented with corpus provenance | F-18 | honest labels; trace data into the measured region |
