# Paper 0001 — rANS: the design of the canonical implementation

> *Layer: Algorithm/Subsystem.  Companion: `docs/bitstream-contract.md` (the
> exact formats), `docs/papers/0002-word-rans.md` (the word coder), paper
> 0003 (SIMD).  This paper explains **why** the implementation is the way it
> is; the bitstream contract pins **what** it must be.*

## 1. History: where rANS comes from

Asymmetric Numeral Systems (ANS) was introduced by Jarek Duda in 2009 as a
family of entropy coders that achieve near-arithmetic-coding compression
with the speed of Huffman coding.  The core idea: instead of writing one
codeword per symbol (Huffman) or maintaining an arithmetic interval
(arithmetic coding), ANS maintains a single integer *state* `x` and maps
each symbol to a state transition `C(s, x)` that is invertible.

The rANS variant ("range ANS") — the one this repository implements — was
popularised by Fabian Giesen's public-domain `ryg_rans` repository
(`rans_byte.h`, `rans64.h`, `rans_word_sse41.h`, `main_alias.cpp`), which
became the de-facto reference implementation for the game-industry and
compression-community usage of rANS.  `ryg-rans-rs` is a **forensic
reconstruction** of that exact implementation: same stream formats,
byte-identical output, cross-decoding proven by oracle courts.  The pinned
revision is recorded in `docs/bitstream-contract.md`.

Why reconstruct rather than reimplement?  Because the value of a codec is
interoperability: a stream encoded by this library must decode on the
original C implementation and vice versa.  Any "improvement" that changes
the stream is a format break.  The reconstruction discipline — byte-exact
parity with a pinned upstream revision, proven by an oracle — is the only
way to get the interoperability benefit while rewriting the code in a
memory-safe language.

## 2. Why rANS

For the use case this repository targets (general-purpose entropy coding
with strict correctness requirements), rANS has a specific profile:

| Property | rANS | Huffman | Arithmetic coding |
|----------|------|---------|-------------------|
| Compression vs entropy bound | within ~1% at scale 12 | loses `log2` bits per symbol | essentially exact |
| Decode speed (per symbol) | a few integer ops | table lookup | multiply + interval update per symbol |
| State | one integer | none | an interval |
| Hardware vectorisation | natural (interleaved lanes) | awkward | serial by construction |
| Table size | 4 KiB–64 KiB | small | none |

The decisive property for this project is the last one: rANS's state
machine vectorises because multiple independent states can be interleaved
(see §6), which is what makes multi-GB/s SIMD decode possible.  The
compression penalty at `scale_bits = 12` is negligible for the target
workloads, and the strict, stateless arithmetic makes formal verification
tractable (paper 0007).

## 3. The arithmetic: deriving `C(s, x)` and `D(x)`

### 3.1 The forward (encode) transition

Let the alphabet symbols `s ∈ [0, N)` have normalized frequencies `freq[s]`
that sum to `M = 2^scale_bits` (4096 at scale 12), with cumulative starts
`cum[s]`.  Define `start = cum[s]` and `freq = freq[s]`.

The encoding transition is:

```text
C(s, x) = floor(x / freq) * M + (x mod freq) + start
```

with the guarantee that if `x ≥ L` (the renormalization lower bound) then
`C(s, x) ≥ L` as well — the state never drops below `L`, so the decoder
only needs to read input when the state grows past `M * L`.

The inverse (decode) transition reads a symbol out of the state and
contracts it:

```text
slot = x mod M          // which slot inside the current state
sym  = symbol owning slot
x    = freq[sym] * floor(x / M) + (slot - cum[sym])
```

Decoding is **renormalization-first**: before reading the next symbol, the
decoder shifts new input bits/words into the state until `x ≥ L`.  This is
the mirror of the encoder, which flushes state words out.

### 3.2 Why the formulas take this exact shape

The `floor(x / freq) * M + (x mod freq)` structure is the "range" in rANS:
each symbol's values are spread through the state space in `freq`-sized
strides of size `M`.  The `+ start` offset aligns the symbol's range to its
cumulative position.  Three properties must hold exactly:

1. **Bijectivity**: for a fixed `x`, the map `s → C(s, x)` is injective —
   each symbol lands in a disjoint range and the decoder's `slot` uniquely
   identifies the symbol.
2. **Monotonicity**: `C(s, x)` must be monotonically increasing in `x` so
   the decoder's `floor(x / M)` recovers the quotient.
3. **Range preservation**: `x ≥ L ⟹ C(s, x) ≥ L`, so the encoder never
   needs to renormalize before the first symbol and the decoder's
   renormalization is always sufficient.

These three properties are the invariant set that every one of the four
surfaces (byte, R64, word, alias) re-establishes with different constants.

## 4. Division vs reciprocal: why the fast path exists

### 4.1 The cost of division

The encode transition requires `q = x / freq` and `r = x mod freq`.
Integer division on x86-64 is a multi-uop instruction with a latency of
roughly 20–40 cycles (throughput one per ~4 cycles on recent cores),
whereas multiply-high is a single uop.  In a hot loop that processes one
symbol per iteration, division dominates: the difference between 3 GB/s and
10+ GB/s is almost entirely this instruction.

### 4.2 Alverson's reciprocal trick

Instead of computing `x / freq` directly, precompute a fixed-point
reciprocal of `freq`:

```text
recip_freq = ceil(2^shift / freq)     // shift = scale_bits + 32 (byte),
                                      //         scale_bits + 64 (R64)
q ≈ (x * recip_freq) >> shift         // multiply-high
```

Because `x` is bounded (`x < M * L`), a careful choice of `shift` makes the
approximation exact for every reachable `x`.  The bias analysis is:

```text
x * ceil(2^s / freq)  ∈  [x * 2^s / freq,  x * (2^s / freq + 1))
q_approx = floor(x * recip / 2^s)  ∈  {q, q + 1}  with the right
bounds on shift making the +1 case impossible
```

The upstream implementation uses a slightly adjusted bias (`(M - 1) <<
shift` in the error term) so the multiplication never overestimates.  This
exact bias is part of the bitstream contract — a decoder that uses a
different reciprocal convention will produce different bytes.

The Kani proofs (`kani/reciprocal_proof.rs`, `kani/r64_reciprocal_proof.rs`)
mechanically verify `reciprocal_put_symbol == division_put_symbol` for
every reachable state at a set of concrete frequencies (freq 1, 2, 3, 7,
16, 255, 4095 for byte; 1, 2, max for R64), and the oracle court verifies
byte-exact parity with the upstream C implementation over randomized
models.  The `freq == 1` case is special-cased (`x = (x << scale_bits) +
start`) because the general reciprocal path cannot represent a reciprocal
of 1 with the required precision in the byte variant's shift budget.

### 4.3 Why the division path is kept

The division-based path is the *reference*: it is what the Kani proofs and
the oracle compare the reciprocal path against.  Keeping both makes the
"two implementations of the same contract" pattern that drives the whole
verification strategy (paper 0007).  The CLI's `compare arithmetic`
subcommand exercises both paths byte-for-byte on the same input.

## 5. Renormalization: the contract that must not move

Every surface maintains its state in `[L, M * L)`:

| Surface | `L` | `M` | Renorm unit |
|---------|-----|-----|-------------|
| Byte | `1 << 23` | `1 << 31` (implicit) | byte |
| R64 | `1 << 31` | `1 << 63` (implicit) | u32 word |
| Word | `1 << 16` | `4096 = 1 << 12` | u16 word |

Encoder: after emitting `C(s, x)`, while `x >= M * L` flush the low 8/16/32
bits of the state.  Decoder: before reading a symbol, while `x < L` shift
the next input unit in.

The asymmetry — the encoder flushes **after** the transition, the decoder
renormalizes **before** — is what makes the stream self-terminating on the
decode side without a symbol count in the common case, and it is exactly
why the *order* of flush and transition is part of the format contract.
Changing the order produces a stream that decodes to different bytes.

## 6. Interleaving: why multiple states

### 6.1 The problem with one state

A single rANS state is a serial dependency chain: every symbol's transition
depends on the previous symbol's state.  That serializes the encode/decode
loop and — critically — prevents SIMD, because the next symbol's state is
unknown until the current one is computed.

### 6.2 Two-state interleaving (byte)

`rans_byte.h` provides a two-stream interleaved encoder/decoder: symbols
alternate between two independent states, each renormalized with its own
byte stream.  On a superscalar core the two chains overlap, hiding the
latency of the reciprocal multiply.

### 6.3 K-way interleaving (word)

The word coder generalises this to 8 and 16 lanes: symbol `i` is processed
by lane `i mod K`.  Each lane is an independent rANS state with its own
renormalization word stream.  Decoding output byte `i` reads lane `i & 15`,
advances that lane's state, and renormalizes if needed.  The lanes' input
word streams are the interleaved tails of the K encoder streams.

This is the enabling structure for SIMD: a vector of K states can be
advanced with one `vpand`-style mask, one gather, and K parallel multiply
units — no cross-lane dependency exists by construction.  Paper 0003 covers
the kernels; paper 0002 covers the word coder's table layout.

## 7. Performance: what the numbers mean

The sealed performance evidence lives in `evidence/performance/` (run
`phase-l-20260802b`, 800 cases × 100 samples).  The methodology is paper
0005.  The headline properties, measured on an AMD Ryzen 7 9800X3D with
`-C target-cpu=native`:

* Scalar word decode runs in the hundreds of MB/s per core;
* SSE4.1 8-way and AVX-512 16-way kernels reach multiple GB/s;
* The parallel engine adds a ~1.4× single-worker overhead over the raw
  kernel, dominated by the mandatory dual SHA-256 per block (payload +
  decoded-output integrity), not by scheduling.

The honest reading: **throughput is bought with integrity work**.  The
design deliberately hashes every block twice; a user who drops integrity
gains speed, but the default is strict.  No benchmark in this repository
claims superiority over an implementation with different integrity work —
comparative methodology (L.14) is documented separately in
`docs/performance/comparative.md`.

## 8. SIMD: what the kernels do and why

The SIMD layer (paper 0003) implements the word-coder decode path for
SSE4.1 (8-way), AVX2 (manual/hardware gather, 2×8, Uniform256), AVX-512VL
(8-way, manual gather, 2×8) and AVX-512 (16-way, manual gather).  The key
design rule, inherited from the pinned upstream `rans_word_sse41.h`: **the
decode table is a 4096-slot packed table indexed by `state & 0xfff`**, and
each slot packs `(freq, bias, sym)` into one u32 so a single gather loads
an entire step's inputs.

Every kernel has a scalar reference, a differential test, report-parity
courts (words consumed, final states), disassembly courts (the expected
instructions are present in native builds), and a ledgered unsafe surface
(`unsafe-ledger.toml`).  The `# Safety` contract for each `unsafe fn` is
the complete story: provenance, bounds, alignment, CPU-feature
requirements, and caller list.

## 9. Failure modes: the ones that actually happen

1. **Reciprocal bias drift** — a "helpful" change to the bias term silently
   breaks cross-decoding with upstream.  Caught by the oracle court.
2. **Renormalization boundary changes** — moving `L` "for efficiency"
   produces self-consistent-but-wrong streams.  Caught by cross-decoding
   and truncation-at-every-byte fuzz targets.
3. **Flush/transition order inversion** — the most common reimplementation
   mistake.  Caught by the state-trace oracle (`trace` court).
4. **Truncated stream reads** — decoding past the payload end.  Caught by
   the `malformed` parser, bounds-checked table indexing, and the fuzz
   target that truncates at every byte/word.
5. **Model sum drift** — a normalized model whose frequencies do not sum to
   `1 << scale_bits` produces a table with holes or overlaps.  Caught by
   the normalizer property tests and the Kani packed-entry proofs.
6. **Zero-frequency symbols** — a symbol with `freq = 0` has no slots; a
   decoder that searches for it returns garbage.  The packed-table
   constructor rejects it (paper 0002).

## 10. Oracle strategy: why byte-exact parity is the standard

The oracle court compiles the pinned upstream C (with its own build
recipe), runs both implementations over a deterministic corpus of models
and lengths, and requires byte-identical encoded output and byte-identical
decoded output (plus matching words-consumed/final-state reports where the
Rust side exposes them).  The receipts in `evidence/receipts/` (court IDs
`RYG_RANS.BYTE.*`, `RYG_RANS.R64.*`, `RYG_RANS.WORD.*`,
`RYG_RANS.ALIAS.*`, `RYG_RANS.SSE41.*`, `RYG_RANS.AVX512*.*`) are the
sealed record.  The methodology is `docs/oracle-method.md`; the residual
doctrine (`docs/residual-doctrine.md`) governs how a parity failure is
recorded — never silently "fixed" in the docs.

## 11. Evidence: the chain from code to seal

Every algorithm in this repository is traceable:

```text
public API → execution path → observable effect → test → adversarial court
→ evidence artifact → seal verification
```

The seal gate (`cargo xtask seal`) is the single authoritative final gate.
For rANS specifically the chain is: the arithmetic in `ryg-rans-rs-core`
→ the byte-exact oracle receipts → the Kani proofs → the fuzz targets →
the comparative court → the sealed performance run.  A claim (e.g. "byte
rANS is bit-exact with upstream") is not true because this paper says so;
it is true because the receipt `RYG_RANS.BYTE.RECIPROCAL.S12` exists, its
hash verifies, and the seal gate passes.

## 12. Future work

* Wider state (128-bit) or adaptive (tANS-style) variants — outside the
  pinned contract, would be a new surface with its own receipts.
* Encoder SIMD: the decode side vectorises naturally; the encode side has a
  serial dependency per lane that limits gains (documented in paper 0003).
* Calibrated crossover thresholds for backend selection on more
  microarchitectures (the current model is deliberately conservative:
  non-explicit policies select scalar).
* Compression-efficiency studies at higher `scale_bits` (the word coder is
  pinned at 12; the byte/R64 surfaces accept configurable scales).
