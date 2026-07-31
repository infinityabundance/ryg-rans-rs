//! # `ryg-rans encode` — write a RYGRANS v1 container
//!
//! Streams the input in `block_size` chunks, choosing RLE / rANS / RAW per
//! block, and writes a fully hashed container.  Memory use is bounded by
//! one block plus its compressed payload and model.

use crate::container::block::Block;
use crate::container::codec;
use crate::container::footer::FileFooter;
use crate::container::header::FileHeader;
use crate::container::writer::ContainerWriter;
use crate::error::AppError;
use crate::limits::Limits;
use crate::ops::{BlockChoice, open_input, open_output, select_block};
use clap::ArgMatches;
use sha2::Digest;
use std::io::{Read, Write};

/// Execute `encode`.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let output_path = matches
        .get_one::<String>("output")
        .map(String::as_str)
        .unwrap_or("-");
    let codec_name = matches
        .get_one::<String>("codec")
        .map(String::as_str)
        .unwrap_or("byte-interleaved2");
    let model_mode = matches
        .get_one::<String>("model")
        .map(String::as_str)
        .unwrap_or("per-block");
    let scale_bits: u8 = matches
        .get_one::<String>("scale-bits")
        .map(|s| {
            s.parse().map_err(|_| {
                crate::error::AppError::Format(crate::error::FormatError {
                    detail: format!("invalid scale-bits '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(crate::ops::DEFAULT_SCALE_BITS);
    let block_size_str = matches
        .get_one::<String>("block-size")
        .map(String::as_str)
        .unwrap_or("1MiB");
    let block_size = Limits::parse_size(block_size_str).map_err(|e| {
        crate::error::AppError::Format(crate::error::FormatError {
            detail: format!("invalid block-size: {}", e),
            block_index: None,
            offset: None,
        })
    })?;
    let arithmetic = matches
        .get_one::<String>("arithmetic")
        .map(String::as_str)
        .unwrap_or("auto");
    let always_compress = matches.get_flag("always-compress");
    let force = matches.get_flag("force");
    let force_tty = matches.get_flag("force-tty");

    let limits = Limits::default();

    // Block size must fit the payload and block limits.
    let block_size = block_size.min(limits.max_block_bytes as u64) as u32;
    let block_size = block_size.min(limits.max_payload_bytes) as u32;
    if block_size == 0 {
        return Err(crate::error::AppError::ResourceLimit(
            crate::error::ResourceLimitError {
                detail: "block size is zero".into(),
                limit: 1,
                requested: 0,
            },
        ));
    }

    let codec_id = crate::ops::codec_from_name(codec_name)?;

    // Only per-block model mode is implemented; other modes are typed errors.
    if model_mode != "per-block" {
        return Err(crate::error::AppError::Unsupported(
            crate::error::UnsupportedError {
                detail: format!(
                    "model mode '{}' not implemented; use 'per-block'",
                    model_mode
                ),
            },
        ));
    }
    if arithmetic != "auto" {
        return Err(crate::error::AppError::Unsupported(
            crate::error::UnsupportedError {
                detail: format!(
                    "arithmetic path '{}' not selectable in the CLI; the reciprocal fast path is always used",
                    arithmetic
                ),
            },
        ));
    }

    let mut input = open_input(input_path, limits.max_input_bytes)?;
    let mut output = open_output(output_path, force, force_tty)?;

    let header = FileHeader {
        default_codec_id: codec_id,
        default_scale_bits: scale_bits,
        default_model_mode: 0, // per-block
        declared_block_size: block_size,
        flags: if always_compress {
            crate::container::header::flags::ALWAYS_COMPRESS
        } else {
            0
        },
        ..FileHeader::default()
    };

    let mut writer = ContainerWriter::new(&mut output);
    writer.write_header(&header)?;

    let mut block_index: u64 = 0;
    let mut total_uncompressed: u64 = 0;
    let mut chunk = vec![0u8; block_size as usize];
    loop {
        // Read one block-sized chunk (short read at EOF).
        let mut filled = 0usize;
        while filled < chunk.len() {
            let n = input.read(&mut chunk[filled..]).map_err(|e| {
                crate::error::AppError::Io(crate::error::IoError {
                    path: Some(input_path.into()),
                    detail: format!("read input: {}", e),
                })
            })?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled == 0 {
            break; // clean EOF
        }
        limits.check_block_count(block_index + 1)?;
        total_uncompressed = limits.check_output_total(total_uncompressed, filled as u64)?;

        let data = &chunk[..filled];
        let choice = select_block(codec_id, scale_bits, data, always_compress)?;

        let block = match &choice {
            BlockChoice::Raw => Block::new_raw(block_index, data.to_vec()),
            BlockChoice::Rle { symbol, count } => Block::new_rle(block_index, *symbol, *count),
            BlockChoice::Rans { payload, model } => Block::new_rans(
                block_index,
                codec_id,
                scale_bits,
                codec::codec_states(codec_id).unwrap_or(1),
                filled as u32,
                payload.clone(),
                model.clone(),
            ),
        };

        // RLE blocks carry `decoded_sha256` computed at construction from
        // the repeated data; RAW blocks carry payload==decoded.  rANS blocks
        // are constructed with a zero decoded hash ("computed by caller
        // after decode"), so the encoder computes it here from the source
        // data — the container must never carry an unset decoded hash, or
        // strict verification would (correctly) reject it.
        let decoded_for_hash = match &choice {
            BlockChoice::Rle { symbol, count } => vec![*symbol; *count as usize],
            _ => data.to_vec(),
        };
        let mut block = block;
        if matches!(choice, BlockChoice::Rans { .. }) {
            let mut h = sha2::Sha256::new();
            h.update(&decoded_for_hash);
            let hash: [u8; 32] = h.finalize().into();
            block.decoded_sha256 = hash;
        }
        writer.write_block(&block, &decoded_for_hash)?;
        block_index += 1;
    }

    let footer: FileFooter = writer.write_footer()?;
    writer.flush()?;
    drop(writer);

    if output_path != "-" {
        let _ = writeln!(
            std::io::stderr(),
            "encoded {} block(s), {} bytes uncompressed, {} bytes payload",
            footer.block_count,
            footer.total_uncompressed_length,
            footer.total_payload_length
        );
    }
    Ok(())
}
