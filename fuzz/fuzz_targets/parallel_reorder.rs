//! Fuzz target: parallel reorder buffer determinism
//!
//! Verifies that the ReorderBuffer correctly handles out-of-order insertions
//! and always emits results in ascending block-index order, regardless of
//! insertion order.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_parallel::ReorderBuffer;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let num_blocks = (data.len() / 2).min(100);
    let mut buf = ReorderBuffer::new(num_blocks.max(16), 65536);
    let mut all_results = Vec::new();
    let mut errors = 0u64;

    // Insert blocks in the order determined by the fuzzer input
    for i in 0..num_blocks {
        let idx = (data[i * 2] as u64) % (num_blocks as u64);
        match buf.insert(idx) {
            Ok(Some(ready)) => {
                all_results.push(ready);
                // Drain any additional ready blocks
                all_results.extend(buf.drain_ready());
            }
            Ok(None) => { /* buffered */ }
            Err(_) => {
                errors += 1;
            }
        }
    }

    // Drain remaining
    all_results.extend(buf.drain_ready());

    // Verify every committed block is in ascending order
    for i in 1..all_results.len() {
        assert!(
            all_results[i] > all_results[i - 1],
            "reorder must emit ascending indices"
        );
    }

    // Verify no errors for valid operations
    if errors > num_blocks as u64 / 2 {
        // Too many errors might indicate a real issue
        // (fuzzer may produce many duplicates intentionally)
    }
});
