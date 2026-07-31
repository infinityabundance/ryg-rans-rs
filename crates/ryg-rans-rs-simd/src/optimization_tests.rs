//! # Optimization backend tests

use crate::avx512::{
    DecodeJob, decode_batch_interleaved16_avx512, decode_interleaved8_manual_gather_into,
    decode_interleaved8_manual_gather_kernel, decode_interleaved16_2x8_into,
    decode_interleaved16_2x8_kernel, decode_interleaved16_manual_gather_into,
    decode_interleaved16_manual_gather_kernel,
};
use crate::model_kernels::{
    decode_interleaved16_uniform256_avx512, decode_interleaved16_uniform256_avx512_into,
};
use crate::packed_table::{PackedWordTable, decode_interleaved16_scalar, encode_interleaved16};
use alloc::vec;
use alloc::vec::Vec;

fn umodel() -> (Vec<u32>, Vec<u32>) {
    let t: u32 = 1 << 12;
    let b = t / 256;
    let mut f = vec![b; 256];
    f[255] += t - f.iter().sum::<u32>();
    let mut c = vec![0u32; 257];
    for i in 0..256 {
        c[i + 1] = c[i] + f[i];
    }
    (f, c)
}
fn inp(f: &[u32], len: usize) -> Vec<u8> {
    let t: u64 = 1 << 12;
    let ns = f.iter().filter(|&&x| x > 0).count().max(1);
    let mut cum = vec![0u32; f.len() + 1];
    for i in 0..f.len() {
        cum[i + 1] = cum[i] + f[i];
    }
    let mut r: u64 = 42;
    (0..len)
        .map(|_| {
            r = r
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let tr = r % t;
            for s in 0..ns {
                if (tr as u32) < cum[s + 1] {
                    return s as u8;
                }
            }
            (ns - 1) as u8
        })
        .collect()
}
fn ce16(i: &[u8], f: &[u32], c: &[u32]) -> Vec<u16> {
    encode_interleaved16(i, f, c, 12).unwrap()
}

#[test]
fn mg8_rt() {
    if !cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw"
    )) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65] {
        let i = inp(&f, sz);
        let cp = crate::encode_8way_for_test(&i, &f, &c);
        unsafe {
            let (o, _) = decode_interleaved8_manual_gather_kernel(&cp, &pk, i.len()).unwrap();
            assert_eq!(o, i, "mg8 size={}", sz);
        }
    }
}

#[test]
fn mg16_rt() {
    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65] {
        let i = inp(&f, sz);
        let cp = ce16(&i, &f, &c);
        let (ro, rr) = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap();
        unsafe {
            let (o, r) = decode_interleaved16_manual_gather_kernel(&cp, &pk, i.len()).unwrap();
            assert_eq!(o, ro, "mg16 out sz={}", sz);
            assert_eq!(r.words_consumed, rr.words_consumed, "mg16 wc sz={}", sz);
            assert_eq!(r.final_states, rr.final_states, "mg16 st sz={}", sz);
        }
    }
}

#[test]
fn x2x8_rt() {
    if !cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw"
    )) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65] {
        let i = inp(&f, sz);
        let cp = ce16(&i, &f, &c);
        let (ro, rr) = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap();
        unsafe {
            let (o, r) = decode_interleaved16_2x8_kernel(&cp, &pk, i.len()).unwrap();
            assert_eq!(o, ro, "2x8 out sz={}", sz);
            assert_eq!(r.words_consumed, rr.words_consumed, "2x8 wc sz={}", sz);
            assert_eq!(r.final_states, rr.final_states, "2x8 st sz={}", sz);
        }
    }
}

#[test]
fn tf16_rt() {
    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[0, 1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65] {
        let i = inp(&f, sz);
        let cp = ce16(&i, &f, &c);
        let (ro, rr) = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap();
        unsafe {
            let (o, r) = decode_interleaved16_uniform256_avx512(&cp, i.len()).unwrap();
            assert_eq!(o, ro, "tf out sz={}", sz);
            assert_eq!(r.words_consumed, rr.words_consumed, "tf wc sz={}", sz);
            assert_eq!(r.final_states, rr.final_states, "tf st sz={}", sz);
        }
    }
}

// ---- Batched decode: multi-job and multi-length tests ----

#[test]
fn batch1_rt() {
    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[0, 1, 7, 16, 32, 64, 128] {
        let i = inp(&f, sz);
        let cp = ce16(&i, &f, &c);
        let ro = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap().0;
        let mut out = vec![0u8; sz];
        let mut jobs = vec![DecodeJob {
            compressed: &cp,
            table: &pk,
            output: &mut out,
        }];
        unsafe {
            decode_batch_interleaved16_avx512(&mut jobs).unwrap();
        }
        assert_eq!(out, ro, "batch1 size={}", sz);
    }
}

#[test]
fn batch_multi_job() {
    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &num_jobs in &[2, 4, 5, 7, 9] {
        for &sz in &[0, 1, 7, 15, 16, 17, 31, 33, 64, 257] {
            // First, build all compressed streams and expected outputs
            let mut comp_vecs: Vec<Vec<u16>> = Vec::new();
            let mut exp_vecs: Vec<Vec<u8>> = Vec::new();
            let mut out_vecs: Vec<Vec<u8>> = Vec::new();

            for _j in 0..num_jobs {
                let i = inp(&f, sz);
                let cp = ce16(&i, &f, &c);
                let ro = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap().0;
                exp_vecs.push(ro);
                comp_vecs.push(cp);
                out_vecs.push(vec![0xFFu8; sz]);
            }

            // Now create jobs that borrow from the vectors
            // Safety: each job accesses disjoint output, never aliased
            let mut job_refs: Vec<DecodeJob> = Vec::new();
            for j in 0..num_jobs {
                let out_ptr = out_vecs[j].as_mut_ptr();
                let out_len = out_vecs[j].len();
                let out_slice = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_len) };
                job_refs.push(DecodeJob {
                    compressed: &comp_vecs[j],
                    table: &pk,
                    output: out_slice,
                });
            }

            unsafe {
                decode_batch_interleaved16_avx512(&mut job_refs).unwrap();
            }

            for j in 0..num_jobs {
                assert_eq!(
                    out_vecs[j], exp_vecs[j],
                    "batch {} jobs, size {}, job {}",
                    num_jobs, sz, j
                );
            }
        }
    }
}

// ---- _into API tests: verify output buffer is actually written ----

#[test]
fn into_mg8_writes_output() {
    if !cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw"
    )) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[1, 7, 8, 15, 16, 31, 64] {
        let i = inp(&f, sz);
        let cp = crate::encode_8way_for_test(&i, &f, &c);
        unsafe {
            let (expected, _) =
                decode_interleaved8_manual_gather_kernel(&cp, &pk, i.len()).unwrap();
            let mut poisoned = vec![0xFFu8; sz];
            let _report = decode_interleaved8_manual_gather_into(&cp, &pk, &mut poisoned).unwrap();
            assert_eq!(poisoned, expected, "into_mg8 content sz={}", sz);
            assert_eq!(poisoned, i, "into_mg8 correct sz={}", sz);
        }
    }
}

#[test]
fn into_mg16_writes_output() {
    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[1, 7, 8, 15, 16, 31, 64] {
        let i = inp(&f, sz);
        let cp = ce16(&i, &f, &c);
        let (expected, _) = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap();
        unsafe {
            let mut poisoned = vec![0xFFu8; sz];
            let _report = decode_interleaved16_manual_gather_into(&cp, &pk, &mut poisoned).unwrap();
            assert_eq!(poisoned, expected, "into_mg16 content sz={}", sz);
        }
    }
}

#[test]
fn into_2x8_writes_output() {
    if !cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512vl",
        target_feature = "avx512bw"
    )) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[1, 7, 8, 15, 16, 31, 64] {
        let i = inp(&f, sz);
        let cp = ce16(&i, &f, &c);
        let (expected, _) = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap();
        unsafe {
            let mut poisoned = vec![0xFFu8; sz];
            let _report = decode_interleaved16_2x8_into(&cp, &pk, &mut poisoned).unwrap();
            assert_eq!(poisoned, expected, "into_2x8 content sz={}", sz);
        }
    }
}

#[test]
fn into_tf16_writes_output() {
    if !cfg!(all(target_feature = "avx512f", target_feature = "avx512bw")) {
        return;
    }
    let (f, c) = umodel();
    let pk = PackedWordTable::from_freqs(&f, &c, 12).unwrap();
    for &sz in &[0, 1, 7, 8, 15, 16, 31, 64] {
        let i = inp(&f, sz);
        let cp = ce16(&i, &f, &c);
        let (expected, _) = decode_interleaved16_scalar(&cp, &pk, i.len()).unwrap();
        unsafe {
            let mut poisoned = vec![0xFFu8; sz];
            let _report = decode_interleaved16_uniform256_avx512_into(&cp, &mut poisoned).unwrap();
            assert_eq!(poisoned, expected, "into_tf16 content sz={}", sz);
        }
    }
}
