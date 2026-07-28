use std::path::Path;

/// Cross-decoding court runner.
///
/// Invokes the C oracle adapter, runs Rust encode/decode, compares results,
/// and writes sealed receipts.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let oracle_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("oracle/adapter/rans_trace");
    let scale_bits: u32 = args.get(2).map(|s| s.parse().unwrap_or(12)).unwrap_or(12);
    let seed: u64 = args.get(3).map(|s| s.parse().unwrap_or(42)).unwrap_or(42);
    let num_cases: usize = args.get(4).map(|s| s.parse().unwrap_or(10)).unwrap_or(10);

    println!("=== ryg-rans-rs Cross-Decoding Court ===");
    println!("Oracle:     {}", oracle_path);
    println!("Scale bits: {}", scale_bits);
    println!("Seed:       {}", seed);
    println!("Cases:      {}", num_cases);
    println!();

    // Verify oracle exists
    if !Path::new(oracle_path).exists() {
        let alt_path = "../oracle/adapter/rans_trace";
        if Path::new(alt_path).exists() {
            return run_with_path(alt_path, scale_bits, seed, num_cases);
        }
        eprintln!("ERROR: oracle not found at '{}'", oracle_path);
        eprintln!("Build it first: cd oracle/adapter && make");
        std::process::exit(1);
    }

    run_with_path(oracle_path, scale_bits, seed, num_cases)
}

fn run_with_path(
    oracle_path: &str,
    scale_bits: u32,
    seed: u64,
    num_cases: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    // ---- Byte rANS cross-decoding court ----
    println!("--- Court: RYG_RANS.BYTE.CROSS_DECODE ---");
    let byte_receipt =
        ryg_rans_rs_oracle::run_byte_cross_court(oracle_path, scale_bits, seed, num_cases)?;

    println!("  Verdict: {}", byte_receipt.verdict);
    println!("  Cases:   {}", byte_receipt.case_count);
    println!(
        "  Matched: {}/{}",
        byte_receipt.pairs_matched, byte_receipt.pairs_compared
    );
    println!("  Residuals: {}", byte_receipt.residual_count);

    let receipt_path = format!("reports/drafts/receipt-{}.json", byte_receipt.court_id);
    if let Some(parent) = Path::new(&receipt_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&receipt_path, serde_json::to_string_pretty(&byte_receipt)?)?;
    println!("  Receipt: {}", receipt_path);

    // ---- 64-bit rANS cross-decoding court ----
    println!();
    println!("--- Court: RYG_RANS.R64.CROSS_DECODE ---");
    let r64_receipt =
        ryg_rans_rs_oracle::run_r64_cross_court(oracle_path, scale_bits, seed, num_cases)?;

    println!("  Verdict: {}", r64_receipt.verdict);
    println!("  Cases:   {}", r64_receipt.case_count);
    println!(
        "  Matched: {}/{}",
        r64_receipt.pairs_matched, r64_receipt.pairs_compared
    );
    println!("  Residuals: {}", r64_receipt.residual_count);

    let r64_receipt_path = format!("reports/drafts/receipt-{}.json", r64_receipt.court_id);
    std::fs::write(
        &r64_receipt_path,
        serde_json::to_string_pretty(&r64_receipt)?,
    )?;
    println!("  Receipt: {}", r64_receipt_path);

    // ---- Summary ----
    println!();
    println!("=== Summary ===");
    if byte_receipt.verdict == "admitted_match" && r64_receipt.verdict == "admitted_match" {
        println!("ALL COURTS PASSED — both byte and 64-bit cross-decoding sealed.");
        Ok(())
    } else {
        eprintln!("SOME COURTS FAILED:");
        if byte_receipt.verdict != "admitted_match" {
            eprintln!("  byte: {}", byte_receipt.verdict);
        }
        if r64_receipt.verdict != "admitted_match" {
            eprintln!("  64-bit: {}", r64_receipt.verdict);
        }
        std::process::exit(1);
    }
}
