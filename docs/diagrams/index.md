# Architecture Diagrams

> *Layer: cross-cutting (referenced by the papers and crate READMEs).  The
> diagrams are Mermaid; the renderer supports flowchart, sequence, class,
> state, ER, gantt, pie, and journey types.  Every diagram here describes
> the *actual* architecture — traced from code, not from intent.*

## 1. Repository → crates → dependencies

```mermaid
flowchart TD
    REPO[ryg-rans-rs] --> CORE[ryg-rans-rs-core<br/>no_std, forbid unsafe<br/>byte/R64/word/alias rANS]
    REPO --> SIMD[ryg-rans-rs-simd<br/>SSE4.1/AVX2/AVX-512 kernels<br/>ledgered unsafe]
    REPO --> PAR[ryg-rans-rs-parallel<br/>bounded executor, reorder,<br/>scratch, model cache]
    REPO --> CLI[ryg-rans-rs-cli<br/>RYGRANS v1 container,<br/>10 subcommands]
    REPO --> BENCH[ryg-rans-rs-bench<br/>Criterion + courts, publish=false]
    REPO --> ORACLE[ryg-rans-rs-oracle<br/>forensic courts]
    REPO --> CASE[ryg-rans-rs-casefile<br/>evidence schema]
    REPO --> XT[xtask<br/>seal gates, run wrappers]
    SIMD --> CORE
    PAR --> CORE
    PAR -. simd feature .-> SIMD
    CLI --> CORE
    CLI --> PAR
    CLI -. simd feature .-> SIMD
    BENCH --> CORE
    BENCH --> SIMD
    BENCH --> PAR
    BENCH --> CASE
    ORACLE --> CORE
    ORACLE --> SIMD
    ORACLE --> CASE
```

## 2. Encoding pipeline (CLI / container level)

```mermaid
flowchart LR
    IN[input bytes] --> CHUNK[block-size chunks]
    CHUNK --> SELECT[block selection: RLE / rANS / RAW]
    SELECT --> ENC[codec encode<br/>byte/r64/word]
    ENC --> BLK[block record: header + model + payload<br/>+ payload SHA-256 + decoded SHA-256]
    BLK --> WRITE[ContainerWriter: streaming serializer]
    WRITE --> FOOT[footer: block count, container SHA-256,<br/>decoded-stream SHA-256]
    FOOT --> OUT[.rygr container]
```

## 3. Parallel decode pipeline

```mermaid
flowchart LR
    CONT[.rygr container] --> PARSE[ContainerReader: header + block records]
    PARSE --> PLAN[FixedBlockPlan: thread-independent boundaries]
    PLAN --> EXEC[bounded live executor]
    EXEC --> REORDER[ReorderBuffer: atomic commit batches<br/>bounded by max_buffered_output_bytes]
    REORDER --> SINK[ordered blocks / streaming sink]
    SINK --> HASH[stream SHA-256 in canonical order]
    HASH --> OUT[decoded output]
    EXEC -. cancellation .-> CANCEL[CancellationToken]
    EXEC -. scratch .-> SCRATCH[per-worker WorkerScratch]
    EXEC -. model cache .-> CACHE[ModelCache: Arc-shared packed table]
```

## 4. The bounded executor (workers, queues, cancellation, sink)

```mermaid
sequenceDiagram
    participant P as Producer thread
    participant J as Bounded job channel
    participant W as K workers (each with exclusive scratch)
    participant R as Bounded result channel
    participant C as Coordinator
    participant B as ReorderBuffer (bounded by output budget)
    P->>J: submit task (blocking send = backpressure)
    J->>W: recv task
    W->>W: run task (checks cancel; catch_unwind; reset scratch)
    W->>R: send result (blocking send = bounded in-flight)
    R->>C: recv result
    C->>B: insert → commit batch (ascending indexes)
    C->>C: drain while producer submits (no deadlock)
    Note over P,W: cancellation is polled at each yield point;
    Note over C: completeness checked at the API boundary
```

## 5. Evidence chain: implementation → receipt → seal → release

```mermaid
flowchart LR
    IMPL[implementation commit] --> BENCHMARK[benchmark-run wrapper<br/>captures commit/tree/lock/rustc/RUSTFLAGS]
    BENCHMARK --> CRIT[Criterion tree + 800 preflight records]
    CRIT --> EXPORT[exporter: benchmark.json + sample.json + estimates.json]
    EXPORT --> MAN[10 manifests]
    MAN --> REC[10 receipts: file SHA-256 + canonical SHA-256]
    REC --> RUNIDX[run-local index]
    RUNIDX --> TOPIDX[canonical evidence/performance/index.json]
    IMPL --> COURTS[behavioural courts: oracle + Phase L]
    COURTS --> BREC[158 receipts + manifests]
    TOPIDX --> SEAL[cargo xtask seal — the single authoritative gate]
    BREC --> SEAL
    DOCKER[Docker matrix 11/11 at the implementation commit] --> SEAL
    SEAL --> RELEASE[publication in dependency order + tag v0.3.0]
```

## 6. Backend dispatch

```mermaid
flowchart TD
    POL[BackendPolicy] --> PORT[Portable / ScalarPreferred / Auto]
    POL --> MA[ModelAware]
    POL --> EX[Explicit]
    PORT --> SCALAR[Scalar8 / Scalar16]
    MA --> SCALAR
    MA --> U256[Uniform256TableFree16<br/>validated uniform model only]
    EX --> VALID[format compatibility matrix:<br/>8-way↔codec7, 16-way↔codec8,<br/>Uniform256↔model, batch↔context]
    VALID --> KERNELS[SSE4.1 8-way / AVX2 manual+gather /<br/>AVX2 2x8 / AVX-512VL 8-way /<br/>AVX-512 16-way / batch4]
    KERNELS --> CAP[execution-time capability check:<br/>runtime CPU + compiled features]
    CAP -->|absent| ERR[BackendUnavailable — typed error,<br/>never silent scalar substitution]
    VALID -->|invalid| ERR2[BackendFormatMismatch /<br/>BackendRequiresBatchContext]
```

## 7. Integrity decision (per block)

```mermaid
flowchart TD
    START[block] --> PAY[payload SHA-256 matches stored?]
    PAY -->|no| FAIL1[fail: PayloadHash]
    PAY -->|yes| DEC[decode succeeds?]
    DEC -->|no| FAIL2[fail: Codec / Format]
    DEC -->|yes| DH[decoded SHA-256 vs stored]
    DH -->|stored zero| STRICT[IntegrityPolicy::Strict?]
    STRICT -->|yes| FAIL3[fail: DecodedHashMissing]
    STRICT -->|no - legacy opt-in| UNSET[report Unset, allow]
    DH -->|nonzero mismatch| FAIL4[fail: DecodedHashMismatch]
    DH -->|nonzero match| PASS[pass]
```

## 8. Cancellation and completeness

```mermaid
flowchart TD
    CANCEL[CancellationToken] --> WORKERS[workers poll before each task]
    CANCEL --> PROD[producer polls before each submission]
    WORKERS --> SKIP[skipped tasks counted]
    PROD --> STOP[submission stops]
    EXEC[executor: was_cancelled && short results?] -->|yes| ECANCEL[Err Cancelled completed/expected]
    EXEC -->|no, short| EINC[Err IncompleteExecution — internal bug]
    EXEC -->|complete| OK[Ok + report]
    API[public *_with_cancel boundary] --> CHECK[error::check_completeness re-asserts<br/>before returning Ok]
```

## 9. CLI command surface

```mermaid
flowchart LR
    CLI[ryg-rans] --> ENC[encode]
    CLI --> DEC[decode]
    CLI --> INS[inspect]
    CLI --> VER[verify]
    CLI --> MOD[model build/inspect/validate/compare]
    CLI --> TRC[trace]
    CLI --> CMP[compare arithmetic/backends/files]
    CLI --> BCH[bench]
    CLI --> CAP[capabilities]
    CLI --> COM[completions]
    ENC --> CANCEL[SIGINT/SIGTERM/--timeout → exit 11]
    DEC --> CANCEL
    VER --> CANCEL
    ENC --> EXIT[stable exit codes 0-11]
    DEC --> EXIT
    VER --> EXIT
```

## 10. Docker matrix

```mermaid
flowchart LR
    SRC[immutable source snapshot at the implementation commit] --> JOBS
    JOBS[11 jobs] --> O[oracle-gcc]
    JOBS --> P[package-audit]
    JOBS --> M[msrv]
    JOBS --> A[cross-aarch64]
    JOBS --> MU[rust-musl-build]
    JOBS --> S[sanitizers]
    JOBS --> R[rust-stable-tests]
    JOBS --> C[cross-court]
    JOBS --> MI[miri]
    JOBS --> PE[performance]
    JOBS --> PA[parallel-stable]
    O --> STAMP[evidence/docker-matrix.json<br/>11/11 exit 0, log hashes, git_commit prefix-match]
    P --> STAMP
    M --> STAMP
    A --> STAMP
    MU --> STAMP
    S --> STAMP
    R --> STAMP
    C --> STAMP
    MI --> STAMP
    PE --> STAMP
    PA --> STAMP
    STAMP --> SEAL[cargo xtask seal]
```
