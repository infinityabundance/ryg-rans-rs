# ryg-rans-rs-core

> **The deterministic algorithmic core of rANS entropy coding — pure safe Rust.**
> `#![no_std]` + `#![forbid(unsafe_code)]` — zero-allocation encode/decode hot paths.
> Byte rANS · R64 rANS · Word rANS · Alias (Vose) — division and reciprocal paths.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](../../LICENSE-APACHE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-core)](https://crates.io/crates/ryg-rans-rs-core)
[![docs.rs](https://img.shields.io/docsrs/ryg-rans-rs-core)](https://docs.rs/ryg-rans-rs-core/latest/ryg_rans_rs_core/)

**Version: 0.4.0** (workspace) · 57 unit tests · 9 Kani proofs · 4 behaviour-sealed surfaces
(120 of the project's 144 Phase-K behavioural receipts)

---

## Table of Contents

1. [What This Crate Does](#what-this-crate-does)
2. [Understanding the rANS Arithmetic](#understanding-the-rans-arithmetic)
3. [What This Crate Does NOT Do](#what-this-crate-does-not)
4. [Trust Boundaries and Input Invariants](#trust-boundaries-and-input-invariants)
5. [Resource Behaviour](#resource-behaviour)
6. [Backend Semantics](#backend-semantics)
7. [SIMD Requirements](#simd-requirements)
8. [Unsafe Boundaries](#unsafe-boundaries)
9. [Evidence Model](#evidence-model)
10. [Performance Methodology](#performance-methodology)
11. [Limitations](#limitations)
12. [Examples](#examples)
13. [Troubleshooting](#troubleshooting)
14. [Versioning](#versioning)
15. [Reading Order](#reading-order)

---

## What This Crate Does

This crate is a **from-scratch, native Rust reconstruction** of the scalar rANS
encoder/decoder variants published in Fabian Giesen's public-domain
[`ryg_rans`](https://github.com/rygorous/ryg_rans) repository. It is **not** a wrapper,
binding, or FFI facade — every arithmetic operation is independently implemented and
verified against the compiled C/C++ reference through the project's parity courts.

It exposes **four algorithmic surfaces** built on the same ANS state machine, each
targeting a different throughput/latency trade-off:

| Surface | State width | Renorm unit | Key constants | Upstream origin |
|---------|-------------|-------------|---------------|-----------------|
| **Byte rANS** | `u32` (31-bit effective) | `u8` (byte) | `RANS_BYTE_L = 1 << 23` | `rans_byte.h` |
| **R64 rANS** | `u64` (63-bit effective) | `u32` (word) | `RANS64_L = 1 << 31` | `rans64.h` |
| **Word rANS** | `u32` | `u16` (16-bit word) | `RANS_WORD_L = 1 << 16`, `RANS_WORD_SCALE_BITS = 12` | `rans_word_sse41.h` |
| **Alias rANS** | `u32` (byte rANS) | `u8` (byte) | `ALIAS_LOG2_NSYMS = 8` | `main_alias.cpp` |

All four surfaces share common structure:

- **Division-based reference path**: `C(s, x) = ((x / freq) << scale_bits) + (x % freq) + start`.
- **Reciprocal-multiply fast path** (Byte, R64): Alverson's "Integer Division using
  Reciprocals" multiply-high approximation replaces the integer division in the encode
  hot loop (`rans_byte_enc_put_symbol`, `rans64_enc_put_symbol`, `rans64_mul_hi`).
- **Two-state interleaving** for byte rANS (`ByteInterleavedEncoder`,
  `ByteInterleavedDecoder`): two alternating states for superscalar throughput.
- **Step-only decoder operations** (`rans_byte_dec_advance_step`,
  `rans_byte_dec_advance_symbol_step`): decode without renormalization, enabling
  interleaved decoding patterns.
- **Table-based word rANS decode**: a 4096-slot `(freq, bias)` table
  (`RansWordTables`, `RansWordSlot`) with a `slot2sym` mapping; the foundation for the
  SIMD crate's packed-table format.
- **Vose's alias method** for byte rANS: O(1) symbol lookup during decode instead of
  binary search (`rans_byte_alias_build_table`, `rans_byte_alias_dec_advance`).
- **Malformed-input validation**: the `malformed` module (`ValidationError`,
  `RenormGuard`, stream/frequency-model validators) — see
  [Trust Boundaries](#trust-boundaries-and-input-invariants).

### Encoding semantics (from upstream)

- **Reverse order**: symbols must be encoded last-to-first (stack discipline).
- **Reverse-growing output**: the backward writers (`BackwardByteWriter`,
  `BackwardWord32Writer`, `BackwardWord16Writer`) start at the end of a caller-provided
  buffer and move toward the beginning; `encoded()` returns the written region.
- **Renormalization**: when the state exceeds a symbol-specific threshold `x_max`, the
  lowest byte/word is emitted and the state is shifted.
- **Flush**: the remaining state is written as 4 bytes (byte rANS), 2 × `u32`
  (R64), or 2 × `u16` (word rANS).

### I/O abstraction

Trait-based I/O keeps the algorithms storage-agnostic while staying zero-cost:

- `BackwardWriter` (`write_byte`, `write_u32_le`) — implemented by
  `BackwardByteWriter`, `SliceBackwardWriter`.
- `ForwardReader` (`read_byte`, `read_u32_le`) — implemented by `ByteReader` and by
  `&[u8]` directly.
- R64-specific: `BackwardWord32Writer` / `Word32Reader` (u32 word I/O).
- Word-rANS-specific: `BackwardWord16Writer` / `Word16Reader` (u16 word I/O).

Every read/write is bounds-checked and returns `None`/`Err(())` on exhaustion rather
than panicking or over-reading.

### Feature flags

```toml
[features]
default = []    # no features — pure no_std, no alloc
alloc = []      # alias-method table construction and encode/decode API
std = []        # std::error::Error impls for the error types
```

- **default**: the crate is usable in any `no_std` environment; all encode/decode hot
  paths work with caller-provided buffers and allocate nothing.
- **alloc**: enables `AliasTable` and `rans_byte_alias_build_table` (also compiled under
  `test`), and the alias encode/decode functions `rans_byte_alias_enc_put`,
  `rans_byte_alias_dec_get`, `rans_byte_alias_dec_renorm`, `rans_byte_alias_dec_advance`.
  `rans_byte_alias_normalize_freqs` is available without any feature.
- **std**: implements `std::error::Error` for `EncodeError`, `DecodeError`,
  `ModelError`, and `malformed::ValidationError`. It does **not** imply `alloc`.

Downstream feature usage (verified against `Cargo.toml`): `ryg-rans-rs-parallel` and
`ryg-rans-rs-oracle` depend on this crate with `features = ["alloc"]`; `ryg-rans-rs-simd`
depends on it with no extra features.

---

## Understanding the rANS Arithmetic

### The core insight: compressing states instead of bits

Traditional entropy coders (Huffman, arithmetic coding) encode symbols as sequences of
bits. rANS (range Asymmetric Numeral Systems) encodes symbols as **state transitions**:
a single integer `x` is the entire encoding state, and each symbol updates it via a
carefully designed function. A symbol with probability `p ≈ freq / total` adds
approximately `-log2(p)` bits to the state, making rANS a near-optimal entropy coder
with the computational simplicity of integer arithmetic.

### Encoding: `C(s, x)`

```
C(s, x) = ((x / freq_s) << scale_bits) + (x % freq_s) + start_s
```

The state space is partitioned into `M = 1 << scale_bits` slots. Symbol `s` owns
`freq_s` consecutive slots starting at `start_s` (its cumulative frequency). `x / freq_s`
selects the row, `x % freq_s` the position inside the symbol's block, and `start_s`
offsets to the correct symbol's region.

### Decoding: the inverse

```
slot   = x & (M - 1)            // which symbol owns this slot?
symbol = table[slot]            // O(1) lookup instead of binary search
x'     = freq_s × (x >> scale_bits) + (slot - start_s)
```

These are mathematical inverses:

```
decode(C(s, x)):
  slot   = C(s, x) & (M - 1) = (x % freq_s) + start_s
  symbol = table[slot] = s
  x'     = freq_s × (C(s, x) >> scale_bits) + (x % freq_s)
         = freq_s × (x / freq_s) + (x % freq_s)
         = x
```

The encoding formula is designed so the top bits of the new state carry the quotient —
that is the key insight that makes ANS invertible with a simple table lookup.

### Renormalization

The state must stay within a valid range so encode/decode operations keep enough
headroom:

- **Encoding**: if `state >= x_max` (where `x_max = ((L >> scale_bits) << renorm_bits) × freq`),
  emit the low 8/16/32 bits and shift right.
- **Decoding**: if `state < L`, read one byte/word from the stream and shift left.

| Variant | L | Renorm unit | Max consecutive renorm steps (worst case) |
|---------|---|-------------|-------------------------------------------|
| Byte rANS (32-bit) | `RANS_BYTE_L = 2^23` | 1 byte | 4 (32 bits / 8); `RenormGuard::new_byte()` budgets 16 |
| R64 (64-bit) | `RANS64_L = 2^31` | 1 `u32` word | 2 (63 bits / 32); `RenormGuard::new_r64()` budgets 8 |
| Word rANS (32-bit) | `RANS_WORD_L = 2^16` | 1 `u16` word | 1; `RenormGuard::new_word()` budgets 8 |

### Reciprocal multiplication: avoiding division

Integer division is expensive. The reciprocal fast path replaces `x / freq` with a
multiply-high and shift (Alverson's method):

```
q        = ((x × rcp_freq) >> 32) >> rcp_shift
new_state = x + bias + q × cmpl_freq
```

`RansByteEncSymbol` / `Rans64EncSymbol` precompute `x_max`, `rcp_freq`, `rcp_shift`,
`bias`, and `cmpl_freq`. The `freq == 1` special case (also used by the alias surface)
uses `rcp_freq = ~0`, `rcp_shift = 0`, `bias = start + M - 1`.

**Kani proof**: `kani_reciprocal_equals_division` (byte) and
`kani_r64_reciprocal_equals_division` (R64) prove the reciprocal path produces the exact
same state as the division path for **all** valid parameters — the approximation is
exact, not merely close.

---

## What This Crate Does NOT Do

These are deliberate trust boundaries. Anything outside this list lives in another crate:

| Not in this crate | Where it lives |
|-------------------|----------------|
| No I/O beyond in-memory slices — no files, sockets, or streams | The reader/writer types wrap `&[u8]` / `&mut [u8]` only |
| No container format — no block headers, no RYGRANS v1 container (`docs/container-format-v1.md`) | `ryg-rans-rs-parallel`, `ryg-rans-rs-cli` |
| No parallel engine — no executor, no worker pool, no cancellation, no reorder buffering | `ryg-rans-rs-parallel` |
| No SIMD kernels — no SSE4.1/AVX2/AVX-512 decode, no packed tables, no auto-dispatch | `ryg-rans-rs-simd` |
| No backend selection or dispatch — the **codec** (stream format) is defined here, the **backend** (execution engine) is chosen elsewhere | `ryg-rans-rs-simd/src/backends.rs`, `ryg-rans-rs-parallel` decode plan |
| No frequency normalization — `normalize_frequencies` (raw counts → scale_bits-exact model) | `ryg-rans-rs-parallel` |
| No runtime CPU detection, no performance measurement | `ryg-rans-rs-simd` (detection), `ryg-rans-rs-bench` (Criterion) |

The dependency flow is unidirectional: `ryg-rans-rs-simd → ryg-rans-rs-core` and
`ryg-rans-rs-parallel → ryg-rans-rs-core`. The core has no knowledge of SIMD, parallelism,
or container formats; it is a pure computational kernel that can be verified, tested, and
proved independently of the rest of the project.

---

## Trust Boundaries and Input Invariants

Malformed input produces **typed errors, never panics**. This is the crate's core safety
contract: a correctness bug in the core propagates as a detectable wrong-symbol decode or
a typed error, not as undefined behavior or an out-of-bounds access.

### Error types

| Type | Variants | Meaning |
|------|----------|---------|
| `EncodeError` | `OutputTooSmall` | The output buffer is exhausted |
| `DecodeError` | `InputTooShort` | The compressed stream is truncated |
| `ModelError` | `EmptyInput`, `ZeroTotal`, `InvalidScaleBits`, `ZeroFrequency`, `FrequencyOutOfRange`, `StartOutOfRange`, `TotalMismatch`, `WorkspaceTooSmall` | Frequency-model / symbol-construction validation |
| `malformed::ValidationError` | `TruncatedStream`, `ExcessiveRenormalization`, `ZeroFrequency`, `CumulativeOverflow`, `InvalidScaleBits`, `RangeOverflow`, `TrailingData` | Pre-decode stream validation |

All error types implement `Display`; with the `std` feature they also implement
`std::error::Error`. `malformed::validation_to_decode_error` maps a `ValidationError`
onto the decoder's `DecodeError` for callers that do not want to expose validation
details.

### Input invariants

- **Symbol construction is validated**: `RansByteEncSymbol::new`,
  `Rans64EncSymbol::new`, `RansByteDecSymbol::new`, `Rans64DecSymbol::new` check
  `scale_bits` range (1..=16 / 1..=31), `freq != 0`, `start <= total`, and
  `freq <= total - start`, returning `ModelError` variants. The `new_unchecked`
  constructors are `pub(crate)` — external callers cannot bypass validation.
- **Word rANS scale is pinned**: `rans_word_check_scale_bits` rejects any value other
  than `RANS_WORD_SCALE_BITS` (12); `rans_word_dec_sym` debug-asserts it in the hot path
  (upstream hardcodes this value).
- **Decode tables are caller-supplied**: `rans_word_dec_sym` indexes
  `tables.slots[slot]` / `tables.slot2sym[slot]` with `slot = x & (M - 1)` (masked to
  0..4095). The caller must provide tables with at least 4096 entries; construction of
  valid tables is the job of the SIMD crate's `build_word_tables` /
  `PackedWordTable::from_freqs`.
- **Transactional state mutation**: on error, encode/decode state is left consistent but
  partially advanced (documented on each function); the caller should treat the
  operation as failed and re-encode from a known state.

### The `malformed` module

Defensive checks for untrusted compressed streams, separated from the hot-path
arithmetic. Callers may skip validation for already-trusted input.

| Function / type | What it checks |
|-----------------|----------------|
| `validate_byte_compressed(&[u8])` | ≥ 4 bytes for byte-rANS init |
| `validate_r64_compressed(&[u8])` | ≥ 8 bytes for R64 init |
| `validate_word_compressed(&[u16])` | ≥ 2 words for word-rANS init |
| `validate_byte_scale_bits(u32)` / `validate_r64_scale_bits(u32)` | 1..=16 / 1..=31 |
| `validate_freq_model(&cum, &freqs, scale_bits)` | `start + freq <= 1 << scale_bits`; cumulative frequencies monotonically non-decreasing |
| `RenormGuard::new_byte()/new_r64()/new_word()` + `check()`/`reset()` | Bounds consecutive renormalization iterations so a corrupted stream cannot spin |
| `has_dominant_symbol` / `is_single_symbol` / `has_freq_one` | Edge-case model classification for block-type decisions (e.g. RLE eligibility) |

---

## Resource Behaviour

- **Zero allocation in the hot paths**: every encoder/decoder function operates on
  caller-provided buffers (`BackwardByteWriter::new(&mut buf)`, `ByteReader::new(&buf)`,
  `SliceBackwardWriter`, etc.). There are no hidden `Vec`/`Box` allocations in encode or
  decode.
- **The only heap allocation in this crate** is alias-table construction:
  `AliasTable::alias_remap` is a `Vec<u32>` with `2^scale_bits` entries (up to 65,536
  entries = 256 KiB at `scale_bits = 16`). It is gated behind the `alloc` feature
  (`AliasTable` and `rans_byte_alias_build_table` are also compiled under `test`).
- **Output sizing**: writers report exhaustion with `Err(())` /
  `EncodeError::OutputTooSmall`; no writer ever grows a buffer implicitly. Safe sizing
  guidance: each symbol emits at most one renormalization byte/word, plus the flush
  (4 bytes byte rANS, 2 × `u32` R64, 2 × `u16` word rANS). The facade crate documents a
  conservative byte-rANS bound of `symbols.len() * 4 + 16 + 4` bytes.
- **Deterministic**: identical input produces identical output and identical errors,
  with no thread-count, schedule, or iteration-order dependence.

---

## Backend Semantics

Per the project glossary, a **codec** is the stream format (number of states,
renormalization unit, scale constraint) and a **backend** is the execution engine that
decodes a stream. This crate implements codec arithmetic only — every function is a
single deterministic implementation, so there is no backend choice to record here.

The **requested-vs-executed backend** contract (requested == executed, or a typed error;
no silent substitution) is realized in `ryg-rans-rs-simd/src/backends.rs` (the
`DecodeResult.backend` field and the `_checked` wrappers returning
`DecodeError::UnsupportedBackend`) and enforced at plan time by `ryg-rans-rs-parallel`.
Consumers of this crate who need backend identity should use those layers.

---

## SIMD Requirements

**None.** This crate is pure scalar Rust (`no_std`, no intrinsics, no target-feature
attributes). It compiles for any target that supports `core`, including embedded
microcontrollers, kernels, and WebAssembly. SIMD acceleration of Word rANS decode lives
in `ryg-rans-rs-simd`.

---

## Unsafe Boundaries

**Zero `unsafe`.** The crate carries `#![forbid(unsafe_code)]`, a compile-time guarantee
enforced by the compiler, not a convention. Because the Kani proofs in `kani/` verify
properties over safe Rust, they cannot be undermined by hidden unsafe operations.

All `unsafe` code in the project is confined to the SIMD crate (intrinsics) and the
benchmark crate (FFI oracle comparison). The core's `BackwardWord32Writer` requires its
buffer length to be a multiple of 4 (`debug_assert`), and index arithmetic throughout
uses checked/`saturating_*` operations on untrusted lengths.

---

## Evidence Model

### Behaviour

The four surfaces are **Sealed** at the Phase K baseline (the 144-receipt evidence
index). The core's four surfaces account for 120 of those receipts:

| Surface | Behaviour receipts (Phase K baseline) | Court ID prefix |
|---------|---------------------------------------|-----------------|
| Byte rANS (division + reciprocal, single + interleaved2) | 44 | `RYG_RANS.BYTE.*` |
| R64 rANS (division + reciprocal, single + interleaved2) | 44 | `RYG_RANS.R64.*` |
| Word rANS (scalar table-based, single + interleaved2) | 16 | `RYG_RANS.WORD.*` |
| Alias method (Vose, single + interleaved2) | 16 | `RYG_RANS.ALIAS.*` |

Each behavioural receipt is a SHA-256-chained JSON artifact
(`evidence/receipts/receipt-RYG_RANS.*.json`) whose manifest lists every input case and
per-case verdict; the receipt carries a canonical self-hash and the seal gate
(`cargo xtask seal`) verifies every link. Receipt counts are generated from the evidence
index — never hardcoded.

### Performance

No performance receipt is sealed for this crate. The Phase K performance run is retained
under `evidence/performance/runs/phase-k-*` as **superseded** evidence (residuals
L1-A…L1-S in `evidence/phase-l/gap-ledger.md`: fabricated sample counts, hardcoded
verification flags, empty hashes, 99-byte archive path truncation). The Phase L.18
pipeline (`cargo xtask benchmark-run` → `cargo xtask performance-seal` → `cargo xtask
seal`) regenerates the `RYG_RANS.PERF.*` receipts. Until that run passes the seal gate,
no performance claim from this crate or its users may be marked **Sealed**.

### Formal proofs

Nine Kani harnesses in `kani/` symbolically prove arithmetic properties for **all**
valid inputs (run with `cargo kani`, not `cargo test`; requires the Kani toolchain):

| Proof | File | What it proves |
|-------|------|----------------|
| `kani_enc_symbol_new_valid` | `kani/enc_symbol_new_proof.rs` | Any valid `(start, freq, scale_bits)` → `RansByteEncSymbol::new` returns `Ok` |
| `kani_enc_symbol_new_invalid_scale` | `kani/enc_symbol_new_proof.rs` | `scale_bits = 0` or `> 16` → `InvalidScaleBits` |
| `kani_enc_symbol_new_zero_freq` | `kani/enc_symbol_new_proof.rs` | `freq = 0` → `ZeroFrequency` |
| `kani_byte_encode_decode_inversion` | `kani/encode_decode_inversion_proof.rs` | `D(s, C(s, x)) = x` for any valid state/symbol |
| `kani_reciprocal_equals_division` | `kani/reciprocal_proof.rs` | Byte rANS reciprocal fast path == division reference |
| `kani_r64_reciprocal_equals_division` | `kani/r64_reciprocal_proof.rs` | Same proof for the 64-bit state space |
| `kani_state_update_no_overflow` | `kani/packed_entry_proof.rs` | `freq × (x >> 12) + bias` does not overflow `u32` for 12-bit freq/bias and valid states |
| `kani_packed_entry_fields` | `kani/packed_entry_proof.rs` | Pack/unpack of the 32-bit packed entry preserves all three fields |
| `kani_slot_index_bounded` | `kani/packed_entry_proof.rs` | `state & 4095` is always in `0..4096` |

### Residual linkage

- **L11-A** (RESOLVED): repository-wide unwrap/panic audit; malformed-input paths return
  typed errors.
- **L13-A** (RESOLVED): public-API audit; dead types removed; `docs/public-api/`
  inventory committed.
- **L15-A** (OPEN): remove the former overclaim language (see gap ledger L15-A); this
  README is part of that pass.
- **L15-B** (PARTIAL), **L15-C/D** (OPEN): documentation-wide fixes tracked in
  `evidence/phase-l/gap-ledger.md`.

The claim-verification chain is: README claim → producing code → test/court that pins it
→ receipt in `evidence/` → seal gate. If any link is missing, the claim is not sealed.

---

## Performance Methodology

This crate is **not benchmarked directly for throughput**. It is the scalar reference
against which the SIMD backends and the Criterion suite are measured: every SIMD kernel
and every benchmark case verifies byte-exact output, words-consumed, and final states
against the core's scalar arithmetic before any timing.

The measured surfaces live in `ryg-rans-rs-bench` (13 Criterion bench targets), and the
Phase L.14 comparative court (`benches/comparative.rs`) measures the Rust core against
upstream C via `ryg-rans-sys` on the same host. Historical Phase K throughput figures
are **superseded** and appear only in the root README's "Phase K key findings" section;
the Phase L.18 re-seal regenerates the `RYG_RANS.PERF.*` receipts through
`cargo xtask benchmark-run`. No current throughput numbers are claimed in this README.

---

## Limitations

Honest negatives — things this crate deliberately does not provide:

- **Word rANS `scale_bits` is fixed at 12** (upstream constraint). Other values are
  rejected by `rans_word_check_scale_bits`.
- **No word-rANS table builder here**: `RansWordTables` is a plain view over
  caller-provided slices. The builders live in `ryg-rans-rs-simd` (`build_word_tables`,
  `PackedWordTable::from_freqs`).
- **Alias surface requires `alloc`**: without the `alloc` feature, only
  `rans_byte_alias_normalize_freqs` is compiled; the table builder and the alias
  encode/decode API are unavailable.
- **No 8-way / 16-way interleaving, no batch, no Uniform256 fast path** — those are SIMD
  crate surfaces.
- **No frequency normalization** — `normalize_frequencies` lives in
  `ryg-rans-rs-parallel`.
- **The CLI wires only a subset of codecs** (encode: byte-single, byte-interleaved2,
  r64-single, word-single; decode: codecs 1, 2, 3, 5, 7) — see the workspace `AGENTS.md`
  "Current limitations" section.
- **Kani proofs do not run under `cargo test`**; they require the Kani toolchain.
- **Zero-allocation does not mean zero-buffer**: callers must size output buffers; the
  writers report `OutputTooSmall`/`Err(())` on exhaustion.

---

## Examples

### Byte rANS — reciprocal encode, division decode

```rust
use ryg_rans_rs_core::{
    RansByteState, RansByteEncSymbol, RansByteDecSymbol,
    BackwardByteWriter, ByteReader,
    rans_byte_enc_put_symbol, rans_byte_enc_flush,
    rans_byte_dec_init, rans_byte_dec_advance_symbol,
};

let scale_bits = 14;
let freq = (1u32 << scale_bits) / 256; // uniform 256-symbol model
let mut buf = [0u8; 4096];

let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
let sym = RansByteEncSymbol::new(0, freq, scale_bits).unwrap();
rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).unwrap();
rans_byte_enc_flush(&state, &mut writer).unwrap();
let encoded = writer.encoded();

let mut reader = ByteReader::new(encoded);
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let dsym = RansByteDecSymbol::new(0, freq).unwrap();
rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
```

### Word rANS — table-based decode with a manually built table

```rust
use ryg_rans_rs_core::{
    RansWordState, RansWordSlot, RansWordTables,
    BackwardWord16Writer, Word16Reader,
    rans_word_enc_init, rans_word_enc_put, rans_word_enc_flush,
    rans_word_dec_init, rans_word_dec_sym, rans_word_dec_renorm,
    RANS_WORD_SCALE_BITS,
};

let scale_bits = RANS_WORD_SCALE_BITS; // 12
let freq = 16u32;                      // uniform-256: 4096 / 256
let mut buf = [0u8; 1024];

let mut writer = BackwardWord16Writer::new(&mut buf);
let mut state = rans_word_enc_init();
// Encode symbol 65 ('A'): start = 65 * freq.
rans_word_enc_put(&mut state, &mut writer, 65 * freq, freq, scale_bits).unwrap();
rans_word_enc_flush(&state, &mut writer).unwrap();
let compressed = writer.encoded();

// Decode table: symbol s owns slots [s * freq, (s + 1) * freq).
let mut slots = [RansWordSlot { freq: freq as u16, bias: 0 }; 4096];
let mut slot2sym = [0u8; 4096];
for s in 0..256u16 {
    for i in 0..freq as usize {
        let slot = s as usize * freq as usize + i;
        slots[slot] = RansWordSlot { freq: freq as u16, bias: i as u16 };
        slot2sym[slot] = s as u8;
    }
}
let tables = RansWordTables { slots: &slots, slot2sym: &slot2sym };

let mut reader = Word16Reader::new(compressed);
let mut dec_state = rans_word_dec_init(&mut reader).unwrap();
let symbol = rans_word_dec_sym(&mut dec_state, &tables, scale_bits);
rans_word_dec_renorm(&mut dec_state, &mut reader).unwrap();
assert_eq!(symbol, 65);
```

### Alias method — Vose's table (requires the `alloc` feature)

```rust
use ryg_rans_rs_core::{
    RansByteState, BackwardByteWriter, ByteReader,
    rans_byte_enc_flush, rans_byte_dec_init,
    rans_byte_alias_normalize_freqs, rans_byte_alias_build_table,
    rans_byte_alias_enc_put, rans_byte_alias_dec_advance,
};

let scale_bits = 12;
let raw = [3000u32, 900, 196]; // 3-symbol model, normalized to 4096
let (freqs, cum) = rans_byte_alias_normalize_freqs(&raw, 3, 1 << scale_bits).unwrap();
let table = rans_byte_alias_build_table(&freqs, &cum, scale_bits);

let symbols = [0u8, 1, 2, 1, 0];
let mut buf = [0u8; 4096];
let mut writer = BackwardByteWriter::new(&mut buf);
let mut state = RansByteState::new();
for &s in symbols.iter().rev() {
    rans_byte_alias_enc_put(&mut state, &mut writer, &table, s, scale_bits).unwrap();
}
rans_byte_enc_flush(&state, &mut writer).unwrap();

let mut reader = ByteReader::new(writer.encoded());
let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
let mut decoded = [0u8; 5];
for out in decoded.iter_mut() {
    *out = rans_byte_alias_dec_advance(&mut dec_state, &mut reader, &table, scale_bits).unwrap();
}
assert_eq!(decoded, symbols);
```

---

## Troubleshooting

| Symptom | Likely cause / fix |
|---------|--------------------|
| `EncodeError::OutputTooSmall` | Output buffer too small. Size it with headroom: at most one renorm byte/word per symbol plus the flush (see [Resource Behaviour](#resource-behaviour)). |
| `DecodeError::InputTooShort` | Truncated stream. Run the `malformed` validators (`validate_*_compressed`) before decoding untrusted input. |
| `ModelError::InvalidScaleBits` on word-rANS calls | Word rANS requires `scale_bits == 12` (`RANS_WORD_SCALE_BITS`); anything else is rejected by design. |
| Wrong symbols / wrong `cum2sym` mapping | Byte-rANS decode maps `rans_byte_dec_get` results through your own `cum2sym` table — it must match the encoder's model exactly. |
| Alias API missing | The alias build/encode/decode API is gated on the `alloc` feature: `cargo build --features alloc`. |
| Kani proofs do not run | They need the Kani toolchain (`cargo kani`), not `cargo test`. |
| Suspicion of over-read/UB | There is none by construction: `forbid(unsafe_code)` and bounds-checked readers/writers. Report any violation as a residual in `evidence/phase-l/gap-ledger.md`. |

---

## Versioning

`0.4.0` (workspace-wide). The crate follows the project's format invariants:

- **Bit-exact stream parity with the pinned upstream `ryg_rans` revision** is frozen:
  any change that alters an encoded stream is a breaking format change and invalidates
  every receipt (see `docs/bitstream-contract.md`).
- **Determinism** is frozen: same input → same output and same canonical error.
- SemVer applies to the Rust API; format stability is governed by the bitstream
  contract, not by the crate version alone.
- Evidence artifacts are bound to the code commit; the seal gate rejects stale evidence
  (source-freshness gate).

## Reading Order

1. Root [`README.md`](../../README.md) — project framing and Evidence Status.
2. [`docs/architecture.md`](../../docs/architecture.md)
3. [`docs/bitstream-contract.md`](../../docs/bitstream-contract.md) — the pinned stream formats.
4. [`docs/glossary.md`](../../docs/glossary.md) — the exact terminology used here.
5. [`docs/unsafe-ledger.md`](../../docs/unsafe-ledger.md)
6. [`AGENTS.md`](../../AGENTS.md) — ground truth for contributors.
7. `crates/ryg-rans-rs-core/src/lib.rs` — module docs and full API.
8. The [`ryg-rans-rs-simd`](../ryg-rans-rs-simd/README.md) README — how these surfaces
   are accelerated and how the packed table is built.

9. `docs/papers/0001-rans-design.md` and `docs/papers/0007-proof-philosophy.md` — the design and proof methodology; `docs/adr/0001` and `0002` — the format-contract and reciprocal decisions.
---

*Part of the ryg-rans-rs project. Version 0.4.0.*
