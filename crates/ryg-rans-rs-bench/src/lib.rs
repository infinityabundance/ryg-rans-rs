//! # ryg-rans-rs-bench — Criterion Benchmark Suite
//!
//! This crate provides the Criterion benchmark infrastructure across all
//! execution tiers: scalar, SSE4.1, AVX2, AVX-512, specialized, batched,
//! parallel, container, and dispatch.
//!
//! ## Module organization
//!
//! Benchmarks are defined as separate binaries under `benches/`.
//! Shared infrastructure lives in the `common` module.
//!
//! ## Usage
//!
//! ```bash
//! # All benchmarks
//! cargo bench -p ryg-rans-rs-bench
//!
//! # Specific tier
//! cargo bench -p ryg-rans-rs-bench --bench scalar
//! cargo bench -p ryg-rans-rs-bench --bench avx2
//!
//! # With native CPU features
//! RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench --bench avx2
//!
//! # Save/compare baselines
//! cargo bench -p ryg-rans-rs-bench --bench avx2 -- --save-baseline phase-j-avx2
//! cargo bench -p ryg-rans-rs-bench --bench avx2 -- --baseline phase-j-avx2
//! ```

pub mod common;
pub mod exporter;
