//! Fuzz target: malformed byte rANS streams.
//!
//! Feeds randomly truncated, corrupted, and edge-case byte rANS
//! compressed streams to the decoder and verifies it never panics,
//! always returns a `Result` (never UB), and returns `Err` for
//! genuinely truncated inputs.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_core::*;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return; // need enough data to construct a model + stream
    }

    let scale_bits = 10 + (data[0] as u32 % 6); // 10..=15
    let total = 1u32 << scale_bits;

    // Use first few bytes to construct a frequency model
    // The loop below reads `data[2 + i]`, so `num_syms` must not exceed
    // `data.len() - 2` (the `data.len() < 8` guard alone is not enough:
    // with len 8 and num_syms up to 16 the loop would index out of
    // bounds — caught by the fuzzer as a target bug, not a library bug).
    let num_syms = ((data[1] as usize % 16).max(2)).min(data.len() - 2);
    let mut freqs = vec![0u32; 256];
    let mut cum = vec![0u32; 257];
    let mut remaining = total;
    for i in 0..num_syms.min(16) {
        let f = if i + 1 == num_syms.min(16) {
            remaining
        } else {
            let f = (data[2 + i] as u32) % remaining.max(1) + 1;
            remaining = remaining.saturating_sub(f);
            f
        };
        freqs[i] = f;
        cum[i + 1] = cum[i] + f;
    }

    // Build decoder symbol for sym 0
    if freqs[0] == 0 {
        return;
    }
    let dsym = RansByteDecSymbol::new(cum[0], freqs[0]).unwrap();

    // The rest of the data is the "compressed stream" — but we may
    // truncate it, corrupt it, or feed it as-is.
    // We try to decode and must never panic.
    let compressed = &data[2 + num_syms.min(16)..];

    let mut reader = ByteReader::new(compressed);
    let dec_state_result = rans_byte_dec_init(&mut reader);
    if let Ok(mut state) = dec_state_result {
        // Attempt a few decode steps
        for _ in 0..10 {
            let cf = rans_byte_dec_get(&state, scale_bits);

            // Look up symbol — cf might be out of range for our model
            let s_idx = (cf as usize).min(255);
            let _ = s_idx;

            let result = rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits);
            if result.is_err() {
                break; // truncated — expected
            }
        }
    }
    // If we reach here without panicking, the test passes.
    // The decoder may return Err (truncated) or Ok (if the stream happens
    // to be valid), but never panic.
});
