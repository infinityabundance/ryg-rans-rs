//! # ryg-rans-rs-simd
//!
//! **SSE4.1 accelerated rANS decoder kernels.** (Scaffold — not yet implemented.)
//!
//! When implemented, this crate will provide:
//!
//! - **`RansSimdDec`**: A four-lane SIMD decoder operating on 32-bit state lanes.
//! - **`rans_simd_dec_init`**: Load 4 × 32-bit state words from the input stream
//!   in a single SIMD load.
//! - **`rans_simd_dec_sym`**: Decode 4 symbols in parallel using table gathers,
//!   lane-wise multiply-add, and slot extraction.
//! - **`rans_simd_dec_renorm`**: Renormalize 4 lanes using all 16 shuffle masks,
//!   sign-biased unsigned comparison, and byte/word consumption tracking.
//! - **Two-decoder orchestration**: Two `RansSimdDec` units for 8-way interleaved
//!   streams matching the upstream `main_simd.cpp` pattern.
//!
//! ## Architecture
//!
//! The upstream SIMD decoder (`rans_word_sse41.h`) uses:
//!
//! - SSE4.1 `_mm_cvtsi128_si32` / `_mm_extract_epi32` for slot extraction
//! - Scalar table gathers into `__m128i` via `_mm_cvtsi32_si128` + `_mm_insert_epi32`
//! - `_mm_unpacklo_epi64` for freq/bias interleaving
//! - `_mm_mullo_epi32` for lane-wise multiplication
//! - 16 precomputed shuffle masks for byte extraction from unaligned input
//! - Sign-biased comparison via XOR with `0x80000000`
//! - `_mm_blendv_epi8` for conditional state update
//!
//! ## Safety
//!
//! This crate will contain `unsafe` blocks for SSE4.1 intrinsics. Each block
//! will document:
//!
//! - Preconditions (aligned or unaligned access requirements)
//! - Bounds assumptions (minimum readable length)
//! - CPU feature assumptions (`sse4.1` target feature)
//! - Soundness justification

// SSE4.1 accelerated rANS decoder kernels
