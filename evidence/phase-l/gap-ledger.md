# Phase L — Gap Ledger

Baseline commit: `7bbf4a25d9b3b087d0abb81aba59f73788a677fe`
Baseline tree: `424e6d577675d5e01de43f80ff05acda9271bd25`
rustc: 1.96.0 (ac68faa20 2026-05-25)
cargo: 1.96.0

Host: AMD Ryzen 7 9800X3D, 8 cores / 16 threads, 1 socket, 1 NUMA node.

Status legend: **RESOLVED** (fix committed, tests pass) · **PARTIAL** (part of the
residual addressed; remainder tracked) · **OPEN** (not yet addressed).

---

## L.0 — Baseline freeze

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L0-A | LOW | Baseline metadata captured (commit, tree, rustc, cargo) | RESOLVED | `955ffe0` |
| L0-B | MEDIUM | Baseline command outputs (check/test/smoke/seal logs) were not archived under `evidence/phase-l/baseline/` — only metadata files were saved | OPEN | Full command-log capture lands with the L.18 benchmark-run wrapper; baseline outputs will be regenerated from the frozen L.0 commit state and archived alongside the L.18 run |

## L.1 — Phase K performance seal defects

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L1-A | CRITICAL | All 831 records: sample_count=1 (fabricated), verification_passed=true (hardcoded), status="pass", empty output_hash, profile="unknown", api="unknown", threads=1/1, 798/831 bytes=0, 798/831 throughput=0 | OPEN | L.18 exporter rewrite (benchmark.json/sample.json join + preflight channel) |
| L1-B | CRITICAL | Exporter derives identity from sanitized directory names, not Criterion benchmark.json (group_id, function_id, value_str, full_id, throughput) | OPEN | L.18 |
| L1-C | CRITICAL | Sample count fabricated (defaults to 1 when absent); must read sample.json | OPEN | L.18 |
| L1-D | CRITICAL | Verification hardcoded true; no preflight evidence channel exists | OPEN | L.18 |
| L1-E | CRITICAL | Runtime provenance captured at seal time, not benchmark time; needs benchmark-run wrapper | OPEN | L.18 |
| L1-F | CRITICAL | Commit binding is tautological (assign then compare same value) | OPEN | L.18 |
| L1-G | CRITICAL | Command log is empty (hashed but never written) | OPEN | L.18 |
| L1-H | HIGH | Host metadata hashed but not stored as artifact (host.json) | OPEN | L.18 |
| L1-I | HIGH | CPU feature reporting uses compile-time cfg on sealer binary → empty feature set | OPEN | L.18 |
| L1-J | HIGH | RUSTFLAGS lost (empty in manifest despite -C target-cpu=native) | OPEN | L.18 |
| L1-K | HIGH | Custom tar truncates paths to 99 bytes; need tar crate with PAX/GNU long-name | OPEN | L.18 |
| L1-L | CRITICAL | Index sha256 = canonical self-hash, not final-file hash; needs two distinct fields | OPEN | L.18 |
| L1-M | MEDIUM | Receipt verdicts are untyped strings; need typed enums | OPEN | L.18 |
| L1-N | HIGH | executed/verified counters derived from fabricated defaults | OPEN | L.18 |
| L1-O | HIGH | Parity citations not updated (still performance_status:"unsealed") | OPEN | L.18 |
| L1-P | HIGH | No canonical top-level evidence/performance/index.json | OPEN | L.18 |
| L1-Q | HIGH | cargo xtask seal does not validate performance evidence | OPEN | L.18/L.20 |
| L1-R | CRITICAL | Behavioral self-hash verification falsely reports success after skipping | OPEN | L.18/L.20 |
| L1-S | HIGH | Phase K artifacts must be marked superseded, not deleted | OPEN | L.18 (quarantine step) |

## L.2 — Decoded-output integrity verification bug

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L2-A | CRITICAL | Verifier computes decoded_hash_ok but aggregate failure ignores it; block passes when decoded hash mismatches nonzero stored hash | RESOLVED | `955ffe0` — aggregate condition now includes decoded-hash verdict; tests cover corrupted-payload vs corrupted-model discrimination |
| L2-B | HIGH | No IntegrityPolicy enum (Strict / AllowLegacyUnsetDecodedHash) | RESOLVED | `955ffe0` — `IntegrityPolicy` in `config.rs`; Strict is default on all verify/CLI/court/evidence paths; legacy requires explicit opt-in |
| L2-C | HIGH | HashVerification enum missing (Match/Mismatch/Unset/NotComputed) | RESOLVED | `955ffe0` |
| L2-D | HIGH | BlockErrorKind::DecodedHashMissing / DecodedHashMismatch missing (uses generic Codec) | RESOLVED | `955ffe0` — typed variants, no longer generic `Codec` |

## L.3 — Cancellation

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L3-A | CRITICAL | High-level APIs create internal tokens; no public cancellation APIs | RESOLVED | `d5329ae` — `encode_blocks_with_cancel`, `decode_blocks_with_cancel`, `decode_streaming_with_cancel`, `verify_blocks_with_cancel` |
| L3-B | CRITICAL | Cancellation can return Ok with fewer results than declared (silent truncation) | RESOLVED | `d5329ae` — completeness invariant: `completed_results != expected_block_count` → `ParallelError::IncompleteExecution`/`Cancelled { completed, expected }`; never `Ok` with fewer blocks |
| L3-C | HIGH | No IncompleteExecution / Cancelled error variants with counts | RESOLVED | `d5329ae` |
| L3-D | MEDIUM | No CLI signal handling (SIGINT/SIGTERM/timeout) | OPEN | CLI cancellation wiring scheduled with L.15 CLI audit |

## L.4 — Bounded executor

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L4-A | CRITICAL | Executor materializes all tasks + accumulates all results in Mutex<Vec>; not end-to-end bounded | RESOLVED | `b0e0d51` — producer thread + bounded job channel + bounded result channel + coordinator drain |
| L4-B | CRITICAL | max_buffered_output_bytes can't bound peak memory in current architecture | RESOLVED | `b0e0d51` — output budget enforced against the live reorder stage |
| L4-C | HIGH | No live coordinator loop with select! interleaving submit/drain | RESOLVED | `b0e0d51` |
| L4-D | HIGH | No streaming sink APIs (decode_to_writer, encode_to_writer, etc.) | RESOLVED | `b0e0d51` — sink/writer APIs; streaming path does not collect all jobs/results |
| L4-E | HIGH | max_buffered_input_bytes inert (never enforced) | RESOLVED | `b0e0d51` + `f9ea4d6` — enforced during submission |

## L.5 — ReorderBuffer

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L5-A | HIGH | insert() returns Option<T>; callers must remember drain_ready() — fragile protocol | RESOLVED | `020ddae` |
| L5-B | HIGH | Need atomic commit batches: insert() → Result<Vec<T>> | RESOLVED | `020ddae` — `insert(item) -> Result<Vec<T>, BlockError>` returns the newly-committable chain; contiguous ascending indexes; no separate drain required; permutation property tests |

## L.6 — Config field audit

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L6-A | HIGH | max_buffered_input_bytes inert | RESOLVED | `f9ea4d6` |
| L6-B | HIGH | parallel_threshold_bytes not implemented (no sequential fallback) | RESOLVED | `f9ea4d6` — `ExecutionMode::SequentialThresholdFallback`, requested/effective workers reported (effective=1) |
| L6-C | HIGH | affinity policies not implemented (None/Compact/Spread/Explicit) | RESOLVED | `f9ea4d6` — Linux `sched_setaffinity` per worker with `sched_getaffinity` verification; invalid explicit lists → typed config error |
| L6-D | HIGH | smt_policy not topology-aware | RESOLVED | `f9ea4d6` — topology read (core ids per CPU); SMT siblings excluded for PhysicalOnly; report records topology source |
| L6-E | HIGH | disable_simd semantics incomplete | RESOLVED | `f9ea4d6` — forces scalar; explicit SIMD + disable_simd → config conflict |
| L6-F | HIGH | disable_inner_batching has no distinct path (or must be removed) | RESOLVED | `f9ea4d6` — field removed; no config theater retained |
| L6-G | LOW | error_policy single-option redundancy | RESOLVED | `f9ea4d6` — redundant field removed |
| L6-H | LOW | worker_stack_size needs observable tests | RESOLVED | `f9ea4d6` — custom stack test, too-small stack failure, metadata records stack size |
| L6-I | MEDIUM | max_in_flight_blocks needs peak in-flight proof | RESOLVED | `f9ea4d6` — queue capacity and peak in-flight counters in executor report |

## L.7 — WorkerScratch wiring

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L7-A | HIGH | WorkerScratch/ScratchPool public but never used in production | RESOLVED | `29bb0e7` — one exclusive scratch per worker (`ExecutorTask::run(worker, cancel, scratch)`); reset between tasks; bounded retained capacity; allocation-count instrumentation tests |

## L.8 — ModelCache wiring

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L8-A | HIGH | ModelCache/plan_cache_key exist but no production decode path uses them | RESOLVED | `29bb0e7` — `cached_model_artifacts()` joins decode path (`decode.rs`); key = (model_sha256, scale_bits, codec_id); backend selection after lookup; bounded entries/bytes; eviction metrics; corrupt model never cached |

## L.9 — Backend semantics

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L9-A | HIGH | Explicit SSE4.1/AVX512/manual-gather requests rewritten to scalar during planning | RESOLVED | `f0ae16e` — exact backend mapping; explicit request executes exactly or returns typed error; no silent scalar substitution |
| L9-B | HIGH | No codec/backend format-compatibility enforcement | RESOLVED | `f0ae16e` — 8-way↔codec7, 16-way↔codec8, Uniform256↔validated model, RAW/RLE↔block type enforced at plan time |
| L9-C | MEDIUM | Batch4 not coordinator-executable through one-block API | RESOLVED | `f0ae16e` — Batch4 requires coordinator batch context; one-block API returns explicit typed error |

## L.10 — SSE unsafe quarantine

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L10-A | HIGH | SSE helpers lack local #[target_feature] attributes | RESOLVED | `8588699` — `#[target_feature(enable = "ssse3,sse4.1")]` on every SSE helper; `# Safety` sections; caller lists |
| L10-B | MEDIUM | Unsafe ledger not generated from source; may claim things not true | RESOLVED | `8588699` + `7fe286b` — `unsafe-ledger.toml` with bidirectional test (ledger↔source inventory), disassembly courts (`pshufb`, `pblendvb`, `vpermd`, `vpgatherdd`, `vpmovdb`) |

## L.11 — Algorithmic audit

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L11-A | HIGH | Repository-wide unwrap/expect/index/panic audit needed | RESOLVED | `7fe286b` — audit complete; test-only/invariant uses annotated locally; malformed-input paths return typed errors; report-parity defect fixed (`decode_simd_8way_unchecked_with_report`) |

## L.12 — Code commentary for disproved suspicions

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L12-A | LOW | Local commentary for block-header `try_into().unwrap()` and executor mutex-poisoning suspicion | RESOLVED | `7fe286b` — exact invariants stated at each point; mutex commentary updated to channel-based architecture |

## L.13 — Public API audit

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L13-A | MEDIUM | Disconnected public types: ScratchPool, WorkerScratch, ModelCache, ParallelExecutionReport, schedule types, resource estimation | RESOLVED | `f1db7b2` — dead `schedule.rs`/`report.rs` removed; `estimate_memory` contract-tested (saturating-overflow bug fixed); `docs/public-api/` inventory committed; `cargo public-api` + `cargo semver-checks` baseline captured |

## L.14 — Comparative benchmark court

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L14-A | MEDIUM | ryg-rans-sys C wrappers compiled by `cc` with default flags (no `-march=native`); C byte-decode bench includes per-symbol FFI crossing + `rans_dec_symbol_init` per byte, while the Rust side is direct calls. Where the C side lacks auto-vectorisation the comparison favours Rust; word-decode (no per-symbol init on either side) measures near-parity, bounding the handicap | RESOLVED | Current commit (bench `comparative`, `RUSTFLAGS="-C target-cpu=native"`) — methodological residual, recorded not fixed |
| L14-B | MEDIUM | The `rans` 0.4.0 crate (m4tx) exposes a different API/format (not upstream ryg_rans); byte-for-byte comparison is impossible without format adaptation. Documented and excluded; not claimed as a comparison | RESOLVED | Current commit — residual recorded; alternative pinned and excluded with reason |

Comparative methodology and results are in `docs/performance/comparative.md` (L.14 court).

## L.15 — Documentation

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L15-A | HIGH | Remove "critical-safety-infrastructure quality" overclaim everywhere | RESOLVED | Current commit — no occurrence remains in docs/READMEs/source comments/Cargo.toml descriptions; `cargo xtask no-overclaim` gate added to `check` prevents reintroduction |
| L15-B | HIGH | Remove stray -.o file; add root .gitignore entries | RESOLVED | `.gitignore` hardened `c38928a`; tracked `-.o` removed `7fac502`; `criterion/` anchored to `/target/criterion/` `3bf5dd9` so committed evidence is never ignored; recurrence gate part of L.20 |
| L15-C | MEDIUM | Fix root README header (malformed Markdown) | RESOLVED | Current commit — header rewritten (all bold markers closed, counts accurate, honest performance status: Re-sealing L.18), reading order added, CLI section matches the wired implementation, FFI policy updated for the L.14 comparative court, Docker count corrected to 11 |
| L15-D | MEDIUM | Add AGENTS.md, llms.txt, docs/glossary.md, reading order | RESOLVED | Current commit — all four exist; glossary defines every term; AGENTS.md states ground-truth files, invariants, commands, unsafe rules |
| L15-E | HIGH | CLI subcommands were scaffolding ("not yet implemented") while READMEs claimed "Production-grade"/"deeply implemented — not a scaffold" | RESOLVED | `0fa5936` — encode (byte-single, byte-interleaved2, r64-single, word-single + RLE/RAW fallback), decode (strict integrity), inspect, verify, model build/inspect/validate/compare, trace (byte-single), compare arithmetic/backends/files, and bench are all wired and integration-tested. Wiring surfaced and fixed: container reader 4-byte tag misalignment, model normalizer wrong-sum bug (4286≠4096; latent infinite loop; debug_assert no-op in release), main.rs exit-code collapse, unreachable exit 6, open_output --force create bug |
| L15-F | MEDIUM | core lib.rs module doc claimed `default` enables `std` and `std` activates `alloc`; Cargo.toml has `default = []` and `std = []` (independent) | RESOLVED | Current commit — module doc corrected to match Cargo.toml; all three feature combos (none/alloc/std) verified compiling |

## L.16 — Testing

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L16-A | MEDIUM | Proptest targets for reorder permutations, partition, normalization, etc. | RESOLVED | Current commit — reorder permutation property (any permutation of 0..24, 256 cases, `tests/reorder_proptest.rs`), duplicate/stale-index rejection, CLI model normalizer property (random histograms × scales, sum-exactness + round-trip, `tests/model_proptest.rs`) |
| L16-B | MEDIUM | Fuzz targets for parsers and codecs | RESOLVED | Current commit — fuzz workspace repaired; all 9 targets execute; fuzzing found and fixed three target bugs (malformed_byte out-of-bounds model reads, word_rans single-symbol u32 threshold overflow, parallel_block_plan short-slice unwrap) plus the r64 target's 1 GiB-per-iteration allocation; all targets run clean |
| L16-C | MEDIUM | Loom models for new executor | RESOLVED | Current commit — executor made loom-instrumentable via `sync.rs` swap layer; 2 channel courts + 5 executor courts (no lost tasks, cancellation race completeness, panic no-wedge, sink completeness, reorder ascending) pass under `LOOM_MAX_PREEMPTIONS=2`.  The queue model work caught and fixed a real missed-wakeup race (sender count outside the mutex); loom's own mpsc is unusable for multi-consumer (Send-but-not-Sync receiver) |
| L16-D | MEDIUM | Sanitizer/Miri runs | PARTIAL | Current commit — ASan ran as part of every cargo-fuzz run (default `-Zsanitizer=address`); Miri passes the full core suite (57 tests); CLI Miri run excludes process-spawning integration tests (isolation blocks `open`); the parallel crate's Miri run exceeds practical time bounds; UBSan spot coverage deferred to the L.20 gate matrix |
| L16-E | MEDIUM | Two R64 kani instances (freq=3, freq=65535) and fully-symbolic-scale reciprocal instances do not terminate within practical time bounds (symbolic division not bit-blastable).  The identity is otherwise formally verified (21 proofs: 3 symbol-construction, 3 packed-entry, 7 byte reciprocal, 3 R64 reciprocal, 5 inversion) and pinned by differential round-trip tests plus the L.14 comparative court (byte-identical output with upstream C) | PARTIAL | Current commit — restructured to concrete-freq harness instances (21 proofs verified); the two intractable instances remain a documented accepted limitation |

## L.17 — Performance

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L17-A | MEDIUM | Component isolation (decode vs hash vs model) not measured | OPEN | L.17 |
| L17-B | MEDIUM | Queue-depth sweep, affinity, SMT measurements missing | OPEN | L.17 |

## L.19 — Phase L courts

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L19-A | HIGH | 14 new courts required with manifests and receipts | OPEN | L.19 |

## L.20 — Seal gate

| ID | Severity | Issue | Status | Resolution |
|----|----------|-------|--------|------------|
| L20-A | HIGH | Seal gate must validate performance evidence and never print success for skipped checks | OPEN | L.20 |

---

## Resolution tracking

Each residual records: severity, affected files, reproduction, expected/actual behavior, proposed fix, test requirement, evidence requirement, resolution commit.

- L2-A..D — `crates/ryg-rans-rs-parallel/src/decode.rs`, `config.rs`, `error.rs`. Reproduced by court: block with intact payload + corrupt model bytes decoded to wrong output while payload hash matched. Fixed by including the decoded-hash verdict in the aggregate failure condition and adding `DecodedHashMissing`/`DecodedHashMismatch`. Tests: 15-combination matrix incl. per-backend verdict equality, CLI exit code, per-category counts.
- L3-A..C — `crates/ryg-rans-rs-parallel/src/decode.rs`, `encode.rs`, `executor.rs`, `error.rs`, `cancellation.rs`. Completeness counters (declared/submitted/started/completed/cancelled/skipped/returned) in `ExecutorReport`; `Cancelled { completed, expected }`; 15-scenario cancellation race tests incl. panic/cancellation priority.
- L4-A..E — `crates/ryg-rans-rs-parallel/src/executor.rs`. Producer thread + bounded job channel + bounded result channel; coordinator drains results live and commits in order; input/output budgets enforced against live stages; streaming sink APIs; stress tests (slow block 0, 10 GiB-equivalent synthetic, deadlock/backpressure).
- L5-A..B — `crates/ryg-rans-rs-parallel/src/reorder.rs`. `insert -> Result<Vec<T>>` atomic commit; N≤9 exhaustive permutation test; duplicate/stale/missing-gap/overflow/error-recovery tests.
- L6-A..I — `crates/ryg-rans-rs-parallel/src/config.rs`, `executor.rs`, `decode.rs`, `encode.rs`. Field-by-field wiring; `disable_inner_batching` and single-option `error_policy` removed; each field has an observable single-field test.
- L7-A — `crates/ryg-rans-rs-parallel/src/executor.rs`, `scratch.rs`. Exclusive per-worker scratch via `ExecutorTask::run(worker, cancel, scratch)`; no shared mutable scratch; reset between tasks; retained capacity bounded; allocation-count tests.
- L8-A — `crates/ryg-rans-rs-parallel/src/cache.rs`, `decode.rs`, `decode_plan.rs`. `cached_model_artifacts` in the decode path; bounded entries/bytes; eviction; corrupt-model exclusion; cache-equivalence tests.
- L9-A..C — `crates/ryg-rans-rs-parallel/src/decode_plan.rs`, `decode.rs`. Explicit backend executed-exactly-or-typed-error; format-compatibility matrix enforced; Batch4 coordinator-context rule.
- L10-A..B — `crates/ryg-rans-rs-simd/src/lib.rs`, `unsafe-ledger.toml`, tests. Local `#[target_feature]` on SSE helpers; bidirectional ledger test; disassembly courts.
- L11-A — workspace-wide audit; fixed SSE4.1 report parity; typed errors for malformed inputs; annotated remaining invariant-based unwraps.
- L13-A — removed dead types; `estimate_memory` overflow fix; public-API inventory under `docs/public-api/`.
- L14-A..B — comparative court; see `docs/performance/comparative.md`.
