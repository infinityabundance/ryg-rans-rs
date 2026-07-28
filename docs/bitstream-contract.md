# Bitstream Contract

## Byte-aligned 32-bit rANS

### State
- 32-bit unsigned integer.
- Initialized to `RANS_BYTE_L = 2^23`.
- Must always stay in valid normalization interval after encoding/decoding.

### Encoding (Reverse Order)
1. Renormalize: if `state >= x_max`, emit LSB bytes until `state < x_max`.
   - `x_max = ((L >> scale_bits) << 8) * freq`
2. Compute new state: `C(s, x) = floor(x / freq) * M + (x % freq) + start`
   - where `M = 1 << scale_bits`
3. Process symbols in **reverse order** (last input symbol first).
4. Output bytes written **backwards** from end of buffer.

### Fast Encoding (Reciprocal)
- Instead of `x / freq`, compute `q = mul_hi(x, rcp_freq) >> rcp_shift`.
- New state: `x + bias + q * cmpl_freq`.
- For `freq == 1`: `rcp_freq = ~0u32`, `rcp_shift = 0`, `bias = start + M - 1`.

### Decoding (Forward Order)
1. Read 4 bytes as little-endian u32 for initial state.
2. Get cumulative frequency: `state & (M - 1)`.
3. Look up symbol from cumulative-to-symbol table.
4. Advance: `x = freq * (state >> scale_bits) + (state & (M - 1)) - start`.
5. Renormalize: if `x < L`, read bytes until `x >= L`.
6. Process symbols in **forward order** (first encoded symbol first).

### Flush
- Write remaining 32-bit state as 4 little-endian bytes.

## 64-bit rANS

### State
- 64-bit unsigned integer (63-bit effective).
- Initialized to `RANS64_L = 2^31`.

### Encoding
1. Renormalize: if `state >= x_max`, emit one 32-bit word (native endian).
   - `x_max = ((RANS64_L >> scale_bits) << 32) * freq`
2. Same arithmetic as byte rANS but with 64-bit state.
3. Flush: write 2 × 32-bit words (u32 cast of lower and upper halves).

### Decoding
1. Read 2 × 32-bit words as initial 64-bit state.
2. Advance: `x = freq * (state >> scale_bits) + (state & (M - 1)) - start`.
3. Renormalize: if `x < L`, read one 32-bit word: `x = (x << 32) | word`.

## Host Endianness
- The 32-bit rANS byte stream is little-endian (byte-oriented, always unambiguous).
- The 64-bit rANS emits native-endian `uint32_t` words.
  - On little-endian x86-64, this matches little-endian.
  - On big-endian hosts, the upstream format would differ.
