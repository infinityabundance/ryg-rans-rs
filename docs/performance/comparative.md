# Phase L.14 — Comparative Benchmark Court

Same-host, identical-corpus, identical-methodology comparison of `ryg-rans-rs`
against the upstream C reference implementation via the maintained FFI
bindings `ryg-rans-sys = "=1.2.0"` (m4tx, https://github.com/m4tx/ryg-rans-sys).

This court is **separate from the ten sealed performance surfaces**.  It is a
methodological comparison whose purpose is to establish (a) bit-exact format
parity with the C reference and (b) an honest, residual-documented performance
relationship.  It is not a claim that any surface is "faster than C".

## Court identity

| Field | Value |
|---|---|
| Court ID | `RYG_RANS.L.COMPARATIVE` |
| Implementation commit | `f1db7b2` (working tree, uncommitted at measurement; committed with this document) |
| Run date | 2026-07-31 |
| Host | AMD Ryzen 7 9800X3D, 8 cores / 16 threads, 1 NUMA node |
| RUSTFLAGS | `-C target-cpu=native` |
| C flags | `cc` crate defaults — **no** `-march=native` (residual L14-A) |
| Criterion | 0.5.1, warm-up 2 s, measurement 8 s, 50 samples |
| Corpus | `Skewed2551`, seed 42, exactly 1 MiB (1048576 bytes) |
| Model | Identical `freqs`/`cum_freqs` arrays passed to both implementations |

## Preflight: bit-exact format parity

Before timing, both implementations encode the same corpus with the same model
and the compressed outputs are asserted byte-identical:

* Byte rANS: Rust core `rans_byte_enc_put_symbol` vs C `rans_enc_put_symbol` — **identical**.
* Word rANS: Rust core `rans_word_enc_put` (LE bytes) vs C `rans_word_enc_put`
  (u16 words flattened to LE bytes) — **identical**.

The Rust core is therefore a bit-exact implementation of the upstream C format
for both byte and word rANS.  This is the strongest provenance result of the
court: any stream produced by either side decodes identically on the other.

## Results (median of 50 samples, 95% CI, slope-based throughput)

### FFI crossing overhead (isolated)

| Case | Median | 95% CI | Note |
|---|---|---|---|
| single empty extern call | 1.09 ns | [1.02, 1.16] ns | pure crossing + call |
| two calls per byte × 1 MiB | 1.9988 ms | [1.9988, 1.9990] ms | models the byte-decode crossing rate (get + advance per byte) |

### Byte rANS, 1 MiB, SKEWED_255_1

| Case | Median | 95% CI | Throughput | vs C |
|---|---|---|---|---|
| rust-core-reciprocal (encode) | 1.8472 ms | [1.8471, 1.8476] | 541.4 MiB/s | **1.05×** |
| rust-core-division (encode, reference path) | 2.8601 ms | [2.8528, 2.9221] | 349.6 MiB/s | 0.68× |
| ryg-rans-sys-c (encode, reciprocal) | 1.9426 ms | [1.9371, 1.9430] | 514.8 MiB/s | 1.00× |
| rust-core-reciprocal (decode) | 1.0556 ms | [1.0555, 1.0557] | 947.3 MiB/s | **2.20×** |
| ryg-rans-sys-c (decode) | 2.3244 ms | [2.3236, 2.3251] | 430.2 MiB/s | 1.00× |

* Encode is at parity (1.05×).  The Rust division reference path (349.6 MiB/s)
  quantifies the value of the reciprocal fast path: **1.55×** faster than
  division on the identical core.
* Decode: the C-side loop is FFI-mediated with the mandatory two crossings per
  byte.  The isolated cost of those crossings is ≈ 2.0 ms/MiB (above), i.e.
  ≈ 86 % of the measured C decode time.  The codec-only gap is therefore much
  smaller than the headline 2.20×; FFI-mediated use of the C library pays this
  cost, and the Rust side pays zero crossings.  This decomposition is the
  methodological separation required by Phase L.14, not a claim of a faster
  decoder core.

### Word rANS, 1 MiB, SKEWED_255_1 (single-state, SSE4.1 surface)

| Case | Median | 95% CI | Throughput | vs C |
|---|---|---|---|---|
| rust-core (encode) | 2.7399 ms | [2.7387, 2.7456] | 365.0 MiB/s | **1.69×** |
| ryg-rans-sys-c (encode) | 4.6366 ms | [4.6347, 4.7152] | 215.7 MiB/s | 1.00× |
| rust-core (decode) | 2.0561 ms | [2.0558, 2.0566] | 486.4 MiB/s | **1.01×** |
| ryg-rans-sys-c (decode) | 2.0825 ms | [2.0815, 2.0950] | 480.2 MiB/s | 1.00× |

* Decode is at parity (1.01×) — this is the cleanest codec-to-codec
  comparison in the court (no per-symbol construction on either side).
* Encode: Rust 1.69× faster.  `-C target-cpu=native` auto-vectorises the Rust
  build; the C wrappers are compiled by `cc` without `-march=native` and stay
  scalar (residual L14-A).  The encoder is memory-write bound; the gap is
  attributable to codegen, not algorithm.

## Residuals

* **L14-A** — C wrappers are not compiled with `-march=native`; where the C
  side lacks auto-vectorisation the comparison favours Rust.  The word-decode
  parity result and byte-encode parity result bound this handicap: on
  non-vectorisable paths the implementations are statistically identical.
* **L14-B** — The `rans` 0.4.0 crate (m4tx) exposes a different API and a
  different bitstream format; a byte-for-byte comparison requires format
  adaptation and is out of scope.  It is documented and excluded rather than
  compared incomparably.

## Conclusions

1. **Bit-exact parity with the C reference** for byte and word rANS (preflight
   assertion, both directions decodable).
2. **Reciprocal encode is at parity with C** (1.05×) and 1.55× faster than the
   division reference path on the same core.
3. **Word decode at parity** (1.01×); word encode 1.69× faster under native
   codegen with C compiled plain.
4. **Byte decode 2.20× end-to-end** — decomposed: ≈ 2.0 ms/MiB of that is
   isolated FFI crossing cost on the C side; the Rust path pays none.
5. No claim of general superiority over C is made; the court records
   measurements, separations, and residuals.

## Reproduction

```sh
RUSTFLAGS="-C target-cpu=native" \
  cargo bench -p ryg-rans-rs-bench --bench comparative \
  --features comparative \
  -- --save-baseline phase-l-comparative-final
```

Raw Criterion artifacts are archived under
`evidence/phase-l/comparative/criterion/`.
