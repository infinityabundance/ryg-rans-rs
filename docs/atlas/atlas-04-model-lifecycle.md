# Atlas 4 — Model Lifecycle

**Purpose:** from raw model bytes to shared decode tables.

```mermaid
flowchart LR
    MB[model bytes] --> KEY[ModelCacheKey::from_model: sha256 + scale + codec]
    KEY --> CACHE[ModelArtifactCache: exact accounting, single-flight]
    CACHE -->|miss| BUILD[build_validated_model_artifacts]
    CACHE -->|hit| ARC[shared Arc<PackedWordTable>]
    CACHE -->|hit| ART[ValidatedModelArtifacts: freqs Arc + packed table Arc]
    CACHE -->|miss| BUILD[validate sum == 1<<scale, build 16KiB table]
    BUILD --> ART
    ART --> PLAN[create_decode_plan: backend selected AFTER lookup]
    PLAN --> EXEC[execute_decode_plan borrows the table]
```

Key design points: the cache stores only model-derived immutable artifacts
(never the backend choice); a corrupt model is never admitted; eviction is
FIFO (deterministic); caching never changes error identity.  This lifecycle
is why repeated models (Uniform/Global policy) avoid per-block table
construction.

**Related:** ADR-0009; paper 0004 §7; parallel `cache.rs`, `decode_plan.rs`;
court `RYG_RANS.L.MODEL_CACHE.INTEGRATION`.
