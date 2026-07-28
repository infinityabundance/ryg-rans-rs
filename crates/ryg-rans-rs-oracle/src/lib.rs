//! # ryg-rans-rs-oracle
//!
//! Cross-decoding oracle harness. Produces tracked receipts under `evidence/receipts/`
//! and canonical case manifests under `evidence/manifests/`.

use std::process::Command;

/// Whether the Rust side uses division or reciprocal paths.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CourtPath {
    Division,
    Reciprocal,
}

impl CourtPath {
    pub fn label(&self) -> &'static str {
        match self {
            CourtPath::Division => "DIVISION",
            CourtPath::Reciprocal => "RECIPROCAL",
        }
    }

    pub fn c_enc_op(&self, variant: &str) -> &'static str {
        match (variant, self) {
            ("byte", CourtPath::Division) => "enc-stream-byte-div",
            ("byte", CourtPath::Reciprocal) => "enc-stream-byte",
            ("r64", CourtPath::Division) => "enc-stream-r64-div",
            ("r64", CourtPath::Reciprocal) => "enc-stream-r64",
            _ => unreachable!(),
        }
    }

    pub fn c_dec_op(&self, variant: &str) -> &'static str {
        match (variant, self) {
            ("byte", CourtPath::Division) => "dec-stream-byte-div",
            ("byte", CourtPath::Reciprocal) => "dec-stream-byte",
            ("r64", CourtPath::Division) => "dec-stream-r64-div",
            ("r64", CourtPath::Reciprocal) => "dec-stream-r64",
            _ => unreachable!(),
        }
    }
}

/// Result of a single cross-decoding test case.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaseResult {
    pub case_id: String,
    pub input_hex: String,
    pub frequencies: Vec<u32>,
    pub scale_bits: u32,
    pub c_compressed_hex: String,
    pub rust_compressed_hex: String,
    pub compressed_match: bool,
    pub c_self_decode: bool,
    pub rust_self_decode: bool,
    pub c_to_rust: bool,
    pub rust_to_c: bool,
}

/// A canonical case manifest with SHA-256.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CaseManifest {
    pub schema_version: u32,
    pub court_id: String,
    pub court_path: String,
    pub variant: String,
    pub scale_bits: u32,
    pub seed: u64,
    pub cases: Vec<CaseResult>,
}

/// A sealed court receipt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub court_id: String,
    pub court_path: String,
    pub variant: String,
    pub scale_bits: u32,
    pub seed: u64,
    pub num_cases: u32,
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

/// Run a byte cross-decoding court.
pub fn run_byte_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
    path: CourtPath,
) -> Result<(Receipt, CaseManifest, Vec<u8>), String> {
    let variant = "byte";
    let court_id = format!(
        "RYG_RANS.BYTE.{}.SINGLE_STATE.UNIFORM256.S{}",
        path.label(),
        scale_bits
    );
    run_court(
        oracle_path,
        scale_bits,
        seed,
        num_cases,
        path,
        variant,
        &court_id,
    )
}

/// Run a 64-bit cross-decoding court.
pub fn run_r64_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
    path: CourtPath,
) -> Result<(Receipt, CaseManifest, Vec<u8>), String> {
    let variant = "r64";
    let court_id = format!(
        "RYG_RANS.R64.{}.SINGLE_STATE.UNIFORM256.S{}",
        path.label(),
        scale_bits
    );
    run_court(
        oracle_path,
        scale_bits,
        seed,
        num_cases,
        path,
        variant,
        &court_id,
    )
}

fn run_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
    path: CourtPath,
    variant: &str,
    court_id: &str,
) -> Result<(Receipt, CaseManifest), String> {
    let c_enc_op = path.c_enc_op(variant);
    let c_dec_op = path.c_dec_op(variant);

    let mut cases = Vec::with_capacity(num_cases);
    let mut residuals = Vec::new();

    for case_idx in 0..num_cases {
        let case_seed = seed.wrapping_add(case_idx as u64);
        let result = run_single_case(
            oracle_path,
            scale_bits,
            case_seed,
            case_idx,
            path,
            variant,
            c_enc_op,
            c_dec_op,
        )?;

        let all_ok = result.c_self_decode
            && result.rust_self_decode
            && result.compressed_match
            && result.c_to_rust
            && result.rust_to_c;

        if !all_ok {
            residuals.push(format!("{}.{:06}", court_id, case_idx));
        }

        cases.push(result);
    }

    // Build manifest struct first, then serialize to bytes
    let manifest = CaseManifest {
        schema_version: 1,
        court_id: court_id.to_string(),
        court_path: path.label().to_string(),
        variant: variant.to_string(),
        scale_bits,
        seed,
        cases,
    };
    // Serialize exactly once — hash and write use same bytes
    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| format!("manifest serialize: {}", e))?;
    let manifest_sha256 = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(&manifest_bytes);
        format!("{:x}", h.finalize())
    };

    // Calculate receipt hash
    let pairs_compared = cases.len() as u64 * 5;
    let pairs_matched: u64 = cases
        .iter()
        .map(|r| {
            [
                r.c_self_decode,
                r.rust_self_decode,
                r.compressed_match,
                r.c_to_rust,
                r.rust_to_c,
            ]
            .iter()
            .filter(|&&x| x)
            .count() as u64
        })
        .sum();
    let residual_count = residuals.len() as u32;

    let verdict = if residual_count == 0 {
        "admitted_match"
    } else {
        "admitted_partial"
    };

    let code_commit = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let receipt_json = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": 1,
        "court_id": court_id,
        "court_path": path.label(),
        "variant": variant,
        "scale_bits": scale_bits,
        "seed": seed,
        "num_cases": num_cases,
        "verdict": verdict,
        "upstream_commit": "c9d162d996fd600315af9ae8eb89d832576cb32d",
        "code_commit": code_commit,
        "pairs_compared": pairs_compared,
        "pairs_matched": pairs_matched,
        "residual_count": residual_count,
        "residual_ids": residuals,
        "manifest_sha256": manifest_sha256,
        "receipt_sha256": "",
        "reproduction_command": format!(
            "cargo run -p ryg-rans-rs-oracle -- {} {} {} {}",
            oracle_path, scale_bits, seed, num_cases
        ),
        "oracle_compiler": "g++ -O3",
    }))
    .map_err(|e| format!("receipt serialize: {}", e))?;

    let receipt_sha256 = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(receipt_json.as_bytes());
        format!("{:x}", h.finalize())
    };

    // Final receipt with SHA-256 self-hash
    let receipt: Receipt = serde_json::from_str(
        &serde_json::to_string(&serde_json::json!({
            "schema_version": 1,
            "court_id": court_id,
            "court_path": path.label(),
            "variant": variant,
            "scale_bits": scale_bits,
            "seed": seed,
            "num_cases": num_cases,
            "verdict": verdict,
            "upstream_commit": "c9d162d996fd600315af9ae8eb89d832576cb32d",
            "code_commit": code_commit,
            "pairs_compared": pairs_compared,
            "pairs_matched": pairs_matched,
            "residual_count": residual_count,
            "residual_ids": residuals,
            "manifest_sha256": manifest_sha256,
            "receipt_sha256": receipt_sha256,
            "reproduction_command": format!(
                "cargo run -p ryg-rans-rs-oracle -- {} {} {} {}",
                oracle_path, scale_bits, seed, num_cases
            ),
            "oracle_compiler": "g++ -O3",
        }))
        .map_err(|e| format!("receipt final: {}", e))?,
    )
    .map_err(|e| format!("receipt deserialize: {}", e))?;

    Ok((receipt, manifest, manifest_bytes))
}

fn run_single_case(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    case_idx: usize,
    path: CourtPath,
    variant: &str,
    c_enc_op: &str,
    c_dec_op: &str,
) -> Result<CaseResult, String> {
    let input = generate_input(seed, case_idx);

    // Build frequency model: uniform 256, total = 1 << scale_bits
    let total: u64 = 1u64 << scale_bits;
    let mut freqs = vec![0u32; 256];
    let base = if variant == "byte" {
        (total / 256) as u32
    } else {
        (total / 256) as u32
    };
    for f in freqs.iter_mut() {
        *f = base;
    }
    let rem = if variant == "byte" {
        (total as u32) - (base * 256)
    } else {
        (total as u32) - (base * 256)
    };
    if rem > 0 {
        freqs[255] += rem;
    }

    let freq_csv = freqs
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let input_hex = hex::encode(&input);

    // Cumulative frequencies
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

    // C encode
    let c_enc_out = Command::new(oracle_path)
        .args([c_enc_op, &scale_bits.to_string(), &freq_csv, &input_hex])
        .output()
        .map_err(|e| format!("C enc: {}", e))?;
    if !c_enc_out.status.success() {
        return Err(format!("C enc exit {}", c_enc_out.status));
    }
    let c_enc_json: serde_json::Value =
        serde_json::from_slice(&c_enc_out.stdout).map_err(|e| format!("C enc JSON: {}", e))?;
    let c_compressed_hex = c_enc_json["compressed_hex"]
        .as_str()
        .ok_or("C enc missing compressed_hex")?
        .to_string();
    let c_compressed = hex::decode(&c_compressed_hex).map_err(|e| format!("C hex: {}", e))?;

    // C self-decode (fail-closed: must be present)
    let c_self_decode = c_enc_json
        .get("decode_ok")
        .and_then(|v| v.as_bool())
        .ok_or("C enc missing decode_ok")?;

    // Rust encode using the correct path
    let rust_compressed = if variant == "byte" {
        rust_byte_encode(&input, &freqs, &cum_freqs, scale_bits, path)?
    } else {
        rust_r64_encode(&input, &freqs, &cum_freqs, scale_bits, path)?
    };
    let rust_compressed_hex = hex::encode(&rust_compressed);

    // Rust self-decode
    let cum2sym: Vec<u8> = (0..(total as usize))
        .map(|i| {
            for s in 0..256 {
                if i >= cum_freqs[s] as usize && i < cum_freqs[s + 1] as usize {
                    return s as u8;
                }
            }
            0
        })
        .collect();

    let rust_self_decode = if variant == "byte" {
        rust_byte_decode(
            &rust_compressed,
            &cum2sym,
            &freqs,
            &cum_freqs,
            scale_bits,
            input.len(),
            path,
        )
        .map(|d| d == input)
        .unwrap_or(false)
    } else {
        rust_r64_decode(
            &rust_compressed,
            &cum2sym,
            &freqs,
            &cum_freqs,
            scale_bits,
            input.len(),
            path,
        )
        .map(|d| d == input)
        .unwrap_or(false)
    };

    // C→Rust cross-decode
    let c_to_rust = if variant == "byte" {
        rust_byte_decode(
            &c_compressed,
            &cum2sym,
            &freqs,
            &cum_freqs,
            scale_bits,
            input.len(),
            path,
        )
        .map(|d| d == input)
        .unwrap_or(false)
    } else {
        rust_r64_decode(
            &c_compressed,
            &cum2sym,
            &freqs,
            &cum_freqs,
            scale_bits,
            input.len(),
            path,
        )
        .map(|d| d == input)
        .unwrap_or(false)
    };

    // Rust→C cross-decode
    let dec_output = Command::new(oracle_path)
        .args([
            c_dec_op,
            &scale_bits.to_string(),
            &freq_csv,
            &rust_compressed_hex,
            &input.len().to_string(),
        ])
        .output()
        .map_err(|e| format!("C dec: {}", e))?;
    let rust_to_c = if dec_output.status.success() {
        let dec_json: serde_json::Value =
            serde_json::from_slice(&dec_output.stdout).map_err(|e| format!("C dec JSON: {}", e))?;
        let decoded_hex = dec_json["decoded_hex"]
            .as_str()
            .ok_or("C dec missing decoded_hex")?;
        hex::decode(decoded_hex).unwrap_or_default() == input
    } else {
        false
    };

    let compressed_match = rust_compressed_hex == c_compressed_hex;
    let case_id = format!("CASE.{:06}", case_idx);

    Ok(CaseResult {
        case_id,
        input_hex,
        frequencies: freqs,
        scale_bits,
        c_compressed_hex,
        rust_compressed_hex,
        compressed_match,
        c_self_decode,
        rust_self_decode,
        c_to_rust,
        rust_to_c,
    })
}

// ---- Rust byte encode (correct path dispatch) ----

fn rust_byte_encode(
    input: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; input.len() * 4 + 16 + 4];
    let mut writer = ryg_rans_rs_core::BackwardByteWriter::new(&mut out);
    let mut state = ryg_rans_rs_core::RansByteState::new();

    for idx in (0..input.len()).rev() {
        let s = input[idx] as usize;
        match path {
            CourtPath::Division => {
                ryg_rans_rs_core::rans_byte_enc_put(
                    &mut state,
                    &mut writer,
                    cum_freqs[s],
                    freqs[s],
                    scale_bits,
                )
                .map_err(|_| "byte div enc")?;
            }
            CourtPath::Reciprocal => {
                let esym =
                    ryg_rans_rs_core::RansByteEncSymbol::new(cum_freqs[s], freqs[s], scale_bits)
                        .map_err(|_| "byte recip sym")?;
                ryg_rans_rs_core::rans_byte_enc_put_symbol(&mut state, &mut writer, &esym)
                    .map_err(|_| "byte recip enc")?;
            }
        }
    }
    ryg_rans_rs_core::rans_byte_enc_flush(&state, &mut writer).map_err(|_| "byte flush")?;

    Ok(writer.encoded().to_vec())
}

// ---- Rust byte decode (correct path dispatch) ----

fn rust_byte_decode(
    compressed: &[u8],
    cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    num: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    let mut reader = ryg_rans_rs_core::ByteReader::new(compressed);
    let mut state =
        ryg_rans_rs_core::rans_byte_dec_init(&mut reader).map_err(|_| "byte dec init")?;
    let mut output = vec![0u8; num];

    for i in 0..num {
        let cf = ryg_rans_rs_core::rans_byte_dec_get(&state, scale_bits);
        let s = cum2sym.get(cf as usize).copied().unwrap_or(0) as usize;
        output[i] = s as u8;
        match path {
            CourtPath::Division => {
                ryg_rans_rs_core::rans_byte_dec_advance(
                    &mut state,
                    &mut reader,
                    cum_freqs[s],
                    freqs[s],
                    scale_bits,
                )
                .map_err(|_| "byte div dec")?;
            }
            CourtPath::Reciprocal => {
                let dsym = ryg_rans_rs_core::RansByteDecSymbol::new(cum_freqs[s], freqs[s])
                    .map_err(|_| "byte dec sym")?;
                ryg_rans_rs_core::rans_byte_dec_advance_symbol(
                    &mut state,
                    &mut reader,
                    &dsym,
                    scale_bits,
                )
                .map_err(|_| "byte recip dec")?;
            }
        }
    }
    Ok(output)
}

// ---- Rust 64-bit encode (correct path dispatch) ----

fn rust_r64_encode(
    input: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    let mut out = vec![0u8; (input.len() * 2 + 4) * 4];
    let mut writer = ryg_rans_rs_core::BackwardWord32Writer::new(&mut out);
    let mut state = ryg_rans_rs_core::Rans64State::new();

    for idx in (0..input.len()).rev() {
        let s = input[idx] as usize;
        match path {
            CourtPath::Division => {
                ryg_rans_rs_core::rans64_enc_put(
                    &mut state,
                    &mut writer,
                    cum_freqs[s],
                    freqs[s],
                    scale_bits,
                )
                .map_err(|_| "r64 div enc")?;
            }
            CourtPath::Reciprocal => {
                let esym =
                    ryg_rans_rs_core::Rans64EncSymbol::new(cum_freqs[s], freqs[s], scale_bits)
                        .map_err(|_| "r64 recip sym")?;
                ryg_rans_rs_core::rans64_enc_put_symbol(&mut state, &mut writer, &esym)
                    .map_err(|_| "r64 recip enc")?;
            }
        }
    }
    ryg_rans_rs_core::rans64_enc_flush(&state, &mut writer).map_err(|_| "r64 flush")?;

    Ok(writer.encoded().to_vec())
}

// ---- Rust 64-bit decode (correct path dispatch) ----

fn rust_r64_decode(
    compressed: &[u8],
    cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    num: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    let mut reader = ryg_rans_rs_core::Word32Reader::new(compressed);
    let mut state = ryg_rans_rs_core::rans64_dec_init(&mut reader).map_err(|_| "r64 dec init")?;
    let mut output = vec![0u8; num];

    for i in 0..num {
        let cf = ryg_rans_rs_core::rans64_dec_get(&state, scale_bits);
        let s = cum2sym.get(cf as usize).copied().unwrap_or(0) as usize;
        output[i] = s as u8;
        match path {
            CourtPath::Division => {
                ryg_rans_rs_core::rans64_dec_advance(
                    &mut state,
                    &mut reader,
                    cum_freqs[s],
                    freqs[s],
                    scale_bits,
                )
                .map_err(|_| "r64 div dec")?;
            }
            CourtPath::Reciprocal => {
                let dsym = ryg_rans_rs_core::Rans64DecSymbol::new(cum_freqs[s], freqs[s])
                    .map_err(|_| "r64 dec sym")?;
                ryg_rans_rs_core::rans64_dec_advance_symbol(
                    &mut state,
                    &mut reader,
                    &dsym,
                    scale_bits,
                )
                .map_err(|_| "r64 recip dec")?;
            }
        }
    }
    Ok(output)
}

fn generate_input(seed: u64, case_idx: usize) -> Vec<u8> {
    let s = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let len = 1 + ((s >> 32) as usize) % 64;
    let mut rng_state = s.wrapping_add(case_idx as u64);
    let mut data = Vec::with_capacity(len);
    for _ in 0..len {
        rng_state = rng_state.wrapping_mul(6364136223846793005).wrapping_add(1);
        data.push(((rng_state >> 32) & 0xff) as u8);
    }
    data
}
