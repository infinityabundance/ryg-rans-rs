# Paper 0006 — The evidence system: receipts, manifests, seals

> *Layer: Subsystem.  Companion: `docs/residual-doctrine.md`,
> `docs/oracle-method.md`, `docs/glossary.md` (exact terms),
> `docs/papers/0005-performance-methodology.md`.  Code:
> `crates/ryg-rans-rs-casefile/` (schema), `crates/ryg-rans-rs-oracle/`
> (behavioural courts), `xtask/` (seal gates).*

## 1. Why evidence

The frozen invariant of this repository is that a claim is true only when
traceable through **code → test → court → receipt → seal**.  The evidence
system is the machinery that makes that trace mechanically checkable:
artifacts are hashed, chains are joined by exact IDs, and the seal gate
fails rather than printing "verified" for a skipped check.

## 2. Behavioural evidence

### 2.1 The courts

Courts are deterministic programs that exercise a real code path against a
reference (the upstream C oracle for codec parity; the pinned invariant for
integrity/cancellation/boundedness courts).  Each court produces a
**manifest** (what was run: inputs, expected results) and a **receipt**
(what happened: per-case verdicts, actual results, residual references,
implementation commit, artifact hashes).

### 2.2 The receipt chain

```text
court_id ─▶ manifest-<id>.json   (case list, expected results)
         ─▶ receipt-<id>.json    (per-case verdicts, actual results,
                                  manifest_sha256, implementation_commit,
                                  evidence_commit)
         ─▶ evidence/index.json  (court_id → receipt file SHA-256)
```

The seal verifies:

* every receipt file exists and its SHA-256 matches the index;
* every manifest SHA-256 inside its receipt matches the manifest file;
* every cited receipt's verdict is the required one
  (`admitted_match` for oracle courts, `passed` for Phase L courts);
* `code_commit`/`implementation_commit` is an ancestor of HEAD and no
  covered source changed after it;
* Phase L receipts verify their **canonical self-hash** (the receipt's
  content hashed with the self-hash field omitted — the L1-R scheme);
  legacy oracle receipts are reported as "no verifiable canonical scheme"
  rather than falsely "verified".

## 3. Performance evidence

### 3.1 The run wrapper

`cargo xtask benchmark-run` produces a run directory:

```text
evidence/performance/runs/<run-id>/
  run-manifest.json   commit, tree SHA, Cargo.lock SHA-256, rustc, RUSTFLAGS
  host.json           CPU model, features, microcode, SMT, governor, kernel…
  cpuinfo.txt         raw /proc/cpuinfo
  rustc-vV.txt        compiler identity
  environment.json    RUSTFLAGS, SMT state, governor, CPU count
  commands.log        workdir, command line, start/exit/finish, artifact paths
  preflight/          one BenchmarkPreflightRecord per benchmark case
  criterion.tar.zst   the deterministic raw Criterion tree (tar crate, PAX,
                      sorted, round-trip verified)
  RUN_COMPLETE        written only after every benchmark finished
```

### 3.2 The seal step

`cargo xtask performance-seal` exports 10 manifests + 10 receipts from the
Criterion metadata and preflight records, then the **canonical top-level
index** `evidence/performance/index.json` identifies the active run and
hashes the run-local index.

### 3.3 The dual-hash receipt model

Every performance receipt carries two hashes (L1-L doctrine):

* `receipt_file_sha256` — SHA-256 of the exact final receipt bytes on disk;
* `receipt_canonical_sha256` — SHA-256 of the canonical receipt content
  with its self-hash field omitted.

The index verifies both.  These are never conflated — a canonical self-hash
is not a file hash.

### 3.4 Verdicts are typed

`PerformanceReceiptVerdict` is `SealedMeasurement` |
`SealedWithResiduals` | `Rejected`; benchmark case statuses are typed
enums.  Unknown serialized values are rejected by the schema.

## 4. The seal gate

`cargo xtask seal` is the single authoritative final gate (L.20).  Its
gates, in order: clean working tree; workspace build; all tests; required
feature matrices; unsafe inventory; unsafe-ledger equality; behavioural
receipt index, file hashes, canonical self-hashes, manifest hashes;
performance top-level index, run index, receipt file + canonical hashes,
manifest hashes, raw archive integrity + round-trip, results JSON/CSV
hashes, host metadata, command log, preflight records, backend identity,
thread identity, sample counts, throughput calculations; evidence-model
citations with exact set equality; residual accounting; README generated
status; source freshness; Docker matrix; no unexpected binary artifacts;
crate version consistency; Cargo.lock consistency; publication dry-run;
documentation links; rustdoc warnings; README doctests; public API semver
report; no forbidden overclaim language.

The gate fails on any warning affecting evidence validity, and it **never
prints success for a skipped verification** (the L1-R rule: verify, or
report that the schema has no verifiable self-hash — never both).

## 5. Residuals

A residual is a recorded defect: severity, affected files, reproduction,
expected/actual behaviour, proposed fix, test requirement, evidence
requirement, resolution commit.  Residuals are **never deleted**; they are
resolved (fix + tests + evidence) or accepted with justification (the
ledger's OPEN/PARTIAL entries are accepted limitations, e.g. L16-D
sanitizer coverage, L16-E the two intractable Kani instances, L17-C
unavailable hardware counters).  The gap ledger
(`evidence/phase-l/gap-ledger.md`) is the index; the seal's residual
accounting fails on any *active* performance-evidence residual.

## 6. Docker: the cross-toolchain matrix

The Docker matrix (11 jobs: oracle-gcc, package-audit, msrv,
cross-aarch64, rust-musl-build, sanitizers, rust-stable-tests, cross-court,
miri, performance, parallel-stable) builds and tests the repository from an
immutable source snapshot at the exact implementation commit, in
disposable containers with dropped capabilities and no network.  The stamp
(`evidence/docker-matrix.json`) records run ID, git commit (short SHA,
prefix-matched against the evidence `code_commit`), job exit codes, and log
hashes.  The seal requires 11/11 jobs with exit 0.

## 7. Provenance and publication

Provenance is captured at benchmark time, not seal time: the run wrapper
records the source identity before compilation, and the sealer compares it
against the intended commit — a Criterion directory copied from another
machine or generated from a dirty tree fails the binding.  Publication
(the L.22 process) publishes in dependency order from the exact sealed
commit, with `cargo package --list` audited for build artifacts, the
publication dry-run resolving only after dependencies are live, and the
release tag pointing at the sealed evidence commit.  The benchmark crate's
publication status is explicit (`publish = false`).

## 8. The evidence model

`docs-src/models/parity.model.json` is the machine-readable map of surfaces
→ claims → receipts, with `performance_status` and the `phase_l_courts`
citation list.  The seal requires **exact set equality** between expected
performance IDs, the evidence-model citations, the canonical performance
index IDs, the receipt file IDs, the manifest file IDs, the result-directory
IDs, and the preflight IDs.  Any mismatch is a seal failure — no partial
credit, no "close enough".
