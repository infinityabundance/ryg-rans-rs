//! Fuzz target: parallel reorder buffer determinism
//!
//! Verifies that the ReorderBuffer (Phase L.5 atomic-commit API) correctly
//! handles out-of-order, duplicate, and stale insertions and only ever
//! commits strictly ascending block indices.  This target must never panic,
//! and every committed batch must be strictly ascending (the L.5 invariant:
//! insertion returns everything newly committable in ascending order).

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_parallel::{BufferSized, HasBlockIndex, ReorderBuffer};

#[derive(Debug, Clone)]
struct TestBlock {
    index: u64,
}

impl HasBlockIndex for TestBlock {
    fn block_index(&self) -> u64 {
        self.index
    }
}

impl BufferSized for TestBlock {
    fn buffer_size(&self) -> u64 {
        8
    }
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 2 {
        return;
    }

    let num_blocks = (data.len() / 2).min(100).max(1);
    let mut buf = ReorderBuffer::<TestBlock>::new(num_blocks.max(16), 1 << 20);

    // Insert blocks in the order determined by the fuzzer input.  Each
    // input byte pair selects a block index (duplicates and out-of-order
    // indices are intentionally possible).
    for i in 0..num_blocks {
        let idx = (data[i * 2] as u64) % (num_blocks as u64);
        match buf.insert(TestBlock { index: idx }) {
            Ok(committed) => {
                // Phase L.5 invariant: committed batches are strictly
                // ascending.
                let mut prev: Option<u64> = None;
                for b in &committed {
                    if let Some(p) = prev {
                        assert!(
                            b.index > p,
                            "commit batch must be strictly ascending, got {} after {}",
                            b.index,
                            p
                        );
                    }
                    prev = Some(b.index);
                }
            }
            Err(_) => {
                // Duplicate, stale index, or resource limit — typed
                // rejection, never a panic.
            }
        }
    }

    // Drain any remaining tail; it must also be ascending.
    let tail = buf.drain_ready();
    let mut prev: Option<u64> = None;
    for b in &tail {
        if let Some(p) = prev {
            assert!(
                b.index > p,
                "drain tail must be strictly ascending, got {} after {}",
                b.index,
                p
            );
        }
        prev = Some(b.index);
    }
});
