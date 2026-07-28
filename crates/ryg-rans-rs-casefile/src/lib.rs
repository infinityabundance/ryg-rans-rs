//! # ryg-rans-rs-casefile
//!
//! **Typed evidence schema foundation for rANS forensic court proceedings.**
//!
//! This crate provides the data types used throughout the ryg-rans-rs forensic
//! testing infrastructure to record, reproduce, and track court proceedings
//! between the Rust implementation and the compiled C/C++ oracle.
//!
//! ## Core Types
//!
//! | Type | Purpose |
//! |------|---------|
//! | [`Casefile`] | A complete, self-contained test case: inputs, model, expected outputs, and environment metadata |
//! | [`Receipt`] | The verdict of a completed court: which cases were compared, how many matched, and which residuals remain |
//! | [`Residual`] | A single observed difference between implementations: class, severity, status, and reproduction command |
//!
//! ## Design
//!
//! - **Deterministic**: All fields are explicitly typed with no ambient state.
//!   Case generation must use fixed seeds and named PRNG algorithms.
//! - **Content-addressed**: Large payloads are referenced by SHA-256 hash and
//!   stored separately from the casefile manifest.
//! - **Self-describing**: Every casefile records its schema version, upstream
//!   commit, compiler, host architecture, and endianness.
//!
//! ## Usage
//!
//! ```rust
//! # use ryg_rans_rs_casefile::*;
//! let case = Casefile::new("RYG_RANS.BYTE.BITSTREAM.000001", "byte32");
//! println!("Case: {} (variant: {})", case.case_id, case.variant);
//!
//! let residual = Residual {
//!     case_id: "RYG_RANS.BYTE.BITSTREAM.000001",
//!     court_id: "RYG_RANS.BYTE.BITSTREAM",
//!     variant: "byte32",
//!     upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
//!     class: "byte_mismatch",
//!     severity: "S1",
//!     status: "open",
//! };
//! println!("{}", residual);
//! ```

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
use alloc::vec::Vec;
use core::fmt;

/// Schema version for casefiles.
pub const CASEFILE_SCHEMA_VERSION: u32 = 1;

/// A deterministic test case for rANS encoding/decoding.
///
/// Contains everything needed to reproduce a specific test: input data,
/// frequency model, scale bits, interleave setting, and expected outputs.
/// Large payloads are referenced by content hash and stored separately.
#[derive(Clone, Debug)]
pub struct Casefile {
    pub schema_version: u32,
    pub case_id: &'static str,
    pub upstream_commit: &'static str,
    pub variant: &'static str,
    pub operation: &'static str,
    pub seed: u64,
    pub input_sha256: Option<[u8; 32]>,
    pub input: Vec<u8>,
    pub scale_bits: u32,
    pub frequencies: Vec<u32>,
    pub cumulative_frequencies: Vec<u32>,
    pub interleave: u32,
}

impl Casefile {
    /// Create a new casefile with the given case ID and variant.
    ///
    /// Automatically sets the upstream commit to the pinned revision
    /// and initializes default values for all fields.
    pub fn new(case_id: &'static str, variant: &'static str) -> Self {
        Self {
            schema_version: CASEFILE_SCHEMA_VERSION,
            case_id,
            upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d",
            variant,
            operation: "encode_decode",
            seed: 0,
            input_sha256: None,
            input: Vec::new(),
            scale_bits: 14,
            frequencies: Vec::new(),
            cumulative_frequencies: Vec::new(),
            interleave: 1,
        }
    }
}

/// A court receipt documenting the result of an oracle comparison.
///
/// Receipts are the primary evidence artifact. A receipt with
/// `admitted_match` is required before any surface can be labelled `full`
/// in the parity model.
#[derive(Clone, Debug)]
pub struct Receipt {
    pub schema_version: u32,
    pub court_id: &'static str,
    pub case_count: u32,
    pub verdict: &'static str,
    pub upstream_commit: &'static str,
    pub rust_commit: Option<&'static str>,
    pub pairs_compared: u64,
    pub pairs_matched: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<&'static str>,
    pub timestamp: Option<u64>,
}

/// A residual documenting an observed difference between implementations.
///
/// Residuals are first-class engineering artifacts. Every observed difference
/// must be recorded, classified, and tracked until resolved or explicitly
/// admitted as a safe divergence.
#[derive(Clone, Debug)]
pub struct Residual {
    pub case_id: &'static str,
    pub court_id: &'static str,
    pub variant: &'static str,
    pub upstream_commit: &'static str,
    pub class: &'static str,
    pub severity: &'static str,
    pub status: &'static str,
}

impl fmt::Display for Residual {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} ({}): {} [{}] - {}",
            self.case_id, self.court_id, self.class, self.severity, self.status
        )
    }
}
