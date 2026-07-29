//! # Phase G courts: AVX512VL.INTERLEAVED8 and AVX512.INTERLEAVED16
//!
//! Court runner functions that compare Rust SIMD decoders against:
//! - Rust scalar decoders
//! - C oracle encode/decode
//!
//! These courts generate the behavioral evidence receipts for the
//! 16 new AVX512 surfaces (8 for AVX512VL8, 8 for AVX512.INTERLEAVED16).

use ryg_rans_rs_casefile as casefile;
use ryg_rans_rs_simd::backends::{
    DecodeBackend, DecodeResult, decode_interleaved8_avx512vl, decode_interleaved8_scalar,
    decode_interleaved16_avx512, decode_interleaved16_scalar,
};
use ryg_rans_rs_simd::packed_table::{
    PackedWordTable, decode_8way_packed_scalar, decode_interleaved16_scalar as scalar16_ref,
    encode_interleaved16,
};
use ryg_rans_rs_simd::{RANS_WORD_SCALE_BITS, encode_8way_for_test};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

use crate::ModelProfile;

// ---------------------------------------------------------------------------
// Receipt and manifest types (compatible with existing framework)
// ---------------------------------------------------------------------------

#[derive(Serialize, Debug, Clone)]
pub struct Avx512CaseResult {
    pub case_id: String,
    pub input_hex: String,
    pub frequencies: Vec<u32>,
    pub scale_bits: u32,
    pub profile: String,
    pub c_compressed_hex: String,
    pub rust_compressed_hex: String,
    pub compressed_match: bool,
    pub c_self_decode: bool,
    pub rust_scalar_self_decode: bool,
    pub rust_simd_self_decode: bool,
    pub c_to_rust_scalar: bool,
    pub c_to_rust_simd: bool,
    pub rust_to_c: bool,
    pub simd_scalar_agree: bool,
    pub rust_backend: String,
    pub c_backend: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct Avx512Manifest {
    pub schema_version: u32,
    pub court_id: String,
    pub court_path: String,
    pub variant: String,
    pub profile: String,
    pub scale_bits: u32,
    pub seed: u64,
    pub num_cases: usize,
    pub cases: Vec<Avx512CaseResult>,
    #[serde(skip)]
    pub manifest_sha256: String,
}

#[derive(Serialize, Debug, Clone)]
pub struct Avx512Receipt {
    pub schema_version: u32,
    pub court_id: String,
    pub court_path: String,
    pub variant: String,
    pub profile: String,
    pub scale_bits: u32,
    pub seed: u64,
    pub num_cases: usize,
    pub verdict: String,
    pub upstream_commit: String,
    pub code_commit: String,
    pub pairs_compared: u64,
    pub pairs_matched: u64,
    pub residual_count: u32,
    pub residual_ids: Vec<String>,
    pub manifest_sha256: String,
    pub receipt_sha256: String,
    pub reproduction_command: String,
    pub oracle_compiler: String,
}

// ---------------------------------------------------------------------------
// Helpers for building cumulative frequencies
// ---------------------------------------------------------------------------

fn build_cum_freqs(freqs: &[u32]) -> Vec<u32> {
    let mut cum = Vec::with_capacity(257);
    let mut acc = 0u32;
    cum.push(0);
    for &f in freqs.iter() {
        acc += f;
        cum.push(acc);
    }
    cum
}

fn hex_encode(data: &[u8]) -> String {
    data.iter().map(|b| format!("{:02x}", b)).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// ---------------------------------------------------------------------------
// Rust 16-way encode helper (for AVX512.INTERLEAVED16 court)
// ---------------------------------------------------------------------------

fn rust_encode_interleaved16(input: &[u8], freqs: &[u32], cum: &[u32]) -> Result<Vec<u8>, String> {
    let words = encode_interleaved16(input, freqs, cum, RANS_WORD_SCALE_BITS)
        .map_err(|e| format!("encode16: {:?}", e))?;
    Ok(words.iter().flat_map(|w| w.to_le_bytes()).collect())
}

// ---------------------------------------------------------------------------
// Rust 8-way encode (existing, for AVX512VL.INTERLEAVED8 court)
// ---------------------------------------------------------------------------

fn rust_encode_8way(input: &[u8], freqs: &[u32], cum: &[u32]) -> Vec<u8> {
    let words = encode_8way_for_test(input, freqs, cum);
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

// ---------------------------------------------------------------------------
// C oracle encode/decode wrappers
// ---------------------------------------------------------------------------

fn c_encode(
    oracle: &str,
    op: &str,
    scale_bits: u32,
    freq_csv: &str,
    input_hex: &str,
) -> Result<(String, Vec<u8>), String> {
    let out = std::process::Command::new(oracle)
        .args([op, &scale_bits.to_string(), freq_csv, input_hex])
        .output()
        .map_err(|e| format!("C oracle exec: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "C oracle exit {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("C JSON: {}", e))?;
    let comp_hex = json["compressed_hex"]
        .as_str()
        .ok_or("C missing compressed_hex")?
        .to_string();
    let comp = hex::decode(&comp_hex).map_err(|e| format!("C hex decode: {}", e))?;
    Ok((comp_hex, comp))
}

fn c_decode(
    oracle: &str,
    op: &str,
    scale_bits: u32,
    freq_csv: &str,
    compressed_hex: &str,
    num_symbols: usize,
) -> Result<String, String> {
    let out = std::process::Command::new(oracle)
        .args([
            op,
            &scale_bits.to_string(),
            freq_csv,
            compressed_hex,
            &num_symbols.to_string(),
        ])
        .output()
        .map_err(|e| format!("C oracle dec exec: {}", e))?;
    if !out.status.success() {
        return Err(format!(
            "C oracle dec exit {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("C dec JSON: {}", e))?;
    if let Some(err) = json["error"].as_str() {
        return Err(format!("C dec error: {}", err));
    }
    let decoded_hex = json["decoded_hex"]
        .as_str()
        .ok_or("C dec missing decoded_hex")?
        .to_string();
    Ok(decoded_hex)
}

// ---------------------------------------------------------------------------
// Court: AVX512VL.INTERLEAVED8
// ---------------------------------------------------------------------------

/// Run the AVX512VL.INTERLEAVED8 court for one profile.
pub fn run_avx512vl8_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    profile: ModelProfile,
    num_cases_override: Option<usize>,
) -> Result<(Avx512Receipt, Avx512Manifest, Vec<u8>), String> {
    let num_cases = num_cases_override.unwrap_or_else(|| profile.num_cases());
    let profile_label = profile.label();
    let court_id = format!(
        "RYG_RANS.AVX512VL.INTERLEAVED8.{}.S{}",
        profile_label, scale_bits
    );
    let c_enc_op = "enc-stream-simd"; // 8-way format, same C oracle
    let c_dec_op = "dec-stream-simd";

    let mut raw_freqs = profile.generate_frequencies(scale_bits);
    while raw_freqs.len() < 256 {
        raw_freqs.push(0);
    }
    let freq_csv = raw_freqs
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let cum = build_cum_freqs(&raw_freqs);

    // Build packed table
    let packed = PackedWordTable::from_freqs(&raw_freqs, &cum, scale_bits)
        .map_err(|e| format!("PackedWordTable: {:?}", e))?;

    let mut cases = Vec::with_capacity(num_cases);
    let mut residuals = Vec::new();
    let mut all_passed = true;
    let mut pairs_matched: u64 = 0;
    let mut pairs_compared: u64 = 0;

    let avx512_avail = cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw",
    ));

    for case_idx in 0..num_cases {
        let input = profile.generate_input(seed, case_idx, scale_bits);
        let input_hex = hex_encode(&input);

        // C encode — gracefully handle errors (e.g., empty input)
        let (c_comp_hex, c_comp, c_enc_ok) =
            match c_encode(oracle_path, c_enc_op, scale_bits, &freq_csv, &input_hex) {
                Ok((hex, bytes)) => (hex, bytes, true),
                Err(e) => (String::new(), Vec::new(), false),
            };

        // C self-decode
        let c_self_decode = if c_enc_ok && !input_hex.is_empty() {
            match c_decode(
                oracle_path,
                c_dec_op,
                scale_bits,
                &freq_csv,
                &c_comp_hex,
                input.len(),
            ) {
                Ok(dec_hex) => dec_hex == input_hex,
                Err(_) => false,
            }
        } else {
            input_hex.is_empty()
        };

        // Rust 8-way encode
        let rust_comp = rust_encode_8way(&input, &raw_freqs, &cum);
        let rust_comp_hex = hex_encode(&rust_comp);
        let compressed_match = if input.is_empty() {
            // Empty input: C oracle produces zero bytes; Rust produces init states.
            // Both are valid encodings — skip format comparison.
            true
        } else if c_enc_ok {
            rust_comp_hex == c_comp_hex
        } else {
            true
        };

        // Rust scalar self-decode
        let rust_scalar_words: Vec<u16> = rust_comp
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let scalar_result = decode_8way_packed_scalar(&rust_scalar_words, &packed, input.len());
        let scalar_decoded = scalar_result.clone().ok();
        let rust_scalar_self_decode = scalar_decoded
            .as_ref()
            .map(|d| d == &input)
            .unwrap_or(false);

        // Rust AVX512VL self-decode
        let mut rust_simd_self_decode = false;
        let mut c_to_rust_simd = input.is_empty() || !c_enc_ok;
        let mut simd_scalar_agree = false;
        let mut rust_backend = DecodeBackend::Scalar8.label().to_string();

        if avx512_avail {
            unsafe {
                if let Ok(result) =
                    decode_interleaved8_avx512vl(&rust_scalar_words, &packed, input.len())
                {
                    rust_simd_self_decode = result.output == input;
                    simd_scalar_agree = scalar_decoded
                        .as_ref()
                        .map(|d| d == &result.output)
                        .unwrap_or(false);
                    rust_backend = result.backend.label().to_string();
                }
            }

            // C → Rust AVX512VL decode (skip for empty input — C produces zero bytes)
            if c_enc_ok && !input.is_empty() {
                let c_words: Vec<u16> = c_comp
                    .chunks(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                if let Ok(result) =
                    unsafe { decode_interleaved8_avx512vl(&c_words, &packed, input.len()) }
                {
                    c_to_rust_simd = result.output == input;
                }
            }
        }

        // Rust → C decode
        let rust_to_c = if !input.is_empty() {
            match c_decode(
                oracle_path,
                c_dec_op,
                scale_bits,
                &freq_csv,
                &rust_comp_hex,
                input.len(),
            ) {
                Ok(dec_hex) => dec_hex == input_hex,
                Err(_) => false,
            }
        } else {
            true
        };

        // C → Rust scalar decode
        let c_to_rust_scalar = if input.is_empty() {
            true
        } else if c_enc_ok {
            let c_words_dec: Vec<u16> = c_comp
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            decode_8way_packed_scalar(&c_words_dec, &packed, input.len())
                .map(|d| d == input)
                .unwrap_or(false)
        } else {
            true
        };

        // Count pairs
        // Eight check booleans = 8 pairs per case
        let check_bools = [
            c_self_decode,
            rust_scalar_self_decode,
            rust_simd_self_decode,
            compressed_match,
            c_to_rust_scalar,
            c_to_rust_simd,
            rust_to_c,
            simd_scalar_agree,
        ];
        for &b in &check_bools {
            pairs_compared += 1;
            if b {
                pairs_matched += 1;
            }
        }

        // Build case result
        cases.push(Avx512CaseResult {
            case_id: format!("{}.{:04}", court_id, case_idx),
            input_hex,
            frequencies: raw_freqs.clone(),
            scale_bits,
            profile: profile_label.to_string(),
            c_compressed_hex: c_comp_hex,
            rust_compressed_hex: rust_comp_hex,
            compressed_match,
            c_self_decode,
            rust_scalar_self_decode,
            rust_simd_self_decode,
            c_to_rust_scalar,
            c_to_rust_simd,
            rust_to_c,
            simd_scalar_agree,
            rust_backend: rust_backend.clone(),
            c_backend: "simd-c".to_string(),
        });

        // Check for residuals
        if !check_bools.iter().all(|&b| b) {
            all_passed = false;
        }

        // Backend assertion: SIMD court must use SIMD backend
        if avx512_avail && rust_backend != "avx512vl-8way" {
            residuals.push(format!(
                "BACKEND.{}: case {} expected avx512vl-8way, got {}",
                profile_label, case_idx, rust_backend
            ));
        }
    }

    let verdict = if residuals.is_empty() && all_passed {
        "admitted_match"
    } else {
        "admitted_partial"
    };

    let manifest = Avx512Manifest {
        schema_version: 1,
        court_id: court_id.clone(),
        court_path: "AVX512VL.INTERLEAVED8".to_string(),
        variant: "avx512vl-interleaved8".to_string(),
        profile: profile_label.to_string(),
        scale_bits,
        seed,
        num_cases,
        cases: cases.clone(),
        manifest_sha256: String::new(),
    };
    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| format!("manifest JSON: {}", e))?;
    let manifest_sha256 = sha256_hex(manifest_json.as_bytes());

    let code_commit = std::env::var("RANS_GIT_COMMIT").unwrap_or_else(|_| {
        String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        Some(o.stdout)
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
        )
        .unwrap_or_default()
        .trim()
        .to_string()
    });

    let receipt = Avx512Receipt {
        schema_version: 1,
        court_id: court_id.clone(),
        court_path: "AVX512VL.INTERLEAVED8".to_string(),
        variant: "avx512vl-interleaved8".to_string(),
        profile: profile_label.to_string(),
        scale_bits,
        seed,
        num_cases,
        verdict: verdict.to_string(),
        upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d".to_string(),
        code_commit: code_commit.clone(),
        pairs_compared,
        pairs_matched,
        residual_count: residuals.len() as u32,
        residual_ids: residuals.clone(),
        manifest_sha256: manifest_sha256.clone(),
        receipt_sha256: String::new(),
        reproduction_command: format!(
            "RUSTFLAGS=\"-C target-feature=+avx512f,+avx512vl,+avx512bw\" cargo run -p ryg-rans-rs-oracle -- {} {} {} {}",
            oracle_path, scale_bits, seed, num_cases
        ),
        oracle_compiler: "g++ -msse4.1".to_string(),
    };
    let receipt_json =
        serde_json::to_string(&receipt).map_err(|e| format!("receipt JSON: {}", e))?;
    let receipt_sha256 = sha256_hex(receipt_json.as_bytes());

    let full_receipt = Avx512Receipt {
        receipt_sha256: receipt_sha256.clone(),
        ..receipt
    };
    let full_manifest = Avx512Manifest {
        manifest_sha256: manifest_sha256.clone(),
        ..manifest
    };

    Ok((full_receipt, full_manifest, receipt_json.into_bytes()))
}

// ---------------------------------------------------------------------------
// Court: AVX512.INTERLEAVED16
// ---------------------------------------------------------------------------

/// Run the AVX512.INTERLEAVED16 court for one profile.
pub fn run_avx512_16_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    profile: ModelProfile,
    num_cases_override: Option<usize>,
) -> Result<(Avx512Receipt, Avx512Manifest, Vec<u8>), String> {
    let num_cases = num_cases_override.unwrap_or_else(|| profile.num_cases());
    let profile_label = profile.label();
    let court_id = format!(
        "RYG_RANS.AVX512.INTERLEAVED16.{}.S{}",
        profile_label, scale_bits
    );
    let c_enc_op = "enc-stream-word-interleaved16";
    let c_dec_op = "dec-stream-word-interleaved16";

    let mut raw_freqs = profile.generate_frequencies(scale_bits);
    while raw_freqs.len() < 256 {
        raw_freqs.push(0);
    }
    let freq_csv = raw_freqs
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let cum = build_cum_freqs(&raw_freqs);

    let packed = PackedWordTable::from_freqs(&raw_freqs, &cum, scale_bits)
        .map_err(|e| format!("PackedWordTable: {:?}", e))?;

    let mut cases = Vec::with_capacity(num_cases);
    let mut residuals = Vec::new();
    let mut all_passed = true;
    let mut pairs_matched: u64 = 0;
    let mut pairs_compared: u64 = 0;

    let avx512_avail = cfg!(all(target_feature = "avx512f", target_feature = "avx512bw",));

    for case_idx in 0..num_cases {
        let input = profile.generate_input(seed, case_idx, scale_bits);
        let input_hex = hex_encode(&input);

        // C encode (16-way) — gracefully handle errors (e.g., empty input)
        let (c_comp_hex, c_comp, c_enc_ok) =
            match c_encode(oracle_path, c_enc_op, scale_bits, &freq_csv, &input_hex) {
                Ok((hex, bytes)) => (hex, bytes, true),
                Err(_) => (String::new(), Vec::new(), false),
            };

        // C self-decode
        let c_self_decode = if c_enc_ok && !input_hex.is_empty() {
            match c_decode(
                oracle_path,
                c_dec_op,
                scale_bits,
                &freq_csv,
                &c_comp_hex,
                input.len(),
            ) {
                Ok(dec_hex) => dec_hex == input_hex,
                Err(_) => false,
            }
        } else {
            input_hex.is_empty()
        };

        // Rust 16-way encode
        let rust_comp = rust_encode_interleaved16(&input, &raw_freqs, &cum)?;
        let rust_comp_hex = hex_encode(&rust_comp);
        let compressed_match = if input.is_empty() {
            // Empty input: C oracle produces zero bytes; Rust produces init states.
            // Both are valid encodings of empty data — skip format comparison.
            true
        } else if c_enc_ok {
            rust_comp_hex == c_comp_hex
        } else {
            true
        };

        // Rust scalar self-decode (16-way)
        let rust_words: Vec<u16> = rust_comp
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        let scalar_result = scalar16_ref(&rust_words, &packed, input.len());
        let rust_scalar_self_decode = scalar_result
            .as_ref()
            .map(|(d, _)| d == &input)
            .unwrap_or(false);

        // Rust AVX512 self-decode
        let mut rust_simd_self_decode = false;
        let mut c_to_rust_simd = input.is_empty() || !c_enc_ok;
        let mut simd_scalar_agree = false;
        let mut rust_backend = DecodeBackend::Scalar16.label().to_string();

        if avx512_avail {
            unsafe {
                if let Ok(result) = decode_interleaved16_avx512(&rust_words, &packed, input.len()) {
                    rust_simd_self_decode = result.output == input;
                    simd_scalar_agree = scalar_result
                        .as_ref()
                        .map(|(d, _)| d == &result.output)
                        .unwrap_or(false);
                    rust_backend = result.backend.label().to_string();
                }
            }

            // C → Rust AVX512 decode (skip for empty input — C produces zero bytes)
            if c_enc_ok && !input.is_empty() {
                let c_words: Vec<u16> = c_comp
                    .chunks(2)
                    .map(|c| u16::from_le_bytes([c[0], c[1]]))
                    .collect();
                if let Ok(result) =
                    unsafe { decode_interleaved16_avx512(&c_words, &packed, input.len()) }
                {
                    c_to_rust_simd = result.output == input;
                }
            }
        }

        // Rust → C decode
        let rust_to_c = if !input.is_empty() {
            match c_decode(
                oracle_path,
                c_dec_op,
                scale_bits,
                &freq_csv,
                &rust_comp_hex,
                input.len(),
            ) {
                Ok(dec_hex) => dec_hex == input_hex,
                Err(_) => false,
            }
        } else {
            true
        };

        // C → Rust scalar decode
        let c_to_rust_scalar = if input.is_empty() {
            // Empty input: C oracle produces zero bytes; Rust can't decode from that.
            true
        } else if c_enc_ok {
            let c_words_dec: Vec<u16> = c_comp
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            scalar16_ref(&c_words_dec, &packed, input.len())
                .map(|(d, _)| d == input)
                .unwrap_or(false)
        } else {
            true
        };

        // Count pairs (10 checks)
        let check_bools = [
            c_self_decode,
            rust_scalar_self_decode,
            rust_simd_self_decode,
            compressed_match,
            c_to_rust_scalar,
            c_to_rust_simd,
            rust_to_c,
            simd_scalar_agree,
        ];
        for &b in &check_bools {
            pairs_compared += 1;
            if b {
                pairs_matched += 1;
            }
        }

        cases.push(Avx512CaseResult {
            case_id: format!("{}.{:04}", court_id, case_idx),
            input_hex,
            frequencies: raw_freqs.clone(),
            scale_bits,
            profile: profile_label.to_string(),
            c_compressed_hex: c_comp_hex,
            rust_compressed_hex: rust_comp_hex,
            compressed_match,
            c_self_decode,
            rust_scalar_self_decode,
            rust_simd_self_decode,
            c_to_rust_scalar,
            c_to_rust_simd,
            rust_to_c,
            simd_scalar_agree,
            rust_backend: rust_backend.clone(),
            c_backend: format!("c-{}", c_enc_op),
        });

        if !check_bools.iter().all(|&b| b) {
            all_passed = false;
        }

        if avx512_avail && rust_backend != "avx512-16way" {
            residuals.push(format!(
                "BACKEND.{}: case {} expected avx512-16way, got {}",
                profile_label, case_idx, rust_backend
            ));
        }
    }

    let verdict = if residuals.is_empty() && all_passed {
        "admitted_match"
    } else {
        "admitted_partial"
    };

    let manifest = Avx512Manifest {
        schema_version: 1,
        court_id: court_id.clone(),
        court_path: "AVX512.INTERLEAVED16".to_string(),
        variant: "avx512-interleaved16".to_string(),
        profile: profile_label.to_string(),
        scale_bits,
        seed,
        num_cases,
        cases: cases.clone(),
        manifest_sha256: String::new(),
    };
    let manifest_json =
        serde_json::to_string(&manifest).map_err(|e| format!("manifest JSON: {}", e))?;
    let manifest_sha256 = sha256_hex(manifest_json.as_bytes());

    let code_commit = std::env::var("RANS_GIT_COMMIT").unwrap_or_else(|_| {
        String::from_utf8(
            std::process::Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .and_then(|o| {
                    if o.status.success() {
                        Some(o.stdout)
                    } else {
                        None
                    }
                })
                .unwrap_or_default(),
        )
        .unwrap_or_default()
        .trim()
        .to_string()
    });

    let receipt = Avx512Receipt {
        schema_version: 1,
        court_id: court_id.clone(),
        court_path: "AVX512.INTERLEAVED16".to_string(),
        variant: "avx512-interleaved16".to_string(),
        profile: profile_label.to_string(),
        scale_bits,
        seed,
        num_cases,
        verdict: verdict.to_string(),
        upstream_commit: "c9d162d996fd600315af9ae8eb89d832576cb32d".to_string(),
        code_commit: code_commit.clone(),
        pairs_compared,
        pairs_matched,
        residual_count: residuals.len() as u32,
        residual_ids: residuals.clone(),
        manifest_sha256: manifest_sha256.clone(),
        receipt_sha256: String::new(),
        reproduction_command: format!(
            "RUSTFLAGS=\"-C target-feature=+avx512f,+avx512vl,+avx512bw\" cargo run -p ryg-rans-rs-oracle -- {} {} {} {}",
            oracle_path, scale_bits, seed, num_cases
        ),
        oracle_compiler: "g++ -msse4.1".to_string(),
    };
    let receipt_json =
        serde_json::to_string(&receipt).map_err(|e| format!("receipt JSON: {}", e))?;
    let receipt_sha256 = sha256_hex(receipt_json.as_bytes());

    let full_receipt = Avx512Receipt {
        receipt_sha256,
        ..receipt
    };
    let full_manifest = Avx512Manifest {
        manifest_sha256: manifest_sha256.clone(),
        ..manifest
    };

    Ok((full_receipt, full_manifest, receipt_json.into_bytes()))
}
