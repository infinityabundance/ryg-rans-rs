# Lessons From Building rANS in Rust

*An engineering article.  What a from-scratch, no-unsafe-core
reconstruction of `ryg_rans` taught us about ownership, safety,
performance, verification, and architecture evolution.*

## Abstract

Rebuilding a byte-exact codec in a memory-safe language is a different
discipline from writing one fresh: the stream format is owned by the
upstream, so every "improvement" is a potential format break.  This
article reports the lessons: how `forbid(unsafe_code)` shaped the core,
how the unsafe surface was isolated and quarantined, how ownership
structured the writer/reader traits, and how the oracle became the
arbiter.

## 1. Ownership as an API

The core crate's writer/reader traits (`BackwardByteWriter`,
`ByteReader`, …) encode the encode/decode directionality in the type
system: encoding writes backwards (the state flushes low units first), so
the writer hands out slices from the buffer's end.  Getting the direction
wrong is a compile error, not a runtime bug — ownership and directionality
are the same concern.

## 2. `forbid(unsafe_code)` in the core

The rANS state machine is pure arithmetic on `u32`/`u64` with no pointer
provenance concerns, so the core carries `#![forbid(unsafe_code)]`.  Any
correctness bug propagates as a detectable wrong-symbol decode, never as
undefined behaviour.  The unsafe surface lives in exactly one production
crate (SIMD), where intrinsics require it, and is ledgered, bidirectionally
tested, and disassembly-checked.

## 3. Performance without unsafe

The reciprocal multiply-high path is the classic "performance requires
unsafe" case that does not: it is safe integer arithmetic with a proven
exactness bound.  Kani proves the identity for the concrete frequencies;
the oracle proves byte parity.  Performance and safety were not in
tension because the algorithm's fast path is arithmetic, not memory games.

## 4. Oracle parity as the ground truth

The stream format is the upstream bytes.  "Our tests pass" is necessary
but not sufficient; "the oracle agrees byte for byte on the adversarial
corpus" is the property that makes the library interoperable.  The oracle
is not a test — it is a differential harness whose output is a sealed
receipt.

## 5. Testing the corners

The adversarial corpus is not random: one-symbol models, all-symbols-
observed, maximum frequency, frequency one, renormalization boundaries,
length boundaries, truncation at every byte and word.  The fuzz targets
explore beyond the corpus; they have found real defects (out-of-bounds
model reads, a single-symbol threshold overflow, a short-slice unwrap).

## 6. Architecture evolution

The repository evolved through audit cycles, each of which found a class
of defect: wiring-without-effect (the model cache), doc-vs-code gaps (the
cancellation promise), fabricated evidence (the Phase K exporter),
evidence-destroying tooling (the promote script).  The invariant that
emerged: a claim is true only when traceable through code → test → court →
receipt → seal.  Every architecture change since has been checked against
that chain.

## 7. Hard lessons

1. Plausible structure with missing truth is the failure mode.  Check
   values, not files.
2. A doc comment is a claim; trace it to the code.
3. A check that cannot fail is not a check.
4. History is evidence; supersede, never delete.
5. The final standard: every serious claim traceable through real
   execution to a reproducible, adversarially verified, cryptographically
   bound artifact.

## References

`docs/papers/0001-rans-design.md`, `docs/history/index.md`,
`docs/failures/`, `docs/llm/index.md`.
