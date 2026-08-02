# Documentation Philosophy — the custodian constitution

> *This document is the constitution for every other document in this
> repository.  If a document contradicts this philosophy, the document is
> wrong.*

## What "custodian documentation" means

This repository is not written for the person who wants a quick summary of
what rANS is.  It is written for the person who must, years from now,
understand why every line exists, why every threshold has its value, why
every invariant is load-bearing, and why the evidence chain is arranged the
way it is.  That person is the **custodian**: the engineer — possibly the
same engineer who wrote the code, possibly not — who must be able to:

* modify any subsystem without breaking a documented guarantee;
* regenerate any receipt, any benchmark, any seal, from scratch;
* explain any design decision, including the ones that were rejected;
* recognise which "obvious simplification" would silently destroy a
  correctness property.

Custodian documentation treats **knowledge preservation** as the primary
goal.  Readability is a constraint, never the objective.  When a short
sentence and a long explanation both convey the same fact, the long
explanation wins if it also conveys *why the fact must hold*.

## Why this repository intentionally preserves engineering knowledge rather than minimizing comments

Modern library documentation culture values brevity: comments that restate
the code are noise, and the code is the truth.  That culture is correct for
code whose behaviour is self-evident.  It is catastrophically wrong for
code whose *correctness depends on facts the code cannot express*.

rANS entropy coding, SIMD decode kernels, and deterministic parallel
executors carry their correctness in properties that live **outside** the
lines of code:

* a reciprocal-encoding bias term that must be `(M - 1) << shift` and
  nothing else, because every decoder in the ecosystem was built to that
  exact convention;
* a renormalisation boundary that must not be moved, because the pinned
  upstream bitstream is defined by it;
* an interleaving order that must match the encoder's flush order, which is
  the reverse of the decode order, because that is what upstream defines;
* a worker-pool completeness counter that must be checked at the public-API
  boundary even when the executor already checks it internally, because the
  promise belongs to the API;
* a cache key that must include the codec and scale bits, because the same
  model bytes produce different tables for different codecs.

None of these facts are discoverable from reading the code in isolation.
They were learned the hard way — from the pinned upstream implementation,
from the oracle court, from audits that found real bugs, from benchmarks
that measured the wrong thing.  Writing them down next to the code is not
comment bloat; it is the only way the knowledge survives.

## Why implementation references differ from production libraries

A production library README answers: *what can I call, and what does it
do?*  An implementation reference answers: *why is it this way, what did we
try first, what broke, and how do we know it still works?*

This repository is both.  The crate READMEs answer the first question
accurately and completely.  The papers, ADRs, history, module commentary,
and code annotations answer the second.  The two audiences overlap but are
not the same, which is why the information is **layered** (see
`docs/architecture.md`): each layer explains something different, and no
layer repeats another layer's job.

## Why long-form commentary is intentional

Every long-form document in this repository exists because a shorter
document was tried and failed to preserve something.  The failure mode is
specific: a summary preserves *what*, but the next engineer needs *why*,
and the third engineer needs *why-not-the-alternative*, and the fourth
needs *how-do-we-know*.

A paragraph that explains "the reciprocal path exists because division is
3–5× slower on x86-64 and the upstream ryg_rans implementation pins the
reciprocal convention" cannot be shortened to "division is slow" without
losing the *pinned convention* fact, which is the one that actually
protects correctness.  Long-form is the deliberate choice; brevity is the
failure mode.

## Why preserving rejected designs is valuable

A rejected design is a solved problem.  Someone already evaluated it, found
the specific way it fails, and chose differently.  If that evaluation is
not recorded, the next engineer will re-propose the rejected design, re-run
the evaluation (poorly, because they lack the original context), and either
waste weeks or — worse — accept a subtly broken variant.

The ADRs in `docs/adr/` and the historical record in `docs/history/` exist
so that the work of rejecting an idea is done once.  The record of *why
`disable_inner_batching` was removed* is not nostalgia; it is a defence
against a future PR that re-adds a configuration field with no executable
path (the "configuration theatre" defect class that Phase L.6 explicitly
hunted and eliminated).

## Why future maintainers matter more than current readability

Every document is written for a reader who does not yet exist.  The current
author always knows the context; the future maintainer does not.  When a
comment says "see above", the current author knows what "above" means; the
future maintainer may not.  When a comment says "this is safe because
checked", the future maintainer cannot tell *which* check, *where* it is,
or *what invariant* it protects.

The standard for every annotation in this repository is: **a reader who has
never seen this file must be able to explain the invariant, the failure
mode it prevents, and the test that detects a regression, from the comment
alone.**  That standard biases every decision toward explicitness.

## Why entropy coders require slow, careful reading

Entropy coders are among the most tightly coupled algorithms in software
engineering.  Every symbol's decode depends on the encoder's exact choice
of:

* the normalisation threshold (`RANS_WORD_L`);
* the renormalisation shift (`RANS_WORD_RENORM_SHIFT`);
* the interleaving width and lane assignment;
* the flush order of the trailing state words;
* the reciprocal/bias convention;
* the cumulative-frequency table layout.

Change any one of these and the decoder silently produces garbage that no
amount of internal consistency checking can catch — the stream is
self-consistent but wrong.  This is why the repository maintains a pinned
bitstream contract (`docs/bitstream-contract.md`), a byte-exact oracle court
against the upstream C implementation, and cross-decoding receipts.  The
docs are written to force the reader to slow down and check these
properties, because the code itself cannot.

## Why evidence matters more than claims

The frozen invariant of this repository is: **a claim is true only when it
can be traced through code → test → court → receipt → seal.**  Prose is not
proof.  A README that says "strict integrity is enforced" is a claim; the
court `RYG_RANS.L.VERIFY.DECODED_HASH` plus its receipt plus the seal gate
is the proof.  When the documentation and the code disagree, the
documentation is defective — this is a standing rule (see `AGENTS.md`,
"Stale-document warning"), and fixing the code or the doc and recording the
residual is mandatory.

This philosophy is why the documentation uses precise terms from
`docs/glossary.md` (block, stream, surface, backend, plan, receipt,
manifest, residual, seal, preflight, canonical output, canonical error) and
why every receipt ID in the evidence index is citable from documentation.

## Why comments preserve reasoning rather than syntax

A comment that says `// x = x * 2` is noise.  A comment that says
`// x = x * 2 — doubling here keeps the fixed-point scale at 2^-12 so the
// reciprocal table stays in u32; halving it would overflow slot math at
// scale 16 (see ADR-0003)` is engineering history.  The first restates
what the code already says; the second preserves the reasoning that the
code cannot express.

The rule for every comment in this repository is: **if the comment can be
derived from the code by a competent reader, delete it.  If it cannot, keep
it and make it precise.**  The practical effect is that comments here are
longer, more specific, and more valuable than in a typical codebase — and
that is deliberate.

---

## Consequences (normative)

1. Do not shorten documentation for readability.
2. Do not replace engineering commentary with summaries.
3. Do not remove historical context.
4. Do not collapse detailed explanations into bullet points.
5. Preserve rejected alternatives and the reasons they were rejected.
6. Preserve engineering evolution and audit history.
7. Every important invariant must be documented where it is enforced.
8. Every significant performance decision must explain why it exists.
9. Every unsafe block must explain its complete safety contract.
10. Every benchmark must explain exactly what it measures and what it
    intentionally does **not** measure.
11. Every verification mechanism must explain its confidence boundaries.
12. Every major subsystem must include diagrams, historical context,
    failure modes, and evidence references.
13. When documentation and code disagree, the documentation is defective —
    fix one, record the residual, never "resolve" the disagreement by
    deleting the history.
