//! # `ryg-rans decode` — decode and verify a RYGRANS v1 container
//!
//! Decoded bytes are streamed to the output writer as blocks arrive, so
//! memory use stays bounded by one block.  Every hash (payload, decoded,
//! container, decoded-stream) is verified before success; any mismatch is an
//! integrity error (exit 5).

use crate::container::reader::ContainerReader;
use crate::error::AppError;
use crate::limits::Limits;
use crate::ops::{decode_block, open_input, open_output, parse_timeout};
use clap::ArgMatches;
use std::io::{BufReader, Write};

/// Execute `decode`.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let output_path = matches
        .get_one::<String>("output")
        .map(String::as_str)
        .unwrap_or("-");
    let backend = matches
        .get_one::<String>("backend")
        .map(String::as_str)
        .unwrap_or("auto");
    // The CLI decoder is a single auto-dispatching codec walk (scalar core
    // with the SIMD 8-way kernel for codec 7 when compiled in).  An explicit
    // backend request cannot be honoured — refuse it rather than silently
    // decoding with a different backend (no silent fallback).
    if backend != "auto" {
        return Err(AppError::Unsupported(crate::error::UnsupportedError {
            detail: format!(
                "explicit backend '{}' not implemented in the CLI decoder; the auto dispatcher is used",
                backend
            ),
        }));
    }
    let force = matches.get_flag("force");
    let force_tty = matches.get_flag("force-tty");
    let timeout_secs = parse_timeout(matches)?;

    // Install SIGINT/SIGTERM handlers and (optionally) the timeout watchdog.
    // The block loop polls cancellation between blocks so a Ctrl-C or timeout
    // returns the typed Cancelled error (exit 11) instead of a hard kill.
    let _cancel = crate::signal::CancellationGuard::install(timeout_secs);

    let limits = Limits::default();
    let input = open_input(input_path, limits.max_input_bytes)?;
    let mut output = open_output(output_path, force, force_tty)?;

    let mut cr = ContainerReader::new(BufReader::new(input), limits);
    let _header = cr.read_header()?;
    let mut blocks: u64 = 0;
    let mut total_uncompressed: u64 = 0;
    loop {
        // Cooperative cancellation: polled once per block so a pending
        // SIGINT/SIGTERM/timeout is observed promptly and surfaces as the
        // typed error rather than a signal default action.
        crate::signal::CancellationGuard::check()?;
        match cr.read_block(|info, model_data, payload| decode_block(info, model_data, payload))? {
            Some((_block, decoded)) => {
                output.write_all(&decoded).map_err(|e| {
                    AppError::Io(crate::error::IoError {
                        path: Some(output_path.into()),
                        detail: format!("write output: {}", e),
                    })
                })?;
                blocks += 1;
                total_uncompressed += decoded.len() as u64;
            }
            None => break,
        }
    }
    let _footer = cr.read_footer()?;
    cr.check_trailing_data()?;
    output.flush().map_err(|e| {
        AppError::Io(crate::error::IoError {
            path: Some(output_path.into()),
            detail: format!("flush output: {}", e),
        })
    })?;

    if output_path != "-" {
        let _ = writeln!(
            std::io::stderr(),
            "decoded {} block(s), {} bytes, all hashes verified",
            blocks,
            total_uncompressed
        );
    }
    Ok(())
}
