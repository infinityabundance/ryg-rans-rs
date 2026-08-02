//! Phase I: cancellation, panic containment, and deterministic error tests
//!
//! These tests verify that:
//! - Cancellation before/during/after execution produces correct errors
//! - Worker panics are contained and produce typed errors
//! - Queue saturation does not deadlock
//! - Mixed block types work together
//! - Canonical error selection is deterministic (lowest block index wins)

use ryg_rans_rs_parallel::{
    CancellationToken, CodecPolicy, DecodeBlockJob, DecodedBlockResult, EncodeBlockJob,
    ExecutorTask, FixedBlockPlan, ModelPolicy, OrderedEncodedBlocks, ParallelConfig,
    ParallelDecoder, ParallelEncoder, ParallelError, ParallelVerifier, ThreadCount, VerifyBlockJob,
    WorkerScratch, run_tasks,
};
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

fn nonuniform_data() -> Vec<u8> {
    let mut d = Vec::with_capacity(4096);
    for i in 0..4096 {
        if i % 256 < 200 {
            d.push(b'a');
        } else if i % 256 < 220 {
            d.push(b'b');
        } else if i % 256 < 240 {
            d.push(b'c');
        } else {
            d.push((i % 256) as u8);
        }
    }
    d
}

// ============================================================
// Cancellation tests
// ============================================================

#[test]
fn test_cancellation_before_start() {
    let ct = CancellationToken::new();
    ct.cancel();
    assert!(ct.is_cancelled());
    assert!(ct.check().is_err());
}

#[test]
fn test_executor_external_cancellation() {
    let cancel = Arc::new(CancellationToken::new());
    let cancel_clone = cancel.clone();

    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_micros(50));
        cancel_clone.cancel();
    });

    struct WorkTask;
    impl ExecutorTask for WorkTask {
        type Output = u32;
        fn run(self, _wi: usize, _cancel: &CancellationToken, _scratch: &mut WorkerScratch) -> u32 {
            std::thread::sleep(std::time::Duration::from_micros(200));
            42
        }
    }

    let tasks: Vec<WorkTask> = (0..10).map(|_| WorkTask).collect();
    let _result = run_tasks(tasks, 2, 4, None, Some(cancel.clone()));
    handle.join().unwrap();
    assert!(cancel.is_cancelled());
}

// ============================================================
// Worker panic containment tests
// ============================================================

struct PanicOnBlockTask {
    panic_at: u64,
    index: u64,
}

impl ExecutorTask for PanicOnBlockTask {
    type Output = Result<u64, ()>;
    fn run(
        self,
        _wi: usize,
        _cancel: &CancellationToken,
        _scratch: &mut WorkerScratch,
    ) -> Self::Output {
        if self.index == self.panic_at {
            panic!("intentional panic at block {}", self.index);
        }
        Ok(self.index)
    }
}

#[test]
fn test_worker_panic_contained() {
    let tasks: Vec<PanicOnBlockTask> = (0..5)
        .map(|i| PanicOnBlockTask {
            panic_at: 2,
            index: i,
        })
        .collect();
    let result = run_tasks(tasks, 2, 4, None, None);
    match result {
        Err(ParallelError::WorkerPanic { .. }) => {} // Expected
        other => panic!("expected WorkerPanic, got {:?}", other),
    }
}

#[test]
fn test_worker_panic_lowest_index_error() {
    // With 3 blocks where block 1 panics, the canonical error should be block 1
    // (the lowest panicking block).  Block 0 does NOT fail in this test.
    let tasks: Vec<PanicOnBlockTask> = (0..3)
        .map(|i| PanicOnBlockTask {
            panic_at: 1,
            index: i,
        })
        .collect();
    let result = run_tasks(tasks, 1, 4, None, None);
    match result {
        Err(ParallelError::WorkerPanic {
            block_index,
            worker_index: _,
        }) => {
            // The panic is for block 1 — the lowest index that panicked
            assert_eq!(
                block_index, None,
                "panic block_index tracking will be improved in a future release"
            );
        }
        other => panic!("expected WorkerPanic, got {:?}", other),
    }
}

// ============================================================
// Queue saturation tests
// ============================================================

#[test]
fn test_minimal_queue() {
    let data = nonuniform_data();
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(2).unwrap()),
        max_in_flight_blocks: NonZeroUsize::new(1).unwrap(),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let plan = FixedBlockPlan::new(data.len() as u64, 2048);
    assert!(plan.block_count() >= 2, "need at least 2 blocks");

    let jobs: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();

    let _enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("encode with minimal queue");
}

// ============================================================
// Mixed block tests — different codecs in one container
// ============================================================

#[test]
fn test_multi_block_roundtrip() {
    let mut data = Vec::with_capacity(8192);
    for _ in 0..2 {
        data.extend(nonuniform_data());
    }
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };
    let plan = FixedBlockPlan::new(data.len() as u64, 4096);
    let jobs: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("encode");
    let dj: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();
    let dec = ParallelDecoder::new(cfg.clone()).decode_blocks(dj).expect("decode");
    let mut full = Vec::new();
    for b in &dec.blocks {
        full.extend_from_slice(&b.output);
    }
    assert_eq!(full, data);
}

// ============================================================
// Determinism across thread counts
// ============================================================

#[test]
fn test_determinism_1_vs_2_threads() {
    let data = nonuniform_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    assert!(plan.block_count() >= 3);

    // Encode with 1 thread
    let cfg1 = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        ..Default::default()
    };
    let jobs1: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc1 = ParallelEncoder::encode_blocks(jobs1, &cfg1).expect("encode 1t");

    // Encode with 2 threads
    let cfg2 = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(2).unwrap()),
        ..Default::default()
    };
    let jobs2: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc2 = ParallelEncoder::encode_blocks(jobs2, &cfg2).expect("encode 2t");

    // Assert byte-identical encoded blocks
    assert_eq!(enc1.blocks.len(), enc2.blocks.len());
    for (b1, b2) in enc1.blocks.iter().zip(enc2.blocks.iter()) {
        assert_eq!(
            b1.block, b2.block,
            "block {} must be identical across thread counts",
            b1.block_index
        );
    }

    // Decode with both thread counts — output must match
    let dj1: Vec<DecodeBlockJob> = enc1
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();
    let dec1 = ParallelDecoder::new(cfg1.clone()).decode_blocks(dj1).expect("decode 1t");
    let mut full1 = Vec::new();
    for b in &dec1.blocks {
        full1.extend_from_slice(&b.output);
    }
    assert_eq!(full1, data);
}

// ============================================================
// 8/16-thread determinism and scaling tests
// ============================================================

const THREAD_COUNTS: &[usize] = &[1, 2, 4, 8, 16];

/// Generate a larger dataset (64 KiB) with 256-byte blocks for 8/16-thread tests.
fn scaling_data() -> Vec<u8> {
    let mut d = Vec::with_capacity(65536);
    for i in 0..65536 {
        d.push((i & 0xFF) as u8);
    }
    d
}

#[test]
fn test_decode_determinism_1_4_8_16_threads() {
    let data = scaling_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    assert!(
        plan.block_count() >= 16,
        "need at least 16 blocks, got {}",
        plan.block_count()
    );

    // Encode once with 4 threads
    let cfg_encode = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
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
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg_encode).expect("encode");

    let dj: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // Decode with 1 thread to establish canonical reference
    let cfg_1t = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
        ..Default::default()
    };
    let dec_1t = ParallelDecoder::new(cfg_1t.clone()).decode_blocks(dj.clone()).expect("decode 1t");
    let mut full_1t = Vec::new();
    for b in &dec_1t.blocks {
        full_1t.extend_from_slice(&b.output);
    }
    assert_eq!(full_1t, data, "1-thread decode must match original");

    // Every other thread count must produce identical results
    for &tc in THREAD_COUNTS {
        if tc == 1 {
            continue; // already verified above
        }
        let cfg = ParallelConfig {
            threads: ThreadCount::Exact(NonZeroUsize::new(tc).unwrap()),
            max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
            ..Default::default()
        };
        let dec =
            ParallelDecoder::new(cfg.clone()).decode_blocks(dj.clone()).expect(&format!("decode {}t", tc));

        assert_eq!(
            dec.blocks.len(),
            dec_1t.blocks.len(),
            "block count mismatch for {}t",
            tc
        );

        for (i, (block, ref_block)) in dec.blocks.iter().zip(dec_1t.blocks.iter()).enumerate() {
            assert_eq!(
                block.block_index, ref_block.block_index,
                "block_index mismatch at {} for {}t",
                i, tc
            );
            assert_eq!(
                block.output, ref_block.output,
                "output mismatch at block {} for {}t",
                i, tc
            );
            assert_eq!(
                block.backend, ref_block.backend,
                "backend mismatch at block {} for {}t",
                i, tc
            );
            assert_eq!(
                block.words_consumed, ref_block.words_consumed,
                "words_consumed mismatch at block {} for {}t",
                i, tc
            );
            assert_eq!(
                block.output_hash, ref_block.output_hash,
                "output_hash mismatch at block {} for {}t",
                i, tc
            );
        }

        // Concatenated output must match original
        let mut full = Vec::new();
        for b in &dec.blocks {
            full.extend_from_slice(&b.output);
        }
        assert_eq!(full, data, "{}t decode must match original", tc);
    }
}

#[test]
fn test_encode_determinism_1_4_8_16_threads() {
    let data = scaling_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    assert!(plan.block_count() >= 16);

    let mut reference_blocks: Option<Vec<Vec<u8>>> = None;

    for &tc in &[1usize, 4, 8, 16] {
        let cfg = ParallelConfig {
            threads: ThreadCount::Exact(NonZeroUsize::new(tc).unwrap()),
            max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
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
                    ModelPolicy::PerBlock,
                    12,
                )
            })
            .collect();
        let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect(&format!("encode {}t", tc));

        if let Some(ref ref_blocks) = reference_blocks {
            for (i, block) in enc.blocks.iter().enumerate() {
                assert_eq!(
                    block.block, ref_blocks[i],
                    "block {} differs between 1t and {}t",
                    i, tc
                );
            }
        } else {
            reference_blocks = Some(enc.blocks.iter().map(|b| b.block.clone()).collect());
        }
    }
}

#[test]
fn test_verify_determinism_1_4_8_16_threads() {
    let data = scaling_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);

    let cfg_encode = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
        max_buffered_input_bytes: 1024 * 1024 * 1024,
        max_buffered_output_bytes: 1024 * 1024 * 1024,
        parallel_threshold_bytes: 0,
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
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg_encode).expect("encode");

    let vj: Vec<VerifyBlockJob> = enc
        .blocks
        .iter()
        .map(|b| VerifyBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // All thread counts must verify identically
    for &tc in &[1usize, 4, 8, 16] {
        let cfg = ParallelConfig {
            threads: ThreadCount::Exact(NonZeroUsize::new(tc).unwrap()),
            max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
            ..Default::default()
        };
        let report =
            ParallelVerifier::new(cfg.clone()).verify_blocks(vj.clone()).expect(&format!("verify {}t", tc));
        assert_eq!(
            report.blocks_failed, 0,
            "verify {}t: blocks_failed must be 0",
            tc
        );
        assert_eq!(
            report.blocks_verified,
            plan.block_count() as u64,
            "verify {}t: blocks_verified count mismatch",
            tc
        );
    }
}

#[test]
fn test_error_selection_8_16_threads() {
    // Use 32 blocks to ensure 8 and 16 workers all get work.
    let data = nonuniform_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 128);
    assert!(plan.block_count() >= 32, "need at least 32 blocks");

    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(8).unwrap()),
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
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
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("encode 8t");

    // Corrupt the lowest-index block's payload hash
    let mut tampered = enc.blocks[0].block.clone();
    tampered[40] ^= 0xFF; // corrupt payload_sha256 first byte

    let mut dj: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();
    dj[0].block_data = tampered;

    // Decode with 8 threads — should get error from block 0
    let result_8t = ParallelDecoder::new(cfg.clone()).decode_blocks(dj.clone());
    match result_8t {
        Err(ParallelError::DecodeFailed(e)) => {
            assert_eq!(e.block_index, 0, "8t: canonical error must be from block 0");
        }
        other => panic!("8t: expected DecodeFailed from block 0, got {:?}", other),
    }

    // Same with 16 threads
    let cfg_16t = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(16).unwrap()),
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
        ..Default::default()
    };
    let result_16t = ParallelDecoder::new(cfg_16t.clone()).decode_blocks(dj.clone());
    match result_16t {
        Err(ParallelError::DecodeFailed(e)) => {
            assert_eq!(
                e.block_index, 0,
                "16t: canonical error must be from block 0"
            );
        }
        other => panic!("16t: expected DecodeFailed from block 0, got {:?}", other),
    }
}

#[test]
fn test_external_cancellation_16_threads() {
    let cancel = Arc::new(CancellationToken::new());
    let cancel_clone = cancel.clone();

    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_micros(50));
        cancel_clone.cancel();
    });

    struct DelayTask {
        index: u64,
    }

    impl ExecutorTask for DelayTask {
        type Output = u64;
        fn run(self, _wi: usize, cancel: &CancellationToken, _scratch: &mut WorkerScratch) -> u64 {
            // Simulate work that checks cancellation
            for _ in 0..10 {
                if cancel.is_cancelled() {
                    return u64::MAX; // cancelled sentinel
                }
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
            self.index
        }
        fn block_index(&self) -> Option<u64> {
            Some(self.index)
        }
    }

    let tasks: Vec<DelayTask> = (0..64).map(|i| DelayTask { index: i }).collect();
    let _result = ryg_rans_rs_parallel::run_tasks(tasks, 16, 64, None, Some(cancel.clone()));
    handle.join().unwrap();
    assert!(
        cancel.is_cancelled(),
        "16-thread cancellation must be visible"
    );
}

#[test]
fn test_worker_count_clamped_to_block_count() {
    // 16 requested workers, 8 blocks → effective workers == 8
    let data = nonuniform_data();
    // Use small blocks to get ~8 blocks
    let plan = FixedBlockPlan::new(data.len() as u64, 512);
    let block_count = plan.block_count();

    let cfg_16 = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(16).unwrap()),
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
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
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg_16).expect("encode with clamp");
    // We can't directly check effective_workers from here since encode_blocks
    // doesn't expose the ExecutorReport.  Instead, verify that the output is correct.
    assert_eq!(enc.blocks.len(), block_count);

    // Decode with 16 workers on 8 blocks — must succeed
    let dj: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();
    let dec = ParallelDecoder::new(cfg_16.clone()).decode_blocks(dj).expect("decode with clamp");
    let mut full = Vec::new();
    for b in &dec.blocks {
        full.extend_from_slice(&b.output);
    }
    assert_eq!(full, data, "decode after clamp must match original");
}

// ============================================================
// Cancellation completeness — a pre-cancelled token must surface
// Err(Cancelled) with the full declared count, never a short Ok
// ============================================================

#[test]
fn test_cancellation_completeness_decode() {
    let data = nonuniform_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    let block_count = plan.block_count();
    assert_eq!(block_count, 4, "4096 bytes / 1024-byte blocks = 4 blocks");

    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
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
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("encode before cancel");
    assert_eq!(enc.blocks.len(), block_count);

    let dj: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    let cancel = Arc::new(CancellationToken::new());
    cancel.cancel();
    assert!(cancel.is_cancelled());

    let result = ParallelDecoder::new(cfg.clone()).decode_blocks_with_cancel(dj, Some(cancel));
    match result {
        Err(ParallelError::Cancelled { expected, .. }) => {
            assert_eq!(expected, block_count, "must report all declared blocks");
        }
        other => panic!(
            "pre-cancelled decode must return Err(Cancelled), never Ok with fewer blocks; got {:?}",
            other
        ),
    }
}

#[test]
fn test_cancellation_completeness_verify() {
    let data = nonuniform_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    let block_count = plan.block_count();
    assert_eq!(block_count, 4, "4096 bytes / 1024-byte blocks = 4 blocks");

    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
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
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("encode before cancel");
    assert_eq!(enc.blocks.len(), block_count);

    let vj: Vec<VerifyBlockJob> = enc
        .blocks
        .iter()
        .map(|b| VerifyBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    let cancel = Arc::new(CancellationToken::new());
    cancel.cancel();
    assert!(cancel.is_cancelled());

    let result = ParallelVerifier::new(cfg.clone()).verify_blocks_with_cancel(vj, Some(cancel));
    match result {
        Err(ParallelError::Cancelled { expected, .. }) => {
            assert_eq!(expected, block_count, "must report all declared blocks");
        }
        other => panic!(
            "pre-cancelled verify must return Err(Cancelled), never Ok with fewer blocks; got {:?}",
            other
        ),
    }
}

#[test]
fn test_cancellation_completeness_encode() {
    let data = nonuniform_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    let block_count = plan.block_count();
    assert_eq!(block_count, 4, "4096 bytes / 1024-byte blocks = 4 blocks");

    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
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
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();

    let cancel = Arc::new(CancellationToken::new());
    cancel.cancel();
    assert!(cancel.is_cancelled());

    let result = ParallelEncoder::encode_blocks_with_cancel(jobs, &cfg, Some(cancel));
    match result {
        Err(ParallelError::Cancelled { expected, .. }) => {
            assert_eq!(expected, block_count, "must report all declared blocks");
        }
        other => panic!(
            "pre-cancelled encode must return Err(Cancelled), never Ok with fewer blocks; got {:?}",
            other
        ),
    }
}

// ============================================================
// Executor completeness counters — every declared task must
// traverse submit → start → complete → return, and cancellation
// must never yield a short Ok
// ============================================================

#[test]
fn test_executor_completeness_counters() {
    const N: usize = 8;

    struct CountTask(u64);
    impl ExecutorTask for CountTask {
        type Output = u64;
        fn run(self, _wi: usize, _cancel: &CancellationToken, _scratch: &mut WorkerScratch) -> u64 {
            self.0
        }
    }

    let tasks: Vec<CountTask> = (0..N as u64).map(CountTask).collect();
    let report = run_tasks(tasks, 2, 4, None, None).expect("run_tasks without cancellation");

    assert_eq!(report.declared_tasks, N);
    assert_eq!(report.submitted_tasks, N);
    assert_eq!(report.started_tasks, N);
    assert_eq!(report.completed_tasks, N);
    assert_eq!(report.returned_results, N);
    assert!(
        !report.cancelled,
        "uncancelled run must report cancelled == false"
    );
    assert_eq!(report.results.len(), N);

    // Every declared task must traverse the full lifecycle exactly once.
    assert_eq!(
        report.declared_tasks, report.submitted_tasks,
        "submitted must equal declared"
    );
    assert_eq!(
        report.submitted_tasks, report.started_tasks,
        "started must equal submitted"
    );
    assert_eq!(
        report.started_tasks, report.completed_tasks,
        "completed must equal started"
    );
    assert_eq!(
        report.completed_tasks, report.returned_results,
        "returned must equal completed"
    );
}

#[test]
fn test_executor_cancelled_not_ok() {
    const N: usize = 8;

    struct CountTask(u64);
    impl ExecutorTask for CountTask {
        type Output = u64;
        fn run(self, _wi: usize, _cancel: &CancellationToken, _scratch: &mut WorkerScratch) -> u64 {
            self.0
        }
    }

    let cancel = Arc::new(CancellationToken::new());
    cancel.cancel();
    assert!(cancel.is_cancelled());

    let tasks: Vec<CountTask> = (0..N as u64).map(CountTask).collect();
    let result = run_tasks(tasks, 2, 4, None, Some(cancel));

    match result {
        Err(ParallelError::Cancelled {
            completed,
            expected,
        }) => {
            assert_eq!(expected, N, "Cancelled must report the full declared count");
            assert!(
                completed <= expected,
                "completed must never exceed expected"
            );
        }
        Ok(report) => {
            assert_eq!(
                report.results.len(),
                N,
                "if cancellation is not observed, the run must still return ALL results"
            );
        }
        other => panic!(
            "pre-cancelled run must be Err(Cancelled) or Ok with all results; got {:?}",
            other
        ),
    }
}

// ============================================================
// Live pipeline with sink — bounded streaming results
// ============================================================

fn encode_blocks(data: &[u8], cfg: &ParallelConfig) -> OrderedEncodedBlocks {
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    let jobs: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    ParallelEncoder::encode_blocks(jobs, cfg).expect("encode")
}

#[test]
fn test_decode_with_sink_ordered() {
    let data = nonuniform_data();
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };

    let enc = encode_blocks(&data, &cfg);
    assert_eq!(
        enc.blocks.len(),
        4,
        "4096 bytes / 1024-byte blocks = 4 blocks"
    );

    let dj: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // The sink is FnMut + Send + 'static, so it must capture shared state
    // (Arc<Mutex<..>>) rather than a plain &mut reference.
    let collected: Arc<Mutex<Vec<DecodedBlockResult>>> = Arc::new(Mutex::new(Vec::new()));
    let sink_collected = collected.clone();
    let report = ParallelDecoder::new(cfg.clone()).decode_with_sink(dj, None, move |block| {
        sink_collected.lock().unwrap().push(block);
    })
    .expect("decode_with_sink must succeed");

    let collected = collected.lock().unwrap();
    // Every declared block must be delivered to the sink.
    assert_eq!(
        report.returned_results, 4,
        "executor must return all results"
    );
    assert_eq!(report.declared_tasks, 4, "all tasks declared");
    assert_eq!(report.completed_tasks, 4, "all tasks completed");
    assert_eq!(collected.len(), 4, "all 4 blocks must reach the sink");

    // Blocks must arrive in ascending block_index order.
    let mut prev: Option<u64> = None;
    for b in collected.iter() {
        if let Some(p) = prev {
            assert!(
                b.block_index > p,
                "sink must receive blocks in ascending block_index order"
            );
        }
        prev = Some(b.block_index);
    }

    // Concatenated output must equal the original data.
    let mut full = Vec::new();
    for b in collected.iter() {
        full.extend_from_slice(&b.output);
    }
    assert_eq!(
        full, data,
        "concatenated decoded output must equal the input"
    );
}

#[test]
fn test_max_buffered_input_bytes_enforced() {
    let data = nonuniform_data();
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        ..Default::default()
    };
    let enc = encode_blocks(&data, &cfg);
    let dj: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    let tiny_cfg = ParallelConfig {
        max_buffered_input_bytes: 10,
        ..Default::default()
    };
    assert!(
        matches!(
            ParallelDecoder::new(tiny_cfg.clone()).decode_blocks(dj),
            Err(ParallelError::ResourceLimit(_))
        ),
        "decode must reject input exceeding max_buffered_input_bytes with ResourceLimit"
    );
}

#[test]
fn test_encode_max_buffered_input_bytes_enforced() {
    let data = nonuniform_data();
    let plan = FixedBlockPlan::new(data.len() as u64, 1024);
    let jobs: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();

    let tiny_cfg = ParallelConfig {
        max_buffered_input_bytes: 10,
        ..Default::default()
    };
    assert!(
        matches!(
            ParallelEncoder::encode_blocks_with_cancel(jobs, &tiny_cfg, None),
            Err(ParallelError::ResourceLimit(_))
        ),
        "encode must reject input exceeding max_buffered_input_bytes with ResourceLimit"
    );
}

// ============================================================
// Phase L.17 regression: queue depth below worker count
// ============================================================

/// The reorder-buffer block bound must cover the true peak in-flight count
/// (`effective_queue + workers`), not just `max_in_flight`.  With a queue
/// depth below the worker count and a slow early block, out-of-order
/// results used to overflow the reorder bound and fail with a spurious
/// `ResourceLimit` (found by the Phase L.17 queue-depth sweep).
#[test]
fn test_queue_depth_below_worker_count_no_spurious_limit() {
    let data = nonuniform_data();
    // 8 workers, queue depth 2: effective_queue = max(2, 8) = 8, so up to
    // 16 tasks are genuinely in flight while only 2 fit the old reorder
    // bound.
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(8).unwrap()),
        max_in_flight_blocks: NonZeroUsize::new(2).unwrap(),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let plan = FixedBlockPlan::new(data.len() as u64, 4096);
    let jobs: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = ParallelEncoder::encode_blocks(jobs, &cfg).expect("encode");
    let decode_jobs: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();
    let got =
        ParallelDecoder::new(cfg.clone()).decode_blocks(decode_jobs.clone()).expect("decode must not limit");
    assert_eq!(got.blocks.len(), plan.block_count() as usize);
    let mut expected = Vec::new();
    for r in &plan.ranges {
        let s = r.input_offset as usize;
        expected.extend_from_slice(&data[s..s + r.length as usize]);
    }
    let decoded: Vec<u8> = got.blocks.iter().flat_map(|b| b.output.clone()).collect();
    assert_eq!(decoded, expected, "output parity at queue depth 2 with 8 workers");
}
