# Documentation Inventory (N.0)

> The complete, classified inventory of every documentation artifact in the
> repository.  Nothing is undocumented; every artifact below states its
> purpose, audience, prerequisites, and cross-links.  Generated and
> maintained by the documentation-health conventions in
> `docs/contributing/how-to-extend-documentation.md`; verified by the
> documentation seal gates (N.14/N.21).

## How to read this inventory

Each entry lists: purpose · audience · prerequisites · related ADRs ·
related papers · related code · related receipts · related diagrams.
"Related" means *linked from the artifact itself* (N.12 requires no
isolated documents) and *linkable from the other side* (the index below).

## Repository-level artifacts

| Artifact | Purpose | Audience | Prereqs |
|----------|---------|----------|---------|
| `README.md` | Portal: identity, status, evidence table, entry points by intent (N.3/N.16) | Everyone | none |
| `AGENTS.md` | Ground truth for AI agents: invariants, commands, evidence rules | Agents, maintainers | README |
| `llms.txt` | Machine-readable corpus index for LLM tooling | LLMs, tooling | none |
| `docs/philosophy.md` | The documentation constitution (M.0) | Custodians | none |
| `docs/layers.md` | The layered documentation architecture, never-duplicate rule (M.1) | Custodians, contributors | philosophy |
| `docs/glossary.md` | The exact terminology (block, stream, surface, backend, …) | Everyone | none |
| `docs/references.md` | The canonical bibliography (M.16) | Researchers | none |
| `docs/education.md` | Reading orders + maintainer notes (M.14/M.15/M.20) | All readers | layers |
| `docs/navigation/inventory.md` | THIS FILE (N.0) | Custodians | layers |
| `docs/navigation/knowledge-graph.md` | The encyclopedia: cross-reference every artifact (N.4) | Everyone | inventory |
| `docs/navigation/adrs-by-topic.md` | ADR index grouped by topic (N.9) | Decision readers | adr/ |
| `docs/search/*` | Topic/algorithm/receipt/ADR/paper/diagram/glossary/module/benchmark indexes (N.15) | Everyone | index files |

## Architecture and design

| Artifact | Purpose | Audience | Prereqs |
|----------|---------|----------|---------|
| `docs/architecture.md` | Subsystem map and data flow | All engineers | glossary |
| `docs/bitstream-contract.md` | The pinned upstream stream formats | Codec engineers | glossary, papers/0001 |
| `docs/container-format-v1.md` | The RYGRANS v1 container spec | CLI/container engineers | bitstream-contract |
| `docs/atlas/*` | The architecture atlas: chaptered deep dives with diagrams (N.5) | All engineers | architecture |
| `docs/diagrams/index.md` | Ten mermaid architecture diagrams (M.5) | Everyone | architecture |
| `docs/drafts/*` | Historical working drafts (claim index, court matrix, parity, residuals, unsafe count) | Historians | none |
| `docs/research/*` | Methodology sources, upstream inventory | Researchers | references |

## Papers (M.2, extended N.6)

| Paper | Topic | Audience |
|-------|-------|----------|
| `docs/papers/0001-rans-design.md` | rANS design: arithmetic, reciprocal, renormalization, interleaving | Codec engineers |
| `docs/papers/0002-word-rans.md` | The table-based word coder | Codec engineers |
| `docs/papers/0003-simd.md` | The SIMD kernels and dispatch | SIMD engineers |
| `docs/papers/0004-parallel-engine.md` | The deterministic parallel engine | Parallel engineers |
| `docs/papers/0005-performance-methodology.md` | How the repository measures itself | Performance engineers |
| `docs/papers/0006-evidence.md` | The evidence system | Evidence engineers |
| `docs/papers/0007-proof-philosophy.md` | What each verification mechanism proves | All engineers |
| `docs/papers/0008-llm-assisted-engineering.md` | The LLM methodology | All engineers |
| `docs/articles/*` | Standalone publishable articles (N.6) | The wider engineering community |

## Decisions (M.4, indexed N.9)

| ADR | Decision | Topic group |
|-----|----------|-------------|
| `docs/adr/0001-format-contract.md` | Byte-exact upstream reconstruction | Architecture |
| `docs/adr/0002-reciprocal-fast-path.md` | Reciprocal multiply-high with exact bias | Performance |
| `docs/adr/0003-word-scale-pinned.md` | Word coder scale 12, packed table | SIMD |
| `docs/adr/0004-bounded-live-executor.md` | Bounded live executor | Parallel |
| `docs/adr/0005-canonical-error.md` | Lowest-index error selection | Parallel |
| `docs/adr/0006-strict-integrity-default.md` | Strict decoded-output integrity | Safety |
| `docs/adr/0007-cancellation-completeness-boundary.md` | Completeness at the API boundary | Parallel |
| `docs/adr/0008-exact-backend-semantics.md` | No silent backend fallback | Architecture |
| `docs/adr/0009-model-cache-expensive-artifact.md` | Cache the expensive artifact | Performance |
| `docs/adr/0010-benchmark-time-capture.md` | Evidence captured at benchmark time | Evidence |
| `docs/adr/0011-unsafe-quarantine.md` | Local target features + ledger | Safety |
| `docs/adr/0012-versioning-030.md` | 0.3.0 pre-1.0 minor | Release |
| `docs/adr/0013-configuration-discipline.md` | Every config field observable | Configuration |
| `docs/adr/0014-reorder-atomic-commit.md` | Atomic reorder commit batches | Parallel |
| `docs/adr/0015-per-worker-scratch.md` | Per-worker exclusive scratch | Parallel |

## History, failures, story (M.3, M.7, N.7/N.8/N.10)

| Artifact | Purpose | Audience |
|----------|---------|----------|
| `docs/history/index.md` | Chronological engineering record + invariant timeline | Historians, maintainers |
| `docs/story/index.md` | The engineering story: problem → evidence → correction (N.8) | Everyone |
| `docs/failures/*` | The failure encyclopedia (N.10): every important bug | Maintainers |
| `docs/gap-ledger.md` | The residual ledger (evidence/phase-l/gap-ledger.md is the canonical copy) | Maintainers |

## Evidence and verification

| Artifact | Purpose | Audience |
|----------|---------|----------|
| `docs/residual-doctrine.md` | How defects are recorded, resolved, accepted | Evidence engineers |
| `docs/oracle-method.md` | The oracle court methodology | Oracle engineers |
| `docs/unsafe-ledger.md` | The unsafe surface and its contracts | Safety reviewers |
| `docs/negative-capabilities.md` | What the project deliberately does not do | Evaluators |
| `docs/performance-method.md` | Short-form performance methodology | Performance engineers |
| `docs/performance/phase-l17-analysis.md` | The L.17 component-isolation analysis | Performance engineers |
| `docs/performance/comparative.md` | The L.14 comparative court | Performance engineers |
| `docs/public-api/README.md` | The public-API inventory guide | API reviewers |

## LLM engineering

| Artifact | Purpose | Audience |
|----------|---------|----------|
| `docs/llm/index.md` | Operational checklists, hallucination catalogue (M.12) | LLM-assisted engineers |

## Navigation (this phase)

| Artifact | Purpose |
|----------|---------|
| `docs/navigation/00-first-day.md` … `10-security-review.md` | The eleven reading guides (N.1) |
| `docs/navigation/maps/*.md` + `*.svg` | The learning maps, mermaid + SVG (N.2) |
| `docs/navigation/knowledge-graph.md` | The encyclopedia (N.4) |
| `docs/navigation/adrs-by-topic.md` | ADRs grouped by topic (N.9) |
| `docs/navigation/reading-paths.md` | Curated study paths with hour estimates (N.13) |
| `docs/navigation/commentary.md` | How to read the custodian commentary (N.11) |

## Contributing

| Artifact | Purpose |
|----------|---------|
| `docs/contributing/*` | Contributor experience: review, evidence preservation, extension guides (N.17) |

## Crate READMEs (all linked from the root README portal)

`crates/ryg-rans-rs-core/README.md`, `crates/ryg-rans-rs-simd/README.md`,
`crates/ryg-rans-rs-parallel/README.md`, `crates/ryg-rans-rs-cli/README.md`,
`crates/ryg-rans-rs/README.md`, `crates/ryg-rans-rs-bench/README.md`,
`crates/ryg-rans-rs-oracle/README.md`, `crates/ryg-rans-rs-casefile/README.md`,
`xtask/README.md`.

## Machine-checked artifacts (not prose)

| Artifact | Purpose |
|----------|---------|
| `evidence/` | Receipts, manifests, indexes, performance runs, docker matrix |
| `docs-src/models/parity.model.json` | The machine-readable evidence model |
| `docs/public-api/*.txt` | `cargo public-api` inventories |
| `crates/ryg-rans-rs-simd/unsafe-ledger.toml` | The machine-verified unsafe inventory |
| `docs/navigation/maps/*.svg` | The hand-maintained SVG learning maps |
