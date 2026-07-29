//! # Codec registry — stable codec IDs for container format
//!
//! Codec IDs identify the **stream format** (number of states, renormalization
//! unit, scale constraint).  They do NOT identify the arithmetic implementation
//! (division vs reciprocal) or the decode backend (scalar vs SIMD).

/// Codec ID constants.
///
/// | ID | Name | States | Scale | Renorm |
/// |----|------|--------|-------|--------|
/// | 1 | BYTE_SINGLE | 1 | 1..16 | 8-bit |
/// | 2 | BYTE_INTERLEAVED2 | 2 | 1..16 | 8-bit |
/// | 3 | R64_SINGLE | 1 | 1..31 | 32-bit |
/// | 4 | R64_INTERLEAVED2 | 2 | 1..31 | 32-bit |
/// | 5 | WORD_SINGLE | 1 | 12 | 16-bit |
/// | 6 | WORD_INTERLEAVED2 | 2 | 12 | 16-bit |
/// | 7 | WORD_INTERLEAVED8 | 8 | 12 | 16-bit |
/// | 8 | WORD_INTERLEAVED16 | 16 | 12 | 16-bit |
/// | 9 | ALIAS_SINGLE | 1 | 8..17 | 8-bit |
/// | 10 | ALIAS_INTERLEAVED2 | 2 | 8..17 | 8-bit |
pub mod ids {
    pub const BYTE_SINGLE: u16 = 1;
    pub const BYTE_INTERLEAVED2: u16 = 2;
    pub const R64_SINGLE: u16 = 3;
    pub const R64_INTERLEAVED2: u16 = 4;
    pub const WORD_SINGLE: u16 = 5;
    pub const WORD_INTERLEAVED2: u16 = 6;
    pub const WORD_INTERLEAVED8: u16 = 7;
    pub const WORD_INTERLEAVED16: u16 = 8;
    pub const ALIAS_SINGLE: u16 = 9;
    pub const ALIAS_INTERLEAVED2: u16 = 10;
}

/// Return the canonical name for a codec ID.
pub fn codec_name(id: u16) -> &'static str {
    match id {
        ids::BYTE_SINGLE => "byte-single",
        ids::BYTE_INTERLEAVED2 => "byte-interleaved2",
        ids::R64_SINGLE => "r64-single",
        ids::R64_INTERLEAVED2 => "r64-interleaved2",
        ids::WORD_SINGLE => "word-single",
        ids::WORD_INTERLEAVED2 => "word-interleaved2",
        ids::WORD_INTERLEAVED8 => "word-interleaved8",
        ids::WORD_INTERLEAVED16 => "word-interleaved16",
        ids::ALIAS_SINGLE => "alias-single",
        ids::ALIAS_INTERLEAVED2 => "alias-interleaved2",
        _ => "unknown",
    }
}

/// Return the expected state count for a codec ID.
pub fn codec_states(id: u16) -> Option<u8> {
    match id {
        ids::BYTE_SINGLE | ids::R64_SINGLE | ids::WORD_SINGLE | ids::ALIAS_SINGLE => Some(1),
        ids::BYTE_INTERLEAVED2
        | ids::R64_INTERLEAVED2
        | ids::WORD_INTERLEAVED2
        | ids::ALIAS_INTERLEAVED2 => Some(2),
        ids::WORD_INTERLEAVED8 => Some(8),
        ids::WORD_INTERLEAVED16 => Some(16),
        _ => None,
    }
}

/// Validate scale_bits for a codec ID.
pub fn validate_scale_bits(id: u16, scale_bits: u8) -> Result<(), &'static str> {
    match id {
        ids::BYTE_SINGLE | ids::BYTE_INTERLEAVED2 => {
            if scale_bits >= 1 && scale_bits <= 16 {
                Ok(())
            } else {
                Err("byte rANS requires scale_bits 1..=16")
            }
        }
        ids::R64_SINGLE | ids::R64_INTERLEAVED2 => {
            if scale_bits >= 1 && scale_bits <= 31 {
                Ok(())
            } else {
                Err("R64 requires scale_bits 1..=31")
            }
        }
        ids::WORD_SINGLE
        | ids::WORD_INTERLEAVED2
        | ids::WORD_INTERLEAVED8
        | ids::WORD_INTERLEAVED16 => {
            if scale_bits == 12 {
                Ok(())
            } else {
                Err("word rANS requires scale_bits = 12")
            }
        }
        ids::ALIAS_SINGLE | ids::ALIAS_INTERLEAVED2 => {
            if scale_bits >= 8 && scale_bits <= 17 {
                Ok(())
            } else {
                Err("alias requires scale_bits 8..=17")
            }
        }
        _ => Err("unknown codec"),
    }
}

/// Validate that a codec ID is known and supported.
pub fn is_supported(id: u16) -> bool {
    matches!(
        id,
        ids::BYTE_SINGLE
            | ids::BYTE_INTERLEAVED2
            | ids::R64_SINGLE
            | ids::R64_INTERLEAVED2
            | ids::WORD_SINGLE
            | ids::WORD_INTERLEAVED2
            | ids::WORD_INTERLEAVED8
            | ids::WORD_INTERLEAVED16
            | ids::ALIAS_SINGLE
            | ids::ALIAS_INTERLEAVED2
    )
}
