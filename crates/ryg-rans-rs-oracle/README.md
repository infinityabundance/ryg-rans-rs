# ryg-rans-rs-oracle

> **Forensic cross-decoding court harness.**
> Compares the Rust implementation against compiled C/C++ oracle binaries
> (`oracle/adapter/rans_trace.cpp`) via subprocess, and generates the SHA-256-chained
> behavioural evidence (receipts, manifests, index) under `evidence/`.

**Version: 0.2.0** · 0 tests currently · evidence generation only

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [What This Crate Does NOT Do](#what-this-crate-does-not-do)
3. [Court Surfaces](#court-surfaces)
4. [How Courts Work](#how-courts-work)
5. [Per-Case Checks](#per-case-checks)
6. [The C Oracle Adapter](#the-c-oracle-adapter)
7. [Trust Boundaries and Input Invariants](#trust-boundaries-and-input-invariants)
8. [Resource Behavior](#resource-behavior)
9. [Backend Semantics and SIMD Requirements](#backend-semantics-and-simd-requirements)
10. [Unsafe Boundaries](#unsafe-boundaries)
11. [Evidence Output](#evidence-output)
12. [Usage](#usage)
13. [Development: the `perf` Binary](#development-the-perf-binary)
14. [Limitations (honest)](#limitations-honest)
15. [Troubleshooting](#troubleshooting)
16. [Versioning and Reading Order](#versioning-and-reading-order)

---

## What This Crate Is

This crate is the **behavioural evidence generator**. It contains no rANS
arithmetic itself — it orchestrates deterministic comparisons between the Rust
implementation (via `ryg-rans-rs-core` and `ryg-rans-rs-simd`) and the compiled
C/C++ oracle binary built from `oracle/adapter/`, then writes court **receipts**,
**manifests**, and an **index** that the seal gate (`cargo xtask seal`) verifies.

### Why Process-Level Comparison Instead of FFI?

1. **No unsafe FFI boundary in the harness**: the oracle crate is safe Rust —
   no `extern "C"`, no raw pointers into foreign types.
2. **No C/C++ build dependency for the workspace**: the crate compiles without a
   C/C++ toolchain; the oracle is only needed when generating evidence.
3. **Independent binaries**: process-level comparison proves two independently
   compiled programs agree on the **canonical output** byte-for-byte.
4. **Auditability**: anyone can rebuild the upstream C themselves
   (`cd oracle/adapter && make`) and rerun the courts.

---

## What This Crate Does NOT Do

- **It has no tests.** The crate currently ships **zero `#[test]` functions**
  (verified against `src/`). Its role is evidence generation, and the courts
  themselves are the verification surface — but that is not the same as unit-test
  coverage, and this README does not claim any.
- **It does not seal anything.** Evidence generation is not sealing; the
  authoritative gate is `cargo xtask seal`.
- **It does not run the Docker matrix.** The Docker VM matrix lives at `docker/`
  (`docker/bootstrap-docker.sh`, `docker/compose/matrix.yml`) and is invoked via
  `cargo xtask docker`.
- **It is not a benchmark suite.** The Criterion suite is
  `ryg-rans-rs-bench`. (There is a development-only decode smoke binary, `perf` —
  see below — which is not part of any sealed measurement.)
- **It does not define the evidence schema.** The typed schema types live in
  `ryg-rans-rs-casefile`; this crate declares that crate as a dependency, but its
  behavioural receipts use the harness-local `Receipt`/`CaseManifest` types defined
  in `src/lib.rs`.

---

## Court Surfaces

The harness (`src/main.rs`, `build_court_configs`) runs these courts:

| Variant | Paths | Profiles (single-state, scale_bits 12) | Interleaved2 | Scale sweep |
|---------|-------|-----------------------------------------|--------------|-------------|
| `byte` (32-bit byte rANS) | Division, Reciprocal | 8 profiles | ✅ | ✅ (S10–S16) |
| `r64` (64-bit rANS) | Division, Reciprocal | 8 profiles | ✅ | ✅ (S10–S16) |
| `word` (word rANS, table decode) | Division | 8 profiles | ✅ | — |
| `alias` (alias method) | Division | 8 profiles | ✅ | — |
| `simd` (SSE4.1 8-way) | Division | 8 profiles | — (always 8-way) | — |

The 8 profiles are `Uniform256`, `Freq1Residual`, `Skewed2551`, `Sparse2`,
`Sparse17`, `PrimeResidue`, `RenormBoundary`, `LengthBoundary` (see `ModelProfile`
in `src/lib.rs`).

**Phase G courts** (`src/phase_g.rs`, run via the `run-phase-g` binary) add the
two AVX-512 surfaces:

| Surface | Court function | Court ID pattern |
|---------|----------------|------------------|
| AVX512VL 8-way | `run_avx512vl8_court` | `RYG_RANS.AVX512VL.INTERLEAVED8.<PROFILE>.S12` |
| AVX512 16-way | `run_avx512_16_court` | `RYG_RANS.AVX512.INTERLEAVED16.<PROFILE>.S12` |

**Receipt count is never hardcoded in this README.** The authoritative inventory is
`evidence/index.json`; the Phase K baseline was 144 behavioural receipts, and the
Phase L courts extend the total (see the glossary in
[`docs/glossary.md`](../../docs/glossary.md)).

`src/phase_i.rs` contains **stub** court entry points for parallel encode/decode
determinism: they return empty receipts (no cases, `pairs_compared = 0`) and are
**not wired into the binary**. They are placeholders, not working courts.

---

## How Courts Work

Each court runs `num_cases` cases (per-profile default; see
`ModelProfile::num_cases` — 20 per standard profile, 5 per scale-sweep value, 28
for `LengthBoundary`). For every case the harness:

1. Generates deterministic input (`SimpleRng`, seeded `seed + case_idx`; input
   symbols are constrained to the active alphabet of the frequency table).
2. Asks the C oracle to encode the input → C compressed stream.
3. Encodes the same input with the Rust core → Rust compressed stream.
4. Self-decodes: C decodes its own stream; Rust decodes its own stream.
5. Cross-decodes: Rust decodes C's stream; C decodes Rust's stream.
6. Compares the two compressed streams byte-for-byte.
7. Records every check as a boolean in the case result; a failed check records a
   **residual** (never deleted; resolved or accepted).

The verdict is `admitted_match` when every check of every case passed, else
`admitted_partial`. Receipts, manifests, and the index are written under the
staging/canonical evidence directory (see [Evidence Output](#evidence-output)).

---

## Per-Case Checks

### Byte / R64 / Word / Alias courts (single-state and interleaved2)

| # | Check | What It Proves |
|---|-------|----------------|
| 1 | `c_self_decode` | The C oracle decodes its own output |
| 2 | `rust_self_decode` | Rust decodes its own output |
| 3 | `compressed_match` | C and Rust compressed streams are byte-identical |
| 4 | `c_to_rust` | Rust decodes C's stream to the original input |
| 5 | `rust_to_c` | C decodes Rust's stream to the original input |

(`pairs_compared` = 5 × number of cases for these courts.)

### SIMD / AVX-512 courts (phase_g)

| # | Check | What It Proves |
|---|-------|----------------|
| 1 | `c_self_decode` | C oracle works |
| 2 | `rust_scalar_self_decode` | Rust scalar decoder works |
| 3 | `rust_simd_self_decode` | Rust SIMD decoder works |
| 4 | `compressed_match` | C and Rust compressed streams are byte-identical |
| 5 | `c_to_rust_scalar` | Rust scalar decodes C's stream |
| 6 | `c_to_rust_simd` | Rust SIMD decodes C's stream |
| 7 | `rust_to_c` | C decodes Rust's stream |
| 8 | `simd_scalar_agree` | SIMD and scalar outputs agree |

Empty-input cases skip stream comparisons (both sides' empty encodings are valid).

### Backend assertion

Every SIMD court records `rust_backend`. If the executed backend is not the
expected one (`avx512vl-8way` / `avx512-16way`), a `BACKEND.*` residual is
recorded — a SIMD court can never silently pass via scalar fallback. When the
build lacks the required AVX-512 target features (`cfg!` check), the SIMD checks
fail and the court records residuals; run Phase G courts under the AVX-512
RUSTFLAGS listed below.

---

## The C Oracle Adapter

The oracle is `oracle/adapter/rans_trace.cpp` plus the bundled pinned upstream
headers: `rans_byte.h`, `rans64.h`, `rans_word_sse41.h`, `platform.h`. The
pinned upstream revision (`c9d162d996fd600315af9ae8eb89d832576cb32d`) is recorded
verbatim in every receipt's `upstream_commit` field.

Build:

```sh
cd oracle/adapter && make
```

The Makefile compiles `rans_trace` with `g++ -O3 -msse4.1` (the SIMD surfaces
need SSE4.1). The adapter directory also contains prebuilt `rans_trace_asan` and
`rans_trace_dbg` binaries (sanitizer/debug builds) that are committed in the
tree.

### Stream operations

The adapter speaks a JSON-line protocol over stdout. Each operation takes
`scale_bits freq_csv input_hex` (encode) or `scale_bits freq_csv compressed_hex
num_symbols` (decode) and returns JSON with `compressed_hex`, `decoded_hex`, and/or
`decode_ok`. Operations (from `rans_trace.cpp` usage text):

| Group | Operations |
|-------|-----------|
| byte rANS | `enc-stream-byte`, `dec-stream-byte`, `enc-stream-byte-div`, `dec-stream-byte-div` |
| byte interleaved2 | `enc-stream-byte-interleaved2`, `dec-stream-byte-interleaved2`, `enc-stream-byte-interleaved2-div`, `dec-stream-byte-interleaved2-div` |
| r64 | `enc-stream-r64`, `dec-stream-r64`, `enc-stream-r64-div`, `dec-stream-r64-div` |
| r64 interleaved2 | `enc-stream-r64-interleaved2`, `dec-stream-r64-interleaved2`, `enc-stream-r64-interleaved2-div`, `dec-stream-r64-interleaved2-div` |
| word | `enc-stream-word`, `dec-stream-word`, `enc-stream-word-interleaved2`, `dec-stream-word-interleaved2` |
| SIMD 8-way | `enc-stream-simd`, `dec-stream-simd` |
| SIMD 16-way | `enc-stream-word-interleaved16`, `dec-stream-word-interleaved16` |
| alias | `trace-alias-table`, `enc-stream-alias`, `dec-stream-alias`, `enc-stream-alias-interleaved2`, `dec-stream-alias-interleaved2` |

---

## Trust Boundaries and Input Invariants

- **The oracle binary is the trusted reference.** The harness trusts its stdout
  JSON; if the binary is stale or corrupted, courts fail or (worse) pass against
  the wrong reference. Rebuild it from `oracle/adapter` before generating
  evidence.
- **Frequency invariants**: every court builds a frequency table that sums exactly
  to `1 << scale_bits`; the harness pads the table to 256 entries before sending
  the CSV to the C oracle (which expects 256 entries). Active alphabets are
  constrained so the C oracle never receives `freq=0` mid-table in a way it cannot
  handle.
- **Input invariants**: input bytes are drawn from the active symbol set
  (`num_symbols`), lengths are 64 bytes per case (or the `LengthBoundary` set:
  0–17, 63–65, 127–129, 255–257, 1023).
- **Determinism**: all inputs and models derive from the fixed seed; rerunning
  with the same seed reproduces identical evidence.

---

## Resource Behavior

- **Subprocess per operation**: each C encode/decode spawns one `rans_trace`
  process (`std::process::Command`). A full run spawns thousands of short-lived
  processes; allow time and process slots.
- **No subprocess timeout**: the harness uses blocking `Command::output()` — there
  is no timeout variable. A hung oracle hangs the run.
- **Staging accumulation**: evidence is always generated into a staging directory
  first (`<evidence>.staging/<timestamp>`); failed runs leave the staging
  directory in place for inspection. Filtered runs write to a
  `<evidence>.staging/filtered-<variant>-<timestamp>` directory and are **never**
  promoted.
- **Atomic promotion**: on success the canonical `evidence/` directory is renamed
  to a timestamped backup and the staging directory renamed into place; on
  failure the backup is restored and the run exits non-zero.

---

## Backend Semantics and SIMD Requirements

- **Exact-backend recording**: every SIMD case records `rust_backend`; the
  backend assertion enforces `requested == executed` (glossary terms) or records a
  residual.
- **Phase G courts require AVX-512 compiled in**:

  ```sh
  RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
      cargo run --release -p ryg-rans-rs-oracle --bin run-phase-g
  ```

  Without those features the AVX-512 checks cannot run and the courts record
  residuals rather than pretending to pass.

---

## Unsafe Boundaries

The oracle crate itself is safe Rust with no `unsafe` blocks. It calls the SIMD
crate's `unsafe fn` kernels (e.g. `decode_interleaved8_avx512vl` in `phase_g.rs`)
inside `unsafe` blocks gated by the compile-time feature check — those kernels
carry their own exact `#[target_feature]` attributes and `# Safety` sections in
`ryg-rans-rs-simd` and are inventoried in its machine-verified unsafe ledger.

---

## Evidence Output

### Directory structure

```text
evidence/
├── index.json                       ← { schema_version, code_commit, receipts: [{court_id, sha256}] }
├── receipts/
│   └── receipt-<court_id>.json      ← one per court (verdict, counts, manifest_sha256, receipt_sha256, ...)
├── manifests/
│   └── manifest-<court_id>.json     ← all cases, streams, and per-case verdicts
└── docker-matrix.json               ← written by the Docker matrix run, verified by the seal gate
```

### SHA-256 chain

```text
evidence/index.json
  └── sha256 of → evidence/receipts/receipt-<court_id>.json
                    └── receipt.manifest_sha256 → evidence/manifests/manifest-<court_id>.json
                                                    └── all cases, streams, verdicts
```

Each receipt also carries a `receipt_sha256` self-hash field. **Note**: the
harness writes the self-hash, but the seal gate currently **skips** verifying
behavioural receipt self-hashes (the canonical-serialization scheme differs
between harnesses) — tracked as residual **L1-R** / **L20-A**. The index → receipt
→ manifest chain is fully verified by the seal gate; the self-hash gap is a known,
tracked defect, not a sealed property.

### Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `RANS_EVIDENCE_DIR` | `evidence` | Output root |
| `RANS_GIT_COMMIT` | `git rev-parse HEAD` | Commit recorded in receipts and index |

(`RANS_EVIDENCE_STAGING` and `RANS_ORACLE_TIMEOUT_MS` from earlier documentation
no longer exist; staging is automatic and there is no timeout.)

---

## Usage

### Generate full behavioural evidence

```sh
cd oracle/adapter && make
cargo run -p ryg-rans-rs-oracle -- oracle/adapter/rans_trace [scale_bits] [seed] [num_cases] [variant]
```

Arguments (all optional except the oracle path):

1. `oracle/adapter/rans_trace` — path to the compiled oracle binary (falls back to
   `../oracle/adapter/rans_trace` if the argument is missing and that file exists)
2. `scale_bits` — default scale (12); overridden per-profile (sweep profiles use
   10–16)
3. `seed` — deterministic seed (default 42)
4. `num_cases` — optional case-count override (the profile default is used if
   omitted)
5. `variant` — optional filter (`byte`, `r64`, `word`, `alias`, `simd`); filtered
   runs go to a staging directory and are **never** promoted

On success all receipts/manifests/index are promoted into `evidence/` atomically;
the run prints `ALL COURTS PASSED`. Any non-`admitted_match` verdict makes the run
exit 1.

### Generate the Phase G (AVX-512) receipts

```sh
cd oracle/adapter && make
RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw" \
    cargo run --release -p ryg-rans-rs-oracle --bin run-phase-g
```

`run-phase-g` generates the 8 + 8 AVX-512 courts for the 8 standard profiles,
merges the new receipts into `evidence/` (replacing same-ID entries), and rewrites
`evidence/index.json`.

### Verify the evidence

```sh
cargo xtask seal
```

The seal gate is the single authoritative gate; see
[`xtask/README.md`](../../xtask/README.md).

---

## Development: the `perf` Binary

`src/bin/perf.rs` is a development-only decode smoke benchmark (scalar, SSE4.1,
AVX512VL 8-way, AVX512 16-way at 8 sizes across 5 profiles) that prints a CSV.
It takes an optional size filter (`cargo run --release -p ryg-rans-rs-oracle --bin perf -- 1048576`).
It is **not** the Criterion suite, is **not** wired into evidence generation, and
produces **no sealed measurements**.

---

## Limitations (honest)

1. **Zero unit tests.** The crate's correctness story is the courts themselves;
   there is no `#[test]` coverage of the harness code.
2. **`phase_i.rs` courts are stubs** — not wired into the binary, empty receipts.
3. **Behavioural receipt self-hash verification is skipped by the seal gate**
   (L1-R / L20-A) — the chain is verified; the self-hash field is not yet a sealed
   property.
4. **No subprocess timeout** — a hung oracle hangs the run.
5. **Evidence runs spawn thousands of subprocesses** — a full run is slow by
   design (determinism and independence over speed).
6. **The oracle must be built locally** from `oracle/adapter`; the harness does
   not build it for you.

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| `ERROR: oracle not found. Build it: cd oracle/adapter && make` | The oracle path argument is missing/invalid and no fallback exists. Build the adapter first. |
| Run exits 1 with `SOME COURTS FAILED` | At least one court verdict is `admitted_partial`. Inspect `<evidence>.staging/<timestamp>/` — receipts list `residual_ids`; manifests carry per-case booleans. |
| AVX-512 courts record `rust_simd_self_decode: false` | The build lacks AVX-512 target features; rerun with the RUSTFLAGS from [Backend Semantics](#backend-semantics-and-simd-requirements). |
| `BACKEND.*` residuals on SIMD courts | The executed backend was not the expected one — silent scalar fallback is prohibited. |
| Filtered run "kept in staging" | `--filter`-style runs (the 5th argument) never promote; that is by design. |

---

## Versioning and Reading Order

- **Version**: 0.2.0 (workspace crates).
- **Reading order**: root [`README.md`](../../README.md) →
  [`docs/architecture.md`](../../docs/architecture.md) →
  [`docs/oracle-method.md`](../../docs/oracle-method.md) →
  [`docs/glossary.md`](../../docs/glossary.md) → this README →
  [`crates/ryg-rans-rs-casefile/README.md`](../ryg-rans-rs-casefile/README.md) →
  [`xtask/README.md`](../../xtask/README.md).
- **Ground-truth ledger**: [`evidence/phase-l/gap-ledger.md`](../../evidence/phase-l/gap-ledger.md).

---

*Part of the ryg-rans-rs project. Version 0.2.0. Phase L.15 documentation pass.*
