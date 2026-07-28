# ryg-rans-rs-cli

> CLI tools for rANS encode, decode, inspect, trace, compare, and bench

[![MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](https://github.com/infinityabundance/ryg-rans-rs)
[![build-status](https://img.shields.io/badge/build-scaffold-lightgrey)]()

## Overview

This crate provides command-line tools for working with rANS entropy coding. It is currently a
**scaffold** — the binary compiles and prints a usage message, but the subcommands are not yet
implemented.

The CLI is designed around subcommands powered by the `clap` crate. When fully implemented, it will
serve as both a developer utility for debugging and inspecting rANS streams and a benchmarking
tool for evaluating encoding/decoding throughput.

## Planned Commands

| Command      | Status | Description |
|--------------|--------|-------------|
| `encode`     | ✗      | Encode a file using rANS with a frequency model |
| `decode`     | ✗      | Decode an rANS-compressed file back to original |
| `inspect`    | ✗      | Inspect internal details of an rANS-encoded stream |
| `trace`      | ✗      | Trace encoding/decoder state through a sequence of symbols |
| `compare`    | ✗      | Compare Rust and C/C++ output side by side (invokes oracle) |
| `bench`      | ✗      | Run micro-benchmarks for throughput and latency |

### Encode

```sh
ryg-rans-rs encode --input file.bin --output file.rans --model model.json
```

Planned options: alphabet size, scale bits, interleave count, block size, verbosity.

### Decode

```sh
ryg-rans-rs decode --input file.rans --output file.bin
```

Planned options: auto-detect scale bits and model from stream header.

### Inspect

```sh
ryg-rans-rs inspect file.rans
```

Planned output: stream metadata (scale bits, symbol count, final state), per-block statistics,
entropy analysis.

### Trace

```sh
ryg-rans-rs trace --input file.bin --model model.json
```

Planned output: per-symbol state transitions (state values, emitted bytes, cumulative
frequencies), useful for debugging encoder/decoder mismatches.

### Compare

```sh
ryg-rans-rs compare --rust file.rans --oracle ./oracle_adapter --casefiles ./cases/
```

Planned behavior: invoke the oracle harness and display side-by-side diff of residuals.

### Bench

```sh
ryg-rans-rs bench --size 1MB --iterations 100
```

Planned metrics: encode throughput (MB/s), decode throughput (MB/s), state throughput (Msym/s),
cycle counts, branch mispredictions, cache misses.

## Current Status

```
$ ryg-rans-rs
ryg-rans-rs-cli - rANS encoding tools
Usage: ryg-rans-rs encode|decode|inspect|trace|compare|bench
```

The binary currently prints a usage hint and exits. No subcommand dispatch, argument parsing,
or actual rANS logic is wired up yet.

## Cargo Features

| Feature | Default | Description |
|---------|---------|-------------|
| _(none)_ | — | The CLI crate defines no optional features |

The crate always depends on `ryg-rans-rs` (for the rANS implementation) and `clap` (for CLI
argument parsing). The `simd` and `alloc` features of `ryg-rans-rs` are inherited transitively
through the dependency.

## Development

To run the scaffold binary:

```sh
cargo run --package ryg-rans-rs-cli
```

To add a new subcommand, follow the `clap` derive pattern in `src/main.rs`:

```rust
use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Encode { /* ... */ },
    Decode { /* ... */ },
    // ...
}
```

## Dependencies

| Dependency | Version | Notes |
|------------|---------|-------|
| `ryg-rans-rs` | `0.1.0` | Public facade (rANS implementation) |
| `ryg-rans-rs-core` | `0.1.0` | Direct access to low-level primitives |
| `clap` | `4` | CLI argument parsing (`derive` feature) |
