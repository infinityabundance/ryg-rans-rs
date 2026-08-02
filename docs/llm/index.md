# LLM Engineering — operational record

> *Layer: cross-cutting.  Companion: `docs/papers/0008-llm-assisted-engineering.md`
> (the methodology), `docs/history/` (what actually happened).  This
> directory is the operational layer: the checklists, the prompt
> philosophy, the hallucination catalogue, and the review procedures that
> turn an LLM-assisted workflow into a disciplined one.*

## 1. Prompt philosophy

* Ask for the **observable effect**, not the implementation.  "Wire the
  cache" produces a call site; "make repeated models avoid per-block table
  construction, proven by an allocation/throughput test" produces a fix.
* Ask for the **evidence requirement before the code**.  A residual is
  written with its test and evidence requirements first; the fix is done
  when the receipt exists and the seal passes.
* Ask for **negative claims**.  "What does this NOT do?  What could break
  if this is simplified?  What did you not verify?"  The agent's positive
  self-report is a hypothesis; its negative claims are the audit surface.
* Ask for **traces, not summaries**.  "Trace the guarantee to the final
  return" beats "does the code enforce it?" — the traced answer cites the
  exact lines.

## 2. Review philosophy

* The reviewer's job is to **falsify**, not to confirm.  Every claim is
  checked against the code path, then against a test, then against a
  receipt.
* Review the **values**, not the files.  A receipt that exists is not a
  receipt that is true; check the sample counts, the hashes, the
  verification booleans.
* Review **what else the operation touches**.  A script that "promotes
  evidence" must not rename the evidence tree; a version bump must not
  silently change dependency resolution.
* When the agent's summary and the code disagree, **the code wins and the
  summary is a defect** — fix either, record the residual, never paper
  over it.

## 3. Evidence-first workflow

```text
1. State the claim (doc, comment, commit message).
2. State the observable effect the claim implies.
3. Write the test that fails if the effect is absent.
4. Implement the minimal change that makes the test pass.
5. Write the court that reproduces the original defect class.
6. Generate the receipt; verify the hash.
7. Run the seal; the claim is "Sealed" only after it passes.
```

## 4. Common hallucination patterns (observed in this project)

| Pattern | Example | Defence |
|---------|---------|---------|
| Plausible structure, missing truth | Cache "wired" but storing the trivial artifact; receipts with fabricated defaults | Check observable effect; check values |
| Doc/comment claims the code does not enforce | "Never returns Ok with fewer blocks" while the return path ignores it | Trace to the final return |
| Tautological checks | Binding a hash by assigning then comparing the same value | Require independent capture |
| "Verified" after skipping | Self-hash check skipped, success printed | The tool must say what it did |
| Local optimisation, global destruction | Promote script renaming the whole evidence tree | Review what else the op touches |
| Optimistic error-path coverage | Tests pass because error paths are never exercised | Demand negative tests (truncation, cancellation, panic) |
| Literal-name reachability audit | "ModelCache has no production path" because the search never traced the wrapper that consumes it | Trace wrappers and downstream artifact consumption |
| A "skew" that is identity | Bench data generator remapped symbols to themselves; only 9 of 16 models were distinct | Mode-proof preflights reject wrong cardinalities |
| Exact assertions on scheduler-dependent metrics | Multi-worker thrash counts are interleaving-dependent; exact proofs failed | Worker-aware, data-driven proofs |
| Documented API with no production path | `ModelPolicy::Uniform/Global/External` never read by the encoder | Reachability + observable-effect doctrine |

## 5. Review checklists

### Before accepting a code change

- [ ] The claim has a code path (grep the call site).
- [ ] The effect is observable (a test changes when the behaviour changes).
- [ ] No silent fallback (explicit requests return typed errors).
- [ ] No new inert configuration (every field read in production).
- [ ] No doc comment promises more than the code enforces.
- [ ] No unwrap/panic reachable from untrusted input (or a local invariant
      annotation exists).
- [ ] The change compiles under the feature matrix (default,
      no-default-features, simd, affinity, loom cfg).
- [ ] Evidence impact assessed (covered-source change → regeneration
      scheduled).

### Before accepting an evidence change

- [ ] Values come from execution, not defaults.
- [ ] Hashes are of the actual files.
- [ ] Bindings are independent (not self-assigned).
- [ ] Nothing claims "verified" for a skipped check.
- [ ] History is superseded, never deleted.

### Before accepting a documentation change

- [ ] Terminology matches `docs/glossary.md`.
- [ ] Receipt IDs cited exist in `evidence/index.json`.
- [ ] Links resolve (the seal's documentation-link gate).
- [ ] No overclaim language (the no-overclaim gate).
- [ ] No information removed; additions only, unless correcting a
      documented defect.

## 6. When AI accelerates work

* Mechanical restructurings (moving code, renaming, feature-gating).
* Writing the volume of tests/courts/docs the evidence doctrine requires.
* Running and iterating on build/test/benchmark loops.
* First-draft adversarial review passes (a cheap second opinion that has
  found real bugs in this project: the decoded-hash aggregate condition,
  the reorder bound, the missed wakeup).
* Drafting the papers, ADRs, and this documentation.

## 7. When humans must intervene

* Setting the contract (stream formats, API semantics, evidence doctrine).
* Auditing summaries against code (the doc-vs-code gap is the recurring
  failure).
* Anything irreversible: publication, tags, supersession decisions.
* Choosing what to believe when two sources disagree.
* The final seal review — the seal gate is the machine's verdict, but a
  human reads it.

## 8. How prompts evolved (abridged)

1. "Implement X" → plausible structure, missing truth.
2. "Implement X with tests" → mechanical errors caught, inert wiring not.
3. "Implement X with tests, courts, and receipts; the observable effect
   must be proven" → the current standard.
4. "Audit X adversarially; trace the guarantee; name what you did not
   verify" → the audit standard.

## 9. Lessons learned

1. Plausible structure with missing truth is the failure mode.  Check
   values, not files.
2. A doc comment is a claim.  Trace it.
3. A check that cannot fail is not a check.
4. Never print "verified" after skipping.
5. History is evidence.  Supersede, never delete; record why.
6. Agents for throughput, humans for truth, machines for verification.
7. The agent has no memory; the repository's history, ADRs, and ledger
   are the memory.
8. The final standard: every serious claim traceable through real
   execution to a reproducible, adversarially verified, cryptographically
   bound artifact.
