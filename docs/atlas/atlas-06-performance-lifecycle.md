# Atlas 6 — Performance Lifecycle

**Purpose:** how a number becomes a sealed performance receipt.

```mermaid
flowchart LR
    RUN[cargo xtask benchmark-run] --> CAP[pre-run capture: commit, tree, lock SHA, RUSTFLAGS, host]
    CAP --> BENCH[Criterion suite]
    BENCH --> PRE[preflight records before timing]
    BENCH --> CRIT[raw criterion tree]
    CRIT --> ARCHIVE[criterion.tar.zst: tar crate, PAX, deterministic]
    PRE --> SEAL2[cargo xtask performance-seal]
    ARCHIVE --> SEAL2
    SEAL2 --> RECS[10 receipts: file SHA + canonical SHA]
    RECS --> TOP[evidence/performance/index.json]
```

A case is sealable only when full identity, preflight, hashes, thread
counts, sample counts, throughput derivation, and provenance all verify —
a zero-throughput value is valid only for a latency-unit microbenchmark
whose schema says so.

**Related:** paper 0005; ADR-0010; bench README; `evidence/performance/`.
