# ryg-rans-rs-cli

> **The `ryg-rans` command — rANS entropy coding tool.**  
> **Version 0.2.0** (workspace) · **Phase L.15: fully wired** · **20 integration tests**  
> Versioned block-streaming container format (RYGRANS v1) · SHA-256 integrity
> verification · resource-bounded, deterministic, non-panicking · 10
> subcommands · 10 stable exit codes · 5 shell completions.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-cli)](https://crates.io/crates/ryg-rans-rs-cli)

---

## Table of Contents

1. [What This Crate Is](#what-this-crate-is)
2. [What This Crate Does NOT Do](#what-this-crate-does-not-do)
3. [Commands](#commands)
4. [Container Format](#container-format)
5. [Stable Exit Codes](#stable-exit-codes)
6. [Resource Limits](#resource-limits)
7. [Codec vs Backend Distinction](#codec-vs-backend-distinction)
8. [Safety and Trust Boundaries](#safety-and-trust-boundaries)
9. [Architecture](#architecture)
10. [Evidence Model](#evidence-model)
11. [Performance Methodology](#performance-methodology)
12. [Limitations](#limitations)
13. [Examples](#examples)
14. [Troubleshooting](#troubleshooting)
15. [Versioning](#versioning)
16. [Reading Order](#reading-order)

---

## What This Crate Is

This crate provides the `ryg-rans` command-line tool (binary plus library).
The library (`ryg_rans_rs_cli::run`) parses arguments, dispatches to the
subcommand implementations in `ops/`, and returns the stable exit code; the
binary (`main.rs`) maps it verbatim to `std::process::ExitCode` (nonzero codes
are never collapsed to 1).

All **10 subcommands are wired and integration-tested** (Phase L.15, commit
0fa5936): `encode`, `decode`, `inspect`, `verify`, `model`, `trace`,
`compare`, `bench`, `capabilities`, `completions`.  The crate is
`#![forbid(unsafe_code)]` — all SIMD acceleration is reached through safe
facade APIs (`ryg-rans-rs-simd`'s safe `decode_simd_8way`, which uses the
SIMD kernel when compiled with `sse4.1` and a scalar reference otherwise).

Design principles:

1. **Versioned container format** — RYGRANS v1 (`docs/container-format-v1.md`):
   fixed-size header, per-block records, terminal footer, SHA-256 integrity at
   block and stream level.
2. **Strict integrity** — a block passes only when the stored decoded hash is
   non-zero **and** matches; zero/unset decoded hashes fail (exit 5).
3. **Resource limits enforced during reading** — every bound is checked while
   bytes are consumed, not after (see [Resource Limits](#resource-limits)).
4. **Deterministic output** — identical input + options → byte-identical
   container.  No timestamps, no random identifiers, no host-dependent values.
5. **No silent fallback** — unsupported codecs and explicit backend requests
   return a typed error (exit 6), never a different code path.

---

## What This Crate Does NOT Do

- **Does not use the `ryg-rans-rs-parallel` engine.**  The CLI has no
  dependency on the parallel crate; encode/decode/verify run through the
  crate's own single-threaded streaming pipeline with one shared codec
  dispatcher (`ops::decode_block`).  There is no multi-threaded block decode.
- **Does not support explicit decode backends.**  Only the auto dispatcher
  exists; `decode --backend <anything-but-auto>` returns a typed unsupported
  error (exit 6).  `verify --backend` accepts `auto` and `all-available`,
  both served by the auto dispatcher.
- **Does not implement every codec.**  Encode implements byte-single,
  byte-interleaved2 (default), r64-single, word-single.  Decode implements
  codecs 1, 2, 3, 5 and 7 (8-way via SIMD/scalar); codecs 4, 6, 8, 9, 10
  return a typed unsupported error (exit 6).
- **Does not handle SIGINT/SIGTERM cancellation.**  Signal-handling wiring is
  tracked as residual L3-D in the gap ledger (OPEN).
- **Does not benchmark for the record.**  `bench` is a live smoke
  measurement; the Criterion suite in `ryg-rans-rs-bench` is the sealed
  measurement surface.
- **Does not implement "atomic" file replacement** (temp-file + rename).
  Output files are created with `create_new` (refusing to overwrite) unless
  `--force` is given; success is only reported after every hash verifies.
- **No oracle/FFI.**  The CLI never shells out to C; the oracle comparison
  lives in the separate `ryg-rans-rs-oracle` / bench crates.

---

## Commands

| Command | Status | Behavior |
|---------|--------|----------|
| `encode` | ✅ Implemented | Streaming block encode; RLE / rANS / RAW selection per block.  Codecs: `byte-single`, `byte-interleaved2` (default), `r64-single`, `word-single`.  Other codecs → typed error, exit 6.  Memory bounded by one block + payload + model. |
| `decode` | ✅ Implemented | Strict integrity walk: payload hash, decoded-data hash, container hash, decoded-stream hash all verified; any mismatch → exit 5.  Codecs 1, 2, 3, 5, 7; 4, 6, 8, 9, 10 → typed error (exit 6).  Explicit `--backend` → typed error (exit 6). |
| `inspect` | ✅ Implemented | Human or JSON metadata; `--blocks` lists per-block records; `--deep` decodes and verifies every block. |
| `verify` | ✅ Implemented | Full verification without writing output; human or JSON summary; exit 5 on any failure. |
| `model` | ✅ Implemented | `build` (deterministic integer-only normalizer), `inspect` (JSON), `validate` (frequency-sum check), `compare` (model equality). |
| `trace` | ✅ Implemented | Verifies the whole container, then re-decodes the selected block symbol-by-symbol (state before/after per step).  `byte-single` (codec 1) rANS blocks only; other codecs → typed error (exit 6).  Text or JSONL output. |
| `compare` | ✅ Implemented | `arithmetic` (division vs reciprocal encode, byte-identical), `backends` (dispatcher vs explicit scalar 8-way on codec-7 blocks), `files` (decoded-stream hash equality of two containers). |
| `bench` | ✅ Implemented | In-process throughput over a deterministic skewed synthetic block with a round-trip preflight; the Criterion suite remains the sealed measurement surface. |
| `capabilities` | ✅ Implemented | Compiled/runtime codec + backend inventory as JSON. |
| `completions` | ✅ Implemented | bash, fish, zsh, powershell, elvish. |

---

## Container Format

The CLI operates on the **RYGRANS v1** container
(full specification: `docs/container-format-v1.md`):

```text
┌──────────────────────────────┐
│  File Header  (32 bytes)     │  MAGIC "RYGRANS\0" · version 1.0 ·
│                              │  flags · default_codec_id ·
│                              │  default_scale_bits · default_model_mode ·
│                              │  declared_block_size
├──────────────────────────────┤
│  Block 0  (104-byte header)  │  TAG "BLK1" · kind · index · codec ·
│  + model + payload           │  scale · states · lengths ·
│                              │  payload_sha256 · decoded_sha256
│  Block 1                     │
│  ...                         │
├──────────────────────────────┤
│  Footer (104 bytes)          │  TAG "END1" · block_count ·
│                              │  total_uncompressed · total_payload ·
│                              │  container_sha256 · decoded_stream_sha256
└──────────────────────────────┘
```

Block kinds (constants in `container/mod.rs`):

| Kind | ID | Payload | Decoded-data check |
|------|----|---------|--------------------|
| RAW | 0 | Raw bytes (payload == decoded) | skipped (payload hash already proves equality) |
| RLE | 1 | 1 byte: the repeated symbol | decoded hash = hash of the expanded run |
| RANS | 2 | Canonical rANS stream + model | decoded hash = hash of the decoded output |

Block selection in `encode` is deterministic: single-symbol chunks → RLE;
empty chunks → RAW; otherwise the selected rANS codec, falling back to RAW
when the payload does not shrink the block (unless `--always-compress`).

**Strict integrity:** every block stores its own payload SHA-256 and
decoded-data SHA-256; the footer stores a container-level SHA-256 (over
header + block records) and the decoded-stream SHA-256 (over concatenated
decoded output in ascending block order).  The reader verifies all of them,
rejects trailing bytes after the footer, and rejects duplicate/out-of-order
block indexes (`ContainerReader`, `ops::walk_container`).

---

## Stable Exit Codes

Defined in `exit.rs`; propagated verbatim by the binary (code 6 is reachable
since Phase L.15):

| Code | Meaning | Triggered By |
|------|---------|-------------|
| 0 | Success | All commands on success |
| 2 | Usage error | Invalid arguments (Clap) |
| 3 | I/O error | File not found, permission denied, broken pipe |
| 4 | Container or model format error | Bad magic, truncated stream, invalid model |
| 5 | Integrity verification failure | Payload / decoded / container / decoded-stream hash mismatch |
| 6 | Unsupported codec or format version | Unknown codec, explicit `--backend`, unsupported decode codec |
| 7 | Resource limit exceeded | Input/output/block size or block count exceeds a limit |
| 8 | Parity or comparison mismatch | Arithmetic paths diverge, backends disagree, models differ |
| 9 | Requested backend unavailable | e.g. `compare backends` on a container with no 8-way blocks |
| 10 | Internal invariant failure | Invariant violation (bug) |

---

## Resource Limits

Centralized in `limits.rs` (`Limits` type) and enforced **during reading**,
not after; all accumulation uses checked arithmetic:

| Limit | Default | Hard Maximum |
|-------|---------|-------------|
| Input size | 16 GiB | — |
| Output size | 16 GiB | — |
| Block size | 1 MiB | 64 MiB |
| Payload per block | 1 MiB | — |
| Model encoding | 2 KiB | — |
| Block count | 1,000,000 | — |
| Trace symbols | 256 | — |

`Limits::parse_size` accepts plain bytes and KiB/MiB/GiB suffixes
(1024-based).

---

## Codec vs Backend Distinction

**Codecs identify the stream format** — number of states, renormalization
unit, scale constraint (`container/codec.rs`):

| ID | Name | States | Renorm | Scale |
|----|------|--------|--------|-------|
| 1 | `BYTE_SINGLE` | 1 | 8-bit | 1..=16 |
| 2 | `BYTE_INTERLEAVED2` | 2 | 8-bit | 1..=16 |
| 3 | `R64_SINGLE` | 1 | 32-bit | 1..=31 |
| 4 | `R64_INTERLEAVED2` | 2 | 32-bit | 1..=31 |
| 5 | `WORD_SINGLE` | 1 | 16-bit | 12 |
| 6 | `WORD_INTERLEAVED2` | 2 | 16-bit | 12 |
| 7 | `WORD_INTERLEAVED8` | 8 | 16-bit | 12 |
| 8 | `WORD_INTERLEAVED16` | 16 | 16-bit | 12 |
| 9 | `ALIAS_SINGLE` | 1 | 8-bit | 8..=17 |
| 10 | `ALIAS_INTERLEAVED2` | 2 | 8-bit | 8..=17 |

**Backends are the implementation choice, not the format.**  Division vs
reciprocal are arithmetic implementations; scalar vs SIMD are decode
implementations.  When two implementations produce the same canonical stream,
the distinction belongs in execution metadata, not the format ID.  The
`capabilities` command reports the compiled/runtime inventory:

```sh
ryg-rans capabilities
```

---

## Safety and Trust Boundaries

| Property | Enforcement |
|----------|-------------|
| No unsafe code | `#![forbid(unsafe_code)]` — compile-time |
| No silent fallback | unsupported codec or explicit `--backend` → typed error, exit 6 |
| Strict integrity | zero/unset decoded hashes fail; payload/decoded/container/stream hashes all verified before success |
| Decompression bombs | block size (64 MiB max), payload (1 MiB), model (2 KiB), block count (1M), input/output (16 GiB) enforced during reading |
| Integer overflow | checked arithmetic on every length accumulation |
| No overread | pre-declared lengths, bounds-checked reads |
| No overwrite | output created with `create_new`; `--force` required to replace |
| No binary to TTY | refused unless `--force-tty` |
| No trailing data | bytes after the footer are rejected |
| No unknown formats | unsupported major versions rejected; unknown flags rejected |
| No panic | production paths return typed `AppError` |
| Bounded I/O | `BoundedReader` caps input during reads (`-` = stdin included) |

Trust model: a container is **not trusted until every field, bound, and hash
has been validated**.  The reader (`container/reader.rs`) validates the
header (magic, version, header length, flags, reserved bytes, declared block
size), every block record (tag, kind, codec, scale, states, model/payload
lengths, per-block limits, payload hash, decoded hash), the footer (totals,
container hash, decoded-stream hash), and the absence of trailing data — all
before the command reports success.

---

## Architecture

```
main.rs                    → thin entry point: run() → ExitCode (verbatim)
lib.rs                     → run() dispatch + capabilities + completions + clap tree
ops/
  mod.rs                   → shared machinery: open_input/open_output (guards),
                             BoundedReader, the ONE codec dispatcher
                             (decode_block / select_block), walk_container
  encode.rs decode.rs inspect.rs verify.rs model.rs trace.rs compare.rs bench.rs
container/
  mod.rs                   → constants (magic "RYGRANS\0", sizes, block kinds, tags)
  header.rs                → FileHeader: fixed 32-byte header
  block.rs                 → Block: 104-byte header + model + payload
  footer.rs                → FileFooter: 104-byte footer + container/stream hashes
  codec.rs                 → codec registry: 10 IDs, scale validation, state counts
  model.rs                 → FrequencyModel: canonical sparse encoding, integer-only
                             deterministic normalization
  reader.rs                → ContainerReader: streaming parser with full validation
  writer.rs                → ContainerWriter: streaming serializer with hashing
error.rs                   → AppError: 11 typed variants with structured context
exit.rs                    → 10 stable exit codes + error_to_exit_code
limits.rs                  → Limits: central resource bounds, size parsing
```

Every consumer of a container (decode, inspect `--deep`, verify, compare)
runs the **same** codec dispatcher (`ops::decode_block`) — one dispatcher,
one truth: a container cannot verify under one path and decode under another.

Dependencies: `ryg-rans-rs` (facade), `ryg-rans-rs-core` (`std`),
`ryg-rans-rs-simd` (optional, default on, for the codec-7 8-way decode),
`clap` / `clap_complete`, `serde` / `serde_json`, `sha2`, `hex`.

---

## Evidence Model

| Claim | Evidence |
|-------|----------|
| All 10 subcommands wired, exit codes and stream behavior correct | 20 integration tests in `tests/cli.rs` (end-to-end via `CARGO_BIN_EXE_ryg-rans`) + `tests/model_normalizer.rs`; run with `cargo test -p ryg-rans-rs-cli` |
| Codec behavior, container format, hash semantics | The same code paths are exercised by the project's court/evidence pipeline (Phase L.19 courts OPEN) and pinned by `docs/container-format-v1.md` / `docs/bitstream-contract.md` |
| Performance | Not sealed here: the Criterion suite in `ryg-rans-rs-bench` is the measurement surface; the Phase K run is superseded (gap ledger L1-A…L1-S) and Phase L.18 re-seals.  No performance claim is marked **Sealed**. |

Claim-check path: find the claim in this README → find the producing code
path in `ops/` / `container/` → find the test that pins it → find the receipt
in `evidence/` → run `cargo xtask seal`.  If any link is missing, the claim
is not sealed.

---

## Performance Methodology

- `ryg-rans bench` is a deliberate, dependency-free **live smoke
  measurement**: a deterministic skewed corpus (symbol 0 at 64× frequency,
  xorshift-generated) of the requested size, a one-shot encode+decode
  round-trip preflight that must reproduce the input, then mean MiB/s over
  `--samples` (default 50) encode and decode iterations.  Size is bounded to
  `(0, 64 MiB]`.  It is **not** a replacement for the Criterion suite, which
  remains the sealed measurement surface (`RUSTFLAGS="-C target-cpu=native"
  cargo bench -p ryg-rans-rs-bench`).
- No throughput numbers are quoted here: the Phase K measurements are
  superseded and Phase L.18 is re-sealing.

---

## Limitations

Honest, current (Phase L.15):

- **Codec coverage is partial by design.**  Encode: byte-single,
  byte-interleaved2 (default), r64-single, word-single (others → exit 6).
  Decode: codecs 1, 2, 3, 5, 7 (8-way via SIMD/scalar); 4, 6, 8, 9, 10 →
  exit 6.  `trace` supports byte-single rANS blocks only.
- **No explicit backend selection in the CLI decoder** — only the auto
  dispatcher exists; `decode --backend <name>` → exit 6.
- **Single-threaded.**  No parallel block engine integration
  (`ryg-rans-rs-parallel` is a separate crate with its own block format).
- **`model` supports only the per-block model mode** on encode; global /
  uniform / external modes → typed error (exit 6).
- **`--arithmetic` is not selectable** on encode; the reciprocal fast path is
  always used (`--arithmetic` accepts only `auto`).
- **No signal handling** (SIGINT/SIGTERM/timeout) — gap ledger L3-D, OPEN.
- **`bench` results are smoke numbers**, not sealed measurements.

---

## Examples

```sh
# Encode a file (byte-interleaved2, 1 MiB blocks, per-block models)
ryg-rans encode -i input.dat -o output.rygr

# Encode with an explicit codec / block size
ryg-rans encode -i input.dat -o output.rygr --codec r64-single --block-size 64KiB

# Decode and verify (strict integrity; exit 5 on any hash mismatch)
ryg-rans decode -i output.rygr -o restored.dat

# Inspect container structure (--deep decodes and verifies every block)
ryg-rans inspect -i output.rygr --deep
ryg-rans inspect -i output.rygr --output-format json --blocks

# Verify integrity without writing output
ryg-rans verify -i output.rygr
ryg-rans verify -i output.rygr --output-format json

# Model tooling (binary output for container-identical bytes)
ryg-rans model build -i input.dat -o model.bin --output-format binary
ryg-rans model validate -i model.bin
ryg-rans model inspect -i model.bin
ryg-rans model compare --a model-a.bin --b model-b.bin

# Trace symbol/state transitions of block 0 (byte-single codec only)
ryg-rans trace -i output.rygr --block 0 --max-symbols 64

# Compare division vs reciprocal encoding parity
ryg-rans compare arithmetic -i input.dat

# Compare the auto dispatcher against the explicit scalar 8-way reference
ryg-rans compare backends -i output-8way.rygr

# Compare two containers by decoded-stream hash
ryg-rans compare files --a a.rygr --b b.rygr

# Smoke benchmark (the Criterion suite is the sealed surface)
ryg-rans bench --samples 50
ryg-rans bench --codec word-single --size 4MiB --output-format json

# Show capabilities / generate completions
ryg-rans capabilities
ryg-rans completions bash > /etc/bash_completion.d/ryg-rans
```

Stdio: use `-` for stdin/stdout (`cat input.dat | ryg-rans encode -i - -o
archive.rygr`); binary output to a terminal is refused unless `--force-tty`.

---

## Troubleshooting

| Symptom | Cause / Fix |
|---------|-------------|
| Exit 6 "codec '...' not implemented in the CLI encoder" | Only byte-single, byte-interleaved2, r64-single, word-single can be encoded |
| Exit 6 "explicit backend ... not implemented in the CLI decoder" | Only the auto dispatcher exists; drop `--backend` or use `verify --backend all-available` |
| Exit 6 "codec N not supported by the CLI decoder" | Decode implements codecs 1, 2, 3, 5, 7 only |
| Exit 5 integrity failure | Some hash (payload, decoded, container, decoded-stream) mismatched; use `inspect --deep` to localize the block |
| Exit 4 "trailing data after footer" / "expected block or footer" | The container has extra bytes after the footer or a corrupt block tag |
| Exit 7 resource limit | Input/block/payload/model/block-count bound exceeded; the message states the limit and requested value |
| Exit 9 "container has no 8-way (codec 7) blocks" | `compare backends` needs codec-7 blocks; encode with `--codec word-interleaved8`? — not supported by the CLI encoder, so compare against a container produced elsewhere |
| Exit 3 I/O error | File missing/unreadable, output exists without `--force`, or binary output to a terminal without `--force-tty` |
| `decode` writes partial output before failing | Decoded bytes are streamed as blocks are verified; a failure mid-container leaves partial output on disk — use `verify` for a no-write integrity check |

---

## Versioning

- Version **0.2.0**, shared with the workspace.  Binary name `ryg-rans`.
- Exit codes are stable once documented (`exit.rs`) — changing one is a
  breaking change for automation.
- The RYGRANS v1 container format is pinned by `docs/container-format-v1.md`;
  format changes require a new major container version.

---

## Reading Order

1. `docs/glossary.md` — exact project terminology.
2. Root `README.md` — evidence status, CLI overview, exit codes.
3. `docs/container-format-v1.md` — the RYGRANS v1 specification this crate
   implements.
4. `docs/bitstream-contract.md` — the pinned upstream stream formats.
5. `src/lib.rs`, then `ops/` and `container/` module docs.
6. `evidence/phase-l/gap-ledger.md` — residuals touching the CLI (L.15).

---

*Part of the ryg-rans-rs project. Version 0.2.0. Phase L.*
