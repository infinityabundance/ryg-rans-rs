//! # `ryg-rans model` — deterministic model build / inspect / validate / compare
//!
//! The model format is the canonical sparse encoding defined in
//! `container::model` (2-byte entry count + 5-byte (symbol, u32 freq)
//! entries).  `build` normalises a raw histogram to exactly `2^scale_bits`
//! total using the deterministic integer-only algorithm; the same bytes are
//! stored in containers, so a model built here is byte-identical to the one
//! a container carries for the same data.

use crate::container::model::FrequencyModel;
use crate::error::{AppError, ComparisonError, FormatError};
use crate::limits::Limits;
use crate::ops::{open_input, sha256_hex};
use clap::ArgMatches;
use std::io::Read;

/// Execute `model` and its subcommands.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    match matches.subcommand() {
        Some(("build", m)) => build(m),
        Some(("inspect", m)) => inspect(m),
        Some(("validate", m)) => validate(m),
        Some(("compare", m)) => compare(m),
        _ => Err(AppError::InternalInvariant(
            crate::error::InternalInvariantError {
                detail: "model subcommand required".into(),
            },
        )),
    }
}

fn build(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let scale_bits: u8 = matches
        .get_one::<String>("scale-bits")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(FormatError {
                    detail: format!("invalid scale-bits '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(crate::ops::DEFAULT_SCALE_BITS);
    let output_path = matches
        .get_one::<String>("output")
        .map(String::as_str)
        .unwrap_or("-");
    let fmt = matches
        .get_one::<String>("output-format")
        .map(String::as_str)
        .unwrap_or("json");

    if scale_bits == 0 || scale_bits > 31 {
        return Err(AppError::Format(FormatError {
            detail: format!("scale-bits must be in 1..=31, got {}", scale_bits),
            block_index: None,
            offset: None,
        }));
    }

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
        return Err(AppError::Format(FormatError {
            detail: "empty input: cannot build a model".into(),
            block_index: None,
            offset: None,
        }));
    }

    let mut hist = [0u64; 256];
    for &b in &data {
        hist[b as usize] += 1;
    }
    let model = FrequencyModel::build(&hist, scale_bits)?;

    match fmt {
        "binary" => {
            let bytes = model.to_bytes();
            let mut out = crate::ops::open_output(output_path, true, true)?;
            out.write_all(&bytes).map_err(|e| {
                AppError::Io(crate::error::IoError {
                    path: Some(output_path.into()),
                    detail: format!("write model: {}", e),
                })
            })?;
        }
        "json" => {
            let json = model_json(&model);
            let s = serde_json::to_string_pretty(&json).map_err(|e| {
                AppError::InternalInvariant(crate::error::InternalInvariantError {
                    detail: format!("serialize json: {}", e),
                })
            })?;
            if output_path == "-" {
                println!("{}", s);
            } else {
                std::fs::write(output_path, s).map_err(|e| {
                    AppError::Io(crate::error::IoError {
                        path: Some(output_path.into()),
                        detail: format!("write model: {}", e),
                    })
                })?;
            }
        }
        other => {
            return Err(AppError::Format(FormatError {
                detail: format!("unknown output format '{}'", other),
                block_index: None,
                offset: None,
            }));
        }
    }
    Ok(())
}

fn inspect(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let scale_bits: u8 = matches
        .get_one::<String>("scale-bits")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(FormatError {
                    detail: format!("invalid scale-bits '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(crate::ops::DEFAULT_SCALE_BITS);

    let bytes = read_model_file(input_path)?;
    let model = FrequencyModel::from_bytes(&bytes, scale_bits)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&model_json(&model)).map_err(|e| {
            AppError::InternalInvariant(crate::error::InternalInvariantError {
                detail: format!("serialize json: {}", e),
            })
        })?
    );
    Ok(())
}

fn validate(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let scale_bits: u8 = matches
        .get_one::<String>("scale-bits")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(FormatError {
                    detail: format!("invalid scale-bits '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(crate::ops::DEFAULT_SCALE_BITS);

    let bytes = read_model_file(input_path)?;
    let model = FrequencyModel::from_bytes(&bytes, scale_bits)?;
    let total: u64 = model.frequencies.iter().map(|&f| f as u64).sum();
    let expected = 1u64 << scale_bits;
    if total != expected {
        return Err(AppError::Format(FormatError {
            detail: format!("frequency sum {} != target {}", total, expected),
            block_index: None,
            offset: None,
        }));
    }
    println!(
        "OK: {} active symbols, sum {}, sha256 {}",
        model.active_symbols,
        total,
        sha256_hex(&bytes)
    );
    Ok(())
}

fn compare(matches: &ArgMatches) -> Result<(), AppError> {
    let a = matches
        .get_one::<String>("a")
        .map(String::as_str)
        .ok_or_else(|| {
            AppError::InternalInvariant(crate::error::InternalInvariantError {
                detail: "compare requires --a and --b".into(),
            })
        })?;
    let b = matches
        .get_one::<String>("b")
        .map(String::as_str)
        .ok_or_else(|| {
            AppError::InternalInvariant(crate::error::InternalInvariantError {
                detail: "compare requires --a and --b".into(),
            })
        })?;
    let scale_bits: u8 = matches
        .get_one::<String>("scale-bits")
        .map(|s| {
            s.parse().map_err(|_| {
                AppError::Format(FormatError {
                    detail: format!("invalid scale-bits '{}'", s),
                    block_index: None,
                    offset: None,
                })
            })
        })
        .transpose()?
        .unwrap_or(crate::ops::DEFAULT_SCALE_BITS);

    let bytes_a = read_model_file(a)?;
    let bytes_b = read_model_file(b)?;
    let ma = FrequencyModel::from_bytes(&bytes_a, scale_bits)?;
    let mb = FrequencyModel::from_bytes(&bytes_b, scale_bits)?;
    if ma.frequencies == mb.frequencies {
        println!("models are identical (sha256 {})", sha256_hex(&bytes_a));
        Ok(())
    } else {
        Err(AppError::Comparison(ComparisonError {
            detail: "models differ".into(),
        }))
    }
}

fn read_model_file(path: &str) -> Result<Vec<u8>, AppError> {
    let limits = Limits::default();
    let mut input = open_input(path, limits.max_model_bytes as u64)?;
    let mut bytes = Vec::new();
    input.read_to_end(&mut bytes).map_err(|e| {
        AppError::Io(crate::error::IoError {
            path: Some(path.into()),
            detail: format!("read model: {}", e),
        })
    })?;
    Ok(bytes)
}

fn model_json(model: &FrequencyModel) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = (0..256usize)
        .filter(|&s| model.frequencies[s] > 0)
        .map(|s| {
            serde_json::json!({
                "symbol": s,
                "frequency": model.frequencies[s],
                "start": model.cumulative[s],
            })
        })
        .collect();
    serde_json::json!({
        "scale_bits": model.scale_bits,
        "active_symbols": model.active_symbols,
        "total": 1u64 << model.scale_bits,
        "sha256": sha256_hex(&model.to_bytes()),
        "entries": entries,
    })
}
