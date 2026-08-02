# Engineering History — the chronological record

> *Layer: cross-cutting.  This directory preserves how the repository got
> here: every phase, every important correction, every audit, every
> redesign, every invariant discovered, every evidence improvement, every
> benchmark correction, every architectural decision.  The ADRs in
> `docs/adr/` record individual decisions; this record is the timeline.
> Nothing here is deleted when the repository changes — it is appended.*

## Phase 0 — The reconstruction (Phases A–G)

The core crates were built as a native Rust reconstruction of Fabian
Giesen's public-domain `ryg_rans`.  The four codec surfaces (byte rANS,
R64 rANS, word rANS, alias) were implemented with both division and
reciprocal encode paths, two-state interleaving, and step-only decoder
operations, mirroring `rans_byte.h`, `rans64.h`, `rans_word_sse41.h`, and
`main_alias.cpp`.  The SIMD decode kernels (SSE4.1 8-way) and the AVX-512
kernels (8-way, 16-way) were added, each with scalar references.

**Invariant discovered (the founding one):** the stream format is defined
by the upstream bytes, not by documentation.  This became the oracle
doctrine.

## Phase H — Optimization backends

Uniform256 table-free decode (the model with every frequency exactly 16 at
scale 12 reduces to `sym = slot >> 4`), Batch4, and the 2×8-on-16 pattern
were added as distinct executable plans.  The distinction between a
"codec" (stream format) and a "backend" (execution engine) was formalised
— the vocabulary the whole project now uses (`docs/glossary.md`).

## Phase I — The parallel block engine

The deterministic parallel engine was designed: block plans independent of
thread count, a reorder buffer for ordered commit, cancellation, worker
scratch, a model cache, and exact backend planning.  Two architectural
facts were discovered the hard way:

* **Reorder-buffer bound bug**: the reorder bound was `effective_queue`
  but needed `effective_queue + workers`, or a slow early block could
  stall the pipeline (L.17-B).
* **Missed wakeup in the channel layer**: a sender-count race outside the
  mutex could leave a blocked producer sleeping forever (L.16-C, caught by
  the loom model).

## Phase J — AVX2 portability tier

AVX2 manual-gather, hardware-gather, 2×8-on-16, and Uniform256 kernels were
added, closing the gap between the SSE4.1 and AVX-512 tiers.  The manual
vs hardware gather distinction was kept as two backends because gather
microarchitectural behaviour varies by CPU generation.

## Phase K — Performance evidence (and its failure)

The first performance-sealing pipeline was built: a Criterion suite, an
exporter, receipts, and a seal.  It produced 831 records that were
**structurally present and semantically empty**: sample counts fabricated
as 1, hardcoded `verification_passed`, empty hashes, zero throughput for
798 records, truncated archive paths, a tautological commit binding, and a
seal that printed "verified" after skipping the check.  Phase L exists
because of this failure; the run is retained as superseded
(`evidence/performance/superseded/phase-k-20260731-004044/`).

**Lessons that became invariants:** no fabricated defaults, no hardcoded
verdicts, no empty command logs, no truncated paths, no tautological
binding, no "verified" after skipping, and never delete superseded
evidence.

## Phase L — The adversarial hardening

L.0    Baseline freeze: commit `7bbf4a25`, metadata and (later) full
       command outputs archived under `evidence/phase-l/baseline/`.
L.1    The Phase K evidence defects were quarantined (residuals
       L1-A..L1-S): the exporter was rewritten to read Criterion's
       canonical metadata, real sample counts, real byte counts, real
       thread counts, real backend identities; the run wrapper
       (`benchmark-run`) binds the run to the source at run time;
       preflight records join timing with verification; receipts gained
       dual hashes (file + canonical); typed verdicts replaced free-form
       strings; the canonical top-level performance index was created;
       the main seal integrated performance evidence.
L.2    The decoded-hash integrity bug: the verifier computed
       `decoded_hash_ok` but the aggregate failure condition ignored it —
       a block with an intact payload hash and a corrupted model decoded
       to wrong output and *passed*.  Introduced `IntegrityPolicy`
       (Strict / AllowLegacyUnsetDecodedHash), `HashVerification`
       (Match / Mismatch / Unset / NotComputed), and typed
       `DecodedHashMissing` / `DecodedHashMismatch` errors.  The
       dedicated court proves model corruption cannot pass merely because
       the payload hash is intact.
L.3    Cancellation completeness: public `_with_cancel` APIs, an
       `ExecutorReport` tracking declared/submitted/started/completed/
       cancelled/skipped/returned, `Cancelled { completed, expected }`,
       and — after a later audit reopened the residual — the completeness
       invariant enforced at every public-API boundary via
       `error::check_completeness` (a doc comment had promised what the
       final return paths did not enforce).  The CLI gained SIGINT/
       SIGTERM/timeout cancellation with exit code 11.
L.4    The executor was rebuilt as a genuinely bounded live pipeline:
       producer thread + bounded job channel + bounded result channel +
       live reorder commit, with input/output budgets enforced against
       live stages and streaming sink APIs.
L.5    ReorderBuffer's `insert → Option<T>` + `drain_ready()` protocol was
       replaced by `insert → Result<Vec<T>>` atomic commit batches.
L.6    Every `ParallelConfig` field was wired or removed: sequential
       threshold fallback, affinity (Linux `sched_setaffinity` +
       `sched_getaffinity` verification), SMT topology, `disable_simd`,
       stack-size validation; `disable_inner_batching` and the
       single-option `error_policy` were removed as configuration theatre.
L.7    WorkerScratch was wired into production execution (one exclusive
       scratch per worker, reset between tasks, bounded retention).
L.8    ModelCache was wired into the decode path — and, after a later
       audit, made to cache the expensive packed word table (Arc-shared)
       so repeated models genuinely avoid per-block table construction.
L.9    Explicit backend semantics were made exact: a requested backend
       executes exactly or returns a typed error; the format-compatibility
       matrix (8-way ↔ codec 7, 16-way ↔ codec 8, Uniform256 ↔ validated
       model, batch ↔ coordinator context) is enforced at plan time.
L.10   The SSE helpers were quarantined: local `#[target_feature]` on
       every helper, `# Safety` sections, a bidirectional unsafe-ledger
       test, and disassembly courts.
L.11   The adversarial algorithmic audit: division/reciprocal equivalence,
       all scale bits, renormalization boundaries, truncation at every
       byte/word, packed-table bounds, Vose alias construction, SIMD
       kernel tails and masks.  Found and fixed the SSE4.1 report-parity
       defect and malformed-input panics.
L.12   Code commentary for disproved suspicions (header `try_into().unwrap()`
       invariants; executor mutex-poisoning reasoning).
L.13   Public-API audit: dead `schedule.rs`/`report.rs` removed,
       `estimate_memory` overflow fixed, `docs/public-api/` inventory.
L.14   Comparative benchmark court against `hypersonic-rANS` and the FFI
       `rans` binding, with methodological mismatches recorded as
       residuals.
L.15   Documentation overhaul: overclaim language removed, `-.o` removed,
       `.gitignore` hardened, crate READMEs rewritten, glossary created.
L.16   Testing, fuzzing, formal verification, and concurrency courts:
       proptests, nine fuzz targets (three target bugs found and fixed),
       loom-instrumented executor courts, Kani proofs (21 verified; the
       two intractable instances recorded as an accepted limitation),
       sanitizers/Miri.
L.17   Performance analysis: queue-depth sweep exposed the reorder-bound
       bug; component isolation showed the parallel overhead is dominated
       by dual SHA-256 per block.
L.18   Performance evidence rebuilt and re-sealed: the run wrapper,
       exporter, archive (tar crate with PAX long names), receipts,
       indexes, and the ten-surface matrix.  Re-sealed at `c43d616`
       (run `phase-l-20260802b`, 800 cases × 100 samples).
L.19   Fourteen Phase L behavioural courts with manifests and receipts;
       the oracle promote script was fixed (it had destroyed unrelated
       evidence by renaming the whole tree — L19-B).
L.20   Seal-gate hardening: `cargo xtask seal` became the single
       authoritative final gate (40 gates, from dirty-tree to publication
       dry-run), never printing success for a skipped check.
L.21   The disciplined commit series (baseline/quarantine through
       evidence regeneration).
L.22   Versioning and publication: semver-checks showed the CLI's
       `AppError::Cancelled` variant was breaking → 0.3.0 (pre-1.0 minor);
       all seven crates published from the exact sealed commit; tag
       `v0.3.0` at the sealed evidence commit.

## Post-L — the reopened residuals

After the L.18 re-seal, a further adversarial audit reopened two
"resolved" residuals and found them genuinely unfixed:

* **L8 (ModelCache)**: the cache was consulted but stored only the cheap
  1 KiB frequency parse; the expensive 16 KiB packed table was rebuilt per
  block — no throughput gain.  Fixed by caching the `PackedWordTable`
  (Arc-shared) and borrowing it in `execute_decode_plan`.
* **L3 (cancellation completeness)**: the `_with_cancel` doc comments
  promised guarantees the final return paths delegated to the executor
  instead of enforcing.  Fixed by `error::check_completeness` at every
  public boundary.

The lesson, recorded in paper 0008: "resolved" means the observable
effect is proven, not that the wiring exists.

## Phase M — Custodian documentation

The repository was transformed into the definitive implementation
reference: philosophy, layered architecture, eight design papers, this
history, ADRs, diagrams, module/function/section/line commentary, the
educational layer, and the documentation seal — the knowledge-preservation
phase that ensures no engineering lesson above is ever lost.

---

## The invariant timeline (what was discovered, and when)

| When | Invariant |
|------|-----------|
| Phases A–G | The stream format is the upstream bytes |
| Phase I | Output order, error identity, and memory bound must be schedule-independent |
| Phase K (failure) | Evidence must come from execution, not from defaults |
| L.2 | A decoded-output hash is required; payload hashing alone cannot catch model corruption |
| L.3 | Cancellation can never return a short `Ok`; the guarantee belongs to the API boundary |
| L.4 | Peak memory must be bounded by configuration, not by workload |
| L.6 | Every public configuration field must have an observable effect |
| L.9 | A requested backend executes exactly or returns a typed error |
| L.19 | Evidence is never deleted; it is superseded with a reason |
| L.20 | The seal is the single authoritative final gate and never lies about skipping |

---

# N.7 — Timelines

## Evolution timeline (what shipped, when)

| When | What | Where |
|------|------|-------|
| Phases A–G | the four codec surfaces + SSE4.1/AVX-512 kernels, scalar references | core, simd |
| Phase H | Uniform256, Batch4, 2×8-on-16 backends | simd, parallel |
| Phase I | the parallel block engine | parallel |
| Phase J | the AVX2 tier | simd |
| Phase K | the first (broken) performance pipeline | xtask, bench |
| Phase L | the adversarial hardening (L.0–L.22) | all crates, xtask, evidence |
| Post-L | the reopened-residual fixes (ModelCache, completeness) | parallel |
| Phase M | custodian documentation (M.0–M.22) | docs, source commentary |
| Phase N | navigation, knowledge architecture, publications (N.0–N.22) | docs/navigation, atlas, articles, failures, story |

## Decision timeline (what was decided, when)

| When | Decision | ADR |
|------|----------|-----|
| Phases A–G | reconstruct the pinned upstream bytes | 0001 |
| Phases A–G | reciprocal fast path with exact bias | 0002 |
| Phase G | word scale pinned at 12, packed table | 0003 |
| Phase I | bounded live executor | 0004 |
| Phase I | canonical lowest-index error | 0005 |
| L.2 | strict integrity default | 0006 |
| L.3 | completeness at the API boundary | 0007 |
| L.9 | exact backend semantics | 0008 |
| L.8 + re-open | cache the expensive artifact | 0009 |
| L.18 | benchmark-time capture | 0010 |
| L.10 | unsafe quarantine + ledger | 0011 |
| L.22 | 0.3.0 version decision | 0012 |
| L.6 | configuration discipline | 0013 |
| L.5 | reorder atomic commit | 0014 |
| L.7 | per-worker scratch | 0015 |

## Receipt timeline (what was sealed, when)

| When | Receipt set | Count |
|------|-------------|-------|
| Phases A–F | oracle courts: byte (44) + r64 (44) + word (16) + alias (16) | 120 |
| Phases G–H | SIMD-family oracle courts: SSE4.1 `SIMD.INTERLEAVED8` (8) + `AVX512VL` (8) + `AVX512` (8) | +24 |
| Phase L.19 | the fourteen Phase L courts | +14 |
| Phase L.18 | the ten performance receipts (re-sealed at each implementation commit) | 10 |
| current | total behavioural / performance | 158 / 10 |

The 144 oracle/upstream-parity courts (120 + 24) plus the 14 Phase L
courts make 158 behavioural receipts — the same breakdown the README
evidence table reports, so the two documents cannot drift.

The behavioural total is generated from `evidence/index.json`, never
hardcoded.

## Seal evolution

| When | The seal gained |
|------|-----------------|
| Pre-L | a minimal `check` command |
| L.1 | performance-evidence validation, run binding, no-false-verified |
| L.20 | the 40-gate authoritative seal (dirty-tree → publication dry-run) |
| M.19 | the custodian documentation inventory gate |
| N.14/N.21 | the navigation/atlas/article/knowledge-graph completeness gates |
