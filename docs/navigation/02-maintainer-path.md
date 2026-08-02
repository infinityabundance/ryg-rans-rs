# 02 — Maintainer Path

**Purpose:** become the custodian: able to review, modify, and re-seal any
part of the repository without breaking an invariant.

**Prerequisites:** `01-first-week.md`.

**Required papers:** 0004, 0005, 0006, 0007, 0008.

**Required ADRs:** all sixteen.

**Required source modules:** every module in the parallel crate; the
ledgered functions in the simd crate; the CLI container reader/writer.

**Recommended reading order:**
1. `AGENTS.md` — the ground truth; internalise the frozen invariants.
2. `docs/residual-doctrine.md` — how defects are handled.
3. `docs/papers/0006-evidence.md` — the seal chain.
4. `docs/history/index.md` — the invariant timeline.
5. `docs/education.md` — the maintainer notes ("what not to simplify").
6. The source modules, with the commentary-navigation guide
   (`docs/navigation/commentary.md`).
7. `docs/contributing/` — review and evidence-preservation procedures.

**Expected understanding:** the full change lifecycle
(claim → fix → test → court → receipt → seal → release); which changes
invalidate evidence; how to add a backend, a config field, a court, or a
documentation artifact without degrading the architecture.

**Estimated reading time:** 20–30 hours.

**Exercises:**
1. Simulate a covered-source change: identify every evidence artifact that
   would be invalidated and the regeneration sequence.
2. Review a past fix (e.g. the L8-REOPEN model-cache fix) against the
   review checklist in `docs/llm/index.md`.
3. Run the full seal and explain what each gate protects.

**Common misconceptions:**
- "Evidence regeneration is optional for doc changes." It is only optional
  for *allowed* paths (docs/, READMEs, xtask/, …); covered source always
  invalidates.
- "Version bumps are harmless." They change Cargo.lock, which the
  run-manifest binding checks.

**Related evidence:** the gap ledger; the docker-matrix stamps; the
performance run manifests.

**Future reading:** any role guide; `docs/navigation/reading-paths.md`.
