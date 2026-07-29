use ryg_rans_rs_parallel::{
    CodecPolicy, DecodeBlockJob, EncodeBlockJob, FixedBlockPlan, ModelPolicy, ParallelConfig,
    ParallelDecoder, ParallelEncoder, ThreadCount,
};
use std::num::NonZeroUsize;

fn uniform256() -> Vec<u8> {
    let mut d = Vec::with_capacity(4096);
    for s in 0u8..=255 {
        for _ in 0..16 {
            d.push(s);
        }
    }
    d
}

#[test]
fn test_single_roundtrip() {
    let data = uniform256();
    eprintln!("Data len: {}", data.len());
    eprintln!("Data hash: {:02x?}", {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&data);
        let r = h.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&r);
        out
    });

    let job = EncodeBlockJob::new(
        0,
        data.clone(),
        CodecPolicy::Auto,
        ModelPolicy::PerBlock,
        12,
    );
    let enc =
        ParallelEncoder::encode_blocks(vec![job], &ParallelConfig::default()).expect("encode");
    eprintln!("Block len: {}", enc.blocks[0].block.len());
    eprintln!("Input len: {}", enc.blocks[0].input_length);

    // Manually decode to see what's happening
    let block = &enc.blocks[0].block;
    let ul = u32::from_le_bytes(block[24..28].try_into().unwrap());
    let pl = u32::from_le_bytes(block[28..32].try_into().unwrap());
    let ml = u32::from_le_bytes(block[32..36].try_into().unwrap());
    eprintln!("Header: ul={} pl={} ml={}", ul, pl, ml);

    let payload = &block[104 + ml as usize..104 + ml as usize + pl as usize];
    let words: Vec<u16> = payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    eprintln!("Words: {}", words.len());

    // Check initial states
    let mut st = [0u32; 16];
    for i in 0..16 {
        st[i] = words[i * 2] as u32 | (words[i * 2 + 1] as u32) << 16;
    }
    eprintln!(
        "First 3 init states: {:08x} {:08x} {:08x}",
        st[0], st[1], st[2]
    );

    // Decode one iteration (16 symbols) manually
    let mut out = Vec::new();
    let mut rp = 32usize;
    for iter in 0..5 {
        for lane in 0..16 {
            let x = st[lane];
            let slot = x as usize & 4095;
            let sym = (slot as u32 / 16) as u8;
            out.push(sym);
            let nx = 16 * (x >> 12) + (slot as u32 & 15);
            st[lane] = nx;
            if nx < (1u32 << 16) {
                if rp >= words.len() {
                    eprintln!(
                        "OOB at iter={} lane={} rp={} words={}",
                        iter,
                        lane,
                        rp,
                        words.len()
                    );
                    break;
                }
                st[lane] = (nx << 16) | words[rp] as u32;
                rp += 1;
            }
        }
    }
    eprintln!("Decoded first 80: {:?}", &out[..80.min(out.len())]);
    eprintln!("Expected first 80: {:?}", &data[..80.min(data.len())]);

    // Use the library decoder
    let dj = DecodeBlockJob {
        block_index: 0,
        block_data: enc.blocks[0].block.clone(),
    };
    let dec = ParallelDecoder::decode_blocks(vec![dj], &ParallelConfig::default()).expect("decode");
    assert_eq!(dec.blocks[0].output, data, "decoded output mismatch");
}
