//! # `ryg-rans inspect` — container metadata inspection
//!
//! Reads the header, every block, and the footer, then prints a human or
//! JSON summary.  With `--deep` every block is decoded so payload and
//! decoded-data hashes are verified; without it only structure is reported.

use crate::container::block::Block;
use crate::error::AppError;
use crate::limits::Limits;
use crate::ops::{decode_block, open_input};
use clap::ArgMatches;

/// Execute `inspect`.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let fmt = matches
        .get_one::<String>("output-format")
        .map(String::as_str)
        .unwrap_or("human");
    let show_blocks = matches.get_flag("blocks");
    let deep = matches.get_flag("deep");

    let limits = Limits::default();
    let input = open_input(input_path, limits.max_input_bytes)?;
    let mut cr =
        crate::container::reader::ContainerReader::new(std::io::BufReader::new(input), limits);
    let header = cr.read_header()?;

    let mut blocks: Vec<Block> = Vec::new();
    loop {
        match cr.read_block(|info, model_data, payload| decode_block(info, model_data, payload))? {
            Some((block, _decoded)) => {
                if show_blocks || deep {
                    blocks.push(block);
                }
            }
            None => break,
        }
    }
    let footer = cr.read_footer()?;
    cr.check_trailing_data()?;

    if fmt == "json" {
        let json = serde_json::json!({
            "format": "rygrans-v1",
            "major_version": header.major_version,
            "minor_version": header.minor_version,
            "flags": header.flags,
            "default_codec_id": header.default_codec_id,
            "default_codec_name": crate::container::codec::codec_name(header.default_codec_id),
            "default_scale_bits": header.default_scale_bits,
            "declared_block_size": header.declared_block_size,
            "block_count": footer.block_count,
            "total_uncompressed": footer.total_uncompressed_length,
            "total_payload": footer.total_payload_length,
            "container_sha256": hex::encode(footer.container_sha256),
            "decoded_stream_sha256": hex::encode(footer.decoded_stream_sha256),
            "deep_verified": deep,
            "blocks": if show_blocks || deep {
                blocks.iter().map(block_json).collect::<Vec<_>>()
            } else {
                Vec::new()
            },
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&json).map_err(|e| {
                AppError::InternalInvariant(crate::error::InternalInvariantError {
                    detail: format!("serialize json: {}", e),
                })
            })?
        );
    } else {
        println!(
            "RYGRANS v{}.{} container",
            header.major_version, header.minor_version
        );
        println!(
            "  default codec: {} (id {})",
            crate::container::codec::codec_name(header.default_codec_id),
            header.default_codec_id
        );
        println!("  default scale bits: {}", header.default_scale_bits);
        println!(
            "  declared block size: {} bytes",
            header.declared_block_size
        );
        println!("  blocks: {}", footer.block_count);
        println!(
            "  total uncompressed: {} bytes",
            footer.total_uncompressed_length
        );
        println!("  total payload: {} bytes", footer.total_payload_length);
        println!(
            "  container sha256: {}",
            hex::encode(footer.container_sha256)
        );
        println!(
            "  decoded stream sha256: {}",
            hex::encode(footer.decoded_stream_sha256)
        );
        if deep {
            println!("  deep verification: passed");
        }
        if show_blocks || deep {
            for b in &blocks {
                println!(
                    "  block {}: kind={} codec={} scale={} states={} uncompressed={} payload={} model={}",
                    b.block_index,
                    block_kind_name(b.block_kind),
                    crate::container::codec::codec_name(b.codec_id),
                    b.scale_bits,
                    b.state_count,
                    b.uncompressed_length,
                    b.payload.len(),
                    b.model_data.len(),
                );
            }
        }
    }
    Ok(())
}

fn block_json(b: &Block) -> serde_json::Value {
    serde_json::json!({
        "block_index": b.block_index,
        "block_kind": block_kind_name(b.block_kind),
        "codec_id": b.codec_id,
        "codec_name": crate::container::codec::codec_name(b.codec_id),
        "scale_bits": b.scale_bits,
        "state_count": b.state_count,
        "uncompressed_length": b.uncompressed_length,
        "payload_length": b.payload.len(),
        "model_length": b.model_data.len(),
        "payload_sha256": hex::encode(b.payload_sha256),
        "decoded_sha256": hex::encode(b.decoded_sha256),
    })
}

fn block_kind_name(kind: u8) -> &'static str {
    match kind {
        crate::container::BLOCK_KIND_RAW => "raw",
        crate::container::BLOCK_KIND_RLE => "rle",
        crate::container::BLOCK_KIND_RANS => "rans",
        _ => "unknown",
    }
}
