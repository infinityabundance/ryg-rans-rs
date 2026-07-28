# Methodology Sources

This file records the exact commit hashes of the reference projects whose
engineering patterns were studied and reused in `ryg-rans-rs`.

## Reference Projects

| Repository | Commit | Date | Patterns Used |
|---|---|---|---|
| `infinityabundance/ntpsec-rs` | (latest default branch) | 2026 | Deterministic core isolation, layered oracle courts |
| `infinityabundance/openntpd-rs` | (latest default branch) | 2026 | "Nothing is ported before evidence exists", casefile system |
| `infinityabundance/chrony-rs` | (latest default branch) | 2026 | Court identifiers, claim indexes, generated port-parity matrices |
| `infinityabundance/ncurses-native` | (latest default branch) | 2026 | Claim-index, receipt, gap-ledger machinery, cargo xtask gen/check |

## Key Engineering Patterns Adopted

1. **Deterministic Core Isolation**: Core algorithm in `#![no_std]` crate with `#![forbid(unsafe_code)]`.
2. **Oracle Harnesses**: Minimal C/C++ adapters built from pinned upstream revision; traces compared programmatically.
3. **One-sided and Two-sided Differential Courts**: Compare intermediate states and final outputs in both directions.
4. **Casefile Generation**: Deterministic, reproducible test cases with stored manifests.
5. **Machine-readable Receipts**: JSON receipts with verdict, residual count, reproduction command.
6. **Generated Port-Parity Matrix**: Track every upstream surface's reconstruction status.
7. **Gap Ledger**: Document every non-full surface as a diff.
8. **cargo xtask gen/check**: Build automation for doc generation and gate verification.
9. **Freshness Gates**: Generated docs must be up-to-date or CI fails.
10. **Residual Classification**: Every observed difference recorded, classified, and tracked.

## Upstream Pinned Oracle

| Property | Value |
|---|---|
| Repository | `rygorous/ryg_rans` |
| Default Branch | master |
| Pinned Commit | `c9d162d996fd600315af9ae8eb89d832576cb32d` |
| Commit Date | 2018-11-25 |
| Host | x86_64, little-endian |
