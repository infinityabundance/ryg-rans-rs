# ADR-0006 — Strict decoded-output integrity as the default

Status: Accepted

## Context
The live decoded-hash bug (Phase L.2): the verifier computed
`decoded_hash_ok` but the aggregate failure condition ignored it, so a
block with an intact payload hash and a corrupted model decoded to wrong
output and passed verification.  The payload hash proves the compressed
bytes are intact; it cannot prove the decoded output is correct — model
corruption changes the decode without changing the payload.

## Problem
What is the default integrity contract, and how does a caller opt out?

## Alternatives considered
1. Strict: a zero/unset decoded hash fails; a mismatched decoded hash
   fails; only a matching nonzero hash passes.
2. Legacy-compatible: an unset decoded hash is tolerated (reported as
   Unset), a mismatch still fails.
3. No decoded-output verification.

## Rejected alternatives
- (3) was rejected: it reintroduces the exact bug being fixed.
- (2) as the *default* was rejected: strictness is the safety property,
   and silently weakening it for legacy streams is how the original bug
   survived.  Compatibility must be an explicit, deliberate opt-in.

## Decision
`IntegrityPolicy::Strict` is the default on every verify/CLI/court/
evidence path: payload mismatch → fail; decode failure → fail; unset
decoded hash → `DecodedHashMissing`; mismatch → `DecodedHashMismatch`.
`AllowLegacyUnsetDecodedHash` exists for legacy streams and reports
`Unset` without failing on that ground alone; a mismatch still fails.

## Tradeoffs
Gained: the decoded-output hash genuinely closes the model-corruption
hole.  Given up: seamless decoding of legacy streams with unset decoded
hashes (they now require explicit opt-in).

## Evidence
`crates/ryg-rans-rs-parallel/src/config.rs`; the courts
`RYG_RANS.L.VERIFY.DECODED_HASH` and `RYG_RANS.L.INTEGRITY.STRICT`; the
15-combination test matrix; the CLI exit-code-5 tests.

## Future implications
A format revision that always writes decoded hashes would eventually make
the legacy mode vestigial; it must still exist for old containers.
