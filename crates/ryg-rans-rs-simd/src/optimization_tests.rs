//! # Optimization backend tests

use crate::avx512::{
    decode_interleaved8_manual_gather_kernel,
    decode_interleaved16_2x8_kernel,
    decode_interleaved16_manual_gather_kernel,
    DecodeJob, decode_batch_interleaved16_avx512,
};
use crate::model_kernels::decode_interleaved16_uniform256_avx512;
use crate::packed_table::{
    decode_interleaved16_scalar, encode_interleaved16, PackedWordTable,
};
use alloc::vec;
use alloc::vec::Vec;

fn umodel() -> (Vec<u32>, Vec<u32>) {
    let t: u32 = 1 << 12; let b = t / 256;
    let mut f = vec![b; 256]; f[255] += t - f.iter().sum::<u32>();
    let mut c = vec![0u32; 257];
    for i in 0..256 { c[i+1] = c[i] + f[i]; } (f, c)
}
fn inp(f: &[u32], len: usize) -> Vec<u8> {
    let t: u64 = 1 << 12; let ns = f.iter().filter(|&&x|x>0).count().max(1);
    let mut cum = vec![0u32; f.len()+1];
    for i in 0..f.len() { cum[i+1] = cum[i] + f[i]; }
    let mut r: u64 = 42;
    (0..len).map(|_| {
        r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let tr = r % t;
        for s in 0..ns { if (tr as u32) < cum[s+1] { return s as u8; } }
        (ns-1) as u8
    }).collect()
}
fn ce16(i: &[u8], f: &[u32], c: &[u32]) -> Vec<u16> {
    encode_interleaved16(i, f, c, 12).unwrap()
}

#[test]
fn mg8_rt() {
    if !cfg!(all(target_feature = "avx512f",target_feature = "avx512vl",target_feature = "avx512bw")) { return; }
    let (f,c) = umodel(); let pk = PackedWordTable::from_freqs(&f,&c,12).unwrap();
    for &sz in &[1,7,8,9,15,16,17,31,32,33,63,64,65] {
        let i = inp(&f,sz); let cp = crate::encode_8way_for_test(&i,&f,&c);
        unsafe { let (o,_) = decode_interleaved8_manual_gather_kernel(&cp,&pk,i.len()).unwrap();
        assert_eq!(o,i,"mg8 size={}",sz); }
    }
}

#[test]
fn mg16_rt() {
    if !cfg!(all(target_feature = "avx512f",target_feature = "avx512bw")) { return; }
    let (f,c) = umodel(); let pk = PackedWordTable::from_freqs(&f,&c,12).unwrap();
    for &sz in &[1,7,8,9,15,16,17,31,32,33,63,64,65] {
        let i = inp(&f,sz); let cp = ce16(&i,&f,&c);
        let (ro,rr) = decode_interleaved16_scalar(&cp,&pk,i.len()).unwrap();
        unsafe { let (o,r) = decode_interleaved16_manual_gather_kernel(&cp,&pk,i.len()).unwrap();
        assert_eq!(o,ro,"mg16 out sz={}",sz);
        assert_eq!(r.words_consumed,rr.words_consumed,"mg16 wc sz={}",sz);
        assert_eq!(r.final_states,rr.final_states,"mg16 st sz={}",sz); }
    }
}

#[test]
fn x2x8_rt() {
    if !cfg!(all(target_feature = "avx512f",target_feature = "avx512vl",target_feature = "avx512bw")) { return; }
    let (f,c) = umodel(); let pk = PackedWordTable::from_freqs(&f,&c,12).unwrap();
    for &sz in &[1,7,8,9,15,16,17,31,32,33,63,64,65] {
        let i = inp(&f,sz); let cp = ce16(&i,&f,&c);
        let (ro,rr) = decode_interleaved16_scalar(&cp,&pk,i.len()).unwrap();
        unsafe { let (o,r) = decode_interleaved16_2x8_kernel(&cp,&pk,i.len()).unwrap();
        assert_eq!(o,ro,"2x8 out sz={}",sz);
        assert_eq!(r.words_consumed,rr.words_consumed,"2x8 wc sz={}",sz);
        assert_eq!(r.final_states,rr.final_states,"2x8 st sz={}",sz); }
    }
}

#[test]
fn tf16_rt() {
    if !cfg!(all(target_feature = "avx512f",target_feature = "avx512bw")) { return; }
    let (f,c) = umodel(); let pk = PackedWordTable::from_freqs(&f,&c,12).unwrap();
    for &sz in &[0,1,7,8,9,15,16,17,31,32,33,63,64,65] {
        let i = inp(&f,sz); let cp = ce16(&i,&f,&c);
        let (ro,rr) = decode_interleaved16_scalar(&cp,&pk,i.len()).unwrap();
        unsafe { let (o,r) = decode_interleaved16_uniform256_avx512(&cp,i.len()).unwrap();
        assert_eq!(o,ro,"tf out sz={}",sz);
        assert_eq!(r.words_consumed,rr.words_consumed,"tf wc sz={}",sz);
        assert_eq!(r.final_states,rr.final_states,"tf st sz={}",sz); }
    }
}

#[test]
fn batch1_rt() {
    if !cfg!(all(target_feature = "avx512f",target_feature = "avx512bw")) { return; }
    let (f,c) = umodel(); let pk = PackedWordTable::from_freqs(&f,&c,12).unwrap();
    for &sz in &[0,1,7,16,32,64,128] {
        let i = inp(&f,sz); let cp = ce16(&i,&f,&c);
        let ro = decode_interleaved16_scalar(&cp,&pk,i.len()).unwrap().0;
        let mut out = vec![0u8;sz];
        let mut jobs = vec![DecodeJob{compressed:&cp,table:&pk,output:&mut out}];
        unsafe { decode_batch_interleaved16_avx512(&mut jobs).unwrap(); }
        assert_eq!(out,ro,"batch1 size={}",sz);
    }
}
