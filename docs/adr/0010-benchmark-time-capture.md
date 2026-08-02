# ADR-0010 — Evidence is captured at benchmark time, verified at seal time

Status: Accepted

## Context
The Phase K pipeline captured host metadata, CPU features, git state, and
`RUSTFLAGS` when the sealer ran — not when Criterion ran.  A Criterion
directory could be copied from another machine, or generated from a dirty
tree, and then "sealed" later from a clean tree.  The seal's evidence did
not describe the measurement.

## Problem
How to bind performance evidence to the source and environment that
actually produced it.

## Alternatives considered
1. Keep seal-time capture (the broken design).
2. A run wrapper (`cargo xtask benchmark-run`) that captures everything
   before the run, runs the suite itself, re-captures after, refuses to
   seal if the environment changed materially, and writes a completion
   marker only on full success.
3. Trust the benchmark binary to self-report.

## Rejected alternatives
- (1) was rejected (L1-E): provenance must describe the run.
- (3) was rejected as insufficient alone: self-reporting is part of the
  chain (preflight records) but not the whole chain.

## Decision
`benchmark-run` refuses a dirty tree, creates a unique run directory,
captures commit/tree/Cargo.lock SHA/rustc/`RUSTFLAGS`/host metadata
before compilation, runs the complete suite, re-captures after, refuses
sealing on material change, and writes `RUN_COMPLETE` only on full
success.  The sealer consumes only completed runs and compares the
captured values against the intended implementation commit.  Preflight
records (emitted before timing) join Criterion timing by exact benchmark
ID.  Runtime CPU features, compiled features, and executed backend
features are recorded separately.

## Tradeoffs
Gained: provenance that describes the actual measurement; impossible to
fake by copying directories.  Given up: the convenience of sealing an
arbitrary Criterion directory.

## Evidence
`xtask/src/main.rs` (`cmd_benchmark_run`, `check_performance_evidence`);
run `phase-l-20260802b` artifacts (run-manifest, host.json, commands.log,
preflight/); `docs/papers/0005-performance-methodology.md`.

## Future implications
Any change to what the run wrapper captures must be reflected in the
sealer's comparison; the binding checks (commit, tree, Cargo.lock SHA)
are the load-bearing links.
