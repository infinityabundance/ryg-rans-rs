# Paper 0005 — Performance methodology: how this repository measures itself

> *Layer: Subsystem.  Companion: `docs/performance-method.md` (the short
> form), `docs/performance/phase-l17-analysis.md` (the L.17 analysis),
> `evidence/performance/` (the sealed runs).  Code: `crates/ryg-rans-rs-bench/`,
> `xtask/src/main.rs` (`benchmark-run`, `performance-seal`).*

## 1. Why a methodology paper exists

Performance numbers rot in three ways: they go stale (hardware changes),
they lie (measured with the wrong configuration), and they detach (no proof
that the number came from the code it claims to describe).  This repository
treats all three as evidence-integrity failures, and the methodology exists
to make the failures detectable.  A number in `evidence/performance/` is
trustworthy only if the *entire chain* that produced it is reproducible:
benchmark source → run wrapper → Criterion measurement → preflight records
→ exporter → receipts → index → seal.

## 2. Verification before timing

The core rule: **a benchmark case is timed only after a preflight proves
the case is correct.**  Each benchmark emits a `BenchmarkPreflightRecord`
before timing: the input hash, the output hash, the reference output hash,
words-consumed, final states, the requested vs executed backend, and the
thread counts.  The exporter joins Criterion timing with the preflight
record by exact benchmark ID and rejects:

* missing or duplicate preflight;
* a failed preflight;
* backend identity mismatch (requested ≠ executed);
* output/words-consumed/final-state hash mismatches;
* requested/effective worker inconsistency.

A timing measurement of a *wrong* decode is not a throughput number; it is
noise.  This is the difference between this suite and a naive benchmark.

## 3. Criterion: what is measured and how

Criterion 0.5.1 drives the measurement: warmup, sample count, and
confidence intervals are Criterion's.  The exporter reads the canonical
metadata — `benchmark.json` (`group_id`, `function_id`, `value_str`,
`full_id`, `throughput`), `sample.json` (the actual per-sample timings),
and `estimates.json` — never the flattened directory name.  The actual
sample count is derived from `sample.json` (`iters.len() == times.len()`),
never defaulted.  Throughput byte counts come from Criterion's
`throughput.Bytes` field, never from a directory name.

## 4. Allocation policy and measurement honesty

Benchmarks separate the dimensions a naive suite conflates:

* **Core codec throughput** — the kernel, no container, no hashing.
* **Payload SHA-256 only**, **decoded SHA-256 only**, **both hashes** —
  the integrity work is measured as its own component, because it is the
  dominant overhead in the parallel engine (~1.4× single-worker overhead,
  almost all of it hashing).
* **Model reconstruction**, **plan construction**, **cache lookup** —
  the per-block preparation work, measured separately so scaling loss is
  attributable.
* **Queue scheduling**, **reorder commit**, **allocation**, **scratch
  reuse** — the parallel-engine components.

Attribution rule: never claim a scaling loss is "memory bandwidth" or
"cache" or "SMT" without a component measurement or hardware-counter
evidence for that specific claim (L.17-C records that hardware counters are
unavailable on this host; software decomposition is used instead).

## 5. Host capture: the benchmark-time truth

The `benchmark-run` wrapper captures, **before** the run starts:

* git commit SHA, git tree SHA, Cargo.lock SHA-256;
* `rustc -vV`, `RUSTFLAGS`;
* host metadata (`host.json`: CPU model, features, microcode, SMT state,
  governor, kernel, libc, memory) and `cpuinfo.txt`;
* the exact command line and environment (`commands.log`).

It refuses a dirty tree, runs the suite itself, re-captures the metadata
afterwards, refuses to seal if the environment changed materially, and
writes a `RUN_COMPLETE` marker only on full success.  The sealer compares
the captured values against the intended implementation commit.  Runtime
CPU features, compiled features, and executed backend features are recorded
separately (Phase L.1-I) — a number cannot be attributed to AVX-512 unless
the preflight says an AVX-512 kernel actually executed.

## 6. Interpreting results

* Confidence intervals come from Criterion's estimates; a receipt whose
  timing statistics are non-finite or whose CI is invalid is rejected.
* Zero throughput is valid **only** for an explicitly non-throughput
  microbenchmark whose schema says the unit is latency.
* A faster result is accepted only if the whole chain re-verifies; a
  regression becomes a residual when it exceeds the statistically
  meaningful threshold for the host and workload.
* The ten sealed receipts (800 cases × 100 samples, run
  `phase-l-20260802b`) are the only "Sealed" performance claims; the CLI
  `bench` subcommand is a live smoke measurement and says so.

## 7. Common mistakes this methodology exists to prevent

1. **Timing an unverified decode** — numbers for wrong output (the
   preflight channel exists to make this impossible).
2. **Attributing to the wrong component** — claiming a SIMD speedup when
   the difference is hashing or allocation (component isolation).
3. **Measuring with a different build than claimed** — `RUSTFLAGS` captured
   at run time, verified at seal time.
4. **Copying a Criterion directory from another machine** — the wrapper
   binds the run to the host and tree; a copied directory fails the
   binding checks.
5. **Directory-name identity** — the exporter uses Criterion metadata, not
   the sanitized path.
6. **Fabricated sample counts or byte counts** — derived from
   `sample.json` and `throughput.Bytes`.
7. **Hardcoded verification booleans** — `verification_passed` comes from
   the preflight join, never from a constant.

## 8. Invalid measurements and superseded runs

A measurement is invalid if any link in the chain fails: binding, preflight,
hashes, metadata, or residual accounting.  Invalid and superseded runs are
**retained**, marked with `SUPERSEDED.md`/`INVALIDATED.md`, and the reason
recorded in the gap ledger.  The Phase K run (`phase-k-20260731-004044`) is
the standing example: retained in full, superseded because its exporter
fabricated metadata (residuals L1-A..L1-S).  Deleting history is forbidden;
superseding with a reason is the doctrine.

## 9. Comparative measurement

The L.14 comparative court (bench `comparative`) measures against
`hypersonic-rANS` and the FFI `rans` binding with identical corpora,
models, sizes, operations, allocation policies, thread counts, affinity,
compiler flags, target CPU, warmup, samples, and integrity work — and
separates core codec throughput, model construction, allocation, FFI
crossing, container overhead, hashing, and parallel scheduling.  Where the
comparison is methodologically impossible (different formats), the
alternative is pinned, excluded, and recorded as a residual — never
claimed as a comparison.  See `docs/performance/comparative.md`.
