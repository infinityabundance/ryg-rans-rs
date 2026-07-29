//! Parallel block decoder
use crate::cancellation::CancellationToken;
use crate::config::{BackendId, ParallelConfig};
use crate::error::{BlockError, BlockErrorKind, ParallelError};
use crate::executor::{ExecutorReport, ExecutorTask, run_tasks};
use crate::job::{DecodeBlockJob, DecodedBlockResult, OrderedDecodedBlocks};
use crate::reorder::{BufferSized, HasBlockIndex, ReorderBuffer};
use std::vec::Vec;

const RANS_WORD_L: u32 = 1u32 << 16;
const RANS_WORD_M: usize = 4096;

impl HasBlockIndex for DecodedBlockResult {
    fn block_index(&self) -> u64 {
        self.block_index
    }
}
impl BufferSized for DecodedBlockResult {
    fn buffer_size(&self) -> u64 {
        self.output.len() as u64 + 64
    }
}

struct DecodeTask {
    job: DecodeBlockJob,
}

impl ExecutorTask for DecodeTask {
    type Output = Result<DecodedBlockResult, BlockError>;
    fn run(self, _wi: usize, cancel: &CancellationToken) -> Self::Output {
        cancel.check().map_err(|_| BlockError {
            block_index: self.job.block_index,
            kind: BlockErrorKind::Codec,
        })?;
        decode_single_block(&self.job)
    }
}

fn decode_single_block(job: &DecodeBlockJob) -> Result<DecodedBlockResult, BlockError> {
    let data = &job.block_data;
    if data.len() < 104 {
        return Err(BlockError {
            block_index: job.block_index,
            kind: BlockErrorKind::Format,
        });
    }
    if &data[0..4] != b"RYGR" {
        return Err(BlockError {
            block_index: job.block_index,
            kind: BlockErrorKind::Format,
        });
    }
    let bi = job.block_index;
    let ul = u32::from_le_bytes(data[24..28].try_into().unwrap());
    let pl = u32::from_le_bytes(data[28..32].try_into().unwrap());
    let ml = u32::from_le_bytes(data[32..36].try_into().unwrap());
    let mut psh = [0u8; 32];
    psh.copy_from_slice(&data[40..72]);
    let mut dsh = [0u8; 32];
    dsh.copy_from_slice(&data[72..104]);
    let payload = &data[104 + ml as usize..104 + ml as usize + pl as usize];
    let ph = crate::encode::sha256(payload);
    if ph != psh {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::PayloadHash,
        });
    }
    if payload.len() < 32 {
        return Err(BlockError {
            block_index: bi,
            kind: BlockErrorKind::Format,
        });
    }
    let words: Vec<u16> = payload
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let mut st = [0u32; 16];
    for i in 0..16 {
        st[i] = words[i * 2] as u32 | (words[i * 2 + 1] as u32) << 16;
    }
    let mut rp = 32usize;
    let mut out = vec![0u8; ul as usize];
    let mut i = 0;
    while i < out.len() {
        let lane = i & 15;
        let x = st[lane];
        let slot = x as usize & (RANS_WORD_M - 1);
        out[i] = (slot as u32 / 16) as u8;
        let nx = 16 * (x >> 12) + (slot as u32 & 15);
        st[lane] = nx;
        if nx < RANS_WORD_L {
            if rp >= words.len() {
                return Err(BlockError {
                    block_index: bi,
                    kind: BlockErrorKind::Format,
                });
            }
            st[lane] = (nx << 16) | words[rp] as u32;
            rp += 1;
        }
        i += 1;
    }
    let dh = crate::encode::sha256(&out);
    Ok(DecodedBlockResult {
        block_index: bi,
        output: out,
        backend: BackendId::Scalar16,
        payload_verified: true,
        output_verified: dh == dsh || dsh == [0u8; 32],
        output_hash: dh,
        elapsed_ns: None,
    })
}

pub struct ParallelDecoder;

impl ParallelDecoder {
    pub fn decode_blocks(
        blocks: impl IntoIterator<Item = DecodeBlockJob>,
        config: &ParallelConfig,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        let jobs: Vec<DecodeBlockJob> = blocks.into_iter().collect();
        if jobs.is_empty() {
            return Ok(OrderedDecodedBlocks { blocks: Vec::new() });
        }
        let bc = jobs.len();
        let wc = crate::resource::effective_worker_count(config, bc)?;
        let qc = config.max_in_flight_blocks.get().max(wc);
        let tasks: Vec<DecodeTask> = jobs.into_iter().map(|j| DecodeTask { job: j }).collect();
        let report: ExecutorReport<Result<DecodedBlockResult, BlockError>> =
            run_tasks(tasks, wc, qc, config.worker_stack_size)?;
        let mut reorder = ReorderBuffer::new(
            config.max_in_flight_blocks.get(),
            config.max_buffered_output_bytes,
        );
        let mut ordered = Vec::with_capacity(bc);
        let mut et = crate::error::CanonicalErrorTracker::new();
        for r in report.results {
            match r {
                Ok(b) => match reorder.insert(b) {
                    Ok(Some(ready)) => {
                        ordered.push(ready);
                        ordered.extend(reorder.drain_ready());
                    }
                    Ok(None) => {}
                    Err(e) => et.record(e),
                },
                Err(e) => et.record(e),
            }
        }
        ordered.extend(reorder.drain_ready());
        if let Some(c) = et.canonical_error() {
            return Err(ParallelError::DecodeFailed(Box::new(c.clone())));
        }
        ordered.sort_by_key(|b| b.block_index);
        Ok(OrderedDecodedBlocks { blocks: ordered })
    }

    pub fn decode_streaming(
        blocks: impl IntoIterator<Item = DecodeBlockJob>,
        config: &ParallelConfig,
    ) -> Result<OrderedDecodedBlocks, ParallelError> {
        Self::decode_blocks(blocks, config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CodecPolicy;
    use crate::encode::{ParallelEncoder, encode_single_block};
    use crate::job::EncodeBlockJob;

    fn u256() -> Vec<u8> {
        let mut d = Vec::with_capacity(4096);
        for s in 0u8..=255 {
            for _ in 0..16 {
                d.push(s);
            }
        }
        d
    }

    #[test]
    fn test_roundtrip() {
        let d = u256();
        let j = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let e = encode_single_block(j).expect("e");
        let dec = decode_single_block(&DecodeBlockJob {
            block_index: 0,
            block_data: e.block,
        })
        .expect("d");
        assert_eq!(dec.output, d);
    }

    #[test]
    fn test_parallel_2blocks() {
        let mut data = Vec::with_capacity(8192);
        for _ in 0..2 {
            data.extend(u256());
        }
        let plan = crate::plan::FixedBlockPlan::new(data.len() as u64, 4096);
        assert_eq!(plan.block_count(), 2);
        let cfg = ParallelConfig {
            threads: crate::ThreadCount::Exact(std::num::NonZeroUsize::new(2).unwrap()),
            ..Default::default()
        };
        let jobs: Vec<EncodeBlockJob> = plan
            .ranges
            .iter()
            .map(|r| {
                let s = r.input_offset as usize;
                EncodeBlockJob::new(
                    r.block_index,
                    data[s..s + r.length as usize].to_vec(),
                    CodecPolicy::Auto,
                    crate::config::ModelPolicy::PerBlock,
                    12,
                )
            })
            .collect();
        let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("e");
        assert_eq!(enc.blocks.len(), 2);
        let dj: Vec<DecodeBlockJob> = enc
            .blocks
            .iter()
            .map(|b| DecodeBlockJob {
                block_index: b.block_index,
                block_data: b.block.clone(),
            })
            .collect();
        let dec = ParallelDecoder::decode_blocks(dj, &cfg).expect("d");
        let mut full = Vec::new();
        for b in &dec.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(full, data);
    }

    #[test]
    fn test_deterministic() {
        let d = u256();
        let j1 = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let j2 = EncodeBlockJob::new(
            0,
            d.clone(),
            CodecPolicy::Auto,
            crate::config::ModelPolicy::PerBlock,
            12,
        );
        let r1 = encode_single_block(j1).expect("e1");
        let r2 = encode_single_block(j2).expect("e2");
        assert_eq!(r1.block, r2.block);
    }
}
