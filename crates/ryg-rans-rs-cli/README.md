# ryg-rans-rs-cli

> **CLI tools for rANS encoding, decoding, inspection, tracing, comparison, and benchmarking.**  
> Built on the `ryg-rans-rs` facade and `ryg-rans-rs-core` crate.  
> Uses `clap` for subcommand parsing.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-cli)](https://crates.io/crates/ryg-rans-rs-cli)

---

## Purpose

This crate provides command-line access to all ryg-rans-rs functionality. It is
designed for:
- **Manual testing** and debugging during development
- **Performance measurement** of specific codec configurations
- **Stream inspection** to verify format correctness
- **Comparison** between Rust and C/C++ compressed streams

---

## Planned Commands

| Command | Description | Status |
|---------|-------------|--------|
| `encode` | Encode a file using a specified frequency model | Scaffold |
| `decode` | Decode a rANS stream | Scaffold |
| `inspect` | Inspect a rANS stream's internal state | Scaffold |
| `bench` | Run performance benchmarks | Scaffold |
| `trace` | Trace individual symbol transitions | Scaffold |
| `compare` | Compare Rust and C/C++ compressed streams | Scaffold |
| `courts` | Run oracle courts and generate evidence | Use oracle instead |

The `clap` derive infrastructure is in place, but subcommand implementations are
not yet wired to the algorithmic backends. For immediate functional use:

### For evidence generation

```sh
cargo run -p ryg-rans-rs-oracle -- oracle/adapter/rans_trace 12 42 20
```

### For performance measurement

```sh
cargo run --release -p ryg-rans-rs-oracle --bin perf -- oracle/adapter/rans_trace
```

### For programmatic use

```rust
use ryg_rans_rs::byte::*;
// See the facade crate README for examples
```

---

## Related Crates

- **[`ryg-rans-rs-core`](https://crates.io/crates/ryg-rans-rs-core)** — Deterministic algorithmic core
- **[`ryg-rans-rs-simd`](https://crates.io/crates/ryg-rans-rs-simd)** — SSE4.1 + AVX-512 decode kernels
- **[`ryg-rans-rs`](https://crates.io/crates/ryg-rans-rs)** — Public facade
- **[`ryg-rans-rs-oracle`](https://crates.io/crates/ryg-rans-rs-oracle)** — Forensic court harness & perf benchmarks
- **[`ryg-rans-rs-casefile`](https://crates.io/crates/ryg-rans-rs-casefile)** — Typed evidence schemas
