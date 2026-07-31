//! # `ryg-rans bench` — in-process codec throughput measurement
//!
//! A deliberately simple, dependency-free throughput measurement of the
//! selected codec over a synthetic 1 MiB skewed block.  It is **not** a
//! replacement for the Criterion suite (which remains the sealed
//! measurement surface); it exists so the CLI can report a live number
//! without external tooling.

use crate::error::AppError;
use crate::ops::select_block;
use clap::ArgMatches;
use std::time::Instant;

/// Execute `bench`.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    let codec_name = matches
        .get_one::<String>("codec")
        .map(String::as_str)
        .unwrap_or("byte-interleaved2");
    let size_str = matches
        .get_one::<String>("size")
        .map(String::as_str)
        .unwrap_or("1MiB");
    let samples: usize = matches
        .get_one::<String>("samples")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(crate::error::FormatError {
                    detail: format!("invalid samples '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(50);
    let fmt = matches
        .get_one::<String>("output-format")
        .map(String::as_str)
        .unwrap_or("human");

    let size = crate::limits::Limits::parse_size(size_str).map_err(|e| {
        AppError::Format(crate::error::FormatError {
            detail: format!("invalid size: {}", e),
            block_index: None,
            offset: None,
        })
    })? as usize;
    if size == 0 || size > 64 * 1024 * 1024 {
        return Err(AppError::Format(crate::error::FormatError {
            detail: "bench size must be in (0, 64MiB]".into(),
            block_index: None,
            offset: None,
        }));
    }
    if samples == 0 {
        return Err(AppError::Format(crate::error::FormatError {
            detail: "samples must be > 0".into(),
            block_index: None,
            offset: None,
        }));
    }

    let codec_id = crate::ops::codec_from_name(codec_name)?;

    // Deterministic skewed corpus: symbol 0 at 64x frequency, others rare.
    let mut data = vec![0u8; size];
    let mut x: u64 = 0x9e3779b97f4a7c15;
    for b in data.iter_mut() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        *b = if x % 64 == 0 { (x >> 8) as u8 } else { 0 };
    }

    // Preflight: one encode + decode round trip must reproduce the input.
    let choice = select_block(codec_id, crate::ops::DEFAULT_SCALE_BITS, &data, true)?;
    let (payload, model) = match choice {
        crate::ops::BlockChoice::Rans { payload, model } => (payload, model),
        _ => {
            return Err(AppError::InternalInvariant(
                crate::error::InternalInvariantError {
                    detail: "bench preflight: expected a rANS block".into(),
                },
            ));
        }
    };

    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let _ = select_block(codec_id, crate::ops::DEFAULT_SCALE_BITS, &data, true)?;
        times.push(start.elapsed());
    }
    let enc_total: u128 = times.iter().map(|t| t.as_nanos()).sum();
    let enc_ns = (enc_total / samples as u128) as f64;

    let mut times = Vec::with_capacity(samples);
    for _ in 0..samples {
        let start = Instant::now();
        let info = crate::container::block::BlockHeaderInfo {
            block_index: 0,
            block_kind: crate::container::BLOCK_KIND_RANS,
            codec_id,
            scale_bits: crate::ops::DEFAULT_SCALE_BITS,
            state_count: 1,
            uncompressed_length: data.len() as u32,
            payload_length: payload.len() as u32,
            model_length: model.len() as u32,
            payload_sha256: [0u8; 32],
            decoded_sha256: [0u8; 32],
        };
        let decoded = crate::ops::decode_block(&info, &model, &payload)?;
        if decoded != data {
            return Err(AppError::Comparison(crate::error::ComparisonError {
                detail: "bench preflight: round-trip mismatch".into(),
            }));
        }
        times.push(start.elapsed());
    }
    let dec_total: u128 = times.iter().map(|t| t.as_nanos()).sum();
    let dec_ns = (dec_total / samples as u128) as f64;

    let enc_mib = (size as f64 / enc_ns * 1e9) / (1024.0 * 1024.0);
    let dec_mib = (size as f64 / dec_ns * 1e9) / (1024.0 * 1024.0);

    if fmt == "json" {
        let json = serde_json::json!({
            "codec": codec_name,
            "size": size,
            "samples": samples,
            "encode_mib_s": enc_mib,
            "decode_mib_s": dec_mib,
            "payload_bytes": payload.len(),
            "round_trip_verified": true,
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
            "codec {}: encode {:.1} MiB/s, decode {:.1} MiB/s ({} MiB, {} samples, round trip verified, payload {} bytes)",
            codec_name,
            enc_mib,
            dec_mib,
            size / (1024 * 1024),
            samples,
            payload.len()
        );
    }
    Ok(())
}
