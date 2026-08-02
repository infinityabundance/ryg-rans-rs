# The Engineering Story (N.8)

> This is not marketing.  It is the engineering history of the repository
> told as a story: problem, hypothesis, implementation, evidence,
> unexpected results, corrections, new understanding — repeated.  The
> repository evolved through evidence, not assumptions; this is the record
> of that evolution.  Every claim links to the evidence that drove it.

## Act I — The reconstruction

**Problem.** Build a native Rust rANS codec that interoperates with the
public-domain reference: a stream encoded here must decode there.

**Hypothesis.** Byte-exact reconstruction with an oracle court as the
arbiter is the only way to get interoperability while rewriting the code
in a memory-safe language.

**Implementation.** Four codec surfaces, two encode paths each (division
reference + reciprocal fast path), SIMD kernels, and the oracle harness.

**Evidence.** The oracle receipts agreed byte for byte.  The Kani proofs
later pinned the reciprocal identity for the concrete frequencies.

**Unexpected result.** The reciprocal bias is *part of the stream format*.
A "helpful" change to the bias would break every stream — the proof of
the fast path is also the proof of the pin.

**Correction / new understanding.** The division path is not dead code;
it is the reference the proofs and the oracle compare against.  Keeping
both is the "two implementations of one contract" pattern that drives the
whole verification strategy.

## Act II — The parallel engine

**Problem.** Decode many blocks in parallel without losing order, error
identity, or memory bounds.

**Hypothesis.** A producer thread + two bounded channels + live reorder
commit makes all three true by construction.

**Implementation.** The bounded live executor (ADR-0004), the atomic
reorder commit (ADR-0014), deterministic error selection (ADR-0005).

**Evidence.** Stress tests (10 GiB-equivalent synthetic stream with
bounded RSS), loom courts, and the sealed performance run.

**Unexpected result.** The loom model found a real missed-wakeup race in
the channel layer — the sender count lived outside the mutex.  A reorder
bound of `effective_queue` alone stalled the pipeline; it needed
`effective_queue + workers`.

**Correction / new understanding.** Concurrency defects hide in the
coordination, not the kernels.  Modelling the schedules (loom) found what
testing could not.

## Act III — The integrity bug

**Problem.** A block with an intact payload hash and a corrupted model
decoded to wrong output — and *passed* verification, because the
aggregate failure condition ignored the decoded-hash verdict it computed.

**Hypothesis.** The decoded-output hash must be load-bearing: strict
integrity (zero/unset fails, mismatch fails, only a matching nonzero hash
passes) as the default.

**Implementation.** `IntegrityPolicy`, `HashVerification`,
`DecodedHashMissing`/`DecodedHashMismatch`, and the court that proves
model corruption cannot pass merely because the payload hash is intact.

**Evidence.** The 15-combination court matrix; the CLI exit-code-5 tests.

**New understanding.** Payload hashing proves the compressed bytes are
intact; it cannot prove the decoded output is correct.  Integrity is a
property of the output, not the input.

## Act IV — The evidence failure

**Problem.** The Phase K performance pipeline produced 831 records that
were structurally present and semantically empty: sample counts of 1,
hardcoded verification, zero throughput, truncated archive paths, a
tautological commit binding, and a seal that printed "verified" after
skipping.

**Hypothesis.** Evidence must come from execution: values captured at
benchmark time, joined to preflight records, bound to the source, hashed
as real files, and verified by a seal that cannot lie about skipping.

**Implementation.** The run wrapper, the preflight channel, the
Criterion-metadata exporter, the dual-hash receipts, the canonical
indexes, the 40-gate seal.

**Evidence.** The superseded Phase K run (retained), the re-sealed runs
(`phase-l-20260802b` … `phase-l-20260802e`).

**New understanding.** File presence is not evidence; value provenance is.
The seal's honesty rule (verify, or say you did not) is the load-bearing
gate.

## Act V — The doc-vs-code gaps

**Problem.** Audits reopened two "resolved" residuals: the model cache was
consulted but stored the trivial artifact (no throughput gain), and the
cancellation APIs documented guarantees their return paths did not
enforce.

**Hypothesis.** A wiring claim needs an observable effect; a doc comment
is a claim that must be traced to the code.

**Implementation.** Arc-shared packed-table caching; `check_completeness`
at every public boundary.

**Evidence.** `Arc::ptr_eq` hit tests; pre-cancelled-token tests through
every entry point; the re-sealed evidence at `50eaaee`/`ee8fb0e`.

**New understanding.** "Resolved" means the observable effect is proven.
The agent-adjacent failure classes (plausible structure, doc-vs-code) are
now named in the LLM methodology (paper 0008, `docs/llm/`).

## Act VI — Knowledge preservation

**Problem.** The corpus had grown deep but not navigable; the knowledge
was in the documents but not organised for entry at the correct depth.

**Hypothesis.** Custodian documentation — philosophy, layers, papers,
ADRs, history, failures, story, atlas, navigation, articles — preserves
the knowledge and makes it navigable.

**Implementation.** Phase M (the documentation constitution and the
papers/ADRs/history) and Phase N (the navigation layer, the atlas, the
articles, the failure encyclopedia, the seal gates that keep it healthy).

**Evidence.** The documentation-inventory and navigation-completeness
seal gates; the full seal green.

**New understanding.** The repository is not finished when the code is
correct; it is finished when the *next* engineer can reconstruct every
decision, reproduce every benchmark, and regenerate every receipt — and
when the tooling fails if the documentation architecture degrades.

## Coda

The pattern repeated: problem → hypothesis → implementation → evidence →
unexpected result → correction → new understanding.  The repository's
final state is the accumulation of those cycles, each one recorded, each
one sealed.  That is what makes it a case study in evidence-driven
engineering rather than an assumption-driven one.

**Related:** `docs/history/index.md` (the chronology), `docs/failures/`
(the failure encyclopedia), `docs/papers/0008` (the methodology),
`docs/navigation/knowledge-graph.md` (the map).
