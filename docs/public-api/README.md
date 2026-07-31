# Public API inventory (Phase L.13)

Machine-readable public-surface inventory generated with `cargo public-api`
(`cargo install cargo-public-api`) at commit 7fe286b and later.  Regenerate
with:

```sh
cargo public-api -p ryg-rans-rs-parallel > docs/public-api/parallel.txt
cargo public-api -p ryg-rans-rs-core     > docs/public-api/core.txt
cargo public-api -p ryg-rans-rs-simd     > docs/public-api/simd.txt
```

## Classification rules (Phase L.13)

Every public item is classified as one of:

| Class | Meaning |
|-------|---------|
| `production-integrated` | Has a production call path inside the crate/workspace. |
| `test-only` | Exists only under `#[cfg(test)]` or is consumed only by tests. |
| `extension API` | Explicitly external purpose; contract tested via a public integration test; docs do not claim internal use. |
| `deprecated` | Kept for compatibility; documented as such. |
| `dead/unimplemented` | No call path, no external contract test — removed or implemented. |

## Phase L.13 actions taken

- **Removed `schedule.rs`** (`DelaySchedule`, `DeterministicScheduler`,
  `ScheduleMode`): documented as a test-injection harness that never
  existed; its doc examples did not compile.  Determinism is instead proven
  by thread-count differential tests and the loom model.
- **Removed `report.rs`** (`ParallelExecutionReport`, `ParallelBlockReport`):
  docs claimed "created by the executor" but the executor returns
  `ExecutorReport`; the types had no call path.  `ExecutorReport` +
  `ExecutionMetadata` are the real reporting surface.
- **`estimate_memory` / `ParallelMemoryEstimate`** (`resource.rs`): kept as a
  documented extension API for downstream capacity planning; contract now
  tested (`test_estimate_memory_is_conservative_and_finite`), which caught
  and fixed a non-saturating multiply overflow contradicting the documented
  saturating-arithmetic guarantee.
- **`decode_interleaved16_dominant_sketch`** (simd crate): documented
  placeholder that always returns an error; kept and explicitly documented
  as unimplemented (not silently executed).
- **`ExecutedDecode`, `DecodePlan`, backend semantics**: `plan_backend` +
  `backend` on results; production-integrated and evidence-producing.

## Semver status (checked at 7fe286b)

`cargo semver-checks check-release -p ryg-rans-rs-parallel` reports 9
major-level breaks against published 0.1.30 (`ExecutorTask::run` signature,
`BlockErrorKind` variant reordering, removed public items, ...).  Phase L.22
version decision: pre-1.0 minor bump (breaking changes are permitted within
0.x minor releases per Rust ecosystem convention).
