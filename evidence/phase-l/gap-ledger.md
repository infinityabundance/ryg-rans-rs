# Phase L — Gap Ledger

Baseline commit: `7bbf4a25d9b3b087d0abb81aba59f73788a677fe`
Baseline tree: `424e6d577675d5e01de43f80ff05acda9271bd25`
rustc: 1.96.0 (ac68faa20 2026-05-25)
cargo: 1.96.0

Host: AMD Ryzen 7 9800X3D, 8 cores / 16 threads, 1 socket, 1 NUMA node.

---

## L.1 — Phase K performance seal defects

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L1-A | CRITICAL | All 831 records: sample_count=1 (fabricated), verification_passed=true (hardcoded), status="pass", empty output_hash, profile="unknown", api="unknown", threads=1/1, 798/831 bytes=0, 798/831 throughput=0 | OPEN |
| L1-B | CRITICAL | Exporter derives identity from sanitized directory names, not Criterion benchmark.json (group_id, function_id, value_str, full_id, throughput) | OPEN |
| L1-C | CRITICAL | Sample count fabricated (defaults to 1 when absent); must read sample.json | OPEN |
| L1-D | CRITICAL | Verification hardcoded true; no preflight evidence channel exists | OPEN |
| L1-E | CRITICAL | Runtime provenance captured at seal time, not benchmark time; needs benchmark-run wrapper | OPEN |
| L1-F | CRITICAL | Commit binding is tautological (assign then compare same value) | OPEN |
| L1-G | CRITICAL | Command log is empty (hashed but never written) | OPEN |
| L1-H | HIGH | Host metadata hashed but not stored as artifact (host.json) | OPEN |
| L1-I | HIGH | CPU feature reporting uses compile-time cfg on sealer binary → empty feature set | OPEN |
| L1-J | HIGH | RUSTFLAGS lost (empty in manifest despite -C target-cpu=native) | OPEN |
| L1-K | HIGH | Custom tar truncates paths to 99 bytes; need tar crate with PAX/GNU long-name | OPEN |
| L1-L | CRITICAL | Index sha256 = canonical self-hash, not final-file hash; needs two distinct fields | OPEN |
| L1-M | MEDIUM | Receipt verdicts are untyped strings; need typed enums | OPEN |
| L1-N | HIGH | executed/verified counters derived from fabricated defaults | OPEN |
| L1-O | HIGH | Parity citations not updated (still performance_status:"unsealed") | OPEN |
| L1-P | HIGH | No canonical top-level evidence/performance/index.json | OPEN |
| L1-Q | HIGH | cargo xtask seal does not validate performance evidence | OPEN |
| L1-R | CRITICAL | Behavioral self-hash verification falsely reports success after skipping | OPEN |
| L1-S | HIGH | Phase K artifacts must be marked superseded, not deleted | OPEN |

## L.2 — Decoded-output integrity verification bug

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L2-A | CRITICAL | Verifier computes decoded_hash_ok but aggregate failure ignores it; block passes when decoded hash mismatches nonzero stored hash | OPEN |
| L2-B | HIGH | No IntegrityPolicy enum (Strict / AllowLegacyUnsetDecodedHash) | OPEN |
| L2-C | HIGH | HashVerification enum missing (Match/Mismatch/Unset/NotComputed) | OPEN |
| L2-D | HIGH | BlockErrorKind::DecodedHashMissing / DecodedHashMismatch missing (uses generic Codec) | OPEN |

## L.3 — Cancellation

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L3-A | CRITICAL | High-level APIs create internal tokens; no public cancellation APIs | OPEN |
| L3-B | CRITICAL | Cancellation can return Ok with fewer results than declared (silent truncation) | OPEN |
| L3-C | HIGH | No IncompleteExecution / Cancelled error variants with counts | OPEN |
| L3-D | MEDIUM | No CLI signal handling (SIGINT/SIGTERM/timeout) | OPEN |

## L.4 — Bounded executor

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L4-A | CRITICAL | Executor materializes all tasks + accumulates all results in Mutex<Vec>; not end-to-end bounded | OPEN |
| L4-B | CRITICAL | max_buffered_output_bytes can't bound peak memory in current architecture | OPEN |
| L4-C | HIGH | No live coordinator loop with select! interleaving submit/drain | OPEN |
| L4-D | HIGH | No streaming sink APIs (decode_to_writer, encode_to_writer, etc.) | OPEN |
| L4-E | HIGH | max_buffered_input_bytes inert (never enforced) | OPEN |

## L.5 — ReorderBuffer

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L5-A | HIGH | insert() returns Option<T>; callers must remember drain_ready() — fragile protocol | OPEN |
| L5-B | HIGH | Need atomic commit batches: insert() → Result<Vec<T>> | OPEN |

## L.6 — Config field audit

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L6-A | HIGH | max_buffered_input_bytes inert | OPEN |
| L6-B | HIGH | parallel_threshold_bytes not implemented (no sequential fallback) | OPEN |
| L6-C | HIGH | affinity policies not implemented (None/Compact/Spread/Explicit) | OPEN |
| L6-D | HIGH | smt_policy not topology-aware | OPEN |
| L6-E | HIGH | disable_simd semantics incomplete | OPEN |
| L6-F | HIGH | disable_inner_batching has no distinct path (or must be removed) | OPEN |
| L6-G | LOW | error_policy single-option redundancy | OPEN |
| L6-H | LOW | worker_stack_size needs observable tests | OPEN |
| L6-I | MEDIUM | max_in_flight_blocks needs peak in-flight proof | OPEN |

## L.7 — WorkerScratch wiring

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L7-A | HIGH | WorkerScratch/ScratchPool public but never used in production | OPEN |

## L.8 — ModelCache wiring

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L8-A | HIGH | ModelCache/plan_cache_key exist but no production decode path uses them | OPEN |

## L.9 — Backend semantics

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L9-A | HIGH | Explicit SSE4.1/AVX512/manual-gather requests rewritten to scalar during planning | OPEN |
| L9-B | HIGH | No codec/backend format-compatibility enforcement | OPEN |
| L9-C | MEDIUM | Batch4 not coordinator-executable through one-block API | OPEN |

## L.10 — SSE unsafe quarantine

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L10-A | HIGH | SSE helpers lack local #[target_feature] attributes | OPEN |
| L10-B | MEDIUM | Unsafe ledger not generated from source; may claim things not true | OPEN |

## L.11 — Algorithmic audit

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L11-A | HIGH | Repository-wide unwrap/expect/index/panic audit needed | OPEN |

## L.13 — Public API audit

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L13-A | MEDIUM | Disconnected public types: ScratchPool, WorkerScratch, ModelCache, ParallelExecutionReport, schedule types, resource estimation | OPEN |

## L.15 — Documentation

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L15-A | HIGH | Remove "critical-safety-infrastructure quality" overclaim everywhere | OPEN |
| L15-B | HIGH | Remove stray -.o file; add root .gitignore entries | OPEN |
| L15-C | MEDIUM | Fix root README header (malformed Markdown) | OPEN |
| L15-D | MEDIUM | Add AGENTS.md, llms.txt, docs/glossary.md, reading order | OPEN |

## L.16 — Testing

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L16-A | MEDIUM | Proptest targets for reorder permutations, partition, normalization, etc. | OPEN |
| L16-B | MEDIUM | Fuzz targets for parsers and codecs | OPEN |
| L16-C | MEDIUM | Loom models for new executor | OPEN |
| L16-D | MEDIUM | Sanitizer/Miri runs | OPEN |

## L.17 — Performance

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L17-A | MEDIUM | Component isolation (decode vs hash vs model) not measured | OPEN |
| L17-B | MEDIUM | Queue-depth sweep, affinity, SMT measurements missing | OPEN |

## L.19 — Phase L courts

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L19-A | HIGH | 14 new courts required with manifests and receipts | OPEN |

## L.20 — Seal gate

| ID | Severity | Issue | Status |
|----|----------|-------|--------|
| L20-A | HIGH | Seal gate must validate performance evidence and never print success for skipped checks | OPEN |

---

## Resolution tracking

Each residual must record: severity, affected files, reproduction, expected/actual behavior, proposed fix, test requirement, evidence requirement, resolution commit.
