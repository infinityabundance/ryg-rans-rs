# Gap Ledger

## Byte rANS (full)

No gaps identified. All surfaces classified as `full` pass their unit tests.

### What upstream does
Complete `rans_byte.h`: init, renormalize, division put, flush, decoder init/get/advance, reciprocal symbol init, reciprocal put, step operations, decoder renorm.

### What Rust does
Complete implementation with matching semantics. 21 tests pass including:
- Single-symbol round-trip
- Two-symbol round-trip
- Reciprocal equals division
- freq=1 special case
- Interleaved two-state round-trip
- Writer/reader edge cases

## 64-bit rANS (full)

No gaps identified. All surfaces classified as `full` pass their unit tests.

### What upstream does
Complete `rans64.h`: 64-bit state, 32-bit word renormalization, reciprocal with 128-bit mul_hi, step operations.

### What Rust does
Complete implementation with matching semantics. 18 64-bit-specific tests pass including:
- State transitions
- Reciprocal equals division
- Renorm round-trips
- freq=1 special case
- Step operations

## Word-aligned scalar rANS (scaffold)

### What upstream does
`rans_word_sse41.h` provides:
- 256-symbol table with freq/bias slots
- slot-to-symbol mapping
- Scalar encode with 16-bit renormalization
- Scalar decode with table lookup
- Scalar renormalization

### What Rust currently does
Nothing yet. Surface is classified as scaffold.

### Gap class
Implementation gap.

## SSE4.1 SIMD decoder (scaffold)

### What upstream does
`rans_word_sse41.h` provides:
- Four-lane SIMD decoder
- Two SIMD decoders for eight-way stream
- All 16 renormalization masks
- Shuffle-based byte extraction
- Sign-biased unsigned comparison

### What Rust currently does
Nothing yet. Surface is classified as scaffold.

### Gap class
Implementation gap.

## Alias method (scaffold)

### What upstream does
`main_alias.cpp` provides:
- Vose alias table construction
- Exact normalization with rescue
- Donor selection with backtracking
- Encoder remap
- Constant-time alias decoder

### What Rust currently does
Nothing yet. Surface is classified as scaffold.

### Gap class
Implementation gap.

## Frequency normalization (scaffold)

### What upstream does
`main.cpp` / `main_alias.cpp` provides:
- Frequency counting
- Cumulative frequency calculation
- Resample normalization
- Zero-frequency rescue

### What Rust currently does
Nothing yet. Surface is classified as scaffold.

### Gap class
Implementation gap.
