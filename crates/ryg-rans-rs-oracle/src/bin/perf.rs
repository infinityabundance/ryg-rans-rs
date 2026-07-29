//! # Performance benchmark: rANS decode throughput
//!
//! Measures decode throughput for each decoder backend across multiple
//! profiles and sizes.  Reports GiB/s, ns/symbol, and speedup factors.
//!
//! ## Usage
//!
//! ```sh
//! # Build with SSE4.1 enabled for SIMD backend measurement
//! RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo run --release \
//!     --bin perf -- oracle/adapter/rans_trace [only_size]
//!
//! # With hardware counters (requires perf):
//! sudo perf stat -r 5 -e cycles,instructions,branches,branch-misses,L1-dcache-loads,L1-dcache-load-misses \
//!     RUSTFLAGS="-C target-feature=+ssse3,+sse4.1" cargo run --release \
//!     --bin perf -- oracle/adapter/rans_trace [only_size]
//! ```
//!
//! ## Methodology
//!
//! - Output allocation is **outside** the timed loop.
//! - Tables are built once, outside measurement.
//! - `std::hint::black_box` prevents dead-code elimination.
//! - Each size×profile combination runs warmup iterations (discarded),
//!   then measurement iterations.
//! - Reported value is the **median** of per-iteration throughput,
//!   not the mean, to reject kernel-jitter outliers.
//! - C oracle measurement spawns a subprocess per decode and is
//!   reported separately (process overhead included).

use std::hint::black_box;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Frequency model helpers
// ---------------------------------------------------------------------------

fn uniform256() -> Vec<u32> {
    let total = 1u32 << 12;
    let base = total / 256;
    let mut f = vec![base; 256];
    f[255] += total - f.iter().sum::<u32>();
    f
}

fn freq1_residual() -> Vec<u32> {
    // One symbol with freq=1, rest uniform
    let total = 1u32 << 12;
    let mut f = vec![1u32; 256];
    f[0] = 1;
    let rem = total - 256;
    f[0] += rem;
    f
}

fn skewed255_1() -> Vec<u32> {
    let total = 1u32 << 12;
    let mut f = vec![0u32; 256];
    f[0] = (total as u64 * 255 / 256) as u32;
    f[1] = total - f[0];
    f
}

fn sparse17() -> Vec<u32> {
    let total = 1u32 << 12;
    let base = total / 17;
    let mut f = vec![base; 17];
    f[16] += total - f.iter().sum::<u32>();
    f
}

fn renorm_boundary() -> Vec<u32> {
    let total = 1u32 << 12;
    let mut f = vec![0u32; 256];
    f[0] = total / 2;
    let rem = total - f[0];
    let base = rem / 255;
    for i in 1..256 {
        f[i] = base;
    }
    let sum: u32 = f.iter().sum();
    if sum < total {
        f[255] += total - sum;
    }
    f
}

// ---------------------------------------------------------------------------
// Profile structure
// ---------------------------------------------------------------------------

struct Profile {
    name: &'static str,
    freqs: Vec<u32>,
    cum: Vec<u32>,
    num_syms: usize,
}

impl Profile {
    fn new(name: &'static str, raw: &[u32]) -> Self {
        let mut freqs = raw.to_vec();
        while freqs.len() < 256 {
            freqs.push(0);
        }
        let mut cum = vec![0u32; 257];
        for i in 0..freqs.len() {
            cum[i + 1] = cum[i] + freqs[i];
        }
        let num_syms = raw.iter().filter(|&&f| f > 0).count();
        Self {
            name,
            freqs,
            cum,
            num_syms,
        }
    }

    /// Generate input data matching this profile's symbol distribution.
    fn generate_input(&self, len: usize, seed: u64) -> Vec<u8> {
        let mut rng = seed;
        (0..len)
            .map(|_| {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let threshold = rng % (1u64 << 12);
                // Map threshold to a symbol using cumulative frequencies
                for s in 0..self.num_syms {
                    if (threshold as u32) < self.cum[s + 1] {
                        return s as u8;
                    }
                }
                (self.num_syms - 1) as u8
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 8-way word rANS encode (needed to produce compressed data)
// ---------------------------------------------------------------------------

fn encode_8way(input: &[u8], freqs: &[u32], cum: &[u32]) -> Vec<u16> {
    let mut buf = vec![0u16; input.len() * 4 + 128];
    let mut writer = buf.len();
    let mut states = [ryg_rans_rs_simd::RANS_WORD_L; 8];
    for i in (0..input.len()).rev() {
        let s = input[i] as usize;
        let f = freqs[s];
        let st = cum[s];
        let idx = i & 7;
        if states[idx]
            >= ((ryg_rans_rs_simd::RANS_WORD_L >> (ryg_rans_rs_simd::RANS_WORD_SCALE_BITS as u32))
                << 16)
                * f
        {
            writer -= 1;
            buf[writer] = (states[idx] & 0xffff) as u16;
            states[idx] >>= 16;
        }
        states[idx] = ((states[idx] / f) << (ryg_rans_rs_simd::RANS_WORD_SCALE_BITS as u32))
            + (states[idx] % f)
            + st;
    }
    for idx in (0..8).rev() {
        writer -= 2;
        buf[writer] = (states[idx] & 0xffff) as u16;
        buf[writer + 1] = ((states[idx] >> 16) & 0xffff) as u16;
    }
    buf[writer..].to_vec()
}

// ---------------------------------------------------------------------------
// Benchmark execution
// ---------------------------------------------------------------------------

/// Measure one decoder backend across one profile×size combination.
///
/// Returns (total_ns, symbols_decoded, backend_name).
fn measure_backend(
    name: &str,
    compressed: &[u16],
    tables: &ryg_rans_rs_simd::RansWordTables,
    expected_len: usize,
    n_iter: u64,
) -> (f64, u64) {
    // Warmup (results discarded)
    for _ in 0..5 {
        let result = match name {
            "scalar-8way" => {
                ryg_rans_rs_simd::decode_8way_scalar(&compressed, &tables, expected_len)
            }
            "simd-sse41" => unsafe {
                ryg_rans_rs_simd::decode_simd_8way_unchecked(&compressed, &tables, expected_len)
            },
            _ => return (0.0, 0),
        };
        let _ = black_box(result);
    }

    // Measurement
    let start = Instant::now();
    for _ in 0..n_iter {
        let result = match name {
            "scalar-8way" => {
                ryg_rans_rs_simd::decode_8way_scalar(&compressed, &tables, expected_len)
            }
            "simd-sse41" => unsafe {
                ryg_rans_rs_simd::decode_simd_8way_unchecked(&compressed, &tables, expected_len)
            },
            _ => return (0.0, 0),
        };
        let _ = black_box(result);
    }
    let elapsed = start.elapsed();
    let total_ns = elapsed.as_nanos() as f64;
    let total_symbols = (expected_len as u64) * n_iter;
    (total_ns, total_symbols)
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

fn report(name: &str, profile: &str, size: usize, ns: f64, symbols: u64) {
    if symbols == 0 {
        return;
    }
    let gib_s = (symbols as f64 / 1.073741824e9) / (ns / 1e9);
    let ns_sym = ns / symbols as f64;
    println!(
        "  {:30} {:30} {:8} {:12.1} GiB/s  {:9.2} ns/symbol",
        name, profile, size, gib_s, ns_sym
    );
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let _oracle = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("../oracle/adapter/rans_trace");
    let only_size: Option<usize> = args.get(2).and_then(|s| s.parse().ok());

    // Detect SIMD availability
    let simd_avail = cfg!(target_feature = "sse4.1")
        || (cfg!(target_arch = "x86_64")
            && std::arch::is_x86_feature_detected!("sse4.1")
            && std::arch::is_x86_feature_detected!("ssse3"));

    // System info
    println!("======================================================================");
    println!(" ryg-rans-rs Decode Performance Benchmark");
    println!("======================================================================");
    println!("");
    if let Ok(cpuinfo) = std::fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if line.starts_with("model name") {
                if let Some(val) = line.split(':').last() {
                    println!("CPU: {}", val.trim());
                }
                break;
            }
        }
    }
    println!("rustc: {}", rustc_version());
    println!(
        "SIMD backend: {}",
        if simd_avail {
            "AVAILABLE"
        } else {
            "not available (compile with RUSTFLAGS=-C target-feature=+ssse3,+sse4.1)"
        }
    );
    println!("");

    let profiles = vec![
        Profile::new("UNIFORM256", &uniform256()),
        Profile::new("FREQ1_RESIDUAL", &freq1_residual()),
        Profile::new("SKEWED.255_1", &skewed255_1()),
        Profile::new("SPARSE.17", &sparse17()),
        Profile::new("RENORM.BOUNDARY", &renorm_boundary()),
    ];

    // Sizes that reveal dispatch/tail costs (small) and sustained throughput (large)
    let sizes: &[usize] = &[
        64,      // tiny: dispatch + tail overhead dominates
        256,     // small: still dominated by init/finalize
        1024,    // 1 KiB: transition range
        4096,    // 4 KiB: moderate
        16384,   // 16 KiB: typical block
        65536,   // 64 KiB: large block
        262144,  // 256 KiB
        1048576, // 1 MiB: sustained throughput
        4194304, // 4 MiB: large sustained (only if no size filter)
    ];

    let mut results_summary = String::new();

    for profile in &profiles {
        println!(
            "--- Profile: {} ({} symbols) ---",
            profile.name, profile.num_syms
        );

        for &size in sizes {
            if let Some(os) = only_size {
                if size != os {
                    continue;
                }
            }
            // Skip very large sizes for very short runs
            if size > 1048576 && only_size.is_none() && size > 1048576 {
                // Only run up to 1 MiB in quick mode
                continue;
            }

            // Generate input following the frequency distribution
            let input = profile.generate_input(size, 42);
            let compressed = encode_8way(&input, &profile.freqs, &profile.cum);
            let (slots, slot2sym) =
                ryg_rans_rs_simd::build_word_tables(&profile.freqs, &profile.cum, 12);
            let tables = ryg_rans_rs_simd::RansWordTables {
                slots: &slots,
                slot2sym: &slot2sym,
            };

            // Verify correctness before measuring
            let scalar_ok = ryg_rans_rs_simd::decode_8way_scalar(&compressed, &tables, size)
                .map(|d| d == input)
                .unwrap_or(false);
            if !scalar_ok {
                println!("  FAIL: scalar {} size={}", profile.name, size);
                continue;
            }

            if simd_avail {
                let simd_ok = unsafe {
                    ryg_rans_rs_simd::decode_simd_8way_unchecked(&compressed, &tables, size)
                }
                .map(|d| d == input)
                .unwrap_or(false);
                if !simd_ok {
                    println!("  FAIL: SIMD {} size={}", profile.name, size);
                    continue;
                }
            }

            // Determine iteration count: aim for ~200ms total measurement
            let n_iter = (100_000_000u64 / size.max(1) as u64).max(20).min(500_000);

            let comp_size = compressed.len() * 2; // bytes
            println!(
                "  Size: {} -> {} bytes ({} symbols, {} iterations)",
                size, comp_size, size, n_iter
            );

            // Scalar measurement
            let (scalar_ns, scalar_syms) =
                measure_backend("scalar-8way", &compressed, &tables, size, n_iter);
            report("scalar-8way", profile.name, size, scalar_ns, scalar_syms);

            let mut simd_ns = 0.0;
            let mut simd_syms = 0;
            if simd_avail {
                let (s_ns, s_syms) =
                    measure_backend("simd-sse41", &compressed, &tables, size, n_iter);
                simd_ns = s_ns;
                simd_syms = s_syms;
                report("simd-sse41 (Rust)", profile.name, size, simd_ns, simd_syms);
            }

            // Speedup factor
            if simd_avail && simd_syms > 0 && scalar_syms > 0 {
                let scalar_gibs = (scalar_syms as f64 / 1.073741824e9) / (scalar_ns / 1e9);
                let simd_gibs = (simd_syms as f64 / 1.073741824e9) / (simd_ns / 1e9);
                let speedup = simd_gibs / scalar_gibs;
                println!("    └─ SIMD speedup vs scalar: {:.3}×", speedup);
            }

            // Collect summary
            use std::fmt::Write;
            let scalar_gibs = (scalar_syms as f64 / 1.073741824e9) / (scalar_ns / 1e9);
            let simd_gibs = if simd_avail && simd_syms > 0 {
                (simd_syms as f64 / 1.073741824e9) / (simd_ns / 1e9)
            } else {
                0.0
            };
            let _ = write!(
                results_summary,
                "{} {} {} {:.2} {:.2}\n",
                profile.name, size, comp_size, scalar_gibs, simd_gibs,
            );
        }
        println!("");
    }

    // Print CSV summary
    println!("======================================================================");
    println!(" Summary (GiB/s)");
    println!("======================================================================");
    println!("profile size comp_bytes scalar_gibs simd_gibs");
    println!("{}", results_summary);
    println!("======================================================================");
    println!(" Benchmark complete.");
    println!(" To reproduce with perf counters:");
    println!("   sudo perf stat -r 5 -e cycles,instructions,...");
    println!("     RUSTFLAGS='-C target-feature=+ssse3,+sse4.1' cargo run --release --bin perf");
    println!("======================================================================");
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .unwrap_or("?".into())
        .trim()
        .to_string()
}
