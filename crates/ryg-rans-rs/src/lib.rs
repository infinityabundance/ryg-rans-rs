//! # ryg-rans-rs
//!
//! **Public facade for rANS entropy coding.** Safe, `no_std`-compatible API
//! for byte-aligned and 64-bit rANS encoding and decoding.
//!
//! This crate re-exports the deterministic core from [`ryg-rans-rs-core`]
//! and optionally adds SSE4.1 accelerated decode kernels and
//! allocation-based convenience wrappers.
//!
//! ## Module Structure
//!
//! | Module | Source | Feature | Description |
//! |--------|--------|---------|-------------|
//! | [`byte`] | `ryg-rans-rs-core` | always | Complete rANS core: 32-bit byte and 64-bit variants |
//! | [`simd`] | `ryg-rans-rs-simd` | `simd` | SSE4.1 accelerated decoder kernels |
//! | [`alloc_utils`] | this crate | `alloc` | Convenience encode/decode with `Vec<u8>` |
//!
//! ## Features
//!
//! - **`simd`**: Enable with the `simd` feature. SSE4.1 accelerated decode kernels.
//!   Not enabled by default. Requires a CPU with SSE4.1 support.
//! - **`alloc`**: Adds `alloc_utils` module with heap-allocated convenience APIs.
//!   Requires an allocator (available on `std` targets and `no_std` targets with
//!   a global allocator).
//!
//! ## Usage
//!
//! ### Manual API (zero-allocation, no `alloc` feature needed)
//!
//! ```rust
//! use ryg_rans_rs::byte::{
//!     RansByteState, RansByteEncSymbol,
//!     BackwardByteWriter, ByteReader,
//!     rans_byte_enc_put_symbol, rans_byte_enc_flush,
//!     rans_byte_dec_init, rans_byte_dec_get,
//!     rans_byte_dec_advance_symbol,
//! };
//!
//! // All types and functions from ryg-rans-rs-core are available
//! // under ryg_rans_rs::byte::*
//! ```
//!
//! ### Convenience API (requires `alloc` feature)
//!
//! ```rust
//! # #[cfg(feature = "alloc")] {
//! use ryg_rans_rs::byte::RansByteEncSymbol;
//! use ryg_rans_rs::alloc_utils;
//!
//! let scale_bits = 14;
//! let total = 1u32 << scale_bits;
//! let base_freq = total / 256;
//!
//! let esyms: Vec<_> = (0..256)
//!     .map(|i| RansByteEncSymbol::new(i * base_freq, base_freq, scale_bits))
//!     .collect();
//!
//! let data = b"Hello, rANS!";
//! let encoded = alloc_utils::encode(data, &esyms, scale_bits);
//!
//! let cum2sym: Vec<u8> = (0..total as usize)
//!     .map(|i| (i / base_freq as usize) as u8)
//!     .collect();
//!
//! let dsyms: Vec<_> = (0..256)
//!     .map(|i| ryg_rans_rs::byte::RansByteDecSymbol::new(i * base_freq, base_freq))
//!     .collect();
//!
//! let decoded = alloc_utils::decode(&encoded, &cum2sym, &dsyms, scale_bits, data.len());
//! assert_eq!(&decoded, data);
//! # }
//! ```

#![deny(unsafe_code)]
#![no_std]

#[cfg(feature = "alloc")]
extern crate alloc;

/// Byte-aligned 32-bit and 64-bit rANS types and functions.
///
/// Re-exports everything from [`ryg-rans-rs-core`]. Contains:
///
/// - `RansByteState`, `RansByteEncSymbol`, `RansByteDecSymbol` — 32-bit types
/// - `Rans64State`, `Rans64EncSymbol`, `Rans64DecSymbol` — 64-bit types
/// - `BackwardByteWriter`, `ByteReader` — I/O abstractions
/// - `ByteInterleavedEncoder`, `ByteInterleavedDecoder` — two-state interleaving
/// - `rans_byte_enc_*`, `rans_byte_dec_*` — 32-bit encode/decode functions
/// - `rans64_enc_*`, `rans64_dec_*` — 64-bit encode/decode functions
/// - `EncodeError`, `DecodeError` — error types
pub mod byte {
    pub use ryg_rans_rs_core::*;
}

/// SSE4.1 accelerated decoder kernels.
///
/// This module is available when the `simd` feature is enabled.
/// It provides safe wrappers around SSE4.1 intrinsics for accelerated
/// rANS decoding.
///
/// Currently a scaffold — the full implementation will appear in a future release.
#[cfg(feature = "simd")]
pub mod simd {
    // Future: RansSimdDec,
    //        RansSimdDecInit, RansSimdDecSym, RansSimdDecRenorm
}

/// Convenience allocation-based APIs.
///
/// Requires the `alloc` feature. Provides:
///
/// - `encode(symbols, esyms, scale_bits) -> Vec<u8>` — encode to a heap-allocated buffer
/// - `decode(encoded, cum2sym, dsyms, scale_bits, num_symbols) -> Vec<u8>` — decode to a heap-allocated buffer
#[cfg(feature = "alloc")]
pub mod alloc_utils {
    use crate::byte::*;
    use alloc::vec::Vec;

    /// Encode a slice of bytes using a frequency model and precomputed symbols.
    ///
    /// Returns the encoded bytes. The worst-case output size is
    /// `symbols.len() * 4 + 16 + 4` bytes.
    ///
    /// # Panics
    ///
    /// Panics if the internal output buffer is too small (should not happen
    /// with the worst-case estimate). For controlled environments, use the
    /// manual API with `BackwardByteWriter` and caller-provided storage.
    pub fn encode(symbols: &[u8], esyms: &[RansByteEncSymbol], scale_bits: u32) -> Vec<u8> {
        let max_size = symbols.len() * 4 + 16 + 4;
        let mut buf = alloc::vec![0u8; max_size];
        let mut writer = BackwardByteWriter::new(&mut buf);

        let mut state = RansByteState::new();
        for idx in (0..symbols.len()).rev() {
            rans_byte_enc_put_symbol(&mut state, &mut writer, &esyms[symbols[idx] as usize])
                .expect("encode: output buffer exhausted (this should not happen with worst-case estimate)");
        }
        rans_byte_enc_flush(&state, &mut writer)
            .expect("encode: output buffer exhausted during flush");

        let encoded_start = writer.position();
        buf.drain(..encoded_start);
        buf
    }

    /// Decode bytes using cumulative-frequency table and decoder symbols.
    ///
    /// Returns the decoded bytes.
    ///
    /// # Panics
    ///
    /// Panics if the encoded stream is truncated or malformed. For controlled
    /// environments, use the manual API with `ByteReader` and handle `DecodeError`.
    pub fn decode(
        encoded: &[u8],
        cum2sym: &[u8],
        dsyms: &[RansByteDecSymbol],
        scale_bits: u32,
        num_symbols: usize,
    ) -> Vec<u8> {
        let mut reader = ByteReader::new(encoded);
        let mut state =
            rans_byte_dec_init(&mut reader).expect("decode: truncated stream during init");

        let mut output = alloc::vec![0u8; num_symbols];
        for i in 0..num_symbols {
            let cf = rans_byte_dec_get(&state, scale_bits);
            let s = cum2sym[cf as usize] as usize;
            output[i] = s as u8;
            rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsyms[s], scale_bits)
                .expect("decode: truncated stream during symbol advance");
        }

        output
    }
}
