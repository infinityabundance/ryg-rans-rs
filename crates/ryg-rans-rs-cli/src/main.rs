//! # ryg-rans — rANS entropy coding CLI
//!
//! Thin binary entry point.  All logic lives in `ryg_rans_rs_cli::run`.
//!
//! ## Safety
//!
//! This binary uses safe public APIs only.  SIMD acceleration is accessed
//! through the facade crate's safe auto-dispatch functions.

fn main() -> std::process::ExitCode {
    let code = ryg_rans_rs_cli::run(
        std::env::args_os(),
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &mut std::io::stderr().lock(),
    );
    std::process::ExitCode::from(if code == 0 { 0u8 } else { 1u8 })
}
