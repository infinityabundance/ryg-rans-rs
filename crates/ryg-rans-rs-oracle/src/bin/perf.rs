//! Performance benchmark: SIMD vs scalar vs C word rANS decode.
//! Usage:
//!   cargo run --bin perf --features=ryg-rans-rs-simd -- <oracle-path>

use std::process::Command;
use std::time::Instant;

fn build_tables(
    freqs: &[u32],
    cum_freqs: &[u32],
    scale_bits: u32,
) -> (Vec<ryg_rans_rs_simd::RansWordSlot>, Vec<u8>) {
    ryg_rans_rs_simd::build_word_tables(freqs, cum_freqs, scale_bits)
}

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

fn generate_input(len: usize, num_symbols: usize, seed: u64) -> Vec<u8> {
    let mut rng = seed;
    (0..len)
        .map(|_| {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (rng as usize % num_symbols) as u8
        })
        .collect()
}

fn report(name: &str, ns: f64, symbols: u64) {
    let gib_s = (symbols as f64 / 1.073741824e9) / (ns / 1e9);
    let ns_sym = ns / symbols as f64;
    println!(
        "  {:30} {:9.1} GiB/s  {:8.1} ns/symbol  {:10.0} ns total",
        name, gib_s, ns_sym, ns
    );
}

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
}

fn uniform256() -> Vec<u32> {
    let total = 1u32 << 12;
    let base = total / 256;
    let mut f = vec![base; 256];
    f[255] += total - f.iter().sum::<u32>();
    f
}
fn skewed2() -> Vec<u32> {
    let total = 1u32 << 12;
    let mut f = vec![0u32; 2];
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
fn renorm() -> Vec<u32> {
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let oracle = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("../oracle/adapter/rans_trace");
    let only_size: Option<usize> = args.get(2).and_then(|s| s.parse().ok());

    let simd_avail = cfg!(target_feature = "sse4.1")
        || (cfg!(target_arch = "x86_64")
            && std::arch::is_x86_feature_detected!("sse4.1")
            && std::arch::is_x86_feature_detected!("ssse3"));

    println!(
        "SIMD backend: {}",
        if simd_avail {
            "AVAILABLE"
        } else {
            "not available"
        }
    );
    {
        // CPU model extraction
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
    }
    println!("rustc: {}", rustc_version());
    println!("");

    let profiles = vec![
        Profile::new("UNIFORM256", &uniform256()),
        Profile::new("SKEWED.255_1", &skewed2()),
        Profile::new("SPARSE.17", &sparse17()),
        Profile::new("RENORM.BOUNDARY", &renorm()),
    ];

    let sizes: &[usize] = &[64, 256, 1024, 4096, 16384, 65536, 1048576];

    for profile in &profiles {
        println!("=== Profile: {} ===", profile.name);
        for &size in sizes {
            if let Some(os) = only_size {
                if size != os {
                    continue;
                }
            }
            if size > 1048576 {
                continue;
            }

            let input = generate_input(size, profile.num_syms, 42);
            let compressed = encode_8way(&input, &profile.freqs, &profile.cum);
            let (slots, slot2sym) = build_tables(&profile.freqs, &profile.cum, 12);
            let tables = ryg_rans_rs_simd::RansWordTables {
                slots: &slots,
                slot2sym: &slot2sym,
            };

            let comp_u8: Vec<u8> = compressed.iter().flat_map(|&w| w.to_le_bytes()).collect();
            let comp_hex = hex::encode(&comp_u8);

            // Verify all decoders
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

            let freq_csv = profile
                .freqs
                .iter()
                .map(|f| f.to_string())
                .collect::<Vec<_>>()
                .join(",");
            if size > 0 {
                let c_out = Command::new(oracle)
                    .args([
                        "dec-stream-simd",
                        "12",
                        &freq_csv,
                        &comp_hex,
                        &size.to_string(),
                    ])
                    .output()
                    .unwrap();
                if !c_out.status.success() {
                    println!("  FAIL: C {} size={}", profile.name, size);
                    continue;
                }
            }

            println!("  Size: {} bytes ({} symbols)", comp_u8.len(), size);

            // Warmup
            for _ in 0..5 {
                let _ = std::hint::black_box(ryg_rans_rs_simd::decode_8way_scalar(
                    &compressed,
                    &tables,
                    size,
                ));
                if simd_avail {
                    let _ = std::hint::black_box(unsafe {
                        ryg_rans_rs_simd::decode_simd_8way_unchecked(&compressed, &tables, size)
                    });
                }
            }

            let n_iter = (50_000_000u64 / size.max(1) as u64).max(50).min(500_000);

            // Scalar
            let start = Instant::now();
            for _ in 0..n_iter {
                let _ = std::hint::black_box(ryg_rans_rs_simd::decode_8way_scalar(
                    &compressed,
                    &tables,
                    size,
                ));
            }
            let s_ns = start.elapsed().as_nanos() as f64 / n_iter as f64;
            report("scalar-8way", s_ns, size as u64);

            // Rust SIMD
            if simd_avail {
                let start = Instant::now();
                for _ in 0..n_iter {
                    let _ = std::hint::black_box(unsafe {
                        ryg_rans_rs_simd::decode_simd_8way_unchecked(&compressed, &tables, size)
                    });
                }
                let simd_ns = start.elapsed().as_nanos() as f64 / n_iter as f64;
                report("simd-sse41 (Rust)", simd_ns, size as u64);
            }

            // C SIMD (process-per-call, fewer iterations)
            let n_iter_c = (1_000_000u64 / size.max(1) as u64).max(5).min(10_000);
            if n_iter_c >= 5 {
                let start = Instant::now();
                for _ in 0..n_iter_c {
                    let out = Command::new(oracle)
                        .args([
                            "dec-stream-simd",
                            "12",
                            &freq_csv,
                            &comp_hex,
                            &size.to_string(),
                        ])
                        .output()
                        .unwrap();
                    let _ = std::hint::black_box(out.stdout);
                }
                let c_ns = start.elapsed().as_nanos() as f64 / n_iter_c as f64;
                report("simd-sse41 (C oracle)", c_ns, size as u64);
            }
        }
    }
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
