# Atlas 11 — CLI Architecture

**Purpose:** the `ryg-rans` command surface.

```mermaid
flowchart LR
    CLI[ryg-rans] --> ENC[encode]
    CLI --> DEC[decode]
    CLI --> INS[inspect]
    CLI --> VER[verify]
    CLI --> MOD[model]
    CLI --> TRC[trace]
    CLI --> CMP[compare]
    CLI --> BCH[bench]
    CLI --> CAP[capabilities]
    CLI --> COM[completions]
    ENC --> CANCEL[SIGINT/SIGTERM/--timeout -> exit 11]
    DEC --> CANCEL
    VER --> CANCEL
    ENC --> EXIT[exit codes 0-11]
```

The container reader/writer enforce the RYGRANS v1 format; the single codec
dispatcher serves decode/inspect/verify/compare; strict integrity is the
default; the `signals` feature is the only unsafe surface (default build).

**Related:** `docs/container-format-v1.md`; ADR-0006; CLI README;
`docs/education.md` (CLI maintainer notes); the CLI integration tests.
