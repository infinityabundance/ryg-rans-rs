# 10 — Security Review

**Purpose:** review the repository from an attacker's perspective:
untrusted input handling, unsafe boundaries, resource exhaustion,
determinism, and evidence integrity.

**Prerequisites:** `01-first-week.md`, `04-simd-engineer.md` (for the
unsafe surface).

**Required papers:** 0003 (unsafe surface), 0006 (evidence integrity),
0007 (proof boundaries).

**Required ADRs:** 0011 (unsafe quarantine), 0006 (strict integrity).

**Required source modules:** the malformed-input paths in
`ryg-rans-rs-core/src/malformed.rs` and the fuzz targets in `fuzz/`; every
`unsafe fn` in the simd crate; the CLI's limits and container reader; the
evidence hashing in `xtask/`.

**Recommended reading order:**
1. `docs/negative-capabilities.md` — what the project deliberately does
   not do.
2. `docs/unsafe-ledger.md` — the unsafe surface and its contracts.
3. `docs/papers/0007-proof-philosophy.md` — confidence boundaries.
4. The fuzz targets and malformed-input tests.
5. The `# Safety` sections of every unsafe function.
6. The evidence-integrity gates (L1-R: no "verified after skipping").

**Expected understanding:** the no-panic-on-untrusted-input guarantee and
its enforcement (typed errors, checked arithmetic, bounds-checked
parsers); the unsafe surface's complete safety contracts; how the evidence
chain resists forgery (hashing, binding, preflight); the residual
accounting.

**Estimated reading time:** 10–16 hours.

**Exercises:**
1. Audit one unsafe function against its `# Safety` section.
2. Enumerate the ways a malformed container can fail (each must be a typed
   error, never a panic).
3. Explain how a forged receipt would be caught.

**Common misconceptions:**
- "The ledger lists all unsafe." It is bidirectionally tested against the
  source inventory — add a function, the test fails until the ledger is
  updated.
- "Unsafe is only in the SIMD crate." Correct for production crates
  (core/parallel are forbid; the CLI gates its one unsafe behind the
  `signals` feature).

**Related evidence:** the fuzz corpus results; the unsafe-ledger equality
test; the disassembly courts.

**Future reading:** `02-maintainer-path.md`.
