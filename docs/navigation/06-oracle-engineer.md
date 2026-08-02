# 06 — Oracle Engineer

**Purpose:** understand, run, and extend the forensic oracle courts that
pin byte-exact parity with the upstream C.

**Prerequisites:** `01-first-week.md`.

**Required papers:** 0001 §10, 0007 §5.

**Required ADRs:** 0001.

**Required source modules:** `crates/ryg-rans-rs-oracle/src/` (`main.rs`,
`phase_g.rs`, `phase_i.rs`, `perf.rs`); `oracle/adapter/` (the C build);
`crates/ryg-rans-rs-casefile/` (the receipt schema).

**Recommended reading order:**
1. `docs/oracle-method.md` — the methodology.
2. `docs/papers/0007-proof-philosophy.md` §5 — what the oracle proves.
3. The oracle crate sources and the adapter build.
4. `docs/papers/0006-evidence.md` — how receipts flow.
5. `evidence/receipts/` — read one receipt and its manifest.

**Expected understanding:** how the C oracle is built and driven; how a
court produces a manifest + receipt; how the promote-merge works (and the
L19-B history of why it merges rather than replaces); how to add a court.

**Estimated reading time:** 6–10 hours.

**Exercises:**
1. Build the adapter (`cd oracle/adapter && make`) and run one court.
2. Read a receipt and verify its manifest SHA-256.
3. Explain what the oracle cannot prove (generality).

**Common misconceptions:**
- "The oracle is a test." It is a differential harness whose output is a
  sealed receipt.
- "Promotion replaces evidence." It merges (upsert by court_id); the
  rename-and-delete scheme was a critical bug (L19-B).

**Related evidence:** all `RYG_RANS.BYTE.*`, `R64.*`, `WORD.*`, `ALIAS.*`,
`SIMD.INTERLEAVED8.*`, `AVX512*.*` receipts.

**Future reading:** `07-evidence-engineer.md`.
