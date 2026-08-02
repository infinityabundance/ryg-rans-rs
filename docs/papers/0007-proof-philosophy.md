# Paper 0007 — Proof philosophy: what is proven, what is tested, and why the difference matters

> *Layer: Algorithm/Subsystem.  Companion: `docs/unsafe-ledger.md`,
> `docs/oracle-method.md`, `docs/residual-doctrine.md`.  Code:
> `crates/ryg-rans-rs-core/kani/`, `fuzz/`, `crates/ryg-rans-rs-oracle/`.*

## 1. The confidence stack

This repository deliberately uses **five different verification
mechanisms**, each with different guarantees and different costs:

| Mechanism | Guarantee | Cost | Where used |
|-----------|-----------|------|------------|
| **Kani (model checking)** | For the proven inputs: every reachable execution is verified; the property holds by construction | Bounded by symbolically tractable state spaces | Reciprocal identity, packed-entry bounds, encode/decode inversion, symbol construction |
| **Oracle courts (differential testing)** | For the tested inputs: byte-identical behaviour with the pinned upstream C | Deterministic corpus; adversarial but finite | All codec surfaces, cross-decoding, alias |
| **Property tests (proptest)** | For the generated inputs: the stated property holds | Randomized; the generator defines the space | Reorder permutations, model normalization, malformed headers |
| **Fuzz targets** | For the discovered inputs: no panic/UB; the input space is explored adversarially | Coverage-guided; not exhaustive | Parsers, codecs, SIMD, executor events |
| **Unit tests** | For the enumerated cases: exact expected behaviour | Explicit but hand-chosen | Everything |

The doctrine: a claim cites its mechanism.  "The reciprocal path is exact"
is a Kani claim for the proven frequencies, an oracle claim for the tested
models, and a *hypothesis* beyond both — which is why the boundary of each
proof is recorded (L16-E tracks the two intractable instances explicitly).

## 2. Unsafe reasoning

Every `unsafe fn` in the SIMD crate carries its own exact
`#[target_feature]` attributes (never inherited from the caller), a `#
Safety` section stating pointer provenance, bounds, alignment,
CPU-feature requirements, and the caller list, and a ledger entry
(`unsafe-ledger.toml`) that a bidirectional test keeps equal to the source
inventory.  The disassembly courts prove the compiled output contains the
expected instructions, so a kernel cannot silently scalarize.

The reasoning discipline: **no hidden caller obligation that could be
encoded in a safe type.**  If the caller must uphold a precondition, it is
either enforced by the safe wrapper (runtime feature detection, bounds
checks) or the function is not `unsafe`-but-documented — it is safe, and
the invariant is enforced inside.  The Phase L.10 quarantine is the
standing example: SSE helpers previously relied on the caller's
target-feature context; they now carry their own attributes.

## 3. Kani: what is mechanically proven

The proofs in `crates/ryg-rans-rs-core/kani/`:

* `reciprocal_proof.rs` — for byte rANS, at concrete frequencies
  (1, 2, 3, 7, 16, 255, 4095): the reciprocal-multiply fast path produces
  exactly `C(s, x)` for every reachable state `x`.
* `r64_reciprocal_proof.rs` — the same identity for the 64-bit state
  space (freq 1, 2, max).  The fully-symbolic-scale instances (freq 3,
  65535) do not terminate within practical time bounds because symbolic
  division is not bit-blastable; they are tracked as the accepted
  limitation L16-E and pinned by the differential + oracle tests instead.
* `packed_entry_proof.rs` — `try_pack` accepts exactly the in-range
  `(freq, bias, sym)` triples and rejects everything else.
* `encode_decode_inversion_proof.rs` — decode(invert(encode)) round-trips
  the state machine.
* `enc_symbol_new_proof.rs` — symbol construction yields the documented
  cumulative invariants.

A Kani proof is about the *inputs it was given*.  Concrete-frequency
instances are exhaustive for that frequency and every reachable state,
which is exactly the property the reciprocal path needs (the error term
depends on `freq` and the state range, both bounded).

## 4. Fuzzing: what the corpus explores

The standalone fuzz workspace (`fuzz/`) has nine targets: byte rANS
round-trip, R64 round-trip, word rANS round-trip, malformed byte streams,
alias round-trip, AVX-512VL 8-way, AVX-512 16-way, parallel block plans,
and parallel reorder event sequences.  The malformed-input targets
truncate at every byte/word, so an over-read or a panic at any truncation
point is found.  Fuzzing has already found and fixed real defects
(residuals L16-B: out-of-bounds model reads, a single-symbol u32 threshold
overflow, a short-slice unwrap, and a 1 GiB-per-iteration allocation in
the R64 target).

Fuzzing's guarantee is negative: "no crash found in this many executions".
It is not a proof of absence.  The repository says exactly that.

## 5. Oracle and differential testing: the semantic anchor

The oracle court compiles the pinned upstream C and requires byte-exact
parity.  This is the strongest *semantic* anchor available for a codec,
because the stream format is defined by the upstream bytes, not by our
documentation.  Differential testing extends the same idea internally: the
division and reciprocal paths must agree; every SIMD kernel must agree with
its scalar reference and with every other executable backend (report
parity: output, words-consumed, final-states).

The oracle cannot prove "correct in general" — it proves "identical to the
reference on this corpus".  That is the right standard for a
reconstruction, and it is why the corpus is adversarial (repeated symbols,
all-symbols-observed, one-symbol models, maximum frequency, frequency one,
renormalization boundaries, length boundaries) rather than random.

## 6. Proof boundaries: what each mechanism cannot do

* Kani cannot bit-blast the fully-symbolic R64 division (L16-E).
* Fuzzing cannot prove the absence of crashes.
* The oracle cannot prove generality.
* Property tests inherit their generators' blind spots.
* The unsafe reasoning is an *argument*, mechanically checked only for
  ledger equality and disassembly, not for the argument itself.

Recording these boundaries is not pessimism; it is what makes the stack
honest.  Every receipt and every residual states its mechanism and its
boundary, and the seal refuses to upgrade a weaker mechanism's claim into a
stronger one.

## 7. Engineering confidence: how the pieces compose

The composition rule is simple: **confidence is the intersection of
independent mechanisms.**  The reciprocal path is Kani-proven for the
frequency set, oracle-proven for the corpus, differential-proven against
the division path, and exercised by fuzz round-trips.  No single mechanism
is trusted alone; the intersection is what the receipts record.  When a
fix lands (e.g. the decoded-hash integrity fix), the court that reproduces
the original bug, the test that pins the new behaviour, and the receipt
that seals it all land together — a fix without all three is not finished.
