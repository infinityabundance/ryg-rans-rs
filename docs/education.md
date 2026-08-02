# Education — reading orders and maintainer notes

> *Layer: cross-cutting.  This file is the structured learning path into the
> repository.  Every reading order links into the layered documentation
> (`docs/layers.md`); nothing here restates the layers.*

## Beginner reading order (new to rANS and to this repo)

1. Root `README.md` — what the project is, its status table.
2. `docs/philosophy.md` — why the documentation is written this way.
3. `docs/glossary.md` — the exact terms (block, stream, surface, backend,
   plan, receipt, manifest, residual, seal, preflight, …).
4. `docs/papers/0001-rans-design.md` §1–§5 — the arithmetic, without SIMD.
5. `docs/architecture.md` — the subsystem map.
6. Try the CLI: `cargo run -- encode/decode/verify` on a test file.

## Intermediate reading order (using the library)

1. `docs/layers.md` — where each fact lives, and the never-duplicate rule.
2. `docs/bitstream-contract.md` — the exact stream formats.
3. `docs/papers/0002-word-rans.md` — the table-based word coder.
4. `docs/container-format-v1.md` — the RYGRANS v1 container.
5. The crate README for the crate you are using (facade first, then
   parallel/cli).
6. `docs/papers/0004-parallel-engine.md` — the engine's invariants.

## Advanced implementation reading order (modifying the code)

1. Everything in the intermediate order, then:
2. `docs/papers/0001-rans-design.md` §6–§10 — interleaving, performance,
   failure modes.
3. `docs/papers/0007-proof-philosophy.md` — what each verification
   mechanism proves and where it stops.
4. `docs/unsafe-ledger.md` — the unsafe surface and its contracts.
5. The module commentary in the crate you are changing (M.6 standard:
   Purpose, History, Design, Alternatives, Invariants, Failure modes,
   Performance, Verification, Receipts, Tests, Future evolution,
   References).
6. `docs/adr/` — the decision records that explain why the code is shaped
   this way.
7. `docs/history/` — what broke before, and how it was fixed.

## SIMD specialization reading order

1. `docs/papers/0002-word-rans.md` — the packed table.
2. `docs/papers/0003-simd.md` — the kernels, dispatch, microarchitecture.
3. `docs/unsafe-ledger.md` + `crates/ryg-rans-rs-simd/unsafe-ledger.toml`
   — the safety contracts.
4. The disassembly courts and the `# Safety` sections in the kernel source.
5. `docs/bitstream-contract.md` — the stream formats the kernels must
   produce and consume.

## Evidence reading order (auditing or regenerating evidence)

1. `docs/papers/0006-evidence.md` — the whole chain.
2. `docs/papers/0005-performance-methodology.md` — how numbers are made.
3. `docs/oracle-method.md` — how the oracle courts work.
4. `docs/residual-doctrine.md` — how defects are recorded and resolved.
5. `evidence/phase-l/gap-ledger.md` — the standing record.
6. Run `cargo xtask seal` and read the gate list.

## Parallel reading order

1. `docs/papers/0004-parallel-engine.md`.
2. `docs/adr/0004`, `0005`, `0007`, `0009`, `0013`, `0014`, `0015`.
3. The parallel crate's module commentary (executor, reorder, cache,
   scratch, cancellation).
4. The loom courts and the cancellation/boundedness stress tests.
5. `docs/history/` (Phase I, L.3, L.4, L.6, L.7, L.8 entries).

## LLM engineering reading order

1. `docs/papers/0008-llm-assisted-engineering.md` — the methodology.
2. `docs/llm/index.md` — the operational checklists and hallucination
   catalogue.
3. `docs/history/` — the concrete failure examples.
4. `AGENTS.md` — the ground-truth rules the assistant is bound to.

---

# Decade-later handover review (M.20)

The questions a future custodian must be able to answer, and where this
repository answers them.

| Question | Answer lives in | Verified by |
|----------|-----------------|-------------|
| Could someone maintain this without me? | The papers, ADRs, history, module commentary, and the maintainer notes in this file | The documentation-inventory seal gate; the workspace test suite |
| Could someone understand every design decision? | `docs/adr/` (15 decisions, each with context, alternatives, rejected alternatives, tradeoffs, evidence, future implications) | The ADR files exist (inventory gate); the decisions are cited from the code |
| Could someone reproduce every benchmark? | `docs/papers/0005-performance-methodology.md` + `xtask benchmark-run` + the run directory artifacts (`run-manifest.json`, `commands.log`, `host.json`) | The performance-evidence seal gates (run-manifest binding, Cargo.lock SHA) |
| Could someone regenerate every receipt? | `docs/papers/0006-evidence.md` + `docs/oracle-method.md` + the oracle/courts commands in `AGENTS.md` | The behavioural-receipt seal gates (file + canonical hashes) |
| Could someone understand every invariant? | The frozen invariants in `AGENTS.md`, the invariant timeline in `docs/history/index.md`, the `# Safety` sections, the module commentary | The seal's freshness/ledger/unsafe gates |
| Could someone know what was tried and rejected? | The ADR "Rejected alternatives" sections, the papers' alternatives sections, the history | Presence via the inventory gate; value via the docs |
| Could someone know what is measured and what is not? | Every paper's honesty rules (esp. `0005` §6–§8) and the maintainer notes | The performance-evidence gates |

If any answer were "no", the fix is to write more, not less — the
knowledge-preservation contract of Phase M.

---

# Future maintainer notes (M.15)

Written to the engineer who inherits this repository.  Each note says what
will be tempting to "simplify", why it must not be simplified, where bugs
historically appeared, and which invariants are subtle.

## Core crate (`ryg-rans-rs-core`)

* **Tempting simplification**: "The division path is dead code — the
  reciprocal path is proven equal, remove it."  Do not.  The division path
  is the reference the Kani proofs, the oracle, and the `compare
  arithmetic` court run against.  Removing it removes the ability to
  detect reciprocal drift.  The cost is a few hundred lines that are
  themselves heavily tested.
* **Tempting simplification**: "`freq == 1` is a special case; the general
  path should handle it."  It cannot: the reciprocal of 1 needs a shift
  budget the byte variant does not have.  The special case is load-bearing
  (see ADR-0002).
* **Subtle invariant**: the reverse flush order and the
  initialization-order-differs-from-flush-order rule are part of the
  bitstream contract.  A "cleanup" that makes init order match flush order
  breaks every stream.
* **Bugs historically appeared**: reciprocal bias drift, renormalization
  boundary changes, truncation over-reads, model sum drift, zero-frequency
  symbols.  The fuzz targets and the oracle court exist because of them.

## SIMD crate (`ryg-rans-rs-simd`)

* **Tempting simplification**: "Give the SSE helpers the caller's target
  features again — the attributes are redundant."  They are not redundant;
  they are the safety contract (ADR-0011).  A helper that relies on the
  caller's context is UB the moment the caller changes.
* **Tempting simplification**: "Hardware gather is strictly better; drop
  the manual variants."  Gather behaviour varies by microarchitecture; the
  manual variants exist for hosts where `vpgatherdd` is slow.  The
  planner/benchmark chooses.
* **Subtle invariant**: inactive lanes must never read input.  Any masked
  tail implementation that lets a lane past the block end perform its
  renormalization load is an over-read.
* **Bugs historically appeared**: report divergence between kernels,
  compiler scalarization (caught by disassembly courts), masked over-reads
  (caught by truncation fuzzing).

## Parallel crate (`ryg-rans-rs-parallel`)

* **Tempting simplification**: "The executor's completeness check is
  redundant — the worker accounting already guarantees it."  The guarantee
  belongs to the public API; the boundary check exists because a doc
  comment once promised it without the code enforcing it (ADR-0007).
* **Tempting simplification**: "Materialise all results and sort at the
  end — simpler than the live reorder."  That is the pre-L.4 architecture
  whose `max_buffered_output_bytes` was a lie.  The live pipeline is the
  boundedness.
* **Tempting simplification**: "Add a shared ScratchPool behind a mutex —
  less memory."  A lock in the per-block path serializes workers
  (ADR-0015).
* **Tempting simplification**: "Cache the whole decode plan."  A plan
  depends on runtime backend conditions; only model-derived artifacts are
  cacheable (ADR-0009).
* **Subtle invariants**: the canonical error is the lowest block index;
  worker panic > per-block error > cancellation in priority; the reorder
  bound is `effective_queue + workers` (a "fix" to `effective_queue`
  alone stalls the pipeline — this exact bug was found in L.17-B).
* **Bugs historically appeared**: the decoded-hash aggregate condition
  (L.2), the missed wakeup (L.16-C), the reorder bound (L.17-B), the
  inert model cache (L.8 re-opened), the unenforced cancellation promise
  (L.3 re-opened).

## CLI crate (`ryg-rans-rs-cli`)

* **Tempting simplification**: "Collapse the exit codes into 0/1."
  Automation depends on the documented 0–11 semantics; the binary
  propagates them verbatim.
* **Tempting simplification**: "Remove the `signals` feature; just use
  ctrlc-style handlers."  The feature gating is what keeps
  `--no-default-features` builds fully `forbid(unsafe_code)`.
* **Subtle invariant**: cancellation is block-granular and cooperative;
  a signal or timeout surfaces at the next block boundary with exit 11 —
  never mid-write, never as a silent partial success.

## Evidence pipeline (`xtask`, `evidence/`)

* **Tempting simplification**: "Hand-edit `evidence/performance/index.json`
  — the fields are obvious."  It is generated from the run index by
  script; hand-editing breaks the hashes and the seal catches it.
* **Tempting simplification**: "Delete the superseded Phase K run — it's
  wrong anyway."  Deleting history is forbidden; superseding with a reason
  is the doctrine.  The superseded run is the permanent record of why the
  pipeline was rebuilt.
* **Tempting simplification**: "Print 'verified' when the check was
  skipped — it's equivalent."  It is not equivalent, and the seal is
  designed to fail rather than lie.
* **Subtle invariant**: the run-manifest binds the run to commit + tree +
  Cargo.lock SHA; a version bump changes the lock and requires a full
  regeneration.  Do not "fix" the binding to be looser.
