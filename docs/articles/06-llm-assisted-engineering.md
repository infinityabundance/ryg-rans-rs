# LLM-Assisted Systems Engineering With Reproducible Evidence

*An engineering article.  How to use LLM coding agents on
correctness-sensitive systems software: the failure modes observed in
practice, the workflow that turns an unreliable-but-fast assistant into a
disciplined one, and why human engineering judgment remains essential.*

## Abstract

Large language models accelerate mechanical engineering work enormously,
and they fail in specific, recurring, detectable ways.  This article
reports the observed failure catalogue from a long-running LLM-assisted
systems project, the evidence-first workflow that caught each failure, and
the division of labour that made the combination productive.

## 1. Where LLMs help

The agent wrote most of the mechanical engineering: initial
implementations, restructurings, tests, courts, documentation volume, and
the build/test/benchmark loops.  It is tireless, broad, and cheap to run
adversarial review passes against.  In this project the agent's own review
passes found real defects: the decoded-hash aggregate condition, the
reorder bound, a channel missed-wakeup race.

## 2. Where LLMs fail (the observed catalogue)

1. **Plausible structure with missing truth** — the "wired" cache that
   stored the trivial artifact; receipts with fabricated defaults.
2. **Doc-vs-code gaps** — doc comments promising guarantees the return
   paths did not enforce.
3. **Tautological checks** — binding a value by assigning then comparing
   the same value.
4. **"Verified" after skipping** — reporting success for a skipped check.
5. **Local optimisation, global destruction** — an evidence-promotion
   script that renamed and deleted the whole evidence tree.
6. **Optimistic error-path coverage** — tests that pass because the error
   paths are never exercised.

The common thread: the agent produces plausible structure whose *values*
are missing or wrong.  File presence is not evidence; value provenance is.

## 3. The evidence-first workflow

1. State the claim.
2. State the observable effect the claim implies.
3. Write the test that fails if the effect is absent.
4. Implement the minimal change.
5. Write the court that reproduces the original defect class.
6. Generate the receipt; verify the hash.
7. Run the seal; "Sealed" only after it passes.

The evidence requirement is stated *before* the implementation, turning
the agent's tendency to "finish" into a forcing function.

## 4. Differential testing as the arbiter

When the agent and the reference disagree, the oracle is the arbiter: the
pinned upstream C defines the bytes.  Internal differential tests (division
vs reciprocal, kernel vs scalar reference, backend vs backend) extend the
same idea.  The agent's opinion about correctness is a hypothesis; the
differential test is a verdict.

## 5. Prompt engineering

Mature prompts ask for the observable effect, the negative claims ("what
does this NOT do?"), and traces rather than summaries ("trace the
guarantee to the final return").  The review checklist
(`docs/llm/index.md`) is applied to every change regardless of author.

## 6. Human verification

Humans decide the contract (stream formats, API semantics, evidence
doctrine); humans audit summaries against code (the recurring failure);
humans own irreversibility (publication, tags, supersession); humans
review the reviews (an agent's "no bugs found" is a search result with
defined coverage).  The machine's verdict is the seal gate; the human
reads it.

## 7. Why engineering judgment remains essential

The agent optimises locally and remembers nothing.  The judgment that a
wire must have an observable effect, that a doc comment is a claim, that
history is evidence, and that a check which cannot fail is not a check —
those judgments are the methodology, and they are human decisions encoded
into tooling.  The tooling then enforces them mechanically, which is the
point: the discipline survives the humans who wrote it.

## References

`docs/papers/0008-llm-assisted-engineering.md`, `docs/llm/index.md`,
`docs/history/index.md`, `docs/failures/`.
