# Unsafe Ledger

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity first. Unsafe code is permitted only in the SIMD crate and only when its safety contracts are fully documented.

---

## Current Status

`ryg-rans-core` has `#![forbid(unsafe_code)]`. The SIMD crate has 3 `unsafe fn` with documented safety contracts.

| Crate | Status | `unsafe` blocks |
|---|---|---|
| `ryg-rans-rs-core` | `#![forbid(unsafe_code)]` | 0 |
| `ryg-rans-rs-simd` | Implemented — SSE4.1 intrinsics | 3 unsafe fn (feature-gated under `#[target_feature]`) |
| `ryg-rans-rs` | `#![deny(unsafe_code)]` | 0 |
| `ryg-rans-rs-oracle` | Safe | 0 |
| `ryg-rans-rs-casefile` | `#![no_std]`, safe | 0 |
| `ryg-rans-rs-cli` | Safe | 0 |
| `xtask` | Safe | 0 |

---

## Unsafe Functions in `ryg-rans-rs-simd`

### Block [1]: `rans_simd_dec_init` (src/lib.rs:L192-L199)

**Intrinsic:** `_mm_loadu_si128`  
**Purpose:** Load 4 × 32-bit initial decoder states from 8 × u16 words.

#### Preconditions
- `reader` must point to a slice with at least 8 u16 elements remaining.
- The 128-bit load reads exactly 16 bytes (8 × u16).

#### Alignment
- Uses `_mm_loadu_si128` (unaligned load), so no alignment requirement.
- However, the 16 bytes must be readable memory — guaranteed by the length check.

#### Bounds
- `reader.len() >= 8` is checked before the load. If the check fails, `None` is returned and the load is not executed.

#### CPU Features
- `SSE2` is sufficient for `_mm_loadu_si128` (baseline on x86_64).

#### Soundness
- The function is `unsafe` because it uses `_mm_loadu_si128`, which reads from a raw pointer. The safety contract is: `reader` must have at least 8 elements (checked dynamically) and the resulting pointer must be valid for a 16-byte read (guaranteed by the slice's memory layout).

---

### Block [2]: `rans_simd_dec_sym_unchecked` (src/lib.rs:L208-L239)

**Intrinsics:** `_mm_and_si128`, `_mm_set1_epi32`, `_mm_cvtsi128_si32`, `_mm_extract_epi32` (×3), `_mm_cvtsi32_si128` (×2), `_mm_insert_epi32` (×2), `_mm_unpacklo_epi64`, `_mm_srli_epi32` (×2), `_mm_mullo_epi32`, `_mm_add_epi32`  

**Purpose:** Decode 4 symbols in parallel using table lookups and SIMD arithmetic.

#### Preconditions
- `tables.slots` must have at least `RANS_WORD_M` (4096) entries.
- `tables.slot2sym` must have at least `RANS_WORD_M` entries.
- The lane indices extracted from the state must be valid indices into these tables (guaranteed by the mask: `state & (M-1)` where M = 4096).

#### Alignment
- All SIMD operations are register-to-register after the initial load. No memory alignment requirements beyond the table slice guarantees.

#### Bounds
- Slot indices are masked to `M-1` before table access.
- Table access uses `[]` indexing with the masked index, which Rust checks at runtime in debug builds.

#### CPU Features
- `SSE2` (baseline x86_64): `_mm_and_si128`, `_mm_set1_epi32`, `_mm_cvtsi128_si32`, `_mm_cvtsi32_si128`, `_mm_unpacklo_epi64`, `_mm_srli_epi32`, `_mm_add_epi32`.
- `SSE4.1`: `_mm_extract_epi32`, `_mm_insert_epi32`, `_mm_mullo_epi32`.
- The function is gated by `#[target_feature(enable = "ssse3,sse4.1")]` through `decode_simd_8way_unchecked`, which also gates SSSE3.

#### Soundness
- All memory accesses use safe Rust indexing (`tables.slots[i]`, `tables.slot2sym[i]`). The SIMD operations operate on registers. The `_mm_extract_epi32` and `_mm_insert_epi32` calls operate on `__m128i` values, not pointers. Soundness relies on the `#[target_feature]` gate ensuring the CPU supports these instructions.

---

### Block [3]: `rans_simd_dec_renorm_unchecked` (src/lib.rs:L248-L281)

**Intrinsics:** `_mm_xor_si128`, `_mm_set1_epi32` (×2), `_mm_cmpgt_epi32`, `_mm_movemask_ps`, `_mm_castsi128_ps`, `_mm_loadl_epi64`, `_mm_slli_epi32`, `_mm_load_si128` (aligned), `_mm_shuffle_epi8`, `_mm_or_si128`, `_mm_blendv_epi8`  

**Purpose:** Renormalize 4 SIMD lanes by conditionally reading u16 words from the input stream.

#### Preconditions
- `reader` must have at least `words_needed` u16 elements remaining (checked dynamically via `reader.len() >= words_needed`).
- `SHUFFLE_MASKS` is `#[repr(align(16))]`, satisfying the 16-byte alignment requirement of `_mm_load_si128`.
- A scratch buffer is used to avoid over-reading from the input slice: only `words_needed` words are copied before the SIMD load.

#### Alignment
- `SHUFFLE_MASKS` is `#[repr(align(16))]`, so `_mm_load_si128` on it is safe.
- `_mm_loadl_epi64` loads 8 bytes from a `u16` scratch buffer — only `words_needed` bytes are valid, but `_mm_loadl_epi64` reads exactly 8 bytes from the stack-allocated scratch, which has at least 8 bytes.

#### Bounds
- Input bounds: `reader.len() >= words_needed` is checked before any read.
- Scratch bounds: `scratch` is `[0u16; 4]` = 8 bytes, and `_mm_loadl_epi64` reads exactly 8 bytes from it.
- Shuffle mask: `mask` is in `0..16`, so `mask * 16` indexes into the 256-byte `SHUFFLE_MASKS` array.

#### CPU Features
- `SSE2` (baseline x86_64): `_mm_xor_si128`, `_mm_set1_epi32`, `_mm_slli_epi32`, `_mm_or_si128`, `_mm_loadl_epi64`.
- `SSE3`: `_mm_movemask_ps`, `_mm_castsi128_ps`.
- `SSSE3`: `_mm_shuffle_epi8`.
- `SSE4.1`: `_mm_blendv_epi8`, `_mm_cmpgt_epi32` (when comparing signed i32 — though SSE2 has `_mm_cmpgt_epi32`... actually this is SSE2).
- The function is gated by `#[target_feature(enable = "ssse3,sse4.1")]` through `decode_simd_8way_unchecked`.

#### Soundness
- The scratch buffer prevents out-of-bounds reads from the input slice: `_mm_loadl_epi64` reads exactly 8 bytes from the stack, which is always safe. Only `words_needed` words are copied into the scratch from the reader.
- The shuffle mask load (`_mm_load_si128`) targets a `#[repr(align(16))]` static, guaranteeing 16-byte alignment.
- The `_mm_shuffle_epi8` indices are compile-time constants from `SHUFFLE_MASKS`, guaranteeing valid within-lane byte positions.
- The `_mm_blendv_epi8` mask is computed from the sign comparison, which is the core of the upstream algorithm.
- Soundness relies on the `#[target_feature]` gate ensuring the CPU supports SSSE3 and SSE4.1.

---

## Unsafe in `decode_simd_8way_unchecked` (src/lib.rs:L312-L322)

**Purpose:** Entry point for SIMD 8-way decode. Wraps `simd_decode_inner`.

#### Preconditions
- Caller must ensure the CPU supports SSSE3 and SSE4.1 at runtime.
- The function is `#[target_feature(enable = "ssse3,sse4.1")]` gated.

#### Soundness
- This function is the sole public entry point to the SIMD kernel. It is `unsafe` because the `#[target_feature]` gate does not prevent calling it on CPUs lacking these features; the caller must perform runtime detection.

---

## Unsafe in `decode_simd_8way` (src/lib.rs:L288-L305)

**Purpose:** Safe wrapper around the SIMD path.

#### Soundness
- Uses `#[cfg(target_feature = "sse4.1")]` for compile-time dispatch. When SSE4.1 is enabled at compile time, the inner `unsafe` block is reached and `simd_decode_inner` is called.
- On x86_64 without compile-time SSE4.1, the function falls back to the scalar `decode_8way_scalar` path, which contains no unsafe code.

---

## Audit Trail

| Block | Added | Description | Reviewer |
|-------|-------|-------------|----------|
| 1 | Phase F | `rans_simd_dec_init` — unaligned 128-bit load | Self-reviewed |
| 2 | Phase F | `rans_simd_dec_sym_unchecked` — SIMD symbol decode | Self-reviewed |
| 3 | Phase F | `rans_simd_dec_renorm_unchecked` — SIMD renormalization | Self-reviewed |
| — | Phase H | Scalar fallback in `decode_simd_8way` — no unsafe code | N/A |

No `unsafe` block may be added without a corresponding ledger entry.
