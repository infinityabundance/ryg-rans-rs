# ryg-rans-rs-simd

> **SIMD-accelerated Word rANS decode kernels — SSE4.1, AVX2, AVX512VL, AVX-512.**
> `#![no_std]`, x86_64-only, machine-verified unsafe surface.
> 8-way and 16-way interleaved decode with scalar references and an exact-backend contract.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](../../LICENSE-APACHE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-simd)](https://crates.io/crates/ryg-rans-rs-simd)

**Version: 0.4.0** (workspace) · default build: 27 tests (22 unit + 4 disassembly courts +
1 ledger test) · native build: 58 unit tests + 1 ignored (`--release` only) + courts ·
43 ledgered `unsafe fn`

---

## Table of Contents

1. [What This Crate Does](#what-this-crate-does)
2. [The Kernel Surface](#the-kernel-surface)
3. [The Packed Table Design](#the-packed-table-design)
4. [What This Crate Does NOT Do](#what-this-crate-does-not)
5. [Trust Boundaries and Input Invariants](#trust-boundaries-and-input-invariants)
6. [Resource Behaviour](#resource-behaviour)
7. [Backend Semantics](#backend-semantics)
8. [SIMD Requirements](#simd-requirements)
9. [Unsafe Boundaries](#unsafe-boundaries)
10. [Evidence Model](#evidence-model)
11. [Performance Methodology](#performance-methodology)
12. [Limitations](#limitations)
13. [Examples](#examples)
14. [Troubleshooting](#troubleshooting)
15. [Versioning](#versioning)
16. [Reading Order](#reading-order)

---

## What This Crate Does

This crate implements SIMD-accelerated **Word rANS decode kernels** on the mathematical
foundation of `ryg-rans-rs-core`. Word rANS decode has three computational phases:

1. **Table lookup**: `slot = state & 4095`, then fetch `frequency`, `bias`, `symbol` —
   an address-dependent gather.
2. **State update**: `state = frequency × (state >> 12) + bias` — a lane-wise
   multiply-add.
3. **Renormalization**: lanes with `state < 65536` consume one `u16` each from the
   stream.

The SIMD kernels accelerate phases 1–3 with SSE4.1, AVX2, and AVX-512 intrinsics while a
pure-scalar reference (`decode_8way_scalar`, `decode_8way_packed_scalar`,
`decode_interleaved16_scalar`) defines the byte-exact expected output that every SIMD
kernel must match.

The crate provides:

- **Scalar references** — `decode_8way_scalar` (legacy `RansWordTables`),
  `decode_8way_packed_scalar[_with_report]`, `decode_8way_packed_scalar_into`,
  `decode_interleaved16_scalar`, `decode_interleaved16_scalar_into`.
- **16-way encoder** — `encode_interleaved16` (the 8-way encoder is a test helper,
  `encode_8way_for_test`; see [Limitations](#limitations)).
- **The packed table** — `PackedWordTable` / `PackedWordEntry` (16 KB, 64-byte aligned,
  validated construction) and the legacy `RansWordTables` / `RansWordSlot` + builders
  `build_word_tables` / `rans_word_tables_init_symbol`.
- **Backend identification and dispatch** — the `backends` module (`DecodeBackend`,
  `DecodeResult`, `DecodeError`, `_auto` / `_scalar` / `_checked` functions).
- **AVX2 kernels** (`avx2`, `avx2_renorm`) and **AVX-512 kernels** (`avx512`,
  `model_kernels`) — see the next section.

---

## The Kernel Surface

### SSE4.1 8-way (`lib.rs`)

Two 4-lane SIMD units (`RansSimdDec`, an `__m128i` wrapper) decode 8 symbols per
iteration from the canonical 8-way stream. SSE4.1 has no gather, so each lane's table
entry is extracted, loaded as a scalar, and re-inserted — the extraction serialization is
the kernel's dominant cost. Renormalization uses 16 precomputed shuffle masks
(`SHUFFLE_MASKS`, aligned) and a signed-compare bias trick (`state ^ i32::MIN`) to
implement `state < RANS_WORD_L` without an unsigned compare.

- Safe compile-time-gated wrapper: `decode_simd_8way` (uses the SIMD path when
  `target_feature = "sse4.1"` is compiled in, otherwise the scalar reference — a
  compile-time fallback, not a runtime one).
- Unsafe, locally-gated kernels: `decode_simd_8way_unchecked`,
  `decode_simd_8way_unchecked_with_report` (the report variant returns
  `words_consumed` + 8 final states for cross-backend report parity — Phase L.11 fix),
  and the primitives `rans_simd_dec_init` (SSE2 only), `rans_simd_dec_sym_unchecked`
  (SSE4.1), `rans_simd_dec_renorm_unchecked` (SSSE3+SSE4.1).
- Safe runtime-checked wrapper: `decode_interleaved8_sse41_checked` (takes the legacy
  `RansWordTables`).

### AVX2 backends (`avx2.rs`, `avx2_renorm.rs`)

AVX2 decode on CPUs without AVX-512 (Intel pre-Ice Lake, AMD Zen 1–4):

| Kernel | Format | Notes |
|--------|--------|-------|
| `decode_interleaved8_avx2_manual_gather[_into]` | 8-way | Scalar loads + vector arithmetic (no `vpgatherdd`) |
| `decode_interleaved8_avx2_hardware_gather[_into]` | 8-way | `_mm256_i32gather_epi32` |
| `decode_interleaved16_avx2_2x8[_into]` | 16-way | Two independent 256-bit chains |
| `decode_interleaved16_uniform256_avx2[_into]` | 16-way | Uniform256 table-free (no table, no gather) |
| `decode_batch4_interleaved16_avx2` | 4 × 16-way | Round-robin over up to 4 independent streams (`Avx2DecodeJob`) |

All AVX2 kernels share the renormalization primitive `renorm8_avx2`, which uses a
precomputed **permutation table** (`Avx2RenormPermutations`, 256 masks × 8 `i32` =
8 KB, 32-byte aligned, built by `build_avx2_renorm_table`) so that
`_mm256_permutevar8x32_epi32` distributes compact renorm words to the active lanes in
constant time. `Avx2Context` bundles the permutation table for callers that want to
build it once.

### AVX-512 backends (`avx512.rs`)

The `avx512` module is gated by `#![cfg(target_feature = "avx512bw")]` — see
[SIMD Requirements](#simd-requirements) for why this gate is load-bearing.

| Kernel | Width | Format | ISA |
|--------|-------|--------|-----|
| `decode_interleaved8_avx512vl_kernel` / `_into` | 256-bit, 8 lanes | Canonical 8-way | `avx512f+vl+bw` |
| `decode_interleaved16_avx512_kernel` / `_into` | 512-bit, 16 lanes | 16-way | `avx512f+bw` |
| `decode_interleaved8_manual_gather[_into]` / `_kernel` | 8 lanes | 8-way | `avx512f+vl+bw` |
| `decode_interleaved16_manual_gather[_into]` / `_kernel` | 16 lanes | 16-way | `avx512f+bw` |
| `decode_interleaved16_2x8[_into]` / `_kernel` | 2 × 256-bit | 16-way | `avx512f+vl+bw` |
| `decode_batch_interleaved16_avx512` | 512-bit | 4 × 16-way | `avx512f+bw` |
| `renorm8_avx512vl` / `renorm16_avx512` | — | renorm primitives | `avx512f+vl+bw` / `avx512f+bw` |

The 8-way kernels are drop-in replacements for the canonical 8-way format (same stream
as scalar/SSE4.1/AVX2). The 16-way kernels consume the **new 16-way format**:

- Symbols are encoded in reverse order, assigned to `lane = i & 15`.
- States are flushed in **reverse lane order** (15 → 0) because the writer moves
  backward; the forward reader therefore initializes lanes in ascending order (0 → 15),
  each state as `[low16, high16]` (32 `u16` words total).
- Renormalization mask via `_mm256_cmplt_epu32_mask` / `_mm512_cmplt_epu32_mask`; words
  are compacted into a scratch buffer and distributed with `maskz_expand_epi32`
  (no over-read; inactive lanes never touch caller memory).
- Tails (1–7 / 1–15 symbols) fall back to scalar per-lane logic.

The manual-gather kernels replace the hardware `i32gather` with explicitly unrolled
scalar loads; they serve as a correctness baseline and a performance comparison for the
gather instruction on a given microarchitecture.

### Model-specialized kernels (`model_kernels.rs`, AVX-512BW builds)

- `decode_interleaved16_uniform256_avx512[_into]`: the Uniform256 table-free kernel.
  For a model where every symbol's frequency is exactly `2^(scale_bits-8)` (= 16 at
  scale 12), the decode reduces to pure arithmetic: `symbol = slot >> 4`,
  `bias = slot & 15`, `new_state = 16 × (state >> 12) + bias`. No gather, no table.
  The caller **must** verify the model is Uniform256 before dispatching
  (`check_uniform256`); see [Limitations](#limitations).
- `decode_interleaved16_dominant_sketch`: **intentionally unimplemented placeholder** —
  always returns `Err`; do not use.

---

## The Packed Table Design

### Why a separate table format?

The legacy representation (`RansWordSlot` + `slot2sym`) uses two separate arrays (16 KB +
4 KB). A SIMD gather loads from a single address stream, so two arrays would require two
gathers per iteration. The packed representation puts all three fields in one `u32`:

```text
bits  0..11   frequency  (12 bits)  — `entry & 0x0fff`
bits 12..23   bias       (12 bits)  — `(entry >> 12) & 0x0fff`
bits 24..31   symbol     (8 bits)   — `(entry >> 24) as u8`
```

- **Single gather**: one `_mm256_i32gather_epi32` / `_mm512_i32gather_epi32` loads all
  fields for the whole vector.
- **16 KB table** (`PackedWordTable`, 4096 entries × 4 bytes): L1-resident on modern
  x86 CPUs.
- **64-byte alignment** (`#[repr(align(64))]`): cache-line-aligned base for gather and
  aligned vector loads.

### Construction and validation

`PackedWordTable::from_freqs(&freqs, &cum_freqs, scale_bits)` returns
`Result<Self, ryg_rans_rs_core::ModelError>` and validates:

1. `scale_bits == 12` (`RANS_WORD_SCALE_BITS`).
2. Exact dimensions: 256 frequencies, 257 cumulative frequencies.
3. `cum[0] == 0`, `cum[256] == 4096`, monotonic cumulative, and
   `cum[i+1] - cum[i] == freqs[i]` for every symbol.
4. Every slot (0..4095) is covered by a symbol (no zero-frequency holes).

`PackedWordTable::verify_equivalence(&slots, &slot2sym)` compares all 4096 entries
against the legacy representation and reports the exact slot on mismatch. The Kani proofs
`kani_packed_entry_fields`, `kani_state_update_no_overflow`, and `kani_slot_index_bounded`
(in `ryg-rans-rs-core/kani/packed_entry_proof.rs`) prove the packing round-trips, the
state update cannot overflow `u32`, and the slot index is always in `0..4096`.

---

## What This Crate Does NOT Do

| Not in this crate | Where it lives |
|-------------------|----------------|
| No general 8-way **encoder** — `encode_8way_for_test` is a test/verification helper, not a production encoder | Encoding is the caller's job (core primitives, parallel crate, oracle) |
| No byte rANS / R64 SIMD — SIMD acceleration targets Word rANS decode only | `ryg-rans-rs-core` (scalar byte/R64) |
| No runtime auto-selection of SIMD — `decode_interleaved8_auto` / `decode_interleaved16_auto` always dispatch to the scalar backend by design (see [Backend Semantics](#backend-semantics)) | `backends.rs` |
| No parallel engine, no worker pool, no block container | `ryg-rans-rs-parallel`, `ryg-rans-rs-cli` |
| No I/O beyond in-memory slices | readers/writers are caller-provided |
| No frequency normalization or model building beyond the table constructors | `ryg-rans-rs-parallel` |
| No execution on non-x86_64 targets | `#![cfg(target_arch = "x86_64")]` at crate root |

---

## Trust Boundaries and Input Invariants

Malformed input produces **typed errors, never panics, never over-reads**:

- **Truncated streams**: every decoder checks the initial-state count (16 `u16` words for
  8-way, 32 for 16-way) and every renormalization read, returning
  `DecodeError::InputTooShort` (safe wrappers) or `Err(&'static str)` (low-level
  kernels). The AVX-512 malformed-input tests (`malformed_input_tests.rs`) pin empty,
  partially-initialized, and truncated-during-decode streams.
- **No over-read**: renormalization words are copied into a fixed scratch buffer before
  any vector load; inactive lanes never read caller memory (`maskz_expand_epi32` with
  zero-masking). The `_into` kernels require `output.len() == symbol count` and return
  `DecodeError::OutputLengthMismatch`/`InputTooShort` otherwise.
- **Tables are validated at construction**: `PackedWordTable::from_freqs` enforces the
  model invariants above; the unsafe kernels document "table must have 4096 entries" as a
  caller invariant (unreachable through the public API for `PackedWordTable`).
- **Uniform256 kernels require caller model validation**: dispatching
  `decode_interleaved16_uniform256_*` on a non-uniform model silently produces wrong
  output — the caller must check the model with `check_uniform256` (used by the parallel
  crate's plan validation).
- **`decode_interleaved16_dominant_sketch`** is an unimplemented placeholder that always
  returns `Err` — it can never decode anything.

The `backends::DecodeError` enum is distinct from the core crate's `DecodeError` and
adds: `InvalidTable`, `UnsupportedBackend`, `OutputLengthMismatch`, `TrailingData`,
`StateInvariantViolation`.

---

## Resource Behaviour

- **Allocating decoders** (`decode_interleaved8_auto`, `decode_interleaved16_auto`, the
  explicit kernels without `_into`) allocate the output `Vec<u8>` (size = expected_len).
- **`_into` variants** write into a caller-provided `&mut [u8]` and return only the
  report (`DecodeReport` / `DecodeReport8`: `words_consumed` + 16/8 final states) — no
  allocation in the decode loop.
- **`PackedWordTable`** is a 16 KB heap allocation (4096 × 4 bytes, 64-byte aligned,
  `Box<[PackedWordEntry; 4096]>`).
- **`encode_interleaved16`** allocates up to `2 × symbols.len() + 64` `u16` words,
  capped at 128 MiB (`Encode16Error::BufferOverflow` beyond that).
- **`build_avx2_renorm_table`** builds an 8 KB table (256 × 8 × `i32`) per call; the
  `_checked` AVX2 wrappers build it on every call, and `Avx2Context::new()` builds it
  once for reuse.
- **The crate is `no_std`** but links `alloc` unconditionally (`extern crate alloc`);
  the `std` feature only enables runtime CPUID detection and `extern crate std`.

---

## Backend Semantics

Per the project glossary, **requested backend** is what the caller asked for and
**executed backend** is what actually ran; with the exact-backend contract,
requested == executed or the call returns a typed error — **silent substitution is
prohibited**. This crate implements that contract as follows:

- Every decode result carries the executed backend: `DecodeResult.backend` is a
  `DecodeBackend` variant with a stable, immutable `label()`:
  `scalar-8way`, `sse41-8way`, `avx512vl-8way`, `scalar-16way`, `avx512-16way`,
  `avx512vl-manual-gather-8way`, `avx512-manual-gather-16way`, `avx512vl-2x8-on16`,
  `avx2-manual-gather-8way`, `avx2-hardware-gather-8way`, `avx2-2x8-on16`,
  `avx2-uniform256-tablefree-16way`, `avx2-batch4-on16`. These labels are used verbatim
  in court receipts and benchmark preflight records.
- **Safe `_checked` wrappers** (`decode_interleaved8_sse41_checked`,
  `decode_interleaved8_avx2_manual_gather_checked`,
  `decode_interleaved8_avx2_hardware_gather_checked`,
  `decode_interleaved16_avx2_2x8_checked`,
  `decode_interleaved16_uniform256_avx2_checked`,
  `decode_batch4_interleaved16_avx2_checked`,
  `decode_interleaved8_avx512vl_checked`,
  `decode_interleaved16_avx512_checked`,
  `decode_interleaved8_avx512vl_manual_gather_checked`,
  `decode_interleaved16_avx512_manual_gather_checked`,
  `decode_interleaved16_avx512vl_2x8_checked`): runtime feature detection first; on an
  unsupported CPU they return `Err(DecodeError::UnsupportedBackend)` — the kernel is
  **never executed**, so `Ok` always means the exact requested backend ran.
- **Explicit `unsafe` kernels** (`decode_interleaved8_avx512vl`, `decode_interleaved16_avx512`,
  the manual-gather and 2×8 variants): no detection; the caller guarantees the CPU
  features. On builds without `target_feature = "avx512bw"` the kernel bodies are not
  compiled and these wrappers return `Err(UnsupportedBackend)` (the inputs are
  acknowledged, never silently ignored).
- **`_auto` dispatch is deliberately conservative**: `decode_interleaved8_auto` and
  `decode_interleaved16_auto` always execute the scalar backend (documented rationale:
  on the measured Zen 5 host the L1-resident scalar table beats the SIMD gather paths).
  They do **not** silently pick SIMD. Callers who want a SIMD backend must request it
  explicitly — which is exactly what the parallel crate's plan does, recording
  requested and executed separately.
- **`decode_simd_8way`** (legacy surface) is the one deliberate *compile-time* fallback:
  it uses the SIMD path when compiled with SSE4.1 and the scalar reference otherwise —
  a build-time decision, not a runtime substitution.

---

## SIMD Requirements

### ISAs

| Backend family | Required features |
|----------------|-------------------|
| SSE4.1 8-way | `ssse3` + `sse4.1` (renorm primitives); init primitive is baseline SSE2 only |
| AVX2 (all kernels) | `avx2` |
| AVX512VL 8-way / 2×8-on-16 / manual-gather 8 | `avx512f` + `avx512vl` + `avx512bw` |
| AVX512 16-way / manual-gather 16 | `avx512f` + `avx512bw` |

### Compile-time flags

The crate targets baseline x86_64 by default. To compile SIMD code in, pass the features
explicitly:

```sh
RUSTFLAGS="-C target-cpu=native" cargo build          # host-native
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx2,+avx512f,+avx512vl,+avx512bw" cargo build
```

The `avx512` and `model_kernels` modules are gated with
`#![cfg(target_feature = "avx512bw")]`. **This gate is load-bearing**: rustc 1.96 does
*not* reject feature-gated intrinsics on builds without the features (the code still
compiles and the instruction is still emitted — which would SIGILL on a CPU lacking
AVX-512). The `disasm_court` test `court_avx512_intrinsics_require_crate_level_cfg_gate`
documents this toolchain reality and verifies the crate's defensive gate exists, so
portable builds never *contain* AVX-512 code.

### Runtime detection

Two-tier, in `backends.rs`:

- With the `std` feature: `std::is_x86_feature_detected!(...)` (CPUID at runtime).
- Without `std`: `cfg!(target_feature = "...")` (compile-time features only).

The `#[doc(hidden)]` public helpers `sse41_available_checked`, `avx2_available_checked`,
`avx512vl_available_checked`, `avx512_available_checked` expose the checks to benchmark
tooling; `check_uniform256(model_data, scale_bits)` tests whether a model is Uniform256.

### What happens on unsupported CPUs

- **Safe `_checked` wrappers**: `Err(DecodeError::UnsupportedBackend)`; nothing executes.
- **Safe `_auto` / `_scalar` / `decode_simd_8way` (non-SSE4.1 builds)**: scalar decode —
  this is a *fallback*, and it is the entire executed path (the backend label records
  `scalar-*`).
- **Explicit `unsafe` kernels**: undefined behavior if the CPU lacks the features — the
  caller contract (documented in each `# Safety` section) is to check first.

---

## Unsafe Boundaries

The crate's unsafe surface is **machine-verified**:

- `crates/ryg-rans-rs-simd/unsafe-ledger.toml` inventories every `unsafe fn` (currently
  43 entries across `lib.rs`, `avx2.rs`, `avx2_renorm.rs`, `avx512.rs`, `backends.rs`,
  `model_kernels.rs`, and the `cfg(test)` helper in `avx2_tests.rs`), each with its exact
  `#[target_feature]` attributes, a safety summary, and its caller list.
- The `unsafe_ledger` test (`tests/unsafe_ledger.rs`) checks the ledger ↔ source
  inventory **bidirectionally**: every ledger entry must exist in the source and every
  source `unsafe fn` must be in the ledger, and for explicit feature lists the
  `#[target_feature(enable = "...")]` attribute immediately above the declaration must
  match the ledger exactly. Ledger entries marked `delegates`, `test-only`, or
  `baseline` must *not* carry the attribute. The test fails if ledger and source
  disagree.
- Every `unsafe fn` carries its own exact `#[target_feature]` attributes (Phase L.10
  quarantine) and a `# Safety` section stating pointer provenance, bounds, alignment,
  CPU-feature requirements, and the caller list. No hidden caller obligation exists that
  could be encoded in a safe type.
- **Disassembly courts** (`tests/disasm_court.rs`, run on any x86_64 host) compile
  minimal kernels with explicit target features and assert the expected mnemonics are
  emitted: `pshufb`/`pblendvb` (SSE4.1 8-way), `vpermd`/`vpgatherdd` (AVX2), `vpmovdb`
  (AVX-512 16-way), and the cfg-gate court described above.
- The SSE4.1 renorm uses an aligned `SHUFFLE_MASKS` static (`#[repr(align(16))]`) and a
  stack scratch so variable-count word injection never over-reads.

The safety-relevant residuals behind this design are **L10-A** (local
`#[target_feature]` on every SSE helper) and **L10-B** (bidirectional ledger + disassembly
courts), both RESOLVED in `evidence/phase-l/gap-ledger.md`.

---

## Evidence Model

### Behaviour

Three of the project's seven behaviour-sealed surfaces are this crate's (Phase K
baseline, all `Sealed`):

| Surface | Receipts | Court ID prefix |
|---------|----------|-----------------|
| SSE4.1 SIMD decoder — 8-way interleaved | 8 | `RYG_RANS.SIMD.*` |
| AVX512VL.INTERLEAVED8 | 8 | `RYG_RANS.AVX512VL.*` |
| AVX512.INTERLEAVED16 | 8 | `RYG_RANS.AVX512.*` |

The AVX2 portability tier is **Test-verified** (behaviour receipts pending; Phase L.19
courts). Additional court IDs referenced from the source: `RYG_RANS.L.SSE41.UNSAFE_QUARANTINE`
(SSE4.1 unsafe-fn contract). Every receipt is SHA-256-chained with a canonical self-hash
(`evidence/receipts/`, `evidence/manifests/`, `evidence/index.json`) and verified by the
seal gate.

### Performance

Performance receipts (`RYG_RANS.PERF.*`) are **re-sealing in Phase L.18**. The Phase K
run (`evidence/performance/runs/phase-k-*`) is retained as superseded evidence but has
documented defects (residuals L1-A…L1-S) and must not be cited as current. The new
pipeline is `cargo xtask benchmark-run` → `cargo xtask performance-seal` → `cargo xtask
seal` (see `xtask/README.md`).

### Tests and courts

- Default build (`cargo test -p ryg-rans-rs-simd`): 22 unit tests + 4 disassembly courts
  + 1 unsafe-ledger test = **27 tests passing**.
- Native build (`RUSTFLAGS="-C target-cpu=native"`): 58 unit tests pass, 1 ignored
  (`test_16way_exhaustive_simd_renorm` — all 65,536 16-way renorm masks, `--release`
  only), plus the 4 disassembly courts and the 1 ledger test.
- The AVX-512 test modules (`malformed_input_tests`, `mask_tests`,
  `optimization_tests`, and the `avx512.rs` module tests) compile only when
  `target_feature = "avx512bw"` is enabled; the AVX2 tests compile under
  `target_feature = "avx2"`.
- Fuzz targets in the standalone `fuzz/` workspace include `avx512vl8_roundtrip` and
  `avx512_16way_roundtrip` (scalar/AVX-512 equivalence on random inputs).

---

## Performance Methodology

The Criterion suite lives in **`ryg-rans-rs-bench`** (13 bench targets: `scalar`,
`sse41`, `avx2`, `avx512`, `batch`, `specialized`, `dispatch`, `parallel`, `container`,
`byte_rans`, `r64`, `alias`, and the Phase L.14 `comparative` court). Measurement
discipline:

1. **Verification before timing**: every bench case verifies byte-exact output,
   words-consumed, and final states against the scalar reference before the timing loop
   (`verify_8way` / `verify_16way` in `ryg-rans-rs-bench::common::verification`).
2. **Preflight records**: backend requested/executed, input/output hashes, words
   consumed, final states, thread counts — joined to Criterion timing by exact benchmark
   ID (see `docs/performance-method.md`).
3. **Evidence generation**: `cargo xtask benchmark-run --criterion-dir target/criterion
   --implementation-commit <sha>` then `cargo xtask performance-seal`.

Historical Phase K findings (Zen 5 / Ryzen 7 9800X3D, rustc 1.96, `--release`) are
**superseded** and are reported only in the root README's "Phase K key findings"
section (e.g. scalar-16way ≈ 1.45 GiB/s uniform256, AVX2-uniform256 ≈ 1.47 GiB/s, SSE4.1
≈ 406 MiB/s). This README intentionally cites **no current throughput numbers**: the
Phase L.18 re-seal is the only current measurement surface.

---

## Limitations

- **Auto-dispatch never picks SIMD**: `decode_interleaved8_auto` /
  `decode_interleaved16_auto` are scalar-only. SIMD backends require explicit requests
  (`_checked` wrappers or unsafe kernels) — by design, not omission.
- **The 8-way encoder is test-only** (`encode_8way_for_test`). Production 8-way
  encoding is the caller's responsibility (core primitives / parallel crate).
- **`decode_interleaved16_uniform256_*` requires caller model validation** — wrong
  output on non-uniform models; no runtime check exists inside the kernel.
  `decode_interleaved16_dominant_sketch` is an **unimplemented placeholder** (always
  `Err`).
- **Batch4 is not reachable through a one-block API**: it requires a coordinator-level
  batch context (`Avx2DecodeJob` / `DecodeJob` job arrays); the glossary documents this
  rule.
- **x86_64 only** (`#![cfg(target_arch = "x86_64")]`); AVX-512 kernels additionally
  require an `avx512bw` build, without which the explicit AVX-512 wrappers return
  `UnsupportedBackend`.
- **`_auto`/`_scalar` never verify CPU features** — they are scalar by definition and
  always run.
- **SSE4.1 checked wrapper takes the legacy `RansWordTables`** while AVX2/AVX-512 take
  `PackedWordTable` — the two table types are verified equivalent by
  `verify_equivalence` and the Kani packed-entry proofs.
- **Runtime detection without `std` is compile-time only**: in `no_std` builds the
  `_checked` wrappers succeed only when the target features were enabled via
  `RUSTFLAGS`; they cannot discover a CPU's capabilities at runtime.
- The CLI wires only a subset of codecs (decode: codecs 1, 2, 3, 5, 7 — 8-way via
  SIMD/scalar) — see the workspace `AGENTS.md` "Current limitations" section.

---

## Examples

### 16-way round-trip: encode, packed table, auto decode

```rust
use ryg_rans_rs_simd::{
    backends::decode_interleaved16_auto,
    packed_table::{PackedWordTable, encode_interleaved16},
};

let total = 1u32 << 12; // scale_bits = 12
let base = total / 256; // uniform-256: every symbol has frequency 16
let mut freqs = [base; 256];
freqs[255] += total - freqs.iter().sum::<u32>();
let mut cum = [0u32; 257];
for i in 0..256 {
    cum[i + 1] = cum[i] + freqs[i];
}

let table = PackedWordTable::from_freqs(&freqs, &cum, 12).unwrap();
let compressed = encode_interleaved16(b"hello world", &freqs, &cum, 12).unwrap();
let result = decode_interleaved16_auto(&compressed, &table, 11).unwrap();

assert_eq!(result.output.as_slice(), b"hello world".as_slice());
assert_eq!(result.backend.label(), "scalar-16way"); // executed backend, recorded
```

### Exact-backend request: AVX-512 16-way

```rust
use ryg_rans_rs_simd::backends::decode_interleaved16_avx512_checked;

match decode_interleaved16_avx512_checked(&compressed, &table, expected_len) {
    Ok(result) => {
        // Ok means the Avx512Interleaved16 kernel ACTUALLY executed —
        // never a silent scalar fallback.
        assert_eq!(result.backend.label(), "avx512-16way");
    }
    Err(e) => {
        // e == DecodeError::UnsupportedBackend when the CPU lacks
        // AVX512F+BW or the build lacks -C target-feature=+avx512f,+avx512bw.
        // The kernel was not executed.
    }
}
```

### Scalar 8-way reference with the legacy table

```rust
use ryg_rans_rs_simd::{
    build_word_tables, encode_8way_for_test, decode_8way_scalar, RansWordTables,
};

let total = 1u32 << 12;
let base = total / 256;
let freqs = vec![base; 256]; // sums to 4096 exactly
let mut cum = vec![0u32; 257];
for i in 0..256 {
    cum[i + 1] = cum[i] + freqs[i];
}

let (slots, slot2sym) = build_word_tables(&freqs, &cum, 12);
let tables = RansWordTables { slots: &slots, slot2sym: &slot2sym };

let compressed = encode_8way_for_test(b"hello world", &freqs, &cum);
let out = decode_8way_scalar(&compressed, &tables, 11).unwrap();
assert_eq!(out.as_slice(), b"hello world".as_slice());
```

All function names above are verified against the current crate source
(`src/lib.rs`, `src/backends.rs`, `src/packed_table.rs`).

---

## Troubleshooting

| Symptom | Likely cause / fix |
|---------|--------------------|
| `Err(UnsupportedBackend)` from a `_checked` call | The CPU lacks the ISA, or the build lacks the features. For AVX-512: rebuild with `RUSTFLAGS="-C target-feature=+avx512f,+avx512vl,+avx512bw"`; in `no_std` builds there is no runtime CPUID, so the flags are the only signal. |
| SIMD never runs via `decode_interleaved8_auto` / `decode_interleaved16_auto` | By design the auto path is scalar. Request the SIMD backend explicitly (`_checked` wrappers) or go through the parallel crate's plan, which records requested vs executed. |
| SIGILL at runtime | An explicit `unsafe` kernel was called on a CPU without the required features. Every `unsafe fn` documents its CPU-feature contract in its `# Safety` section; check with the `*_available_checked` helpers first. |
| Wrong output from `decode_interleaved16_uniform256_*` | The model was not Uniform256. Validate with `check_uniform256` before dispatch — the kernel performs no model check. |
| `decode_interleaved16_dominant_sketch` returns `Err` | It is an intentionally unimplemented placeholder. Do not use. |
| `cargo test` shows fewer tests than expected | The AVX-512 test modules compile only under `target_feature = "avx512bw"`; rebuild with `RUSTFLAGS="-C target-cpu=native"` (see [SIMD Requirements](#simd-requirements)). |
| Ledger test fails | `unsafe-ledger.toml` and the source inventory disagree. Change the source and update the ledger together; the test enforces equality bidirectionally. |

---

## Versioning

`0.4.0` (workspace-wide). Format invariants that apply to every kernel in this crate:

- **Bit-exact stream parity** with the pinned upstream format: the 8-way and 16-way
  stream layouts are frozen by `docs/bitstream-contract.md`; any change to an encoded
  stream is a breaking format change.
- **Backend labels are immutable**: `DecodeBackend::label()` strings are used in court
  receipts and preflight records; changing them breaks evidence-chain integrity.
- **Determinism**: same input → same output and same canonical error, independent of
  worker count, completion order, or schedule.
- The unsafe ledger is regenerated/updated together with the source (bidirectional
  test), never by hand alone.

## Reading Order

1. Root [`README.md`](../../README.md) — project framing and Evidence Status.
2. [`docs/architecture.md`](../../docs/architecture.md)
3. [`docs/bitstream-contract.md`](../../docs/bitstream-contract.md) — the pinned stream formats.
4. [`docs/glossary.md`](../../docs/glossary.md) — the exact terminology used here.
5. [`docs/unsafe-ledger.md`](../../docs/unsafe-ledger.md)
6. [`docs/performance-method.md`](../../docs/performance-method.md)
7. [`AGENTS.md`](../../AGENTS.md) — ground truth for contributors.
8. The [`ryg-rans-rs-core`](../ryg-rans-rs-core/README.md) README — the scalar arithmetic
   these kernels accelerate.
9. `crates/ryg-rans-rs-simd/unsafe-ledger.toml` — the machine-verified unsafe inventory.

10. `docs/papers/0002-word-rans.md` and `docs/papers/0003-simd.md` — the table layout and kernel design; `docs/adr/0003` and `0011` — the scale-pin and unsafe-quarantine decisions.
---

*Part of the ryg-rans-rs project. Version 0.4.0.*
