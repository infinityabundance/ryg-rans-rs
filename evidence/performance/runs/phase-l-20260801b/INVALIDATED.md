# INVALIDATED run — phase-l-20260801b

This benchmark run is **invalidated** and must not be treated as sealed
performance evidence. It is retained for the historical record only.

## Why it is invalid

The run included the `batch`, `container`, and `dispatch` Criterion benches,
which do **not** emit structured `BenchmarkPreflightRecord` sidecar records
(residual doctrine: every timed case must have a joined preflight record
before it can be sealed).

`cargo xtask performance-seal` therefore rejected the run:

```text
load_criterion_estimates failed: ... no preflight record
```

The exporter refuses any Criterion estimate without a matching preflight
record (L1-D: verification is never hardcoded; missing preflight is a hard
error, not a default).

## What the run contained

| Artifact | Present |
|---|---|
| `run-manifest.json` (commit `e640965`, tree `79e8a6e5…`) | yes |
| `commands.log` (12 benches, exit 0) | yes |
| `host.json`, `cpuinfo.txt`, `rustc-vV.txt`, `environment.json` | yes |
| `preflight/` (800 records) | yes |
| `RUN_COMPLETE` marker | yes |
| `criterion.tar.zst` | **no** (seal aborted before archiving) |
| receipts / manifests / index | **no** |

## Replacement

The valid full-precision run that supersedes this one is:

```text
evidence/performance/runs/phase-l-20260801c/
```

bound to the same implementation commit `e640965` with only the nine
preflight-emitting benches (`byte_rans`, `r64`, `alias`, `scalar`, `sse41`,
`avx2`, `avx512`, `parallel`, `specialized`).

The `batch` / `container` / `dispatch` benches remain outside the sealed
performance surface (their cases do not map onto the ten performance
receipts and they emit no preflight).
