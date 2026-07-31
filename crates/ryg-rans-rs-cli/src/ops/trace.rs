//! # `ryg-rans trace` — per-symbol state-transition trace
//!
//! Verifies the whole container (same walk as `decode`), then re-decodes the
//! selected block symbol-by-symbol, emitting each symbol with the decoder
//! state before and after the step.  Implemented for the single-state byte
//! rANS codec; other codecs return a typed unsupported error.

use crate::container::block::Block;
use crate::container::codec;
use crate::error::AppError;
use crate::limits::Limits;
use crate::ops::{build_cum2sym, decode_block, open_input};
use clap::ArgMatches;
use ryg_rans_rs_core::{
    ByteReader, RansByteDecSymbol, rans_byte_dec_advance_symbol, rans_byte_dec_get,
    rans_byte_dec_init,
};

/// Execute `trace`.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let block_index: u64 = matches
        .get_one::<String>("block")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(crate::error::FormatError {
                    detail: format!("invalid block index '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(0);
    let max_symbols: usize = matches
        .get_one::<String>("max-symbols")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(crate::error::FormatError {
                    detail: format!("invalid max-symbols '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(256);
    let fmt = matches
        .get_one::<String>("output-format")
        .map(String::as_str)
        .unwrap_or("text");

    let limits = Limits::default();
    let input = open_input(input_path, limits.max_input_bytes)?;
    let mut cr =
        crate::container::reader::ContainerReader::new(std::io::BufReader::new(input), limits);
    let _header = cr.read_header()?;

    // Walk every block (full verification); keep only the target block.
    let mut target: Option<Block> = None;
    let mut seen: u64 = 0;
    loop {
        match cr.read_block(|info, model_data, payload| decode_block(info, model_data, payload))? {
            Some((block, _decoded)) => {
                if seen == block_index {
                    target = Some(block);
                }
                seen += 1;
            }
            None => break,
        }
    }
    let _footer = cr.read_footer()?;
    cr.check_trailing_data()?;

    let block = target.ok_or_else(|| {
        AppError::Format(crate::error::FormatError {
            detail: format!(
                "block {} not found (container has {} blocks)",
                block_index, seen
            ),
            block_index: Some(block_index),
            offset: None,
        })
    })?;

    if block.block_kind != crate::container::BLOCK_KIND_RANS {
        return Err(AppError::Unsupported(crate::error::UnsupportedError {
            detail: format!(
                "trace supports rANS blocks only; block {} is {}",
                block_index,
                match block.block_kind {
                    crate::container::BLOCK_KIND_RAW => "raw",
                    crate::container::BLOCK_KIND_RLE => "rle",
                    _ => "unknown",
                }
            ),
        }));
    }
    if block.codec_id != codec::ids::BYTE_SINGLE {
        return Err(AppError::Unsupported(crate::error::UnsupportedError {
            detail: format!(
                "trace supports byte-single (codec 1) only; block uses codec {}",
                block.codec_id
            ),
        }));
    }

    let model =
        crate::container::model::FrequencyModel::from_bytes(&block.model_data, block.scale_bits)?;
    let scale_bits = block.scale_bits as u32;
    let cum2sym = build_cum2sym(&model);
    let len = block.uncompressed_length as usize;

    let mut reader = ByteReader::new(&block.payload);
    let mut state = rans_byte_dec_init(&mut reader).map_err(|_| {
        AppError::Format(crate::error::FormatError {
            detail: "truncated stream (init)".into(),
            block_index: Some(block_index),
            offset: None,
        })
    })?;
    let n = len.min(max_symbols);
    let mut out = String::new();
    for i in 0..n {
        let before = state.get();
        let cf = rans_byte_dec_get(&state, scale_bits) as usize;
        let s = *cum2sym.get(cf).ok_or_else(|| {
            AppError::Codec(crate::error::CodecError {
                detail: format!("cumulative slot {} out of range", cf),
                codec_id: Some(codec::ids::BYTE_SINGLE),
            })
        })?;
        let dsym =
            RansByteDecSymbol::new(model.cumulative[s as usize], model.frequencies[s as usize])
                .map_err(|e| AppError::Model(e))?;
        rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits).map_err(|_| {
            AppError::Format(crate::error::FormatError {
                detail: "truncated stream during trace".into(),
                block_index: Some(block_index),
                offset: None,
            })
        })?;
        let after = state.get();
        if fmt == "jsonl" {
            out.push_str(&format!(
                "{{\"step\":{},\"symbol\":{},\"state_before\":{},\"state_after\":{},\"cumulative\":{}}}\n",
                i, s, before, after, cf
            ));
        } else {
            out.push_str(&format!(
                "step {:>6}  symbol {:>3}  state {:010x} -> {:010x}\n",
                i, s, before, after
            ));
        }
    }
    print!("{}", out);
    Ok(())
}
