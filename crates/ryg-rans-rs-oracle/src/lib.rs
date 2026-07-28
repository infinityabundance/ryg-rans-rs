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
            ("word", _) => "enc-stream-word",
            _ => unreachable!(),
        }
    }

    pub fn c_dec_op(&self, variant: &str) -> &'static str {
        match (variant, self) {
            ("byte", CourtPath::Division) => "dec-stream-byte-div",
            ("byte", CourtPath::Reciprocal) => "dec-stream-byte",
            ("r64", CourtPath::Division) => "dec-stream-r64-div",
            ("r64", CourtPath::Reciprocal) => "dec-stream-r64",
            ("word", _) => "dec-stream-word",
            _ => unreachable!(),
        }
    }

    /// C oracle operation for interleaved encoding (two-state)
    pub fn c_enc_interleaved_op(&self, variant: &str) -> &'static str {
        match (variant, self) {
            ("byte", CourtPath::Division) => "enc-stream-byte-interleaved2-div",
            ("byte", CourtPath::Reciprocal) => "enc-stream-byte-interleaved2",
            ("r64", CourtPath::Division) => "enc-stream-r64-interleaved2-div",
            ("r64", CourtPath::Reciprocal) => "enc-stream-r64-interleaved2",
            ("word", _) => "enc-stream-word-interleaved2",
            _ => unreachable!(),
        }
    }

    /// C oracle operation for interleaved decoding (two-state)
    pub fn c_dec_interleaved_op(&self, variant: &str) -> &'static str {
        match (variant, self) {
            ("byte", CourtPath::Division) => "dec-stream-byte-interleaved2-div",
            ("byte", CourtPath::Reciprocal) => "dec-stream-byte-interleaved2",
            ("r64", CourtPath::Division) => "dec-stream-r64-interleaved2-div",
            ("r64", CourtPath::Reciprocal) => "dec-stream-r64-interleaved2",
            ("word", _) => "dec-stream-word-interleaved2",
            _ => unreachable!(),
        }
    }
}

/// Model profile for generating frequency tables and input data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ModelProfile {
    /// 256 symbols with equal frequency (the baseline)
    Uniform256,
    /// Frequency-one path: 255 symbols at freq=1, last symbol absorbs remainder
    Freq1Residual,
    /// Extreme skew: one symbol gets 255/256 of total
    Skewed2551,
    /// Only 2 active symbols (sparse alphabet)
    Sparse2,
    /// 17 active symbols
    Sparse17,
    /// Prime-number frequencies producing uneven cumulative ranges
    PrimeResidue,
    /// Frequencies that force renormalization at state-threshold boundaries
    RenormBoundary,
    /// Sweep across scale_bits 10–16
    ScaleSweep,
    /// Input-length boundary cases: 0, 1, odd, even, 255, 256, 257, large
    LengthBoundary,
}

impl ModelProfile {
    pub fn label(&self) -> &'static str {
        match self {
            ModelProfile::Uniform256 => "UNIFORM256",
            ModelProfile::Freq1Residual => "FREQ1.RESIDUAL",
            ModelProfile::Skewed2551 => "SKEWED.255_1",
            ModelProfile::Sparse2 => "SPARSE.2",
            ModelProfile::Sparse17 => "SPARSE.17",
            ModelProfile::PrimeResidue => "PRIME.RESIDUE",
            ModelProfile::RenormBoundary => "RENORM.BOUNDARY",
            ModelProfile::ScaleSweep => "SCALE.SWEEP",
            ModelProfile::LengthBoundary => "LENGTH.BOUNDARY",
        }
    }

    /// Returns the scale_bits for this profile (None = use the passed-in default)
    pub fn scale_bits(&self) -> Option<u32> {
        match self {
            ModelProfile::ScaleSweep => None, // will iterate
            _ => Some(12),
        }
    }

    /// Generates frequency table for this profile.
    pub fn generate_frequencies(&self, scale_bits: u32) -> Vec<u32> {
        let total = 1u64 << scale_bits;
        match self {
            ModelProfile::Uniform256 => {
                let mut freqs = vec![0u32; 256];
                let base = (total / 256) as u32;
                for f in freqs.iter_mut() {
                    *f = base;
                }
                let rem = (total as u32) - (base * 256);
                if rem > 0 {
                    freqs[255] += rem;
                }
                freqs
            }
            ModelProfile::Freq1Residual => {
                // 255 symbols at frequency 1, last symbol absorbs remainder
                // Tests the frequency-one special path while keeping total correct
                let num_syms = 256usize;
                let mut freqs = vec![0u32; num_syms];
                for i in 0..255 {
                    freqs[i] = 1;
                }
                let sum_255 = 255u64;
                freqs[255] = if sum_255 < total {
                    (total - sum_255) as u32
                } else {
                    1
                };
                freqs
            }
            ModelProfile::Skewed2551 => {
                // One symbol gets almost everything
                let mut freqs = vec![0u32; 2];
                freqs[0] = ((total as u64 * 255) / 256) as u32;
                freqs[1] = (total as u32).saturating_sub(freqs[0]);
                freqs
            }
            ModelProfile::Sparse2 => {
                // Only 2 active symbols — the C oracle cannot receive freq=0
                let mut freqs = vec![0u32; 2];
                freqs[0] = (total / 2) as u32;
                freqs[1] = (total as u32) - freqs[0];
                freqs
            }
            ModelProfile::Sparse17 => {
                // Only 17 active symbols
                let mut freqs = vec![0u32; 17];
                let base = (total / 17) as u32;
                for f in freqs.iter_mut() {
                    *f = base;
                }
                let rem = (total as u32) - (base * 17);
                if rem > 0 {
                    freqs[16] += rem;
                }
                freqs
            }
            ModelProfile::PrimeResidue => {
                // Use prime-scaled frequencies with largest-remainder normalization
                // to distribute the residual across the full table.
                let primes = [2u32, 3, 5, 7, 11, 13, 17, 19];
                let target = total as u64;
                let base = target / 256;
                // Compute raw prime-scaled frequencies
                let mut raw = Vec::with_capacity(256);
                let mut raw_sum = 0u64;
                for i in 0..256 {
                    let p = primes[i % 8];
                    let f = (base / p as u64).max(1);
                    raw.push(f);
                    raw_sum += f;
                }
                // Normalize with largest-remainder method
                let mut freqs = vec![0u32; 256];
                let mut allocated = 0u64;
                for i in 0..256 {
                    let exact = (raw[i] as f64 / raw_sum as f64) * target as f64;
                    let floor = exact.floor() as u64;
                    freqs[i] = floor as u32;
                    allocated += floor;
                }
                // Distribute remainder (largest remainder first)
                let mut remainder = target - allocated;
                let mut remainders: Vec<(usize, f64)> = (0..256)
                    .map(|i| {
                        let exact = (raw[i] as f64 / raw_sum as f64) * target as f64;
                        (i, exact - exact.floor())
                    })
                    .collect();
                remainders
                    .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                for (idx, _) in remainders.iter() {
                    if remainder == 0 {
                        break;
                    }
                    freqs[*idx] += 1;
                    remainder -= 1;
                }
                freqs
            }
            ModelProfile::RenormBoundary => {
                // Frequencies designed to trigger renormalization at boundary states
                let mut freqs = vec![0u32; 256];
                // Use very large frequency for first symbol and tiny for others
                // to force frequent renormalization
                freqs[0] = (total / 2) as u32;
                let remaining = (total as u32) - freqs[0];
                let base = remaining / 255;
                for i in 1..256 {
                    freqs[i] = base;
                }
                let sum_others = base * 255;
                if sum_others < remaining {
                    freqs[255] += remaining - sum_others;
                }
                freqs
            }
            ModelProfile::ScaleSweep => {
                // Used for all scale_bits values; generate uniform frequencies
                let mut freqs = vec![0u32; 256];
                let base = (total / 256) as u32;
                for f in freqs.iter_mut() {
                    *f = base;
                }
                let rem = (total as u32) - (base * 256);
                if rem > 0 {
                    freqs[255] += rem;
                }
                freqs
            }
            ModelProfile::LengthBoundary => {
                // Standard uniform frequencies; input length varies
                let mut freqs = vec![0u32; 256];
                let base = (total / 256) as u32;
                for f in freqs.iter_mut() {
                    *f = base;
                }
                let rem = (total as u32) - (base * 256);
                if rem > 0 {
                    freqs[255] += rem;
                }
                freqs
            }
        }
    }

    /// Returns the number of cases to generate for this profile.
    pub fn num_cases(&self) -> usize {
        match self {
            ModelProfile::Uniform256 => 20,
            ModelProfile::Freq1Residual => 20,
            ModelProfile::Skewed2551 => 20,
            ModelProfile::Sparse2 => 20,
            ModelProfile::Sparse17 => 20,
            ModelProfile::PrimeResidue => 20,
            ModelProfile::RenormBoundary => 20,
            ModelProfile::ScaleSweep => 5,      // per scale_bits value
            ModelProfile::LengthBoundary => 12, // one per boundary length
        }
    }

    /// Generate input of appropriate length for this profile.
    /// Input bytes are constrained to the number of symbols in the frequency table.
    pub fn generate_input(&self, seed: u64, case_idx: usize, scale_bits: u32) -> Vec<u8> {
        let freqs = self.generate_frequencies(scale_bits);
        let num_symbols = freqs.len();
        match self {
            ModelProfile::LengthBoundary => {
                let lengths = [0, 1, 63, 64, 65, 127, 128, 129, 255, 256, 257, 1023];
                let len = lengths[case_idx.min(lengths.len() - 1)];
                let mut rng = SimpleRng::new(seed.wrapping_add(case_idx as u64));
                (0..len)
                    .map(|_| (rng.next() as usize % num_symbols) as u8)
                    .collect()
            }
            _ => {
                let len = 64usize;
                // Each case gets a unique seed for diverse inputs
                let mut rng = SimpleRng::new(seed.wrapping_add(case_idx as u64));
                (0..len)
                    .map(|_| (rng.next() as usize % num_symbols) as u8)
                    .collect()
            }
        }
    }
}

/// Simple deterministic RNG for repeatable test generation.
struct SimpleRng(u64);

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_add(0x9e3779b97f4a7c15))
    }
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
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
    pub profile: String,
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
    pub profile: String,
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

/// Court configuration tuple.
pub struct CourtConfig {
    pub variant: &'static str,
    pub path: CourtPath,
    pub profile: ModelProfile,
}

/// Generate all court configurations for the full model profile suite.
pub fn all_court_configs() -> Vec<CourtConfig> {
    let variants = ["byte", "r64"];
    let paths = [CourtPath::Division, CourtPath::Reciprocal];
    let profiles = [
        ModelProfile::Uniform256,
        ModelProfile::Freq1Residual,
        ModelProfile::Skewed2551,
        ModelProfile::Sparse2,
        ModelProfile::Sparse17,
        ModelProfile::PrimeResidue,
        ModelProfile::RenormBoundary,
        ModelProfile::LengthBoundary,
    ];
    let mut configs = Vec::new();
    // ScaleSweep is special: we run it at multiple scale_bits values
    for &variant in &variants {
        for &path in &paths {
            configs.push(CourtConfig {
                variant,
                path,
                profile: ModelProfile::ScaleSweep,
            });
        }
    }
    for &variant in &variants {
        for &path in &paths {
            for &profile in &profiles {
                configs.push(CourtConfig {
                    variant,
                    path,
                    profile,
                });
            }
        }
    }
    configs
}

/// Run a cross-decoding court with a specific model profile.
/// Run an interleaved (two-state) cross-decoding court.
pub fn run_interleaved_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    path: CourtPath,
    variant: &str,
    profile: ModelProfile,
) -> Result<(Receipt, CaseManifest, Vec<u8>), String> {
    let num_cases = profile.num_cases();
    let profile_label = profile.label();
    let court_id = format!(
        "RYG_RANS.{}.{}.INTERLEAVED2.{}.S{}",
        variant.to_uppercase(),
        path.label(),
        profile_label,
        scale_bits
    );

    let c_enc_op = path.c_enc_interleaved_op(variant);
    let c_dec_op = path.c_dec_interleaved_op(variant);

    let mut freqs = profile.generate_frequencies(scale_bits);
    let total: u64 = 1u64 << scale_bits;
    let num_symbols = freqs.len();

    // Pad frequencies to 256 for C oracle
    while freqs.len() < 256 {
        freqs.push(0);
    }

    let cum_freqs: Vec<u32> = {
        let mut cum = 0u32;
        let mut cums = Vec::with_capacity(257);
        cums.push(0);
        for &f in freqs.iter() {
            cum += f;
            cums.push(cum);
        }
        cums
    };
    let freq_csv = freqs
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let mut cases = Vec::with_capacity(num_cases);
    let mut residuals = Vec::new();

    for case_idx in 0..num_cases {
        let input = profile.generate_input(seed, case_idx, scale_bits);
        let input_hex = hex::encode(&input);

        // Helper: call C interleaved decode and return whether output matches input
        let call_c_dec = |compressed_hex: &str, input_len: usize| -> Result<bool, String> {
            let dec_out = Command::new(oracle_path)
                .args([
                    c_dec_op,
                    &scale_bits.to_string(),
                    &freq_csv,
                    compressed_hex,
                    &input_len.to_string(),
                ])
                .output()
                .map_err(|e| format!("C interleaved dec: {}", e))?;
            if !dec_out.status.success() {
                return Err(format!(
                    "C interleaved dec exit {} for case {}",
                    dec_out.status, case_idx
                ));
            }
            let dec_json: serde_json::Value = serde_json::from_slice(&dec_out.stdout)
                .map_err(|e| format!("C interleaved dec JSON: {}", e))?;
            let decoded_hex = dec_json["decoded_hex"]
                .as_str()
                .ok_or("C interleaved dec missing decoded_hex")?;
            Ok(decoded_hex == input_hex)
        };

        // C interleaved encode
        let c_enc_out = Command::new(oracle_path)
            .args([c_enc_op, &scale_bits.to_string(), &freq_csv, &input_hex])
            .output()
            .map_err(|e| format!("C interleaved enc: {}", e))?;
        if !c_enc_out.status.success() {
            return Err(format!(
                "C interleaved enc exit {} for case {}",
                c_enc_out.status, case_idx
            ));
        }
        let c_enc_json: serde_json::Value = serde_json::from_slice(&c_enc_out.stdout)
            .map_err(|e| format!("C interleaved enc JSON: {}", e))?;
        let c_compressed_hex = c_enc_json["compressed_hex"]
            .as_str()
            .ok_or("C interleaved enc missing compressed_hex")?
            .to_string();
        let c_compressed = hex::decode(&c_compressed_hex).map_err(|e| format!("C hex: {}", e))?;

        // C self-decode: C decodes its own compressed stream
        let c_self_decode = call_c_dec(&c_compressed_hex, input.len())?;

        // Rust interleaved encode
        let rust_compressed = match variant {
            "byte" => rust_byte_interleaved_encode(
                &input,
                &freqs,
                &cum_freqs,
                scale_bits,
                num_symbols,
                path,
            )?,
            "r64" => rust_r64_interleaved_encode(
                &input,
                &freqs,
                &cum_freqs,
                scale_bits,
                num_symbols,
                path,
            )?,
            "word" => rust_word_interleaved_encode(
                &input,
                &freqs,
                &cum_freqs,
                scale_bits,
                num_symbols,
                path,
            )?,
            _ => return Err(format!("unknown variant: {}", variant)),
        };
        let rust_compressed_hex = hex::encode(&rust_compressed);

        // Rust self-decode
        let cum2sym: Vec<u8> = (0..(total as usize))
            .map(|i| {
                for s in 0..cum_freqs.len() - 1 {
                    if i >= cum_freqs[s] as usize && i < cum_freqs[s + 1] as usize {
                        return s as u8;
                    }
                }
                0
            })
            .collect();

        let rust_self_decode = match variant {
            "byte" => rust_byte_interleaved_decode(
                &rust_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "r64" => rust_r64_interleaved_decode(
                &rust_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "word" => rust_word_interleaved_decode(
                &rust_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            _ => false,
        };

        // C→Rust cross-decode: Rust decodes C's stream
        let c_to_rust = match variant {
            "byte" => rust_byte_interleaved_decode(
                &c_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "r64" => rust_r64_interleaved_decode(
                &c_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "word" => rust_word_interleaved_decode(
                &c_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            _ => false,
        };

        // Rust→C cross-decode: C decodes Rust's stream
        let rust_to_c = call_c_dec(&rust_compressed_hex, input.len())?;

        let compressed_match = rust_compressed_hex == c_compressed_hex;
        let case_id = format!("CASE.{:06}", case_idx);

        let result = CaseResult {
            case_id,
            input_hex,
            frequencies: freqs.clone(),
            scale_bits,
            c_compressed_hex,
            rust_compressed_hex,
            compressed_match,
            c_self_decode,
            rust_self_decode,
            c_to_rust,
            rust_to_c,
        };

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

    let manifest = CaseManifest {
        schema_version: 1,
        court_id: court_id.clone(),
        court_path: path.label().to_string(),
        variant: format!("{}_interleaved2", variant),
        profile: profile_label.to_string(),
        scale_bits,
        seed,
        cases: cases.clone(),
    };

    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| format!("manifest serialize: {}", e))?;
    let manifest_sha256 = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(&manifest_bytes);
        format!("{:x}", h.finalize())
    };

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

    let code_commit = std::env::var("RANS_GIT_COMMIT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        });

    let receipt_json = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": 1,
        "court_id": &court_id,
        "court_path": path.label(),
        "variant": format!("{}_interleaved2", variant),
        "profile": profile_label,
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

    let receipt: Receipt = serde_json::from_str(
        &serde_json::to_string(&serde_json::json!({
            "schema_version": 1,
            "court_id": &court_id,
            "court_path": path.label(),
            "variant": format!("{}_interleaved2", variant),
            "profile": profile_label,
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

/// Run a cross-decoding court with a specific model profile (single-state).
pub fn run_court_with_profile(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    path: CourtPath,
    variant: &str,
    profile: ModelProfile,
) -> Result<(Receipt, CaseManifest, Vec<u8>), String> {
    let num_cases = profile.num_cases();
    let profile_label = profile.label();
    let court_id = format!(
        "RYG_RANS.{}.{}.SINGLE_STATE.{}.S{}",
        variant.to_uppercase(),
        path.label(),
        profile_label,
        scale_bits
    );

    let c_enc_op = path.c_enc_op(variant);
    let c_dec_op = path.c_dec_op(variant);

    let mut freqs = profile.generate_frequencies(scale_bits);
    let total: u64 = 1u64 << scale_bits;
    let num_symbols = freqs.len();

    // Pad frequencies to exactly 256 for the C oracle (which expects 256 entries)
    while freqs.len() < 256 {
        freqs.push(0);
    }

    // Cumulative frequencies (use the actual active symbols, not the padded zeros)
    let cum_freqs: Vec<u32> = {
        let mut cum = 0u32;
        let mut cums = Vec::with_capacity(257);
        cums.push(0);
        for &f in freqs.iter() {
            cum += f;
            cums.push(cum);
        }
        cums
    };
    let freq_csv = freqs
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join(",");

    let mut cases = Vec::with_capacity(num_cases);
    let mut residuals = Vec::new();

    for case_idx in 0..num_cases {
        let input = profile.generate_input(seed, case_idx, scale_bits);
        let input_hex = hex::encode(&input);
        let case_seed = seed.wrapping_add(case_idx as u64);

        // C encode
        let c_enc_out = Command::new(oracle_path)
            .args([c_enc_op, &scale_bits.to_string(), &freq_csv, &input_hex])
            .output()
            .map_err(|e| format!("C enc: {}", e))?;
        if !c_enc_out.status.success() {
            return Err(format!(
                "C enc exit {} for case {}",
                c_enc_out.status, case_idx
            ));
        }
        let c_enc_json: serde_json::Value =
            serde_json::from_slice(&c_enc_out.stdout).map_err(|e| format!("C enc JSON: {}", e))?;
        let c_compressed_hex = c_enc_json["compressed_hex"]
            .as_str()
            .ok_or("C enc missing compressed_hex")?
            .to_string();
        let c_compressed = hex::decode(&c_compressed_hex).map_err(|e| format!("C hex: {}", e))?;

        let c_self_decode = c_enc_json
            .get("decode_ok")
            .and_then(|v| v.as_bool())
            .ok_or("C enc missing decode_ok")?;

        // Rust encode
        let rust_compressed = match variant {
            "byte" => rust_byte_encode(&input, &freqs, &cum_freqs, scale_bits, path)?,
            "r64" => rust_r64_encode(&input, &freqs, &cum_freqs, scale_bits, path)?,
            "word" => rust_word_encode(&input, &freqs, &cum_freqs, scale_bits, num_symbols, path)?,
            _ => return Err(format!("unknown variant: {}", variant)),
        };
        let rust_compressed_hex = hex::encode(&rust_compressed);

        // Rust self-decode
        let cum2sym: Vec<u8> = (0..(total as usize))
            .map(|i| {
                for s in 0..freqs.len() {
                    if s >= cum_freqs.len() - 1 {
                        break;
                    }
                    if i >= cum_freqs[s] as usize && i < cum_freqs[s + 1] as usize {
                        return s as u8;
                    }
                }
                0
            })
            .collect();

        let rust_self_decode = match variant {
            "byte" => rust_byte_decode(
                &rust_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "r64" => rust_r64_decode(
                &rust_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "word" => rust_word_decode(
                &rust_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            _ => false,
        };

        // C→Rust cross-decode
        let c_to_rust = match variant {
            "byte" => rust_byte_decode(
                &c_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "r64" => rust_r64_decode(
                &c_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            "word" => rust_word_decode(
                &c_compressed,
                &cum2sym,
                &freqs,
                &cum_freqs,
                scale_bits,
                input.len(),
                path,
            )
            .map(|d| d == input)
            .unwrap_or(false),
            _ => false,
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
            let dec_json: serde_json::Value = serde_json::from_slice(&dec_output.stdout)
                .map_err(|e| format!("C dec JSON: {}", e))?;
            let decoded_hex = dec_json["decoded_hex"]
                .as_str()
                .ok_or("C dec missing decoded_hex")?;
            hex::decode(decoded_hex).unwrap_or_default() == input
        } else {
            false
        };

        let compressed_match = rust_compressed_hex == c_compressed_hex;
        let case_id = format!("CASE.{:06}", case_idx);

        let result = CaseResult {
            case_id,
            input_hex,
            frequencies: freqs.clone(),
            scale_bits,
            c_compressed_hex,
            rust_compressed_hex,
            compressed_match,
            c_self_decode,
            rust_self_decode,
            c_to_rust,
            rust_to_c,
        };

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

    let manifest = CaseManifest {
        schema_version: 1,
        court_id: court_id.clone(),
        court_path: path.label().to_string(),
        variant: variant.to_string(),
        profile: profile_label.to_string(),
        scale_bits,
        seed,
        cases: cases.clone(),
    };

    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| format!("manifest serialize: {}", e))?;
    let manifest_sha256 = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(&manifest_bytes);
        format!("{:x}", h.finalize())
    };

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

    let code_commit = std::env::var("RANS_GIT_COMMIT")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            Command::new("git")
                .args(["rev-parse", "HEAD"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "unknown".to_string())
        });

    let receipt_json = serde_json::to_string_pretty(&serde_json::json!({
        "schema_version": 1,
        "court_id": &court_id,
        "court_path": path.label(),
        "variant": variant,
        "profile": profile_label,
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

    let receipt: Receipt = serde_json::from_str(
        &serde_json::to_string(&serde_json::json!({
            "schema_version": 1,
            "court_id": &court_id,
            "court_path": path.label(),
            "variant": variant,
            "profile": profile_label,
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

// ---- Legacy court entry points (kept for backward compat) ----

pub fn run_byte_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
    path: CourtPath,
) -> Result<(Receipt, CaseManifest, Vec<u8>), String> {
    run_court_with_profile(
        oracle_path,
        scale_bits,
        seed,
        path,
        "byte",
        ModelProfile::Uniform256,
    )
}

pub fn run_r64_court(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
    path: CourtPath,
) -> Result<(Receipt, CaseManifest, Vec<u8>), String> {
    run_court_with_profile(
        oracle_path,
        scale_bits,
        seed,
        path,
        "r64",
        ModelProfile::Uniform256,
    )
}

// ---- Rust byte encode/decode (correct path dispatch) ----

macro_rules! use_core {
    () => {
        use ryg_rans_rs_core::{
            BackwardByteWriter, BackwardWord16Writer, BackwardWord32Writer, ByteInterleavedDecoder,
            ByteInterleavedEncoder, ByteReader, ForwardReader, Rans64DecSymbol, Rans64EncSymbol,
            Rans64State, RansByteDecSymbol, RansByteEncSymbol, RansByteState, RansWordSlot,
            RansWordState, RansWordTables, SliceBackwardWriter, Word16Reader, Word32Reader,
            rans_byte_dec_advance, rans_byte_dec_advance_step, rans_byte_dec_advance_symbol,
            rans_byte_dec_advance_symbol_step, rans_byte_dec_get, rans_byte_dec_init,
            rans_byte_dec_renorm, rans_byte_enc_flush, rans_byte_enc_put, rans_byte_enc_put_symbol,
            rans_word_dec_init, rans_word_dec_renorm, rans_word_dec_sym, rans_word_enc_flush,
            rans_word_enc_put, rans64_dec_advance, rans64_dec_advance_step,
            rans64_dec_advance_symbol, rans64_dec_advance_symbol_step, rans64_dec_get,
            rans64_dec_init, rans64_dec_renorm, rans64_enc_flush, rans64_enc_put,
            rans64_enc_put_symbol,
        };
    };
}

fn rust_byte_encode(
    input: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    match path {
        CourtPath::Division => {
            let mut buf = vec![0u8; input.len() * 4 + 32];
            let mut writer = BackwardByteWriter::new(&mut buf);
            let mut state = RansByteState::new();
            for &s in input.iter().rev() {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                rans_byte_enc_put(&mut state, &mut writer, start, freq, scale_bits)
                    .map_err(|e| format!("byte enc put: {:?}", e))?;
            }
            rans_byte_enc_flush(&state, &mut writer)
                .map_err(|e| format!("byte enc flush: {:?}", e))?;
            Ok(writer.encoded().to_vec())
        }
        CourtPath::Reciprocal => {
            let mut buf = vec![0u8; input.len() * 4 + 32];
            let mut writer = BackwardByteWriter::new(&mut buf);
            let mut state = RansByteState::new();
            for &s in input.iter().rev() {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                let sym = RansByteEncSymbol::new(start, freq, scale_bits)
                    .map_err(|e| format!("byte sym create: {:?}", e))?;
                rans_byte_enc_put_symbol(&mut state, &mut writer, &sym)
                    .map_err(|e| format!("byte enc put sym: {:?}", e))?;
            }
            rans_byte_enc_flush(&state, &mut writer)
                .map_err(|e| format!("byte enc flush: {:?}", e))?;
            Ok(writer.encoded().to_vec())
        }
    }
}

fn rust_byte_decode(
    compressed: &[u8],
    cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    expected_len: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let mut reader = ByteReader::new(compressed);
    let mut state =
        rans_byte_dec_init(&mut reader).map_err(|e| format!("byte dec init: {:?}", e))?;
    let mut output = Vec::with_capacity(expected_len);
    for _ in 0..expected_len {
        let cf = rans_byte_dec_get(&state, scale_bits);
        let s = cum2sym[cf as usize];
        output.push(s);
        match path {
            CourtPath::Division => {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                rans_byte_dec_advance(&mut state, &mut reader, start, freq, scale_bits)
                    .map_err(|e| format!("byte dec advance: {:?}", e))?;
            }
            CourtPath::Reciprocal => {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                let dsym = RansByteDecSymbol::new(start, freq)
                    .map_err(|e| format!("byte dec sym: {:?}", e))?;
                rans_byte_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits)
                    .map_err(|e| format!("byte dec advance sym: {:?}", e))?;
            }
        }
    }
    Ok(output)
}

// ---- Rust 64-bit encode/decode (correct path dispatch) ----

fn rust_r64_encode(
    input: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    match path {
        CourtPath::Division => {
            let mut buf = vec![0u8; input.len() * 8 + 32];
            let mut writer = BackwardWord32Writer::new(&mut buf);
            let mut state = Rans64State::new();
            for &s in input.iter().rev() {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                rans64_enc_put(&mut state, &mut writer, start, freq, scale_bits)
                    .map_err(|e| format!("r64 enc put: {:?}", e))?;
            }
            rans64_enc_flush(&state, &mut writer).map_err(|e| format!("r64 enc flush: {:?}", e))?;
            Ok(writer.encoded().to_vec())
        }
        CourtPath::Reciprocal => {
            let mut buf = vec![0u8; input.len() * 8 + 32];
            let mut writer = BackwardWord32Writer::new(&mut buf);
            let mut state = Rans64State::new();
            for &s in input.iter().rev() {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                let sym = Rans64EncSymbol::new(start, freq, scale_bits)
                    .map_err(|e| format!("r64 sym create: {:?}", e))?;
                rans64_enc_put_symbol(&mut state, &mut writer, &sym)
                    .map_err(|e| format!("r64 enc put sym: {:?}", e))?;
            }
            rans64_enc_flush(&state, &mut writer).map_err(|e| format!("r64 enc flush: {:?}", e))?;
            Ok(writer.encoded().to_vec())
        }
    }
}

fn rust_r64_decode(
    compressed: &[u8],
    cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    expected_len: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let mut reader = Word32Reader::new(compressed);
    let mut state = rans64_dec_init(&mut reader).map_err(|e| format!("r64 dec init: {:?}", e))?;
    let mut output = Vec::with_capacity(expected_len);
    for _ in 0..expected_len {
        let cf = rans64_dec_get(&state, scale_bits);
        let s = cum2sym[cf as usize];
        output.push(s);
        match path {
            CourtPath::Division => {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                rans64_dec_advance(&mut state, &mut reader, start, freq, scale_bits)
                    .map_err(|e| format!("r64 dec advance: {:?}", e))?;
            }
            CourtPath::Reciprocal => {
                let start = cum_freqs[s as usize];
                let freq = freqs[s as usize];
                let dsym = Rans64DecSymbol::new(start, freq)
                    .map_err(|e| format!("r64 dec sym: {:?}", e))?;
                rans64_dec_advance_symbol(&mut state, &mut reader, &dsym, scale_bits)
                    .map_err(|e| format!("r64 dec advance sym: {:?}", e))?;
            }
        }
    }
    Ok(output)
}

// ---- Interleaved encode/decode (two-state) ----

fn rust_byte_interleaved_encode(
    input: &[u8],
    _freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    _num_symbols: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let n = input.len();
    let mut buf = vec![0u8; n * 4 + 64];
    let mut writer = BackwardByteWriter::new(&mut buf);
    let mut state0 = RansByteState::new();
    let mut state1 = RansByteState::new();

    // Closure to encode one symbol via the selected path
    let mut enc_one = |state: &mut RansByteState,
                       writer: &mut BackwardByteWriter,
                       sym: usize|
     -> Result<(), String> {
        match path {
            CourtPath::Division => {
                rans_byte_enc_put(state, writer, cum_freqs[sym], _freqs[sym], scale_bits)
                    .map_err(|e| format!("int div enc: {:?}", e))
            }
            CourtPath::Reciprocal => {
                let start = cum_freqs[sym];
                let freq = _freqs[sym];
                let esym = RansByteEncSymbol::new(start, freq, scale_bits)
                    .map_err(|e| format!("int sym: {:?}", e))?;
                rans_byte_enc_put_symbol(state, writer, &esym)
                    .map_err(|e| format!("int enc sym: {:?}", e))
            }
        }
    };

    if n & 1 != 0 {
        let s = input[n - 1] as usize;
        enc_one(&mut state0, &mut writer, s)?;
    }
    let mut i = n & !1;
    while i > 0 {
        let s1 = input[i - 1] as usize;
        let s0 = input[i - 2] as usize;
        enc_one(&mut state1, &mut writer, s1)?;
        enc_one(&mut state0, &mut writer, s0)?;
        i = i.wrapping_sub(2);
    }
    rans_byte_enc_flush(&mut state1, &mut writer)
        .map_err(|e| format!("int enc flush1: {:?}", e))?;
    rans_byte_enc_flush(&mut state0, &mut writer)
        .map_err(|e| format!("int enc flush0: {:?}", e))?;

    Ok(writer.encoded().to_vec())
}

fn rust_byte_interleaved_decode(
    compressed: &[u8],
    cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    expected_len: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let num_symbols = freqs.len().min(256);
    let dsyms: Vec<RansByteDecSymbol> = (0..num_symbols)
        .map(|i| {
            let start = cum_freqs[i];
            let freq = freqs[i];
            if freq > 0 {
                RansByteDecSymbol::new(start, freq).unwrap()
            } else {
                RansByteDecSymbol { start: 0, freq: 1 }
            }
        })
        .collect();

    let mut reader = ByteReader::new(compressed);
    let mut state0 =
        rans_byte_dec_init(&mut reader).map_err(|e| format!("int dec init0: {:?}", e))?;
    let mut state1 =
        rans_byte_dec_init(&mut reader).map_err(|e| format!("int dec init1: {:?}", e))?;

    let n = expected_len;
    let even_n = n & !1;
    let mut output = vec![0u8; n];

    let mut pos = 0;
    while pos < even_n {
        let cf0 = rans_byte_dec_get(&state0, scale_bits);
        let s0 = cum2sym[cf0 as usize] as usize;
        let cf1 = rans_byte_dec_get(&state1, scale_bits);
        let s1 = cum2sym[cf1 as usize] as usize;

        output[pos] = s0 as u8;
        output[pos + 1] = s1 as u8;

        match path {
            CourtPath::Division => {
                rans_byte_dec_advance_step(&mut state0, cum_freqs[s0], freqs[s0], scale_bits);
                rans_byte_dec_advance_step(&mut state1, cum_freqs[s1], freqs[s1], scale_bits);
            }
            CourtPath::Reciprocal => {
                rans_byte_dec_advance_symbol_step(&mut state0, &dsyms[s0], scale_bits);
                rans_byte_dec_advance_symbol_step(&mut state1, &dsyms[s1], scale_bits);
            }
        }
        rans_byte_dec_renorm(&mut state0, &mut reader)
            .map_err(|e| format!("int renorm0 @{}: {:?}", pos, e))?;
        rans_byte_dec_renorm(&mut state1, &mut reader)
            .map_err(|e| format!("int renorm1 @{}: {:?}", pos, e))?;

        pos += 2;
    }

    if even_n < n {
        let cf = rans_byte_dec_get(&state0, scale_bits);
        let s_idx = cum2sym[cf as usize] as usize;
        output[even_n] = s_idx as u8;
        match path {
            CourtPath::Division => {
                rans_byte_dec_advance(
                    &mut state0,
                    &mut reader,
                    cum_freqs[s_idx],
                    freqs[s_idx],
                    scale_bits,
                )
                .map_err(|e| format!("int dec tail div: {:?}", e))?;
            }
            CourtPath::Reciprocal => {
                rans_byte_dec_advance_symbol(&mut state0, &mut reader, &dsyms[s_idx], scale_bits)
                    .map_err(|e| format!("int dec tail sym: {:?}", e))?;
            }
        }
    }

    Ok(output)
}

fn rust_r64_interleaved_encode(
    input: &[u8],
    _freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    _num_symbols: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let n = input.len();
    let mut buf = vec![0u8; n * 8 + 64];
    let mut writer = BackwardWord32Writer::new(&mut buf);
    let mut state0 = Rans64State::new();
    let mut state1 = Rans64State::new();

    let mut enc_one = |state: &mut Rans64State,
                       writer: &mut BackwardWord32Writer,
                       sym: usize|
     -> Result<(), String> {
        match path {
            CourtPath::Division => {
                rans64_enc_put(state, writer, cum_freqs[sym], _freqs[sym], scale_bits)
                    .map_err(|e| format!("r64 int div enc: {:?}", e))
            }
            CourtPath::Reciprocal => {
                let start = cum_freqs[sym];
                let freq = _freqs[sym];
                let esym = Rans64EncSymbol::new(start, freq, scale_bits)
                    .map_err(|e| format!("r64 int sym: {:?}", e))?;
                rans64_enc_put_symbol(state, writer, &esym)
                    .map_err(|e| format!("r64 int enc sym: {:?}", e))
            }
        }
    };

    if n & 1 != 0 {
        let s = input[n - 1] as usize;
        enc_one(&mut state0, &mut writer, s)?;
    }
    let mut i = n & !1;
    while i > 0 {
        let s1 = input[i - 1] as usize;
        let s0 = input[i - 2] as usize;
        enc_one(&mut state1, &mut writer, s1)?;
        enc_one(&mut state0, &mut writer, s0)?;
        i = i.wrapping_sub(2);
    }
    rans64_enc_flush(&mut state1, &mut writer)
        .map_err(|e| format!("r64 int enc flush1: {:?}", e))?;
    rans64_enc_flush(&mut state0, &mut writer)
        .map_err(|e| format!("r64 int enc flush0: {:?}", e))?;

    Ok(writer.encoded().to_vec())
}

fn rust_r64_interleaved_decode(
    compressed: &[u8],
    cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    expected_len: usize,
    path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let num_symbols = freqs.len().min(256);
    let dsyms: Vec<Rans64DecSymbol> = (0..num_symbols)
        .map(|i| {
            let start = cum_freqs[i];
            let freq = freqs[i];
            if freq > 0 {
                Rans64DecSymbol::new(start, freq).unwrap()
            } else {
                Rans64DecSymbol { start: 0, freq: 1 }
            }
        })
        .collect();

    let mut reader = Word32Reader::new(compressed);
    let mut state0 =
        rans64_dec_init(&mut reader).map_err(|e| format!("r64 int dec init0: {:?}", e))?;
    let mut state1 =
        rans64_dec_init(&mut reader).map_err(|e| format!("r64 int dec init1: {:?}", e))?;

    let n = expected_len;
    let even_n = n & !1;
    let mut output = vec![0u8; n];

    let mut pos = 0;
    while pos < even_n {
        let cf0 = rans64_dec_get(&state0, scale_bits);
        let s0 = cum2sym[cf0 as usize] as usize;
        let cf1 = rans64_dec_get(&state1, scale_bits);
        let s1 = cum2sym[cf1 as usize] as usize;

        output[pos] = s0 as u8;
        output[pos + 1] = s1 as u8;

        match path {
            CourtPath::Division => {
                rans64_dec_advance_step(&mut state0, cum_freqs[s0], freqs[s0], scale_bits);
                rans64_dec_advance_step(&mut state1, cum_freqs[s1], freqs[s1], scale_bits);
            }
            CourtPath::Reciprocal => {
                rans64_dec_advance_symbol_step(&mut state0, &dsyms[s0], scale_bits);
                rans64_dec_advance_symbol_step(&mut state1, &dsyms[s1], scale_bits);
            }
        }
        rans64_dec_renorm(&mut state0, &mut reader)
            .map_err(|e| format!("r64 int renorm0 @{}: {:?}", pos, e))?;
        rans64_dec_renorm(&mut state1, &mut reader)
            .map_err(|e| format!("r64 int renorm1 @{}: {:?}", pos, e))?;

        pos += 2;
    }

    if even_n < n {
        let cf = rans64_dec_get(&state0, scale_bits);
        let s_idx = cum2sym[cf as usize] as usize;
        output[even_n] = s_idx as u8;
        match path {
            CourtPath::Division => {
                rans64_dec_advance(
                    &mut state0,
                    &mut reader,
                    cum_freqs[s_idx],
                    freqs[s_idx],
                    scale_bits,
                )
                .map_err(|e| format!("r64 int tail div: {:?}", e))?;
            }
            CourtPath::Reciprocal => {
                rans64_dec_advance_symbol(&mut state0, &mut reader, &dsyms[s_idx], scale_bits)
                    .map_err(|e| format!("r64 int tail sym: {:?}", e))?;
            }
        }
    }

    Ok(output)
}

// ---- Word rANS encode/decode (division only, scale_bits=12 per upstream) ----

fn rust_word_encode(
    input: &[u8],
    _freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    _num_symbols: usize,
    _path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let mut buf = vec![0u8; input.len() * 4 + 32];
    let mut writer = BackwardWord16Writer::new(&mut buf);
    let mut state = RansWordState::new();
    for &s in input.iter().rev() {
        let start = cum_freqs[s as usize];
        let freq = _freqs[s as usize];
        rans_word_enc_put(&mut state, &mut writer, start, freq, scale_bits)
            .map_err(|e| format!("word enc put: {:?}", e))?;
    }
    rans_word_enc_flush(&state, &mut writer).map_err(|e| format!("word enc flush: {:?}", e))?;
    Ok(writer.encoded().to_vec())
}

fn rust_word_decode(
    compressed: &[u8],
    _cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    expected_len: usize,
    _path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    // Build word decode tables
    let m = 1usize << scale_bits;
    let mut slots = vec![RansWordSlot { freq: 0, bias: 0 }; m];
    let mut slot2sym = vec![0u8; m];
    for i in 0..freqs.len().min(256) {
        let freq = freqs[i];
        if freq > 0 {
            let start = cum_freqs[i] as usize;
            for j in 0..freq as usize {
                let slot = start + j;
                if slot < m {
                    slot2sym[slot] = i as u8;
                    slots[slot] = RansWordSlot {
                        freq: freq as u16,
                        bias: j as u16,
                    };
                }
            }
        }
    }
    let tables = RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };

    let mut reader = Word16Reader::new(compressed);
    let mut state =
        rans_word_dec_init(&mut reader).map_err(|e| format!("word dec init: {:?}", e))?;
    let mut output = vec![0u8; expected_len];
    for i in 0..expected_len {
        let s = rans_word_dec_sym(&mut state, &tables, scale_bits);
        output[i] = s;
        rans_word_dec_renorm(&mut state, &mut reader)
            .map_err(|e| format!("word dec renorm {}: {:?}", i, e))?;
    }
    Ok(output)
}

fn rust_word_interleaved_encode(
    input: &[u8],
    _freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    _num_symbols: usize,
    _path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    let n = input.len();
    let mut buf = vec![0u8; n * 4 + 64];
    let mut writer = BackwardWord16Writer::new(&mut buf);
    let mut state0 = RansWordState::new();
    let mut state1 = RansWordState::new();

    if n & 1 != 0 {
        let s = input[n - 1] as usize;
        rans_word_enc_put(
            &mut state0,
            &mut writer,
            cum_freqs[s],
            _freqs[s],
            scale_bits,
        )
        .map_err(|e| format!("word int enc tail: {:?}", e))?;
    }
    let mut i = n & !1;
    while i > 0 {
        let s1 = input[i - 1] as usize;
        let s0 = input[i - 2] as usize;
        rans_word_enc_put(
            &mut state1,
            &mut writer,
            cum_freqs[s1],
            _freqs[s1],
            scale_bits,
        )
        .map_err(|e| format!("word int enc s1@{}: {:?}", i, e))?;
        rans_word_enc_put(
            &mut state0,
            &mut writer,
            cum_freqs[s0],
            _freqs[s0],
            scale_bits,
        )
        .map_err(|e| format!("word int enc s0@{}: {:?}", i, e))?;
        i = i.wrapping_sub(2);
    }
    rans_word_enc_flush(&mut state1, &mut writer)
        .map_err(|e| format!("word int enc flush1: {:?}", e))?;
    rans_word_enc_flush(&mut state0, &mut writer)
        .map_err(|e| format!("word int enc flush0: {:?}", e))?;

    Ok(writer.encoded().to_vec())
}

fn rust_word_interleaved_decode(
    compressed: &[u8],
    _cum2sym: &[u8],
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
    expected_len: usize,
    _path: CourtPath,
) -> Result<Vec<u8>, String> {
    use_core!();
    // Build word decode tables
    let m = 1usize << scale_bits;
    let mut slots = vec![RansWordSlot { freq: 0, bias: 0 }; m];
    let mut slot2sym = vec![0u8; m];
    for i in 0..freqs.len().min(256) {
        let freq = freqs[i];
        if freq > 0 {
            let start = cum_freqs[i] as usize;
            for j in 0..freq as usize {
                let slot = start + j;
                if slot < m {
                    slot2sym[slot] = i as u8;
                    slots[slot] = RansWordSlot {
                        freq: freq as u16,
                        bias: j as u16,
                    };
                }
            }
        }
    }
    let tables = RansWordTables {
        slots: &slots,
        slot2sym: &slot2sym,
    };

    let mut reader = Word16Reader::new(compressed);
    let mut state0 =
        rans_word_dec_init(&mut reader).map_err(|e| format!("word int dec init0: {:?}", e))?;
    let mut state1 =
        rans_word_dec_init(&mut reader).map_err(|e| format!("word int dec init1: {:?}", e))?;

    let n = expected_len;
    let even_n = n & !1;
    let mut output = vec![0u8; n];

    let mut pos = 0;
    while pos < even_n {
        output[pos] = rans_word_dec_sym(&mut state0, &tables, scale_bits);
        output[pos + 1] = rans_word_dec_sym(&mut state1, &tables, scale_bits);
        rans_word_dec_renorm(&mut state0, &mut reader)
            .map_err(|e| format!("word int renorm0 @{}: {:?}", pos, e))?;
        rans_word_dec_renorm(&mut state1, &mut reader)
            .map_err(|e| format!("word int renorm1 @{}: {:?}", pos, e))?;
        pos += 2;
    }

    if even_n < n {
        output[even_n] = rans_word_dec_sym(&mut state0, &tables, scale_bits);
        rans_word_dec_renorm(&mut state0, &mut reader)
            .map_err(|e| format!("word int dec tail: {:?}", e))?;
    }

    Ok(output)
}
