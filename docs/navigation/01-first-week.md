# 01 — First Week

**Purpose:** become a competent reader of the codebase: able to trace a
claim from a README through code, tests, courts, and receipts.

**Prerequisites:** `00-first-day.md`.

**Required papers:** 0001 (all), 0004, 0006.

**Required ADRs:** 0001, 0002, 0004, 0006, 0007.

**Required source modules:**
- `crates/ryg-rans-rs-core/src/lib.rs` (surfaces, constants)
- `crates/ryg-rans-rs-parallel/src/{executor,reorder,decode}.rs`
- `crates/ryg-rans-rs-cli/src/ops/{encode,decode,verify}.rs`

**Recommended reading order:**
1. `docs/architecture.md` — the subsystem map.
2. `docs/papers/0001-rans-design.md` — the full arithmetic story.
3. `docs/papers/0004-parallel-engine.md` — the engine.
4. `docs/papers/0006-evidence.md` — the evidence chain.
5. `docs/bitstream-contract.md` — the exact formats.
6. The three source modules above, reading their module docs first.
7. `docs/history/index.md` — what broke before and how.

**Expected understanding:** trace one claim end-to-end
(claim → code → test → court → receipt → seal); explain the reciprocal
path, the reorder buffer, and the completeness invariant.

**Estimated reading time:** 6–10 hours.

**Exercises:**
1. Pick a README claim and verify it through the five-step procedure in
   `AGENTS.md`.
2. Explain why `execute_decode_plan` borrows a cached table instead of
   building one.
3. Explain what `check_completeness` protects and why it exists at the API
   boundary (ADR-0007).

**Common misconceptions:**
- "The oracle proves correctness." It proves byte-exact parity on a finite
  corpus (paper 0007 §5).
- "Cancellation aborts work." It is cooperative; in-flight blocks finish.

**Related evidence:** the Phase L court receipts; the performance run
indexes.

**Future reading:** `02-maintainer-path.md` or a role guide (03–10).
