# Search Indexes (N.15)

> Discoverability layer: every artifact type indexed.  Use these to find
> *where* a topic lives, then follow the links.  The indexes are generated
> conventions: they are maintained by the documentation-health rules in
> `docs/contributing/how-to-extend-documentation.md`, and the seal's
> navigation gates verify that the indexed artifacts exist.

## Topic index

| Topic | Where |
|-------|-------|
| rANS arithmetic | papers/0001, bitstream-contract, atlas-02 |
| word coder / packed table | papers/0002, atlas-04 |
| reciprocal fast path | papers/0001 §4, ADR-0002, kani/ |
| renormalization | papers/0001 §5, bitstream-contract |
| interleaving | papers/0001 §6, papers/0003 |
| SIMD kernels | papers/0003, atlas-09, unsafe-ledger |
| parallel engine | papers/0004, atlas-08, ADRs 0004–0009/0013–0015 |
| cancellation | ADR-0007, papers/0004 §5, CLI signal.rs |
| integrity | ADR-0006, atlas-03, decode.rs |
| container format | container-format-v1, atlas-02/03/11 |
| evidence chain | papers/0006, atlas-05, residual-doctrine |
| performance methodology | papers/0005, atlas-06, performance-method |
| proof philosophy | papers/0007, unsafe-ledger |
| LLM-assisted engineering | papers/0008, llm/, story/ |
| failures | failures/, history/, gap-ledger |

## Algorithm index

| Algorithm | Papers | Code |
|-----------|--------|------|
| byte rANS (division + reciprocal) | 0001 | core `RansByte*` |
| R64 rANS | 0001 | core `Rans64*` |
| word rANS (scalar) | 0002 | core word surface |
| word rANS (SIMD) | 0002, 0003 | simd kernels |
| alias (Vose) | 0001 §2 | core alias |
| reciprocal multiply-high | 0001 §4 | core enc symbols |
| Vose alias construction | 0001 §2, 0007 | core alias table |
| uniform256 table-free | 0002 §3.4, 0003 §3.2 | simd + parallel |

## Receipt index

| Receipt family | Meaning | Where |
|----------------|---------|-------|
| `RYG_RANS.BYTE.*` | byte oracle parity | evidence/receipts |
| `RYG_RANS.R64.*` | R64 oracle parity | evidence/receipts |
| `RYG_RANS.WORD.*` | word oracle parity | evidence/receipts |
| `RYG_RANS.ALIAS.*` | alias oracle parity | evidence/receipts |
| `RYG_RANS.SIMD.INTERLEAVED8.*` | SSE4.1 8-way parity | evidence/receipts |
| `RYG_RANS.AVX512VL.*` / `AVX512.*` | AVX-512 parity | evidence/receipts |
| `RYG_RANS.L.*` | Phase L courts | evidence/receipts |
| `RYG_RANS.PERF.*` | performance surfaces | evidence/performance/.../receipts |

## ADR index

See `docs/navigation/adrs-by-topic.md` (N.9).

## Paper index

`docs/papers/0001..0008` — see the inventory (`docs/navigation/inventory.md`).

## Diagram index

`docs/diagrams/index.md` (ten architecture diagrams); `docs/atlas/*`
(chapter diagrams); `docs/navigation/maps/*` (learning maps, mermaid +
SVG).

## Glossary index

`docs/glossary.md` — every term, one file, alphabetically grouped by
section (data model, codec/backend, evidence, concurrency,
configuration).

## Module index

| Crate | Modules |
|-------|---------|
| core | `lib.rs`, `malformed.rs` |
| simd | `lib.rs`, `packed_table.rs`, `backends.rs`, `avx2.rs`, `avx2_renorm.rs`, `avx512.rs`, `model_kernels.rs` |
| parallel | `lib.rs`, `config.rs`, `error.rs`, `executor.rs`, `cancellation.rs`, `job.rs`, `plan.rs`, `reorder.rs`, `scratch.rs`, `cache.rs`, `decode_plan.rs`, `decode.rs`, `encode.rs`, `verify.rs`, `resource.rs`, `affinity.rs`, `sync.rs`, `block.rs` |
| cli | `lib.rs`, `signal.rs`, `error.rs`, `exit.rs`, `limits.rs`, `container/*`, `ops/*` |
| bench | `benches/*`, `src/courts/*`, `src/common/*` |
| oracle | `main.rs`, `phase_g.rs`, `phase_i.rs`, `perf.rs`, `lib.rs` |
| casefile | `lib.rs` |

## Benchmark index

| Benchmark | Measures | Crate |
|-----------|----------|-------|
| `byte_rans`, `r64`, `alias` | core surfaces | bench |
| `scalar` | word scalar | bench |
| `sse41`, `avx2`, `avx512` | SIMD kernels | bench |
| `specialized` | Uniform256 etc. | bench |
| `parallel` | the engine | bench |
| `batch`, `container`, `dispatch`, `parallel_l17` | component isolation | bench |
| `comparative` | L.14 alternatives | bench (feature-gated) |

## Navigation index

`docs/navigation/`: `inventory.md`, `00..10` guides, `maps/`, `knowledge-graph.md`,
`adrs-by-topic.md`, `commentary.md`, `reading-paths.md`.
