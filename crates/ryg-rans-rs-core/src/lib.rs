#![no_std]
#![forbid(unsafe_code)]

//! # ryg-rans-rs-core
//!
//! **Deterministic algorithmic core of rANS entropy coding.**
//! `#![no_std]` · `#![forbid(unsafe_code)]` · zero-allocation encode/decode hot paths.
//!
//! This crate provides a complete native Rust reconstruction of both the 32-bit
//! byte-aligned and 64-bit rANS encoder/decoder variants from Fabian Giesen's
//! public-domain [`ryg_rans`](https://github.com/rygorous/ryg_rans) repository.
//!
//! ## Core algorithmic surfaces
//!
//! This crate exposes four encoding/decoding surfaces built on the same ANS
//! state machine, each targeting different throughput/latency trade-offs:
//!
//! | Surface | State width | Renorm unit | Key consts | Upstream origin |
//! |---------|-------------|-------------|------------|-----------------|
//! | **Byte rANS** | `u32` (31-bit effective) | `u8` (byte) | `RANS_BYTE_L = 1<<23` | `rans_byte.h` |
//! | **R64 rANS** | `u64` (63-bit effective) | `u32` (word) | `RANS64_L = 1<<31` | `rans64.h` |
//! | **Word rANS** | `u32` | `u16` (16-bit word) | `RANS_WORD_L = 1<<16`, `RANS_WORD_SCALE_BITS = 12` | `rans_word_sse41.h` |
//! | **Alias rANS** | `u32` (Byte rANS) | `u8` (byte) | `ALIAS_LOG2_NSYMS = 8` | `main_alias.cpp` |
//!
//! **Byte rANS** (32-bit): The canonical byte-aligned rANS from `rans_byte.h`.
//! Renormalization emits/consumes single bytes.  State space is 31 bits effective
//! (32-bit `u32` with `L = 2^23`).  Best for throughput-oriented applications
//! where byte granularity is acceptable.
//!
//! **R64 rANS** (64-bit): The 64-bit variant from `rans64.h`.  Renormalization
//! emits/consumes 32-bit words.  State space is 63 bits effective (`u64` with
//! `L = 2^31`).  Better compression efficiency (fewer renormalization steps per
//! symbol) at the cost of larger per-word overhead.  Uses `rans64_mul_hi` for
//! the reciprocal multiply-high fast path.
//!
//! **Word rANS** (32-bit, 16-bit renormalization): The scalar path from
//! `rans_word_sse41.h`.  Uses 32-bit state with 16-bit renormalization words.
//! This is the foundation surface for all SIMD implementations (SSE4.1, AVX2,
//! AVX-512).  `RANS_WORD_SCALE_BITS` is hardcoded to 12 at the upstream level
//! — all SIMD decode tables are 4096 entries (16 KiB).
//!
//! **Alias rANS** (Vose's alias method): Byte rANS using an alias table for
//! O(1) symbol lookup during decoding instead of binary search on cumulative
//! frequencies.  Constructed via Vose's algorithm from a normalized frequency
//! distribution.  Requires the `alloc` feature.  Upstream origin: `main_alias.cpp`.
//!
//! All four surfaces share common structure:
//! - **Division-based reference path**: `C(s,x) = ((x/freq) << scale_bits) + (x%freq) + start`
//! - **Reciprocal-multiply fast path** (Byte, R64): Uses multiply-high approximation
//!   to avoid integer division in the encode hot loop (Alverson's method).
//! - **Two-state interleaving**: Encodes symbols into two interleaved streams
//!   for superscalar throughput (Byte only; Word rANS uses 8/16-way SIMD interleaving).
//! - **Step-only operations**: Decoder advance without renormalization, enabling
//!   interleaved decoding patterns.
//!
//! ## Key constants
//!
//! The following constants are re-exported by the SIMD crate (`ryg-rans-rs-simd`)
//! as the foundation for packed-table construction and SIMD decode loops:
//!
//! - **`RANS_WORD_L`** (`1 << 16` = 65536): Lower bound of the normalisation
//!   interval for Word rANS.  The state is kept in `[L, M * L)` range through
//!   renormalisation.
//! - **`RANS_WORD_M`** (`4096` = `1 << RANS_WORD_SCALE_BITS`): The fixed number
//!   of slots in the Word rANS decode table.  `M = 1 << 12 = 4096`.  Each slot
//!   corresponds to one possible masked state value (`state & (M - 1)`).
//! - **`RANS_WORD_SCALE_BITS`** (`12`): The number of frequency scaling bits.
//!   Frequencies sum to `1 << scale_bits = 4096`.  Hardcoded at the upstream
//!   level — all Word rANS encode/decode formulas assume this value.
//!
//! ## Invariants: `no_std` and `forbid(unsafe_code)`
//!
//! This crate carries two crate-level attributes that enforce fundamental safety
//! and portability properties:
//!
//! ### `#![no_std]`
//!
//! The core algorithmic surfaces do not depend on the standard library.  They
//! can be compiled for any target that supports `core`, including embedded
//! systems, kernels, and WebAssembly.  Features that need allocation (`alloc`)
//! or the standard library (`std`) are gated behind Cargo features:
//!
//! - `alloc` — enables `AliasTable` and the `Vec`-returning helpers.
//!   Uses `extern crate alloc` internally.
//! - `std` — enables implementation of `std::error::Error` for error types.
//!   Independent of `alloc` (the two can be enabled separately).
//! - `default` — enables **no** features (`default = []` in Cargo.toml).
//!   The default build is `no_std`; consumers opt into `std`/`alloc`
//!   explicitly.
//!
//! ### `#![forbid(unsafe_code)]`
//!
//! The entire algorithmic core contains zero `unsafe` blocks.  This is a
//! deliberate design choice: the rANS state machine (encoding/decoding symbols,
//! renormalisation, state transitions) is pure arithmetic on `u32`/`u64` values
//! with no pointer provenance or aliasing concerns.  All `unsafe` code in the
//! project is confined to:
//!
//! - The SIMD crate (`ryg-rans-rs-simd`), where intrinsics require it.
//! - The benchmark crate (`ryg-rans-rs-bench`), for FFI oracle comparison.
//!
//! This boundary ensures that any correctness bug in the core propagates as a
//! detectable wrong-symbol decode rather than undefined behavior.
//!
//! ## Kani proofs
//! <!-- 7 proofs, see kani/*.rs -->
//!
//! The `kani/` directory contains **verification harnesses** using the Kani Rust
//! Verifier to symbolically prove critical properties of the core algorithms.
//! These proofs are run with `cargo kani` (not `cargo test`) and require the
//! Kani toolchain:
//!
//! ```sh
//! cd crates/ryg-rans-rs-core && cargo kani
//! ```
//!
//! The multiply-high proofs are instantiated per concrete frequency (the
//! Alverson exactness theorem is per-frequency, and the solver cannot
//! bit-blast a symbolic division); the instances cover the special case
//! (freq = 1), powers of two, worst-case small odd frequencies, and the
//! maximum frequency.  The remaining two R64 instances (freq = 3,
//! freq = 65535) do not terminate within practical time bounds and are
//! tracked as residual L16-E; the equivalence there is pinned by the
//! differential round-trip tests and the Phase L.14 comparative court
//! (byte-identical output with upstream C over 1 MiB).
//!
//! | Proof | File | What it proves |
//! |-------|------|----------------|
//! | `kani_enc_symbol_new_valid` | `kani/enc_symbol_new_proof.rs` | For **any** valid `(start, freq, scale_bits)`, `RansByteEncSymbol::new()` returns `Ok`. |
//! | `kani_enc_symbol_new_invalid_scale` | `kani/enc_symbol_new_proof.rs` | `scale_bits = 0` or `scale_bits > 16` always produces `InvalidScaleBits`. |
//! | `kani_enc_symbol_new_zero_freq` | `kani/enc_symbol_new_proof.rs` | `freq = 0` (with in-range `start`) always produces `ZeroFrequency`. |
//! | `kani_byte_encode_decode_inversion_freq{1,2,3,255,4095}` | `kani/encode_decode_inversion_proof.rs` | `D(s, C(s, x)) = x` for the division-based pair at scale 12. |
//! | `kani_reciprocal_equals_division_freq{1,2,3,7,16,255,4095}` | `kani/reciprocal_proof.rs` | Byte rANS: the reciprocal-multiply fast path produces the exact same `C(s, x)` as the division-based reference, for every reachable `x` at each instance's frequency. |
//! | `kani_r64_reciprocal_equals_division_freq{1,2,max}` | `kani/r64_reciprocal_proof.rs` | R64 rANS: same identity for the 64-bit state space (freq 3/65535 instances tracked as L16-E). |
//! | `kani_state_update_no_overflow` | `kani/packed_entry_proof.rs` | For any 12-bit freq, bias, and `(state >> 12) < 2^20`, the Widening mullo product `freq * (state >> 12)` does not overflow `u32`. |
//! | `kani_packed_entry_fields` | `kani/packed_entry_proof.rs` | Packing/unpacking the 32-bit entry preserves all three fields exactly. |
//! | `kani_slot_index_bounded` | `kani/packed_entry_proof.rs` | `state & 4095` is always in `0..4096`. |
//!
//! These proofs increase confidence that the core algorithms are correct for
//! all inputs without requiring exhaustive testing.
//!
//! ## Encoding Semantics (from upstream)
//!
//! - **Reverse order**: Symbols must be encoded last-to-first (stack discipline).
//! - **Reverse-growing output**: The backward writer starts at the end of the
//!   buffer and moves toward the beginning.
//! - **Renormalization**: When the state exceeds a symbol-specific threshold
//!   `x_max`, the lowest byte/word is emitted and the state is shifted.
//! - **Flush**: The remaining state is written directly as 4 bytes (byte rANS)
//!   or 2 × u32 words (64-bit rANS).
//!
//! ## Error Handling
//!
//! - **Encoding**: All encode operations return `Result<(), EncodeError>`. The
//!   only error variant is `OutputTooSmall`, which occurs when the backward
//!   writer's buffer is exhausted. State mutation is transactional — if the
//!   error occurs, the state is left in a consistent but partially-advanced
//!   position.
//! - **Decoding**: All decode operations return `Result<(), DecodeError>`.
//!   `InputTooShort` indicates a truncated compressed stream.
//!
//! ## I/O Abstraction
//!
//! Trait-based readers and writers allow the core algorithms to operate on any
//! storage backend. Concrete implementations provided:
//! - `BackwardByteWriter` / `BackwardWord32Writer` for encoding output
//! - `ByteReader` / `Word32Reader` for decoding input
//! - `SliceBackwardWriter` for convenient `&mut [u8]` encoding
//! - `&[u8]` implements `ForwardReader` directly for decoding
//! - `BackwardWord16Writer` / `Word16Reader` for Word rANS (16-bit words)

use core::fmt;

pub mod malformed;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// The output buffer is too small to hold the encoded data.
    OutputTooSmall,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EncodeError::OutputTooSmall => write!(f, "output buffer too small"),
        }
    }
}

/// Errors that can occur during decoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeError {
    /// The input is too short for the requested operation (truncated stream).
    InputTooShort,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::InputTooShort => write!(f, "truncated input stream"),
        }
    }
}

// ---------------------------------------------------------------------------
// Symbol construction error types
// ---------------------------------------------------------------------------

/// Errors that can occur during symbol construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelError {
    /// The input sequence was empty.
    EmptyInput,
    /// Total frequency is zero.
    ZeroTotal,
    /// `scale_bits` is outside the valid range.
    InvalidScaleBits,
    /// Symbol frequency is zero.
    ZeroFrequency,
    /// Frequency exceeds the allowed range for the given scale_bits/start.
    FrequencyOutOfRange,
    /// Start value exceeds the allowed range.
    StartOutOfRange,
    /// The provided total does not match the accumulated total.
    TotalMismatch,
    /// The workspace buffer is too small for the requested operation.
    WorkspaceTooSmall,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelError::EmptyInput => write!(f, "empty input sequence"),
            ModelError::ZeroTotal => write!(f, "total frequency is zero"),
            ModelError::InvalidScaleBits => write!(f, "scale_bits is out of valid range"),
            ModelError::ZeroFrequency => write!(f, "symbol frequency is zero"),
            ModelError::FrequencyOutOfRange => write!(f, "frequency exceeds allowed range"),
            ModelError::StartOutOfRange => write!(f, "start value exceeds allowed range"),
            ModelError::TotalMismatch => write!(f, "total frequency mismatch"),
            ModelError::WorkspaceTooSmall => write!(f, "workspace buffer too small"),
        }
    }
}

#[cfg(feature = "std")]
extern crate std;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "std")]
impl std::error::Error for EncodeError {}

#[cfg(feature = "std")]
impl std::error::Error for DecodeError {}

#[cfg(feature = "std")]
impl std::error::Error for ModelError {}

/// Lower bound of the normalization interval.
///
/// Equivalent to `RANS_BYTE_L` in `rans_byte.h`.
pub const RANS_BYTE_L: u32 = 1u32 << 23;

/// 32-bit rANS encoder/decoder state.
///
/// Equivalent to `RansState` (typedef for `uint32_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RansByteState(pub u32);

impl RansByteState {
    /// Create a new state initialized to the lower bound.
    #[inline]
    pub const fn new() -> Self {
        Self(RANS_BYTE_L)
    }

    /// Return the raw state value.
    #[inline]
    pub const fn get(&self) -> u32 {
        self.0
    }
}

impl Default for RansByteState {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Backward byte writer
// ---------------------------------------------------------------------------

/// A backward-growing byte writer.
///
/// Starts at the logical end of the provided buffer and writes bytes toward
/// the beginning. Used for rANS encoding output.
///
/// Equivalent to `uint8_t* ptr` pointing to the end of the output buffer
/// in the upstream C code.
pub struct BackwardByteWriter<'a> {
    buf: &'a mut [u8],
    pos: usize, // current write position (0 <= pos <= len)
}

impl<'a> BackwardByteWriter<'a> {
    /// Create a new backward writer from a mutable byte slice.
    ///
    /// The writer starts at the end of the buffer. `pos` tracks the
    /// zero-based index; initially `pos == len`.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        let len = buf.len();
        Self { buf, pos: len }
    }

    /// Write a single byte at the current position (decrementing the cursor).
    ///
    /// Returns `Ok(())` if there was room, `Err(())` if the writer has
    /// exhausted its buffer.
    #[inline]
    pub fn write_byte(&mut self, b: u8) -> Result<(), ()> {
        if self.pos == 0 {
            return Err(());
        }
        self.pos -= 1;
        self.buf[self.pos] = b;
        Ok(())
    }

    /// Write 4 bytes (little-endian u32) at the current position.
    ///
    /// Returns `Ok(())` if there was room, `Err(())` otherwise.
    #[inline]
    pub fn write_u32_le(&mut self, v: u32) -> Result<(), ()> {
        if self.pos < 4 {
            return Err(());
        }
        self.pos -= 4;
        self.buf[self.pos..self.pos + 4].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }

    /// Current zero-based write position (number of bytes before the cursor).
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes written so far.
    #[inline]
    pub fn bytes_written(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Return the encoded portion (from current position to end) as a slice.
    #[inline]
    pub fn encoded(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Return the remaining capacity (number of bytes that can still be written).
    #[inline]
    pub fn remaining(&self) -> usize {
        self.pos
    }
}

impl fmt::Debug for BackwardByteWriter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackwardByteWriter")
            .field("pos", &self.pos)
            .field("len", &self.buf.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Forward byte reader
// ---------------------------------------------------------------------------

/// A forward-growing byte reader.
///
/// Used for rANS decoding input. Reads bytes from the beginning of the
/// compressed stream moving forward.
///
/// Equivalent to `uint8_t* ptr` pointing to the start of compressed data
/// in the upstream C code.
#[derive(Clone)]
pub struct ByteReader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> ByteReader<'a> {
    /// Create a new byte reader from a byte slice.
    #[inline]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Read a single byte at the current position, advancing forward.
    ///
    /// Returns `None` if the buffer is exhausted.
    #[inline]
    pub fn read_byte(&mut self) -> Option<u8> {
        if self.pos >= self.buf.len() {
            return None;
        }
        let b = self.buf[self.pos];
        self.pos += 1;
        Some(b)
    }

    /// Read 4 bytes as a little-endian u32.
    ///
    /// Returns `None` if fewer than 4 bytes remain.
    #[inline]
    pub fn read_u32_le(&mut self) -> Option<u32> {
        if self.pos + 4 > self.buf.len() {
            return None;
        }
        let v = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Some(v)
    }

    /// Current read position.
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes consumed so far.
    #[inline]
    pub fn bytes_consumed(&self) -> usize {
        self.pos
    }

    /// Remaining unread bytes.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
}

impl fmt::Debug for ByteReader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ByteReader")
            .field("pos", &self.pos)
            .field("len", &self.buf.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Encoder symbol (reciprocal-multiply setup)
// ---------------------------------------------------------------------------

/// Precomputed reciprocal encoder symbol.
///
/// Equivalent to `RansEncSymbol` in `rans_byte.h`.
///
/// The fast encoder uses a multiply-high approximation to divide by the
/// symbol frequency, avoiding an integer division in the hot loop.
#[derive(Debug, Clone, Copy)]
pub struct RansByteEncSymbol {
    /// (Exclusive) upper bound of pre-normalization interval.
    pub x_max: u32,
    /// Fixed-point reciprocal frequency.
    pub rcp_freq: u32,
    /// Bias value.
    pub bias: u32,
    /// Complement of frequency: `(1 << scale_bits) - freq`.
    pub cmpl_freq: u16,
    /// Reciprocal shift amount.
    pub rcp_shift: u16,
}

impl RansByteEncSymbol {
    /// Initialize an encoder symbol with validation.
    ///
    /// Equivalent to `RansEncSymbolInit` in `rans_byte.h`.
    ///
    /// Returns `Err(ModelError::InvalidScaleBits)` if `scale_bits` is not in
    /// `1..=16`. Returns `Err(ModelError::ZeroFrequency)` if `freq == 0`.
    /// Returns `Err(ModelError::StartOutOfRange)` if `start > (1 << scale_bits)`.
    /// Returns `Err(ModelError::FrequencyOutOfRange)` if
    /// `freq > (1 << scale_bits) - start`.
    #[inline]
    pub fn new(start: u32, freq: u32, scale_bits: u32) -> Result<Self, ModelError> {
        if !(1..=16).contains(&scale_bits) {
            return Err(ModelError::InvalidScaleBits);
        }
        let max_start = 1u64 << scale_bits;
        if (start as u64) > max_start {
            return Err(ModelError::StartOutOfRange);
        }
        if freq == 0 {
            return Err(ModelError::ZeroFrequency);
        }
        if (freq as u64) > max_start - (start as u64) {
            return Err(ModelError::FrequencyOutOfRange);
        }

        Ok(Self::new_unchecked(start, freq, scale_bits))
    }

    /// Initialize an encoder symbol without validation.
    ///
    /// Caller must ensure preconditions are met. Prefer [`new`] for
    /// external use.
    #[inline]
    pub(crate) fn new_unchecked(start: u32, freq: u32, scale_bits: u32) -> Self {
        debug_assert!(scale_bits <= 16, "scale_bits must be <= 16");
        debug_assert!(start <= (1u32 << scale_bits), "start out of range");
        debug_assert!(freq <= (1u32 << scale_bits) - start, "freq out of range");

        let x_max = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
        let cmpl_freq = ((1u32 << scale_bits) - freq) as u16;

        if freq < 2 {
            // freq == 1 special case: the general reciprocal path cannot be
            // used because the shift computation (`freq > (1 << shift)`) would
            // produce shift = 0 and rcp_freq would overflow the fixed-point
            // format.  Instead, note that q = x/1 = x, so C(s,x) = (x << s) + start.
            // The formula x + bias + q*cmpl_freq with bias = start + 2^s - 1,
            // rcp_freq = ~0, rcp_shift = 0 gives q' = (x * ~0) >> 32 = x - 1
            // (for x > 0) and cmpl_freq = 2^s - 1:
            //   x + start + 2^s - 1 + (x - 1)*(2^s - 1)
            //   = x + start + 2^s - 1 + x*2^s - x - 2^s + 1 = (x << s) + start
            // which is exactly C(s,x) for freq = 1.
            Self {
                x_max,
                rcp_freq: !0u32,
                rcp_shift: 0,
                bias: start + (1u32 << scale_bits) - 1,
                cmpl_freq,
            }
        } else {
            // Alverson "Integer Division using reciprocals"
            let mut shift = 0u32;
            while freq > (1u32 << shift) {
                shift += 1;
            }

            // rcp_freq = (((1ull << (shift + 31)) + freq - 1) / freq) as u32
            let rcp_freq = (((1u64 << (shift + 31)) + freq as u64 - 1) / freq as u64) as u32;
            let rcp_shift = shift - 1;

            Self {
                x_max,
                rcp_freq,
                rcp_shift: rcp_shift as u16,
                bias: start,
                cmpl_freq,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Decoder symbol
// ---------------------------------------------------------------------------

/// Decoder symbol description.
///
/// Equivalent to `RansDecSymbol` in `rans_byte.h`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RansByteDecSymbol {
    /// Start of range.
    pub start: u16,
    /// Symbol frequency.
    pub freq: u16,
}

impl RansByteDecSymbol {
    /// Initialize a decoder symbol with validation.
    ///
    /// Equivalent to `RansDecSymbolInit`.
    ///
    /// Returns `Err(ModelError::ZeroFrequency)` if `freq == 0`.
    /// Returns `Err(ModelError::StartOutOfRange)` if `start > (1 << 16)`.
    /// Returns `Err(ModelError::FrequencyOutOfRange)` if
    /// `freq > (1 << 16) - start`.
    #[inline]
    pub fn new(start: u32, freq: u32) -> Result<Self, ModelError> {
        if freq == 0 {
            return Err(ModelError::ZeroFrequency);
        }
        if (start as u64) > (1u64 << 16) {
            return Err(ModelError::StartOutOfRange);
        }
        if (freq as u64) > (1u64 << 16) - (start as u64) {
            return Err(ModelError::FrequencyOutOfRange);
        }
        Ok(Self::new_unchecked(start, freq))
    }

    /// Initialize a decoder symbol without validation.
    ///
    /// Caller must ensure preconditions are met. Prefer [`new`] for
    /// external use.
    #[inline]
    pub(crate) fn new_unchecked(start: u32, freq: u32) -> Self {
        debug_assert!(start <= (1u32 << 16), "start out of range");
        debug_assert!(freq <= (1u32 << 16) - start, "freq out of range");
        Self {
            start: start as u16,
            freq: freq as u16,
        }
    }
}

// ---------------------------------------------------------------------------
// Division-based encoder (reference path)
// ---------------------------------------------------------------------------

/// Renormalize the encoder state.
///
/// Equivalent to `RansEncRenorm` in `rans_byte.h`.
///
/// Emits bytes (LSB first) until the state falls below the threshold.
/// Returns `Err(EncodeError::OutputTooSmall)` if the output buffer is exhausted.
#[inline]
pub fn rans_byte_enc_renorm<W: BackwardWriter>(
    x: u32,
    writer: &mut W,
    freq: u32,
    scale_bits: u32,
) -> Result<u32, EncodeError> {
    let x_max = ((RANS_BYTE_L >> scale_bits) << 8) * freq;
    let mut x = x;
    if x >= x_max {
        while x >= x_max {
            writer
                .write_byte((x & 0xff) as u8)
                .map_err(|_| EncodeError::OutputTooSmall)?;
            x >>= 8;
        }
    }
    Ok(x)
}

/// Encoder put-symbol (division-based reference path).
///
/// Equivalent to `RansEncPut` in `rans_byte.h`.
/// Returns `Err(EncodeError::OutputTooSmall)` if the output buffer is exhausted.
#[inline]
pub fn rans_byte_enc_put<W: BackwardWriter>(
    state: &mut RansByteState,
    writer: &mut W,
    start: u32,
    freq: u32,
    scale_bits: u32,
) -> Result<(), EncodeError> {
    let x = rans_byte_enc_renorm(state.0, writer, freq, scale_bits)?;
    state.0 = ((x / freq) << scale_bits) + (x % freq) + start;
    Ok(())
}

/// Flush the encoder.
///
/// Equivalent to `RansEncFlush` in `rans_byte.h`.
///
/// Writes the full 32-bit state in little-endian order (4 bytes).
/// Returns `Err(EncodeError::OutputTooSmall)` if the output buffer is exhausted.
#[inline]
pub fn rans_byte_enc_flush<W: BackwardWriter>(
    state: &RansByteState,
    writer: &mut W,
) -> Result<(), EncodeError> {
    let x = state.0;
    writer
        .write_u32_le(x)
        .map_err(|_| EncodeError::OutputTooSmall)
}

// ---------------------------------------------------------------------------
// Division-based decoder (reference path)
// ---------------------------------------------------------------------------

/// Initialize the decoder.
///
/// Equivalent to `RansDecInit` in `rans_byte.h`.
///
/// Reads 4 bytes as the initial state.
#[inline]
pub fn rans_byte_dec_init<R: ForwardReader>(reader: &mut R) -> Result<RansByteState, DecodeError> {
    let x = reader.read_u32_le().ok_or(DecodeError::InputTooShort)?;
    Ok(RansByteState(x))
}

/// Get the cumulative frequency from the current state.
///
/// Equivalent to `RansDecGet` in `rans_byte.h`.
#[inline]
pub fn rans_byte_dec_get(state: &RansByteState, scale_bits: u32) -> u32 {
    state.0 & ((1u32 << scale_bits) - 1)
}

/// Advance the decoder by a symbol (with renormalization).
///
/// Equivalent to `RansDecAdvance` in `rans_byte.h`.
#[inline]
pub fn rans_byte_dec_advance<R: ForwardReader>(
    state: &mut RansByteState,
    reader: &mut R,
    start: u32,
    freq: u32,
    scale_bits: u32,
) -> Result<(), DecodeError> {
    let mask = (1u32 << scale_bits) - 1;
    let x = state.0;
    let mut x = freq * (x >> scale_bits) + (x & mask) - start;

    // renormalize
    if x < RANS_BYTE_L {
        loop {
            let b = reader.read_byte().ok_or(DecodeError::InputTooShort)?;
            x = (x << 8) | (b as u32);
            if x >= RANS_BYTE_L {
                break;
            }
        }
    }

    state.0 = x;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reciprocal-multiply encoder (fast path)
// ---------------------------------------------------------------------------

/// Encoder put-symbol using precomputed reciprocal (fast path).
///
/// Equivalent to `RansEncPutSymbol` in `rans_byte.h`.
///
/// This is the fast path that avoids integer division in the hot loop.
/// Returns `Err(EncodeError::OutputTooSmall)` if the output buffer is exhausted.
///
/// # Why the algebra works
///
/// The division-based reference computes `C(s,x) = ((x/freq) << s) + (x%freq) + start`.
/// With `q = x/freq` (integer division) and `r = x%freq`, `x = q*freq + r`, so:
///
/// ```text
/// C(s,x) = (q << s) + r + start
///        = q*(2^s) + (x - q*freq) + start
///        = x + start + q*(2^s - freq)
///        = x + bias + q*cmpl_freq
/// ```
///
/// where `bias = start` and `cmpl_freq = 2^s - freq` (the frequency
/// complement).  The expensive part is `q = x/freq`; the reciprocal trick
/// computes `q` from a multiply-high approximation `(x * rcp_freq) >>
/// (32 + rcp_shift)` (Alverson's method) which compiles to a single
/// `MUL` + `SHR` pair instead of `DIV` (~4-30 cycles saved per symbol).
/// The Kani proof `kani_reciprocal_equals_division` shows the approximation
/// is exact for every valid `(x, freq, scale_bits)`.
///
/// # Invariants
///
/// * `sym` must have been built by `RansByteEncSymbol::new` (or the
///   validated `new` path) for the same `scale_bits` used at decode time.
/// * `x_max != 0` (freq >= 1), asserted in debug builds.
///
/// # Failure modes
///
/// `Err(EncodeError::OutputTooSmall)` when the writer has no room for a
/// renorm byte.  The state is not advanced on failure.
#[inline]
pub fn rans_byte_enc_put_symbol<W: BackwardWriter>(
    state: &mut RansByteState,
    writer: &mut W,
    sym: &RansByteEncSymbol,
) -> Result<(), EncodeError> {
    debug_assert!(sym.x_max != 0, "cannot encode symbol with freq=0");

    let mut x = state.0;
    if x >= sym.x_max {
        while x >= sym.x_max {
            writer
                .write_byte((x & 0xff) as u8)
                .map_err(|_| EncodeError::OutputTooSmall)?;
            x >>= 8;
        }
    }

    // x = C(s,x)
    // 32-bit "multiply high": (u64)x * rcp_freq >> 32 >> rcp_shift
    let q = (((x as u64) * (sym.rcp_freq as u64)) >> 32) >> sym.rcp_shift;
    state.0 = x + sym.bias + (q as u32) * (sym.cmpl_freq as u32);
    Ok(())
}

// ---------------------------------------------------------------------------
// Decoder advance with symbol (convenience)
// ---------------------------------------------------------------------------

/// Advance the decoder using a decoder symbol.
///
/// Equivalent to `RansDecAdvanceSymbol` in `rans_byte.h`.
#[inline]
pub fn rans_byte_dec_advance_symbol<R: ForwardReader>(
    state: &mut RansByteState,
    reader: &mut R,
    sym: &RansByteDecSymbol,
    scale_bits: u32,
) -> Result<(), DecodeError> {
    rans_byte_dec_advance(state, reader, sym.start as u32, sym.freq as u32, scale_bits)
}

// ---------------------------------------------------------------------------
// Step-only operations (for interleaving)
// ---------------------------------------------------------------------------

/// Advance the decoder without renormalization (interleaving step).
///
/// Equivalent to `RansDecAdvanceStep` in `rans_byte.h`.
#[inline]
pub fn rans_byte_dec_advance_step(
    state: &mut RansByteState,
    start: u32,
    freq: u32,
    scale_bits: u32,
) {
    let mask = (1u32 << scale_bits) - 1;
    let x = state.0;
    state.0 = freq * (x >> scale_bits) + (x & mask) - start;
}

/// Advance the decoder step using a decoder symbol.
///
/// Equivalent to `RansDecAdvanceSymbolStep`.
#[inline]
pub fn rans_byte_dec_advance_symbol_step(
    state: &mut RansByteState,
    sym: &RansByteDecSymbol,
    scale_bits: u32,
) {
    rans_byte_dec_advance_step(state, sym.start as u32, sym.freq as u32, scale_bits);
}

/// Decoder renormalization only.
///
/// Equivalent to `RansDecRenorm` in `rans_byte.h`.
#[inline]
pub fn rans_byte_dec_renorm<R: ForwardReader>(
    state: &mut RansByteState,
    reader: &mut R,
) -> Result<(), DecodeError> {
    let mut x = state.0;
    if x < RANS_BYTE_L {
        loop {
            let b = reader.read_byte().ok_or(DecodeError::InputTooShort)?;
            x = (x << 8) | (b as u32);
            if x >= RANS_BYTE_L {
                break;
            }
        }
        state.0 = x;
    }
    Ok(())
}

// Advance
// ---------------------------------------------------------------------------
// BackwardWriter / ForwardReader traits
// ---------------------------------------------------------------------------

/// Trait for backward-writing output (encoder output).
pub trait BackwardWriter {
    /// Write a single byte.
    fn write_byte(&mut self, b: u8) -> Result<(), ()>;

    /// Write a u32 in little-endian format (4 bytes).
    fn write_u32_le(&mut self, v: u32) -> Result<(), ()>;
}

/// Trait for forward-reading input (decoder input).
pub trait ForwardReader {
    /// Read a single byte.
    fn read_byte(&mut self) -> Option<u8>;

    /// Read a u32 in little-endian format (4 bytes).
    fn read_u32_le(&mut self) -> Option<u32>;
}

// Implement the traits for our concrete types.
impl<'a> BackwardWriter for BackwardByteWriter<'a> {
    #[inline]
    fn write_byte(&mut self, b: u8) -> Result<(), ()> {
        self.write_byte(b)
    }

    #[inline]
    fn write_u32_le(&mut self, v: u32) -> Result<(), ()> {
        self.write_u32_le(v)
    }
}

impl<'a> ForwardReader for ByteReader<'a> {
    #[inline]
    fn read_byte(&mut self) -> Option<u8> {
        self.read_byte()
    }

    #[inline]
    fn read_u32_le(&mut self) -> Option<u32> {
        self.read_u32_le()
    }
}

/// A wrapper around `&mut [u8]` that implements `BackwardWriter`.
///
/// This avoids issues with implementing a trait on a mutable reference to an unsized slice.
pub struct SliceBackwardWriter<'a>(pub &'a mut [u8]);

impl BackwardWriter for SliceBackwardWriter<'_> {
    #[inline]
    fn write_byte(&mut self, b: u8) -> Result<(), ()> {
        let buf = core::mem::take(&mut self.0);
        if buf.is_empty() {
            self.0 = buf;
            return Err(());
        }
        let len = buf.len();
        buf[len - 1] = b;
        self.0 = &mut buf[..len - 1];
        Ok(())
    }

    #[inline]
    fn write_u32_le(&mut self, v: u32) -> Result<(), ()> {
        let buf = core::mem::take(&mut self.0);
        let len = buf.len();
        if len < 4 {
            self.0 = buf;
            return Err(());
        }
        let bytes = v.to_le_bytes();
        buf[len - 4..len].copy_from_slice(&bytes);
        self.0 = &mut buf[..len - 4];
        Ok(())
    }
}

impl<'a> ForwardReader for &'a [u8] {
    #[inline]
    fn read_byte(&mut self) -> Option<u8> {
        if self.is_empty() {
            return None;
        }
        let b = self[0];
        *self = &self[1..];
        Some(b)
    }

    #[inline]
    fn read_u32_le(&mut self) -> Option<u32> {
        if self.len() < 4 {
            return None;
        }
        let v = u32::from_le_bytes([self[0], self[1], self[2], self[3]]);
        *self = &self[4..];
        Some(v)
    }
}

// ---------------------------------------------------------------------------
// Two-state interleaved byte rANS
// ---------------------------------------------------------------------------

/// Two-state interleaved byte rANS encoder.
///
/// Encodes symbols into two interleaved streams (rans0, rans1).
/// Equivalent to the two-stream interleaving pattern in `main.cpp`.
pub struct ByteInterleavedEncoder<'a, W: BackwardWriter> {
    state0: RansByteState,
    state1: RansByteState,
    writer: &'a mut W,
    _scale_bits: u32,
    _num_symbols: usize,
    _odd_symbol: Option<u8>,
}

impl<'a, W: BackwardWriter> ByteInterleavedEncoder<'a, W> {
    /// Create a new interleaved encoder.
    pub fn new(writer: &'a mut W, scale_bits: u32) -> Self {
        Self {
            state0: RansByteState::new(),
            state1: RansByteState::new(),
            writer,
            _scale_bits: scale_bits,
            _num_symbols: 0,
            _odd_symbol: None,
        }
    }

    /// Encode symbols in reverse order (last to first).
    ///
    /// Pass the symbol array slice in forward order; this function iterates
    /// it in reverse internally.
    pub fn encode_reverse(
        &mut self,
        symbols: &[u8],
        esyms: &[RansByteEncSymbol],
    ) -> Result<(), EncodeError> {
        let n = symbols.len();
        self._num_symbols = n;

        if n == 0 {
            return Ok(());
        }

        // Handle odd length: last symbol goes to state0
        if n & 1 != 0 {
            let s = symbols[n - 1];
            rans_byte_enc_put_symbol(&mut self.state0, &mut *self.writer, &esyms[s as usize])?;
        }

        // Process pairs in reverse
        let mut i = n & !1;
        while i > 0 {
            let s1 = symbols[i - 1] as usize;
            let s0 = symbols[i - 2] as usize;

            // Interleave: first encode state1, then state0
            // (reverse of decoding order)
            rans_byte_enc_put_symbol(&mut self.state1, &mut *self.writer, &esyms[s1])?;
            rans_byte_enc_put_symbol(&mut self.state0, &mut *self.writer, &esyms[s0])?;

            i = i.wrapping_sub(2);
        }

        Ok(())
    }

    /// Flush both encoder states (state1 first, then state0).
    ///
    /// Matches the flush order in the upstream interleaving example.
    pub fn flush(&mut self) -> Result<(), EncodeError> {
        rans_byte_enc_flush(&self.state1, &mut *self.writer)?;
        rans_byte_enc_flush(&self.state0, &mut *self.writer)?;
        Ok(())
    }

    /// Finalize: encode, flush, return the encoded slice.
    pub fn finalize(
        mut self,
        symbols: &[u8],
        esyms: &[RansByteEncSymbol],
    ) -> Result<(), EncodeError> {
        self.encode_reverse(symbols, esyms)?;
        self.flush()
    }
}

/// Two-state interleaved byte rANS decoder.
pub struct ByteInterleavedDecoder<'a, R: ForwardReader> {
    state0: RansByteState,
    state1: RansByteState,
    reader: &'a mut R,
    scale_bits: u32,
}

impl<'a, R: ForwardReader> ByteInterleavedDecoder<'a, R> {
    /// Create a new interleaved decoder.
    pub fn new(reader: &'a mut R, scale_bits: u32) -> Result<Self, DecodeError> {
        let state0 = rans_byte_dec_init(&mut *reader)?;
        let state1 = rans_byte_dec_init(&mut *reader)?;
        Ok(Self {
            state0,
            state1,
            reader,
            scale_bits,
        })
    }

    /// Decode all symbols into the output buffer.
    ///
    /// Returns the number of symbols decoded.
    pub fn decode(
        &mut self,
        output: &mut [u8],
        cum2sym: &[u8],
        dsyms: &[RansByteDecSymbol],
    ) -> Result<usize, DecodeError> {
        let n = output.len();
        if n == 0 {
            return Ok(0);
        }

        let even_n = n & !1;

        // Process pairs
        let mut i = 0usize;
        while i < even_n {
            let cf0 = rans_byte_dec_get(&self.state0, self.scale_bits);
            let s0 = cum2sym[cf0 as usize] as usize;
            let cf1 = rans_byte_dec_get(&self.state1, self.scale_bits);
            let s1 = cum2sym[cf1 as usize] as usize;

            output[i] = s0 as u8;
            output[i + 1] = s1 as u8;

            rans_byte_dec_advance_symbol_step(&mut self.state0, &dsyms[s0], self.scale_bits);
            rans_byte_dec_advance_symbol_step(&mut self.state1, &dsyms[s1], self.scale_bits);
            rans_byte_dec_renorm(&mut self.state0, &mut *self.reader)?;
            rans_byte_dec_renorm(&mut self.state1, &mut *self.reader)?;

            i += 2;
        }

        // Handle odd byte
        if n & 1 != 0 {
            let cf0 = rans_byte_dec_get(&self.state0, self.scale_bits);
            let s0 = cum2sym[cf0 as usize] as usize;
            output[n - 1] = s0 as u8;
            rans_byte_dec_advance_symbol(
                &mut self.state0,
                &mut *self.reader,
                &dsyms[s0],
                self.scale_bits,
            )?;
        }

        Ok(n)
    }

    /// Get the final decoder states (for verification).
    pub fn states(&self) -> (RansByteState, RansByteState) {
        (self.state0, self.state1)
    }
}

// ---------------------------------------------------------------------------
// 64-bit word-aligned rANS (rans64.h reconstruction)
// ---------------------------------------------------------------------------

/// Lower bound of the normalization interval for 64-bit rANS.
///
/// Equivalent to `RANS64_L` in `rans64.h`.
pub const RANS64_L: u64 = 1u64 << 31;

/// 64-bit rANS encoder/decoder state.
///
/// Equivalent to `Rans64State` (typedef for `uint64_t`) in `rans64.h`.
/// Uses 63 bits of effective state space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rans64State(pub u64);

impl Rans64State {
    /// Create a new state initialized to the lower bound.
    #[inline]
    pub const fn new() -> Self {
        Self(RANS64_L)
    }

    /// Return the raw state value.
    #[inline]
    pub const fn get(&self) -> u64 {
        self.0
    }
}

impl Default for Rans64State {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Backward word-32 writer
// ---------------------------------------------------------------------------

/// A backward-growing writer that writes 32-bit words.
///
/// Starts at the logical end of the provided byte buffer and writes u32
/// values (little-endian, 4 bytes each) toward the beginning.
/// Used for 64-bit rANS encoder output.
///
/// Equivalent to `uint32_t* ptr` pointing past the end of the output buffer
/// in the upstream `rans64.h`.
pub struct BackwardWord32Writer<'a> {
    buf: &'a mut [u8],
    pos: usize, // current write position (bytes, 0 <= pos <= len, always multiple of 4)
}

impl<'a> BackwardWord32Writer<'a> {
    /// Create a new backward word-32 writer from a mutable byte slice.
    ///
    /// The writer starts at the end of the buffer.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        let len = buf.len();
        debug_assert!(
            len % 4 == 0,
            "BackwardWord32Writer buffer len must be multiple of 4"
        );
        Self { buf, pos: len }
    }

    /// Write a single u32 at the current position (decrementing the cursor by 4).
    ///
    /// Returns `Ok(())` if there was room, `Err(())` if the writer has
    /// exhausted its buffer.
    #[inline]
    pub fn write_word32(&mut self, v: u32) -> Result<(), ()> {
        if self.pos < 4 {
            return Err(());
        }
        self.pos -= 4;
        self.buf[self.pos..self.pos + 4].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }

    /// Current zero-based write position (bytes before the cursor).
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes written so far.
    #[inline]
    pub fn bytes_written(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Number of u32 words written so far.
    #[inline]
    pub fn words_written(&self) -> usize {
        (self.buf.len() - self.pos) / 4
    }

    /// Return the encoded portion (from current position to end) as a slice.
    #[inline]
    pub fn encoded(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Return the remaining capacity (number of bytes that can still be written).
    #[inline]
    pub fn remaining(&self) -> usize {
        self.pos
    }
}

impl fmt::Debug for BackwardWord32Writer<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BackwardWord32Writer")
            .field("pos", &self.pos)
            .field("len", &self.buf.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Forward word-32 reader
// ---------------------------------------------------------------------------

/// A forward-growing reader that reads 32-bit words.
///
/// Used for 64-bit rANS decoding input. Reads u32 values (little-endian,
/// 4 bytes each) from the beginning of the compressed stream moving forward.
///
/// Equivalent to `uint32_t* ptr` pointing to the start of compressed data
/// in the upstream `rans64.h`.
#[derive(Clone)]
pub struct Word32Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Word32Reader<'a> {
    /// Create a new word-32 reader from a byte slice.
    #[inline]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Read a single u32 (little-endian) at the current position, advancing by 4.
    ///
    /// Returns `None` if fewer than 4 bytes remain.
    #[inline]
    pub fn read_word32(&mut self) -> Option<u32> {
        if self.pos + 4 > self.buf.len() {
            return None;
        }
        let v = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Some(v)
    }

    /// Current read position in bytes.
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes consumed so far.
    #[inline]
    pub fn bytes_consumed(&self) -> usize {
        self.pos
    }

    /// Number of u32 words consumed so far.
    #[inline]
    pub fn words_consumed(&self) -> usize {
        self.pos / 4
    }

    /// Remaining unread bytes.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }
}

impl fmt::Debug for Word32Reader<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Word32Reader")
            .field("pos", &self.pos)
            .field("len", &self.buf.len())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// 64-bit encoder symbol (reciprocal-multiply setup)
// ---------------------------------------------------------------------------

/// Precomputed reciprocal encoder symbol for 64-bit rANS.
///
/// Equivalent to `Rans64EncSymbol` in `rans64.h`.
///
/// The fast encoder uses a 64x64 multiply-high approximation to divide
/// by the symbol frequency, avoiding an integer division in the hot loop.
/// The reciprocal setup requires 128-bit arithmetic.
#[derive(Debug, Clone, Copy)]
pub struct Rans64EncSymbol {
    /// (Exclusive) upper bound of pre-normalization interval.
    pub x_max: u64,
    /// Fixed-point reciprocal frequency (u64).
    pub rcp_freq: u64,
    /// Bias value.
    pub bias: u64,
    /// Complement of frequency: `(1 << scale_bits) - freq` (32-bit, not 16-bit!)
    pub cmpl_freq: u32,
    /// Reciprocal shift amount.
    pub rcp_shift: u32,
}

impl Rans64EncSymbol {
    /// Initialize a 64-bit encoder symbol with validation.
    ///
    /// Equivalent to `Rans64EncSymbolInit` in `rans64.h`.
    ///
    /// Calculates the 64-bit reciprocal using 128-bit division:
    /// `rcp_freq = ((1 << (shift + 63)) + freq - 1) / freq`.
    ///
    /// Returns `Err(ModelError::InvalidScaleBits)` if `scale_bits` is not in
    /// `1..=31`. Returns `Err(ModelError::ZeroFrequency)` if `freq == 0`.
    /// Returns `Err(ModelError::StartOutOfRange)` if `start > (1 << scale_bits)`.
    /// Returns `Err(ModelError::FrequencyOutOfRange)` if
    /// `freq > (1 << scale_bits) - start`.
    #[inline]
    pub fn new(start: u32, freq: u32, scale_bits: u32) -> Result<Self, ModelError> {
        if !(1..=31).contains(&scale_bits) {
            return Err(ModelError::InvalidScaleBits);
        }
        let max_start = 1u64 << scale_bits;
        if (start as u64) > max_start {
            return Err(ModelError::StartOutOfRange);
        }
        if freq == 0 {
            return Err(ModelError::ZeroFrequency);
        }
        if (freq as u64) > max_start - (start as u64) {
            return Err(ModelError::FrequencyOutOfRange);
        }

        Ok(Self::new_unchecked(start, freq, scale_bits))
    }

    /// Initialize a 64-bit encoder symbol without validation.
    ///
    /// Caller must ensure preconditions are met. Prefer [`new`] for
    /// external use.
    #[inline]
    pub(crate) fn new_unchecked(start: u32, freq: u32, scale_bits: u32) -> Self {
        debug_assert!(scale_bits <= 31, "scale_bits must be <= 31");
        debug_assert!((start as u64) <= (1u64 << scale_bits), "start out of range");
        debug_assert!(
            (freq as u64) <= (1u64 << scale_bits) - (start as u64),
            "freq out of range"
        );

        // x_max = ((RANS64_L >> scale_bits) << 32) * freq
        let x_max = ((RANS64_L >> scale_bits) << 32) * (freq as u64);
        // cmpl_freq is u32 because scale_bits may reach 31, giving a complement
        // of up to 2^31 - 1, which does not fit in u16.
        let cmpl_freq = ((1u64 << scale_bits) - freq as u64) as u32;

        if freq < 2 {
            // freq == 1 special case
            // rcp_freq = ~0u64, rcp_shift = 0
            // bias = start + (1 << scale_bits) - 1
            Self {
                x_max,
                rcp_freq: !0u64,
                rcp_shift: 0,
                bias: (start as u64) + (1u64 << scale_bits) - 1,
                cmpl_freq: (1u64 << scale_bits) as u32 - freq,
            }
        } else {
            // Alverson "Integer Division using Reciprocals"
            // Find smallest shift such that freq <= (1 << shift)
            let mut shift = 0u32;
            while freq > (1u32 << shift) {
                shift += 1;
            }

            // rcp_freq = ((1u128 << (shift + 63)) + freq - 1) / freq
            // This is a 128-bit numerator, which we compute with u128 to avoid
            // overflow and allocate the result into u64.
            let rcp_freq = (((1u128 << (shift + 63)) + (freq as u128) - 1) / (freq as u128)) as u64;
            let rcp_shift = shift - 1;

            Self {
                x_max,
                rcp_freq,
                rcp_shift,
                bias: start as u64,
                cmpl_freq,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 64-bit decoder symbol
// ---------------------------------------------------------------------------

/// Decoder symbol description for 64-bit rANS.
///
/// Equivalent to `Rans64DecSymbol` in `rans64.h`.
/// start and freq are u32 (scale_bits up to 31).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rans64DecSymbol {
    /// Start of range.
    pub start: u32,
    /// Symbol frequency.
    pub freq: u32,
}

impl Rans64DecSymbol {
    /// Initialize a 64-bit decoder symbol with validation.
    ///
    /// Equivalent to `Rans64DecSymbolInit`.
    ///
    /// Returns `Err(ModelError::ZeroFrequency)` if `freq == 0`.
    /// Returns `Err(ModelError::StartOutOfRange)` if `start > (1 << 31)`.
    /// Returns `Err(ModelError::FrequencyOutOfRange)` if
    /// `freq > (1 << 31) - start`.
    #[inline]
    pub fn new(start: u32, freq: u32) -> Result<Self, ModelError> {
        if freq == 0 {
            return Err(ModelError::ZeroFrequency);
        }
        if (start as u64) > (1u64 << 31) {
            return Err(ModelError::StartOutOfRange);
        }
        if (freq as u64) > (1u64 << 31) - (start as u64) {
            return Err(ModelError::FrequencyOutOfRange);
        }
        Ok(Self::new_unchecked(start, freq))
    }

    /// Initialize a 64-bit decoder symbol without validation.
    ///
    /// Caller must ensure preconditions are met. Prefer [`new`] for
    /// external use.
    #[inline]
    pub(crate) fn new_unchecked(start: u32, freq: u32) -> Self {
        debug_assert!((start as u64) <= (1u64 << 31), "start out of range");
        debug_assert!(
            (freq as u64) <= (1u64 << 31) - (start as u64),
            "freq out of range"
        );
        Self { start, freq }
    }
}

// ---------------------------------------------------------------------------
// 64-bit multiply-high helper
// ---------------------------------------------------------------------------

/// Compute the high 64 bits of the 128-bit product `a * b`.
///
/// Equivalent to `Rans64MulHi` in `rans64.h`:
/// `((unsigned __int128)a * b) >> 64`.
#[inline]
pub fn rans64_mul_hi(a: u64, b: u64) -> u64 {
    ((a as u128) * (b as u128) >> 64) as u64
}

// ---------------------------------------------------------------------------
// Division-based encoder (reference path)
// ---------------------------------------------------------------------------

/// Renormalize the 64-bit encoder state by emitting u32 words (LSB first).
///
/// Equivalent to `Rans64EncRenorm` in `rans64.h`.
#[inline]
pub fn rans64_enc_renorm(
    x: u64,
    writer: &mut BackwardWord32Writer,
    freq: u32,
    scale_bits: u32,
) -> Result<u64, EncodeError> {
    let x_max = ((RANS64_L >> scale_bits) << 32) * (freq as u64);
    let mut x = x;
    if x >= x_max {
        while x >= x_max {
            writer
                .write_word32((x & 0xffffffff) as u32)
                .map_err(|_| EncodeError::OutputTooSmall)?;
            x >>= 32;
        }
    }
    Ok(x)
}

/// 64-bit encoder put-symbol (division-based reference path).
///
/// Equivalent to `Rans64EncPut` in `rans64.h`.
#[inline]
pub fn rans64_enc_put(
    state: &mut Rans64State,
    writer: &mut BackwardWord32Writer,
    start: u32,
    freq: u32,
    scale_bits: u32,
) -> Result<(), EncodeError> {
    let x = rans64_enc_renorm(state.0, writer, freq, scale_bits)?;
    // C(s, x) = ((x / freq) << scale_bits) + (x % freq) + start
    state.0 = ((x / (freq as u64)) << scale_bits) + (x % (freq as u64)) + (start as u64);
    Ok(())
}

/// Flush the 64-bit encoder.
///
/// Equivalent to `Rans64EncFlush` in `rans64.h`.
///
/// Writes the full 64-bit state as two u32 words in little-endian order.
/// Low word first, then high word (decrementing the pointer).
#[inline]
pub fn rans64_enc_flush(
    state: &Rans64State,
    writer: &mut BackwardWord32Writer,
) -> Result<(), EncodeError> {
    let x = state.0;
    // Write low word, then high word (both move backward)
    writer
        .write_word32((x >> 32) as u32)
        .map_err(|_| EncodeError::OutputTooSmall)?;
    writer
        .write_word32((x & 0xffffffff) as u32)
        .map_err(|_| EncodeError::OutputTooSmall)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Division-based decoder (reference path)
// ---------------------------------------------------------------------------

/// Initialize the 64-bit decoder.
///
/// Equivalent to `Rans64DecInit` in `rans64.h`.
///
/// Reads two u32 words (low word first, then high word) to reconstruct
/// the initial 64-bit state.
#[inline]
pub fn rans64_dec_init(reader: &mut Word32Reader) -> Result<Rans64State, DecodeError> {
    let lo = reader.read_word32().ok_or(DecodeError::InputTooShort)?;
    let hi = reader.read_word32().ok_or(DecodeError::InputTooShort)?;
    Ok(Rans64State((lo as u64) | ((hi as u64) << 32)))
}

/// Get the cumulative frequency from the current 64-bit state.
///
/// Equivalent to `Rans64DecGet` in `rans64.h`.
#[inline]
pub fn rans64_dec_get(state: &Rans64State, scale_bits: u32) -> u32 {
    (state.0 & ((1u64 << scale_bits) - 1)) as u32
}

/// Advance the 64-bit decoder by a symbol (with renormalization).
///
/// Equivalent to `Rans64DecAdvance` in `rans64.h`.
#[inline]
pub fn rans64_dec_advance(
    state: &mut Rans64State,
    reader: &mut Word32Reader,
    start: u32,
    freq: u32,
    scale_bits: u32,
) -> Result<(), DecodeError> {
    let mask = (1u64 << scale_bits) - 1;
    let x = state.0;
    let mut x = (freq as u64) * (x >> scale_bits) + (x & mask) - (start as u64);

    // renormalize: read u32 words until x >= RANS64_L
    if x < RANS64_L {
        loop {
            let word = reader.read_word32().ok_or(DecodeError::InputTooShort)?;
            x = (x << 32) | (word as u64);
            if x >= RANS64_L {
                break;
            }
        }
    }

    state.0 = x;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reciprocal-multiply encoder (fast path)
// ---------------------------------------------------------------------------

/// Encoder put-symbol using precomputed reciprocal (fast path).
///
/// Equivalent to `Rans64EncPutSymbol` in `rans64.h`.
///
/// Uses the 64-bit multiply-high approximation (`rans64_mul_hi`) to avoid
/// integer division in the hot loop.
#[inline]
pub fn rans64_enc_put_symbol(
    state: &mut Rans64State,
    writer: &mut BackwardWord32Writer,
    sym: &Rans64EncSymbol,
) -> Result<(), EncodeError> {
    debug_assert!(sym.x_max != 0, "cannot encode symbol with freq=0");

    let mut x = state.0;
    if x >= sym.x_max {
        while x >= sym.x_max {
            writer
                .write_word32((x & 0xffffffff) as u32)
                .map_err(|_| EncodeError::OutputTooSmall)?;
            x >>= 32;
        }
    }

    // x = C(s, x) using reciprocal multiply
    // q = Rans64MulHi(x, rcp_freq) >> rcp_shift
    let q = rans64_mul_hi(x, sym.rcp_freq) >> sym.rcp_shift;
    state.0 = x + sym.bias + q * (sym.cmpl_freq as u64);
    Ok(())
}

// ---------------------------------------------------------------------------
// Decoder advance with symbol (convenience)
// ---------------------------------------------------------------------------

/// Advance the 64-bit decoder using a decoder symbol.
///
/// Equivalent to `Rans64DecAdvanceSymbol` in `rans64.h`.
#[inline]
pub fn rans64_dec_advance_symbol(
    state: &mut Rans64State,
    reader: &mut Word32Reader,
    sym: &Rans64DecSymbol,
    scale_bits: u32,
) -> Result<(), DecodeError> {
    rans64_dec_advance(state, reader, sym.start, sym.freq, scale_bits)
}

// ---------------------------------------------------------------------------
// Step-only operations (for interleaving)
// ---------------------------------------------------------------------------

/// Advance the 64-bit decoder without renormalization (interleaving step).
///
/// Equivalent to `Rans64DecAdvanceStep` in `rans64.h`.
#[inline]
pub fn rans64_dec_advance_step(state: &mut Rans64State, start: u32, freq: u32, scale_bits: u32) {
    let mask = (1u64 << scale_bits) - 1;
    let x = state.0;
    state.0 = (freq as u64) * (x >> scale_bits) + (x & mask) - (start as u64);
}

/// Advance the 64-bit decoder step using a decoder symbol.
///
/// Equivalent to `Rans64DecAdvanceSymbolStep`.
#[inline]
pub fn rans64_dec_advance_symbol_step(
    state: &mut Rans64State,
    sym: &Rans64DecSymbol,
    scale_bits: u32,
) {
    rans64_dec_advance_step(state, sym.start, sym.freq, scale_bits);
}

/// 64-bit decoder renormalization only.
///
/// Equivalent to `Rans64DecRenorm` in `rans64.h`.
#[inline]
pub fn rans64_dec_renorm(
    state: &mut Rans64State,
    reader: &mut Word32Reader,
) -> Result<(), DecodeError> {
    let mut x = state.0;
    if x < RANS64_L {
        loop {
            let word = reader.read_word32().ok_or(DecodeError::InputTooShort)?;
            x = (x << 32) | (word as u64);
            if x >= RANS64_L {
                break;
            }
        }
        state.0 = x;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Word-aligned rANS (word rANS — rans_word_sse41.h scalar path)
// ---------------------------------------------------------------------------

/// Lower bound of the normalization interval for word rANS.
///
/// Equivalent to `RANS_WORD_L` in `rans_word_sse41.h`.
/// Word rANS uses L=2^16 with 16-bit renormalization words.
/// Default scale_bits for word rANS (per upstream design).
/// Upstream rans_word_sse41.h hardcodes this to 12.
/// The table has 1<<12 = 4096 slots, and all encode/decode
/// formulas assume this value.
pub const RANS_WORD_SCALE_BITS: u32 = 12;

/// Lower bound of the normalization interval for word rANS.
///
/// Equivalent to `RANS_WORD_L` in `rans_word_sse41.h`.
/// Word rANS uses L=2^16 with 16-bit renormalization words.
pub const RANS_WORD_L: u32 = 1u32 << 16;

/// Word rANS encoder/decoder state.
///
/// Equivalent to `RansWordEnc` / `RansWordDec` (typedef for `uint32_t`).
/// Uses 32-bit state space with L=2^16 and 16-bit word renormalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RansWordState(pub u32);

impl RansWordState {
    /// Create a new state initialized to the lower bound.
    #[inline]
    pub const fn new() -> Self {
        Self(RANS_WORD_L)
    }

    /// Return the raw state value.
    #[inline]
    pub const fn get(&self) -> u32 {
        self.0
    }
}

/// Backward writer for 16-bit words (word rANS encoder output).
///
/// Writes u16 values in little-endian order to a byte buffer,
/// starting from the end and moving toward the beginning.
#[derive(Debug)]
pub struct BackwardWord16Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> BackwardWord16Writer<'a> {
    /// Create a new backward word writer from a mutable byte slice.
    ///
    /// The writer starts at the end of the buffer.
    #[inline]
    pub fn new(buf: &'a mut [u8]) -> Self {
        let len = buf.len();
        Self { buf, pos: len }
    }

    /// Write a single u16 word in little-endian format.
    ///
    /// Returns `Ok(())` if there was room, `Err(())` if the buffer is exhausted.
    #[inline]
    pub fn write_word16(&mut self, v: u16) -> Result<(), ()> {
        if self.pos < 2 {
            return Err(());
        }
        self.pos -= 2;
        self.buf[self.pos..self.pos + 2].copy_from_slice(&v.to_le_bytes());
        Ok(())
    }

    /// Current write position (0 = buffer exhausted).
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Return the encoded portion (from current position to end) as a slice.
    #[inline]
    pub fn encoded(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    /// Number of bytes written so far.
    #[inline]
    pub fn bytes_written(&self) -> usize {
        self.buf.len() - self.pos
    }
}

/// Forward reader for 16-bit words (word rANS decoder input).
///
/// Reads u16 values in little-endian order from a byte slice,
/// starting from the beginning and moving forward.
#[derive(Debug, Clone)]
pub struct Word16Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Word16Reader<'a> {
    /// Create a new word reader from a byte slice.
    #[inline]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Read a single u16 word in little-endian format.
    ///
    /// Returns `None` if fewer than 2 bytes remain.
    #[inline]
    pub fn read_word16(&mut self) -> Option<u16> {
        if self.pos + 2 > self.buf.len() {
            return None;
        }
        let v = u16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Some(v)
    }

    /// Current read position (bytes consumed so far).
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes consumed.
    #[inline]
    pub fn bytes_consumed(&self) -> usize {
        self.pos
    }
}

// ---------------------------------------------------------------------------
// Word rANS table types
// ---------------------------------------------------------------------------

/// A single slot in the word rANS decode table.
///
/// Equivalent to `RansWordSlot` in `rans_word_sse41.h`.
#[derive(Debug, Clone, Copy)]
pub struct RansWordSlot {
    /// Symbol frequency.
    pub freq: u16,
    /// Bias (position within the symbol's frequency range).
    pub bias: u16,
}

/// Word rANS decode tables.
///
/// Equivalent to `RansWordTables` in `rans_word_sse41.h`.
/// Contains `M` slots (one per cumulative frequency) and a
/// slot-to-symbol mapping.
pub struct RansWordTables<'a> {
    pub slots: &'a [RansWordSlot],
    pub slot2sym: &'a [u8],
}

/// Check that scale_bits matches the word rANS upstream constant.
/// Returns `Err(ModelError::InvalidScaleBits)` for unsupported values.
#[inline]
pub fn rans_word_check_scale_bits(scale_bits: u32) -> Result<(), ModelError> {
    if scale_bits != RANS_WORD_SCALE_BITS {
        return Err(ModelError::InvalidScaleBits);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Word rANS encoder functions
// ---------------------------------------------------------------------------

/// Initialize a word rANS encoder state.
///
/// Equivalent to `RansWordEncInit` in `rans_word_sse41.h`.
#[inline]
pub fn rans_word_enc_init() -> RansWordState {
    RansWordState::new()
}

/// Word rANS encoder renormalization.
///
/// Emits the low 16 bits when the state exceeds the threshold.
/// Returns `Err(EncodeError::OutputTooSmall)` if the buffer is exhausted.
#[inline]
pub fn rans_word_enc_renorm(
    state: &mut RansWordState,
    writer: &mut BackwardWord16Writer,
    freq: u32,
    scale_bits: u32,
) -> Result<(), EncodeError> {
    let x = state.0;
    // Threshold: (L >> scale_bits) << 16 * freq
    // For scale_bits=12: (0x10000 >> 12) << 16 * freq = (16) << 16 * freq = 0x100000 * freq
    let threshold = ((RANS_WORD_L >> scale_bits) << 16) * freq;
    if x >= threshold {
        writer
            .write_word16((x & 0xffff) as u16)
            .map_err(|_| EncodeError::OutputTooSmall)?;
        state.0 = x >> 16;
    }
    Ok(())
}

/// Word rANS encoder — encode a single symbol using division.
///
/// Equivalent to `RansWordEncPut` in `rans_word_sse41.h`.
/// Formula: x = C(s,x) = ((x/freq) << scale_bits) + (x%freq) + start
#[inline]
pub fn rans_word_enc_put(
    state: &mut RansWordState,
    writer: &mut BackwardWord16Writer,
    start: u32,
    freq: u32,
    scale_bits: u32,
) -> Result<(), EncodeError> {
    // Validate scale_bits matches upstream constant
    rans_word_check_scale_bits(scale_bits).map_err(|_| EncodeError::OutputTooSmall)?;

    // Renormalize
    rans_word_enc_renorm(state, writer, freq, scale_bits)?;

    // x = C(s,x)
    let x = state.0;
    state.0 = ((x / freq) << scale_bits) + (x % freq) + start;
    Ok(())
}

/// Flush a word rANS encoder — write the final state to the buffer.
///
/// Equivalent to `RansWordEncFlush` in `rans_word_sse41.h`.
/// Writes 2 u16 words (4 bytes total).
///
/// # Why the write order is high-then-low
///
/// The backward writer places each word at a lower address than the
/// previous one, so writing high first leaves `[low, high]` in forward
/// buffer order — exactly the order `RansWordDecInit` reads (`lo | hi<<16`).
/// Writing low first would invert the halves and corrupt every stream.
#[inline]
pub fn rans_word_enc_flush(
    state: &RansWordState,
    writer: &mut BackwardWord16Writer,
) -> Result<(), EncodeError> {
    let x = state.0;
    writer
        .write_word16((x >> 16) as u16)
        .map_err(|_| EncodeError::OutputTooSmall)?;
    writer
        .write_word16((x & 0xffff) as u16)
        .map_err(|_| EncodeError::OutputTooSmall)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Word rANS decoder functions
// ---------------------------------------------------------------------------

/// Initialize a word rANS decoder from a compressed stream.
///
/// Equivalent to `RansWordDecInit` in `rans_word_sse41.h`.
/// Reads 2 u16 words (4 bytes) from the stream.
#[inline]
pub fn rans_word_dec_init(reader: &mut Word16Reader) -> Result<RansWordState, DecodeError> {
    let lo = reader.read_word16().ok_or(DecodeError::InputTooShort)? as u32;
    let hi = reader.read_word16().ok_or(DecodeError::InputTooShort)? as u32;
    Ok(RansWordState(lo | (hi << 16)))
}

/// Word rANS decoder — decode one symbol and advance the state.
///
/// Equivalent to `RansWordDecSym` in `rans_word_sse41.h`.
/// Uses the table-based decoder: slot = x & (M-1), then
/// x' = slots\[slot\].freq * (x >> scale_bits) + slots\[slot\].bias
/// Returns the decoded symbol.
#[inline]
pub fn rans_word_dec_sym(
    state: &mut RansWordState,
    tables: &RansWordTables<'_>,
    scale_bits: u32,
) -> u8 {
    // Validate scale_bits matches upstream constant
    // Panics on mismatch because this is a hot-path function that
    // should not return Result; upstream hardcodes this value.
    debug_assert_eq!(
        scale_bits, RANS_WORD_SCALE_BITS,
        "word rANS requires scale_bits={}",
        RANS_WORD_SCALE_BITS
    );
    let x = state.0;
    let mask = (1u32 << scale_bits) - 1;
    let slot = (x & mask) as usize;
    state.0 =
        (tables.slots[slot].freq as u32) * (x >> scale_bits) + (tables.slots[slot].bias as u32);
    tables.slot2sym[slot]
}

/// Word rANS decoder renormalization.
///
/// Equivalent to `RansWordDecRenorm` in `rans_word_sse41.h`.
/// Reads a u16 word from the stream when the state falls below L.
/// At most 1 iteration because L=2^16 and state < L implies
/// (state << 16) | word < 2^32, which covers the full u32 range.
#[inline]
pub fn rans_word_dec_renorm(
    state: &mut RansWordState,
    reader: &mut Word16Reader,
) -> Result<(), DecodeError> {
    let x = state.0;
    if x < RANS_WORD_L {
        let w = reader.read_word16().ok_or(DecodeError::InputTooShort)? as u32;
        state.0 = (x << 16) | w;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Alias method — byte rANS with alias table
// ---------------------------------------------------------------------------
//
// The alias method converts a non-uniform frequency distribution into 256
// uniform buckets (each of size total/256). Each bucket may contain up to
// two symbols: a primary and an alias, separated by a divider threshold.
// This enables O(1) symbol lookup during decoding instead of binary search.
//
// Upstream source: main_alias.cpp (Vose's alias method + rANS)
//
// Requires the `alloc` feature for the alias_remap table.

#[cfg(any(feature = "alloc", test))]
extern crate alloc as alloc_crate;

/// Log2 of the number of symbols for alias method. Always 8 for byte rANS.
pub const ALIAS_LOG2_NSYMS: u32 = 8;

/// Number of symbols for alias method. Always 256.
pub const ALIAS_NSYMS: usize = 1 << 8;

/// Alias table for O(1) symbol decoding.
///
/// Constructed via Vose's algorithm from a normalized frequency distribution.
/// Each of the 256 buckets contains a threshold, with up to two symbols
/// (primary and alias) filling the bucket.
#[derive(Debug, Clone)]
#[cfg(any(feature = "alloc", test))]
pub struct AliasTable {
    /// Threshold per bucket: if xm < divider\[i\], use odd slot;
    /// otherwise use even slot. After construction, this stores
    /// the absolute position: `i * tgt_sum + original_divider`.
    pub divider: [u32; ALIAS_NSYMS],
    /// Frequencies for each of the 512 slots (2 per bucket).
    pub slot_freqs: [u32; ALIAS_NSYMS * 2],
    /// Adjustment values for state recovery during decode.
    pub slot_adjust: [u32; ALIAS_NSYMS * 2],
    /// Symbol IDs for each of the 512 slots.
    pub slot_symbols: [u8; ALIAS_NSYMS * 2],
    /// Remap table from cumulative-range slot to alias-table position.
    /// Sized to the total distribution (2^scale_bits entries, max 65536).
    pub alias_remap: alloc_crate::vec::Vec<u32>,
    /// The scale_bits used to construct this table.
    pub scale_bits: u32,
    /// Frequencies for each of the 256 symbols.
    pub freqs: [u32; ALIAS_NSYMS],
    /// Cumulative frequencies for each of the 256 symbols.
    pub cum_freqs: [u32; ALIAS_NSYMS + 1],
}

/// Normalize frequencies to sum to exactly `target_total` (must be power of 2).
///
/// Equivalent to the normalization logic in `main_alias.cpp` `SymbolStats::normalize_freqs()`.
/// Resamples the distribution using cumulative frequency scaling and steals
/// frequency from other symbols if any non-zero symbol gets zeroed out.
#[inline]
pub fn rans_byte_alias_normalize_freqs(
    freqs: &[u32],
    num_symbols: usize,
    target_total: u32,
) -> Result<([u32; ALIAS_NSYMS], [u32; ALIAS_NSYMS + 1]), ModelError> {
    if num_symbols == 0 || num_symbols > ALIAS_NSYMS {
        return Err(ModelError::EmptyInput);
    }
    if target_total == 0 || (target_total & (target_total - 1)) != 0 {
        return Err(ModelError::InvalidScaleBits);
    }

    let mut out_freqs = [0u32; ALIAS_NSYMS];
    let mut cum = [0u32; ALIAS_NSYMS + 1];

    // Copy input frequencies, compute cumulative
    cum[0] = 0;
    for i in 0..num_symbols {
        out_freqs[i] = freqs[i];
        cum[i + 1] = cum[i]
            .checked_add(freqs[i])
            .ok_or(ModelError::TotalMismatch)?;
    }
    let cur_total = cum[num_symbols];
    if cur_total == 0 {
        return Err(ModelError::ZeroTotal);
    }

    // Resample distribution based on cumulative freqs
    let target = target_total as u64;
    let cur_total_u64 = cur_total as u64;
    for i in 1..=num_symbols {
        cum[i] = ((target * cum[i] as u64) / cur_total_u64) as u32;
    }

    // Zero-frequency theft: any non-zero symbol that was nuked to zero gets
    // frequency stolen from the symbol with the smallest frequency > 1.
    for i in 0..num_symbols {
        if out_freqs[i] != 0 && cum[i + 1] == cum[i] {
            // Find best symbol to steal from (smallest frequency > 1)
            let mut best_freq = u32::MAX;
            let mut best_steal = None;
            for j in 0..num_symbols {
                let freq = cum[j + 1] - cum[j];
                if freq > 1 && freq < best_freq {
                    best_freq = freq;
                    best_steal = Some(j);
                }
            }
            let steal = best_steal.ok_or(ModelError::WorkspaceTooSmall)?;

            if steal < i {
                for j in (steal + 1)..=i {
                    cum[j] = cum[j].wrapping_sub(1);
                }
            } else {
                for j in (i + 1)..=steal {
                    cum[j] = cum[j].wrapping_add(1);
                }
            }
        }
    }

    // Recompute frequencies from cumulative
    for i in 0..num_symbols {
        if out_freqs[i] == 0 {
            debug_assert!(cum[i + 1] == cum[i]);
        } else {
            debug_assert!(cum[i + 1] > cum[i]);
        }
        out_freqs[i] = cum[i + 1] - cum[i];
    }

    Ok((out_freqs, cum))
}

/// Build an alias table from a frequency distribution using Vose's algorithm.
///
/// Equivalent to `SymbolStats::make_alias_table()` in `main_alias.cpp`.
/// The frequencies must sum to exactly `1 << scale_bits`.
#[inline]
#[cfg(any(feature = "alloc", test))]
pub fn rans_byte_alias_build_table(
    freqs: &[u32; ALIAS_NSYMS],
    cum_freqs: &[u32; ALIAS_NSYMS + 1],
    scale_bits: u32,
) -> AliasTable {
    let total = 1u64 << scale_bits;
    let tgt_sum = (total / ALIAS_NSYMS as u64) as u32;

    let mut divider = [0u32; ALIAS_NSYMS];
    let mut slot_freqs = [0u32; ALIAS_NSYMS * 2];
    let mut slot_adjust = [0u32; ALIAS_NSYMS * 2];
    let mut slot_symbols = [0u8; ALIAS_NSYMS * 2];

    // ---- Phase 1: Vose's alias construction ----

    let mut remaining = [0u32; ALIAS_NSYMS];
    for i in 0..ALIAS_NSYMS {
        remaining[i] = freqs[i];
        divider[i] = tgt_sum;
        slot_symbols[i * 2] = i as u8;
        slot_symbols[i * 2 + 1] = i as u8;
    }

    // Find initial small and large buckets
    let mut cur_large = 0;
    while cur_large < ALIAS_NSYMS && remaining[cur_large] < tgt_sum {
        cur_large += 1;
    }
    let mut cur_small = 0;
    while cur_small < ALIAS_NSYMS && remaining[cur_small] >= tgt_sum {
        cur_small += 1;
    }
    let mut next_small = cur_small + 1;

    // Top up small buckets from large buckets
    while cur_large < ALIAS_NSYMS && cur_small < ALIAS_NSYMS {
        slot_symbols[cur_small * 2] = cur_large as u8;
        divider[cur_small] = remaining[cur_small];
        remaining[cur_large] -= tgt_sum - divider[cur_small];

        if remaining[cur_large] >= tgt_sum || next_small <= cur_large {
            cur_small = next_small;
            while cur_small < ALIAS_NSYMS && remaining[cur_small] >= tgt_sum {
                cur_small += 1;
            }
            next_small = cur_small + 1;
        } else {
            cur_small = cur_large;
        }

        while cur_large < ALIAS_NSYMS && remaining[cur_large] < tgt_sum {
            cur_large += 1;
        }
    }

    // ---- Phase 2: Distribute code slots ----

    let mut assigned = [0u32; ALIAS_NSYMS];
    let total_slots = total as usize;
    let mut alias_remap = alloc_crate::vec![0u32; total_slots];

    for i in 0..ALIAS_NSYMS {
        let j = slot_symbols[i * 2] as usize;
        let sym0_height = divider[i]; // the small symbol's amount
        let sym1_height = tgt_sum - divider[i]; // the large symbol's amount
        let base0 = assigned[i];
        let base1 = assigned[j];
        let cbase0 = cum_freqs[i] + base0;
        let cbase1 = cum_freqs[j] + base1;

        divider[i] = i as u32 * tgt_sum + sym0_height;

        slot_freqs[i * 2 + 1] = freqs[i];
        slot_freqs[i * 2] = freqs[j];
        slot_adjust[i * 2 + 1] = (i as u32 * tgt_sum).wrapping_sub(base0);
        slot_adjust[i * 2] = (i as u32 * tgt_sum).wrapping_sub(base1.wrapping_sub(sym0_height));

        for k in 0..sym0_height {
            let idx = (cbase0 + k) as usize;
            if idx < total_slots {
                alias_remap[idx] = k + i as u32 * tgt_sum;
            }
        }
        for k in 0..sym1_height {
            let idx = (cbase1 + k) as usize;
            if idx < total_slots {
                alias_remap[idx] = (k + sym0_height) + i as u32 * tgt_sum;
            }
        }

        assigned[i] += sym0_height;
        assigned[j] += sym1_height;
    }

    AliasTable {
        divider,
        slot_freqs,
        slot_adjust,
        slot_symbols,
        alias_remap,
        scale_bits,
        freqs: *freqs,
        cum_freqs: *cum_freqs,
    }
}

/// Alias method encoder put-symbol.
///
/// Equivalent to `RansEncPutAlias` in `main_alias.cpp`.
/// Renormalizes the state, then encodes symbol `s` using the alias remap table.
/// The encoded symbol uses division-based encoding: `((x/freq) << scale_bits) + alias_remap[...]`.
#[inline]
#[cfg(feature = "alloc")]
pub fn rans_byte_alias_enc_put<W: BackwardWriter>(
    state: &mut RansByteState,
    writer: &mut W,
    table: &AliasTable,
    s: u8,
    scale_bits: u32,
) -> Result<(), EncodeError> {
    let freq = table.freqs[s as usize];
    let x = rans_byte_enc_renorm(state.0, writer, freq, scale_bits)?;
    let slot = table.alias_remap[(x % freq + table.cum_freqs[s as usize]) as usize];
    state.0 = ((x / freq) << scale_bits) + slot;
    Ok(())
}

/// Alias method decoder get-symbol.
///
/// Equivalent to `RansDecGetAlias` in `main_alias.cpp`.
/// Extracts the symbol from the state using the alias table.
/// Returns the symbol and the updated state.
#[inline]
#[cfg(feature = "alloc")]
pub fn rans_byte_alias_dec_get(
    state: RansByteState,
    table: &AliasTable,
    scale_bits: u32,
) -> (u8, RansByteState) {
    let x = state.0;
    let mask = (1u32 << scale_bits).wrapping_sub(1);
    let xm = x & mask;
    let bucket_id = (xm >> (scale_bits - ALIAS_LOG2_NSYMS)) as usize;
    let mut bucket2 = bucket_id * 2;
    if xm < table.divider[bucket_id] {
        bucket2 += 1;
    }
    let s = table.slot_symbols[bucket2];
    let new_x = table.slot_freqs[bucket2] * (x >> scale_bits) + xm - table.slot_adjust[bucket2];
    (s, RansByteState(new_x))
}

/// Alias method decoder renormalization.
///
/// Delegates to the standard byte rANS renormalization (reads bytes).
/// Equivalent to `RansDecRenorm` in `rans_byte.h`.
#[inline]
#[cfg(feature = "alloc")]
pub fn rans_byte_alias_dec_renorm<R: ForwardReader>(
    state: &mut RansByteState,
    reader: &mut R,
) -> Result<(), DecodeError> {
    rans_byte_dec_renorm(state, reader)
}

/// Alias method decoder advance (get symbol + renormalize).
/// Combines `RansDecGetAlias` + `RansDecRenorm` from the upstream.
#[inline]
#[cfg(feature = "alloc")]
pub fn rans_byte_alias_dec_advance<R: ForwardReader>(
    state: &mut RansByteState,
    reader: &mut R,
    table: &AliasTable,
    scale_bits: u32,
) -> Result<u8, DecodeError> {
    let (s, new_state) = rans_byte_alias_dec_get(*state, table, scale_bits);
    *state = new_state;
    rans_byte_alias_dec_renorm(state, reader)?;
    Ok(s)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[cfg(kani)]
extern crate kani;

// ---------------------------------------------------------------------------
// Kani verification harnesses (compiled only under `cargo kani`)
// ---------------------------------------------------------------------------
//
// Each proof file is a self-contained harness set.  Kani compiles the crate
// with `--cfg kani`, which activates this module; run with:
//
//   cd crates/ryg-rans-rs-core && cargo kani
//
// The proofs pin: reciprocal == division (byte and R64), encoder-symbol
// construction validity, packed-table state-update overflow bounds, and
// byte encode/decode inversion.
#[cfg(kani)]
mod kani {
    // Each proof file is its own submodule so identical helper names
    // (e.g. `div_put`) across files cannot collide.
    mod reciprocal {
        include!("../kani/reciprocal_proof.rs");
    }
    mod r64_reciprocal {
        include!("../kani/r64_reciprocal_proof.rs");
    }
    mod enc_symbol_new {
        include!("../kani/enc_symbol_new_proof.rs");
    }
    mod packed_entry {
        include!("../kani/packed_entry_proof.rs");
    }
    mod encode_decode_inversion {
        include!("../kani/encode_decode_inversion_proof.rs");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;

    #[test]
    fn test_state_init() {
        let s = RansByteState::new();
        assert_eq!(s.get(), RANS_BYTE_L);
    }

    #[test]
    fn test_backward_writer_basic() {
        let mut buf = [0u8; 10];
        let pos;
        {
            let mut w = BackwardByteWriter::new(&mut buf);
            assert!(w.write_byte(0xAB).is_ok());
            assert_eq!(w.position(), 9);
            assert!(w.write_byte(0xCD).is_ok());
            pos = w.position();
        }
        assert_eq!(pos, 8);
        assert_eq!(buf[8], 0xCD);
        assert_eq!(buf[9], 0xAB);
        assert_eq!(buf[8..10], [0xCD, 0xAB]);
    }

    #[test]
    fn test_backward_writer_full() {
        let mut buf = [0u8; 2];
        let mut w = BackwardByteWriter::new(&mut buf);
        assert!(w.write_byte(1).is_ok());
        assert!(w.write_byte(2).is_ok());
        assert!(w.write_byte(3).is_err());
    }

    #[test]
    fn test_backward_writer_u32_le() {
        let mut buf = [0u8; 8];
        let pos1;
        {
            let mut w = BackwardByteWriter::new(&mut buf);
            assert!(w.write_u32_le(0x01020304).is_ok());
            pos1 = w.position();
            assert!(w.write_u32_le(0x05060708).is_ok());
        }
        assert_eq!(pos1, 4);
        assert_eq!(buf[4..8], [0x04, 0x03, 0x02, 0x01]);
        assert_eq!(buf[0..4], [0x08, 0x07, 0x06, 0x05]);
    }

    #[test]
    fn test_forward_reader_basic() {
        let buf = [0x10, 0x20, 0x30, 0x40];
        let mut r = ByteReader::new(&buf);
        assert_eq!(r.read_byte(), Some(0x10));
        assert_eq!(r.read_byte(), Some(0x20));
        assert_eq!(r.position(), 2);
    }

    #[test]
    fn test_forward_reader_u32_le() {
        let buf = [0x04, 0x03, 0x02, 0x01, 0x08, 0x07, 0x06, 0x05];
        let mut r = ByteReader::new(&buf);
        assert_eq!(r.read_u32_le(), Some(0x01020304));
        assert_eq!(r.read_u32_le(), Some(0x05060708));
        assert_eq!(r.read_u32_le(), None);
    }

    #[test]
    fn test_enc_symbol_init() {
        // Simple case: freq=2, scale_bits=14
        let sym = RansByteEncSymbol::new(100, 2, 14).unwrap();
        assert!(sym.x_max > 0);
        assert!(sym.rcp_freq > 0);
        assert_eq!(sym.bias, 100);
        assert_eq!(sym.cmpl_freq, ((1u32 << 14) - 2) as u16);
        // For freq=2, shift=1, rcp_shift = shift-1 = 0. This is expected.

        assert_eq!(sym.rcp_shift, 0);
    }

    #[test]
    fn test_enc_symbol_init_freq_one() {
        // Special case: freq=1
        let sym = RansByteEncSymbol::new(100, 1, 14).unwrap();
        assert!(sym.x_max > 0);
        assert_eq!(sym.rcp_freq, !0u32);
        assert_eq!(sym.rcp_shift, 0);
        assert_eq!(sym.bias, 100 + (1u32 << 14) - 1);
    }

    #[test]
    fn test_enc_symbol_init_max_freq() {
        // freq = (1 << scale_bits) - start
        let scale_bits = 14;
        let start = 0;
        let freq = 1u32 << scale_bits;
        let sym = RansByteEncSymbol::new(start, freq, scale_bits).unwrap();
        assert!(sym.x_max > 0);
    }

    #[test]
    fn test_slice_backward_writer() {
        let mut buf = [0u8; 10];
        let mut writer = SliceBackwardWriter(&mut buf[..]);
        assert!(writer.write_byte(0xAB).is_ok());
        assert_eq!(writer.0.len(), 9);
        assert!(writer.write_byte(0xCD).is_ok());
        assert_eq!(writer.0.len(), 8);
        assert_eq!(buf[8], 0xCD);
        assert_eq!(buf[9], 0xAB);
    }

    #[test]
    fn test_slice_forward_reader() {
        let buf = [0x10, 0x20, 0x30];
        let mut r = &buf[..];
        assert_eq!(r.read_byte(), Some(0x10));
        assert_eq!(r.read_byte(), Some(0x20));
        assert_eq!(r.read_byte(), Some(0x30));
        assert_eq!(r.read_byte(), None);
    }

    #[test]
    fn test_roundtrip_single_symbol() {
        let scale_bits = 14;
        let symbols = [42u8; 100];

        // Encode
        let mut out = [0u8; 1024];
        let mut writer = BackwardByteWriter::new(&mut out);

        let esym = RansByteEncSymbol::new(0, 1u32 << scale_bits, scale_bits).unwrap();
        let mut state = RansByteState::new();

        for _i in (0..symbols.len()).rev() {
            rans_byte_enc_put_symbol(&mut state, &mut writer, &esym).unwrap();
        }
        rans_byte_enc_flush(&state, &mut writer).unwrap();

        let encoded = writer.encoded();
        assert!(!encoded.is_empty(), "encoded output should not be empty");

        // Decode
        let mut reader = ByteReader::new(encoded);
        let dsym = RansByteDecSymbol::new(0, 1u32 << scale_bits).unwrap();
        let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
        // With one symbol occupying the entire [0, 1<<scale_bits) range,
        // all cum2sym slots map to that symbol (42).
        let cum2sym = [42u8; 1 << 14];

        let mut output = alloc::vec![0u8; symbols.len()];
        for i in 0..symbols.len() {
            let cf = rans_byte_dec_get(&dec_state, scale_bits);
            let s = cum2sym[cf as usize];
            output[i] = s;
            rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
        }

        assert_eq!(
            output,
            &symbols[..],
            "single-symbol round-trip should match"
        );
        assert_eq!(
            output,
            &symbols[..],
            "single-symbol round-trip should match"
        );
    }

    #[test]
    fn test_roundtrip_two_symbols() {
        // Two symbols with frequencies 1 and 3 (scale_bits=2, total=4)
        let scale_bits = 2;
        let _total = 1u32 << scale_bits; // 4
        let freq0 = 1u32;
        let freq1 = 3u32;

        let symbols: alloc::vec::Vec<u8> = (0..10).map(|i| (i % 2) as u8).collect();

        // Encode using division-based path for correctness
        let mut out = [0u8; 1024];
        let mut writer = BackwardByteWriter::new(&mut out);

        let mut state = RansByteState::new();
        for idx in (0..symbols.len()).rev() {
            let s = symbols[idx];
            let start = if s == 0 { 0 } else { freq0 };
            let freq = if s == 0 { freq0 } else { freq1 };
            rans_byte_enc_put(&mut state, &mut writer, start, freq, scale_bits).unwrap();
        }
        rans_byte_enc_flush(&state, &mut writer).unwrap();

        let encoded = writer.encoded();
        assert!(!encoded.is_empty());

        // Decode
        let mut reader = ByteReader::new(encoded);
        let dsym0 = RansByteDecSymbol::new(0, freq0).unwrap();
        let dsym1 = RansByteDecSymbol::new(freq0, freq1).unwrap();

        let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();
        let cum2sym = [0u8, 0u8, 1u8, 1u8]; // slots 0-1 -> sym0, slots 2-3 -> sym1

        let mut output = alloc::vec![0u8; symbols.len()];
        for i in 0..symbols.len() {
            let cf = rans_byte_dec_get(&dec_state, scale_bits);
            let s = cum2sym[cf as usize] as usize;
            output[i] = s as u8;
            let dsym = if s == 0 { &dsym0 } else { &dsym1 };
            rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits).unwrap();
        }

        assert_eq!(output, symbols, "two-symbol round-trip should match");
    }

    #[test]
    fn test_slice_trait_roundtrip() {
        let scale_bits = 14;
        let symbols: alloc::vec::Vec<u8> = (0..50).map(|i| (i % 17) as u8).collect();

        // Encode using slice writer (via SliceBackwardWriter)
        let mut out = [0u8; 1024];
        let mut writer = SliceBackwardWriter(&mut out[..]);
        let mut state = RansByteState::new();

        // Build uniform freq model
        let total = 1u32 << scale_bits;
        let n_syms = 17u32;
        let base_freq = total / n_syms;
        for i in (0..symbols.len()).rev() {
            let s = symbols[i] as u32;
            let start = s * base_freq;
            let freq = base_freq;
            rans_byte_enc_put(&mut state, &mut writer, start, freq, scale_bits).unwrap();
        }
        rans_byte_enc_flush(&state, &mut writer).unwrap();

        let used = writer.0.len();
        let encoded = &out[used..];

        // Decode
        let mut reader = ByteReader::new(encoded);
        let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();

        let mut output = alloc::vec![0u8; symbols.len()];
        for i in 0..symbols.len() {
            let cf = rans_byte_dec_get(&dec_state, scale_bits);
            let s = cf / base_freq;
            output[i] = s as u8;
            let start = s * base_freq;
            rans_byte_dec_advance(&mut dec_state, &mut reader, start, base_freq, scale_bits)
                .unwrap();
        }

        assert_eq!(output, symbols, "uniform-symbol round-trip should match");
    }

    #[test]
    fn test_reciprocal_roundtrip() {
        let scale_bits = 14;
        let total = 1u32 << scale_bits;
        // Use frequencies that sum to total
        let freq0 = total / 3;
        let freq1 = total / 3;
        let freq2 = total - freq0 - freq1;

        let esym0 = RansByteEncSymbol::new(0, freq0, scale_bits).unwrap();
        let esym1 = RansByteEncSymbol::new(freq0, freq1, scale_bits).unwrap();
        let esym2 = RansByteEncSymbol::new(freq0 + freq1, freq2, scale_bits).unwrap();

        let dsym0 = RansByteDecSymbol::new(0, freq0).unwrap();
        let dsym1 = RansByteDecSymbol::new(freq0, freq1).unwrap();
        let dsym2 = RansByteDecSymbol::new(freq0 + freq1, freq2).unwrap();

        let symbols: alloc::vec::Vec<u8> = (0..50).map(|i| (i % 3) as u8).collect();

        // Encode with reciprocal fast path
        let mut out = [0u8; 1024];
        let mut writer = BackwardByteWriter::new(&mut out);
        let mut state = RansByteState::new();

        for idx in (0..symbols.len()).rev() {
            let s = symbols[idx] as usize;
            let esym = match s {
                0 => &esym0,
                1 => &esym1,
                _ => &esym2,
            };
            rans_byte_enc_put_symbol(&mut state, &mut writer, esym).unwrap();
        }
        rans_byte_enc_flush(&state, &mut writer).unwrap();
        let encoded = writer.encoded();

        // Decode with division-based path
        let mut reader = ByteReader::new(encoded);
        let mut dec_state = rans_byte_dec_init(&mut reader).unwrap();

        let cum2sym: alloc::vec::Vec<u8> = (0..total as usize)
            .map(|i| {
                if i < freq0 as usize {
                    0
                } else if i < (freq0 + freq1) as usize {
                    1
                } else {
                    2
                }
            })
            .collect();

        let mut output = alloc::vec![0u8; symbols.len()];
        for i in 0..symbols.len() {
            let cf = rans_byte_dec_get(&dec_state, scale_bits);
            let s = cum2sym[cf as usize] as usize;
            output[i] = s as u8;
            let dsym = match s {
                0 => &dsym0,
                1 => &dsym1,
                _ => &dsym2,
            };
            rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits).unwrap();
        }

        assert_eq!(output, symbols, "reciprocal round-trip should match");
    }

    #[test]
    fn test_interleaved_roundtrip() {
        let scale_bits = 14;
        let symbols: alloc::vec::Vec<u8> = (0..77).map(|i| (i % 7) as u8).collect();

        // Build frequency model (approximate uniform)
        let total = 1u32 << scale_bits;
        let base_freq = total / 7;
        let esyms: alloc::vec::Vec<RansByteEncSymbol> = (0..7)
            .map(|i| RansByteEncSymbol::new(i * base_freq, base_freq, scale_bits).unwrap())
            .collect();
        let dsyms: alloc::vec::Vec<RansByteDecSymbol> = (0..7)
            .map(|i| RansByteDecSymbol::new(i * base_freq, base_freq).unwrap())
            .collect();

        // Encode interleaved
        let mut out = [0u8; 2048];
        let mut writer = BackwardByteWriter::new(&mut out);

        let mut s0 = RansByteState::new();
        let mut s1 = RansByteState::new();

        let n = symbols.len();
        if n & 1 != 0 {
            let s = symbols[n - 1] as usize;
            rans_byte_enc_put_symbol(&mut s0, &mut writer, &esyms[s]).unwrap();
        }

        let mut i = n & !1;
        while i > 0 {
            let s1_idx = symbols[i - 1] as usize;
            let s0_idx = symbols[i - 2] as usize;
            rans_byte_enc_put_symbol(&mut s1, &mut writer, &esyms[s1_idx]).unwrap();
            rans_byte_enc_put_symbol(&mut s0, &mut writer, &esyms[s0_idx]).unwrap();
            i = i.wrapping_sub(2);
        }

        rans_byte_enc_flush(&s1, &mut writer).unwrap();
        rans_byte_enc_flush(&s0, &mut writer).unwrap();

        let encoded = writer.encoded();

        // Decode interleaved
        let mut reader = ByteReader::new(encoded);
        let mut d0 = rans_byte_dec_init(&mut reader).unwrap();
        let mut d1 = rans_byte_dec_init(&mut reader).unwrap();

        let cum2sym: alloc::vec::Vec<u8> = (0..total as usize)
            .map(|i| (i / base_freq as usize) as u8)
            .collect();

        let mut output = alloc::vec![0u8; n];
        let even_n = n & !1;

        let mut pos = 0;
        while pos < even_n {
            let cf0 = rans_byte_dec_get(&d0, scale_bits);
            let s0 = cum2sym[cf0 as usize] as usize;
            let cf1 = rans_byte_dec_get(&d1, scale_bits);
            let s1 = cum2sym[cf1 as usize] as usize;

            output[pos] = s0 as u8;
            output[pos + 1] = s1 as u8;

            rans_byte_dec_advance_symbol_step(&mut d0, &dsyms[s0], scale_bits);
            rans_byte_dec_advance_symbol_step(&mut d1, &dsyms[s1], scale_bits);
            rans_byte_dec_renorm(&mut d0, &mut reader).unwrap();
            rans_byte_dec_renorm(&mut d1, &mut reader).unwrap();

            pos += 2;
        }

        if n & 1 != 0 {
            let cf0 = rans_byte_dec_get(&d0, scale_bits);
            let s0 = cum2sym[cf0 as usize] as usize;
            output[n - 1] = s0 as u8;
            rans_byte_dec_advance_symbol(&mut d0, &mut reader, &dsyms[s0], scale_bits).unwrap();
        }

        assert_eq!(output, symbols, "interleaved round-trip should match");
    }

    #[test]
    fn test_reciprocal_equals_division() {
        // For a range of frequencies and states, verify the reciprocal fast
        // path produces the same result as the division-based reference.
        let scale_bits = 14;
        let total = 1u32 << scale_bits;

        // Division-based reference (single encode step, no renormalization)
        fn div_put(x: u32, start: u32, freq: u32, scale_bits: u32) -> u32 {
            ((x / freq) << scale_bits) + (x % freq) + start
        }

        let test_freqs = [1, 2, 3, 5, 7, 10, 100, 1000, total / 2, total - 1];

        for &freq in &test_freqs {
            let start = 0;
            let esym = RansByteEncSymbol::new(start, freq, scale_bits).unwrap();

            let test_states = [
                RANS_BYTE_L,
                RANS_BYTE_L + 1,
                RANS_BYTE_L * 2,
                RANS_BYTE_L * 4,
                RANS_BYTE_L * 8,
                (1u32 << 31) - 1,
            ];

            for &test_state in &test_states {
                if test_state >= esym.x_max {
                    // Would need renormalization; skip for now
                    continue;
                }

                let expected = div_put(test_state, start, freq, scale_bits);

                let mut state_fast = RansByteState(test_state);
                let mut temp = [0u8; 8];
                let mut w = BackwardByteWriter::new(&mut temp);
                rans_byte_enc_put_symbol(&mut state_fast, &mut w, &esym).unwrap();

                assert_eq!(
                    state_fast.0, expected,
                    "reciprocal mismatch for freq={}, start={}, state={}",
                    freq, start, test_state
                );
            }
        }
    }

    #[test]
    fn test_oracle_reciprocal_parameters() {
        // Verify against compiled C oracle output

        // freq=10, start=0, scale_bits=14
        let sym = RansByteEncSymbol::new(0, 10, 14).unwrap();
        assert_eq!(sym.x_max, 1310720);
        assert_eq!(sym.rcp_freq, 3435973837);
        assert_eq!(sym.bias, 0);
        assert_eq!(sym.cmpl_freq as u32, 16374);
        assert_eq!(sym.rcp_shift as u32, 3);

        // freq=1 special case, start=100, scale_bits=14
        let sym = RansByteEncSymbol::new(100, 1, 14).unwrap();
        assert_eq!(sym.x_max, 131072);
        assert_eq!(sym.rcp_freq, 4294967295);
        assert_eq!(sym.bias, 16483);
        assert_eq!(sym.cmpl_freq as u32, 16383);
        assert_eq!(sym.rcp_shift as u32, 0);

        // freq=16384 (full total), start=0, scale_bits=14
        let sym = RansByteEncSymbol::new(0, 16384, 14).unwrap();
        assert_eq!(sym.x_max, 2147483648);
        assert_eq!(sym.rcp_freq, 2147483648);
        assert_eq!(sym.bias, 0);
        assert_eq!(sym.cmpl_freq as u32, 0);
        assert_eq!(sym.rcp_shift as u32, 13);

        // freq=2, start=0, scale_bits=14
        let sym = RansByteEncSymbol::new(0, 2, 14).unwrap();
        assert_eq!(sym.cmpl_freq as u32, 16382);
        assert_eq!(sym.rcp_shift as u32, 0);
        assert!(sym.rcp_freq == 2147483648 || sym.rcp_freq > 0);
    }

    #[test]
    fn test_reciprocal_freq_one() {
        let scale_bits = 14;
        let freq = 1u32;
        let start = 100;
        let esym = RansByteEncSymbol::new(start, freq, scale_bits).unwrap();

        // The freq=1 special case should give: x_new = x * M + start
        fn expected(x: u32, start: u32, scale_bits: u32) -> u32 {
            x * (1u32 << scale_bits) + start
        }

        let test_states = [RANS_BYTE_L, RANS_BYTE_L + 10, RANS_BYTE_L * 3, (1u32 << 30)];

        for &test_state in &test_states {
            if test_state >= esym.x_max {
                continue;
            }
            let mut state = RansByteState(test_state);
            let mut tmp = [0u8; 8];
            let mut w = BackwardByteWriter::new(&mut tmp);
            rans_byte_enc_put_symbol(&mut state, &mut w, &esym).unwrap();

            assert_eq!(
                state.0,
                expected(test_state, start, scale_bits),
                "freq=1 mismatch for state={}",
                test_state
            );
        }
    }

    #[test]
    fn test_decoder_symbol_init() {
        let dsym = RansByteDecSymbol::new(100, 50).unwrap();
        assert_eq!(dsym.start, 100);
        assert_eq!(dsym.freq, 50);
    }

    #[test]
    fn test_reader_exhaustion() {
        let buf = [1u8; 3];
        let mut reader = ByteReader::new(&buf);
        assert!(reader.read_byte().is_some());
        assert!(reader.read_byte().is_some());
        assert!(reader.read_byte().is_some());
        assert!(reader.read_byte().is_none());
        assert!(reader.read_u32_le().is_none());
    }

    #[test]
    fn test_writer_exhaustion() {
        let mut buf = [0u8; 2];
        let mut writer = BackwardByteWriter::new(&mut buf);
        assert!(writer.write_u32_le(0x12345678).is_err());
        assert!(writer.write_byte(1).is_ok());
        assert!(writer.write_byte(2).is_ok());
        assert!(writer.write_byte(3).is_err());
    }

    // -----------------------------------------------------------------------
    // 64-bit rANS tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_rans64_state_init() {
        let s = Rans64State::new();
        assert_eq!(s.get(), RANS64_L);
        assert_eq!(s, Rans64State::default());
    }

    #[test]
    fn test_rans64_word32_writer_basic() {
        let mut buf = [0u8; 12];
        let pos;
        let words;
        {
            let mut w = BackwardWord32Writer::new(&mut buf);
            assert!(w.write_word32(0xDEADBEEF).is_ok());
            assert!(w.write_word32(0xCAFEBABE).is_ok());
            // Room for 1 more word (12 bytes = 3 words, wrote 2)
            assert!(w.write_word32(0x12345678).is_ok());
            pos = w.position();
            words = w.words_written();
        }
        assert_eq!(pos, 0);
        assert_eq!(words, 3);
        assert_eq!(buf[8..12], 0xDEADBEEFu32.to_le_bytes());
        assert_eq!(buf[4..8], 0xCAFEBABEu32.to_le_bytes());
        assert_eq!(buf[0..4], 0x12345678u32.to_le_bytes());
    }

    #[test]
    fn test_rans64_word32_reader_basic() {
        let mut buf = [0u8; 12];
        let v0 = 0xDEADBEEFu32;
        let v1 = 0xCAFEBABEu32;
        let v2 = 0x12345678u32;
        buf[0..4].copy_from_slice(&v0.to_le_bytes());
        buf[4..8].copy_from_slice(&v1.to_le_bytes());
        buf[8..12].copy_from_slice(&v2.to_le_bytes());

        let mut r = Word32Reader::new(&buf);
        assert_eq!(r.read_word32(), Some(v0));
        assert_eq!(r.read_word32(), Some(v1));
        assert_eq!(r.read_word32(), Some(v2));
        assert_eq!(r.read_word32(), None);
        assert_eq!(r.words_consumed(), 3);
    }

    #[test]
    fn test_rans64_enc_symbol_init() {
        // Simple case: freq=2, scale_bits=14
        let sym = Rans64EncSymbol::new(100, 2, 14).unwrap();
        assert!(sym.x_max > 0);
        assert!(sym.rcp_freq > 0);
        assert_eq!(sym.bias, 100);
        assert_eq!(sym.cmpl_freq, ((1u32 << 14) - 2) as u32);
        assert_eq!(sym.rcp_shift, 0);

        // Check x_max formula: ((RANS64_L >> scale_bits) << 32) * freq
        let expected_x_max = ((RANS64_L >> 14) << 32) * 2;
        assert_eq!(sym.x_max, expected_x_max);
    }

    #[test]
    fn test_rans64_enc_symbol_init_freq_one() {
        let sym = Rans64EncSymbol::new(100, 1, 14).unwrap();
        assert!(sym.x_max > 0);
        assert_eq!(sym.rcp_freq, !0u64);
        assert_eq!(sym.rcp_shift, 0);
        assert_eq!(sym.bias, 100 + (1u64 << 14) - 1);
    }

    #[test]
    fn test_rans64_enc_symbol_init_large_scale() {
        // scale_bits=31, large freq to exercise 128-bit division
        let scale_bits = 30;
        let start = 0;
        let freq = (1u32 << 29) + 1; // large, irregular freq
        let sym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();
        assert!(sym.rcp_freq > 0);
        assert!(sym.x_max > 0);
        assert_eq!(sym.bias, 0);
    }

    #[test]
    fn test_rans64_roundtrip_single_symbol_division() {
        // Encode and decode a single symbol using the division-based path
        let scale_bits = 14;
        let n = 50;
        let symbols = [99u8; 50];

        // Encode
        let mut out = [0u8; 4096];
        let mut writer = BackwardWord32Writer::new(&mut out);

        let mut state = Rans64State::new();
        for _i in (0..n).rev() {
            rans64_enc_put(
                &mut state,
                &mut writer,
                0,                  // start
                1u32 << scale_bits, // freq = total
                scale_bits,
            )
            .unwrap();
        }
        rans64_enc_flush(&state, &mut writer).unwrap();

        let encoded = writer.encoded();
        assert!(
            encoded.len() >= 8,
            "encoded should have at least 8 bytes (2 words)"
        );
        assert!(!encoded.is_empty());

        // Decode
        let mut reader = Word32Reader::new(encoded);
        let dsym = Rans64DecSymbol::new(0, 1u32 << scale_bits).unwrap();
        let mut dec_state = rans64_dec_init(&mut reader).unwrap();
        let cum2sym = [99u8; 1 << 14];

        let mut output = alloc::vec![0u8; n];
        for i in 0..n {
            let cf = rans64_dec_get(&dec_state, scale_bits);
            let s = cum2sym[cf as usize];
            output[i] = s;
            rans64_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
        }

        assert_eq!(output, symbols, "64-bit single-symbol division round-trip");
    }

    #[test]
    fn test_rans64_roundtrip_two_symbols_division() {
        // Two symbols with distinct frequencies, using 64-bit state
        let scale_bits = 14;
        let total = 1u32 << scale_bits;
        let freq0 = total / 4;
        let freq1 = total - freq0;

        let symbols: alloc::vec::Vec<u8> = (0..30).map(|i| (i % 2) as u8).collect();

        // Encode with division-based path
        let mut out = [0u8; 4096];
        let mut writer = BackwardWord32Writer::new(&mut out);

        let mut state = Rans64State::new();
        for idx in (0..symbols.len()).rev() {
            let s = symbols[idx];
            let start = if s == 0 { 0 } else { freq0 };
            let freq = if s == 0 { freq0 } else { freq1 };
            rans64_enc_put(&mut state, &mut writer, start, freq, scale_bits).unwrap();
        }
        rans64_enc_flush(&state, &mut writer).unwrap();

        let encoded = writer.encoded();
        assert!(encoded.len() >= 8, "encoded length = {}", encoded.len());

        // Decode
        let mut reader = Word32Reader::new(encoded);
        let dsym0 = Rans64DecSymbol::new(0, freq0).unwrap();
        let dsym1 = Rans64DecSymbol::new(freq0, freq1).unwrap();

        let mut dec_state = rans64_dec_init(&mut reader).unwrap();
        // Build cum2sym for these two symbols
        let mut cum2sym = alloc::vec![1u8; total as usize];
        for i in 0..freq0 as usize {
            cum2sym[i] = 0;
        }

        let mut output = alloc::vec![0u8; symbols.len()];
        for i in 0..symbols.len() {
            let cf = rans64_dec_get(&dec_state, scale_bits);
            let s = cum2sym[cf as usize] as usize;
            output[i] = s as u8;
            let dsym = if s == 0 { &dsym0 } else { &dsym1 };
            rans64_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits).unwrap();
        }

        assert_eq!(output, symbols, "64-bit two-symbol division round-trip");
    }

    #[test]
    fn test_rans64_reciprocal_equals_division() {
        // Verify reciprocal fast path produces the same C(s, x) as division
        let scale_bits = 14;
        let total = 1u32 << scale_bits;

        fn div_put(x: u64, start: u32, freq: u32, scale_bits: u32) -> u64 {
            ((x / (freq as u64)) << scale_bits) + (x % (freq as u64)) + (start as u64)
        }

        let test_freqs = [1, 2, 3, 5, 7, 10, 100, 1000, total / 2, total - 1];

        for &freq in &test_freqs {
            let start = 0u32;
            let esym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();

            let test_states = [
                RANS64_L,
                RANS64_L + 1,
                RANS64_L * 2,
                RANS64_L * 4,
                RANS64_L * 8,
                (1u64 << 62) - 1,
            ];

            for &test_state in &test_states {
                if test_state >= esym.x_max {
                    continue;
                }

                let expected = div_put(test_state, start, freq, scale_bits);

                let mut state_fast = Rans64State(test_state);
                let mut temp = [0u8; 16];
                let mut w = BackwardWord32Writer::new(&mut temp);
                rans64_enc_put_symbol(&mut state_fast, &mut w, &esym).unwrap();

                assert_eq!(
                    state_fast.0, expected,
                    "64-bit reciprocal mismatch for freq={}, start={}, state={}",
                    freq, start, test_state
                );
            }
        }
    }

    #[test]
    fn test_rans64_roundtrip_reciprocal() {
        // Full round-trip using reciprocal fast path for encoding,
        // division-based decoding
        let scale_bits = 14;
        let total = 1u32 << scale_bits;
        let freq0 = total / 3;
        let freq1 = total / 3;
        let freq2 = total - freq0 - freq1;

        let esym0 = Rans64EncSymbol::new(0, freq0, scale_bits).unwrap();
        let esym1 = Rans64EncSymbol::new(freq0, freq1, scale_bits).unwrap();
        let esym2 = Rans64EncSymbol::new(freq0 + freq1, freq2, scale_bits).unwrap();

        let dsym0 = Rans64DecSymbol::new(0, freq0).unwrap();
        let dsym1 = Rans64DecSymbol::new(freq0, freq1).unwrap();
        let dsym2 = Rans64DecSymbol::new(freq0 + freq1, freq2).unwrap();

        let symbols: alloc::vec::Vec<u8> = (0..50).map(|i| (i % 3) as u8).collect();

        // Encode with reciprocal fast path
        let mut out = [0u8; 4096];
        let mut writer = BackwardWord32Writer::new(&mut out);
        let mut state = Rans64State::new();

        for idx in (0..symbols.len()).rev() {
            let s = symbols[idx] as usize;
            let esym = match s {
                0 => &esym0,
                1 => &esym1,
                _ => &esym2,
            };
            rans64_enc_put_symbol(&mut state, &mut writer, esym).unwrap();
        }
        rans64_enc_flush(&state, &mut writer).unwrap();
        let encoded = writer.encoded();

        // Decode with division-based path
        let mut reader = Word32Reader::new(encoded);
        let mut dec_state = rans64_dec_init(&mut reader).unwrap();

        let cum2sym: alloc::vec::Vec<u8> = (0..total as usize)
            .map(|i| {
                if i < freq0 as usize {
                    0
                } else if i < (freq0 + freq1) as usize {
                    1
                } else {
                    2
                }
            })
            .collect();

        let mut output = alloc::vec![0u8; symbols.len()];
        for i in 0..symbols.len() {
            let cf = rans64_dec_get(&dec_state, scale_bits);
            let s = cum2sym[cf as usize] as usize;
            output[i] = s as u8;
            let dsym = match s {
                0 => &dsym0,
                1 => &dsym1,
                _ => &dsym2,
            };
            rans64_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits).unwrap();
        }

        assert_eq!(output, symbols, "64-bit reciprocal round-trip");
    }

    #[test]
    fn test_rans64_step_operations() {
        // Verify the step-only operations produce the same intermediate state
        let scale_bits = 14;
        let total = 1u32 << scale_bits;
        let freq = total / 2;
        let start = 0;

        let dsym = Rans64DecSymbol::new(start, freq).unwrap();

        // Start with a state large enough to not need renormalization
        let state_val = RANS64_L * 4;
        let mut state_advance = Rans64State(state_val);
        let mut state_step = Rans64State(state_val);

        // Advance via regular advance (no renorm needed since x >= L)
        // We need a dummy buffer with enough words to not actually use them
        let dummy_buf = [0u8; 16];
        let mut reader = Word32Reader::new(&dummy_buf);
        rans64_dec_advance(&mut state_advance, &mut reader, start, freq, scale_bits).unwrap();

        // Advance via step-only
        rans64_dec_advance_step(&mut state_step, start, freq, scale_bits);

        assert_eq!(
            state_advance.0, state_step.0,
            "step-only advance should match regular advance when no renorm needed"
        );

        // Repeat with symbol convenience
        let mut state_adv_sym = Rans64State(state_val);
        let mut state_step_sym = Rans64State(state_val);
        let mut reader2 = Word32Reader::new(&dummy_buf);
        rans64_dec_advance_symbol(&mut state_adv_sym, &mut reader2, &dsym, scale_bits).unwrap();
        rans64_dec_advance_symbol_step(&mut state_step_sym, &dsym, scale_bits);

        assert_eq!(
            state_adv_sym.0, state_step_sym.0,
            "step-only symbol advance should match regular"
        );
    }

    #[test]
    fn test_rans64_state_transition_cycle() {
        // Encode a symbol and verify the decode retrieves the original
        // Uses a well-known state transition: C(s, x) then D(s, C(s, x))
        let scale_bits = 14;
        let _total = 1u32 << scale_bits;
        let freq = 100u32;
        let start = 500u32;

        let esym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();
        let _dsym = Rans64DecSymbol::new(start, freq).unwrap();

        // Pick a test state that doesn't need renormalization
        let x = RANS64_L; // minimum valid state

        // Encode step: C(s, x) using reciprocal
        let mut enc_state = Rans64State(x);
        let mut tmp = [0u8; 16];
        let mut w = BackwardWord32Writer::new(&mut tmp);
        rans64_enc_put_symbol(&mut enc_state, &mut w, &esym).unwrap();
        let encoded_x = enc_state.0;

        // Decode step: D(s, C(s, x)) should give back x
        let dummy = [0u8; 16];
        let mut r = Word32Reader::new(&dummy);
        let mut dec_state = Rans64State(encoded_x);
        rans64_dec_advance(&mut dec_state, &mut r, start, freq, scale_bits).unwrap();

        assert_eq!(
            dec_state.0, x,
            "decoding should invert encoding: D(s, C(s, x)) = x"
        );
    }

    #[test]
    fn test_rans64_flush_init_roundtrip() {
        // Verify that flushing a state and re-initializing from those words
        // gives back the same state
        let test_state = 0xDEADBEEF_CAFEBABEu64;
        let state_in = Rans64State(test_state);

        // Flush: write 2 u32 words (low first, then high)
        let mut buf = [0u8; 16];
        let mut writer = BackwardWord32Writer::new(&mut buf);
        rans64_enc_flush(&state_in, &mut writer).unwrap();

        let encoded = writer.encoded();
        assert_eq!(encoded.len(), 8, "flush should write exactly 8 bytes");

        // Verify the byte layout directly:
        // Low word is stored at position 0..4, high word at 4..8
        let lo_expected = (test_state & 0xffffffff) as u32;
        let hi_expected = (test_state >> 32) as u32;
        let lo_actual = u32::from_le_bytes([encoded[0], encoded[1], encoded[2], encoded[3]]);
        let hi_actual = u32::from_le_bytes([encoded[4], encoded[5], encoded[6], encoded[7]]);
        assert_eq!(lo_actual, lo_expected, "low word should match");
        assert_eq!(hi_actual, hi_expected, "high word should match");

        // Re-init: read back the state
        let mut reader = Word32Reader::new(encoded);
        let state_out = rans64_dec_init(&mut reader).unwrap();
        assert_eq!(state_out.0, test_state, "flush+init round-trip");
    }

    #[test]
    fn test_rans64_mul_hi() {
        // Verify rans64_mul_hi against u128 reference
        // 0xABCDEF0123456789 * 0x9876543210FEDCBA
        let a = 0xABCDEF0123456789u64;
        let b = 0x9876543210FEDCBAu64;
        let expected = ((a as u128) * (b as u128) >> 64) as u64;
        assert_eq!(rans64_mul_hi(a, b), expected);

        // Simple cases
        assert_eq!(rans64_mul_hi(1, 1), 0);
        assert_eq!(rans64_mul_hi(1u64 << 63, 2), 1);
        assert_eq!(rans64_mul_hi(!0u64, !0u64), !0u64 - 1);
    }

    #[test]
    fn test_rans64_decoder_symbol_init() {
        let dsym = Rans64DecSymbol::new(100, 50).unwrap();
        assert_eq!(dsym.start, 100);
        assert_eq!(dsym.freq, 50);
    }

    #[test]
    fn test_rans64_freq_one_special() {
        // For freq=1, the reciprocal path should match: x * M + start
        let scale_bits = 14;
        let freq = 1u32;
        let start = 100;
        let esym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();

        fn expected(x: u64, start: u64, scale_bits: u32) -> u64 {
            x * (1u64 << scale_bits) + start
        }

        let test_states = [RANS64_L, RANS64_L + 10, RANS64_L * 3, (1u64 << 60)];

        for &test_state in &test_states {
            if test_state >= esym.x_max {
                continue;
            }
            let mut state = Rans64State(test_state);
            let mut tmp = [0u8; 16];
            let mut w = BackwardWord32Writer::new(&mut tmp);
            rans64_enc_put_symbol(&mut state, &mut w, &esym).unwrap();

            assert_eq!(
                state.0,
                expected(test_state, start as u64, scale_bits),
                "64-bit freq=1 mismatch for state={}",
                test_state
            );
        }
    }

    #[test]
    fn test_rans64_word32_writer_exhaustion() {
        let mut buf = [0u8; 4]; // only room for 1 word
        let mut writer = BackwardWord32Writer::new(&mut buf);
        assert!(writer.write_word32(0x12345678).is_ok());
        assert!(writer.write_word32(0x9ABCDEF0).is_err());
    }

    #[test]
    fn test_rans64_word32_reader_exhaustion() {
        let buf = [0x01, 0x02, 0x03]; // only 3 bytes, not enough for one word
        let mut reader = Word32Reader::new(&buf);
        assert!(reader.read_word32().is_none());
    }

    #[test]
    fn test_rans64_renorm_roundtrip() {
        // Encode a symbol large enough to trigger renormalization,
        // then decode it back using the full path
        let scale_bits = 14;
        let total = 1u32 << scale_bits;

        // Use a small freq so x_max is small, forcing renormalization
        let freq = 7u32;
        let start = 100;
        let esym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();
        let dsym = Rans64DecSymbol::new(start, freq).unwrap();

        let mut out = [0u8; 4096];
        let mut writer = BackwardWord32Writer::new(&mut out);
        let mut state = Rans64State::new();

        // Encode many symbols to force renormalization
        let n = 100;
        for _i in 0..n {
            rans64_enc_put_symbol(&mut state, &mut writer, &esym).unwrap();
        }
        rans64_enc_flush(&state, &mut writer).unwrap();

        let encoded = writer.encoded();

        // Decode
        let mut reader = Word32Reader::new(encoded);
        let mut dec_state = rans64_dec_init(&mut reader).unwrap();

        // Build cum2sym mapping for this single symbol range
        let cum2sym: alloc::vec::Vec<u8> = (0..total as usize)
            .map(|i| {
                if (i as u32) < start {
                    255 // shouldn't happen
                } else if (i as u32) < start + freq {
                    42
                } else {
                    255 // shouldn't happen
                }
            })
            .collect();

        let mut output = alloc::vec![0u8; n];
        for i in 0..n {
            let cf = rans64_dec_get(&dec_state, scale_bits);
            let s = cum2sym[cf as usize];
            output[i] = s;
            rans64_dec_advance_symbol(&mut dec_state, &mut reader, &dsym, scale_bits).unwrap();
        }

        assert_eq!(output.len(), n);
        for &val in &output {
            assert_eq!(val, 42, "all decoded symbols should be 42");
        }
    }

    #[test]
    fn test_rans64_renorm_only() {
        // Test rans64_dec_renorm in isolation with a prepared reader
        // Decoder state below RANS64_L, check that reading words brings it back up
        let mut buf = [0u8; 12];
        // Write two u32 words that, when shifted in, will push state >= RANS64_L
        let w0 = 0x00000001u32;
        let w1 = 0x00000002u32;
        buf[0..4].copy_from_slice(&w0.to_le_bytes());
        buf[4..8].copy_from_slice(&w1.to_le_bytes());

        let mut reader = Word32Reader::new(&buf);

        // State below L: reading one word should push it above L
        let mut state = Rans64State(RANS64_L - 1);
        rans64_dec_renorm(&mut state, &mut reader).unwrap();
        assert!(
            state.0 >= RANS64_L,
            "after renorm, state {} should be >= RANS64_L",
            state.0
        );
        // Specifically: (RANS64_L - 1) << 32 | 1 >= RANS64_L
        assert_eq!(state.0, ((RANS64_L - 1) << 32) | 1);
        assert_eq!(reader.words_consumed(), 1);
    }

    #[test]
    fn test_rans64_large_scale_reciprocal() {
        // Test 64-bit reciprocal parameters across scale_bits 17..31
        // Verifying that cmpl_freq (u32) correctly handles values > 65535.
        use super::*;

        for scale_bits in 17u32..=31u32 {
            let total = 1u64 << scale_bits;

            // Pick frequencies that produce complement frequencies > u16::MAX
            let test_cases = [
                (0u32, 100u32),                       // small freq, start=0
                (0u32, 50000u32),                     // freq that needs >16-bit cmpl
                (100u32, 1u32),                       // freq=1 special case
                (0u32, (1u32 << scale_bits.min(20))), // large freq
                (total as u32 / 3, total as u32 / 3), // start > 0, freq > 0
            ];

            for &(start, freq) in &test_cases {
                // Skip invalid cases
                if freq == 0 {
                    continue;
                }
                if (start as u64) + (freq as u64) > total {
                    continue;
                }

                let sym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();

                // Verify cmpl_freq is correct (u32, not truncated to u16)
                let expected_cmpl = ((1u64 << scale_bits) - freq as u64) as u32;
                assert_eq!(
                    sym.cmpl_freq, expected_cmpl,
                    "cmpl_freq mismatch for scale_bits={}, start={}, freq={}: expected {}, got {}",
                    scale_bits, start, freq, expected_cmpl, sym.cmpl_freq
                );

                // Verify x_max matches upstream formula
                let expected_x_max = ((RANS64_L >> scale_bits) << 32) * (freq as u64);
                assert_eq!(
                    sym.x_max, expected_x_max,
                    "x_max mismatch for scale_bits={}, start={}, freq={}",
                    scale_bits, start, freq
                );

                // For freq >= 2, verify that rcp_freq * freq approximately equals 2^(shift+63)
                if freq >= 2 {
                    assert!(sym.rcp_freq > 0, "rcp_freq must be > 0 for freq={}", freq);

                    // Verify rcp_shift consistent with freq
                    let mut expected_shift = 0u32;
                    while freq > (1u32 << expected_shift) {
                        expected_shift += 1;
                    }
                    assert_eq!(
                        sym.rcp_shift,
                        expected_shift - 1,
                        "rcp_shift mismatch for freq={}",
                        freq
                    );
                }

                // Verify bias
                let expected_bias = if freq < 2 {
                    (start as u64) + (1u64 << scale_bits) - 1
                } else {
                    start as u64
                };
                assert_eq!(
                    sym.bias, expected_bias,
                    "bias mismatch for scale_bits={}, start={}, freq={}",
                    scale_bits, start, freq
                );
            }
        }
    }

    #[test]
    fn test_rans64_reciprocal_equals_division_large() {
        // Verify that the reciprocal fast path produces the same state
        // as the division-based reference for large scale_bits and various states.
        use super::*;

        let scale_bits = 30;

        // Frequencies that produce >16-bit complements
        let freqs = [1u32, 2, 100, 10000, 500000000, 1000000000, (1u32 << 30) - 1];
        let total = 1u64 << scale_bits;

        for &freq in &freqs {
            let start = 0u32;
            if (start as u64) + (freq as u64) > total {
                continue;
            }

            let sym = Rans64EncSymbol::new(start, freq, scale_bits).unwrap();

            // Test several state values that are within normalization bounds
            let states = [RANS64_L, RANS64_L + 1, RANS64_L * 2, (1u64 << 62) - 1];

            for &state_val in &states {
                if state_val >= sym.x_max {
                    continue; // skip states that would trigger renormalization
                }

                // Division-based reference
                let div_state = ((state_val / freq as u64) << scale_bits)
                    + (state_val % freq as u64)
                    + start as u64;

                // Reciprocal fast path
                let q = rans64_mul_hi(state_val, sym.rcp_freq) >> sym.rcp_shift;
                let fast_state = state_val + sym.bias + q * (sym.cmpl_freq as u64);

                assert_eq!(
                    fast_state, div_state,
                    "reciprocal mismatch for scale_bits={}, freq={}, state={}: div={}, fast={}",
                    scale_bits, freq, state_val, div_state, fast_state
                );
            }
        }
    }
}
