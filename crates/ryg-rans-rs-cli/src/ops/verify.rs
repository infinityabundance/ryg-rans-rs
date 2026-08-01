//! # `ryg-rans verify` — full integrity verification without output
//!
//! Walks the container exactly like `decode` (same dispatcher, same hash
//! checks) but discards decoded bytes.  Exits 0 on success, 5 on any
//! integrity failure, and reports a per-block summary.

use crate::error::AppError;
use crate::limits::Limits;
use crate::ops::{decode_block, open_input, parse_timeout};
use clap::ArgMatches;

/// Execute `verify`.
pub fn run(matches: &ArgMatches) -> Result<(), AppError> {
    let input_path = matches
        .get_one::<String>("input")
        .map(String::as_str)
        .unwrap_or("-");
    let fmt = matches
        .get_one::<String>("output-format")
        .map(String::as_str)
        .unwrap_or("human");
    let backend = matches
        .get_one::<String>("backend")
        .map(String::as_str)
        .unwrap_or("auto");
    let timeout_secs = parse_timeout(matches)?;
    // Cooperative cancellation: SIGINT/SIGTERM handlers + optional timeout
    // watchdog; the block loop polls so cancellation returns the typed
    // Cancelled error (exit 11) instead of a hard kill.
    let _cancel = crate::signal::CancellationGuard::install(timeout_secs);
    // Same no-silent-fallback rule as `decode`: only the auto dispatcher is
    // implemented in the CLI verifier.
    if backend != "auto" && backend != "all-available" {
        return Err(AppError::Unsupported(crate::error::UnsupportedError {
            detail: format!(
                "explicit backend '{}' not implemented in the CLI verifier; the auto dispatcher is used",
                backend
            ),
        }));
    }

    let limits = Limits::default();
    let input = open_input(input_path, limits.max_input_bytes)?;
    let mut cr =
        crate::container::reader::ContainerReader::new(std::io::BufReader::new(input), limits);
    let header = cr.read_header()?;

    let mut blocks: u64 = 0;
    let mut bytes: u64 = 0;
    loop {
        // Poll once per block so SIGINT/SIGTERM/timeout surfaces as the typed
        // Cancelled error at a block boundary.
        crate::signal::CancellationGuard::check()?;
        match cr.read_block(|info, model_data, payload| decode_block(info, model_data, payload))? {
            Some((_block, decoded)) => {
                blocks += 1;
                bytes += decoded.len() as u64;
            }
            None => break,
        }
    }
    let footer = cr.read_footer()?;
    cr.check_trailing_data()?;

    if fmt == "json" {
        let json = serde_json::json!({
            "verified": true,
            "format": "rygrans-v1",
            "block_count": blocks,
            "total_uncompressed": bytes,
            "container_sha256": hex::encode(footer.container_sha256),
            "decoded_stream_sha256": hex::encode(footer.decoded_stream_sha256),
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
            "OK: {} block(s), {} bytes, all payload/decoded/container hashes verified (codec {})",
            blocks,
            bytes,
            crate::container::codec::codec_name(header.default_codec_id)
        );
    }
    Ok(())
}
