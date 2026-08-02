# Evidence-Driven Verification for Compression Libraries

*An engineering article.  How a compression library can make every serious
claim traceable through real execution to a cryptographically bound
artifact — and why "the tests pass" is not enough.*

## Abstract

Compression libraries are uniquely vulnerable to plausible-but-wrong
implementations: a stream can be internally consistent and still wrong
(renormalization boundary moved, reciprocal bias drifted), and a "fix"
can wire a feature without changing any behaviour.  This article presents
the verification architecture used by `ryg-rans-rs`: differential oracle
courts, behavioural and performance receipts, a canonical seal gate, and
the doctrine that prose is not proof.

## 1. The problem with "tests pass"

Unit tests enumerate the cases the author thought of.  For a codec whose
correctness is defined by an external reference (the upstream C bytes),
the author's imagination is not the standard.  Three defect classes escape
unit tests:

1. **Format drift** — output is self-consistent but not interoperable.
2. **Inert wiring** — a documented feature has no observable effect.
3. **Fabricated evidence** — receipts exist but their values are defaults.

## 2. The oracle philosophy

Compile the pinned upstream C with its own recipe; run both
implementations over a deterministic adversarial corpus (repeated
symbols, one-symbol models, all-symbols-observed, renormalization
boundaries, length boundaries); require byte-identical output.  The oracle
cannot prove generality, but it proves the strongest practical property: on
the corpus that exercises the format's corners, the two implementations
agree byte for byte.

## 3. Behavioural receipts

A court produces a manifest (inputs, expected results) and a receipt
(per-case verdicts, actual results, residual references, implementation
commit, artifact hashes).  The index maps court ID → receipt file SHA-256.
The seal verifies every link: file hashes, manifest hashes, canonical
self-hashes, verdict types, and commit ancestry.  Legacy receipts with no
verifiable canonical scheme are *reported as such* — never falsely
"verified" (the L1-R rule: verify, or say you did not).

## 4. Performance receipts

Performance evidence adds provenance: the run wrapper captures commit,
tree, Cargo.lock SHA, rustc, `RUSTFLAGS`, and host metadata *before* the
run; a `RUN_COMPLETE` marker is written only on full success; the sealer
compares captured values against the intended implementation commit.  Each
benchmark case emits a preflight record before timing (input/output
hashes, backend requested/executed, thread counts); timing joins preflight
by exact benchmark ID.  A timing of a wrong decode is noise, and the chain
rejects it.

## 5. Seals

The seal gate is the single authoritative final gate: clean tree, build,
tests, feature matrices, unsafe ledger equality, behavioural chain,
performance chain, Docker matrix, publication dry-run, documentation
integrity.  It fails on any warning affecting evidence validity and never
prints success for a skipped verification.

## 6. Why receipts exist

A receipt converts "we believe X" into "X was observed, under these
conditions, from this commit, and the artifact hash verifies".  The
conversion is what makes claims auditable years later by someone who was
not present.

## 7. Historical defects the architecture caught

* The decoded-hash aggregate bug: a block with intact payload + corrupted
  model decoded wrong and passed.  The decoded-output hash closes it.
* The inert model cache: consulted but storing the trivial artifact; no
  throughput gain.  The observable-effect doctrine closes it.
* The unenforced cancellation promise: documented, delegated, not checked
  at the boundary.  The boundary-completeness rule closes it.
* The Phase K fabricated metadata: 831 records with sample counts of 1 and
  hardcoded verification.  Value-provenance rules close it.

## 8. The evidence lifecycle

observation → manifest → receipt → index → seal → README status.  Every
link is hashed; every link is verified; every supersession is recorded and
retained (superseded evidence is never deleted).

## 9. Lessons

1. Verification is a chain, not a step: the weakest link defines the
   confidence.
2. Values must come from execution, not defaults.
3. A check that cannot fail is not a check.
4. Never delete history; supersede with a reason.

## References

`docs/papers/0006-evidence.md`, `docs/papers/0005-performance-methodology.md`,
`docs/residual-doctrine.md`, `docs/oracle-method.md`, the gap ledger, and
`evidence/`.
