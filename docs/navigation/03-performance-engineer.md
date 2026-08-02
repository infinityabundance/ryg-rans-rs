# 03 — Performance Engineer

**Purpose:** measure, interpret, and improve performance without producing
misleading numbers.

**Prerequisites:** `01-first-week.md`.

**Required papers:** 0001 §7, 0003 §6, 0005, 0007 §4.

**Required ADRs:** 0002, 0009, 0010.

**Required source modules:** `crates/ryg-rans-rs-bench/` (benches,
preflight, exporter), `xtask/src/main.rs` (`benchmark-run`,
`performance-seal`, the perf gates).

**Recommended reading order:**
1. `docs/papers/0005-performance-methodology.md` — the doctrine: verify
   before timing, component isolation, host capture.
2. `docs/performance/phase-l17-analysis.md` — the L.17 decomposition.
3. `docs/performance/comparative.md` — the L.14 comparative court.
4. `docs/adr/0010` — benchmark-time capture.
5. The bench crate's benches and the exporter code.
6. `evidence/performance/` — read a run: `run-manifest.json`,
   `host.json`, `commands.log`, `preflight/`, the receipts.

**Expected understanding:** how a sealed number is produced; what it does
and does not claim; how to run a new benchmark run and seal it; how to
decompose a scaling loss into components.

**Estimated reading time:** 10–16 hours.

**Exercises:**
1. Re-run the parallel bench and compare against the sealed receipt.
2. Explain why the parallel overhead is dominated by dual SHA-256.
3. Decompose a hypothetical "decode got slower" report into
   kernel/hashing/allocation/scheduling hypotheses with the measurement
   that would confirm each.

**Common misconceptions:**
- "RUSTFLAGS doesn't matter." Portable builds run the scalar reference;
   `-C target-cpu=native` is recorded at run time and verified at seal.
- "A copied Criterion directory is fine." The wrapper binds the run to the
   host, tree, and lock.

**Related evidence:** the ten performance receipts; `evidence/performance/index.json`.

**Future reading:** `04-simd-engineer.md`, `05-parallel-engineer.md`.
