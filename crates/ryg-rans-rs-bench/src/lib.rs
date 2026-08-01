//! # ryg-rans-rs-bench — Criterion Benchmark Suite
//!
//! This crate provides the Criterion benchmark infrastructure across all
//! execution tiers: scalar, SSE4.1, AVX2, AVX-512, specialized, batched,
//! parallel, container, and dispatch.
//!
//! ## Module organization
//!
//! ```text
//! ryg-rans-rs-bench/
//! ├── benches/               # Criterion benchmark binaries (one per tier)
//! │   ├── scalar.rs          # Scalar Word rANS
//! │   ├── sse41.rs           # SSE4.1 4-way
//! │   ├── avx2.rs            # AVX2 8-way / 2×8-on-16
//! │   ├── avx512.rs          # AVX512VL 8-way / AVX512 16-way / 2×8-on-16 / batch
//! │   ├── specialized.rs     # Single-profile direct-coded tables
//! │   ├── parallel.rs        # Multi-threaded decode (rayon)
//! │   └── container.rs       # Container-level round-trip benchmarks
//! ├── src/
//! │   ├── lib.rs             # Crate root — re-exports common + exporter
//! │   ├── exporter.rs        # JSON/CSV structured summary export
//! │   └── common/
//! │       ├── mod.rs         # Module root
//! │       ├── corpus.rs      # Deterministic benchmark corpora (8 model profiles)
//! │       ├── verification.rs# Backend verification against scalar reference
//! │       └── metadata.rs    # System metadata (CPU, commit, rustc version)
//! ```
//!
//! ## How to run
//!
//! ```bash
//! # All benchmarks
//! cargo bench -p ryg-rans-rs-bench
//!
//! # Specific tier
//! cargo bench -p ryg-rans-rs-bench --bench scalar
//! cargo bench -p ryg-rans-rs-bench --bench avx2
//!
//! # With native CPU features (required for AVX2/AVX512)
//! RUSTFLAGS="-C target-cpu=native" cargo bench -p ryg-rans-rs-bench --bench avx2
//!
//! # Save/compare baselines across git revisions
//! cargo bench -p ryg-rans-rs-bench --bench avx2 -- --save-baseline phase-j-avx2
//! cargo bench -p ryg-rans-rs-bench --bench avx2 -- --baseline phase-j-avx2
//!
//! # With structured export (JSON + CSV written to artifacts/)
//! cargo bench -p ryg-rans-rs-bench --bench scalar -- --quiet \
//!   && cargo run -p ryg-rans-rs-bench --bin export -- target/criterion artifacts/
//! ```
//!
//! ## Benchmark structure
//!
//! Every benchmark binary follows the same pattern:
//!
//! 1. **Verification**: Before registering any Criterion group, the binary runs
//!    each backend against the scalar reference via `common::verification::*`.
//!    If verification fails, the process panics immediately — no timing data
//!    is collected for an incorrect backend.
//! 2. **Corpus generation**: Each `Corpus` is generated from a `(profile, seed,
//!    length)` tuple.  The same tuple always produces identical data, frequencies,
//!    and compressed streams (determinism guaranteed by `StdRng::seed_from_u64`).
//! 3. **Criterion groups**: One group per `(backend, api, profile, length)`
//!    combination, using `Criterion::throughput()` for GiB/s reporting.
//! 4. **Baseline comparison**: Results can be saved/baselined for regression
//!    detection across commits.
//!
//! ## Execution tiers
//!
//! | Benchmark | Backends | Stream format |
//! |-----------|----------|---------------|
//! | `scalar` | Scalar (division, packed table) | 8-way, 16-way |
//! | `sse41` | SSE4.1 packed-table decode | 4-way |
//! | `avx2` | AVX2 HW gather, AVX2 manual gather, 2×8-on-16 | 8-way, 16-way |
//! | `avx512` | AVX512VL 8-way, AVX512 16-way, 2×8-on-16, batch | 8-way, 16-way |
//! | `specialized` | Direct-coded tables for specific model profiles | 16-way |
//! | `parallel` | Rayon-based multi-stream decode | 16-way |
//! | `container` | Container-level encode→flush→decode→verify round-trip | 8-way |
//!
//! ## Structured export
//!
//! After benchmarks complete, `exporter::load_criterion_estimates` walks the
//! `target/criterion` directory tree and produces two files:
//! - `results.json`: Full structured summary with all `BenchRecord` fields.
//! - `results.csv`: Tabular summary for spreadsheet/analysis tools.
//! The exporter also computes a SHA-256 hash of the JSON content for integrity
//! tracking in CI pipelines.

pub mod common;
pub mod courts;
pub mod exporter;
