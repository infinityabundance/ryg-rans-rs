# Upstream Source Inventory

## Repository: ryg_rans
**URL:** https://github.com/rygorous/ryg_rans  
**Pinned Commit:** c9d162d996fd600315af9ae8eb89d832576cb32d  
**Date:** 2018-11-25  
**Host:** x86_64, little-endian  

## File Inventory

| File | Description | Reconstructed |
|------|-------------|---------------|
| `rans_byte.h` | Byte-aligned 32-bit rANS encoder/decoder | Core: `crates/ryg-rans-core/src/lib.rs` |
| `rans64.h` | 64-bit rANS encoder/decoder (32-bit words) | Not yet |
| `rans_word_sse41.h` | Word-aligned SSE4.1 rANS decoder | Not yet |
| `main.cpp` | Example program: byte rANS, stats, interleaving | Test reference |
| `main64.cpp` | Example program: 64-bit rANS | Test reference |
| `main_simd.cpp` | Example program: SIMD/interleaved word rANS | Test reference |
| `main_alias.cpp` | Example program: alias method + rANS | Not yet |
| `platform.h` | Platform utilities (timer, rdtsc, ALIGNSPEC) | Oracle only |
| `Makefile` | Build rules | Oracle only |
| `README` | Documentation | Reference |
| `LICENSE` | Public domain | Acknowledgement |
| `book1` | Test corpus (Calgary corpus) | Oracle test data |

## Upstream Oracle Binaries

All built from the pinned commit:

| Binary | Source | Status |
|--------|--------|--------|
| `exam` | `main.cpp` + `rans_byte.h` | Built and verified |
| `exam64` | `main64.cpp` + `rans64.h` | Built and verified |
| `exam_sse41` | `main_simd.cpp` + `rans_word_sse41.h` | Built and verified |
| `exam_alias` | `main_alias.cpp` + `rans_byte.h` | Built and verified |

## Surface Classification

| Surface | Status | Evidence |
|---------|--------|----------|
| Byte rANS (division-based) | `full` | 21 passing unit tests, oracle cross-tests pending |
| Byte rANS (reciprocal fast) | `full` | Tests pass, reciprocal==division proven |
| Byte rANS (two-state interleaved) | `full` | Interleaved roundtrip test passes |
| Byte rANS (encoder symbol init) | `full` | Tested with freq=1, freq=2, various freqs |
| Byte rANS (backward writer) | `full` | Tests pass |
| Byte rANS (forward reader) | `full` | Tests pass |
| 64-bit rANS (rans64.h) | `scaffold` | Not yet implemented |
| Word-aligned rANS (scalar) | `scaffold` | Not yet implemented |
| SSE4.1 decoder | `scaffold` | Not yet implemented |
| Alias method | `scaffold` | Not yet implemented |
| Normalization (stats) | `scaffold` | Not yet implemented |
