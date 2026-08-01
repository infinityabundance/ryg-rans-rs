//! # CLI operations — shared machinery
//!
//! The subcommand implementations live in sibling modules (`encode`,
//! `decode`, `inspect`, `verify`, `model`, `trace`, `compare`, `bench`).
//! This module holds the pieces they share:
//!
//! * bounded input/output helpers (resource limits enforced during I/O),
//! * the per-block codec dispatcher (block kind → codec → decoded bytes),
//! * the container walk used by decode/inspect/verify/compare.
//!
//! ## Why the dispatcher lives here
//!
//! Every consumer of a container (decode, inspect --deep, verify, compare)
//! must run the **same** codec logic, otherwise a container could verify
//! under one path and decode under another.  One dispatcher = one truth.

pub mod bench;
pub mod compare;
pub mod decode;
pub mod encode;
pub mod inspect;
pub mod model;
pub mod trace;
pub mod verify;

use crate::container::block::Block;
use crate::container::codec;
use crate::container::reader::ContainerReader;
use crate::error::{AppError, CodecError, FormatError, IoError, UnsupportedError};
use crate::limits::Limits;
use clap::ArgMatches;
use ryg_rans_rs_core::{
    BackwardByteWriter, BackwardWord16Writer, BackwardWord32Writer, ByteInterleavedDecoder,
    ByteInterleavedEncoder, ByteReader, Rans64DecSymbol, Rans64EncSymbol, Rans64State,
    RansByteDecSymbol, RansByteEncSymbol, RansByteState, RansWordSlot, RansWordState,
    RansWordTables, Word16Reader, Word32Reader, rans_byte_dec_advance_symbol, rans_byte_dec_get,
    rans_byte_dec_init, rans_byte_enc_flush, rans_byte_enc_put_symbol, rans_word_dec_init,
    rans_word_dec_renorm, rans_word_dec_sym, rans_word_enc_flush, rans_word_enc_put,
    rans64_dec_advance_symbol, rans64_dec_get, rans64_dec_init, rans64_enc_flush,
    rans64_enc_put_symbol,
};
use sha2::{Digest, Sha256};
use std::io::{BufReader, IsTerminal, Read, Write};

/// Default scale bits used when the user does not specify one.
pub const DEFAULT_SCALE_BITS: u8 = 12;

/// Parse the shared `--timeout` argument into a fractional-seconds bound.
///
/// Returns a typed `Format` error for a non-numeric, negative, or non-finite
/// value so a typo like `--timeout -1` never silently disables the watchdog
/// (which is what `0` is documented to mean).
pub fn parse_timeout(matches: &ArgMatches) -> Result<f64, AppError> {
    let secs: f64 = matches
        .get_one::<String>("timeout")
        .map(|s| {
            s.parse::<f64>().map_err(|_| {
                AppError::Format(FormatError {
                    detail: format!(
                        "invalid --timeout '{}': expected seconds (fractional allowed, e.g. 0.5)",
                        s
                    ),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(0.0);
    if !secs.is_finite() || secs < 0.0 {
        return Err(AppError::Format(FormatError {
            detail: "invalid --timeout: must be a finite non-negative number of seconds".into(),
            block_index: None,
            offset: None,
        }));
    }
    Ok(secs)
}

/// Result of walking a whole container.
#[derive(Debug, Clone)]
pub struct DecodeOutcome {
    pub block_count: u64,
    pub total_uncompressed: u64,
    pub total_payload: u64,
    pub decoded_stream_sha256: [u8; 32],
}

/// The kind of block the encoder chose for one chunk of input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockChoice {
    /// Raw block: bytes stored verbatim.
    Raw,
    /// Run-length block: `symbol` repeated `count` times.
    Rle { symbol: u8, count: u32 },
    /// rANS block: compressed payload plus canonical model bytes.
    Rans { payload: Vec<u8>, model: Vec<u8> },
}

/// Open an input source ('-' = stdin) with a byte cap enforced during reads.
pub fn open_input(path: &str, max_bytes: u64) -> Result<Box<dyn Read>, AppError> {
    if path == "-" {
        Ok(Box::new(BoundedReader::new(
            std::io::stdin().lock(),
            max_bytes,
        )))
    } else {
        let file = std::fs::File::open(path).map_err(|e| {
            AppError::Io(IoError {
                path: Some(path.into()),
                detail: format!("open input: {}", e),
            })
        })?;
        Ok(Box::new(BoundedReader::new(file, max_bytes)))
    }
}

/// Open an output source ('-' = stdout) honouring the force/force-tty guards.
///
/// Binary output to a terminal is refused unless `force_tty` is set; existing
/// files are refused unless `force` is set.
pub fn open_output(path: &str, force: bool, force_tty: bool) -> Result<Box<dyn Write>, AppError> {
    if path == "-" {
        if !force_tty && std::io::stdout().is_terminal() {
            return Err(AppError::Io(IoError {
                path: None,
                detail: "refusing binary output to a terminal (use --force-tty)".into(),
            }));
        }
        Ok(Box::new(std::io::stdout().lock()))
    } else {
        // `--force`: create if missing, truncate if present.
        // default: create_new only (refuse to overwrite).
        let file = if force {
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
        } else {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
        }
        .map_err(|e| {
            AppError::Io(IoError {
                path: Some(path.into()),
                detail: format!("open output: {}", e),
            })
        })?;
        Ok(Box::new(file))
    }
}

/// Reader that refuses to yield more than `max_bytes` total.
///
/// This is the enforcement point for `Limits::max_input_bytes`; the cap is
/// applied while reading, so a hostile or oversized stream cannot allocate
/// past the limit (allocation is bounded by the caller's chunk size).
pub struct BoundedReader<R: Read> {
    inner: R,
    max_bytes: u64,
    read_bytes: u64,
}

impl<R: Read> BoundedReader<R> {
    pub fn new(inner: R, max_bytes: u64) -> Self {
        Self {
            inner,
            max_bytes,
            read_bytes: 0,
        }
    }
}

impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.read_bytes >= self.max_bytes {
            return Ok(0); // EOF at the cap
        }
        let remaining = self.max_bytes - self.read_bytes;
        let take = (buf.len() as u64).min(remaining) as usize;
        let n = self.inner.read(&mut buf[..take])?;
        self.read_bytes += n as u64;
        Ok(n)
    }
}

/// Build the cumulative-frequency → symbol table for byte rANS decode.
///
/// `cum2sym[cf]` maps a cumulative-frequency slot to its symbol.  The model
/// is validated by the caller (`FrequencyModel::from_bytes`), so every slot
/// in `0..2^scale_bits` is covered exactly once.
pub fn build_cum2sym(model: &crate::container::model::FrequencyModel) -> Vec<u8> {
    let total = 1usize << model.scale_bits;
    let mut cum2sym = vec![0u8; total];
    for s in 0..256usize {
        let start = model.cumulative[s] as usize;
        let end = model.cumulative[s + 1] as usize;
        for cf in start..end {
            cum2sym[cf] = s as u8;
        }
    }
    cum2sym
}

/// Build word rANS decode tables (slots + slot2sym) for a model.
///
/// Equivalent to the upstream `RansWordTablesInitSymbol` per-symbol filling
/// used by `rans_word_sse41.h`; used by the scalar word decoder.
pub fn build_word_tables(
    model: &crate::container::model::FrequencyModel,
) -> (Vec<RansWordSlot>, Vec<u8>) {
    let m = 1usize << model.scale_bits;
    let mut slots = vec![RansWordSlot { freq: 0, bias: 0 }; m];
    let mut slot2sym = vec![0u8; m];
    for s in 0..256usize {
        let freq = model.frequencies[s] as usize;
        let start = model.cumulative[s] as usize;
        for i in 0..freq {
            let slot = start + i;
            if slot < m {
                slots[slot] = RansWordSlot {
                    freq: freq as u16,
                    bias: i as u16,
                };
                slot2sym[slot] = s as u8;
            }
        }
    }
    (slots, slot2sym)
}

/// Compute the SHA-256 of a byte slice (hex-encoded).
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    let out = h.finalize();
    let mut s = String::with_capacity(64);
    for b in out {
        use std::fmt::Write as _;
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Decode one block's payload into its decoded bytes, dispatching on the
/// block kind and codec ID.  This is the single codec truth for every
/// consumer of a container.
pub fn decode_block(
    info: &crate::container::block::BlockHeaderInfo,
    model_data: &[u8],
    payload: &[u8],
) -> Result<Vec<u8>, AppError> {
    match info.block_kind {
        crate::container::BLOCK_KIND_RAW => {
            // RAW blocks carry the decoded bytes verbatim; the reader has
            // already verified payload_sha256 == decoded bytes.
            Ok(payload.to_vec())
        }
        crate::container::BLOCK_KIND_RLE => {
            // RLE blocks: payload[0] repeated uncompressed_length times.
            let sym = *payload.first().ok_or_else(|| {
                AppError::Format(FormatError {
                    detail: "RLE block with empty payload".into(),
                    block_index: Some(info.block_index),
                    offset: None,
                })
            })?;
            let len = info.uncompressed_length as usize;
            Ok(vec![sym; len])
        }
        crate::container::BLOCK_KIND_RANS => {
            let model =
                crate::container::model::FrequencyModel::from_bytes(model_data, info.scale_bits)?;
            let scale_bits = info.scale_bits as u32;
            match info.codec_id {
                codec::ids::BYTE_SINGLE => decode_byte_single(
                    info.uncompressed_length as usize,
                    payload,
                    &model,
                    scale_bits,
                ),
                codec::ids::BYTE_INTERLEAVED2 => decode_byte_interleaved2(
                    info.uncompressed_length as usize,
                    payload,
                    &model,
                    scale_bits,
                ),
                codec::ids::R64_SINGLE => decode_r64_single(
                    info.uncompressed_length as usize,
                    payload,
                    &model,
                    scale_bits,
                ),
                codec::ids::WORD_SINGLE => decode_word_single(
                    info.uncompressed_length as usize,
                    payload,
                    &model,
                    scale_bits,
                ),
                codec::ids::WORD_INTERLEAVED8 => {
                    #[cfg(feature = "simd")]
                    {
                        decode_word_8way(info.uncompressed_length as usize, payload, &model)
                    }
                    // The 8-way codec requires the SIMD crate (packed tables +
                    // scalar/SIMD kernels).  With `--no-default-features` the
                    // crate is not compiled in; per the no-silent-fallback
                    // doctrine an explicit typed error is returned rather than
                    // silently reinterpreting the stream as single-state.
                    #[cfg(not(feature = "simd"))]
                    {
                        Err(AppError::Unsupported(UnsupportedError {
                            detail: format!(
                                "codec {} requires the `simd` feature (CLI built with --no-default-features)",
                                codec::ids::WORD_INTERLEAVED8
                            ),
                        }))
                    }
                }
                other => Err(AppError::Unsupported(UnsupportedError {
                    detail: format!("codec {} not supported by the CLI decoder", other),
                })),
            }
        }
        other => Err(AppError::Format(FormatError {
            detail: format!("unknown block kind {}", other),
            block_index: Some(info.block_index),
            offset: None,
        })),
    }
}

/// Single-state byte rANS decode (codec 1).
fn decode_byte_single(
    len: usize,
    payload: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u32,
) -> Result<Vec<u8>, AppError> {
    let mut reader = ByteReader::new(payload);
    let mut state = rans_byte_dec_init(&mut reader).map_err(codec_err)?;
    let cum2sym = build_cum2sym(model);
    let mut out = vec![0u8; len];
    for o in out.iter_mut() {
        let cf = rans_byte_dec_get(&state, scale_bits) as usize;
        let s = *cum2sym.get(cf).ok_or_else(|| {
            AppError::Codec(CodecError {
                detail: format!("cumulative slot {} out of range", cf),
                codec_id: Some(codec::ids::BYTE_SINGLE),
            })
        })?;
        let dsym =
            RansByteDecSymbol::new(model.cumulative[s as usize], model.frequencies[s as usize])
                .map_err(|e| AppError::Model(e))?;
        *o = s;
        rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits).map_err(|_| {
            AppError::Format(FormatError {
                detail: "truncated byte rANS stream".into(),
                block_index: None,
                offset: None,
            })
        })?;
    }
    Ok(out)
}

/// Two-state interleaved byte rANS decode (codec 2).
fn decode_byte_interleaved2(
    len: usize,
    payload: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u32,
) -> Result<Vec<u8>, AppError> {
    let cum2sym = build_cum2sym(model);
    // Unseen symbols get a valid placeholder (never indexed: `cum2sym` only
    // maps slots to observed symbols).
    let dsyms: Vec<RansByteDecSymbol> = (0..256usize)
        .map(|s| {
            let f = model.frequencies[s];
            if f == 0 {
                RansByteDecSymbol::new(0, 1)
            } else {
                RansByteDecSymbol::new(model.cumulative[s], f)
            }
        })
        .collect::<Result<_, _>>()
        .map_err(|e| AppError::Model(e))?;
    let mut reader = ByteReader::new(payload);
    let mut dec = ByteInterleavedDecoder::new(&mut reader, scale_bits).map_err(|_| {
        AppError::Format(FormatError {
            detail: "truncated interleaved stream (init)".into(),
            block_index: None,
            offset: None,
        })
    })?;
    let mut out = vec![0u8; len];
    dec.decode(&mut out, &cum2sym, &dsyms).map_err(|_| {
        AppError::Format(FormatError {
            detail: "truncated interleaved byte rANS stream".into(),
            block_index: None,
            offset: None,
        })
    })?;
    Ok(out)
}

/// Single-state 64-bit rANS decode (codec 3).
fn decode_r64_single(
    len: usize,
    payload: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u32,
) -> Result<Vec<u8>, AppError> {
    let mut reader = Word32Reader::new(payload);
    let mut state = rans64_dec_init(&mut reader).map_err(|_| {
        AppError::Format(FormatError {
            detail: "truncated R64 stream (init)".into(),
            block_index: None,
            offset: None,
        })
    })?;
    let cum2sym = build_cum2sym(model);
    let mut out = vec![0u8; len];
    for o in out.iter_mut() {
        let cf = rans64_dec_get(&state, scale_bits) as usize;
        let s = *cum2sym.get(cf).ok_or_else(|| {
            AppError::Codec(CodecError {
                detail: format!("cumulative slot {} out of range", cf),
                codec_id: Some(codec::ids::R64_SINGLE),
            })
        })?;
        let dsym =
            Rans64DecSymbol::new(model.cumulative[s as usize], model.frequencies[s as usize])
                .map_err(|e| AppError::Model(e))?;
        *o = s;
        rans64_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits).map_err(|_| {
            AppError::Format(FormatError {
                detail: "truncated R64 stream".into(),
                block_index: None,
                offset: None,
            })
        })?;
    }
    Ok(out)
}

/// Single-state word rANS decode (codec 5).
fn decode_word_single(
    len: usize,
    payload: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u32,
) -> Result<Vec<u8>, AppError> {
    let (slots, slot2sym) = build_word_tables(model);
    let tables = RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };
    let mut reader = Word16Reader::new(payload);
    let mut state = rans_word_dec_init(&mut reader).map_err(|_| {
        AppError::Format(FormatError {
            detail: "truncated word rANS stream (init)".into(),
            block_index: None,
            offset: None,
        })
    })?;
    let mut out = vec![0u8; len];
    for o in out.iter_mut() {
        *o = rans_word_dec_sym(&mut state, &tables, scale_bits);
        rans_word_dec_renorm(&mut state, &mut reader).map_err(|_| {
            AppError::Format(FormatError {
                detail: "truncated word rANS stream".into(),
                block_index: None,
                offset: None,
            })
        })?;
    }
    Ok(out)
}

/// Eight-way interleaved word rANS decode (codec 7) via the SIMD crate.
///
/// The canonical 8-way stream is a sequence of u16 words; the container
/// stores the payload as little-endian bytes, so the payload is converted to
/// words before forwarding.  `decode_simd_8way` selects the SIMD kernel at
/// compile time and falls back to its scalar 8-way reference when SSE4.1 is
/// not compiled in.
///
/// Requires the `simd` feature (the optional `ryg-rans-rs-simd` dependency);
/// callers must gate on `#[cfg(feature = "simd")]`.
#[cfg(feature = "simd")]
fn decode_word_8way(
    len: usize,
    payload: &[u8],
    model: &crate::container::model::FrequencyModel,
) -> Result<Vec<u8>, AppError> {
    let words = payload_to_words(payload)?;
    let (slots, slot2sym) = ryg_rans_rs_simd::build_word_tables(
        &model.frequencies,
        &model.cumulative,
        model.scale_bits as u32,
    );
    let tables = ryg_rans_rs_simd::RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };
    ryg_rans_rs_simd::decode_simd_8way(&words, &tables, len).map_err(|e| {
        AppError::Format(FormatError {
            detail: format!("8-way stream decode failed: {}", e),
            block_index: None,
            offset: None,
        })
    })
}

/// Convert a byte payload to little-endian u16 words.
///
/// Only the 8-way codecs consume word payloads; the function is compiled in
/// only when the `simd` feature (which provides the 8-way kernels) is on.
#[cfg(feature = "simd")]
fn payload_to_words(payload: &[u8]) -> Result<Vec<u16>, AppError> {
    if payload.len() % 2 != 0 {
        return Err(AppError::Format(FormatError {
            detail: "8-way payload has odd byte length".into(),
            block_index: None,
            offset: None,
        }));
    }
    Ok(payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect())
}

/// Explicit scalar 8-way decode (codec 7) — used by `compare backends` as
/// the reference side, independent of compile-time SIMD selection.
///
/// Requires the `simd` feature; the `compare` op gates its only call site.
#[cfg(feature = "simd")]
pub fn decode_word_8way_scalar_explicit(
    len: usize,
    payload: &[u8],
    model: &crate::container::model::FrequencyModel,
) -> Result<Vec<u8>, AppError> {
    let words = payload_to_words(payload)?;
    let (slots, slot2sym) = ryg_rans_rs_simd::build_word_tables(
        &model.frequencies,
        &model.cumulative,
        model.scale_bits as u32,
    );
    let tables = ryg_rans_rs_simd::RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };
    ryg_rans_rs_simd::decode_8way_scalar(&words, &tables, len).map_err(|e| {
        AppError::Format(FormatError {
            detail: format!("scalar 8-way decode failed: {}", e),
            block_index: None,
            offset: None,
        })
    })
}

fn codec_err(_e: ryg_rans_rs_core::DecodeError) -> AppError {
    AppError::Format(FormatError {
        detail: "truncated stream (init)".into(),
        block_index: None,
        offset: None,
    })
}

/// Walk a complete container, decoding every block through `decode_block`,
/// verifying every hash, and invoking `sink` with each decoded block.
///
/// Used by decode (sink writes bytes), inspect --deep (sink records
/// metadata), verify (sink counts), and compare (sink hashes).
pub fn walk_container<R: Read, F>(
    reader: R,
    limits: Limits,
    mut sink: F,
) -> Result<DecodeOutcome, AppError>
where
    F: FnMut(&Block, &[u8]) -> Result<(), AppError>,
{
    let mut cr = ContainerReader::new(BufReader::new(reader), limits);
    let _header = cr.read_header()?;
    loop {
        match cr.read_block(|info, model_data, payload| decode_block(info, model_data, payload))? {
            Some((block, decoded)) => sink(&block, &decoded)?,
            None => break,
        }
    }
    let _footer = cr.read_footer()?;
    cr.check_trailing_data()?;
    Ok(DecodeOutcome {
        block_count: cr.block_count_seen(),
        total_uncompressed: cr.total_uncompressed_seen(),
        total_payload: cr.total_payload_seen(),
        decoded_stream_sha256: cr.decoded_stream_hash(),
    })
}

/// Map a codec name string (CLI arg) to a codec ID.
pub fn codec_from_name(name: &str) -> Result<u16, AppError> {
    match name {
        "byte-single" => Ok(codec::ids::BYTE_SINGLE),
        "byte-interleaved2" => Ok(codec::ids::BYTE_INTERLEAVED2),
        "r64-single" => Ok(codec::ids::R64_SINGLE),
        "word-single" => Ok(codec::ids::WORD_SINGLE),
        other => Err(AppError::Unsupported(UnsupportedError {
            detail: format!("codec '{}' not implemented in the CLI encoder", other),
        })),
    }
}

/// Choose and produce the block kind for one chunk of input.
///
/// Decision order:
/// 1. Empty or single-symbol chunk → RLE (or RAW when the chunk is tiny).
/// 2. Otherwise encode with the selected rANS codec.
/// 3. If the rANS payload does not shrink the chunk and `always_compress` is
///    false → RAW fallback.
///
/// The model is rebuilt per block (per-block model mode); the container
/// stores the canonical model bytes so decode is self-contained.
pub fn select_block(
    codec_id: u16,
    scale_bits: u8,
    data: &[u8],
    always_compress: bool,
) -> Result<BlockChoice, AppError> {
    // A single-symbol chunk is always RLE; an empty chunk is RAW.
    let first = *data.first().unwrap_or(&0u8);
    if data.iter().all(|&b| b == first) {
        return if data.is_empty() {
            Ok(BlockChoice::Raw)
        } else {
            Ok(BlockChoice::Rle {
                symbol: first,
                count: data.len() as u32,
            })
        };
    }

    // Validate scale bits for the codec before touching the model.
    codec::validate_scale_bits(codec_id, scale_bits).map_err(|e| {
        AppError::Format(FormatError {
            detail: format!("invalid scale bits for codec {}: {}", codec_id, e),
            block_index: None,
            offset: None,
        })
    })?;

    let mut hist = [0u64; 256];
    for &b in data {
        hist[b as usize] += 1;
    }
    let model = crate::container::model::FrequencyModel::build(&hist, scale_bits)?;
    let model_bytes = model.to_bytes();

    // Word rANS tables store freq in a u16 slot.  With at least two active
    // symbols the model builder keeps every frequency < 2^scale_bits
    // (they sum to exactly 2^scale_bits), so u16 overflow is impossible;
    // guard anyway so a future model change cannot silently truncate.
    if codec_id == codec::ids::WORD_SINGLE && model.frequencies.iter().any(|&f| f > 0x7fff) {
        return Err(AppError::Codec(CodecError {
            detail: "word rANS cannot represent a frequency above 32767".into(),
            codec_id: Some(codec_id),
        }));
    }

    let payload = match codec_id {
        codec::ids::BYTE_SINGLE => encode_byte_single(data, &model, scale_bits)?,
        codec::ids::BYTE_INTERLEAVED2 => encode_byte_interleaved2(data, &model, scale_bits)?,
        codec::ids::R64_SINGLE => encode_r64_single(data, &model, scale_bits)?,
        codec::ids::WORD_SINGLE => encode_word_single(data, &model, scale_bits)?,
        other => {
            return Err(AppError::Unsupported(UnsupportedError {
                detail: format!("codec {} not implemented in the CLI encoder", other),
            }));
        }
    };

    // Incompressible-data policy: keep the payload only when it actually
    // shrinks the block (unless always_compress demands rANS).
    if !always_compress && payload.len() >= data.len() {
        return Ok(BlockChoice::Raw);
    }

    Ok(BlockChoice::Rans {
        payload,
        model: model_bytes,
    })
}

fn encode_byte_single(
    data: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u8,
) -> Result<Vec<u8>, AppError> {
    let mut buf = vec![0u8; data.len().saturating_mul(4).saturating_add(64)];
    let mut writer = BackwardByteWriter::new(&mut buf);
    let mut state = RansByteState::new();
    for &s in data.iter().rev() {
        let sym = RansByteEncSymbol::new(
            model.cumulative[s as usize],
            model.frequencies[s as usize],
            scale_bits as u32,
        )
        .map_err(|e| AppError::Model(e))?;
        rans_byte_enc_put_symbol(&mut state, &mut writer, &sym).map_err(|_| {
            AppError::Codec(CodecError {
                detail: "byte encode buffer exhausted".into(),
                codec_id: Some(codec::ids::BYTE_SINGLE),
            })
        })?;
    }
    rans_byte_enc_flush(&state, &mut writer).map_err(|_| {
        AppError::Codec(CodecError {
            detail: "byte encode flush failed".into(),
            codec_id: Some(codec::ids::BYTE_SINGLE),
        })
    })?;
    Ok(writer.encoded().to_vec())
}

fn encode_byte_interleaved2(
    data: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u8,
) -> Result<Vec<u8>, AppError> {
    let mut buf = vec![0u8; data.len().saturating_mul(4).saturating_add(64)];
    let mut writer = BackwardByteWriter::new(&mut buf);
    // Symbols are indexed by symbol value inside `encode_reverse`, which only
    // touches symbols present in `data`.  Unseen symbols get a valid
    // placeholder (never read) so the eager table construction cannot fail
    // on zero-frequency entries.
    let esyms: Vec<RansByteEncSymbol> = (0..256usize)
        .map(|s| {
            let f = model.frequencies[s];
            if f == 0 {
                RansByteEncSymbol::new(0, 1, scale_bits as u32)
            } else {
                RansByteEncSymbol::new(model.cumulative[s], f, scale_bits as u32)
            }
        })
        .collect::<Result<_, _>>()
        .map_err(|e| AppError::Model(e))?;
    let enc = ByteInterleavedEncoder::new(&mut writer, scale_bits as u32);
    enc.finalize(data, &esyms).map_err(|_| {
        AppError::Codec(CodecError {
            detail: "interleaved encode failed".into(),
            codec_id: Some(codec::ids::BYTE_INTERLEAVED2),
        })
    })?;
    Ok(writer.encoded().to_vec())
}

fn encode_r64_single(
    data: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u8,
) -> Result<Vec<u8>, AppError> {
    let mut buf = vec![0u8; data.len().saturating_mul(8).saturating_add(64)];
    let mut writer = BackwardWord32Writer::new(&mut buf);
    let mut state = Rans64State::new();
    for &s in data.iter().rev() {
        let sym = Rans64EncSymbol::new(
            model.cumulative[s as usize],
            model.frequencies[s as usize],
            scale_bits as u32,
        )
        .map_err(|e| AppError::Model(e))?;
        rans64_enc_put_symbol(&mut state, &mut writer, &sym).map_err(|_| {
            AppError::Codec(CodecError {
                detail: "R64 encode buffer exhausted".into(),
                codec_id: Some(codec::ids::R64_SINGLE),
            })
        })?;
    }
    rans64_enc_flush(&state, &mut writer).map_err(|_| {
        AppError::Codec(CodecError {
            detail: "R64 encode flush failed".into(),
            codec_id: Some(codec::ids::R64_SINGLE),
        })
    })?;
    Ok(writer.encoded().to_vec())
}

fn encode_word_single(
    data: &[u8],
    model: &crate::container::model::FrequencyModel,
    scale_bits: u8,
) -> Result<Vec<u8>, AppError> {
    let mut buf = vec![0u8; data.len().saturating_mul(4).saturating_add(64)];
    let mut writer = BackwardWord16Writer::new(&mut buf);
    let mut state = RansWordState::new();
    for &s in data.iter().rev() {
        rans_word_enc_put(
            &mut state,
            &mut writer,
            model.cumulative[s as usize],
            model.frequencies[s as usize],
            scale_bits as u32,
        )
        .map_err(|_| {
            AppError::Codec(CodecError {
                detail: "word encode buffer exhausted".into(),
                codec_id: Some(codec::ids::WORD_SINGLE),
            })
        })?;
    }
    rans_word_enc_flush(&state, &mut writer).map_err(|_| {
        AppError::Codec(CodecError {
            detail: "word encode flush failed".into(),
            codec_id: Some(codec::ids::WORD_SINGLE),
        })
    })?;
    Ok(writer.encoded().to_vec())
}
