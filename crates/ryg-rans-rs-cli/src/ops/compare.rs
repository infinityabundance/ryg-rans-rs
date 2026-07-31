//! # `ryg-rans compare` — parity comparisons
//!
//! * `compare arithmetic`: encodes the same input with the division-based
//!   reference path and the reciprocal fast path, asserting byte-identical
//!   compressed output (the Kani-proven equivalence, exercised end-to-end).
//! * `compare backends`: decodes a codec-7 (8-way) container with the scalar
//!   reference and the SIMD 8-way decoder, asserting identical output.
//! * `compare files`: decodes two containers and compares decoded-stream
//!   hashes.

use crate::container::codec;
use crate::error::{AppError, ComparisonError};
use crate::limits::Limits;
use crate::ops::open_input;
use clap::ArgMatches;
use ryg_rans_rs_core::{
    BackwardByteWriter, RansByteEncSymbol, RansByteState, rans_byte_enc_flush, rans_byte_enc_put,
    rans_byte_enc_put_symbol,
};
use std::io::Read;

/// Execute `compare` and its subcommands.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    match matches.subcommand() {
        Some(("arithmetic", m)) => arithmetic(m),
        Some(("backends", m)) => backends(m),
        Some(("files", m)) => files(m),
        _ => Err(AppError::InternalInvariant(
            crate::error::InternalInvariantError {
                detail: "compare subcommand required".into(),
            },
        )),
    }
}

fn arithmetic(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let scale_bits: u8 = matches
        .get_one::<String>("scale-bits")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(crate::error::FormatError {
                    detail: format!("invalid scale-bits '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(crate::ops::DEFAULT_SCALE_BITS);

    let limits = Limits::default();
    let mut input = open_input(input_path, limits.max_input_bytes)?;
    let mut data = Vec::new();
    input.read_to_end(&mut data).map_err(|e| {
        AppError::Io(crate::error::IoError {
            path: Some(input_path.into()),
            detail: format!("read input: {}", e),
        })
    })?;
    if data.is_empty() {
        return Err(AppError::Format(crate::error::FormatError {
            detail: "empty input".into(),
            block_index: None,
            offset: None,
        }));
    }

    let mut hist = [0u64; 256];
    for &b in &data {
        hist[b as usize] += 1;
    }
    let model = crate::container::model::FrequencyModel::build(&hist, scale_bits)?;

    // Division path.
    let div_comp = {
        let mut buf = vec![0u8; data.len().saturating_mul(4).saturating_add(64)];
        let mut writer = BackwardByteWriter::new(&mut buf);
        let mut state = RansByteState::new();
        for &s in data.iter().rev() {
            rans_byte_enc_put(
                &mut state,
                &mut writer,
                model.cumulative[s as usize],
                model.frequencies[s as usize],
                scale_bits as u32,
            )
            .map_err(|_| {
                AppError::Codec(crate::error::CodecError {
                    detail: "division encode failed".into(),
                    codec_id: Some(codec::ids::BYTE_SINGLE),
                })
            })?;
        }
        rans_byte_enc_flush(&state, &mut writer).map_err(|_| {
            AppError::Codec(crate::error::CodecError {
                detail: "division flush failed".into(),
                codec_id: Some(codec::ids::BYTE_SINGLE),
            })
        })?;
        writer.encoded().to_vec()
    };

    // Reciprocal path.
    let rec_comp = {
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
                AppError::Codec(crate::error::CodecError {
                    detail: "reciprocal encode failed".into(),
                    codec_id: Some(codec::ids::BYTE_SINGLE),
                })
            })?;
        }
        rans_byte_enc_flush(&state, &mut writer).map_err(|_| {
            AppError::Codec(crate::error::CodecError {
                detail: "reciprocal flush failed".into(),
                codec_id: Some(codec::ids::BYTE_SINGLE),
            })
        })?;
        writer.encoded().to_vec()
    };

    if div_comp != rec_comp {
        return Err(AppError::Comparison(ComparisonError {
            detail: format!(
                "division and reciprocal streams differ ({} vs {} bytes)",
                div_comp.len(),
                rec_comp.len()
            ),
        }));
    }
    println!(
        "OK: division and reciprocal encode {} identical bytes (scale_bits={})",
        div_comp.len(),
        scale_bits
    );
    Ok(())
}

fn backends(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");

    let limits = Limits::default();
    let input = open_input(input_path, limits.max_input_bytes)?;
    let mut cr =
        crate::container::reader::ContainerReader::new(std::io::BufReader::new(input), limits);
    let _header = cr.read_header()?;

    // Decode every block with the standard dispatcher (stream A).  For each
    // 8-way (codec 7) block, additionally decode with the explicit scalar
    // reference and compare against the dispatcher result (which uses the
    // SIMD kernel when compiled in).
    let mut stream_a: Vec<u8> = Vec::new();
    let mut stream_b: Vec<u8> = Vec::new();
    let mut eightway_blocks: u64 = 0;
    loop {
        match cr.read_block(|info, model_data, payload| {
            crate::ops::decode_block(info, model_data, payload)
        })? {
            Some((block, decoded)) => {
                stream_a.extend_from_slice(&decoded);
                if block.block_kind == crate::container::BLOCK_KIND_RANS
                    && block.codec_id == codec::ids::WORD_INTERLEAVED8
                {
                    let model = crate::container::model::FrequencyModel::from_bytes(
                        &block.model_data,
                        block.scale_bits,
                    )?;
                    let scalar = crate::ops::decode_word_8way_scalar_explicit(
                        block.uncompressed_length as usize,
                        &block.payload,
                        &model,
                    )?;
                    if scalar != decoded {
                        return Err(AppError::Comparison(ComparisonError {
                            detail: format!(
                                "scalar reference and dispatcher outputs differ at block {}",
                                block.block_index
                            ),
                        }));
                    }
                    stream_b.extend_from_slice(&scalar);
                    eightway_blocks += 1;
                } else {
                    stream_b.extend_from_slice(&decoded);
                }
            }
            None => break,
        }
    }
    let _footer = cr.read_footer()?;
    cr.check_trailing_data()?;

    if eightway_blocks == 0 {
        return Err(AppError::Backend(crate::error::BackendError {
            detail: "container has no 8-way (codec 7) blocks to compare".into(),
            backend: "simd-8way".into(),
        }));
    }
    if stream_a != stream_b {
        return Err(AppError::Comparison(ComparisonError {
            detail: "decoded streams differ between backends".into(),
        }));
    }
    println!(
        "OK: dispatcher (SIMD when compiled) and explicit scalar 8-way produced identical output ({} bytes, {} 8-way block(s))",
        stream_a.len(),
        eightway_blocks
    );
    Ok(())
}

fn files(matches: &ArgMatches) -> Result<(), AppError> {
    let a = matches
        .get_one::<String>("a")
        .map(String::as_str)
        .ok_or_else(|| {
            AppError::InternalInvariant(crate::error::InternalInvariantError {
                detail: "compare files requires --a and --b".into(),
            })
        })?;
    let b = matches
        .get_one::<String>("b")
        .map(String::as_str)
        .ok_or_else(|| {
            AppError::InternalInvariant(crate::error::InternalInvariantError {
                detail: "compare files requires --a and --b".into(),
            })
        })?;

    let limits = Limits::default();
    let hash_a = hash_decoded_stream(a, &limits)?;
    let hash_b = hash_decoded_stream(b, &limits)?;
    if hash_a.0 != hash_b.0 || hash_a.1 != hash_b.1 {
        return Err(AppError::Comparison(ComparisonError {
            detail: format!(
                "containers differ: A ({} bytes, sha256 {}) vs B ({} bytes, sha256 {})",
                hash_a.1, hash_a.0, hash_b.1, hash_b.0
            ),
        }));
    }
    println!(
        "OK: containers decode to identical streams ({} bytes, sha256 {})",
        hash_a.1, hash_a.0
    );
    Ok(())
}

fn hash_decoded_stream(path: &str, limits: &Limits) -> Result<(String, u64), AppError> {
    let input = open_input(path, limits.max_input_bytes)?;
    let outcome = crate::ops::walk_container(input, limits.clone(), |_b, _decoded| Ok(()))?;
    Ok((
        hex::encode(outcome.decoded_stream_sha256),
        outcome.total_uncompressed,
    ))
}
