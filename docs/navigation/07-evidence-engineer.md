# 07 — Evidence Engineer

**Purpose:** own the evidence pipeline: receipts, manifests, indexes,
seals, residuals, and regeneration.

**Prerequisites:** `01-first-week.md`.

**Required papers:** 0005, 0006, 0007.

**Required ADRs:** 0010.

**Required source modules:** `xtask/src/main.rs` (all gates, the run
wrapper, the exporters); `crates/ryg-rans-rs-casefile/` (the schema);
`crates/ryg-rans-rs-bench/src/common/preflight.rs`.

**Recommended reading order:**
1. `docs/papers/0006-evidence.md` — the whole chain.
2. `docs/residual-doctrine.md` — defect accounting.
3. `docs/papers/0005-performance-methodology.md` — the run wrapper.
4. `xtask/src/main.rs` gate by gate.
5. `evidence/phase-l/gap-ledger.md` — the standing record.
6. `evidence/performance/` — inspect a complete run.

**Expected understanding:** the behavioural and performance chains; the
dual-hash receipt model; the run-manifest binding; when regeneration is
required; how the seal never lies about skipping; how to regenerate and
re-seal.

**Estimated reading time:** 12–18 hours.

**Exercises:**
1. Regenerate the top-level performance index from a run index by script
   (never by hand).
2. Explain why a version bump invalidates performance evidence.
3. Trace the "no verified after skipping" rule to its gate.

**Common misconceptions:**
- "The index SHA is the receipt hash." There are two fields: the file
  SHA-256 and the canonical self-hash (L1-L).
- "A superseded run can be deleted." Never — it is marked and retained.

**Related evidence:** everything under `evidence/`; the docker matrix
stamps.

**Future reading:** `02-maintainer-path.md`, `10-security-review.md`.
