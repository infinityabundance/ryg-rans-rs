# Residual Summary

No residuals have been recorded yet; residual discovery begins with the cross-decoding courts.
All 44 unit tests pass and cover the full byte rANS and 64-bit rANS surfaces.
No sealed oracle receipts exist yet; all surface claims are backed by Rust self-tests only.

## By Severity

| Severity | Count | Status |
|----------|-------|--------|
| S0 (invalidates) | 0 | N/A |
| S1 (major mismatch) | 0 | N/A |
| S2 (edge case) | 0 | N/A |
| S3 (documentation) | 0 | N/A |

## Surface Implementation Status

| Surface | Status |
|---------|--------|
| Byte rANS (division) | `implemented_unsealed` |
| Byte rANS (reciprocal) | `implemented_unsealed` |
| Byte rANS (interleaved) | `implemented_unsealed` |
| 64-bit rANS (division) | `implemented_unsealed` |
| 64-bit rANS (reciprocal) | `implemented_unsealed` |
| Word scalar rANS | `scaffold` |
| SSE4.1 decoder | `scaffold` |
| Alias table | `scaffold` |
| Alias encode | `scaffold` |
| Alias decode | `scaffold` |
| Frequency normalization | `scaffold` |

## By Class

No residuals recorded.

## Resolution Notes

The oracle adapter (`oracle/adapter/rans_trace`) has been built and tested.
Reciprocal parameters match between Rust and C implementations (self-test only —
no compiled oracle comparison has been sealed). All surfaces are marked
`implemented_unsealed` or `scaffold`; no `full` or `sealed` statuses have been achieved.
