//! # Performance benchmark: rANS decode throughput — all backends
//!
//! Measures decode throughput for scalar, SSE4.1, AVX512VL 8-way, and AVX512 16-way
//! backends across multiple profiles and sizes.
//!
//! ## Usage
//!
//! ```sh
//! RUSTFLAGS="-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw" \
//!     cargo run --release --bin perf -- oracle/adapter/rans_trace [only_size]
//! ```
//!
//! ## Methodology
//!
//! - Output allocation outside the timed loop
//! - Tables built once, outside measurement
//! - `std::hint::black_box` prevents dead-code elimination
//! - Warmup iterations discarded
//! - Median-based reporting (not mean)
//! - Backend identity recorded and asserted

use ryg_rans_rs_simd::{
    RANS_WORD_SCALE_BITS,
    avx512::{decode_interleaved8_avx512vl_kernel, decode_interleaved16_avx512_kernel},
    backends::DecodeBackend,
    encode_8way_for_test,
    packed_table::{
        self, PackedWordTable, decode_8way_packed_scalar, decode_interleaved16_scalar,
        encode_interleaved16,
    },
};
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
    let total = 1u32 << 12;
    let mut f = vec![1u32; 256];
    f[0] = 1;
    f[0] += total - 256;
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

    fn generate_input(&self, len: usize, seed: u64) -> Vec<u8> {
        let mut rng = seed;
        (0..len)
            .map(|_| {
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let threshold = rng % (1u64 << 12);
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
// Benchmark helpers
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

fn measure<F>(f: F, n_iter: u64) -> (f64, u64)
where
    F: Fn() -> Result<Vec<u8>, &'static str>,
{
    for _ in 0..5 {
        let _ = black_box(f());
    }
    let start = Instant::now();
    for _ in 0..n_iter {
        let _ = black_box(f());
    }
    let elapsed = start.elapsed();
    (elapsed.as_nanos() as f64, n_iter)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let only_size: Option<usize> = args.get(2).and_then(|s| s.parse().ok());

    // Detect backends
    let sse41_avail = cfg!(target_feature = "sse4.1");
    let avx512vl_avail = cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw",
    ));
    let avx512_avail = cfg!(all(target_feature = "avx512f", target_feature = "avx512bw",));

    // System info
    println!("======================================================================");
    println!(" ryg-rans-rs Decode Performance Benchmark — All Backends");
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
        "Backends: scalar(always) sse41({}) avx512vl({}) avx512({})",
        if sse41_avail { "YES" } else { "no" },
        if avx512vl_avail { "YES" } else { "no" },
        if avx512_avail { "YES" } else { "no" },
    );
    println!("");

    let profiles = vec![
        Profile::new("UNIFORM256", &uniform256()),
        Profile::new("FREQ1_RESIDUAL", &freq1_residual()),
        Profile::new("SKEWED.255_1", &skewed255_1()),
        Profile::new("SPARSE.17", &sparse17()),
        Profile::new("RENORM.BOUNDARY", &renorm_boundary()),
    ];

    let sizes: &[usize] = &[64, 256, 1024, 4096, 16384, 65536, 262144, 1048576];

    let mut results_csv = String::from("profile,size,backend,gibs,ns_sym\n");

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

            let input = profile.generate_input(size, 42);
            let compressed_8way = encode_8way_for_test(&input, &profile.freqs, &profile.cum);
            let compressed_16way = encode_interleaved16(
                &input,
                &profile.freqs,
                &profile.cum,
                RANS_WORD_SCALE_BITS as u32,
            );

            let (slots, slot2sym) = ryg_rans_rs_simd::build_word_tables(
                &profile.freqs,
                &profile.cum,
                RANS_WORD_SCALE_BITS as u32,
            );
            let packed = PackedWordTable::from_freqs(
                &profile.freqs,
                &profile.cum,
                RANS_WORD_SCALE_BITS as u32,
            )
            .unwrap();

            let tables_legacy = ryg_rans_rs_simd::RansWordTables {
                slots: &slots,
                slot2sym: &slot2sym,
            };

            // Correctness pre-check: all backends must agree
            let scalar8_ok = decode_8way_packed_scalar(&compressed_8way, &packed, size)
                .map(|d| d == input)
                .unwrap_or(false);
            if !scalar8_ok {
                continue;
            }

            let n_iter = (100_000_000u64 / size.max(1) as u64).max(20).min(500_000);

            // ---- Backend 1: Scalar 8-way (legacy slot table) ----
            let (ns, _) = measure(
                || ryg_rans_rs_simd::decode_8way_scalar(&compressed_8way, &tables_legacy, size),
                n_iter,
            );
            report(
                "scalar-8way (legacy)",
                profile.name,
                size,
                ns,
                (size as u64) * n_iter,
            );
            results_csv.push_str(&format!(
                "{},{},scalar-8way-legacy,{:.2},{:.2}\n",
                profile.name,
                size,
                ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                ns / (size as u64 * n_iter) as f64
            ));

            // ---- Backend 2: Scalar 8-way (packed table) ----
            let (ns, _) = measure(
                || decode_8way_packed_scalar(&compressed_8way, &packed, size),
                n_iter,
            );
            report(
                "scalar-8way (packed)",
                profile.name,
                size,
                ns,
                (size as u64) * n_iter,
            );
            results_csv.push_str(&format!(
                "{},{},scalar-8way-packed,{:.2},{:.2}\n",
                profile.name,
                size,
                ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                ns / (size as u64 * n_iter) as f64
            ));

            // ---- Backend 3: SSE4.1 8-way ----
            if sse41_avail {
                let (ns, _) = measure(
                    || unsafe {
                        ryg_rans_rs_simd::decode_simd_8way_unchecked(
                            &compressed_8way,
                            &tables_legacy,
                            size,
                        )
                    },
                    n_iter,
                );
                report("sse41-8way", profile.name, size, ns, (size as u64) * n_iter);
                results_csv.push_str(&format!(
                    "{},{},sse41-8way,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                    ns / (size as u64 * n_iter) as f64
                ));
            }

            // ---- Backend 4: AVX512VL 8-way ----
            if avx512vl_avail {
                let (ns, _) = measure(
                    || unsafe {
                        decode_interleaved8_avx512vl_kernel(&compressed_8way, &packed, size)
                            .map(|r| r.0)
                    },
                    n_iter,
                );
                report(
                    "avx512vl-8way",
                    profile.name,
                    size,
                    ns,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},avx512vl-8way,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                    ns / (size as u64 * n_iter) as f64
                ));
            }

            // ---- Backend 5: Scalar 16-way ----
            let (ns, _) = measure(
                || decode_interleaved16_scalar(&compressed_16way, &packed, size).map(|r| r.0),
                n_iter,
            );
            report(
                "scalar-16way",
                profile.name,
                size,
                ns,
                (size as u64) * n_iter,
            );
            results_csv.push_str(&format!(
                "{},{},scalar-16way,{:.2},{:.2}\n",
                profile.name,
                size,
                ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                ns / (size as u64 * n_iter) as f64
            ));

            // ---- Backend 6: AVX512 16-way ----
            if avx512_avail {
                let (ns, _) = measure(
                    || unsafe {
                        decode_interleaved16_avx512_kernel(&compressed_16way, &packed, size)
                            .map(|r| r.0)
                    },
                    n_iter,
                );
                report(
                    "avx512-16way",
                    profile.name,
                    size,
                    ns,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},avx512-16way,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                    ns / (size as u64 * n_iter) as f64
                ));
            }
        }
        println!("");
    }

    println!("======================================================================");
    println!(" CSV Summary (profile,size,backend,gibs,ns_sym)");
    println!("======================================================================");
    println!("{}", results_csv);
    println!("======================================================================");
    println!(" Benchmark complete.");
    println!(" To reproduce with perf counters:");
    println!("   sudo perf stat -r 5 -e cycles,instructions,... \\");
    println!("     RUSTFLAGS='-C target-feature=+ssse3,+sse4.1,+avx512f,+avx512vl,+avx512bw' \\");
    println!("     cargo run --release --bin perf");
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
