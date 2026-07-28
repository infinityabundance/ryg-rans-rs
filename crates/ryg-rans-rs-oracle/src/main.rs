use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let oracle_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("oracle/adapter/rans_trace");
    let scale_bits: u32 = args.get(2).map(|s| s.parse().unwrap_or(12)).unwrap_or(12);
    let seed: u64 = args.get(3).map(|s| s.parse().unwrap_or(42)).unwrap_or(42);
    let num_cases: usize = args.get(4).map(|s| s.parse().unwrap_or(10)).unwrap_or(10);

    let oracle = if Path::new(oracle_path).exists() {
        oracle_path
    } else {
        "../oracle/adapter/rans_trace"
    };

    if !Path::new(oracle).exists() {
        eprintln!("ERROR: oracle not found at '{}'", oracle);
        eprintln!("Build it first: cd oracle/adapter && make");
        std::process::exit(1);
    }

    let receipt_dir = "evidence/receipts";
    std::fs::create_dir_all(receipt_dir)?;

    let courts: Vec<(
        &str,
        fn(&str, u32, u64, usize) -> Result<ryg_rans_rs_oracle::Receipt, String>,
    )> = vec![
        (
            "BYTE.RECIPROCAL",
            ryg_rans_rs_oracle::run_byte_reciprocal_cross_court,
        ),
        (
            "BYTE.DIVISION",
            ryg_rans_rs_oracle::run_byte_division_cross_court,
        ),
        (
            "R64.RECIPROCAL",
            ryg_rans_rs_oracle::run_r64_reciprocal_cross_court,
        ),
        (
            "R64.DIVISION",
            ryg_rans_rs_oracle::run_r64_division_cross_court,
        ),
    ];

    let mut all_passed = true;

    for (name, court_fn) in courts {
        println!("--- Court: {} ---", name);
        let receipt = court_fn(oracle, scale_bits, seed, num_cases)?;
        println!(
            "  Verdict: {}  ({}/{})",
            receipt.verdict, receipt.pairs_matched, receipt.pairs_compared
        );
        println!("  Residuals: {}", receipt.residual_count);

        if receipt.verdict != "admitted_match" {
            all_passed = false;
        }

        let path = format!("{}/receipt-{}.json", receipt_dir, receipt.court_id);
        std::fs::write(&path, serde_json::to_string_pretty(&receipt)?)?;
        println!("  Receipt: {}", path);
        println!();
    }

    if all_passed {
        println!("ALL COURTS PASSED — receipts in {}/", receipt_dir);
        Ok(())
    } else {
        eprintln!("SOME COURTS FAILED");
        std::process::exit(1);
    }
}
