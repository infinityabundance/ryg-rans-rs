# 09 — LLM Engineer

**Purpose:** use LLM coding agents on this repository responsibly: the
prompt philosophy, the review checklists, and the failure catalogue.

**Prerequisites:** `00-first-day.md`.

**Required papers:** 0008.

**Required ADRs:** none (the methodology is paper- and checklist-based).

**Required source modules:** none directly; the review checklists reference
all crates.

**Recommended reading order:**
1. `docs/papers/0008-llm-assisted-engineering.md` — the methodology.
2. `docs/llm/index.md` — the operational checklists and hallucination
   catalogue.
3. `docs/history/index.md` — the concrete failure examples.
4. `AGENTS.md` — the ground-truth rules agents must follow.

**Expected understanding:** the six observed failure patterns (plausible
structure with missing truth, doc-vs-code gaps, tautological checks,
"verified after skipping", local-optimisation-global-destruction,
optimistic error coverage); the evidence-first workflow; when to trust an
agent and when a human must intervene.

**Estimated reading time:** 3–6 hours.

**Exercises:**
1. Given an agent's claim "the cache is wired", list the checks that would
   falsify it (answer: observable effect — allocation or throughput
   evidence).
2. Apply the review checklist to a past commit.

**Common misconceptions:**
- "The agent's summary is a record." It is a hypothesis; trace it.
- "Agent review passes prove correctness." They are search results with
  defined coverage.

**Related evidence:** the gap ledger's reopened residuals (L8-REOPEN,
L3-REOPEN) — both are agent-adjacent failure examples.

**Future reading:** `02-maintainer-path.md`.
