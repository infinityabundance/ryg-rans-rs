# Oracle Court Methodology

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity — every Rust encoder/decoder must produce and consume byte-identical streams to the upstream C/C++ reference. Residual primacy — every observed diff is a first-class artifact until resolved.

---

## Overview

The oracle is a court system that holds the Rust implementation accountable to the upstream C/C++ reference. Each "court" tests a specific algorithmic surface by compiling the upstream source, extracting execution traces, and comparing them against the Rust implementation at multiple levels of fidelity.

The oracle does **not** use FFI. Upstream C/C++ is compiled to standalone binaries, and the oracle communicates with them via subprocess, JSON casefiles, and captured output. This keeps the Rust workspace free of C/C++ build dependencies.

---

## How the Oracle Works

### 1. Upstream C/C++ Compilation

The upstream repository (`ryg_rans`, commit `c9d162d9`) is cloned to a controlled path (typically a Docker volume or a `vendor/` directory excluded from the workspace). The `Makefile` from the pinned commit is used to build the oracle binaries:

| Binary | Source | Purpose |
|---|---|---|
| `exam` | `main.cpp` + `rans_byte.h` | 32-bit byte rANS reference |
| `exam64` | `main64.cpp` + `rans64.h` | 64-bit rANS reference |
| `exam_sse41` | `main_simd.cpp` + `rans_word_sse41.h` | SIMD decoder reference |
| `exam_alias` | `main_alias.cpp` + `rans_byte.h` | Alias method reference |

Compilation is done with `-O2` and no sanitizers (to match real-world behavior) and with `-fsanitize=undefined` in a separate pass (to catch upstream UB). The oracle binaries are statically linked and copied into a known path.

### 2. Trace Extraction

The oracle instruments the oracle binaries and the Rust library to emit **state transition traces**:

```
[cycle] state_before → operation → state_after  [symbol, freq, cumulative]
```

For encoding, each cycle captures:
- State before renormalization.
- Bytes emitted during renormalization.
- State after renormalization.
- The division or reciprocal computation and its result.
- State after the put operation.

For decoding, each cycle captures:
- Initial state.
- Cumulative frequency from mask.
- Symbol lookup result.
- Advanced state before renormalization.
- Bytes consumed during renormalization.
- Final state after renormalization.

Traces are emitted as newline-delimited JSON (NDJSON) so they can be compared line-by-line, field-by-field.

### 3. State Transition Comparison

For each deterministic casefile, the oracle:

1. Runs the upstream C/C++ binary with the casefile, capturing the full trace.
2. Runs the Rust implementation (`ryg-rans-core`) with the same casefile, capturing the same trace format.
3. Aligns traces by cycle index and compares every field with exact equality.

A **residual** is created for any mismatch at the state-transition level. The residual records:
- The case ID and variant.
- The cycle index and operation.
- The expected (upstream) and actual (Rust) values.
- The input data and parameters that produced the mismatch.

### 4. Cross-Decoding

Cross-decoding tests the **bitstream parity** doctrine at the highest level:

1. **Rust → C decode**: Encode with Rust, decode with upstream C/C++. If the C binary reproduces the original input, the Rust encoder produces a valid bitstream.
2. **C → Rust decode**: Encode with upstream C/C++, decode with Rust. If Rust reproduces the original input, the Rust decoder is bitstream-compatible.

A residual is created at this level only if the bitstream is consumed successfully but produces wrong output. If the bitstream is structurally invalid (can't be decoded at all), that is a separate malformed-input case.

Cross-decoding is the **final verdict** for bitstream parity. A surface is not marked `full` until all three levels pass: mathematical, state-transition, and cross-decode.

---

## Court Hierarchy

Courts are ordered from most fundamental to most applied. A surface must pass all lower courts before proceeding to the next.

```
Level 1: Mathematical Court
Level 2: State-Transition Court
Level 3: Bitstream Court
Level 4: Cross-Decode Court
Level 5: Malformed-Input Court
Level 6: Codegen Court (informative)
```

### Level 1 — Mathematical Court

**Scope:** Individual arithmetic operations — `mul_hi`, reciprocal approximation, division, modular reduction.

**Method:** Compare computed values against a pure-arithmetic reference (64-bit integer arithmetic, Rust's built-in division). No I/O, no renormalization, no encoder/decoder state machine.

**Pass condition:** Every computed value matches the upstream reference exactly for all valid inputs.

### Level 2 — State-Transition Court

**Scope:** Full encoder/decoder state machine — renormalize, put, flush, init, get, advance, renorm.

**Method:** Compare trace-for-trace against compiled upstream C/C++. Every state transition must match exactly.

**Pass condition:** Every cycle in the trace matches upstream for all casefiles in the suite. Zero residuals.

### Level 3 — Bitstream Court

**Scope:** The byte-level output of encoding and the byte-level input of decoding.

**Method:** Compare the exact sequence of bytes produced by encoding and consumed by decoding. This is the byte-level equivalent of state-transition comparison but operating on the final output rather than internal state.

**Pass condition:** Encoded output is byte-for-byte identical to upstream output. Decoded output is byte-for-byte identical to upstream output.

### Level 4 — Cross-Decode Court

**Scope:** Cross-implementation decoding — Rust-encoded → C-decoded, C-encoded → Rust-decoded.

**Method:** Encode with one implementation, decode with the other. Compare the decoded output to the original input.

**Pass condition:** Decoded output matches original input exactly in both directions. This is the definitive proof of bitstream parity.

### Level 5 — Malformed-Input Court

**Scope:** Behavior on invalid or malformed bitstreams — truncated input, corrupted frequencies, invalid states.

**Method:** Feed malformed inputs to both implementations and compare behavior. The Rust implementation must not panic, must not read out of bounds, and must produce deterministic error behavior.

**Pass condition:** Rust implementation handles all malformed inputs safely. Error behavior is documented. No undefined behavior.

### Level 6 — Codegen Court (Informative)

**Scope:** Compiler codegen quality — instruction selection, inlining, register allocation.

**Method:** Compare Godbolt output or disassembly of hot loops between Rust and C/C++ at equivalent optimization levels.

**Pass condition:** Not a pass/fail court. Results inform optimization decisions. The Rust implementation is not required to match C/C++ codegen, but significant regressions should be understood and documented.

---

## Court Run Lifecycle

```
1. Casefile generation
   └─ Deterministic: same seed → same casefile
   └─ Covers: single symbol, many symbols, interleaved, edge cases

2. Court execution (per algorithmic surface)
   ├─ Compile upstream C/C++ binaries
   ├─ For each casefile:
   │   ├─ Run upstream binary → trace_A
   │   ├─ Run Rust harness → trace_B
   │   ├─ Compare traces at current court level
   │   └─ Record residuals for any mismatch
   └─ Produce Receipt:
       ├─ court_id, case_count, verdict
       ├─ pairs_compared, pairs_matched
       └─ residuals (if any)

3. Residual triage
   ├─ Classify: "implementation", "oracle", "casefile"
   ├─ Assign severity: S0–S3
   └─ Assign status: "open", "investigating", "fixed", "wontfix"

4. Gate check
   └─ All courts must pass for the surface to be marked "full"
```

---

## Receipt Format

Courts produce JSON receipts that are stored in `reports/oracle/`:

```json
{
    "schema_version": 1,
    "court_id": "state-transition.byte-rans",
    "case_count": 128,
    "verdict": "pass",
    "upstream_commit": "c9d162d996fd600315af9ae8eb89d832576cb32d",
    "rust_commit": "abc123def456",
    "pairs_compared": 15872,
    "pairs_matched": 15872,
    "residual_count": 0,
    "residual_ids": [],
    "timestamp": 1735689600
}
```

A `verdict` of `"fail"` means one or more residuals were recorded at that court level. The corresponding residuals are linked by `residual_ids` and stored in `reports/residuals/`.
