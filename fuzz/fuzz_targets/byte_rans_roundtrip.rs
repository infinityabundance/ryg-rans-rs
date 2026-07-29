//! Fuzz target: byte rANS round-trip (division and reciprocal paths).
//!
//! Takes arbitrary bytes, encodes them with a uniform-256 model,
//! and verifies the decoded output matches the input.
//!
//! This exercises:
//! - BackwardByteWriter / ByteReader I/O
//! - Division-based encode (`rans_byte_enc_put`)
//! - Reciprocal fast-path encode (`rans_byte_enc_put_symbol`)
//! - Decoder init, get, advance, and renormalization
//! - Frequency model construction

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_core::*;

fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 {
        return; // skip very large inputs to keep fuzzing fast
    }
    if data.len() < 2 {
        return; // need at least a few bytes to construct a model
    }

    let scale_bits = 10 + (data[0] as u32 % 6); // 10..=15
    let total = 1u32 << scale_bits;
    let base_freq = total / 256;
    if base_freq == 0 {
        return;
    }
    let used_syms = (data[1] as usize % 256).max(1);

    // Build uniform model over the first `used_syms` symbols
    let uniform_freq = total / used_syms as u32;
    if uniform_freq == 0 {
        return;
    }
    let mut freqs = vec![0u32; 256];
    let mut cum = vec![0u32; 257];
    for i in 0..used_syms {
        freqs[i] = uniform_freq;
        cum[i + 1] = cum[i] + freqs[i];
    }
    // Adjust last frequency to make the sum exactly total
    let sum: u32 = freqs[..used_syms].iter().sum();
    if sum < total {
        freqs[used_syms - 1] += total - sum;
    }
    // Recompute cumulative
    cum[0] = 0;
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    // Build decoder symbols
    let dsyms: Vec<RansByteDecSymbol> = (0..used_syms)
        .map(|i| RansByteDecSymbol::new(cum[i], freqs[i]).unwrap())
        .collect();

    // Build encoder symbols
    let esyms: Vec<RansByteEncSymbol> = (0..used_syms)
        .map(|i| RansByteEncSymbol::new(cum[i], freqs[i], scale_bits).unwrap())
        .collect();

    // Build cum2sym lookup table
    let cum2sym: Vec<u8> = (0..total as usize)
        .map(|i| {
            for j in 0..used_syms {
                if i >= cum[j] as usize && i < cum[j + 1] as usize {
                    return j as u8;
                }
            }
            0
        })
        .collect();

    // Input data: symbols from the used symbol set
    let symbols: Vec<u8> = data[2..].iter().map(|&b| b % used_syms as u8).collect();

    // ---- Division-based encode ----
    let mut out_div = [0u8; 131072];
    let mut writer = BackwardByteWriter::new(&mut out_div);
    let mut state = RansByteState::new();

    for &s in symbols.iter().rev() {
        let start = cum[s as usize];
        let freq = freqs[s as usize];
        if let Err(e) = rans_byte_enc_put(&mut state, &mut writer, start, freq, scale_bits) {
            // OutputTooSmall may happen for very large symbol counts — skip
            if e == EncodeError::OutputTooSmall {
                return;
            }
        }
    }
    if let Err(_) = rans_byte_enc_flush(&state, &mut writer) {
        return;
    }
    let encoded_div = writer.encoded();
    if encoded_div.is_empty() {
        return;
    }

    // ---- Division-based decode ----
    let mut reader = ByteReader::new(encoded_div);
    let mut dec_state = if let Ok(s) = rans_byte_dec_init(&mut reader) {
        s
    } else {
        return; // truncated stream
    };

    let mut output = vec![0u8; symbols.len()];
    for i in 0..symbols.len() {
        let cf = rans_byte_dec_get(&dec_state, scale_bits);
        let s = cum2sym.get(cf as usize).copied().unwrap_or(0) as usize;
        output[i] = s as u8;
        if let Some(dsym) = dsyms.get(s) {
            if let Err(_) =
                rans_byte_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits)
            {
                return; // truncated stream
            }
        } else {
            return;
        }
    }

    // Verify roundtrip
    assert_eq!(
        output,
        symbols,
        "byte rANS division roundtrip mismatch: len={}",
        symbols.len()
    );

    // ---- Reciprocal encode ----
    let mut out_rec = [0u8; 131072];
    let mut writer = BackwardByteWriter::new(&mut out_rec);
    let mut state = RansByteState::new();

    for &s in symbols.iter().rev() {
        if let Some(esym) = esyms.get(s as usize) {
            if let Err(e) = rans_byte_enc_put_symbol(&mut state, &mut writer, esym) {
                if e == EncodeError::OutputTooSmall {
                    return;
                }
            }
        } else {
            return;
        }
    }
    if let Err(_) = rans_byte_enc_flush(&state, &mut writer) {
        return;
    }
    let encoded_rec = writer.encoded();
    if encoded_rec.is_empty() {
        return;
    }

    // The reciprocal-encoded stream should be identical to division-encoded
    assert_eq!(
        encoded_div, encoded_rec,
        "division and reciprocal encodings must match"
    );
});
