# xtask

> Build-system and evidence automation for ryg-rans-rs.
> `cargo xtask <command>` — the seal gate here is the **single authoritative final
> gate** for every "Sealed" claim in the project.

**Package version: 0.1.0** (workspace edition 2024, workspace-internal, never published)

---

## Table of Contents

1. [What xtask Is](#what-xtask-is)
2. [Command Inventory](#command-inventory)
3. [`check` — Pre-Release Gates](#check--pre-release-gates)
4. [`seal` — The Authoritative Final Gate](#seal--the-authoritative-final-gate)
5. [`performance-seal` — Performance Evidence](#performance-seal--performance-evidence)
6. [`benchmark-run` — Phase L.18 (planned)](#benchmark-run--phase-l18-planned)
7. [`docker` — The Docker VM Matrix](#docker--the-docker-vm-matrix)
8. [Evidence Structure](#evidence-structure)
9. [Evidence-Generation Workflow](#evidence-generation-workflow)
10. [Current Limitations (L.15/L.18)](#current-limitations-l15l18)
11. [Troubleshooting](#troubleshooting)
12. [Versioning and Reading Order](#versioning-and-reading-order)

---

## What xtask Is

xtask is a small cargo subcommand binary (`xtask/src/main.rs`) that runs the
project's automation gates and evidence tooling. It is a workspace member
(`[workspace]` in the root `Cargo.toml`), so it is invoked as `cargo xtask
<command> [args]`.

It depends on `ryg-rans-rs-bench` (the Criterion exporter), `ryg-rans-rs-casefile`
with the `std` feature (the performance-evidence types), plus `serde`, `serde_json`,
`sha2`, and `zstd` (for the Criterion archive). It does **not** depend on any rANS
arithmetic crate.

---

## Command Inventory

Every command in this table exists in `xtask/src/main.rs` today. Commands marked
**not implemented** exit non-zero with an explicit message.

| Command | Status | What it does |
|---------|--------|--------------|
| `cargo xtask gen` | ⚠️ not implemented | Documented as "Generate documentation"; exits 1. |
| `cargo xtask check` | ✅ implemented | Pre-release smoke gates (no-FFI, no-upstream-source, forbid-unsafe, docs/drafts, core test count, docker matrix informational). |
| `cargo xtask seal` | ✅ implemented | **The authoritative final gate** — see [below](#seal--the-authoritative-final-gate). |
| `cargo xtask performance-seal [--criterion-dir D] [--run-dir D] [--implementation-commit H]` | ✅ implemented | Turns a Criterion tree into performance manifests, receipts, and index under the run directory. |
| `cargo xtask benchmark-run ...` | 🚧 **planned, Phase L.18** | Does **not exist** in the current source. The Phase L.18 pipeline introduces a wrapper that runs the Criterion suite and captures preflight provenance at benchmark time (residuals L1-A…L1-N). Do not invoke it yet. |
| `cargo xtask no-ffi` | ✅ implemented | Standalone FFI scan: `cargo tree -p ryg-rans-rs --invert -e no-dev`, rejects any FFI-keyworded dependency. |
| `cargo xtask no-upstream-source` | ✅ implemented | Standalone scan of production crate `src/` trees for upstream-source inclusion patterns. |
| `cargo xtask package-audit` | ⚠️ not implemented | Documented as "Verify cargo package"; exits 1. |
| `cargo xtask residuals ...` | ⚠️ not implemented | Documented as "List/verify residuals"; exits 1. |
| `cargo xtask docker [RUN_ID]` | ✅ implemented | Runs `docker/bootstrap-docker.sh` (passes `RUN_ID` through if given). |
| `cargo xtask docker preflight` | ⚠️ note | The usage text lists `docker preflight`; the dispatcher currently treats any second argument as `RUN_ID`. Verify against the bootstrap script before relying on preflight. |

---

## `check` — Pre-Release Gates

`cargo xtask check` runs, in order:

1. **no-ffi** — `cargo tree -p ryg-rans-rs --invert -e no-dev`; any line matching
   `ffi`, `libc`, `cc`, `bindgen`, or `cmake` fails the gate.
2. **no-upstream-source** — scans `crates/ryg-rans-rs-core|casefile|simd|parallel|cli|ryg-rans-rs/src`
   for `#[path = "../upstream` / `include!("../upstream` inclusion.
3. **forbid(unsafe_code) in core** — `crates/ryg-rans-rs-core/src/lib.rs`.
4. **docs/drafts exists** — `docs/drafts/` must be a directory.
5. **core test count** — runs `cargo test -p ryg-rans-rs-core -- --list`, prints
   the actual count, fails below **50**.
6. **no-ffi facade** — `crates/ryg-rans-rs/src/lib.rs` must contain
   `#![forbid(unsafe_code)]`.
7. **Docker matrix** — checks `evidence/docker-matrix.json`; **informational in
   `check`** (a failure is reported but does not block).

`check` is a smoke gate, not the seal. Only `seal` is authoritative.

---

## `seal` — The Authoritative Final Gate

`cargo xtask seal` is the **single authoritative final gate**. No claim in the
READMEs may be marked **Sealed** until this gate passes fully. It is designed to
**never print success for a skipped verification**; the one tracked exception is
noted below (L1-R / L20-A).

The gate currently runs, in order:

| # | Gate | What it checks |
|---|------|----------------|
| 0 | **Dirty-tree** | `git status --porcelain=v1`; rejects uncommitted changes to covered paths. Exempted: `evidence/`, `docs/`, `Cargo.lock`, `.gitignore`, any `*/README.md`, `README.md`, `xtask/README.md`. |
| 1 | **Workspace check** | `cargo check --workspace` exits 0. |
| 2 | **Core tests** | `cargo test -p ryg-rans-rs-core` passes. |
| 3 | **Parity model valid** | `docs-src/models/parity.model.json` is well-formed JSON. |
| 4 | **Upstream exists** | `docs-src/models/upstream.json` is present. |
| 5 | **Claims have receipts** | Every claim with `behavior_status: "full"` cites a non-empty `receipt` ID. |
| 5a | **Court path valid** | Each cited receipt's `court_path` matches its variant (`DIVISION`/`RECIPROCAL` for byte/r64, `DIVISION` for word). |
| 5b | **Receipts exist + verdict** | Every cited `evidence/receipts/receipt-<id>.json` exists, has verdict `admitted_match`, `num_cases`/`case_count` > 0, a non-empty `manifest_sha256`, and `pairs_matched == pairs_compared`. |
| 5c | **Index ↔ parity model** | Every index receipt is cited in the parity model and every cited receipt is in the index (bidirectional). |
| 5d | **Receipt SHA-256** | Every index entry's `sha256` equals the SHA-256 of the receipt file; each receipt's `code_commit` is an ancestor of `HEAD`. |
| 5e | **Manifest SHA-256** | Every manifest's SHA-256 equals the receipt's `manifest_sha256`. |
| 5f | **Receipt self-hash** | ⚠️ **Currently skipped.** The code `continue`s past self-hash verification for all receipts (the Phase A-F vs Phase G harnesses serialize differently, so hashes are incompatible). The gate then prints "all receipt SHA-256 self-hashes verified" — this is exactly the defect tracked as **L1-R** (behavioural self-hash verification falsely reports success after skipping) and **L20-A** (the gate must never print success for skipped checks). Until L.20, do not treat the self-hash line as real verification. |
| 5f′ | **Source freshness** | `git diff --name-only <earliest code_commit>..HEAD`; only `evidence/`, `docs/`, `docs-src/`, `xtask/`, `docker/`, `.cargo/`, `.gitignore`, root `Cargo.toml`/`Cargo.lock`, `README.md`, and crate `Cargo.toml` version bumps may differ. Oracle-crate source changes require a reseal. |
| 6 | **Forbid unsafe** | `#![forbid(unsafe_code)]` present in core and casefile crate roots. |
| 7 | **Docker matrix** | `evidence/docker-matrix.json`: schema ≥ 2, `run_id`, `git_commit`, `all_passed`, `job_count == 11`, every one of the 11 expected jobs (oracle-gcc, package-audit, msrv, cross-aarch64, rust-musl-build, sanitizers, rust-stable-tests, cross-court, miri, performance, parallel-stable) present with `exit_code == 0` and a non-empty `log_sha256`; `git_commit` must match the evidence index `code_commit`. |

**Phase L.20** expands the seal gate to ~40 checks, including validating
performance evidence end-to-end (top-level index, run index, receipt file hashes,
canonical self-hashes, manifests, archive integrity, preflight records,
backend/thread identity, README regeneration) — see the root README's seal table
and residual **L1-Q / L20-A**.

---

## `performance-seal` — Performance Evidence

`cargo xtask performance-seal` converts a Criterion measurement tree into
performance evidence. **It does not run benchmarks** — the Criterion suite runs
first (via `cargo bench -p ryg-rans-rs-bench` today, via the planned
`benchmark-run` wrapper in Phase L.18).

### Arguments

| Flag | Default | Meaning |
|------|---------|---------|
| `--criterion-dir <dir>` | `target/criterion` | Criterion output tree to consume |
| `--run-dir <dir>` | `evidence/performance` | Where to write the run evidence |
| `--implementation-commit <hash>` | git HEAD | Commit the measured implementation |

`run_id` is always the current git HEAD hash; `host_id` is `<hostname>-<arch>`.

### What it does (15 steps)

1. Verifies clean git state (dirty tree → warning, recorded).
2. Collects host metadata (`/proc/cpuinfo`, SMT state, governor, rustc, Criterion
   version, `RUSTFLAGS`) into a hashed JSON.
3. Loads Criterion results via `ryg_rans_rs_bench::exporter::load_criterion_estimates`.
4. Groups records into the **10 expected performance surfaces**
   (`RYG_RANS.PERF.BYTE`, `.R64`, `.WORD.SCALAR`, `.ALIAS`, `.SSE41.INTERLEAVED8`,
   `.AVX512VL.INTERLEAVED8`, `.AVX512.INTERLEAVED16`, `.PHASE_H`, `.PHASE_J.AVX2`,
   `.PHASE_I.PARALLEL`); unclassified IDs are warnings.
5. Creates `<run-dir>/{manifests,receipts}`.
6. Writes per-surface canonical `results.json`/`results.csv` (via
   `exporter::export_summary`).
7. Archives the Criterion tree to `<run-dir>/criterion.tar.zst` (zstd level 3).
   Note: the current tar writer truncates names at 99 bytes — residual **L1-K**.
8. Hashes every artifact. The **commands log is currently empty** (residual
   **L1-G**).
9. Writes one `PerformanceManifest` per surface (all cases, artifact hashes, host
   metadata, `dirty_tree` flag).
10. Writes one `PerformanceReceipt` per surface with a computed self-hash.
11. Writes `<run-dir>/index.json` (`PerformanceIndex`: implementation commit,
    run ID, host ID, receipt entries).
12. Validates set equality: expected IDs == receipt IDs == manifest IDs == index
    IDs.
13. Validates every receipt's `manifest_sha256` against the actual manifest hash.
14. Verifies receipt self-hashes (zero the field, re-serialize, re-hash).
15. Prints a per-surface summary table; **any accumulated warning fails the
    command** (non-zero exit).

---

## `benchmark-run` — Phase L.18 (planned)

The **current source has no `benchmark-run` command**. Per `AGENTS.md` and the
root README, Phase L.18 introduces it as the wrapper that runs the Criterion suite
and captures benchmark-time provenance (backend requested/executed, input/output
hashes, words consumed, final states, thread counts — the **preflight** channel
that the current exporter lacks, residuals L1-A…L1-N). The planned invocation
shape is:

```sh
cargo xtask benchmark-run --criterion-dir target/criterion \
  --implementation-commit "$(git rev-parse HEAD)"
```

followed by `cargo xtask performance-seal ...` and then `cargo xtask seal`.
Until L.18 lands, the Phase K run (`evidence/performance/runs/phase-k-20260731-004044/`)
remains archived as **superseded** evidence and no performance claim is Sealed.

---

## `docker` — The Docker VM Matrix

`cargo xtask docker [RUN_ID]` executes `docker/bootstrap-docker.sh`, which drives
`docker/compose/matrix.yml` — an **11-service** matrix (oracle-gcc,
rust-stable-tests, rust-musl-build, package-audit, cross-court, miri, msrv,
cross-aarch64, sanitizers, performance, parallel-stable), built from the
Dockerfiles in `docker/dockerfiles/`. The run stamps
`evidence/docker-matrix.json`, which the seal gate verifies (gate 7). The matrix
is mandatory evidence: `seal` fails without a valid stamp.

---

## Evidence Structure

```text
evidence/
├── index.json                  ← behavioural index: { schema_version, code_commit, receipts: [{court_id, sha256}] }
├── receipts/
│   └── receipt-<court_id>.json ← behavioural receipts (verdict, counts, manifest_sha256, receipt_sha256, ...)
├── manifests/
│   └── manifest-<court_id>.json← all cases, streams, per-case verdicts
├── performance/
│   └── runs/
│       └── <run-id>/
│           ├── index.json          ← PerformanceIndex
│           ├── criterion.tar.zst   ← archived Criterion raw tree
│           ├── manifests/          ← manifest-<PERF_ID>.json (10 surfaces)
│           ├── receipts/           ← receipt-<PERF_ID>.json (10 surfaces)
│           └── RYG_RANS.PERF.*/    ← per-surface results.json / results.csv
├── docker-matrix.json          ← 11-job matrix stamp (seal gate 7)
└── phase-l/
    ├── gap-ledger.md           ← every residual, severity, status, resolution commit
    ├── baseline/               ← Phase L.0 baseline metadata
    └── comparative/            ← Phase L.14 court artifacts (criterion/ tree)
```

Hash chains: the behavioural chain is `index → receipt (manifest_sha256) →
manifest`; performance artifacts carry their own per-artifact SHA-256s and a
receipt self-hash. The seal gate recomputes the behavioural index/receipt/manifest
hashes and the performance pipeline recomputes artifact hashes at generation time.

---

## Evidence-Generation Workflow

```sh
# 1. Build the oracle adapter (behavioural evidence)
cd oracle/adapter && make

# 2. Generate behavioural evidence (staging → atomic promotion on success)
RANS_EVIDENCE_DIR=evidence cargo run -p ryg-rans-rs-oracle -- \
    oracle/adapter/rans_trace 12 42 20

# 3. Run the Docker matrix (mandatory for seal)
cargo xtask docker

# 4. Performance evidence (Phase L.18 pipeline; benchmark-run is planned)
RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench
cargo xtask performance-seal --criterion-dir target/criterion \
  --run-dir evidence/performance/runs/<run-id> \
  --implementation-commit "$(git rev-parse HEAD)"

# 5. The authoritative final gate — must pass fully before any "Sealed" claim
cargo xtask seal
```

The seal gate is the single authoritative final gate (L.20 expands it to ~40
checks); it must pass fully before any "Sealed" claim is made, and it never
prints success for skipped verifications.

---

## Current Limitations (L.15/L.18)

1. **`benchmark-run` does not exist yet** — planned for Phase L.18; the Phase K
   performance run is superseded (residuals L1-A…L1-S).
2. **Behavioural receipt self-hash verification is skipped** in `seal` (L1-R /
   L20-A) — see gate 5f above.
3. **Commands log is empty** in `performance-seal` (L1-G); **host metadata is
   hashed but not stored as an artifact** (L1-H); **CPU features are compile-time
   cfg on the sealer binary** (L1-I); **RUSTFLAGS can be empty in manifests**
   (L1-J); the **tar writer truncates 99-byte names** (L1-K); the **index sha256
   conflates self-hash and file hash** (L1-L).
4. **`seal` does not yet validate performance evidence** (L1-Q / L20-A).
5. `gen`, `package-audit`, and `residuals` are documented but unimplemented.
6. The `check` command's Docker-matrix check is informational only.

All of the above are tracked in
[`evidence/phase-l/gap-ledger.md`](../evidence/phase-l/gap-ledger.md) — residuals
are never deleted; they are resolved or accepted.

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `seal` fails: `dirty working tree: uncommitted change to '...'` | Commit or stash changes to covered source files. READMEs, docs, evidence, Cargo.lock, and .gitignore are exempt. |
| `seal` fails: `docker-matrix.json ... job_count=10 (expected 11)` | The matrix stamp is stale; rerun `cargo xtask docker` (11 services). |
| `seal` fails: `docker-matrix.json not found` | The Docker matrix has not been run; `cargo xtask docker` is mandatory evidence. |
| `seal` fails: `test count N is below expected minimum of 50` | `cargo test -p ryg-rans-rs-core -- --list` found fewer than 50 tests. |
| `seal` fails: `receipt ... SHA-256 mismatch` | A receipt file was edited by hand or the index is stale. Regenerate evidence (files under `evidence/` are machine-generated — never hand-edit). |
| `seal` fails: `source file changed after code_commit ...` | Covered source changed since the evidence commit; reseal or accept only via the allowlist. |
| `performance-seal` fails with accumulated warnings | Any warning (empty surface, unclassified IDs, dirty tree, hash mismatch, missing archive) fails the command — fix the underlying run and retry. |
| `error: gate not implemented: ...` | The command is one of `gen`, `package-audit`, `residuals` — intentionally unimplemented. |
| `error: unknown command: ...` | Typo; see the usage text printed by `cargo xtask` with no arguments. |

---

## Versioning and Reading Order

- **Package version**: 0.1.0 (workspace crates are 0.1.30). Workspace-internal;
  never published.
- **Reading order**: root [`README.md`](../README.md) →
  [`docs/architecture.md`](../docs/architecture.md) →
  [`docs/glossary.md`](../docs/glossary.md) →
  [`evidence/phase-l/gap-ledger.md`](../evidence/phase-l/gap-ledger.md) →
  [`crates/ryg-rans-rs-bench/README.md`](../crates/ryg-rans-rs-bench/README.md) →
  [`crates/ryg-rans-rs-oracle/README.md`](../crates/ryg-rans-rs-oracle/README.md) →
  this README.
- **Ground truth**: `AGENTS.md` at the repository root states the exact test
  commands, frozen invariants, and the evidence-generation commands.

---

*Part of the ryg-rans-rs project. Package version 0.1.0. Phase L.15 documentation pass.*
