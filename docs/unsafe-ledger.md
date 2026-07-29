# Unsafe Ledger

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity first. Unsafe code is permitted only in the SIMD crate and only when its safety contracts are fully documented.

---

## Current Status

`ryg-rans-core` has `#![forbid(unsafe_code)]`. There is currently **zero** unsafe code in the workspace.

| Crate | Status | `unsafe` blocks |
|---|---|---|
| `ryg-rans-core` | `#![forbid(unsafe_code)]` | 0 |
| `ryg-rans-simd` | Implemented — SSE4.1 intrinsics | 7 unsafe fn (feature-gated under `#[target_feature]`), 42 intrinsic calls in `rans_simd_dec_sym_unchecked` and `rans_simd_dec_renorm_unchecked` |
| `ryg-rans` | `#![deny(unsafe_code)]` | 0 |
| `ryg-rans-oracle` | Safe | 0 |
| `ryg-rans-casefile` | `#![no_std]`, safe | 0 |
| `ryg-rans-cli` | Safe | 0 |
| `xtask` | Safe | 0 |

This ledger will be populated when `ryg-rans-simd` is implemented.

---

## Policy for Future Unsafe Blocks

When the SSE4.1 decoder is implemented in `crates/ryg-rans-simd`, every `unsafe` block must be documented. The documentation must include the following sections. Each block that is not trivially safe by construction requires a separate entry in this ledger.

### Documentation Requirements

Every `unsafe` block in `ryg-rans-simd` MUST be preceded by a comment block that documents:

#### Preconditions

What must be true before entering this `unsafe` block. Examples:

- "`ptr` must be non-null, aligned to 16 bytes, and point to at least 16 readable bytes."
- "`lane_mask` must be in `0..16`."
- "The caller must have called `_mm_setcsr` with the default rounding mode."

#### Alignment Assumptions

What alignment is required and how it is guaranteed:

- "Input buffer is 16-byte aligned because we use `#[repr(align(16))]` on the allocation wrapper."
- "Unaligned loads are used (`_mm_loadu_si128`), so no alignment requirement; however the buffer must not be null."

#### Bounds

How the bounds are verified before the unsafe block:

- "`offset` was checked against `buf.len()` before this block: `offset + 16 <= buf.len()`."
- "The shuffle mask `indices` is compile-time constant, so bounds on `_mm_shuffle_epi8` are guaranteed by the mask values."

#### CPU Features

Which CPU feature gates guard this block:

- "Guarded by `#[cfg(target_feature = "sse4.1")]` and a runtime `is_x86_feature_detected!("sse4.1")` check."
- "Called only from the `sse41_decoder` module which is conditionally compiled."

#### Soundness Justification

A concise explanation of why this block is sound:

- "All memory accesses are within the allocated buffer because the bounds check above ensures 16 bytes are available. The `_mm_loadu_si128` intrinsic reads exactly 16 bytes. No aliasing violations exist because the buffer is not aliased within this function."

### Ledger Entry Format

```markdown
## Block #[N]: `crates/ryg-rans-simd/src/decode.rs:L42-L48`

**Intrinsic:** `_mm_shuffle_epi8`  
**Purpose:** Byte extraction from 16-wide SIMD lane.

### Preconditions
- The shuffle mask is a compile-time constant array.
- `lane_data` holds 16 valid bytes from a previous load.

### Alignment
- `lane_data` is an `__m128i` value, not a pointer — no alignment requirement.

### Bounds
- N/A — operation is register-to-register. No memory access.

### CPU Features
- Functions using this block are gated on `cfg(target_feature = "sse4.1")`.
- Runtime check: `is_x86_feature_detected!("sse4.1")` in the public entry point.

### Soundness
- `_mm_shuffle_epi8` is a pure computation on SIMD registers with no memory side effects. It is safe when called from a context where SSE4.1 is available. The feature gate guarantees this at compile time for the call site.
```

---

## Audit Trail

When an `unsafe` block is added:

1. An entry is created in this ledger with a unique block number.
2. The block is reviewed by at least one other contributor before merging.
3. The entry includes a link to the PR or commit that introduced the block.

No `unsafe` block may be added without a corresponding ledger entry. This is enforced by the `cargo xtask no-ffi` gate (which also checks for unexpected `unsafe`).
