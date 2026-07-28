//! # ryg-rans-rs-oracle
//!
//! **Cross-decoding oracle harness.**
//!
//! Generates deterministic casefiles, encodes/decodes through both the
//! compiled C/C++ oracle and the Rust implementation, compares results
//! byte-for-byte, and writes sealed court receipts.
//!
//! ## Usage
//!
//! ```rust,no_run
//! use ryg_rans_rs_oracle::court::*;
//!
//! // Run the byte cross-decoding court
//! let receipt = run_byte_cross_court(
//!     "/path/to/oracle/adapter/rans_trace",
//!     12,     // scale_bits
//!     1024,   // seed
//! )?;
//!
//! println!("Verdict: {}", receipt.verdict);
//! ```

use std::process::Command;

/// Result of a single cross-decoding test case.
#[derive(Debug, Clone)]
pub struct CrossDecodeResult {
    pub case_id: String,
    pub input: Vec<u8>,
    pub frequencies: Vec<u32>,
    pub scale_bits: u32,
    pub c_compressed_hex: String,
    pub c_decode_ok: bool,
    pub rust_compressed_hex: String,
    pub rust_decode_ok: bool,
    pub compressed_match: bool,
    pub c_decode_rust: bool,
    pub rust_decode_c: bool,
}

/// A sealed court receipt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub court_id: String,
    pub case_count: u32,
    pub verdict: String,
    pub upstream_commit: String,
    pub pairs_compared: u64,
    pub pairs_matched: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<String>,
    pub reproduction_command: String,
    pub timestamp: String,
}

/// Run the full byte rANS cross-decoding court.
///
/// # Arguments
///
/// * `oracle_path` — Path to the compiled `rans_trace` C oracle adapter.
/// * `scale_bits` — Number of scale bits (e.g., 12).
/// * `seed` — Deterministic seed for case generation.
/// * `num_cases` — Number of test cases to generate.
pub fn run_byte_cross_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
) -> Result<Receipt, String> {
    let court_id = format!("RYG_RANS.BYTE.CROSS_DECODE.v1");
    let mut results = Vec::new();
    let mut residuals = Vec::new();

    for case_idx in 0..num_cases {
        let case_seed = seed.wrapping_add(case_idx as u64);
        let result = run_single_byte_case(oracle_path, scale_bits, case_seed, case_idx)?;

        if !result.compressed_match || !result.c_decode_rust || !result.rust_decode_c {
            residuals.push(format!("{}.{}", court_id, case_idx));
        }

        results.push(result);
    }

    let pairs_compared = results.len() as u64;
    let pairs_matched = results
        .iter()
        .filter(|r| r.compressed_match && r.c_decode_rust && r.rust_decode_c)
        .count() as u64;
    let residual_count = residuals.len() as u32;

    let verdict = if residual_count == 0 {
        "admitted_match".to_string()
    } else if pairs_matched > 0 {
        "admitted_partial".to_string()
    } else {
        "admitted_divergence".to_string()
    };

    let receipt = Receipt {
        schema_version: 1,
        court_id: court_id.clone(),
        case_count: results.len() as u32,
        verdict,
        upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d".to_string(),
        pairs_compared,
        pairs_matched,
        residual_count,
        residual_ids: residuals,
        reproduction_command: format!(
            "cargo run -p ryg-rans-rs-oracle -- {} {} {}",
            oracle_path, scale_bits, seed
        ),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    Ok(receipt)
}

/// Run the 64-bit rANS cross-decoding court.
pub fn run_r64_cross_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
) -> Result<Receipt, String> {
    let court_id = format!("RYG_RANS.R64.CROSS_DECODE.v1");
    let mut results = Vec::new();
    let mut residuals = Vec::new();

    for case_idx in 0..num_cases {
        let case_seed = seed.wrapping_add(case_idx as u64);
        let result = run_single_r64_case(oracle_path, scale_bits, case_seed, case_idx)?;

        if !result.compressed_match || !result.c_decode_rust || !result.rust_decode_c {
            residuals.push(format!("{}.{}", court_id, case_idx));
        }

        results.push(result);
    }

    let pairs_compared = results.len() as u64;
    let pairs_matched = results
        .iter()
        .filter(|r| r.compressed_match && r.c_decode_rust && r.rust_decode_c)
        .count() as u64;
    let residual_count = residuals.len() as u32;

    let verdict = if residual_count == 0 {
        "admitted_match".to_string()
    } else if pairs_matched > 0 {
        "admitted_partial".to_string()
    } else {
        "admitted_divergence".to_string()
    };

    let receipt = Receipt {
        schema_version: 1,
        court_id,
        case_count: results.len() as u32,
        verdict,
        upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d".to_string(),
        pairs_compared,
        pairs_matched,
        residual_count,
        residual_ids: residuals,
        reproduction_command: format!(
            "cargo run -p ryg-rans-rs-oracle -- {} {} {}",
            oracle_path, scale_bits, seed
        ),
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    Ok(receipt)
}

/// Run a single byte rANS test case through both C and Rust.
fn run_single_byte_case(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    case_idx: usize,
) -> Result<CrossDecodeResult, String> {
    let total = 1u32 << scale_bits;

    // Generate deterministic input
    let input = generate_input(seed, case_idx);

    // Build uniform frequency model (each of 256 symbols gets equal share)
    let mut freqs = vec![0u32; 256];
    let base_freq = total / 256;
    for f in freqs.iter_mut() {
        *f = base_freq;
    }
    // Assign remainder to last symbol
    let remainder = total - (base_freq * 256);
    if remainder > 0 {
        freqs[255] += remainder;
    }

    let freq_csv = freqs
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let input_hex = hex::encode(&input);

    // 1. C encode
    let c_output = Command::new(oracle_path)
        .args([
            "enc-stream-byte",
            &scale_bits.to_string(),
            &freq_csv,
            &input_hex,
        ])
        .output()
        .map_err(|e| format!("C oracle execution failed: {}", e))?;

    if !c_output.status.success() {
        return Err(format!("C oracle exited with {}", c_output.status));
    }

    let c_json: serde_json::Value = serde_json::from_slice(&c_output.stdout)
        .map_err(|e| format!("C oracle JSON parse failed: {}", e))?;

    let c_compressed_hex = c_json["compressed_hex"]
        .as_str()
        .ok_or("C oracle missing compressed_hex")?
        .to_string();
    let c_decode_ok = c_json["decode_ok"].as_bool().unwrap_or(false);

    // 2. Build frequency model matching C oracle
    let cum_freqs: Vec<u32> = {
        let mut cum = 0u32;
        let mut cums = Vec::with_capacity(257);
        cums.push(0);
        for &f in &freqs {
            cum += f;
            cums.push(cum);
        }
        cums
    };

    // Rust encode with correct cumulative starts
    let esyms: Vec<_> = (0..256)
        .map(|i| {
            ryg_rans_rs_core::RansByteEncSymbol::new(cum_freqs[i], freqs[i], scale_bits).unwrap()
        })
        .collect();

    let mut out = vec![0u8; input.len() * 4 + 16 + 4];
    let mut writer = ryg_rans_rs_core::BackwardByteWriter::new(&mut out);
    let mut state = ryg_rans_rs_core::RansByteState::new();

    for idx in (0..input.len()).rev() {
        let s = input[idx] as usize;
        ryg_rans_rs_core::rans_byte_enc_put_symbol(&mut state, &mut writer, &esyms[s])
            .map_err(|_| "Rust encode: output too small")?;
    }
    ryg_rans_rs_core::rans_byte_enc_flush(&state, &mut writer)
        .map_err(|_| "Rust encode flush failed")?;

    let rust_compressed = writer.encoded();
    let rust_compressed_hex = hex::encode(rust_compressed);

    // 3. Rust decode of C-compressed data (C→Rust cross-decode)
    let c_compressed =
        hex::decode(&c_compressed_hex).map_err(|e| format!("hex decode failed: {}", e))?;

    let dsyms: Vec<_> = (0..256)
        .map(|i| ryg_rans_rs_core::RansByteDecSymbol::new(cum_freqs[i], freqs[i]).unwrap())
        .collect();

    let cum2sym: Vec<u8> = (0..total as usize)
        .map(|i| {
            for s in 0..256 {
                if i >= cum_freqs[s] as usize && i < cum_freqs[s + 1] as usize {
                    return s as u8;
                }
            }
            0
        })
        .collect();

    let c_decode_rust_result =
        decode_bytes(&c_compressed, &cum2sym, &dsyms, scale_bits, input.len());
    let c_decode_rust = c_decode_rust_result.map(|d| d == input).unwrap_or(false);

    // 4. C decode of Rust-compressed data (Rust→C cross-decode)
    let rust_compress_hex_for_c = hex::encode(rust_compressed);
    let dec_output = Command::new(oracle_path)
        .args([
            "dec-stream-byte",
            &scale_bits.to_string(),
            &freq_csv,
            &rust_compress_hex_for_c,
            &input.len().to_string(),
        ])
        .output()
        .map_err(|e| format!("C decoder execution failed: {}", e))?;

    let rust_decode_c = if dec_output.status.success() {
        let dec_json: serde_json::Value = serde_json::from_slice(&dec_output.stdout)
            .map_err(|e| format!("C decoder JSON parse failed: {}", e))?;
        let decoded_hex = dec_json["decoded_hex"].as_str().unwrap_or("");
        let decoded = hex::decode(decoded_hex).unwrap_or_default();
        decoded == input
    } else {
        false
    };

    // 5. Rust decode of Rust data (self-consistency check)
    let rust_decode_ok = decode_bytes(rust_compressed, &cum2sym, &dsyms, scale_bits, input.len())
        .map(|d| d == input)
        .unwrap_or(false);

    // Compare compressed streams
    let compressed_match = rust_compressed_hex == c_compressed_hex;

    let case_id = format!("RYG_RANS.BYTE.CROSS_DECODE.{:06}", case_idx);

    Ok(CrossDecodeResult {
        case_id,
        input,
        frequencies: freqs,
        scale_bits,
        c_compressed_hex,
        c_decode_ok,
        rust_compressed_hex,
        rust_decode_ok,
        compressed_match,
        c_decode_rust,
        rust_decode_c,
    })
}

/// Run a single 64-bit rANS test case through both C and Rust.
fn run_single_r64_case(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    case_idx: usize,
) -> Result<CrossDecodeResult, String> {
    let total: u64 = 1u64 << scale_bits;

    let input = generate_input(seed, case_idx);

    let mut freqs = vec![0u32; 256];
    let base_freq = (total / 256) as u32;
    for f in freqs.iter_mut() {
        *f = base_freq;
    }
    let remainder = (total - (base_freq as u64 * 256)) as u32;
    if remainder > 0 {
        freqs[255] += remainder;
    }

    let freq_csv = freqs
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let input_hex = hex::encode(&input);

    // 1. C encode
    let c_output = Command::new(oracle_path)
        .args([
            "enc-stream-r64",
            &scale_bits.to_string(),
            &freq_csv,
            &input_hex,
        ])
        .output()
        .map_err(|e| format!("C r64 oracle failed: {}", e))?;

    if !c_output.status.success() {
        return Err(format!("C r64 oracle exited with {}", c_output.status));
    }

    let c_json: serde_json::Value = serde_json::from_slice(&c_output.stdout)
        .map_err(|e| format!("C r64 JSON parse failed: {}", e))?;

    let c_compressed_hex = c_json["compressed_hex"]
        .as_str()
        .ok_or("C r64 missing compressed_hex")?
        .to_string();
    let c_decode_ok = c_json["decode_ok"].as_bool().unwrap_or(false);

    // 2. Compute cumulative frequencies matching C oracle
    let cum_freqs: Vec<u32> = {
        let mut cum = 0u32;
        let mut cums = Vec::with_capacity(257);
        cums.push(0);
        for &f in &freqs {
            cum += f;
            cums.push(cum);
        }
        cums
    };

    // Rust encode with correct cumulative starts
    let esyms: Vec<_> = (0..256)
        .map(|i| {
            ryg_rans_rs_core::Rans64EncSymbol::new(cum_freqs[i], freqs[i], scale_bits).unwrap()
        })
        .collect();

    let mut out_words = vec![0u8; (input.len() * 2 + 4) * 4];
    let mut writer = ryg_rans_rs_core::BackwardWord32Writer::new(&mut out_words);
    let mut state = ryg_rans_rs_core::Rans64State::new();

    for idx in (0..input.len()).rev() {
        let s = input[idx] as usize;
        ryg_rans_rs_core::rans64_enc_put_symbol(&mut state, &mut writer, &esyms[s])
            .map_err(|_| "Rust r64 encode: output too small")?;
    }
    ryg_rans_rs_core::rans64_enc_flush(&state, &mut writer).map_err(|_| "Rust r64 flush failed")?;

    let rust_compressed = writer.encoded();
    let rust_compressed_hex = hex::encode(&rust_compressed);

    // 3. Rust decode of C data
    let c_compressed = hex::decode(&c_compressed_hex).map_err(|e| format!("hex decode: {}", e))?;

    let dsyms: Vec<_> = (0..256)
        .map(|i| ryg_rans_rs_core::Rans64DecSymbol::new(cum_freqs[i], freqs[i]).unwrap())
        .collect();

    let cum2sym: Vec<u8> = (0..total as usize)
        .map(|i| {
            for s in 0..256 {
                if i >= cum_freqs[s] as usize && i < cum_freqs[s + 1] as usize {
                    return s as u8;
                }
            }
            0
        })
        .collect();

    let c_decode_rust = r64_decode_bytes(&c_compressed, &cum2sym, &dsyms, scale_bits, input.len())
        .map(|d| d == input)
        .unwrap_or(false);

    // 4. C decode of Rust data
    let rust_hex_for_c = hex::encode(&rust_compressed);
    let dec_output = Command::new(oracle_path)
        .args([
            "dec-stream-r64",
            &scale_bits.to_string(),
            &freq_csv,
            &rust_hex_for_c,
            &input.len().to_string(),
        ])
        .output()
        .map_err(|e| format!("C r64 decoder failed: {}", e))?;

    let rust_decode_c = if dec_output.status.success() {
        let dec_json: serde_json::Value = serde_json::from_slice(&dec_output.stdout)
            .map_err(|e| format!("C r64 decoder JSON parse: {}", e))?;
        let decoded_hex = dec_json["decoded_hex"].as_str().unwrap_or("");
        let decoded = hex::decode(decoded_hex).unwrap_or_default();
        decoded == input
    } else {
        false
    };

    let rust_decode_ok =
        r64_decode_bytes(&rust_compressed, &cum2sym, &dsyms, scale_bits, input.len())
            .map(|d| d == input)
            .unwrap_or(false);

    let compressed_match = rust_compressed_hex == c_compressed_hex;
    let case_id = format!("RYG_RANS.R64.CROSS_DECODE.{:06}", case_idx);

    Ok(CrossDecodeResult {
        case_id,
        input,
        frequencies: freqs,
        scale_bits,
        c_compressed_hex,
        c_decode_ok,
        rust_compressed_hex,
        rust_decode_ok,
        compressed_match,
        c_decode_rust,
        rust_decode_c,
    })
}

/// Generate deterministic input data for a test case.
fn generate_input(seed: u64, case_idx: usize) -> Vec<u8> {
    // Seeded deterministic generation
    let s = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let len = 1 + ((s >> 32) as usize) % 64; // 1..64 bytes

    let mut rng_state = s.wrapping_add(case_idx as u64);
    let mut data = Vec::with_capacity(len);
    for _ in 0..len {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        data.push(((rng_state >> 32) & 0xff) as u8);
    }
    data
}

/// Decode bytes using the Rust byte rANS decoder.
fn decode_bytes(
    compressed: &[u8],
    cum2sym: &[u8],
    dsyms: &[ryg_rans_rs_core::RansByteDecSymbol],
    scale_bits: u32,
    num_symbols: usize,
) -> Result<Vec<u8>, String> {
    let mut reader = ryg_rans_rs_core::ByteReader::new(compressed);
    let mut state =
        ryg_rans_rs_core::rans_byte_dec_init(&mut reader).map_err(|_| "dec init failed")?;

    let mut output = vec![0u8; num_symbols];
    for i in 0..num_symbols {
        let cf = ryg_rans_rs_core::rans_byte_dec_get(&state, scale_bits);
        let s = cum2sym.get(cf as usize).copied().unwrap_or(0) as usize;
        output[i] = s as u8;
        ryg_rans_rs_core::rans_byte_dec_advance_symbol(
            &mut state,
            &mut reader,
            &dsyms[s],
            scale_bits,
        )
        .map_err(|_| "dec advance failed")?;
    }
    Ok(output)
}

/// Decode bytes using the Rust 64-bit rANS decoder.
fn r64_decode_bytes(
    compressed: &[u8],
    cum2sym: &[u8],
    dsyms: &[ryg_rans_rs_core::Rans64DecSymbol],
    scale_bits: u32,
    num_symbols: usize,
) -> Result<Vec<u8>, String> {
    let mut reader = ryg_rans_rs_core::Word32Reader::new(compressed);
    let mut state =
        ryg_rans_rs_core::rans64_dec_init(&mut reader).map_err(|_| "r64 dec init failed")?;

    let mut output = vec![0u8; num_symbols];
    for i in 0..num_symbols {
        let cf = ryg_rans_rs_core::rans64_dec_get(&state, scale_bits);
        let s = cum2sym.get(cf as usize).copied().unwrap_or(0) as usize;
        output[i] = s as u8;
        ryg_rans_rs_core::rans64_dec_advance_symbol(&mut state, &mut reader, &dsyms[s], scale_bits)
            .map_err(|_| "r64 dec advance failed")?;
    }
    Ok(output)
}
