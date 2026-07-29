# ryg-rans-rs-cli

> CLI tools for rANS encoding, decoding, inspection, tracing, comparison, and benchmarking.  
> Built on the `ryg-rans-rs` facade and `ryg-rans-rs-core` crate.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE)
[![Crates.io](https://img.shields.io/crates/v/ryg-rans-rs-cli)](https://crates.io/crates/ryg-rans-rs-cli)

## Status

**Scaffold.** The crate compiles and is published, but user-facing subcommands are not yet implemented.

The core `clap` infrastructure is in place. The following commands are planned but not yet wired to the algorithmic backends:

| Command | Description | Status |
|---------|-------------|--------|
| `encode` | Encode a file using a specified frequency model | Scaffold |
| `decode` | Decode a rANS stream | Scaffold |
| `inspect` | Inspect a rANS stream's internal state | Scaffold |
| `bench` | Run performance benchmarks (delegates to oracle's perf binary) | Scaffold |
| `trace` | Trace individual symbol transitions | Scaffold |
| `compare` | Compare Rust and C/C++ compressed streams | Scaffold |

## Usage

```sh
# When implemented:
# cargo run -- encode --input file.txt --output file.rans
# cargo run -- decode --input file.rans --output file.txt
```

## Performance Benchmarking

For immediate throughput measurement, use the oracle crate's `perf` binary:

```sh
cd ../ryg-rans-rs-oracle
RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo run --release --bin perf
```

## Related Crates

- **[`ryg-rans-rs-core`](https://crates.io/crates/ryg-rans-rs-core)** — Deterministic algorithmic core (`no_std`, `forbid(unsafe_code)`)
- **[`ryg-rans-rs-simd`](https://crates.io/crates/ryg-rans-rs-simd)** — SSE4.1 accelerated decode kernels
- **[`ryg-rans-rs`](https://crates.io/crates/ryg-rans-rs)** — Public facade crate (re-exports core + optional SIMD)
- **[`ryg-rans-rs-oracle`](https://crates.io/crates/ryg-rans-rs-oracle)** — Forensic court harness & performance benchmarks
- **[`ryg-rans-rs-casefile`](https://crates.io/crates/ryg-rans-rs-casefile)** — Typed evidence schemas
