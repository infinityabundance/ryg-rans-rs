# References

> *Layer: cross-cutting.  The papers and module commentary cite these by
> short name.  This file is the canonical bibliography.*

## rANS / ANS

* J. Duda, *Asymmetric Numeral Systems: entropy coding combining speed of
  Huffman coding with compression rate of arithmetic coding* (2009–2013).
  The founding paper for ANS; rANS is the range-based variant.
* J. Duda, *Asymmetric numeral systems* (PhD dissertation, Jagiellonian
  University, 2013) — the full treatment including the FELCS variants.
* F. Giesen, *ryg_rans* — the public-domain reference implementation this
  repository reconstructs byte-for-byte.  Pinned revision recorded in
  `docs/bitstream-contract.md`.  Files: `rans_byte.h`, `rans64.h`,
  `rans_word_sse41.h`, `main_alias.cpp`.
* F. Giesen, *A ryg blog* — "Ryg's rANS" and related posts on entropy
  coding and SIMD implementation techniques.

## Division-free arithmetic

* R. Alverson, *Integer Division using reciprocals* (1991).  The
  multiply-high reciprocal technique used for the division-free encode
  path (ADR-0002).

## Alias method

* M. Vose, *A linear algorithm for generating random numbers with a given
  distribution* (IEEE TPAMI, 1991).  The O(N) alias-table construction
  used by the alias surface.

## SIMD / x86

* Intel, *Intel 64 and IA-32 Architectures Optimization Reference Manual* —
  gather latency/throughput guidance, `vpgatherdd` behaviour across
  generations.
* Intel Intrinsics Guide — instruction semantics for the intrinsic surface
  (`_mm256_i32gather_epi32`, `_mm512_i32gather_epi32`, `pshufb`,
  `pblendvb`, `vpermd`, `vpmovdb`).
* AMD, *Software Optimization Guide for AMD Family 19h Processors* (Zen 4)
  and Family 1Ah (Zen 5) — the microarchitecture guidance behind the
  manual-gather vs hardware-gather variants (paper 0003 §4).

## Verification

* The Kani Rust Verifier documentation — the model-checking harness
  semantics used in `crates/ryg-rans-rs-core/kani/`.
* The Rust Reference, *The Rustonomicon* — unsafe code rules, aliasing,
  and the target-feature/`#[target_feature]` contract the SIMD crate's
  safety sections rely on.
* The `proptest` crate documentation — the property-testing framework used
  for reorder permutations and model normalization.
* The `loom` crate documentation — the deterministic scheduler modelling
  used for the executor courts.

## Measurement

* The Criterion.rs documentation — the statistical measurement framework;
  `benchmark.json` / `sample.json` / `estimates.json` semantics the
  exporter reads.
* The `tar` crate documentation — PAX/GNU long-name handling used by the
  archive writer (L1-K).

## Evidence engineering

* The project's own evidence doctrine, in priority order:
  `AGENTS.md` (ground truth), `docs/papers/0006-evidence.md` (the system),
  `docs/residual-doctrine.md` (defect accounting), `docs/glossary.md`
  (terms).
