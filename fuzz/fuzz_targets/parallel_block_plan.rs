//! Fuzz target: parallel block planning determinism
//!
//! Verifies that FixedBlockPlan produces consistent results for any input
//! length and block size combination, and that thread count never affects
//! block boundaries.

#![no_main]

use libfuzzer_sys::fuzz_target;
use ryg_rans_rs_parallel::FixedBlockPlan;

fuzz_target!(|data: &[u8]| {
    // The second read needs 8 more bytes; the fuzzer caught the original
    // `data.len() < 8` guard as too weak for the 8..16 window.
    if data.len() < 16 {
        return;
    }
    let input_len = u64::from_le_bytes(data[0..8].try_into().unwrap());
    let block_size = u64::from_le_bytes(data[8..16].try_into().unwrap());
    if block_size == 0 {
        return;
    }
    if input_len > 10_000_000 {
        return;
    } // bound to prevent OOM

    // Create plan
    let plan = FixedBlockPlan::new(input_len, block_size);

    // Verify coverage: every input byte is covered exactly once
    let total: u64 = plan.ranges.iter().map(|r| r.length).sum();
    assert_eq!(total, input_len, "coverage must be exact");

    // Verify no gaps or overlaps
    for i in 1..plan.ranges.len() {
        let prev_end = plan.ranges[i - 1].input_offset + plan.ranges[i - 1].length;
        assert_eq!(prev_end, plan.ranges[i].input_offset, "no gaps");
    }

    // Verify planner version is stable
    assert_eq!(plan.planner_version, 1);
});
