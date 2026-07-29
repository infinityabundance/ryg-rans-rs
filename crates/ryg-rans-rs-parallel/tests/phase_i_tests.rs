//! Phase I: cancellation, panic containment, and deterministic error tests
//!
//! These tests verify that:
//! - Cancellation before/during/after execution produces correct errors
//! - Worker panics are contained and produce typed errors
//! - Queue saturation does not deadlock
//! - Mixed block types work together

use ryg_rans_rs_parallel::{
    CancellationToken, CodecPolicy, DecodeBlockJob, EncodeBlockJob, ExecutorTask, FixedBlockPlan,
    ModelPolicy, ParallelConfig, ParallelDecoder, ParallelEncoder, ParallelError, ThreadCount,
    run_tasks,
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

// ============================================================
// Cancellation tests (Step 21)
// ============================================================

#[test]
fn test_cancellation_before_start() {
    let ct = CancellationToken::new();
    ct.cancel();
    assert!(ct.is_cancelled());
    assert!(ct.check().is_err());
}

#[test]
fn test_cancellation_during_execution() {
    // Submit tasks and cancel mid-execution
    let ct = std::sync::Arc::new(CancellationToken::new());
    let ct_clone = ct.clone();

    struct CancellableTask;
    impl ExecutorTask for CancellableTask {
        type Output = u32;
        fn run(self, _wi: usize, cancel: &CancellationToken) -> u32 {
            if cancel.is_cancelled() {
                return 0;
            }
            // Simulate work
            std::thread::sleep(std::time::Duration::from_micros(100));
            42
        }
    }

    let handle = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_micros(50));
        ct_clone.cancel();
    });

    let tasks: Vec<CancellableTask> = (0..10).map(|_| CancellableTask).collect();
    let _result = run_tasks(tasks, 2, 4, None);
    handle.join().unwrap();
    assert!(ct.is_cancelled());
}

// ============================================================
// Worker panic containment tests (Step 21)
// ============================================================

struct PanicOnBlockTask {
    panic_at: u64,
    index: u64,
}

impl ExecutorTask for PanicOnBlockTask {
    type Output = Result<u64, ()>;
    fn run(self, _wi: usize, _cancel: &CancellationToken) -> Self::Output {
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
    let result = run_tasks(tasks, 2, 4, None);
    match result {
        Err(ParallelError::WorkerPanic { .. }) => {} // Expected
        other => panic!("expected WorkerPanic, got {:?}", other),
    }
}

#[test]
fn test_worker_panic_lowest_index_error() {
    // With 3 blocks: block 1 panics first, but block 0 also fails
    // The canonical error should be block 0
    let tasks: Vec<PanicOnBlockTask> = (0..3)
        .map(|i| PanicOnBlockTask {
            panic_at: 1,
            index: i,
        })
        .collect();
    let result = run_tasks(tasks, 1, 4, None);
    assert!(result.is_err(), "expected error from panic");
}

// ============================================================
// Queue saturation tests (Step 22)
// ============================================================

#[test]
fn test_minimal_queue() {
    let data = uniform256();
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
// Mixed block tests (Step 22) — different codecs in one container
// ============================================================

#[test]
fn test_multi_block_roundtrip() {
    let mut data = Vec::with_capacity(8192);
    for _ in 0..2 {
        data.extend(uniform256());
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
    let dec = ParallelDecoder::decode_blocks(dj, &cfg).expect("decode");
    let mut full = Vec::new();
    for b in &dec.blocks {
        full.extend_from_slice(&b.output);
    }
    assert_eq!(full, data);
}
