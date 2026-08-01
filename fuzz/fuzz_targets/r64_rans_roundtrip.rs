//! Fuzz target: 64-bit rANS round-trip (division and reciprocal paths).
//!
//! Takes arbitrary bytes, encodes them with a uniform model,
//! and verifies the decoded output matches the input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_core::*;

fuzz_target!(|data: &[u8]| {
    if data.len() > 65536 || data.len() < 2 {
        return;
    }

    let scale_bits = 10 + (data[0] as u32 % 20).max(1).min(30); // 11..=30
    let total = 1u32 << scale_bits;
    let used_syms = (data[1] as usize % 256).max(1);
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
    let sum: u32 = freqs[..used_syms].iter().sum();
    if sum < total {
        freqs[used_syms - 1] += total - sum;
    }
    cum[0] = 0;
    for i in 0..256 {
        cum[i + 1] = cum[i] + freqs[i];
    }

    let dsyms: Vec<Rans64DecSymbol> = (0..used_syms)
        .map(|i| Rans64DecSymbol::new(cum[i], freqs[i]).unwrap())
        .collect();

    let esyms: Vec<Rans64EncSymbol> = (0..used_syms)
        .map(|i| Rans64EncSymbol::new(cum[i], freqs[i], scale_bits).unwrap())
        .collect();

    // Cumulative-frequency -> symbol lookup via binary search.  (An earlier
    // version materialised a Vec of size 2^scale_bits — up to 1 GiB per
    // iteration at scale 30, which stalled the fuzzer.)
    fn cum2sym(cum: &[u32], cf: u32) -> u8 {
        // cum is ascending; find the largest j with cum[j] <= cf.
        let mut lo = 0usize;
        let mut hi = cum.len() - 1;
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if cum[mid] <= cf {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        lo as u8
    }

    let symbols: Vec<u8> = data[2..].iter().map(|&b| b % used_syms as u8).collect();

    // Division-based encode
    let mut out = vec![0u8; symbols.len() * 8 + 256];
    let mut writer = BackwardWord32Writer::new(&mut out);
    let mut state = Rans64State::new();

    for &s in symbols.iter().rev() {
        let start = cum[s as usize];
        let freq = freqs[s as usize];
        if let Err(e) = rans64_enc_put(&mut state, &mut writer, start, freq, scale_bits) {
            if e == EncodeError::OutputTooSmall {
                return;
            }
        }
    }
    if let Err(_) = rans64_enc_flush(&state, &mut writer) {
        return;
    }
    let encoded = writer.encoded();
    if encoded.len() < 8 {
        return;
    }

    // Division-based decode
    let mut reader = Word32Reader::new(encoded);
    let mut dec_state = if let Ok(s) = rans64_dec_init(&mut reader) {
        s
    } else {
        return;
    };

    let mut output = vec![0u8; symbols.len()];
    for i in 0..symbols.len() {
        let cf = rans64_dec_get(&dec_state, scale_bits);
        let s = cum2sym(&cum, cf) as usize;
        output[i] = s as u8;
        if let Some(dsym) = dsyms.get(s) {
            if let Err(_) = rans64_dec_advance_symbol(&mut dec_state, &mut reader, dsym, scale_bits)
            {
                return;
            }
        } else {
            return;
        }
    }

    assert_eq!(
        output,
        symbols,
        "r64 roundtrip mismatch: len={}",
        symbols.len()
    );

    // Reciprocal encode verification
    let mut out2 = vec![0u8; symbols.len() * 8 + 256];
    let mut writer2 = BackwardWord32Writer::new(&mut out2);
    let mut state2 = Rans64State::new();

    for &s in symbols.iter().rev() {
        if let Some(esym) = esyms.get(s as usize) {
            if let Err(e) = rans64_enc_put_symbol(&mut state2, &mut writer2, esym) {
                if e == EncodeError::OutputTooSmall {
                    return;
                }
            }
        } else {
            return;
        }
    }
    if let Err(_) = rans64_enc_flush(&state2, &mut writer2) {
        return;
    }
    let encoded2 = writer2.encoded();

    // Division and reciprocal streams must match
    assert_eq!(encoded, encoded2, "r64 division and reciprocal must match");
});
