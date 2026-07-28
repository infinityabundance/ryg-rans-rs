# Port Parity Matrix

Drafted from `docs-src/models/parity.model.json`.

| Surface | Status | Upstream File | Tests |
|---------|--------|---------------|-------|
| Byte rANS (division) | `implemented_unsealed` | `rans_byte.h` | 44 |
| Byte rANS (reciprocal) | `implemented_unsealed` | `rans_byte.h` | 44 |
| Byte rANS (interleaved) | `implemented_unsealed` | `main.cpp` | 44 |
| 64-bit rANS (division) | `implemented_unsealed` | `rans64.h` | 44 |
| 64-bit rANS (reciprocal) | `implemented_unsealed` | `rans64.h` | 44 |
| Word scalar rANS | `scaffold` | `rans_word_sse41.h` | 0 |
| SSE4.1 decoder | `scaffold` | `rans_word_sse41.h` | 0 |
| Alias table | `scaffold` | `main_alias.cpp` | 0 |
| Alias encode | `scaffold` | `main_alias.cpp` | 0 |
| Alias decode | `scaffold` | `main_alias.cpp` | 0 |
| Frequency normalization | `scaffold` | `main.cpp` | 0 |

## Implemented Unsealed Surfaces

| Function | Rust Name | Status |
|----------|-----------|--------|
| `RansEncInit` | `RansByteState::new()` | `implemented_unsealed` |
| `RansEncRenorm` | `rans_byte_enc_renorm` | `implemented_unsealed` |
| `RansEncPut` | `rans_byte_enc_put` | `implemented_unsealed` |
| `RansEncFlush` | `rans_byte_enc_flush` | `implemented_unsealed` |
| `RansDecInit` | `rans_byte_dec_init` | `implemented_unsealed` |
| `RansDecGet` | `rans_byte_dec_get` | `implemented_unsealed` |
| `RansDecAdvance` | `rans_byte_dec_advance` | `implemented_unsealed` |
| `RansEncSymbolInit` | `RansByteEncSymbol::new()` | `implemented_unsealed` |
| `RansDecSymbolInit` | `RansByteDecSymbol::new()` | `implemented_unsealed` |
| `RansEncPutSymbol` | `rans_byte_enc_put_symbol` | `implemented_unsealed` |
| `RansDecAdvanceStep` | `rans_byte_dec_advance_step` | `implemented_unsealed` |
| `RansDecRenorm` | `rans_byte_dec_renorm` | `implemented_unsealed` |
| `Rans64EncInit` | `Rans64State::new()` | `implemented_unsealed` |
| `Rans64EncPut` | `rans64_enc_put` | `implemented_unsealed` |
| `Rans64EncFlush` | `rans64_enc_flush` | `implemented_unsealed` |
| `Rans64DecInit` | `rans64_dec_init` | `implemented_unsealed` |
| `Rans64DecGet` | `rans64_dec_get` | `implemented_unsealed` |
| `Rans64DecAdvance` | `rans64_dec_advance` | `implemented_unsealed` |
| `Rans64EncSymbolInit` | `Rans64EncSymbol::new()` | `implemented_unsealed` |
| `Rans64DecSymbolInit` | `Rans64DecSymbol::new()` | `implemented_unsealed` |
| `Rans64EncPutSymbol` | `rans64_enc_put_symbol` | `implemented_unsealed` |
| `Rans64MulHi` | `rans64_mul_hi` | `implemented_unsealed` |
| `Rans64DecAdvanceStep` | `rans64_dec_advance_step` | `implemented_unsealed` |
| `Rans64DecRenorm` | `rans64_dec_renorm` | `implemented_unsealed` |

## Status Meanings

| Status | Meaning |
|--------|---------|
| `implemented_unsealed` | Rust implementation exists and is tested. No sealed oracle comparison receipt exists. |
| `partial` | Rust implementation covers some but not all upstream behavior. |
| `scaffold` | Module structure declared but no substantive implementation yet. |
