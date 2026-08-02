# Contributing (N.17)

> How to contribute to `ryg-rans-rs` without degrading the architecture:
> understanding, reviewing, extending, and preserving evidence.  The
> ground rules are in `AGENTS.md`; this directory is the how-to.

## How to understand the repository

1. Start at the README portal → `docs/navigation/00-first-day.md`.
2. Pick the role guide matching your change
   (`docs/navigation/04-simd-engineer.md` for SIMD, `05-parallel-engineer.md`
   for the engine, `07-evidence-engineer.md` for evidence, …).
3. Read the module commentary of every module you touch (M.6 section set)
   and the related ADRs before writing code.

## How to review changes

Apply the five-step claim check to every change, regardless of author:

1. Find the claim (doc, comment, commit message).
2. Find the producing code path — trace it yourself.
3. Find the test that fails if the behaviour changes.
4. Find the receipt in `evidence/`.
5. Run the seal gate.

Plus the `docs/llm/index.md` review checklists (observable effect, no
silent fallback, no inert config, no doc-vs-code gap, no panics from
untrusted input, feature-matrix compile, evidence impact).

## How to preserve evidence

* **Never hand-edit evidence.**  `evidence/**` is generated; hand-edits
  break the hashes and the seal rejects them.
* **Never delete evidence.**  Supersede with a reason
  (`SUPERSEDED.md`/`INVALIDATED.md`) and a residual.
* **Covered source changes invalidate evidence.**  `crates/*/src` changes
  require regeneration (benchmark, oracle, courts, docker, seal) because
  the benchmark binaries are built from those crates.
* **The source-freshness gate is the authority on what does *not* need
  regeneration.**  `xtask/src/main.rs` (the `source freshness` gate, list
  `allowed_prefixes`) allowlists `docs/`, `docs-src/`, `xtask/`, `docker/`,
  `.cargo/`, `Cargo.toml`/`Cargo.lock`, crate `README.md`s, and
  `evidence/` itself: a change confined to those paths re-seals without a
  new benchmark run, because the sealed binaries are bit-identical.  The
  dirty-tree gate similarly permits *uncommitted* changes only in
  `docs/`, `evidence/`, `README.md`, `xtask/README.md`, `Cargo.lock`, and
  `.gitignore`; everything else must be committed before sealing.
* **A version bump changes Cargo.lock** — which the run-manifest binding
  checks — so evidence regenerates at the release version.

## Documentation performance (N.18)

Large corpora only remain navigable if duplication is structural, not
accidental.  The rules that keep this corpus scalable:

* **One fact, one home.**  Every fact lives at the lowest layer that can
  explain it completely (`docs/layers.md`); every other layer *references*
  it with a link.  Restating a formula in a second document creates a
  second authority that will drift.
* **Reference over restatement.**  A crate README cites a paper and an
  ADR; a paper cites the receipt index; the index cites the seal.  If you
  find yourself re-explaining something that has a home, link to the home
  instead.
* **Entry points are the budget.**  Readers enter at the README portal and
  the navigation guides; every document is reachable from an index
  (`docs/search/`, `docs/navigation/knowledge-graph.md`) and from its
  related-artifact cross-links (N.12).  A document that nothing links to is
  a dead document.
* **Health is measured, not assumed.**  The seal gates measure corpus
  health: the link checker fails on broken relative markdown links; the
  navigation
  gate fails on missing guides, maps, atlas chapters, articles, or portal
  markers; the inventory (`docs/navigation/inventory.md`) is the index of
  everything documented.  Adding an artifact without registering it in the
  inventory and the gates is a documentation defect.
* **Length is not duplication.**  Long custodian commentary is intentional
  (`docs/philosophy.md`); what is forbidden is *repetition* — the same fact
  in two places where one link would do.

## How to extend documentation

* Follow `docs/layers.md`: put the fact at the lowest layer that can
  explain it completely; reference, don't restate, at the others.
* Add every new document to `docs/navigation/inventory.md` and the search
  indexes; link it from the knowledge graph.
* Every document needs the N.12 cross-reference set: related papers, ADRs,
  code, receipts, benchmarks, history, diagrams.
* The seal's navigation gates verify the required artifacts exist — a new
  required artifact must be registered in the gate list.

## How to maintain custodian commentary

* New or modified code carries the five levels (module → function →
  section → line) per `docs/navigation/commentary.md`.
* Every annotation answers: why, which invariant, what breaks if changed,
  which receipt pins it, which test detects regression.
* Historical failure commentary cites the `docs/failures/` entry.

## How to add a new SIMD backend

1. Implement the kernel with its own `#[target_feature]` and a complete
   `# Safety` section.
2. Add the scalar reference (or prove the existing reference covers it).
3. Register it in `unsafe-ledger.toml` (the bidirectional test fails
   otherwise).
4. Add it to the planner's `DecodePlan`/`BackendId`, the compatibility
   matrix, and the backend-identity mapping.
5. Add differential + report-parity tests; verify the disassembly court
   recognises the new instructions.
6. Add the bench case and the preflight path.
7. Follow `docs/navigation/04-simd-engineer.md` end to end.

## How to extend the oracle

1. Build the adapter (`cd oracle/adapter && make`).
2. Add the court generator (see `docs/oracle-method.md`).
3. Generate, then **merge** (never replace) into `evidence/` — the
   promote-merge rule (F-03).
4. Record the receipts in the index; the seal verifies them.

## How to preserve invariants

* The frozen invariants in `AGENTS.md` are not negotiable.
* An invariant change is a format/API decision: ADR + full evidence
  regeneration + version decision.
* When you discover a defect, write the residual (severity, reproduction,
  expected/actual, fix, test requirement, evidence requirement) before the
  fix — evidence-first.
