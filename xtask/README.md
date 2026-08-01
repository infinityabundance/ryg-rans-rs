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
6. [`benchmark-run` — The Benchmark Wrapper](#benchmark-run--the-benchmark-wrapper)
7. [`courts-run` — Phase L Behavioural Courts](#courts-run--phase-l-behavioural-courts)
8. [`docker` — The Docker VM Matrix](#docker--the-docker-vm-matrix)
9. [Evidence Structure](#evidence-structure)
10. [Evidence-Generation Workflow](#evidence-generation-workflow)
11. [Current Limitations](#current-limitations)
12. [Troubleshooting](#troubleshooting)
13. [Versioning and Reading Order](#versioning-and-reading-order)

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
**not implemented** exit non-zero with an explicit message — they never print
success.

| Command | Status | What it does |
|---------|--------|--------------|
| `cargo xtask gen` | ⚠️ not implemented | Documented as "Generate documentation"; exits 1. |
| `cargo xtask check` | ✅ implemented | Pre-release smoke gates (no-FFI, no-upstream-source, forbid-unsafe, docs/drafts, core test count, no-overclaim, docker matrix informational). |
| `cargo xtask seal` | ✅ implemented | **The authoritative final gate** — see [below](#seal--the-authoritative-final-gate). |
| `cargo xtask performance-seal [--criterion-dir D] [--run-dir D] [--implementation-commit H]` | ✅ implemented | Turns a Criterion tree into performance manifests, receipts, and index under the run directory. |
| `cargo xtask benchmark-run [--criterion-dir D] [--run-dir D] -- [cargo bench args]` | ✅ implemented | Provenance-bound benchmark wrapper — refuses a dirty tree, captures before/after metadata, runs the suite, writes `RUN_COMPLETE` only on full success. |
| `cargo xtask courts-run [--implementation-commit H] [--only ID]` | ✅ implemented | Runs the fourteen Phase L behavioural courts, writes manifests/receipts, updates `evidence/index.json` + parity citations, regenerates the README table, and runs the full seal. |
| `cargo xtask no-ffi` | ✅ implemented | Standalone FFI scan: `cargo tree -p ryg-rans-rs --invert -e no-dev`, rejects any FFI-keyworded dependency. |
| `cargo xtask no-upstream-source` | ✅ implemented | Standalone scan of production crate `src/` trees for upstream-source inclusion patterns. |
| `cargo xtask no-overclaim` | ✅ implemented | Scans READMEs/docs/source for forbidden overclaim language (L.15). |
| `cargo xtask package-audit` | ⚠️ not implemented | Documented as "Verify cargo package"; exits 1. (The seal's publication dry-run gate covers packaging.) |
| `cargo xtask residuals ...` | ⚠️ not implemented | Documented as "List/verify residuals"; exits 1. (Residual accounting is enforced inside `seal`.) |
| `cargo xtask docker [RUN_ID]` | ✅ implemented | Runs `docker/bootstrap-docker.sh` (passes `RUN_ID` through if given). |

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
7. **no-overclaim** — the forbidden-phrase scan (L.15).
8. **Docker matrix** — checks `evidence/docker-matrix.json`; **informational in
   `check`** (a failure is reported but does not block).

`check` is a smoke gate, not the seal. Only `seal` is authoritative.

---

## `seal` — The Authoritative Final Gate

`cargo xtask seal` is the **single authoritative final gate**. No claim in the
READMEs may be marked **Sealed** until this gate passes fully. It is designed to
**never print success for a skipped verification**: every check either verifies
for real or reports that the artifact carries no verifiable field — it never
prints "verified" after skipping.

The gate runs, in order:

| # | Gate | What it checks |
|---|------|----------------|
| 0 | **Dirty-tree** | `git status --porcelain=v1`; rejects uncommitted changes to covered paths. Exempted: `evidence/`, `docs/`, `docs-src/models/parity.model.json`, `Cargo.lock`, `.gitignore`, any `*/README.md`, `README.md`, `xtask/README.md`. |
| 1 | **Workspace check** | `cargo check --workspace` exits 0. |
| 2 | **Core tests** | `cargo test -p ryg-rans-rs-core` passes. |
| 3 | **Workspace tests** | `cargo test --workspace` passes (L.20 gate 3). |
| 4 | **Parity model valid** | `docs-src/models/parity.model.json` is well-formed JSON. |
| 5 | **Upstream exists** | `docs-src/models/upstream.json` is present. |
| 6 | **Claims have receipts** | Every claim with `behavior_status: "full"` cites a non-empty `receipt` ID. |
| 7 | **Court path valid** | Each cited receipt's `court_path` matches its variant (`DIVISION`/`RECIPROCAL` for byte/r64, `DIVISION` for word). |
| 8 | **Receipts exist + verdict** | Every cited `evidence/receipts/receipt-<id>.json` exists, has verdict `admitted_match`, `num_cases` > 0, non-empty `manifest_sha256`, `pairs_matched == pairs_compared`. |
| 9 | **Phase L courts** | The parity model's `phase_l_courts` array equals the expected 14 IDs; every Phase L receipt has verdict `passed`, zero failed cases, a manifest hash, and an implementation commit that is an ancestor of HEAD. |
| 10 | **Index ↔ parity model** | Every index receipt is cited in the parity model and every cited receipt is in the index (bidirectional). |
| 11 | **Receipt SHA-256** | Every index entry's `sha256` equals the SHA-256 of the receipt file; each receipt's `code_commit` is an ancestor of `HEAD`. |
| 12 | **Manifest SHA-256** | Every manifest's SHA-256 equals the receipt's `manifest_sha256`. |
| 13 | **Receipt self-hash** | Phase L receipt self-hashes are verified with the typed struct (field order preserved, `receipt_sha256` emptied). Legacy oracle receipts (Phase A-G, two serialization schemes) are reported as "no verifiable canonical scheme" — never falsely verified (L1-R). |
| 14 | **Source freshness** | `git diff --name-only <code_commit>..HEAD`; only `evidence/`, `docs/`, `docs-src/`, `xtask/`, `docker/`, `.cargo/`, `.gitignore`, root `Cargo.toml`/`Cargo.lock`, `README.md`, crate `Cargo.toml` version bumps, and the listed crate READMEs may differ. Oracle-crate source changes require a reseal. |
| 15 | **Forbid unsafe** | `#![forbid(unsafe_code)]` in core and casefile crate roots. |
| 16 | **Docker matrix** | `evidence/docker-matrix.json`: schema ≥ 2, `run_id`, `git_commit` (must prefix-match the evidence index `code_commit`), `all_passed`, `job_count == 11`, every expected job present with `exit_code == 0` and a non-empty `log_sha256`. |
| 17 | **Performance evidence** | Top-level index (10 entries, exact set equality), active run dir, run-index SHA-256, run-manifest commit + Cargo.lock SHA-256 binding (L1-F), every receipt final-file + canonical self-hash, manifests, results JSON/CSV, host.json/commands.log hashes, raw Criterion archive, sealed verdicts, sample-count minimums, finite numerics, ordered confidence intervals, backend identity. |
| 18 | **README evidence counts** | The README Evidence Status totals row matches the behavioural and performance indexes (generated, never hand-edited). |
| 19 | **Unsafe ledger** | `cargo test -p ryg-rans-rs-simd --test unsafe_ledger` (bidirectional source↔ledger equality). |
| 20 | **Disassembly courts** | `cargo test -p ryg-rans-rs-simd --test disasm_court`. |
| 21 | **No unexpected binaries** | No `*.o/*.a/*.so/*.dylib/*.dll/*.exe/*.profraw/*.profdata/*.rlib/*.rmeta` outside `target/`/`evidence/`/`oracle/adapter`. |
| 22 | **Crate version consistency** | All publishable crates share one version. |
| 23 | **Cargo.lock consistency** | `Cargo.lock` is present and contains the workspace crates. |
| 24 | **Overclaim language** | No forbidden overclaim phrase anywhere (L.15/L.20 gate 40). |
| 25 | **Publication dry-run** | `cargo package -p <each publishable crate> --allow-dirty --no-verify` (L.20 gate 35). |
| 26 | **Documentation links** | Markdown links in READMEs and `docs/` resolve (L.20 gate 36). |
| 27 | **rustdoc warnings** | `cargo doc --workspace --no-deps` emits no warnings (L.20 gate 37). |
| 28 | **README doctests** | `cargo test -p ryg-rans-rs --doc` passes (L.20 gate 38). |
| 29 | **Public API inventory** | `docs/public-api/*.txt` exists for every publishable crate (L.20 gate 39). |
| 30 | **Residual accounting** | Any `OPEN` residual in the ledger's L.19/L.20 sections blocks the seal (L.20 gate 28). |

Any accumulated warning fails the command with a non-zero exit.

---

## `performance-seal` — Performance Evidence

`cargo xtask performance-seal` converts a Criterion measurement tree into
performance evidence. **It does not run benchmarks** — the Criterion suite runs
first via `cargo xtask benchmark-run` (below), which also captures the preflight
sidecar records this exporter joins against.

### Arguments

| Flag | Default | Meaning |
|------|---------|---------|
| `--criterion-dir <dir>` | `target/criterion` | Criterion output tree to consume |
| `--run-dir <dir>` | `evidence/performance` | Where to write the run evidence |
| `--implementation-commit <hash>` | git HEAD | Commit the measured implementation |

`run_id` is always the current git HEAD hash; `host_id` is `<hostname>-<arch>`.

### What it does

1. Verifies clean git state (dirty source tree → warning, recorded; evidence
   paths are expected untracked).
2. Collects host metadata (`/proc/cpuinfo`, SMT state, governor, rustc, Criterion
   version, `RUSTFLAGS`) into a hashed JSON; hashes the exact run-dir
   `host.json` file bytes when present (L1-H).
3. Loads Criterion results via
   `ryg_rans_rs_bench::exporter::load_criterion_estimates` — benchmark identity
   comes from `benchmark.json` (`full_id`), sample counts from `sample.json`,
   byte counts from `throughput.Bytes`/`Elements` (L1-B/L1-C/L18-D).
4. Joins every Criterion estimate with its benchmark-run **preflight record**
   (L1-D): missing, duplicate, or failed preflight is a hard error; backend
   identity, output hashes, words-consumed, and final-state hashes come from
   the preflight, never fabricated.
5. Groups records into the **10 expected performance surfaces**
   (`RYG_RANS.PERF.BYTE`, `.R64`, `.WORD.SCALAR`, `.ALIAS`, `.SSE41.INTERLEAVED8`,
   `.AVX512VL.INTERLEAVED8`, `.AVX512.INTERLEAVED16`, `.PHASE_H`, `.PHASE_J.AVX2`,
   `.PHASE_I.PARALLEL`); unclassified IDs are warnings.
6. Creates `<run-dir>/{manifests,receipts}` and writes per-surface canonical
   `results.json`/`results.csv`.
7. Archives the Criterion tree to `<run-dir>/criterion.tar.zst` with the `tar`
   crate (PAX long-name support — paths are never truncated; L1-K).
8. Hashes every artifact: the exact `commands.log` bytes (an empty or missing
   commands log is a seal-visible defect, L1-G), the exact `host.json` bytes,
   the archive, results, and each receipt's final file bytes.
9. Writes one `PerformanceManifest` per surface (all cases, artifact hashes,
   host metadata, `dirty_tree` flag).
10. Writes one `PerformanceReceipt` per surface with typed verdicts
    (`SealedMeasurement`/`SealedWithResiduals`/`Rejected`), a canonical
    self-hash, and a final-file hash (L1-L/L1-M).
11. Writes `<run-dir>/index.json` (`PerformanceIndex`).
12. Validates set equality: expected IDs == receipt IDs == manifest IDs == index
    IDs, and every receipt's `manifest_sha256` against the actual manifest.
13. Verifies receipt self-hashes for real (zero the field, re-serialize with the
    typed struct, re-hash).
14. Prints a per-surface summary table; **any accumulated warning fails the
    command**.

---

## `benchmark-run` — The Benchmark Wrapper

`cargo xtask benchmark-run` is the provenance-bound wrapper that executes the
Criterion suite (residuals L1-E/L1-F). `performance-seal` consumes only runs it
produced.

```sh
RUSTFLAGS="-C target-cpu=native" cargo xtask benchmark-run \
  --criterion-dir target/criterion \
  --run-dir evidence/performance/runs/<run-id> \
  -- --bench byte_rans --bench r64 --bench alias --bench scalar --bench sse41 \
     --bench avx2 --bench avx512 --bench parallel --bench specialized
```

It:

1. **Refuses a dirty tree** (any uncommitted change, including `evidence/`).
2. Establishes a run identity and **clears/isolates the Criterion tree** so stale
   measurements from earlier runs cannot leak into the export.
3. Captures metadata **before compilation**: commit, tree SHA, `Cargo.lock`
   SHA-256, rustc identity, `RUSTFLAGS`, bench args, timestamp; writes
   `run-manifest.json`.
4. Writes `host.json`, `cpuinfo.txt`, `rustc-vV.txt`, and `environment.json` as
   artifacts (L1-H/L1-I/L1-J).
5. Runs the bench suite with `RYG_RANS_PREFLIGHT_DIR` pointing at the run's
   `preflight/` directory — every benchmark case emits a
   `BenchmarkPreflightRecord` sidecar before timing (L1-D).
6. Captures metadata **after execution**; refuses to proceed if the tree or
   `Cargo.lock` changed materially during the run.
7. Writes `commands.log` (workdir, command line, rustflags, preflight dir, start,
   exit status, finish, post-run tree SHA) — never empty (L1-G).
8. Writes `RUN_COMPLETE` **only after every benchmark finished successfully**.

---

## `courts-run` — Phase L Behavioural Courts

`cargo xtask courts-run [--implementation-commit H] [--only ID]` executes the
fourteen Phase L behavioural courts (real code paths, per-case verdicts):

- `RYG_RANS.L.VERIFY.DECODED_HASH` — decoded-hash integrity matrix (L.2)
- `RYG_RANS.L.INTEGRITY.STRICT` — strict vs compatibility integrity policy
- `RYG_RANS.L.CANCEL.COMPLETENESS` — cancellation completeness counters (L.3)
- `RYG_RANS.L.EXECUTOR.BOUNDED` — bounded live pipeline, input/output budgets (L.4)
- `RYG_RANS.L.REORDER.ATOMIC_COMMIT` — atomic reorder commit, exhaustive perms (L.5)
- `RYG_RANS.L.CONFIG.WIRING` — every `ParallelConfig` field observable (L.6)
- `RYG_RANS.L.SCRATCH.INTEGRATION` — `WorkerScratch` in production (L.7)
- `RYG_RANS.L.MODEL_CACHE.INTEGRATION` — `ModelCache` hits/eviction/equivalence (L.8)
- `RYG_RANS.L.BACKEND.EXPLICIT` — exact backend semantics, format matrix (L.9)
- `RYG_RANS.L.SSE41.UNSAFE_QUARANTINE` — ledger equality, target features (L.10)
- `RYG_RANS.L.PERFORMANCE.EXPORT` — exporter canonical-identity correctness
- `RYG_RANS.L.PERFORMANCE.ARCHIVE` — deterministic tar round-trip, traversal
- `RYG_RANS.L.PERFORMANCE.RECEIPT_CHAIN` — dual receipt hashes, run index chain
- `RYG_RANS.L.PUBLIC_API.REACHABILITY` — no disconnected public API (L.13)

It writes `evidence/manifests/manifest-<id>.json` and
`evidence/receipts/receipt-<id>.json` (typed verdicts, canonical self-hash),
upserts `evidence/index.json`, updates the parity model `phase_l_courts`
citations, regenerates the README Evidence Status table from the indexes, and
then runs the full seal gate. It refuses a dirty source tree first.

---

## `docker` — The Docker VM Matrix

`cargo xtask docker [RUN_ID]` executes `docker/bootstrap-docker.sh`, which drives
`docker/compose/matrix.yml` — an **11-service** matrix (oracle-gcc,
rust-stable-tests, rust-musl-build, package-audit, cross-court, miri, msrv,
cross-aarch64, sanitizers, performance, parallel-stable), built from the
Dockerfiles in `docker/dockerfiles/`. The run stamps
`evidence/docker-matrix.json`, which the seal gate verifies (gate 16). The matrix
is mandatory evidence: `seal` fails without a valid stamp whose `git_commit`
prefix-matches the evidence index `code_commit` — the matrix must run from the
exact source commit that produced the evidence.

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
│   ├── index.json              ← canonical top-level index (active run, run-index SHA-256, dual receipt hashes)
│   └── runs/
│       └── <run-id>/
│           ├── index.json          ← PerformanceIndex
│           ├── criterion.tar.zst   ← archived Criterion raw tree
│           ├── manifests/          ← manifest-<PERF_ID>.json (10 surfaces)
│           ├── receipts/           ← receipt-<PERF_ID>.json (10 surfaces)
│           ├── preflight/          ← per-case BenchmarkPreflightRecord sidecars
│           ├── run-manifest.json   ← before/after provenance (commit, tree, lock SHA, rustc, RUSTFLAGS)
│           ├── host.json, cpuinfo.txt, rustc-vV.txt, environment.json
│           ├── commands.log        ← execution-order command log (never empty)
│           ├── RUN_COMPLETE        ← written only after full success
│           └── RYG_RANS.PERF.*/    ← per-surface results.json / results.csv
├── docker-matrix.json          ← 11-job matrix stamp (seal gate 16)
└── phase-l/
    ├── gap-ledger.md           ← every residual, severity, status, resolution commit
    ├── baseline/               ← Phase L.0 baseline metadata
    └── comparative/            ← Phase L.14 court artifacts (criterion/ tree)
```

Hash chains: the behavioural chain is `index → receipt (manifest_sha256) →
manifest`; performance artifacts carry their own per-artifact SHA-256s, a
receipt canonical self-hash, and a final-file hash, joined through the run index
to the canonical top-level index (L1-L/L1-P).

---

## Evidence-Generation Workflow

```sh
# 1. Build the oracle adapter (behavioural evidence)
cd oracle/adapter && make

# 2. Generate behavioural evidence (128 oracle courts; merges, never replaces)
RANS_EVIDENCE_DIR=evidence cargo run -p ryg-rans-rs-oracle -- \
    oracle/adapter/rans_trace 12 42

# 3. Generate the 16 AVX512 Phase G receipts (host must support AVX512)
RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
    cargo run --release -p ryg-rans-rs-oracle --bin run-phase-g -- \
    oracle/adapter/rans_trace

# 4. Run the Docker matrix (mandatory for seal; must run at the evidence commit)
cargo xtask docker

# 5. Performance evidence: provenance-bound benchmark run, then seal
RUSTFLAGS="-C target-cpu=native" cargo xtask benchmark-run \
  --criterion-dir target/criterion \
  --run-dir evidence/performance/runs/<run-id> \
  -- --bench byte_rans --bench r64 --bench alias --bench scalar --bench sse41 \
     --bench avx2 --bench avx512 --bench parallel --bench specialized
cargo xtask performance-seal --criterion-dir target/criterion \
  --run-dir evidence/performance/runs/<run-id> \
  --implementation-commit "$(git rev-parse HEAD)"

# 6. Run the fourteen Phase L behavioural courts (also regenerates README)
cargo xtask courts-run --implementation-commit "$(git rev-parse HEAD)"

# 7. The authoritative final gate — must pass fully before any "Sealed" claim
cargo xtask seal
```

The seal gate is the single authoritative final gate; it must pass fully before
any "Sealed" claim is made, and it never prints success for skipped verifications.

---

## Current Limitations

1. `gen`, `package-audit`, and `residuals` are documented but unimplemented
   (they exit non-zero with an explicit message). The seal gate covers packaging
   (publication dry-run) and residual accounting internally.
2. The `check` command's Docker-matrix check is informational only; the seal's
   is mandatory.
3. Legacy oracle behavioural receipts (Phase A-G) predate a single canonical
   serialization scheme; the seal reports them as "no verifiable canonical
   scheme" rather than verifying (L1-R) — Phase L court receipts ARE verified.
4. Hardware performance counters (perf) are unavailable on the reference host;
   component isolation uses software decomposition (L17-C, accepted).

All residuals are tracked in
[`evidence/phase-l/gap-ledger.md`](../evidence/phase-l/gap-ledger.md) — residuals
are never deleted; they are resolved or accepted.

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `seal` fails: `dirty working tree: uncommitted change to '...'` | Commit or stash changes to covered source files. READMEs, docs, evidence, Cargo.lock, parity model, and .gitignore are exempt. |
| `seal` fails: `docker-matrix.json ... job_count=10 (expected 11)` | The matrix stamp is stale; rerun `cargo xtask docker` (11 services) at the evidence commit. |
| `seal` fails: `docker-matrix.json git_commit=... does not match evidence code_commit` | The Docker matrix must run from the exact source commit that produced the evidence (short SHA prefix match). Check out the evidence commit and re-run. |
| `seal` fails: `run-manifest Cargo.lock SHA-256 ... does not match` | `Cargo.lock` changed after the benchmark run; re-run the benchmark suite at the sealed commit. |
| `seal` fails: `receipt ... SHA-256 mismatch` | A receipt file was edited by hand or the index is stale. Regenerate evidence (files under `evidence/` are machine-generated — never hand-edit). |
| `seal` fails: `source file changed after code_commit ...` | Covered source changed since the evidence commit; reseal or accept only via the allowlist. |
| `seal` fails: `open residuals in the L.19/L.20 sections` | Update the gap ledger: every residual this seal implements must be resolved (or explicitly accepted) before sealing. |
| `performance-seal` fails with accumulated warnings | Any warning (empty surface, unclassified IDs, dirty tree, hash mismatch, missing archive, missing preflight) fails the command — fix the underlying run and retry. |
| `benchmark-run` refuses a dirty tree | Commit or stash everything (including `evidence/`) before running; the wrapper requires a fully clean tree. |
| `error: gate not implemented: ...` | The command is one of `gen`, `package-audit`, `residuals` — intentionally unimplemented (covered inside `seal`). |
| `error: unknown command: ...` | Typo; see the usage text printed by `cargo xtask` with no arguments. |

---

## Versioning and Reading Order

- **Package version**: 0.1.0 (workspace crates are 0.2.0). Workspace-internal;
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

*Part of the ryg-rans-rs project. Package version 0.1.0. Phase L.20 documentation pass.*
