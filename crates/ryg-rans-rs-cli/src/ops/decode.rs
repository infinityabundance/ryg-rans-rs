//! # `ryg-rans decode` — decode and verify a RYGRANS v1 container
//!
//! Decoded bytes are streamed to the output writer as blocks arrive, so
//! memory use stays bounded by one block.  Every hash (payload, decoded,
//! container, decoded-stream) is verified before success; any mismatch is an
//! integrity error (exit 5).

use crate::container::reader::ContainerReader;
use crate::error::AppError;
use crate::limits::Limits;
use crate::ops::{decode_block, open_input, open_output};
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
    let _backend = matches
        .get_one::<String>("backend")
        .map(String::as_str)
        .unwrap_or("auto");
    let force = matches.get_flag("force");
    let force_tty = matches.get_flag("force-tty");

    let limits = Limits::default();
    let input = open_input(input_path, limits.max_input_bytes)?;
    let mut output = open_output(output_path, force, force_tty)?;

    let mut cr = ContainerReader::new(BufReader::new(input), limits);
    let _header = cr.read_header()?;
    let mut blocks: u64 = 0;
    let mut total_uncompressed: u64 = 0;
    loop {
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
