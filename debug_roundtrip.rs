//! Debug test for roundtrip

use ryg_rans_rs_parallel::config::CodecPolicy;
use ryg_rans_rs_parallel::encode::{encode_single_block, sha256};
use ryg_rans_rs_parallel::job::EncodeBlockJob;

fn main() {
    // Create uniform256 data: 256 symbols × 16 copies = 4096 bytes
    let mut data = Vec::with_capacity(4096);
    for s in 0u8..=255 {
        for _ in 0..16 {
            data.push(s);
        }
    }
    println!("Data len: {}", data.len());

    let job = EncodeBlockJob::new(
        0,
        data.clone(),
        CodecPolicy::Auto,
        ryg_rans_rs_parallel::config::ModelPolicy::PerBlock,
        12,
    );
    let result = encode_single_block(job).expect("encode");
    println!("Block size: {}", result.block.len());
    println!("Input length: {}", result.input_length);
    println!("Payload hash: {:02x?}", result.payload_hash);
    println!("Decoded hash: {:02x?}", result.decoded_hash);
    println!("Data hash: {:02x?}", sha256(&data));

    // Try to decode
    let block = &result.block;
    println!("Block first 40 bytes: {:02x?}", &block[..40]);
    println!("Header size: {:02x?}", &block[4..6]);
    let ul = u32::from_le_bytes(block[24..28].try_into().unwrap());
    let pl = u32::from_le_bytes(block[28..32].try_into().unwrap());
    let ml = u32::from_le_bytes(block[32..36].try_into().unwrap());
    println!("Uncompressed len: {}", ul);
    println!("Payload len: {}", pl);
    println!("Model len: {}", ml);

    let payload_start = 104usize + ml as usize;
    let payload_end = payload_start + pl as usize;
    let payload = &block[payload_start..payload_end];
    println!(
        "Payload bytes: {} (first 10: {:02x?})",
        payload.len(),
        &payload[..10.min(payload.len())]
    );

    // Convert to u16 words
    let words: Vec<u16> = payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    println!(
        "Words: {} (first 5: {:?})",
        words.len(),
        &words[..5.min(words.len())]
    );

    // Try manual decode
    let mut st = [0u32; 16];
    for i in 0..16 {
        st[i] = words[i * 2] as u32 | (words[i * 2 + 1] as u32) << 16;
    }
    println!("Initial states: {:?}", &st[..3]);

    let mut rp = 32usize;
    let mut out = vec![0u8; data.len()];
    let mut i = 0;
    while i < out.len() {
        let lane = i & 15;
        let x = st[lane];
        let slot = x as usize & 4095;
        out[i] = (slot as u32 / 16) as u8;
        let nx = 16 * (x >> 12) + (slot as u32 & 15);
        st[lane] = nx;
        if nx < (1u32 << 16) {
            if rp >= words.len() {
                println!(
                    "OOB at i={}, lane={}, rp={}, words={}",
                    i,
                    lane,
                    rp,
                    words.len()
                );
                break;
            }
            st[lane] = (nx << 16) | words[rp] as u32;
            rp += 1;
        }
        i += 1;
    }
    println!(
        "Decoded {} bytes, rp={}, match={}",
        i,
        rp,
        out[..i] == data[..i]
    );
    if out != data {
        for j in 0..data.len().min(out.len()) {
            if out[j] != data[j] {
                println!(
                    "First mismatch at byte {}: expected {} got {}",
                    j, data[j], out[j]
                );
                break;
            }
        }
        println!("out len: {}, data len: {}", out.len(), data.len());
    }
}
