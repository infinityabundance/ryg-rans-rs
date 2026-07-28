# Port Parity Matrix

Generated from `docs-src/models/parity.model.json`.

| Surface | Status | Upstream File | Tests |
|---------|--------|---------------|-------|
| Byte rANS (division) | `full` | `rans_byte.h` | 42 |
| Byte rANS (reciprocal) | `full` | `rans_byte.h` | 42 |
| Byte rANS (interleaved) | `full` | `main.cpp` | 42 |
| 64-bit rANS (division) | `full` | `rans64.h` | 42 |
| 64-bit rANS (reciprocal) | `full` | `rans64.h` | 42 |
| Word scalar rANS | `scaffold` | `rans_word_sse41.h` | 0 |
| SSE4.1 decoder | `scaffold` | `rans_word_sse41.h` | 0 |
| Alias table | `scaffold` | `main_alias.cpp` | 0 |
| Alias encode | `scaffold` | `main_alias.cpp` | 0 |
| Alias decode | `scaffold` | `main_alias.cpp` | 0 |
| Frequency normalization | `scaffold` | `main.cpp` | 0 |

## Full Surfaces

| Function | Rust Name |
|----------|-----------|
| `RansEncInit` | `RansByteState::new()` |
| `RansEncRenorm` | `rans_byte_enc_renorm` |
| `RansEncPut` | `rans_byte_enc_put` |
| `RansEncFlush` | `rans_byte_enc_flush` |
| `RansDecInit` | `rans_byte_dec_init` |
| `RansDecGet` | `rans_byte_dec_get` |
| `RansDecAdvance` | `rans_byte_dec_advance` |
| `RansEncSymbolInit` | `RansByteEncSymbol::new()` |
| `RansDecSymbolInit` | `RansByteDecSymbol::new()` |
| `RansEncPutSymbol` | `rans_byte_enc_put_symbol` |
| `RansDecAdvanceStep` | `rans_byte_dec_advance_step` |
| `RansDecRenorm` | `rans_byte_dec_renorm` |
| `Rans64EncInit` | `Rans64State::new()` |
| `Rans64EncPut` | `rans64_enc_put` |
| `Rans64EncFlush` | `rans64_enc_flush` |
| `Rans64DecInit` | `rans64_dec_init` |
| `Rans64DecGet` | `rans64_dec_get` |
| `Rans64DecAdvance` | `rans64_dec_advance` |
| `Rans64EncSymbolInit` | `Rans64EncSymbol::new()` |
| `Rans64DecSymbolInit` | `Rans64DecSymbol::new()` |
| `Rans64EncPutSymbol` | `rans64_enc_put_symbol` |
| `Rans64MulHi` | `rans64_mul_hi` |
| `Rans64DecAdvanceStep` | `rans64_dec_advance_step` |
| `Rans64DecRenorm` | `rans64_dec_renorm` |
