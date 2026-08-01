//! Fuzz target: Word rANS round-trip (single-state, interleaved2).
//!
//! Exercises the word-aligned rANS encoder and decoder with
//! arbitrary input data and frequency models.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_core::*;

/// Convert `&[u16]` to a `Vec<u8>` for use with `Word16Reader`.
fn u16_slice_to_bytes(slice: &[u16]) -> Vec<u8> {
    slice.iter().flat_map(|&w| w.to_le_bytes()).collect()
}

fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 || data.len() < 4 {
        return;
    }

    let scale_bits = RANS_WORD_SCALE_BITS; // must be 12 per upstream
    let total = 1u32 << scale_bits;
    // A single-symbol model (used_syms = 1) gives freq = total = 2^12,
    // which overflows the u32 renorm threshold `(L >> 12) << 16 * freq`
    // (= 2^32) — exactly what the fuzzer hit.  Real encoders never reach
    // that state: single-symbol blocks are RLE.  Require >= 2 symbols.
    let used_syms = ((data[0] as usize % 255).max(2)).min(256);
    let uniform_freq = total / used_syms as u32;
    if uniform_freq == 0 {
        return;
    }

    // Build frequency model
    let mut freqs = vec![0u32; 256];
    let mut cum = vec![0u32; 257];
    for i in 0..used_syms {
        freqs[i] = uniform_freq;
        cum[i + 1] = cum[i] + freqs[i];
    }
    let sum: u32 = freqs[..used_syms].iter().sum();
    if sum < total {
        freqs[used_syms - 1] += total - sum;
    }
    cum[0] = 0;
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    // Build word rANS tables using the core crate's own types (the core
    // decoder's `rans_word_dec_sym` takes `ryg_rans_rs_core::RansWordTables`;
    // the SIMD crate's structurally identical type is distinct).
    let mut slots = vec![RansWordSlot { freq: 0, bias: 0 }; 4096];
    let mut slot2sym = vec![0u8; 4096];
    for s in 0..256usize {
        let f = freqs[s] as usize;
        let start = cum[s] as usize;
        for i in 0..f {
            let slot = start + i;
            if slot < slots.len() {
                slots[slot] = RansWordSlot {
                    freq: f as u16,
                    bias: i as u16,
                };
                slot2sym[slot] = s as u8;
            }
        }
    }
    let tables = RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };

    // Input symbols
    let symbols: Vec<u8> = data[1..].iter().map(|&b| b % used_syms as u8).collect();

    if symbols.is_empty() {
        return;
    }

    // ---- Word rANS single-state encode ----
    let mut buf = vec![0u16; symbols.len() * 4 + 128];
    let mut writer = buf.len();
    let mut state = RansWordState::new();

    for &s in symbols.iter().rev() {
        let freq = freqs[s as usize];
        let start = cum[s as usize];
        // Renorm check
        let threshold = ((RANS_WORD_L >> scale_bits) << 16) * freq;
        if state.0 >= threshold {
            if writer == 0 {
                return;
            }
            writer -= 1;
            buf[writer] = (state.0 & 0xffff) as u16;
            state.0 >>= 16;
        }
        // Encode
        state.0 = ((state.0 / freq) << scale_bits) + (state.0 % freq) + start;
    }
    // Flush: write 2 u16 words
    if writer < 2 {
        return;
    }
    writer -= 2;
    buf[writer] = (state.0 & 0xffff) as u16;
    buf[writer + 1] = ((state.0 >> 16) & 0xffff) as u16;

    let compressed = &buf[writer..];
    let comp_bytes = u16_slice_to_bytes(compressed);

    // ---- Word rANS decode ----
    let mut reader = Word16Reader::new(&comp_bytes);
    let mut dec_state = if let Ok(s) = rans_word_dec_init(&mut reader) {
        s
    } else {
        return;
    };

    let mut output = vec![0u8; symbols.len()];
    for out_sym in output.iter_mut() {
        let s = rans_word_dec_sym(&mut dec_state, &tables, scale_bits);
        *out_sym = s;
        if let Err(_) = rans_word_dec_renorm(&mut dec_state, &mut reader) {
            return;
        }
    }

    assert_eq!(
        output,
        symbols,
        "word rANS roundtrip mismatch: len={}",
        symbols.len()
    );
});
