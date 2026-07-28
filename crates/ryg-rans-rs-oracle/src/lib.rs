//! # ryg-rans-rs-oracle
//!
//! **Development-only harness for comparing Rust rANS against compiled C/C++.**
//!
//! This crate is **not intended to be shipped as a production dependency**.
//! It exists to:
//!
//! 1. Build and invoke the pinned C/C++ oracle adapter (`oracle/adapter/rans_trace`).
//! 2. Run equivalent Rust operations through `ryg-rans-rs-core`.
//! 3. Compare outputs field-by-field and produce court receipts.
//! 4. Generate residuals for every observed difference.
//!
//! ## Workflow
//!
//! 1. Build the oracle adapter: `cd oracle/adapter && make`
//! 2. Generate a deterministic casefile with known inputs and expected model.
//! 3. Run the C oracle: `./rans_trace enc-symbol-init 0 10 14` produces JSON.
//! 4. Run the Rust equivalent through this crate.
//! 5. Compare fields and produce a `Receipt` with `admitted_match` or residuals.
//!
//! ## Receipt Generation
//!
//! A receipt records:
//! - Court ID, case count, and verdict
//! - Upstream and Rust commit hashes
//! - Number of compared and matched pairs
//! - Count and IDs of any residuals
//! - Exact reproduction command
//!
//! ## Integrity
//!
//! This crate must never become a dependency of a shipped crate
//! (ryg-rans-rs-core, ryg-rans-rs-simd, ryg-rans-rs, etc.).

// Oracle harness
