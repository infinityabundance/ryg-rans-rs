# 04 — SIMD Engineer

**Purpose:** understand, verify, and extend the SIMD decode kernels.

**Prerequisites:** `01-first-week.md`, comfortable with x86 intrinsics.

**Required papers:** 0002, 0003.

**Required ADRs:** 0003, 0011.

**Required source modules:** `crates/ryg-rans-rs-simd/src/{lib.rs,
packed_table.rs, backends.rs, avx2*.rs, avx512.rs}`; the ledger
`unsafe-ledger.toml`; the parallel crate's `decode_plan.rs`.

**Recommended reading order:**
1. `docs/papers/0002-word-rans.md` — the packed table.
2. `docs/papers/0003-simd.md` — the kernels and dispatch philosophy.
3. `docs/unsafe-ledger.md` — the safety contract model.
4. The kernel sources, reading each `# Safety` section.
5. The disassembly courts and the report-parity tests.
6. The oracle receipts for the SIMD surfaces.

**Expected understanding:** the packed-entry layout and why; the manual vs
hardware gather distinction; the no-masked-over-read invariant; the
`#[target_feature]` + ledger + disassembly discipline; how to add a new
backend without breaking the courts.

**Estimated reading time:** 12–20 hours.

**Exercises:**
1. Explain why a 12/12/8 bit packing exists and what breaks if `bias` gains
   a bit.
2. Trace one kernel's safety contract: provenance, bounds, alignment, CPU
   features, callers.
3. Run the unsafe-ledger equality test and the disassembly courts.

**Common misconceptions:**
- "Caller-context target features are fine." They are a hidden caller
  obligation — prohibited (ADR-0011).
- "Hardware gather is always faster." It is microarchitecture-dependent;
  that is why manual variants exist.

**Related evidence:** `RYG_RANS.SIMD.INTERLEAVED8.*`, `RYG_RANS.AVX512VL.*`,
`RYG_RANS.AVX512.*` receipts; the disassembly courts.

**Future reading:** `03-performance-engineer.md`, `07-evidence-engineer.md`.
