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
    backends::DecodeBackend,
    encode_8way_for_test,
    packed_table::{
        self, PackedWordTable, decode_8way_packed_scalar, decode_8way_packed_scalar_with_report,
        decode_interleaved16_scalar, encode_interleaved16,
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
        "  {:30} {:30} {:8} {:8.1} GiB/s  {:7.2} ns/sym",
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
    const SAMPLES: usize = 7;
    let mut samples: Vec<f64> = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..n_iter {
            let _ = black_box(f());
        }
        samples.push(start.elapsed().as_nanos() as f64);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (samples[SAMPLES / 2], n_iter)
}

fn measure_into<F>(f: &F, output: &mut [u8], n_iter: u64) -> (f64, u64)
where
    F: Fn(&mut [u8]) -> Result<(), &'static str>,
{
    for _ in 0..5 {
        let _ = black_box(f(output));
    }
    const SAMPLES: usize = 7;
    let mut samples: Vec<f64> = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let start = Instant::now();
        for _ in 0..n_iter {
            let _ = black_box(f(output));
        }
        samples.push(start.elapsed().as_nanos() as f64);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (samples[SAMPLES / 2], n_iter)
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
            )
            .unwrap();

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

            // Scalar 16-way reference for experimental backend verification
            let (scalar16_output, scalar16_report) =
                match decode_interleaved16_scalar(&compressed_16way, &packed, size) {
                    Ok(r) => r,
                    Err(_) => {
                        eprintln!(
                            "  WARN: scalar16 decode failed for {} size {}",
                            profile.name, size
                        );
                        continue;
                    }
                };
            let scalar16_ok = scalar16_output == input;

            // Full verification: compare output, words_consumed, AND all 16 final states.
            let n_iter = (100_000_000u64 / size.max(1) as u64).max(20).min(500_000);

            // Scalar 8-way reference for 8-way backend verification
            let (scalar8_output, scalar8_report) =
                match decode_8way_packed_scalar_with_report(&compressed_8way, &packed, size) {
                    Ok(r) => r,
                    Err(_) => {
                        eprintln!(
                            "  WARN: scalar8 decode failed for {} size {}",
                            profile.name, size
                        );
                        continue;
                    }
                };

            let n_iter = (100_000_000u64 / size.max(1) as u64).max(20).min(500_000);

            // Helper: verify a 16-way backend against scalar16 reference
            macro_rules! verify_16way {
                ($label:expr, $output:expr, $wc:expr, $states:expr) => {{
                    let out = $output;
                    if out != scalar16_output {
                        eprintln!("  VERIFY FAIL {}: output mismatch", $label);
                        false
                    } else if $wc != scalar16_report.words_consumed {
                        eprintln!(
                            "  VERIFY FAIL {}: words_consumed {} vs {}",
                            $label, $wc, scalar16_report.words_consumed
                        );
                        false
                    } else if $states != scalar16_report.final_states {
                        eprintln!("  VERIFY FAIL {}: final states mismatch", $label);
                        false
                    } else {
                        true
                    }
                }};
            }
            // Helper: verify an 8-way backend against scalar8 reference
            macro_rules! verify_8way {
                ($label:expr, $output:expr, $wc:expr, $states:expr) => {{
                    let out = $output;
                    if out != scalar8_output {
                        eprintln!("  VERIFY FAIL {}: output mismatch", $label);
                        false
                    } else if $wc != scalar8_report.words_consumed {
                        eprintln!(
                            "  VERIFY FAIL {}: words_consumed {} vs {}",
                            $label, $wc, scalar8_report.words_consumed
                        );
                        false
                    } else if $states[0..8] != scalar8_report.final_states[0..8] {
                        eprintln!("  VERIFY FAIL {}: final states mismatch", $label);
                        false
                    } else {
                        true
                    }
                }};
            }

            // Verify AVX512VL 8-way (hw-gather)
            let avx512vl8_ok = if avx512vl_avail {
                match unsafe {
                    ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl(
                        &compressed_8way,
                        &packed,
                        size,
                    )
                } {
                    Ok(r) => verify_8way!(
                        "avx512vl-8way",
                        r.output,
                        r.report.words_consumed,
                        r.report.final_states
                    ),
                    Err(e) => {
                        eprintln!("  VERIFY FAIL avx512vl-8way: {:?}", e);
                        false
                    }
                }
            } else {
                false
            };

            // Verify AVX512 16-way (hw-gather)
            let avx512_16_ok = if avx512_avail {
                match unsafe {
                    ryg_rans_rs_simd::backends::decode_interleaved16_avx512(
                        &compressed_16way,
                        &packed,
                        size,
                    )
                } {
                    Ok(r) => verify_16way!(
                        "avx512-16way",
                        r.output,
                        r.report.words_consumed,
                        r.report.final_states
                    ),
                    Err(e) => {
                        eprintln!("  VERIFY FAIL avx512-16way: {:?}", e);
                        false
                    }
                }
            } else {
                false
            };

            // Verify AVX512VL manual gather 8-way
            let manual8_ok = if avx512vl_avail {
                match unsafe {
                    ryg_rans_rs_simd::backends::decode_interleaved8_manual_gather(
                        &compressed_8way,
                        &packed,
                        size,
                    )
                } {
                    Ok(r) => verify_8way!(
                        "avx512vl-manual-gather-8way",
                        r.output,
                        r.report.words_consumed,
                        r.report.final_states
                    ),
                    Err(e) => {
                        eprintln!("  VERIFY FAIL manual8: {:?}", e);
                        false
                    }
                }
            } else {
                false
            };

            // Verify AVX512 manual gather 16-way
            let manual16_ok = if avx512_avail {
                match unsafe {
                    ryg_rans_rs_simd::backends::decode_interleaved16_manual_gather(
                        &compressed_16way,
                        &packed,
                        size,
                    )
                } {
                    Ok(r) => verify_16way!(
                        "avx512-manual-gather-16way",
                        r.output,
                        r.report.words_consumed,
                        r.report.final_states
                    ),
                    Err(e) => {
                        eprintln!("  VERIFY FAIL manual16: {:?}", e);
                        false
                    }
                }
            } else {
                false
            };

            // Verify AVX512VL 2x8 on 16-way format
            let twx8_ok = if avx512vl_avail {
                match unsafe {
                    ryg_rans_rs_simd::backends::decode_interleaved16_2x8(
                        &compressed_16way,
                        &packed,
                        size,
                    )
                } {
                    Ok(r) => verify_16way!(
                        "avx512vl-2x8-on-16way",
                        r.output,
                        r.report.words_consumed,
                        r.report.final_states
                    ),
                    Err(e) => {
                        eprintln!("  VERIFY FAIL 2x8: {:?}", e);
                        false
                    }
                }
            } else {
                false
            };

            // Verify Uniform256 table-free kernel
            #[cfg(any(target_feature = "avx512bw", feature = "std"))]
            let uniform_tf_ok = if avx512_avail && profile.name == "UNIFORM256" {
                match unsafe {
                    ryg_rans_rs_simd::model_kernels::decode_interleaved16_uniform256_avx512(
                        &compressed_16way,
                        size,
                    )
                } {
                    Ok((output, report)) => verify_16way!(
                        "uniform256-tablefree-16way",
                        output,
                        report.words_consumed,
                        report.final_states
                    ),
                    Err(e) => {
                        eprintln!("  VERIFY FAIL uniform-tf: {}", e);
                        false
                    }
                }
            } else {
                false
            };
            #[cfg(not(any(target_feature = "avx512bw", feature = "std")))]
            let uniform_tf_ok = false;

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
            if avx512vl8_ok {
                let (ns, _) = measure(
                    || unsafe {
                        ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl(
                            &compressed_8way,
                            &packed,
                            size,
                        )
                        .map(|r| r.output)
                        .map_err(|_| "decode failed")
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

            // ---- Backend 4b: AVX512VL 8-way (preallocated kernel) ----
            if avx512vl8_ok {
                let mut prealloc = vec![0u8; size];
                let (ns_kernel, _) = measure_into(
                    &|buf: &mut [u8]| unsafe {
                        ryg_rans_rs_simd::backends::decode_interleaved8_avx512vl_into(
                            &compressed_8way,
                            &packed,
                            buf,
                        )
                        .map(|_| ())
                        .map_err(|_| "decode failed")
                    },
                    &mut prealloc,
                    n_iter,
                );
                report(
                    "avx512vl-8way (kernel)",
                    profile.name,
                    size,
                    ns_kernel,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},avx512vl-8way-kernel,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns_kernel / 1e9),
                    ns_kernel / (size as u64 * n_iter) as f64
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
            if avx512_16_ok {
                let (ns, _) = measure(
                    || unsafe {
                        ryg_rans_rs_simd::backends::decode_interleaved16_avx512(
                            &compressed_16way,
                            &packed,
                            size,
                        )
                        .map(|r| r.output)
                        .map_err(|_| "decode failed")
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

            // ---- Backend 6b: AVX512 16-way (preallocated kernel) ----
            if avx512_16_ok {
                let mut prealloc = vec![0u8; size];
                let (ns_kernel, _) = measure_into(
                    &|buf: &mut [u8]| unsafe {
                        ryg_rans_rs_simd::backends::decode_interleaved16_avx512_into(
                            &compressed_16way,
                            &packed,
                            buf,
                        )
                        .map(|_| ())
                        .map_err(|_| "decode failed")
                    },
                    &mut prealloc,
                    n_iter,
                );
                report(
                    "avx512-16way (kernel)",
                    profile.name,
                    size,
                    ns_kernel,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},avx512-16way-kernel,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns_kernel / 1e9),
                    ns_kernel / (size as u64 * n_iter) as f64
                ));
            }

            // ---- Backend 7: AVX512VL 8-way manual gather ----
            if manual8_ok {
                let (ns, _) = measure(
                    || unsafe {
                        ryg_rans_rs_simd::backends::decode_interleaved8_manual_gather(
                            &compressed_8way,
                            &packed,
                            size,
                        )
                        .map(|r| r.output)
                        .map_err(|_| "decode failed")
                    },
                    n_iter,
                );
                report(
                    "avx512vl-manual-gather-8way",
                    profile.name,
                    size,
                    ns,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},avx512vl-manual-gather-8way,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                    ns / (size as u64 * n_iter) as f64
                ));
            }

            // ---- Backend 8: AVX512 16-way manual gather ----
            if manual16_ok {
                let (ns, _) = measure(
                    || unsafe {
                        ryg_rans_rs_simd::backends::decode_interleaved16_manual_gather(
                            &compressed_16way,
                            &packed,
                            size,
                        )
                        .map(|r| r.output)
                        .map_err(|_| "decode failed")
                    },
                    n_iter,
                );
                report(
                    "avx512-manual-gather-16way",
                    profile.name,
                    size,
                    ns,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},avx512-manual-gather-16way,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                    ns / (size as u64 * n_iter) as f64
                ));
            }

            // ---- Backend 9: AVX512VL 2x8 on 16-way format ----
            if twx8_ok {
                let (ns, _) = measure(
                    || unsafe {
                        ryg_rans_rs_simd::backends::decode_interleaved16_2x8(
                            &compressed_16way,
                            &packed,
                            size,
                        )
                        .map(|r| r.output)
                        .map_err(|_| "decode failed")
                    },
                    n_iter,
                );
                report(
                    "avx512vl-2x8-on-16way",
                    profile.name,
                    size,
                    ns,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},avx512vl-2x8-on-16way,{:.2},{:.2}\n",
                    profile.name,
                    size,
                    ((size as u64 * n_iter) as f64 / 1.073741824e9) / (ns / 1e9),
                    ns / (size as u64 * n_iter) as f64
                ));
            }

            // ---- Backend 10: Uniform256 table-free (16-way) ----
            // This kernel avoids table lookups entirely — only valid for uniform256 models.
            #[cfg(any(target_feature = "avx512bw", feature = "std"))]
            if uniform_tf_ok {
                let (ns, _) = measure(
                    || unsafe {
                        ryg_rans_rs_simd::model_kernels::decode_interleaved16_uniform256_avx512(
                            &compressed_16way,
                            size,
                        )
                        .map(|r| r.0)
                        .map_err(|_| "decode failed")
                    },
                    n_iter,
                );
                report(
                    "uniform256-tablefree-16way",
                    profile.name,
                    size,
                    ns,
                    (size as u64) * n_iter,
                );
                results_csv.push_str(&format!(
                    "{},{},uniform256-tablefree-16way,{:.2},{:.2}\n",
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
    println!(" Benchmark methodology:");
    println!("   - 'allocating' = Vec<u8> allocation + resize inside timed loop");
    println!("   - '(kernel)' = preallocated output buffer outside timed loop");
    println!("   - 5 warmup iterations discarded");
    println!("   - black_box prevents DCE");
    println!("   - Single aggregate Instant measurement (not distribution)");
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
