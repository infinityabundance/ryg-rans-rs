//! # Property tests: ReorderBuffer atomic commit
//!
//! The `insert -> Result<Vec<T>>` contract: inserting items in **any**
//! order must commit exactly `[0, 1, ..., N-1]` in strictly ascending
//! order, each item exactly once, and every committed chain must be
//! contiguous.  The exhaustive N <= 9 permutation test lives in
//! `reorder.rs`; this file adds random larger permutations via proptest.

use proptest::prelude::*;
use ryg_rans_rs_parallel::{BufferSized, HasBlockIndex, ReorderBuffer};

/// A `HasBlockIndex + BufferSized` item for the property tests.
struct Item(u64);

impl HasBlockIndex for Item {
    fn block_index(&self) -> u64 {
        self.0
    }
}
impl BufferSized for Item {
    fn buffer_size(&self) -> u64 {
        8
    }
}

/// Insert a permutation of `0..n` in the given order; collect committed
/// batches; assert the concatenation is exactly `0..n` ascending.
fn run_permutation(order: &[u64]) {
    let n = order.len() as u64;
    let mut buf = ReorderBuffer::<Item>::new(4096, 1 << 20);
    let mut committed: Vec<u64> = Vec::new();
    for &idx in order {
        assert!(idx < n, "permutation element in range");
        let batch = buf.insert(Item(idx)).expect("insert must succeed");
        // Every batch must be strictly ascending and contiguous.
        for w in batch.windows(2) {
            assert_eq!(
                w[1].block_index(),
                w[0].block_index() + 1,
                "contiguous ascending"
            );
        }
        committed.extend(batch.iter().map(|i| i.block_index()));
    }
    assert_eq!(
        committed,
        (0..n).collect::<Vec<u64>>(),
        "concatenated commit batches equal [0..n)"
    );
}

proptest! {
    /// Any permutation of `0..n` for n in 1..=24 commits exactly `[0..n)`.
    /// The permutation is a deterministic shuffle seeded by proptest.
    #[test]
    fn reorder_any_permutation_commits_exactly(n in 1usize..=24, seed in any::<u64>()) {
        let mut order: Vec<u64> = (0..n as u64).collect();
        let mut x = seed;
        for i in (1..order.len()).rev() {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1);
            let j = (x % (i as u64 + 1)) as usize;
            order.swap(i, j);
        }
        run_permutation(&order);
    }
}

/// Random insertion orders with duplicate/stale-index rejection: inserting
/// an index twice must not corrupt the commit sequence.
#[test]
fn reorder_duplicate_insert_is_rejected_or_benign() {
    let mut buf = ReorderBuffer::<Item>::new(4096, 1 << 20);
    let mut committed: Vec<u64> = Vec::new();
    // Insert 0, then 0 again: the duplicate must be a typed error (stale
    // index), never a panic or a wrong commit.
    committed.extend(
        buf.insert(Item(0))
            .unwrap()
            .into_iter()
            .map(|i| i.block_index()),
    );
    assert!(buf.insert(Item(0)).is_err(), "duplicate must be rejected");
    committed.extend(
        buf.insert(Item(1))
            .unwrap()
            .into_iter()
            .map(|i| i.block_index()),
    );
    committed.extend(
        buf.insert(Item(2))
            .unwrap()
            .into_iter()
            .map(|i| i.block_index()),
    );
    assert_eq!(
        committed,
        vec![0, 1, 2],
        "commit sequence unaffected by duplicate"
    );
}
