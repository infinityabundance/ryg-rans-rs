# Atlas 10 — Oracle Architecture

**Purpose:** how the C oracle pins byte-exact parity.

```mermaid
flowchart LR
    C[oracle/adapter/rans_trace: built from pinned upstream] --> COURT[court generator]
    RUST[core/simd implementation] --> COURT
    COURT --> CASES[deterministic corpus: models x lengths x scales]
    CASES --> COMPARE[byte-exact compare + report compare]
    COMPARE --> MAN2[manifest]
    COMPARE --> REC2[receipt: admitted_match per case]
    REC2 --> PROMOTE[promote: MERGE into evidence/ by court_id]
```

The promote step merges (upsert by court_id) and never replaces the tree —
the rename-and-delete scheme was a critical bug (L19-B) that destroyed
unrelated evidence.

**Related:** `docs/oracle-method.md`; paper 0007 §5; ADR-0001; oracle
README; the oracle receipts.
