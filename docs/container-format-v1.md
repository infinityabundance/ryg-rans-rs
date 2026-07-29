# RYGRANS Container Format v1

> **Canonical block-streaming container for rANS-compressed data.**  
> Version 1.0 — little-endian, stream-decodable, bounded-memory, deterministically serialized.

---

## 1. Overview

The RYGRANS v1 container format wraps rANS-compressed data in a self-describing,
block-structured container. It supports:

- **Multiple codec formats**: Byte rANS, 64-bit rANS, Word rANS, Alias method
- **Multiple block kinds**: RAW (uncompressed), RLE (single-symbol), RANS (compressed)
- **Streaming encode/decode**: bounded memory, no seek required
- **Integrity verification**: per-block and per-stream SHA-256 hashes
- **Deterministic output**: identical input + options → identical container bytes

### 1.1 File structure

```
┌─────────────────────┐
│ File Header (32 B)  │  ← fixed-size, versioned
├─────────────────────┤
│ Block Record 0      │  ← variable-size, independently verifiable
│   ├ Block Header    │
│   ├ Model Data      │  (RANS only)
│   └ Payload Data    │
├─────────────────────┤
│ Block Record 1      │
│   ...               │
├─────────────────────┤
│ Footer (104 B)      │  ← terminal totals and stream hashes
└─────────────────────┘
```

### 1.2 Byte ordering

All multi-byte integers are **little-endian**.

All hashes are **SHA-256**, represented as raw 32-byte sequences in the binary
format, and as 64-character lowercase hexadecimal strings in JSON/metadata.

### 1.3 Definitions

| Term | Definition |
|------|------------|
| `u8` | Unsigned 8-bit integer |
| `u16` | Unsigned 16-bit integer, little-endian |
| `u32` | Unsigned 32-bit integer, little-endian |
| `u64` | Unsigned 64-bit integer, little-endian |
| `[u8; N]` | N bytes of opaque data |
| `SHA256` | 32-byte SHA-256 hash |

---

## 2. File Header

Fixed-size, exactly 32 bytes.

### 2.1 Layout

| Offset | Size | Field | Value / Meaning |
|--------|------|-------|-----------------|
| 0 | 8 | `magic` | `"RYGRANS\0"` (0x52 0x59 0x47 0x52 0x41 0x4E 0x53 0x00) |
| 8 | 2 | `major_version` | `1` |
| 10 | 2 | `minor_version` | `0` |
| 12 | 2 | `header_length` | `32` |
| 14 | 2 | `flags` | Bitfield (see §2.2) |
| 16 | 2 | `default_codec_id` | Default codec for RANS blocks (see §6) |
| 18 | 1 | `default_scale_bits` | Default scale bits (e.g., `12`) |
| 19 | 1 | `default_model_mode` | `0` = per-block, `1` = global, `2` = uniform |
| 20 | 4 | `declared_block_size` | Maximum uncompressed bytes per block |
| 24 | 8 | `reserved` | Must be zero |

### 2.2 Flags bitfield

| Bit | Name | Meaning |
|-----|------|---------|
| 0 | `GLOBAL_MODEL` | All RANS blocks share one model (written before first block) |
| 1 | `ALWAYS_COMPRESS` | Encode chose size over kind (diagnostic) |
| 2 | `HAS_EXTRA_HEADER` | Reserved for future extensions |
| 3–15 | (reserved) | Must be zero |

### 2.3 Validation

A decoder must verify:

- `magic` equals `"RYGRANS\0"`
- `major_version == 1` (reject unknown major versions)
- `header_length == 32` (reject unexpected header sizes)
- `reserved` fields are zero
- Unknown flags bits are zero (unless the spec marks them ignorable)
- `declared_block_size` is non-zero and ≤ configured limit
- `default_scale_bits` is valid for the codec (1..=16 for byte, 1..=31 for R64, 12 for word)

---

## 3. Block Record

Each block record is independently bounded and verifiable.

### 3.1 Block Header

Fixed-size, exactly 104 bytes.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `tag` | `"BLK1"` (0x42 0x4C 0x4B 0x31) |
| 4 | 2 | `block_header_length` | `104` |
| 6 | 1 | `block_version` | `1` |
| 7 | 1 | `block_kind` | `0`=RAW, `1`=RLE, `2`=RANS |
| 8 | 8 | `block_index` | 0-based, sequential, monotonically increasing |
| 16 | 2 | `codec_id` | Codec format ID (see §6) |
| 18 | 1 | `scale_bits` | Scale bits for this block |
| 19 | 1 | `state_count` | Number of interleaved states (1, 2, 8, or 16) |
| 20 | 1 | `model_encoding` | `0`=canonical sparse, `1`=global reference |
| 21 | 1 | `integrity_id` | `0`=SHA-256 |
| 22 | 2 | `reserved` | Must be zero |
| 24 | 4 | `uncompressed_length` | Bytes after decompression |
| 28 | 4 | `payload_length` | Bytes of compressed payload |
| 32 | 4 | `model_length` | Bytes of model data (0 for RAW/RLE) |
| 36 | 4 | `reserved2` | Must be zero |
| 40 | 32 | `payload_sha256` | SHA-256 of the payload bytes |
| 72 | 32 | `decoded_sha256` | SHA-256 of the decoded bytes |

### 3.2 Block kinds

#### RAW (kind=0)

- No model data (`model_length = 0`)
- `payload_length == uncompressed_length`
- Payload is the raw data
- `codec_id`, `scale_bits`, `state_count` use canonical zero or `default_codec_id`

#### RLE (kind=1)

- No model data (`model_length = 0`)
- Payload is exactly 1 byte: the repeated symbol
- Decoder produces `uncompressed_length` copies of that byte
- Codec and scale fields use zero

#### RANS (kind=2)

- Model data precedes payload (see §4)
- Payload is the rANS-compressed byte stream
- Codec, scale, and state fields are meaningful

### 3.3 Block index invariant

Block indices must:

- Start at 0
- Increase by exactly 1 for each subsequent block
- Be checked by the decoder

A gap, duplicate, or decreasing index is a format error.

### 3.4 Validation

Before reading payload:

```text
1. Verify tag == "BLK1"
2. Verify block_version == 1
3. Verify block_kind in {0, 1, 2}
4. Verify block_index == expected_index
5. Verify codec_id is supported
6. Verify scale_bits is valid for codec
7. Verify state_count matches codec
8. Verify reserved fields are zero
9. Verify uncompressed_length <= max_block_size
10. Verify payload_length <= max_block_size
11. Verify model_length <= max_model_size (0 for RAW/RLE)
12. Verify no arithmetic overflow in total accumulation
13. Check limits before allocation
```

---

## 4. Canonical Model Encoding

Used for all RANS block models.

### 4.1 Binary format

```text
u16  entry_count
repeated entry_count times:
    u8   symbol
    u32  normalized_frequency
```

### 4.2 Constraints

- `entry_count` must be ≤ 256
- Symbols must be strictly ascending
- No duplicate symbols
- No zero frequencies
- Frequencies sum to exactly `1 << scale_bits`
- `model_length` must equal `2 + entry_count * 5` (2 + entry_count × (1 + 4))
- Every unused byte sequence must be rejected

### 4.3 Global model encoding

When `GLOBAL_MODEL` flag is set and `model_encoding = 1`:
- Model data appears once before the first block
- Block model fields reference the global model by index 0
- The global model is serialized in the same canonical format

---

## 5. Footer

Fixed-size, exactly 104 bytes. Required. Missing footer = truncated container.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0 | 4 | `tag` | `"END1"` (0x45 0x4E 0x44 0x31) |
| 4 | 2 | `footer_length` | `104` |
| 6 | 1 | `footer_version` | `1` |
| 7 | 1 | `flags` | Reserved, must be zero |
| 8 | 8 | `block_count` | Total blocks in container |
| 16 | 8 | `total_uncompressed_length` | Sum of all uncompressed lengths |
| 24 | 8 | `total_payload_length` | Sum of all payload lengths |
| 32 | 32 | `container_sha256` | SHA-256(header \|\| all block records) |
| 64 | 32 | `decoded_stream_sha256` | SHA-256(concatenation of all decoded blocks) |
| 96 | 8 | `reserved` | Must be zero |

### 5.1 Hash definitions

```text
container_sha256 =
    SHA-256(file_header || block_record_0 || block_record_1 || ... || block_record_N)

decoded_stream_sha256 =
    SHA-256(decoded_block_0 || decoded_block_1 || ... || decoded_block_N)
```

The footer itself is NOT included in `container_sha256`.

### 5.2 Validation

- Verify `tag == "END1"`
- Verify `footer_version == 1`
- Verify `flags` and `reserved` are zero
- Verify `block_count` equals the number of blocks encountered
- Verify `total_uncompressed_length` and `total_payload_length` match accumulated values
- Verify `container_sha256` matches recomputed hash
- Verify `decoded_stream_sha256` matches recomputed hash
- Verify no trailing bytes exist after footer

---

## 6. Codec Registry

Codec IDs identify the **stream format** (number of states, renormalization unit).
They do NOT identify the arithmetic implementation (division vs reciprocal) or
the decode backend (scalar vs SIMD).

| ID | Name | Renorm Unit | States | Scale Bits | Notes |
|----|------|-------------|--------|------------|-------|
| 1 | `BYTE_SINGLE` | 8-bit byte | 1 | 1..16 | Single-state byte rANS |
| 2 | `BYTE_INTERLEAVED2` | 8-bit byte | 2 | 1..16 | Two-state interleaved byte rANS |
| 3 | `R64_SINGLE` | 32-bit word | 1 | 1..31 | Single-state 64-bit rANS |
| 4 | `R64_INTERLEAVED2` | 32-bit word | 2 | 1..31 | Two-state interleaved 64-bit rANS |
| 5 | `WORD_SINGLE` | 16-bit word | 1 | 12 | Single-state Word rANS (table-based) |
| 6 | `WORD_INTERLEAVED2` | 16-bit word | 2 | 12 | Two-state interleaved Word rANS |
| 7 | `WORD_INTERLEAVED8` | 16-bit word | 8 | 12 | Eight-way interleaved Word rANS |
| 8 | `WORD_INTERLEAVED16` | 16-bit word | 16 | 12 | Sixteen-way interleaved Word rANS |
| 9 | `ALIAS_SINGLE` | 8-bit byte | 1 | 8..17 | Single-state alias method |
| 10 | `ALIAS_INTERLEAVED2` | 8-bit byte | 2 | 8..17 | Two-state interleaved alias method |

### 6.1 State counts

| State Count | Codec Families |
|-------------|----------------|
| 1 | BYTE_SINGLE, R64_SINGLE, WORD_SINGLE, ALIAS_SINGLE |
| 2 | BYTE_INTERLEAVED2, R64_INTERLEAVED2, WORD_INTERLEAVED2, ALIAS_INTERLEAVED2 |
| 8 | WORD_INTERLEAVED8 |
| 16 | WORD_INTERLEAVED16 |

### 6.2 Arithmetic vs Backend

Division and reciprocal are arithmetic **implementations** that produce identical
canonical streams. They are not separate codec IDs.

SSE4.1, AVX512VL, and AVX512 are decode **backends**. They are not separate codec IDs.

The codec ID identifies the stream format only.

---

## 7. Determinism

For identical input and options, the container must be byte-identical across runs,
platforms, and backends.

### 7.1 Sources of nondeterminism (forbidden)

- Timestamps in header or footer
- Random identifiers
- Host-dependent values (hostname, PID, thread ID)
- Platform-dependent integer sizes
- Uninitialized padding bytes
- Floating-point arithmetic in model normalization
- Hash algorithm selection

### 7.2 Sources of determinism (required)

- Model normalization uses integer arithmetic only
- Block selection (RAW vs RLE vs RANS) is deterministic
- Tie-breaking rules are fully specified
- Hash computation is canonical (no endianness ambiguity)
- Serialization order is fixed

---

## 8. Extension and Versioning

### 8.1 Forward compatibility

- Unknown `major_version` values must be rejected
- Unknown `minor_version` values within supported major version are acceptable
- Unknown flags bits that are not marked ignorable must be rejected
- Unknown block kinds must be rejected
- Unknown codec IDs must be rejected
- Unknown model encodings must be rejected

### 8.2 Backward compatibility

- v1 decoders must accept v1.0, v1.1, etc.
- v1 decoders may accept v1.x where x > 0
- v1 decoders must reject v2

### 8.3 Golden fixtures

Once v1 is published, golden binary fixtures are committed and must remain
byte-identical across releases. Any incompatible change requires a new format version.

---

## 9. File Extension and MIME

- Recommended extension: `.rygr`
- Recommended MIME type: `application/x-rygrans` (pending registration)
- Default binary name: `ryg-rans`
