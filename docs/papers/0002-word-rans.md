# Paper 0002 — Word rANS: the table-based coder

> *Layer: Algorithm/Subsystem.  Companion: `docs/bitstream-contract.md`,
> `docs/papers/0001-rans-design.md` (§5–6), `docs/papers/0003-simd.md`.
> This paper explains the word coder's mathematics and table layout; the
> code is `ryg-rans-rs-core` (scalar surface) and `ryg-rans-rs-simd`
> (packed tables + kernels).*

## 1. Why a "word" coder exists

The byte coder (`rans_byte.h`) renormalizes one byte at a time, which means
it spends a renormalization step roughly every `log_M(L)` symbols.  The 64-bit
coder (`rans64.h`) renormalizes 32 bits at a time but uses a 63-bit state
that does not fit the SIMD width or the L1 cache as comfortably.  The word
coder sits in between: a 32-bit state renormalized by 16-bit words, with
`M = 4096 = 1 << 12` slots — a table size that is simultaneously:

* small enough to fit in L1 (16 KiB packed, 12 KiB unpacked);
* aligned to the natural SIMD gather width;
* exactly the resolution at which frequency normalization error becomes
  negligible for byte-alphabet workloads.

The upstream origin is `rans_word_sse41.h`, which is the surface all SIMD
work is built on.  `RANS_WORD_SCALE_BITS` is pinned to 12 by upstream and
therefore by this repository's bitstream contract; every formula below
assumes it.

## 2. The mathematics

### 2.1 Constants

```text
RANS_WORD_L            = 1 << 16 = 65536      // renormalization lower bound
RANS_WORD_M            = 1 << 12 = 4096       // number of slots
RANS_WORD_SCALE_BITS   = 12                   // frequencies sum to 4096
state space            = [L, M * L) = [65536, 268435456)
renorm unit            = u16 (little-endian word)
renorm shift           = 16 bits per word
```

### 2.2 Encoder transition

```text
C(s, x) = (x / freq[s]) << 12 | (x % freq[s]) + cum[s]
```

where `cum[s] = Σ_{t<s} freq[t]`.  Renormalization after each symbol:
while `x >= M * L` flush the low 16 bits of `x` to the output stream and
shift `x >>= 16`.

### 2.3 Decoder transition

```text
slot = x & (M - 1)                 // vpand-style mask, no modulo needed
sym  = table.slot2sym[slot]
x    = freq[sym] * (x >> 12) + (slot - cum[sym])
```

Renormalization before each symbol: while `x < L`, read the next u16 word
`w` and set `x = (x << 16) | w`.

The decoder's `x & (M - 1)` is the key SIMD-friendly property: with `M`
a power of two, the modulo is a bitwise AND, and the symbol lookup is a
table read at a known index.

## 3. The packed table: one u32 per slot

### 3.1 The unpacked layout (scalar reference)

The scalar path uses two parallel arrays (`RansWordTables`):

```text
slots[slot]    = RansWordSlot { freq: u16, bias: u16 }   // 4 bytes
slot2sym[slot] = u8                                      // 1 byte
```

Decoding needs three quantities per step: `freq`, `bias = slot - cum[sym]`,
and `sym`.  In the unpacked layout the decoder performs two loads (slot,
then symbol) and derives `bias` from `slot - cum[sym]`.

### 3.2 The packed layout (SIMD)

The packed table (`PackedWordTable`, 16 KiB, 64-byte aligned) stores all
three quantities in a single u32 per slot:

```text
bits 0..11   frequency (12 bits)   entry & 0x0fff
bits 12..23  bias      (12 bits)   (entry >> 12) & 0x0fff
bits 24..31  symbol    (8 bits)    (entry >> 24) as u8
```

A single 32-bit gather (`vpgatherdd`) loads an entire step's inputs in one
transaction; the three fields are extracted with `vpand` / `vpsrld` /
`vpand` — each a single uop on modern x86.  This is the difference between
the two-load-per-slot pattern of the unpacked layout and the one-load
pattern of the packed layout, and it is why the packed layout exists.

### 3.3 Construction

`PackedWordTable::from_freqs(freqs, cum, scale_bits)`:

1. Validates `scale_bits == 12`, `freqs.len() == 256`, `cum.len() == 257`,
   `cum[0] == 0`, `cum[256] == 4096`, monotonicity, and
   `cum[s+1] - cum[s] == freq[s]`.
2. For each slot `0..4096`, finds the owning symbol by scanning the
   cumulative ranges and packs `(freq, bias = slot - start, sym)`.
3. Rejects zero-frequency symbols: a symbol with `freq = 0` owns no slots,
   and a slot with no owner would produce `freq = 0`, which is invalid
   (`ModelError::ZeroFrequency`).

The construction is O(4096 × symbols) worst case (linear scan per slot);
the model cache (Phase L.8) builds it once per unique model and shares it
across blocks via `Arc` — see `crates/ryg-rans-rs-parallel/src/cache.rs`.

### 3.4 Why the three-field packing is exactly 12/12/8

`freq ≤ 4095` fits 12 bits; `bias < freq ≤ 4095` fits 12 bits; `sym < 256`
fits 8 bits.  The packing is tight — no wasted bits — which keeps the entry
at exactly 32 bits, the width of a single gather element.  The Kani proof
`kani/packed_entry_proof.rs` verifies that `try_pack` accepts exactly the
values in range and rejects everything else.

## 4. Decoder layout and the interleaved loop

The K-way decoder (K = 8 or 16) initializes K states from the first `2K`
u16 words (each state = `w0 | w1 << 16`), then processes output bytes in a
K-wide round: for each byte index `i`, lane `i & (K-1)` decodes one symbol.
The tails (`expected_len mod K` bytes) are handled by a scalar tail loop
with exactly `K` possible lengths (0..K-1); each tail length is covered by
the fuzz and differential tests.

The words-consumed and final-states report is a first-class output of the
kernels (`DecodeReport`), pinned by the report-parity courts: every
executable backend for a given block must produce identical output **and**
identical words-consumed/final-states, because the report is a property of
the stream, not of the kernel that happened to run.

## 5. Cache behaviour

The packed table is 16 KiB, aligned to 64 bytes — one L1 cache set's worth
of streaming data, and exactly what a gather-heavy loop wants.  The slot
index `x & 0xfff` spreads accesses across the table; for skewed models the
hot slots cluster in cache.  The 8-way and 16-way kernels each maintain K
independent state registers, so the gather addresses are independent and
the memory-level parallelism hides table latency.

The intentional trade: the table is built once per model and shared (model
cache), so the per-block cost of decode is the stream processing only —
no table construction in the hot path.  This is what makes the parallel
engine's per-block overhead small enough to scale (paper 0004).

## 6. Complexity

* Table construction: O(4096 × S) worst case where S ≤ 256 is the number
  of nonzero symbols; O(4096) for uniform models (single scan).
* Decode per byte: O(1) amortized — one gather + a handful of ALU ops;
  renormalization amortizes to O(1) per byte because each symbol consumes
  at most one word and most symbols consume none.
* Encode per byte: O(1) with the reciprocal path, O(division latency)
  with the division path.
* Memory: table 16 KiB (packed) / 12 KiB (unpacked) per model, shared
  across blocks.

## 7. Alternatives rejected

| Alternative | Why rejected |
|-------------|--------------|
| Binary search on cumulative frequencies per symbol | O(log S) per symbol and hard to vectorise; the table lookup is O(1) and SIMD-friendly |
| Hash map from slot to symbol | Non-contiguous memory, unpredictable access, no SIMD gather pattern |
| Larger `scale_bits` (e.g. 14) | Not upstream; changes the stream format; 16 KiB table is the L1 sweet spot |
| Smaller `scale_bits` (e.g. 10) | Larger compression loss; still not upstream |
| 32-bit renormalization words | That is the R64 surface — different state width, different stream |
| Building the table per block in the parallel engine | The model cache exists precisely to avoid this (Phase L.8) |

## 8. Why this implementation

The repository implements the word coder twice, deliberately:

* a **scalar surface** in `ryg-rans-rs-core` with the upstream slot
  layout, used by the CLI and as the reference;
* **SIMD kernels** in `ryg-rans-rs-simd` with the packed layout, each with
  its own scalar reference for differential testing.

The two layouts must agree on the stream format (they do — the packed
table is just a fused encoding of the unpacked one), and the report-parity
courts pin that agreement.  Keeping the packed and unpacked layouts
separate is what makes the scalar reference readable enough to audit while
the SIMD path stays fast enough to ship.

## 9. Failure modes specific to the word coder

1. **Zero-frequency symbols**: a model with `freq[s] = 0` leaves slots
   unowned; the packed constructor rejects the model rather than emitting a
   table with garbage slots.
2. **Slot overflow at construction**: `bias` or `freq` exceeding the field
   width (`try_pack` rejects values > 4095) — impossible for a validated
   model, provable by the packed-entry Kani proofs.
3. **Table index out of bounds**: `slot = x & 0xfff` is always < 4096 by
   construction, but the kernels still bounds-check the payload's word
   stream (truncation at every word is fuzzed).
4. **Interleaving tail mismatch**: an `expected_len % K` tail decoded with
   the wrong lane mask produces wrong output for the last few bytes; the
   differential tests cover every tail length 0..K-1.
5. **Report divergence**: two backends producing different
   words-consumed/final-states for the same stream would indicate a kernel
   bug; the report-parity courts would fail.
