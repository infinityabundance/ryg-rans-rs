# Documentation Layers — the hierarchical architecture

> Every subsystem and every document in this repository sits in exactly one
> layer of the hierarchy below.  Each layer explains something different.
> **Information is never duplicated between layers**: a fact belongs to the
> lowest layer that can explain it completely, and higher layers reference
> it rather than restate it.

## The hierarchy

```text
Repository          ← README.md, AGENTS.md, llms.txt, docs/philosophy.md
     │
     ▼
Subsystem           ← docs/architecture.md, docs/papers/*, crate READMEs
     │
     ▼
Algorithm           ← docs/bitstream-contract.md, docs/papers/0001..0003,
     │                docs/glossary.md
     ▼
Module              ← module rustdoc in crates/*/src/*.rs
     │                (M.6 custodian module commentary)
     ▼
Function            ← public-fn rustdoc in crates/*/src/*.rs
     │                (M.7 function commentary)
     ▼
Section             ← section annotations inside dense functions
     │                (M.8 section commentary)
     ▼
Individual ops      ← line-level engineering annotations
                      (M.9 line commentary)
```

## What each layer explains — and only that

| Layer | Explains | Does NOT explain | Where |
|-------|----------|------------------|-------|
| **Repository** | What the project is, its frozen invariants, reading order, how to verify claims | Algorithm details, per-crate API | `README.md`, `AGENTS.md`, `llms.txt`, `docs/philosophy.md` |
| **Subsystem** | What a crate/subsystem does, does not do, its trust boundaries, evidence model, performance methodology, limitations | Line-level arithmetic | `docs/architecture.md`, `docs/papers/*`, each crate `README.md` |
| **Algorithm** | The exact stream formats, the arithmetic, why the technique exists, failure modes, alternatives rejected | Which Rust module holds which struct | `docs/bitstream-contract.md`, `docs/container-format-v1.md`, `docs/papers/0001..0003`, `docs/glossary.md` |
| **Module** | The module's purpose, history, design, invariants, failure modes, performance, verification, receipts, tests, future evolution | Per-function parameter semantics (unless load-bearing) | module rustdoc in each `src/*.rs` |
| **Function** | Purpose, inputs, outputs, invariants, safety contract, performance, failure modes, historical notes, receipts, tests | The restated algorithm (see Algorithm layer) | `///` rustdoc on public functions |
| **Section** | What a dense block does, why, the alternative and why it was rejected, interaction with neighbouring sections, evidence | The obvious meaning of individual expressions | `//` annotations inside functions |
| **Individual ops** | Why this instruction exists, why the ordering matters, what breaks if changed, which invariant it preserves, which receipt/test pins it | Syntax explanation | line-level `//` annotations |

## The never-duplicate rule

A fact is written once, at the layer where it is load-bearing, and every
other layer **links** to it.  Concretely:

* The reciprocal bias formula lives in `docs/bitstream-contract.md` (the
  Algorithm layer), is enforced in code, and is referenced — not restated —
  by the module commentary of `crates/ryg-rans-rs-core/src/lib.rs` and by
  paper 0001.
* The completeness invariant lives at the code site where it is enforced
  (function commentary on `check_completeness`), is summarised in the
  parallel crate README (Subsystem layer), and is traced to its court
  receipt in the evidence index.  No layer restates the formula; each layer
  points at the next.
* Performance numbers live in the sealed evidence index and the papers
  (Subsystem/Algorithm layers); crate READMEs cite the run ID instead of
  repeating numbers that would go stale.

## Subsystem-to-layer map

| Subsystem | Repository layer docs | Algorithm layer docs | Module commentary |
|-----------|------------------------|----------------------|-------------------|
| `ryg-rans-rs-core` (byte/R64/word/alias rANS) | crate README | `docs/bitstream-contract.md`, papers 0001, 0002 | `crates/ryg-rans-rs-core/src/lib.rs` |
| `ryg-rans-rs-simd` (SSE4.1/AVX2/AVX-512) | crate README | paper 0003, `docs/unsafe-ledger.md` | `crates/ryg-rans-rs-simd/src/lib.rs` |
| `ryg-rans-rs-parallel` (executor/reorder/cache/scratch) | crate README | paper 0004 | `crates/ryg-rans-rs-parallel/src/*.rs` |
| `ryg-rans-rs-cli` (container + commands) | crate README, root README | `docs/container-format-v1.md` | `crates/ryg-rans-rs-cli/src/*.rs` |
| `ryg-rans-rs-bench` (Criterion + courts) | crate README | paper 0005 | bench crate modules |
| `ryg-rans-rs-oracle` (forensic courts) | crate README | paper 0006, `docs/oracle-method.md` | oracle crate modules |
| `ryg-rans-rs-casefile` (evidence schema) | crate README | paper 0006 | casefile crate modules |

## Cross-cutting layers

* **Evidence chain** (paper 0006, `docs/residual-doctrine.md`): spans every
  subsystem; the evidence model is explained once and referenced from every
  receipt-carrying module.
* **Proof philosophy** (paper 0007, `docs/unsafe-ledger.md`): explains how
  much confidence each verification mechanism provides, and where it stops.
* **LLM-assisted engineering** (paper 0008, `docs/llm/`): explains how this
  repository was built with and without machine assistance, as a reference
  methodology.
* **History** (`docs/history/`): the chronological record; referenced by
  ADRs and module commentary, never duplicated into them.
* **Decisions** (`docs/adr/`): one ADR per significant decision; module
  commentary cites ADR numbers instead of re-deriving the reasoning.
* **Diagrams** (`docs/diagrams/`): architecture-level pictures; referenced
  by the papers and crate READMEs.
* **Educational** (root README reading orders, `docs/education.md`): the
  learning paths; links into the layers above rather than restating them.
