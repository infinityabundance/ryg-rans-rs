# Building an Implementation Reference Instead of a Minimal Library

*An engineering article.  Why a codebase that preserves engineering
knowledge — rejected designs, audit history, invariants, evidence — is the
right shape for long-lived correctness-sensitive infrastructure.*

## Abstract

The dominant documentation culture says: keep comments minimal, let the
code be the truth.  For code whose correctness lives in properties the
code cannot express, that culture is catastrophically wrong.  This article
argues for the custodian model — long-form commentary, decision records,
failure encyclopedias, and an evidence chain — and reports how it was
applied to an entropy-coding library.

## 1. The code cannot express the contract

A renormalization boundary, a reciprocal bias, an interleaving order, a
flush order: none of these are visible in the code that uses them.  They
are pinned by an external reference and by invariants learned through
audits.  A comment that says "this is safe because checked" tells the
future maintainer nothing; the comment must say *which check, where, and
what invariant it protects*.

## 2. Custodian commentary

The repository's module commentary follows a fixed section set — Purpose,
History, Design, Alternatives, Invariants, Failure modes, Performance,
Verification, Receipts, Tests, Future evolution, References.  The function
and line commentary answers the questions the code cannot: why this
ordering, what breaks if changed, which receipt pins it.

## 3. Why rejected designs are recorded

A rejected design is a solved problem.  Recording *why* it was rejected
(`docs/adr/` has a "Rejected alternatives" section in every record) stops
the next engineer from re-proposing it and re-running the failure.  The
record of why `disable_inner_batching` was removed is a defence against a
future PR that re-adds configuration theatre.

## 4. Engineering archaeology

The failure encyclopedia (`docs/failures/`) records every important bug:
original assumption, observed failure, evidence, root cause, fix,
invariant introduced, prevention.  Future maintainers can recognise the
same failure class when it recurs in new clothing — and the audits show
that classes do recur.

## 5. Documentation philosophy

The constitution (`docs/philosophy.md`) states the rules: do not shorten
for readability; do not replace commentary with summaries; do not remove
history; preserve rejected alternatives; every unsafe block explains its
complete safety contract; every benchmark says what it does not measure;
every verification mechanism states its confidence boundary.

## 6. Long-term maintainability

The repository is designed to be maintained by someone who was not
present: the reading guides route by intent, the knowledge graph is the
encyclopedia, the atlas explains how the parts fit, and the seal gates
fail if the documentation architecture degrades.  A maintainer can answer
"could someone reproduce every benchmark?" with a paper, a command, and a
run directory.

## 7. Knowledge preservation beats minimalism

Minimal documentation minimises *writing* effort and maximises *reading*
effort for every future engineer.  For infrastructure with a decades-long
lifetime, the trade inverts: the writing is done once, the reading happens
forever.  The custodian model pays the writing cost once.

## References

`docs/philosophy.md`, `docs/layers.md`, `docs/education.md`,
`docs/navigation/inventory.md`.
