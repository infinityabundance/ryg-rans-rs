//! # ryg-rans-rs-cli — the ryg-rans rANS compression CLI
//!
//! This library implements the `ryg-rans` command-line tool for encoding,
//! decoding, inspecting, verifying, comparing, and benchmarking rANS
//! compressed data using the versioned RYGRANS container format.
//!
//! ## Safety
//!
//! This crate uses `#![forbid(unsafe_code)]`.  All SIMD acceleration is
//! accessed through safe facade APIs that perform runtime feature detection.

#![cfg_attr(not(feature = "signals"), forbid(unsafe_code))]

// The `signals` feature installs SIGINT/SIGTERM handlers via `libc::signal`
// (async-signal-safe atomic-flag handlers, documented in `signal.rs`).  That
// feature is the only place `unsafe` appears in this crate, mirroring how the
// parallel crate gates its libc affinity calls behind the `affinity` feature.
// Default builds (signals on) therefore carry the minimal unsafe surface;
// builds with `--no-default-features` stay fully `forbid(unsafe_code)`.

use clap::{Arg, Command};
use error::AppError;
use std::io::{Read, Write};

pub mod container;
pub mod error;
pub mod exit;
pub mod limits;
pub mod ops;
pub mod signal;

/// Run the CLI with the given arguments.
///
/// Returns the stable exit code (0 on success, 2–11 per the documented
/// table).  The stdio streams are bound inside the operations (the binary
/// passes real stdio); integration tests exercise the compiled binary via
/// `CARGO_BIN_EXE_ryg-rans` so exit codes and stream behaviour are tested
/// end-to-end.
pub fn run(
    args: impl IntoIterator<Item = std::ffi::OsString>,
    _stdin: &mut dyn Read,
    _stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    match cli_entry(args) {
        Ok(()) => exit::codes::SUCCESS,
        Err(e) => {
            let code = exit::error_to_exit_code(&e);
            let _ = writeln!(stderr, "error: {}", e);
            code
        }
    }
}

fn cli_entry<I>(args: I) -> Result<(), error::AppError>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    let app = build_cli();
    let matches = app.try_get_matches_from(args).map_err(|e| {
        AppError::Io(error::IoError {
            path: None,
            detail: e.to_string(),
        })
    })?;

    match matches.subcommand() {
        Some(("encode", m)) => ops::encode::run(m),
        Some(("decode", m)) => ops::decode::run(m),
        Some(("inspect", m)) => ops::inspect::run(m),
        Some(("verify", m)) => ops::verify::run(m),
        Some(("model", m)) => ops::model::run(m),
        Some(("trace", m)) => ops::trace::run(m),
        Some(("compare", m)) => ops::compare::run(m),
        Some(("bench", m)) => ops::bench::run(m),
        Some(("capabilities", _m)) => print_capabilities(),
        Some(("completions", m)) => {
            let shell = m
                .get_one::<String>("shell")
                .map(|s| s.as_str())
                .unwrap_or("bash");
            let mut app = build_cli();
            let mut stdout = std::io::stdout();
            completions(shell, &mut app, &mut stdout)?;
            Ok(())
        }
        _ => {
            // Print help
            let _ = print_help();
            Ok(())
        }
    }
}

fn print_help() -> Result<(), error::AppError> {
    build_cli().print_long_help().map_err(|e| {
        error::AppError::Io(error::IoError {
            path: None,
            detail: e.to_string(),
        })
    })
}

fn print_capabilities() -> Result<(), error::AppError> {
    let json = serde_json::json!({
        "schema_version": 1,
        "command": "capabilities",
        "success": true,
        "tool_version": env!("CARGO_PKG_VERSION"),
        "container_versions": ["1.0"],
        "supported_codecs": [
            {"id": 1, "name": "BYTE_SINGLE"},
            {"id": 2, "name": "BYTE_INTERLEAVED2"},
            {"id": 3, "name": "R64_SINGLE"},
            {"id": 4, "name": "R64_INTERLEAVED2"},
            {"id": 5, "name": "WORD_SINGLE"},
            {"id": 6, "name": "WORD_INTERLEAVED2"},
            {"id": 7, "name": "WORD_INTERLEAVED8"},
            {"id": 8, "name": "WORD_INTERLEAVED16"},
            {"id": 9, "name": "ALIAS_SINGLE"},
            {"id": 10, "name": "ALIAS_INTERLEAVED2"},
        ],
        "supported_backends": [
            {"name": "scalar-8way", "available": true},
            {"name": "sse41-8way", "available": cfg!(target_feature = "sse4.1")},
            {"name": "avx512vl-8way", "available": cfg!(all(
                target_feature = "avx512f",
                target_feature = "avx512vl",
                target_feature = "avx512bw"
            ))},
            {"name": "scalar-16way", "available": true},
            {"name": "avx512-16way", "available": cfg!(all(
                target_feature = "avx512f",
                target_feature = "avx512bw"
            ))},
        ],
        "default_codec": "byte-interleaved2",
        "default_block_size": 1048576,
        "default_scale_bits": 12,
        "msrv": "1.85",
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&json).map_err(|e| {
            error::AppError::Io(error::IoError {
                path: None,
                detail: e.to_string(),
            })
        })?
    );
    Ok(())
}

fn build_cli() -> Command {
    Command::new("ryg-rans")
        .version(env!("CARGO_PKG_VERSION"))
        .about("rANS entropy coding tool — encode, decode, inspect, verify, compare, benchmark")
        .subcommand(
            Command::new("encode")
                .about("Encode input into a versioned .rygr container")
                .arg(
                    Arg::new("input")
                        .short('i')
                        .long("input")
                        .value_name("PATH")
                        .help("Input file path (use '-' for stdin)")
                        .default_value("-"),
                )
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("PATH")
                        .help("Output .rygr file path (use '-' for stdout)")
                        .default_value("-"),
                )
                .arg(
                    Arg::new("codec")
                        .long("codec")
                        .value_name("CODEC")
                        .help("Codec format")
                        .default_value("byte-interleaved2"),
                )
                .arg(
                    Arg::new("model")
                        .long("model")
                        .value_name("MODE")
                        .help("Model mode: per-block, global, uniform, external")
                        .default_value("per-block"),
                )
                .arg(
                    Arg::new("scale-bits")
                        .long("scale-bits")
                        .value_name("N")
                        .help("Frequency scale bits")
                        .default_value("12"),
                )
                .arg(
                    Arg::new("block-size")
                        .long("block-size")
                        .value_name("SIZE")
                        .help("Block size (e.g. 1MiB)")
                        .default_value("1MiB"),
                )
                .arg(
                    Arg::new("arithmetic")
                        .long("arithmetic")
                        .value_name("PATH")
                        .help("Arithmetic: division, reciprocal, auto")
                        .default_value("auto"),
                )
                .arg(
                    Arg::new("always-compress")
                        .long("always-compress")
                        .help("Always use RANS blocks, never RAW")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .help("Overwrite existing output file")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("force-tty")
                        .long("force-tty")
                        .help("Allow binary output to terminal")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(timeout_arg()),
        )
        .subcommand(
            Command::new("decode")
                .about("Strictly decode and verify a .rygr container")
                .arg(
                    Arg::new("input")
                        .short('i')
                        .long("input")
                        .value_name("PATH")
                        .help("Input .rygr file path (use '-' for stdin)")
                        .default_value("-"),
                )
                .arg(
                    Arg::new("output")
                        .short('o')
                        .long("output")
                        .value_name("PATH")
                        .help("Output file path (use '-' for stdout)")
                        .default_value("-"),
                )
                .arg(
                    Arg::new("backend")
                        .long("backend")
                        .value_name("BACKEND")
                        .help("Decode backend: auto, scalar, sse41, avx512vl, avx512")
                        .default_value("auto"),
                )
                .arg(
                    Arg::new("force")
                        .long("force")
                        .help("Overwrite existing output file")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("force-tty")
                        .long("force-tty")
                        .help("Allow binary output to terminal")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(timeout_arg()),
        )
        .subcommand(
            Command::new("inspect")
                .about("Inspect container structure and metadata")
                .arg(
                    Arg::new("input")
                        .short('i')
                        .long("input")
                        .value_name("PATH")
                        .help("Input .rygr file path")
                        .default_value("-"),
                )
                .arg(
                    Arg::new("output-format")
                        .long("output-format")
                        .value_name("FMT")
                        .help("Output format: human, json")
                        .default_value("human"),
                )
                .arg(
                    Arg::new("blocks")
                        .long("blocks")
                        .help("Show block details")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    Arg::new("deep")
                        .long("deep")
                        .help("Verify payload and decoded hashes")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("verify")
                .about("Fully verify without writing decoded output")
                .arg(
                    Arg::new("input")
                        .short('i')
                        .long("input")
                        .value_name("PATH")
                        .help("Input .rygr file path")
                        .default_value("-"),
                )
                .arg(
                    Arg::new("backend")
                        .long("backend")
                        .value_name("BACKEND")
                        .help("Backend: auto, scalar, all-available")
                        .default_value("auto"),
                )
                .arg(
                    Arg::new("output-format")
                        .long("output-format")
                        .value_name("FMT")
                        .help("Output format: human, json")
                        .default_value("human"),
                )
                .arg(timeout_arg()),
        )
        .subcommand(
            Command::new("model")
                .about("Build, inspect, validate, and compare models")
                .subcommand(
                    Command::new("build")
                        .about("Build a deterministic normalized model from input")
                        .arg(
                            Arg::new("input")
                                .short('i')
                                .long("input")
                                .value_name("PATH")
                                .help("Input file path")
                                .default_value("-"),
                        )
                        .arg(
                            Arg::new("scale-bits")
                                .long("scale-bits")
                                .value_name("N")
                                .help("Scale bits")
                                .default_value("12"),
                        )
                        .arg(
                            Arg::new("output")
                                .short('o')
                                .long("output")
                                .value_name("PATH")
                                .help("Output path")
                                .default_value("-"),
                        )
                        .arg(
                            Arg::new("output-format")
                                .long("output-format")
                                .value_name("FMT")
                                .help("Output format: binary, json")
                                .default_value("json"),
                        ),
                )
                .subcommand(
                    Command::new("inspect")
                        .about("Display model contents")
                        .arg(
                            Arg::new("input")
                                .short('i')
                                .long("input")
                                .value_name("PATH")
                                .help("Model file path")
                                .default_value("-"),
                        )
                        .arg(
                            Arg::new("scale-bits")
                                .long("scale-bits")
                                .value_name("N")
                                .help("Scale bits")
                                .default_value("12"),
                        ),
                )
                .subcommand(
                    Command::new("validate")
                        .about("Validate a model file")
                        .arg(
                            Arg::new("input")
                                .short('i')
                                .long("input")
                                .value_name("PATH")
                                .help("Model file path")
                                .default_value("-"),
                        )
                        .arg(
                            Arg::new("scale-bits")
                                .long("scale-bits")
                                .value_name("N")
                                .help("Scale bits")
                                .default_value("12"),
                        ),
                )
                .subcommand(
                    Command::new("compare")
                        .about("Compare two models")
                        .arg(
                            Arg::new("a")
                                .long("a")
                                .value_name("PATH")
                                .help("First model file"),
                        )
                        .arg(
                            Arg::new("b")
                                .long("b")
                                .value_name("PATH")
                                .help("Second model file"),
                        )
                        .arg(
                            Arg::new("scale-bits")
                                .long("scale-bits")
                                .value_name("N")
                                .help("Scale bits")
                                .default_value("12"),
                        ),
                ),
        )
        .subcommand(
            Command::new("trace")
                .about("Trace symbol/state transitions")
                .arg(
                    Arg::new("input")
                        .short('i')
                        .long("input")
                        .value_name("PATH")
                        .help("Input .rygr file path")
                        .default_value("-"),
                )
                .arg(
                    Arg::new("block")
                        .long("block")
                        .value_name("INDEX")
                        .help("Block index to trace")
                        .default_value("0"),
                )
                .arg(
                    Arg::new("max-symbols")
                        .long("max-symbols")
                        .value_name("N")
                        .help("Maximum symbols to trace")
                        .default_value("256"),
                )
                .arg(
                    Arg::new("output-format")
                        .long("output-format")
                        .value_name("FMT")
                        .help("Output format: text, jsonl")
                        .default_value("text"),
                ),
        )
        .subcommand(
            Command::new("compare")
                .about("Compare arithmetic paths, backends, files, or oracle")
                .subcommand(
                    Command::new("arithmetic")
                        .about("Compare division vs reciprocal encoding")
                        .arg(
                            Arg::new("input")
                                .short('i')
                                .long("input")
                                .value_name("PATH")
                                .help("Input file path")
                                .default_value("-"),
                        )
                        .arg(
                            Arg::new("scale-bits")
                                .long("scale-bits")
                                .value_name("N")
                                .help("Scale bits")
                                .default_value("12"),
                        ),
                )
                .subcommand(
                    Command::new("backends")
                        .about("Compare decode backends")
                        .arg(
                            Arg::new("input")
                                .short('i')
                                .long("input")
                                .value_name("PATH")
                                .help("Input .rygr file path (codec 7)")
                                .default_value("-"),
                        ),
                )
                .subcommand(
                    Command::new("files")
                        .about("Compare two .rygr containers")
                        .arg(
                            Arg::new("a")
                                .long("a")
                                .value_name("PATH")
                                .help("First container file"),
                        )
                        .arg(
                            Arg::new("b")
                                .long("b")
                                .value_name("PATH")
                                .help("Second container file"),
                        ),
                ),
        )
        .subcommand(
            Command::new("bench")
                .about("Benchmark production Rust codec backends")
                .arg(
                    Arg::new("codec")
                        .long("codec")
                        .value_name("CODEC")
                        .help("Codec to benchmark (byte-single, byte-interleaved2, r64-single, word-single)")
                        .default_value("byte-interleaved2"),
                )
                .arg(
                    Arg::new("size")
                        .long("size")
                        .value_name("SIZE")
                        .help("Block size")
                        .default_value("1MiB"),
                )
                .arg(
                    Arg::new("samples")
                        .long("samples")
                        .value_name("N")
                        .help("Number of samples")
                        .default_value("50"),
                )
                .arg(
                    Arg::new("output-format")
                        .long("output-format")
                        .value_name("FMT")
                        .help("Output format: human, json")
                        .default_value("human"),
                ),
        )
        .subcommand(
            Command::new("capabilities")
                .about("Show compiled and runtime-supported codecs and backends"),
        )
        .subcommand(
            Command::new("completions")
                .about("Generate shell completion scripts")
                .arg(
                    Arg::new("shell")
                        .value_name("SHELL")
                        .help("Shell type: bash, fish, zsh, powershell, elvish")
                        .default_value("bash"),
                ),
        )
}

/// Shared `--timeout` argument for long-running subcommands.
///
/// A positive value arms a watchdog that cancels the operation with the
/// typed `Cancelled` error (exit code 11) after the given number of seconds.
/// Fractional seconds are accepted (e.g. `0.5`).  `0` (the default) disables
/// the watchdog; SIGINT/SIGTERM cancellation is always installed.
fn timeout_arg() -> clap::Arg {
    clap::Arg::new("timeout")
        .long("timeout")
        .value_name("SECS")
        .help("Cancel after N seconds (fractional allowed, 0 = disabled)")
        .default_value("0")
}

/// Generate shell completions.
fn completions(shell: &str, app: &mut Command, buf: &mut dyn Write) -> Result<(), error::AppError> {
    use clap_complete::generate;
    match shell {
        "bash" => generate(clap_complete::shells::Bash, app, "ryg-rans", buf),
        "fish" => generate(clap_complete::shells::Fish, app, "ryg-rans", buf),
        "zsh" => generate(clap_complete::shells::Zsh, app, "ryg-rans", buf),
        "powershell" => generate(clap_complete::shells::PowerShell, app, "ryg-rans", buf),
        "elvish" => generate(clap_complete::shells::Elvish, app, "ryg-rans", buf),
        _ => {
            return Err(error::AppError::Io(error::IoError {
                path: None,
                detail: format!("unsupported shell: {}", shell),
            }));
        }
    }
    Ok(())
}
