//! # RYGRANS Container Format v1 — types and serialization
//!
//! This module implements the versioned block-streaming container format.
//! Every function is bounds-checked, every allocation is limited, and every
//! struct has canonical serialization.

pub mod block;
pub mod codec;
pub mod footer;
pub mod header;
pub mod model;
pub mod reader;
pub mod writer;

/// Magic bytes for RYGRANS v1 containers.
pub const MAGIC: &[u8; 8] = b"RYGRANS\0";

/// Current major version.
pub const MAJOR_VERSION: u16 = 1;

/// Current minor version.
pub const MINOR_VERSION: u16 = 0;

/// Header size in bytes.
pub const HEADER_SIZE: usize = 32;

/// Block header size in bytes.
pub const BLOCK_HEADER_SIZE: usize = 104;

/// Footer size in bytes.
pub const FOOTER_SIZE: usize = 104;

/// Block tag.
pub const BLOCK_TAG: &[u8; 4] = b"BLK1";

/// Footer tag.
pub const FOOTER_TAG: &[u8; 4] = b"END1";

/// Block kind: RAW (uncompressed).
pub const BLOCK_KIND_RAW: u8 = 0;

/// Block kind: RLE (single-symbol run-length).
pub const BLOCK_KIND_RLE: u8 = 1;

/// Block kind: RANS (rANS-compressed).
pub const BLOCK_KIND_RANS: u8 = 2;
