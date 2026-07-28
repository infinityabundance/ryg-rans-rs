# ryg-rans-rs-oracle

> Cross-decoding court harness for ryg-rans-rs forensic parity.

## Purpose

This crate runs four deterministic courts that compare Rust rANS encoding and decoding against a compiled C/C++ oracle binary:

1. `BYTE.DIVISION` — 32-bit byte rANS via `RansEncPut`/`RansDecAdvance`
2. `BYTE.RECIPROCAL` — 32-bit byte rANS via `RansEncPutSymbol`/`RansDecAdvanceSymbol`
3. `R64.DIVISION` — 64-bit rANS via `Rans64EncPut`/`Rans64DecAdvance`
4. `R64.RECIPROCAL` — 64-bit rANS via `Rans64EncPutSymbol`/`Rans64DecAdvanceSymbol`

Each court generates deterministic casefiles with known inputs and frequency models, pipes the same inputs through the C oracle and the Rust core, compares encoded streams byte-for-byte, and cross-decodes in both directions.

## Usage

```sh
# Build the oracle adapter
cd oracle/adapter && make

# Run all four courts (20 cases each)
cargo run -p ryg-rans-rs-oracle -- oracle/adapter/rans_trace 12 42 20
```

### Environment Variables

- `RANS_EVIDENCE_DIR` — Output directory for receipts and manifests (default: `evidence/`).
- `RANS_GIT_COMMIT` — Commit hash to embed in evidence (default: `git rev-parse HEAD`).

## Evidence Output

Each court produces:

- A `Receipt` JSON file containing verdict, counts, SHA-256 chains, reproduction command.
- A `CaseManifest` JSON file containing all input cases, frequency models, C and Rust compressed streams, and per-case check results.
- An `index.json` accumulating all receipt references.

The evidence is SHA-256-chained: manifest hash is embedded in the receipt, receipt hash is embedded in the index. Self-hashes prevent undetected modification.
