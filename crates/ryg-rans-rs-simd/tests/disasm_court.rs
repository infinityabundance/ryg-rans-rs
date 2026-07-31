//! # Disassembly courts (Phase L.10)
//!
//! Prove that the expected ISA instructions are emitted for each kernel
//! family under the corresponding target features.  Each court compiles a
//! minimal kernel that uses the same intrinsic family as the production
//! kernels and asserts the expected mnemonics appear in the emitted
//! assembly.
//!
//! Evidence chain: these courts prove the **toolchain emits** the ISA
//! instructions for the feature flags; the native-build execution tests in
//! the parallel crate prove the real kernels **execute** and produce
//! byte-identical output.  Together they close the "kernel compiled" and
//! "kernel executed" gap.
//!
//! Courts run on any x86_64 host (they compile with explicit target
//! features, independent of the host CPU).

use std::path::PathBuf;
use std::process::Command;

/// Compile `body` with the given bare target features (comma-separated,
/// no `+` prefix) and return the emitted assembly (lowercased), or panic
/// with the compiler diagnostics.
fn emit_asm(body: &str, target_features: &str) -> String {
    let cargo_flag = target_features
        .split(',')
        .map(|f| format!("+{f}"))
        .collect::<Vec<_>>()
        .join(",");
    let src = format!(
        "#![no_std]\n\
         use core::arch::x86_64::*;\n\
         #[inline(never)]\n\
         #[allow(unused_unsafe, unused_variables)]\n\
         #[target_feature(enable = \"{target_features}\")]\n\
         pub unsafe fn kernel(p: *const i32, n: usize) -> i32 {{\n\
         \x20   unsafe {{\n{body}\n    }}\n\
         }}\n"
    );
    let dir = std::env::temp_dir().join(format!(
        "ryg-disasm-court-{}-{}",
        std::process::id(),
        target_features.replace(',', "_")
    ));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let file = dir.join("court.rs");
    std::fs::write(&file, &src).expect("write court source");

    let out = Command::new("rustc")
        .current_dir(&dir)
        .args([
            "--crate-type=lib",
            "--emit=asm",
            "-C",
            "opt-level=3",
            "-C",
            &format!("target-feature={cargo_flag}"),
            "-o",
            "-",
        ])
        .arg(&file)
        .output()
        .expect("run rustc");

    let _ = std::fs::remove_dir_all(&dir);

    if !out.status.success() {
        panic!(
            "rustc failed for features {target_features}:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let asm = String::from_utf8_lossy(&out.stdout).to_lowercase();
    assert!(
        !asm.is_empty(),
        "empty assembly emitted for features {target_features}"
    );
    asm
}

/// Assert the emitted assembly contains every expected mnemonic.
fn assert_mnemonics(asm: &str, target_features: &str, expected: &[&str]) {
    for m in expected {
        assert!(
            asm.contains(m),
            "features {target_features}: expected mnemonic `{m}` in emitted assembly"
        );
    }
}

/// Assert the emitted assembly contains none of the forbidden mnemonics.
fn assert_no_mnemonics(asm: &str, target_features: &str, forbidden: &[&str]) {
    for m in forbidden {
        assert!(
            !asm.contains(m),
            "features {target_features}: unexpected mnemonic `{m}` in emitted assembly"
        );
    }
}

#[test]
fn court_sse41_emits_pshufb_and_pblendvb() {
    // The SSE4.1 8-way kernel uses `_mm_shuffle_epi8` (PSHUFB, SSSE3) for
    // renorm word placement and `_mm_blendv_epi8` (PBLENDVB, SSE4.1) for
    // masked lane update.
    let asm = emit_asm(
        "let a = _mm_loadu_si128(p as *const __m128i);\
         let b = _mm_shuffle_epi8(a, _mm_set1_epi8(2));\
         let m = _mm_loadu_si128((p as *const __m128i).add(1));\
         let bl = _mm_blendv_epi8(a, b, m);\
         let bl = core::hint::black_box(bl);\
         let sums = _mm_sad_epu8(bl, _mm_setzero_si128());\
         let acc = _mm_add_epi32(sums, _mm_shuffle_epi32(sums, 0xee));\
         _mm_cvtsi128_si32(acc) + (n as i32)",
        "ssse3,sse4.1",
    );
    assert_mnemonics(&asm, "ssse3+sse4.1", &["pshufb", "pblendvb"]);
    // PBLENDVB's AVX-256 successor must not appear in a 128-bit-only build.
    assert_no_mnemonics(&asm, "ssse3+sse4.1", &["vpblendvb"]);
}

#[test]
fn court_avx2_emits_vpermd_and_vpgatherdd() {
    // The AVX2 manual-gather kernel uses `_mm256_permutevar8x32_epi32`
    // (VPERMD) and the hardware-gather kernel uses `_mm256_i32gather_epi32`
    // (VPGATHERDD).
    let asm = emit_asm(
        "let a = _mm256_loadu_si256(p as *const __m256i);\
         let idx = _mm256_set1_epi32(7);\
         let perm = _mm256_permutevar8x32_epi32(a, idx);\
         let g = _mm256_i32gather_epi32(p, idx, 4);\
         let perm = core::hint::black_box(perm);\
         let g = core::hint::black_box(g);\
         let mut acc: i32 = n as i32;\
         acc = acc.wrapping_add(_mm256_extract_epi32(perm, 0));\
         acc = acc.wrapping_add(_mm256_extract_epi32(perm, 4));\
         acc = acc.wrapping_add(_mm256_extract_epi32(g, 1));\
         acc = acc.wrapping_add(_mm256_extract_epi32(g, 5));\
         acc",
        "avx2",
    );
    assert_mnemonics(&asm, "avx2", &["vpermd", "vpgatherdd"]);
    // VPMOVDB is AVX-512-only; it must not appear in an AVX2 build.
    assert_no_mnemonics(&asm, "avx2", &["vpmovdb"]);
}

#[test]
fn court_avx512_emits_vpmovdb() {
    // The AVX-512 16-way kernel narrows 16 i32 lanes to 16 bytes with
    // `_mm512_cvtepi32_epi8` (VPMOVDB — an AVX-512F/BW-only instruction).
    let asm = emit_asm(
        "let v = _mm512_loadu_si512(p as *const __m512i);\
         let b = _mm512_cvtepi32_epi8(v);\
         let b = core::hint::black_box(b);\
         let mut acc: i32 = n as i32;\
         acc = acc.wrapping_add(_mm_extract_epi32(b, 0));\
         acc = acc.wrapping_add(_mm_extract_epi32(b, 2));\
         acc",
        "avx512f,avx512bw",
    );
    assert_mnemonics(&asm, "avx512f+avx512bw", &["vpmovdb"]);
    // AVX-512 gather used by the 16-way gather kernels.
    let asm_g = emit_asm(
        "let idx = _mm512_loadu_si512(p as *const __m512i);\
         let g = _mm512_i32gather_epi32(idx, p, 4);\
         let g = core::hint::black_box(g);\
         let lo = _mm512_extracti64x4_epi64(g, 0);\
         let hi = _mm512_extracti64x4_epi64(g, 1);\
         let mut acc: i32 = n as i32;\
         acc = acc.wrapping_add(_mm256_extract_epi32(lo, 0));\
         acc = acc.wrapping_add(_mm256_extract_epi32(lo, 4));\
         acc = acc.wrapping_add(_mm256_extract_epi32(hi, 2));\
         acc = acc.wrapping_add(_mm256_extract_epi32(hi, 6));\
         acc",
        "avx512f,avx512bw",
    );
    assert_mnemonics(&asm_g, "avx512f+avx512bw", &["vpgatherdd"]);
}

#[test]
fn court_avx512_intrinsics_require_crate_level_cfg_gate() {
    // rustc 1.96 does **not** reject feature-gated intrinsics at compile
    // time: compiled without the AVX-512 target features, the intrinsic
    // still compiles and the instruction is still emitted (which would
    // SIGILL on CPUs lacking AVX-512).  This is exactly why the crate's
    // `avx512.rs` module is gated with `#![cfg(target_feature =
    // "avx512bw")]`: portable builds must never *contain* AVX-512 code, and
    // the compiler cannot be relied on to enforce that — the cfg gate is
    // load-bearing.  This court documents the toolchain reality and then
    // verifies the crate's defensive gate exists.
    let src = "use std::arch::x86_64::*;\n\
               #[inline(never)]\n\
               #[no_mangle]\n\
               pub unsafe extern \"C\" fn kernel(p: *const i32, n: usize) -> i32 {\n\
               \x20   let v = _mm512_loadu_si512(p as *const __m512i);\n\
               \x20   let b = _mm512_cvtepi32_epi8(v);\n\
               \x20   _mm_cvtsi128_si32(b) + (n as i32)\n\
               }\n";
    let dir = std::env::temp_dir().join(format!("ryg-disasm-nogate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let file = dir.join("nogate.rs");
    std::fs::write(&file, src).expect("write source");
    let out = Command::new("rustc")
        .current_dir(&dir)
        .args([
            "--crate-type=lib",
            "--emit=asm",
            "-C",
            "opt-level=3",
            "-o",
            "-",
        ])
        .arg(&file)
        .output()
        .expect("run rustc");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "rustc accepted the intrinsic without features on this toolchain; this court documents that acceptance"
    );
    let asm = String::from_utf8_lossy(&out.stdout).to_lowercase();
    assert!(
        asm.contains("vpmovdb"),
        "the AVX-512 instruction is emitted even without the target-feature flag"
    );

    // The crate's defense: the avx512 module must be excluded from portable
    // builds at the module level.
    let avx512_src =
        std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/avx512.rs"))
            .expect("read avx512.rs");
    assert!(
        avx512_src.contains("#![cfg(target_feature = \"avx512bw\")]"),
        "avx512.rs must carry the module-level cfg gate"
    );
}
