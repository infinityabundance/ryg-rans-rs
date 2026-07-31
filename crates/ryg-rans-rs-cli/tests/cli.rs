//! # CLI integration tests
//!
//! These tests exercise the compiled `ryg-rans` binary end-to-end
//! (`CARGO_BIN_EXE_ryg-rans`), so exit codes, stdio behaviour, container
//! hashing, and the strict integrity policy are all tested exactly as a
//! user would experience them.
//!
//! Every test uses a unique temp directory under `std::env::temp_dir()`.

use std::path::PathBuf;
use std::process::{Command, Output};

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_ryg-rans")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "rygrans-cli-test-{}-{}-{:?}",
            tag,
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("spawn ryg-rans")
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> Output {
    use std::io::Write;
    let mut child = Command::new(bin())
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn ryg-rans");
    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    child.wait_with_output().expect("wait ryg-rans")
}

/// Deterministic pseudo-random test data with a strong 0-symbol bias plus
/// some runs (exercises RLE, RANS, and RAW fallback in one stream).
fn make_data(seed: u64, len: usize) -> Vec<u8> {
    let mut x = seed.wrapping_mul(0x9e3779b97f4a7c15).wrapping_add(0x12345);
    let mut data = Vec::with_capacity(len);
    for i in 0..len {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let b = if i % 512 == 0 {
            7 // periodic run trigger
        } else if x % 8 == 0 {
            (x >> 8) as u8
        } else {
            0
        };
        data.push(b);
    }
    data
}

#[test]
fn encode_decode_roundtrip_all_codecs() {
    let td = TempDir::new("roundtrip");
    let data = make_data(42, 200_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    for codec in [
        "byte-single",
        "byte-interleaved2",
        "r64-single",
        "word-single",
    ] {
        let rygr = td.path(&format!("{}.rygr", codec));
        let out = td.path(&format!("{}.out", codec));
        let enc = run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap(),
            "--codec",
            codec,
        ]);
        assert!(
            enc.status.success(),
            "encode {} failed: {}",
            codec,
            String::from_utf8_lossy(&enc.stderr)
        );
        let dec = run(&[
            "decode",
            "-i",
            rygr.to_str().unwrap(),
            "-o",
            out.to_str().unwrap(),
        ]);
        assert!(
            dec.status.success(),
            "decode {} failed: {}",
            codec,
            String::from_utf8_lossy(&dec.stderr)
        );
        assert_eq!(
            std::fs::read(&out).unwrap(),
            data,
            "roundtrip mismatch for {}",
            codec
        );
    }
}

#[test]
fn encode_decode_roundtrip_via_stdin_stdout() {
    let data = make_data(99, 300_000);
    let enc = run_with_stdin(&["encode", "-i", "-", "-o", "-"], &data);
    assert!(
        enc.status.success(),
        "encode: {}",
        String::from_utf8_lossy(&enc.stderr)
    );
    let dec = run_with_stdin(&["decode", "-i", "-", "-o", "-"], &enc.stdout);
    assert!(
        dec.status.success(),
        "decode: {}",
        String::from_utf8_lossy(&dec.stderr)
    );
    assert_eq!(dec.stdout, data, "stdin/stdout roundtrip mismatch");
}

#[test]
fn rle_and_raw_blocks() {
    let td = TempDir::new("kinds");
    // Pure RLE: 100 KiB of the same byte.
    let rle_in = td.path("rle.bin");
    std::fs::write(&rle_in, vec![0x5a; 100_000]).unwrap();
    let rle_rygr = td.path("rle.rygr");
    let rle_out = td.path("rle.out");
    assert!(
        run(&[
            "encode",
            "-i",
            rle_in.to_str().unwrap(),
            "-o",
            rle_rygr.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "decode",
            "-i",
            rle_rygr.to_str().unwrap(),
            "-o",
            rle_out.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(std::fs::read(&rle_out).unwrap().len(), 100_000);

    // Tiny random block: rANS cannot shrink it → RAW fallback.
    let raw_in = td.path("raw.bin");
    let raw_data = make_data(7, 96);
    std::fs::write(&raw_in, &raw_data).unwrap();
    let raw_rygr = td.path("raw.rygr");
    let raw_out = td.path("raw.out");
    assert!(
        run(&[
            "encode",
            "-i",
            raw_in.to_str().unwrap(),
            "-o",
            raw_rygr.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "decode",
            "-i",
            raw_rygr.to_str().unwrap(),
            "-o",
            raw_out.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(std::fs::read(&raw_out).unwrap(), raw_data);
}

#[test]
fn multi_block_roundtrip() {
    let td = TempDir::new("multiblock");
    let data = make_data(5, 500_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let rygr = td.path("out.rygr");
    let out = td.path("out.bin");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap(),
            "--block-size",
            "64KiB"
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "decode",
            "-i",
            rygr.to_str().unwrap(),
            "-o",
            out.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert_eq!(std::fs::read(&out).unwrap(), data);
}

#[test]
fn verify_passes_and_inspect_deep_passes() {
    let td = TempDir::new("verify");
    let data = make_data(11, 150_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let rygr = td.path("out.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap()
        ])
        .status
        .success()
    );
    let v = run(&["verify", "-i", rygr.to_str().unwrap()]);
    assert!(
        v.status.success(),
        "verify failed: {}",
        String::from_utf8_lossy(&v.stderr)
    );
    assert!(String::from_utf8_lossy(&v.stdout).contains("OK"));
    let i = run(&["inspect", "-i", rygr.to_str().unwrap(), "--deep"]);
    assert!(
        i.status.success(),
        "inspect --deep failed: {}",
        String::from_utf8_lossy(&i.stderr)
    );
    assert!(String::from_utf8_lossy(&i.stdout).contains("deep verification: passed"));
}

#[test]
fn corrupted_container_fails_with_integrity_exit_code() {
    let td = TempDir::new("corrupt");
    let data = make_data(13, 100_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let rygr = td.path("out.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap()
        ])
        .status
        .success()
    );

    // Flip a byte in the middle of the payload region.
    let mut bytes = std::fs::read(&rygr).unwrap();
    let mid = bytes.len() / 2;
    bytes[mid] ^= 0xff;
    let corrupt = td.path("corrupt.rygr");
    std::fs::write(&corrupt, &bytes).unwrap();

    let dec = run(&[
        "decode",
        "-i",
        corrupt.to_str().unwrap(),
        "-o",
        td.path("o.bin").to_str().unwrap(),
    ]);
    assert_eq!(dec.status.code(), Some(5), "expected integrity exit code 5");
    let ver = run(&["verify", "-i", corrupt.to_str().unwrap()]);
    assert_eq!(ver.status.code(), Some(5), "expected verify exit code 5");
}

#[test]
fn truncated_container_is_a_format_error() {
    let td = TempDir::new("trunc");
    let data = make_data(17, 50_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let rygr = td.path("out.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap()
        ])
        .status
        .success()
    );
    let bytes = std::fs::read(&rygr).unwrap();
    let truncated = td.path("trunc.rygr");
    std::fs::write(&truncated, &bytes[..bytes.len() - 40]).unwrap();
    let dec = run(&[
        "decode",
        "-i",
        truncated.to_str().unwrap(),
        "-o",
        td.path("o.bin").to_str().unwrap(),
    ]);
    assert_eq!(dec.status.code(), Some(4), "expected format exit code 4");
}

#[test]
fn force_guard_refuses_overwrite_without_force() {
    let td = TempDir::new("force");
    let data = make_data(19, 10_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let rygr = td.path("out.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap()
        ])
        .status
        .success()
    );
    // Second encode without --force must refuse.
    let again = run(&[
        "encode",
        "-i",
        input.to_str().unwrap(),
        "-o",
        rygr.to_str().unwrap(),
    ]);
    assert_eq!(again.status.code(), Some(3), "expected I/O exit code 3");
    // With --force it must succeed.
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap(),
            "--force"
        ])
        .status
        .success()
    );
}

#[test]
fn unsupported_codec_is_exit_6() {
    let td = TempDir::new("unsupported");
    let data = make_data(23, 10_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let enc = run(&[
        "encode",
        "-i",
        input.to_str().unwrap(),
        "-o",
        td.path("o.rygr").to_str().unwrap(),
        "--codec",
        "alias-single",
    ]);
    assert_eq!(
        enc.status.code(),
        Some(6),
        "expected unsupported exit code 6"
    );
}

#[test]
fn model_build_validate_compare() {
    let td = TempDir::new("model");
    let data = make_data(29, 60_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let model = td.path("model.bin");
    let b = run(&[
        "model",
        "build",
        "-i",
        input.to_str().unwrap(),
        "-o",
        model.to_str().unwrap(),
        "--output-format",
        "binary",
    ]);
    assert!(
        b.status.success(),
        "model build: {}",
        String::from_utf8_lossy(&b.stderr)
    );
    let v = run(&["model", "validate", "-i", model.to_str().unwrap()]);
    assert!(
        v.status.success(),
        "model validate: {}",
        String::from_utf8_lossy(&v.stderr)
    );
    assert!(String::from_utf8_lossy(&v.stdout).contains("OK"));
    let c = run(&[
        "model",
        "compare",
        "--a",
        model.to_str().unwrap(),
        "--b",
        model.to_str().unwrap(),
    ]);
    assert!(
        c.status.success(),
        "model compare: {}",
        String::from_utf8_lossy(&c.stderr)
    );
}

#[test]
fn compare_arithmetic_division_reciprocal_identical() {
    let td = TempDir::new("arith");
    let data = make_data(31, 120_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let c = run(&["compare", "arithmetic", "-i", input.to_str().unwrap()]);
    assert!(
        c.status.success(),
        "compare arithmetic: {}",
        String::from_utf8_lossy(&c.stderr)
    );
    assert!(String::from_utf8_lossy(&c.stdout).contains("OK"));
}

#[test]
fn compare_files_equal_and_different() {
    let td = TempDir::new("cmpfiles");
    let data = make_data(37, 80_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let a = td.path("a.rygr");
    let b = td.path("b.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            a.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            b.to_str().unwrap()
        ])
        .status
        .success()
    );
    assert!(
        run(&[
            "compare",
            "files",
            "--a",
            a.to_str().unwrap(),
            "--b",
            b.to_str().unwrap()
        ])
        .status
        .success()
    );

    // Different content must fail with exit 8.
    let other = td.path("other.bin");
    std::fs::write(&other, make_data(41, 80_000)).unwrap();
    let c = td.path("c.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            other.to_str().unwrap(),
            "-o",
            c.to_str().unwrap()
        ])
        .status
        .success()
    );
    let diff = run(&[
        "compare",
        "files",
        "--a",
        a.to_str().unwrap(),
        "--b",
        c.to_str().unwrap(),
    ]);
    assert_eq!(
        diff.status.code(),
        Some(8),
        "expected comparison exit code 8"
    );
}

#[test]
fn trace_reports_symbols_and_states() {
    let td = TempDir::new("trace");
    let data = make_data(43, 20_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let rygr = td.path("out.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap(),
            "--codec",
            "byte-single"
        ])
        .status
        .success()
    );
    let t = run(&[
        "trace",
        "-i",
        rygr.to_str().unwrap(),
        "--block",
        "0",
        "--max-symbols",
        "3",
    ]);
    assert!(
        t.status.success(),
        "trace: {}",
        String::from_utf8_lossy(&t.stderr)
    );
    let stdout = String::from_utf8_lossy(&t.stdout);
    assert!(stdout.contains("step"), "trace output: {}", stdout);
    assert!(stdout.lines().count() == 3, "expected 3 trace lines");
}

#[test]
fn bench_round_trip_verified() {
    let b = run(&["bench", "--samples", "3"]);
    assert!(
        b.status.success(),
        "bench: {}",
        String::from_utf8_lossy(&b.stderr)
    );
    assert!(String::from_utf8_lossy(&b.stdout).contains("round trip verified"));
}

#[test]
fn decode_rejects_tampered_decoded_hash() {
    // Corrupt the decoded-data hash field in the block header: decode must
    // fail with exit 5 even though the payload is untouched (the strict
    // integrity policy requires the decoded hash to match).
    let td = TempDir::new("dec-hash");
    let data = make_data(47, 60_000);
    let input = td.path("input.bin");
    std::fs::write(&input, &data).unwrap();
    let rygr = td.path("out.rygr");
    assert!(
        run(&[
            "encode",
            "-i",
            input.to_str().unwrap(),
            "-o",
            rygr.to_str().unwrap()
        ])
        .status
        .success()
    );

    let mut bytes = std::fs::read(&rygr).unwrap();
    // Header is 32 bytes; block header is 104 bytes with decoded_sha256 at
    // block-header offset 72, i.e. file offset 32 + 72.
    let field = 32 + 72;
    bytes[field] ^= 0xff;
    let tampered = td.path("tampered.rygr");
    std::fs::write(&tampered, &bytes).unwrap();

    let dec = run(&[
        "decode",
        "-i",
        tampered.to_str().unwrap(),
        "-o",
        td.path("o.bin").to_str().unwrap(),
    ]);
    assert_eq!(dec.status.code(), Some(5), "expected integrity exit code 5");
}
