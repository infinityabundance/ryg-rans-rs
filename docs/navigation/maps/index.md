# Learning Maps (N.2)

> Dependency maps for the major learning paths.  Each map exists as Mermaid
> below and as an SVG in this directory (`maps/*.svg`).  The maps show
> dependencies, not a strict timetable — follow the arrows.

## 1. New contributor

```mermaid
flowchart TD
    NEW[New contributor] --> README[README.md portal]
    README --> PHIL[docs/philosophy.md]
    PHIL --> GLOSS[docs/glossary.md]
    GLOSS --> LAYERS[docs/layers.md]
    LAYERS --> P1[docs/papers/0001 rANS design]
    P1 --> ARCH[docs/architecture.md]
    ARCH --> CLI[Try the CLI]
    CLI --> EVID[docs/papers/0006 evidence]
    EVID --> SEAL[cargo xtask seal]
    SEAL --> NEXT[docs/navigation/01-first-week.md]
```

## 2. SIMD

```mermaid
flowchart TD
    SIMD[4-simd-engineer.md] --> P2[docs/papers/0002 word rANS]
    P2 --> P3[docs/papers/0003 SIMD kernels]
    P3 --> UNSAFE[docs/unsafe-ledger.md]
    UNSAFE --> SRC[crates/ryg-rans-rs-simd/src kernels]
    SRC --> LEDGER[unsafe-ledger.toml bidirectional test]
    SRC --> DISASM[disassembly courts]
    SRC --> PARITY[report-parity courts]
    DISASM --> REC[RYG_RANS.SIMD.INTERLEAVED8.* receipts]
    PARITY --> REC
```

## 3. Parallel engine

```mermaid
flowchart TD
    PAR[5-parallel-engine.md] --> P4[docs/papers/0004 parallel engine]
    P4 --> ADR4[docs/adr/0004 bounded executor]
    P4 --> ADR7[docs/adr/0007 completeness boundary]
    ADR7 --> EXEC[crates/ryg-rans-rs-parallel/src/executor.rs]
    EXEC --> REORD[reorder.rs atomic commit]
    REORD --> CANCEL[cancellation.rs]
    CANCEL --> SCRATCH[scratch.rs per-worker]
    SCRATCH --> CACHE[cache.rs Arc-shared table]
    CACHE --> LOOM[loom courts]
    LOOM --> COURTS[RYG_RANS.L.EXECUTOR.BOUNDED receipt]
```

## 4. Evidence

```mermaid
flowchart TD
    EV[7-evidence-engineer.md] --> P6[docs/papers/0006 evidence]
    P6 --> RESID[docs/residual-doctrine.md]
    RESID --> CASE[crates/ryg-rans-rs-casefile schema]
    CASE --> PRE[crates/ryg-rans-rs-bench preflight]
    PRE --> WRAP[xtask benchmark-run wrapper]
    WRAP --> SEAL2[xtask performance-seal]
    SEAL2 --> IDX[evidence/performance/index.json]
    IDX --> GATES[seal gates]
    GATES --> LEDGER[evidence/phase-l/gap-ledger.md]
```

## 5. Performance

```mermaid
flowchart TD
    PERF[3-performance-engineer.md] --> P5[docs/papers/0005 methodology]
    P5 --> L17[docs/performance/phase-l17-analysis.md]
    P5 --> COMP[docs/performance/comparative.md]
    P5 --> ADR10[docs/adr/0010 benchmark-time capture]
    ADR10 --> BENCH[crates/ryg-rans-rs-bench]
    BENCH --> RUN[evidence/performance/runs]
    RUN --> RECEIPTS[10 performance receipts]
```

## 6. Container / CLI

```mermaid
flowchart TD
    CLI[8-cli-engineer.md] --> CONTAINER[docs/container-format-v1.md]
    CONTAINER --> BITSTREAM[docs/bitstream-contract.md]
    BITSTREAM --> OPS[crates/ryg-rans-rs-cli/src/ops]
    OPS --> INTEGRITY[strict integrity policy]
    INTEGRITY --> EXITS[stable exit codes 0-11]
    OPS --> SIGNAL[signal.rs cooperative cancellation]
```

## 7. Release

```mermaid
flowchart TD
    REL[Release] --> FREEZE[freeze implementation commit]
    FREEZE --> BENCH2[benchmark-run]
    BENCH2 --> PSEAL[performance-seal]
    PSEAL --> ORACLE[oracle + phase-g courts]
    ORACLE --> COURTS2[courts-run]
    COURTS2 --> DOCKER[Docker matrix at the commit]
    DOCKER --> SEAL3[full seal gate]
    SEAL3 --> SEMVER[cargo semver-checks]
    SEMVER --> BUMP[version bump]
    BUMP --> PUB[cargo publish in dependency order]
    PUB --> TAG[release tag at the sealed commit]
```

## 8. LLM workflow

```mermaid
flowchart TD
    LLM[9-llm-engineer.md] --> P8[docs/papers/0008 methodology]
    P8 --> CHECK[docs/llm/index.md checklists]
    CHECK --> CLAIM[state the claim]
    CLAIM --> TRACE[trace the code path]
    TRACE --> TEST[write the failing test]
    TEST --> FIX[implement]
    FIX --> COURT[write the court]
    COURT --> RECEIPT[generate the receipt]
    RECEIPT --> SEAL4[run the seal]
    SEAL4 --> DONE[Sealed claim]
```
