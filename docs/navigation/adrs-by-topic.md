# ADRs by Topic (N.9)

> The design decision explorer.  All sixteen ADRs grouped by concern.
> Reading a group gives the full decision history for that concern.

## Performance

* [ADR-0002 — Reciprocal-multiply fast path with the exact upstream bias](../adr/0002-reciprocal-fast-path.md)
  (division-free encode; the bias is part of the bitstream contract)
* [ADR-0009 — Model cache caches the expensive artifact](../adr/0009-model-cache-expensive-artifact.md)
  (the packed table is Arc-shared; the cache is consulted and effective)

## SIMD

* [ADR-0003 — Word coder pinned at scale 12 with a 4096-slot packed table](../adr/0003-word-scale-pinned.md)
* [ADR-0011 — Unsafe quarantine: local `#[target_feature]` + machine-checked ledger](../adr/0011-unsafe-quarantine.md)

## Parallel

* [ADR-0004 — The bounded live executor](../adr/0004-bounded-live-executor.md)
* [ADR-0005 — Deterministic error selection (lowest block index)](../adr/0005-canonical-error.md)
* [ADR-0007 — Cancellation completeness enforced at the public-API boundary](../adr/0007-cancellation-completeness-boundary.md)
* [ADR-0013 — Configuration discipline: every public field has an observable effect](../adr/0013-configuration-discipline.md)
* [ADR-0014 — ReorderBuffer atomic commit batches](../adr/0014-reorder-atomic-commit.md)
* [ADR-0015 — Per-worker exclusive scratch](../adr/0015-per-worker-scratch.md)

## Safety

* [ADR-0006 — Strict decoded-output integrity as the default](../adr/0006-strict-integrity-default.md)
* [ADR-0011 — Unsafe quarantine](../adr/0011-unsafe-quarantine.md)

## Evidence

* [ADR-0010 — Evidence is captured at benchmark time, verified at seal time](../adr/0010-benchmark-time-capture.md)

## Documentation

* (Phase M/N conventions: `docs/philosophy.md`, `docs/layers.md`, and the
  documentation seal gates are the "documentation decisions" — see
  `docs/navigation/inventory.md`.)

## Architecture

* [ADR-0001 — Byte-exact reconstruction of the pinned upstream `ryg_rans`](../adr/0001-format-contract.md)
* [ADR-0008 — Exact backend semantics: no silent fallback](../adr/0008-exact-backend-semantics.md)

## CLI

* [ADR-0006 — Strict integrity default](../adr/0006-strict-integrity-default.md)
  (the CLI's exit-code-5 behaviour)
* (CLI cancellation: exit code 11, `signals` feature — recorded in the
  gap ledger L3-D and the CLI README)

## Testing / Proofs

* [ADR-0007 — Completeness boundary](../adr/0007-cancellation-completeness-boundary.md)
  (the test that pins it: pre-cancelled token → `Cancelled { 0, N }`)
* [ADR-0002 — Reciprocal path](../adr/0002-reciprocal-fast-path.md)
  (the Kani instances that prove it)
* (Proof boundaries beyond ADRs: `docs/papers/0007-proof-philosophy.md`)

## Release

* [ADR-0012 — Versioning: 0.3.0, a pre-1.0 minor per semver-checks](../adr/0012-versioning-030.md)
  (the same process produced 0.4.0 — see the gap ledger L22-D)
