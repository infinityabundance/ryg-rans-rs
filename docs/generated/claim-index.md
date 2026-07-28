# Claim Index

| Court ID | Receipt | Verdict | Functions Cited |
|----------|---------|---------|-----------------|
| UNIT.BYTE.INIT | — | PASS | RansByteState::new |
| UNIT.BYTE.RENORM | — | PASS | rans_byte_enc_renorm |
| UNIT.BYTE.PUT | — | PASS | rans_byte_enc_put |
| UNIT.BYTE.FLUSH | — | PASS | rans_byte_enc_flush |
| UNIT.BYTE.DECODE | — | PASS | rans_byte_dec_init/get/advance |
| UNIT.BYTE.RECIPROCAL | — | PASS | RansByteEncSymbol::new, rans_byte_enc_put_symbol |
| UNIT.BYTE.INTERLEAVE | — | PASS | ByteInterleavedEncoder/Decoder |
| UNIT.R64.INIT | — | PASS | Rans64State::new |
| UNIT.R64.PUT | — | PASS | rans64_enc_put |
| UNIT.R64.FLUSH | — | PASS | rans64_enc_flush |
| UNIT.R64.DECODE | — | PASS | rans64_dec_init/get/advance |
| UNIT.R64.RECIPROCAL | — | PASS | Rans64EncSymbol::new, rans64_enc_put_symbol |
| UNIT.R64.MULHI | — | PASS | rans64_mul_hi |
| ORACLE.BYTE.RECIPROCAL | — | PASS | RansByteEncSymbol parameters match C |

## Receipts

No formal receipts have been sealed yet. All above courts are unit tests that pass.
Formal receipt generation requires the `ryg-rans-oracle` crate with full cross-decoding.
