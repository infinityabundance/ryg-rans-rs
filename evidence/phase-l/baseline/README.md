# Phase L.0 — Baseline command outputs

This directory archives the actual command outputs from the frozen L.0
baseline commit `7bbf4a25d9b3b087d0abb81aba59f73788a677fe` ("Bump all crates
to v0.1.30" — the clean `origin/main` state containing the Phase K
implementation and v0.1.30 release history).

## How these were captured

Each log was produced by checking out the baseline commit into a detached
git worktree (`git worktree add /tmp/ryg-baseline 7bbf4a25`) and running the
exact verification commands from `AGENTS.md` inside it, capturing combined
stdout/stderr and the exit status.  The worktree was removed afterwards; the
main working tree was never modified.  rustc 1.96.0, cargo 1.96.0.

## Results (honest record — the baseline was not seal-clean)

| File | Command | Result |
|------|---------|--------|
| `01-cargo-check-workspace.log` | `cargo check --workspace` | EXIT 0 |
| `02-cargo-test-workspace.log` | `cargo test --workspace` | EXIT 0 (57 core + 56 parallel + 22 simd + 13 casefile + 2 + 1 ...) |
| `03-simd-tests.log` | `cargo test -p ryg-rans-rs-simd` (portable, then `-C target-cpu=native`) | EXIT 0 both (22 portable, 58 native) |
| `04-parallel-tests.log` | `cargo test -p ryg-rans-rs-parallel` | EXIT 0 (56 + 13) |
| `05-seal-gates.log` | `cargo xtask performance-seal` then `cargo xtask seal` | EXIT 1 both — **the Phase K seal was broken**: performance-seal failed (no `target/criterion`), and the main seal failed the source-freshness gate (`benches/common/corpus.rs` changed after its `code_commit`).  This is the exact defect set Phase L exists to repair (L1-A..L1-S, L20). |
| `06-fuzz-check.log` | `cargo check --workspace` in `fuzz/` | EXIT 101 — the standalone fuzz workspace did not build at baseline (repaired in Phase L, residual L16-B). |

## Relationship to the L.18 run

The L.18 benchmark wrapper (`cargo xtask benchmark-run`) is the production
command-log capture mechanism; the sealed run `phase-l-20260802a` carries its
own `commands.log`, `host.json`, `cpuinfo.txt`, `rustc-vV.txt`,
`environment.json`, and `run-manifest.json` under
`evidence/performance/runs/phase-l-20260802a/`.  This directory records the
pre-Phase-L state those artifacts were meant to replace.
