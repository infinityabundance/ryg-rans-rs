# Court Matrix

## Layer 1: Mathematical Unit Courts

| Court | Status | Test | 
|-------|--------|------|
| State init bounds | PASS | `test_state_init` |
| Encoder symbol init | PASS | `test_enc_symbol_init*` |
| Decoder symbol init | PASS | `test_decoder_symbol_init` |
| Writer bounds | PASS | `test_backward_writer_full` |
| Reader bounds | PASS | `test_reader_exhaustion` |
| Reciprocal == division | PASS | `test_reciprocal_equals_division` |
| freq=1 special case | PASS | `test_reciprocal_freq_one` |
| Rans64 mul_hi | PASS | `test_rans64_mul_hi` |

## Layer 2: State-Transition Courts

| Court | Status |
|-------|--------|
| Byte state init | PASS |
| Byte renormalization | PASS |
| Byte put | PASS |
| Byte flush | PASS |
| Byte decode init | PASS |
| Byte decode advance | PASS |
| Byte decode renorm | PASS |
| 64-bit state init | PASS |
| 64-bit renormalization | PASS |
| 64-bit put | PASS |
| 64-bit flush | PASS |
| 64-bit decode | PASS |

## Layer 3: Bitstream Courts

| Court | Status |
|-------|--------|
| Byte single-symbol roundtrip | PASS |
| Byte two-symbol roundtrip | PASS |
| Byte uniform distribution | PASS |
| Byte interleaved 2-state | PASS |
| 64-bit division roundtrip | PASS |
| 64-bit reciprocal roundtrip | PASS |
| 64-bit flush/init roundtrip | PASS |
| 64-bit renorm roundtrip | PASS |

## Layer 4: Cross-Decoding Courts

Not yet implemented. Requires oracle-compiled C encoder outputs.

## Layer 5: Malformed and Safety Courts

Not yet implemented.

## Layer 6: Codegen and Performance Courts

Not yet implemented.
