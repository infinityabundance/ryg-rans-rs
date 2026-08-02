# Atlas 5 — Evidence Lifecycle

**Purpose:** how an observation becomes a sealed receipt.

```mermaid
flowchart LR
    OBS[observation: court runs / benchmark runs] --> MAN[manifest: inputs, expected]
    OBS --> REC[receipt: per-case verdicts, actual, implementation_commit]
    MAN --> CHAIN[manifest_sha256 inside receipt]
    REC --> IDX[evidence/index.json: court_id -> file SHA-256]
    REC --> SELFC[canonical self-hash: content minus self-hash field]
    IDX --> SEAL[cargo xtask seal verifies every link]
    SEAL --> STATUS[README evidence table regenerated from the indexes]
```

The behavioural chain and the performance chain share the doctrine: values
come from execution, hashes are of real files, bindings are independent,
and the seal never prints "verified" for a skipped check (L1-R).

**Related:** paper 0006; ADR-0010; casefile README; oracle README;
`docs/residual-doctrine.md`.
