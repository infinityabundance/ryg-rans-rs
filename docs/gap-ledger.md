# Gap Ledger

> **Last updated:** Phase H — Malformed-stream hardening, fuzzing, Kani proofs, performance benchmarks

## Byte rANS (full, hardened)

**Status:** `full` — All surfaces sealed, malformed-stream hardened, fuzz-tested.

### What upstream does
Complete `rans_byte.h`: init, renormalize, division put, flush, decoder init/get/advance, reciprocal symbol init, reciprocal put, step operations, decoder renorm.

### What Rust does
Complete implementation with matching semantics. 57+ tests pass including:
- Single-symbol round-trip
- Two-symbol round-trip
- Reciprocal equals division (with Kani formal proof)
- freq=1 special case
- Interleaved two-state round-trip
- Writer/reader edge cases
- **Malformed-stream hardening** (Phase H): Truncated-stream detection, renormalization-loop guards, edge-case frequency model validation
- **Fuzz testing** (Phase H): 5 cargo-fuzz targets (byte roundtrip, r64 roundtrip, word roundtrip, malformed byte, alias roundtrip)
- **Kani proofs** (Phase H): Encoder symbol init correctness, reciprocal=division equivalence, encode-decode inversion, R64 reciprocal=division equivalence
- **Performance benchmarks** (Phase H): Comprehensive multi-profile, multi-size decode throughput measurement

### Gap class
None.

## 64-bit rANS (full, hardened)

**Status:** `full` — All surfaces sealed, hardened, proven.

### What upstream does
Complete `rans64.h`: 64-bit state, 32-bit word renormalization, reciprocal with 128-bit mul_hi, step operations.

### What Rust does
Complete implementation with matching semantics. R64-specific tests pass including:
- State transitions
- Reciprocal equals division (with Kani formal proof)
- Renorm round-trips
- freq=1 special case
- Step operations
- Large-scale reciprocal (scale_bits up to 31) with >16-bit cmpl_freq
- **Kani proof**: R64 reciprocal=division equivalence for all valid parameters
- **Fuzz target**: r64 roundtrip (division + reciprocal)

### Gap class
None.

## Word-aligned scalar rANS (full)

**Status:** `full` — All surfaces sealed, fuzz-tested.

### What upstream does
`rans_word_sse41.h` provides:
- 256-symbol table with freq/bias slots
- slot-to-symbol mapping
- Scalar encode with 16-bit renormalization
- Scalar decode with table lookup
- Scalar renormalization

### What Rust does
Complete implementation:
- `RansWordSlot`, `RansWordTables` — 4096-slot frequency/bias table
- `rans_word_enc_init`, `rans_word_enc_put`, `rans_word_enc_flush` — Word-based encode
- `rans_word_dec_init`, `rans_word_dec_sym`, `rans_word_dec_renorm` — Word-based decode
- `build_word_tables` — Table construction from frequency model
- `decode_8way_scalar` — 8-state interleaved scalar decode
- **Fuzz target**: Word rANS roundtrip

### Gap class
None.

## Alias method (full)

**Status:** `full` — All surfaces sealed, fuzz-tested.

### What upstream does
`main_alias.cpp` provides:
- Vose alias table construction
- Exact normalization with rescue
- Donor selection with backtracking
- Encoder remap
- Constant-time alias decoder

### What Rust does
Complete implementation:
- `AliasTable` — Vose's alias table (256 buckets)
- `rans_byte_alias_normalize_freqs` — Exact normalization with zero-frequency theft
- `rans_byte_alias_build_table` — Vose's algorithm alias table construction
- `rans_byte_alias_enc_put` — Division-based encode with alias remap
- `rans_byte_alias_dec_get`, `rans_byte_alias_dec_advance` — O(1) alias decode
- **Fuzz target**: Alias roundtrip

### Gap class
None.

## SSE4.1 SIMD decoder (full, measured)

**Status:** `full` — All surfaces sealed, performance measured.

### What upstream does
`rans_word_sse41.h` provides:
- Four-lane SIMD decoder
- Two SIMD decoders for eight-way stream
- All 16 renormalization masks
- Shuffle-based byte extraction
- Sign-biased unsigned comparison

### What Rust does
Complete implementation:
- `RansSimdDec` — 4-lane SSE4.1 decoder state
- `rans_simd_dec_init`, `rans_simd_dec_sym_unchecked`, `rans_simd_dec_renorm_unchecked` — SIMD decoder kernels
- `decode_simd_8way` — Safe wrapper with compile-time dispatch
- `decode_simd_8way_unchecked` — `#[target_feature]`-gated unsafe kernel
- `decode_8way_scalar` — Pure-Rust scalar reference

**Performance**: Measured on Ryzen 7 9800X3D. Scalar 8-way is ~2.5× faster than SSE4.1 due to gather overhead. Full multi-profile, multi-size benchmarks available.

## Model artifact cache (full, Phase O)

**Status:** `full` — exact accounting, single-flight, explicit ownership, measured effectiveness.

### What the cache does
`ModelArtifactCache` (explicitly owned by `ParallelDecoder`) memoizes the
validated immutable model artifacts — the 256-symbol frequency vector and
(16 KiB) packed word table — keyed by `(model_sha256, scale_bits,
codec_id)`.  Exact per-entry byte accounting; zero capacity disables;
oversized entries are delivered but never retained; one retained entry per
key; N concurrent same-key cold requests perform exactly one construction
(single-flight); cache failure bypasses to the same canonical constructor
and is never a model error.

### Gap class
None.  Measured: warm cache materially improves small-block decode, is
neutral for large blocks, and unique-model streams are a net regression
(`docs/performance/model-cache.md`); FIFO eviction retained on shadow
simulation evidence (ADR-0017); 9 behavioural courts + 5 performance
receipts pin the guarantees.
