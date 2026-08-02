# Atlas 9 — SIMD Hierarchy

**Purpose:** the backend hierarchy and the dispatch rules.

```mermaid
flowchart TD
    BASE[scalar references] --> SSE[SSE4.1 8-way: unpacked RansWordTables]
    BASE --> PACKED[PackedWordTable 16KiB]
    PACKED --> AVX2M[AVX2 manual gather 8]
    PACKED --> AVX2H[AVX2 hardware gather 8]
    PACKED --> AVX2X[AVX2 2x8 on 16]
    PACKED --> AVX512V[AVX-512VL 8-way]
    PACKED --> AVX512M[AVX-512VL manual 8]
    PACKED --> AVX512[AVX-512 16-way]
    PACKED --> AVX512MM[AVX-512 manual 16]
    PACKED --> AVX512X[AVX-512VL 2x8]
    UNIF[Uniform256 validated model] --> TF[table-free scalar]
    UNIF --> AVX2TF[AVX2 table-free]
```

Dispatch: portable/auto/scalar policies select scalar only; ModelAware adds
the table-free kernel for validated uniform models; explicit requests
execute exactly or return a typed error.  Runtime CPU features, compiled
features, and executed backends are recorded separately.

**Related:** papers 0002, 0003; ADR-0003, ADR-0008, ADR-0011;
`docs/unsafe-ledger.md`; receipts `RYG_RANS.SIMD.INTERLEAVED8.*`,
`RYG_RANS.AVX512VL.*`, `RYG_RANS.AVX512.*`; the disassembly courts.
