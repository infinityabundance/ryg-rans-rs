//! # ryg-rans — rANS entropy coding CLI
//!
//! Thin binary entry point.  All logic lives in `ryg_rans_rs_cli::run`.
//!
//! ## Exit codes
//!
//! The documented stable exit codes (0, 2–10) are propagated verbatim:
//! `main` maps the returned code to `ExitCode` without collapsing nonzero
//! values to 1, so automation can rely on the documented semantics.
//!
//! ## Safety
//!
//! This binary uses safe public APIs only.  SIMD acceleration is accessed
//! through the facade crate's safe auto-dispatch functions.

fn main() -> std::process::ExitCode {
    let code = ryg_rans_rs_cli::run(
        std::env::args_os(),
        // Pass the unlocked streams: the operations bind real stdio
        // themselves, and holding a lock here would deadlock a second
        // `stdin().lock()` inside the operations.
        &mut std::io::stdin(),
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    // Stable exit codes are documented as 0 and 2..=10; all fit in u8.
    std::process::ExitCode::from(code.clamp(0, 255) as u8)
}
