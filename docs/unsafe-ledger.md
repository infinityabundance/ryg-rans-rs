# Unsafe Ledger

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity first. Unsafe code is permitted only in the SIMD crate and only when its safety contracts are fully documented.

---

## Current Status

| Crate | Status | `unsafe` blocks |
|---|---|---|
| `ryg-rans-rs-core` | `#![forbid(unsafe_code)]` | 0 |
| `ryg-rans-rs-simd` | Implemented — SSE4.1 + AVX-512 intrinsics | 7 unsafe fn (feature-gated under `#[target_feature]`) |
| `ryg-rans-rs` | `#![deny(unsafe_code)]` | 0 |
| `ryg-rans-rs-oracle` | Safe | 0 |
| `ryg-rans-rs-casefile` | `#![no_std]`, safe | 0 |
| `ryg-rans-rs-cli` | Safe | 0 |
| `xtask` | Safe | 0 |

---

## Unsafe Functions in `ryg-rans-rs-simd`

### Block [1]: `rans_simd_dec_init` (src/lib.rs)

**Intrinsic:** `_mm_loadu_si128`  
**Purpose:** Load 4 × 32-bit initial decoder states from 8 × u16 words.

#### Preconditions
- `reader` must point to a slice with at least 8 u16 elements remaining.
- The 128-bit load reads exactly 16 bytes (8 × u16).

#### Alignment
- Uses `_mm_loadu_si128` (unaligned load), so no alignment requirement.

#### Bounds
- `reader.len() >= 8` is checked before the load.

#### CPU Features
- `SSE2` (baseline x86_64).

#### Soundness
- The function is `unsafe` because `_mm_loadu_si128` reads from a raw pointer. Bounds check ensures safety.

---

### Block [2]: `rans_simd_dec_sym_unchecked` (src/lib.rs)

**Intrinsics:** `_mm_and_si128`, `_mm_set1_epi32`, `_mm_cvtsi128_si32`, `_mm_extract_epi32`, `_mm_cvtsi32_si128`, `_mm_insert_epi32`, `_mm_unpacklo_epi64`, `_mm_srli_epi32`, `_mm_mullo_epi32`, `_mm_add_epi32`  

**Purpose:** Decode 4 symbols in parallel using table lookups and SIMD arithmetic.

#### Preconditions
- Tables must have ≥ 4096 entries (masked access ensures this).
- Lane indices are masked to 0..4095.

#### CPU Features
- `SSE2` (baseline) + `SSE4.1` (`_mm_extract_epi32`, `_mm_insert_epi32`, `_mm_mullo_epi32`).

#### Soundness
- All memory accesses use safe Rust indexing. SIMD operations operate on registers.

---

### Block [3]: `rans_simd_dec_renorm_unchecked` (src/lib.rs)

**Intrinsics:** `_mm_xor_si128`, `_mm_set1_epi32`, `_mm_cmpgt_epi32`, `_mm_movemask_ps`, `_mm_castsi128_ps`, `_mm_loadl_epi64`, `_mm_slli_epi32`, `_mm_load_si128`, `_mm_shuffle_epi8`, `_mm_or_si128`, `_mm_blendv_epi8`  

**Purpose:** Renormalize 4 SIMD lanes by conditionally reading u16 words from input.

#### Preconditions
- `SHUFFLE_MASKS` is `#[repr(align(16))]` for aligned load.
- Scratch buffer prevents over-read from input slice.

#### CPU Features
- `SSE2`, `SSE3`, `SSSE3`, `SSE4.1`.

#### Soundness
- Scratch buffer bounds checked, aligned static for mask load.

---

### Block [4]: `decode_interleaved8_avx512vl_kernel` (avx512.rs)

**Intrinsics (major):** `_mm256_loadu_si256`, `_mm256_storeu_si256`, `_mm256_i32gather_epi32`, `_mm256_and_si256`, `_mm256_set1_epi32`, `_mm256_srli_epi32`, `_mm256_mullo_epi32`, `_mm256_add_epi32`, `_mm256_cmplt_epu32_mask`  

**Purpose:** 8-way AVX512VL Word-rANS decode.

#### Preconditions
- `compressed.len() >= 16` (checked upfront).
- `table` has exactly 4096 entries (guaranteed by `PackedWordTable` invariant).
- `expected_len` matches the encoded symbol count.

#### Alignment
- No alignment requirement — all loads/stores are unaligned (`_mm256_loadu_si256`, `_mm256_storeu_si256`).
- Gather (`_mm256_i32gather_epi32`) uses 4-byte element scale, not requiring alignment.

#### Bounds
- Initial state load: reads 32 bytes (16 u16 → 8 u32) from `compressed[0..16]`. Length check ensures this.
- Gather: table has 4096 entries (16384 bytes), gather indices are masked to 0..4095.
- Symbol store: writes 8 bytes to `output[i..i+8]`, where `i + 8 ≤ expected_len`. `output` is pre-sized to `expected_len`.
- Renorm reads: `reader_pos + words_needed ≤ compressed.len()` checked before any read.
- Tail lane reads: `reader_pos < compressed.len()` checked before reading.

#### Inactive Lane Safety
- Renormalization loop processes lanes 0..7 individually, reading only from active lanes (determined by `renorm_mask`). Inactive lanes are not touched.

#### CPU Features
- `avx512f` (gather, masked operations, 256-bit AVX512VL state).
- `avx512vl` (256-bit AVX512 instructions).
- `avx512bw` (byte/word operations, `_mm256_cmplt_epu32_mask`).

Gated by `#[target_feature(enable = "avx512f,avx512vl,avx512bw")]`.

#### Soundness
- All memory accesses are bounds-checked before execution.
- Table gather is bounded by the 4096-entry table size via `state & (M-1)` mask.
- Symbol output uses pre-allocated `Vec<u8>` with size `expected_len`.
- Lane-wise renormalization avoids masked-load overread issues by using individual scalar reads.
- Tail path uses safe scalar indexing with bounds checks.
- The `reinterpret_cast`-style pointer casts (`u32* → __m256i*`) are valid because all types are plain-old-data with compatible sizes and alignments on x86_64.

---

### Block [5]: `decode_interleaved16_avx512_kernel` (avx512.rs)

**Intrinsics (major):** `_mm512_loadu_si512`, `_mm512_storeu_si512`, `_mm512_i32gather_epi32`, `_mm512_and_si512`, `_mm512_set1_epi32`, `_mm512_srli_epi32`, `_mm512_mullo_epi32`, `_mm512_add_epi32`, `_mm512_cmplt_epu32_mask`  

**Purpose:** 16-way AVX512 Word-rANS decode (new stream format).

#### Preconditions
- `compressed.len() >= 32` (checked upfront).
- `table` has exactly 4096 entries.
- `expected_len` matches the encoded symbol count.

#### Alignment
- All loads/stores are unaligned.

#### Bounds
- Initial state load: reads 64 bytes (32 u16 → 16 u32) from `compressed[0..32]`.
- Gather: masked to 0..4095.
- Symbol store: writes 16 bytes to `output[i..i+16]` via temp buffer.
- Renorm reads: `reader_pos + words_needed ≤ compressed.len()` checked.
- Tail: checked bounds per lane.

#### Inactive Lane Safety
- Lane-wise loop processes only active lanes from `renorm_mask`. Inactive lanes unchanged.

#### CPU Features
- `avx512f` (512-bit operations, gather, masked compare).
- `avx512bw` (byte/word operations).

Gated by `#[target_feature(enable = "avx512f,avx512bw")]`.

#### Soundness
- Same pattern as Block [4] but with 512-bit vectors.
- Symbol output uses a temporary `[u32; 16]` buffer to avoid `packus` interleaving issues — individual lane writes preserve exact lane order.
- Tail path handles all 16 possible remainder lengths correctly (tested exhaustively).

---

### Block [6]: `decode_simd_8way_unchecked` (src/lib.rs)

**Purpose:** SSE4.1 8-way decode entry point.

#### Preconditions
- Caller must ensure CPU supports SSSE3 and SSE4.1.
- Function is `#[target_feature(enable = "ssse3,sse4.1")]`.

#### Soundness
- Calls `simd_decode_inner` which executes SSE4.1 intrinsics. Unsafe because `#[target_feature]` does not enforce runtime detection.

---

## Unsafe in Public API Wrappers (backends.rs)

### `decode_interleaved8_avx512vl` (backends.rs)

**Purpose:** Safe public wrapper around AVX512VL kernel.

#### Preconditions
- Caller must ensure AVX512F + AVX512VL + AVX512BW are available at runtime.

#### Soundness
- Function is `unsafe` because it calls a `#[target_feature]`-gated kernel. The `#[target_feature]` gate does not perform runtime detection; the caller is responsible for this.

### `decode_interleaved16_avx512` (backends.rs)

**Purpose:** Safe public wrapper around AVX512 16-way kernel.

#### Preconditions
- Caller must ensure AVX512F + AVX512BW are available.

#### Soundness
- Same pattern as the 8-way wrapper — caller must ensure CPU support at runtime.

---

## Audit Trail

| Block | Added | Description | Reviewer |
|-------|-------|-------------|----------|
| 1 | Phase F | `rans_simd_dec_init` — unaligned 128-bit load | Self-reviewed |
| 2 | Phase F | `rans_simd_dec_sym_unchecked` — SIMD symbol decode | Self-reviewed |
| 3 | Phase F | `rans_simd_dec_renorm_unchecked` — SIMD renormalization | Self-reviewed |
| 4 | Phase G | `decode_interleaved8_avx512vl_kernel` — AVX512VL 8-way decode | Self-reviewed |
| 5 | Phase G | `decode_interleaved16_avx512_kernel` — AVX512 16-way decode | Self-reviewed |
| 6 | Phase G | Public API wrappers for AVX512 decode | Self-reviewed |

No `unsafe` block may be added without a corresponding ledger entry.
