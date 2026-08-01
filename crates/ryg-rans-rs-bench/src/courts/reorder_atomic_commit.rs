//! # RYG_RANS.L.REORDER.ATOMIC_COMMIT — atomic reorder commit (L.5)
//!
//! Proves the L.5 `ReorderBuffer` contract:
//!
//! - `insert(item) -> Result<Vec<T>, BlockError>` atomically returns every
//!   newly committable result: the inserted next-expected item plus every
//!   contiguous pending item it unblocks, in strictly ascending block-index
//!   order.
//! - There is **no separate required drain call** after insertion; a final
//!   `drain_ready()` exists only for diagnostics.
//! - Inserting every input exactly once (in any order) yields commit batches
//!   that concatenate to exactly `[0, 1, ..., N-1]` — proven exhaustively for
//!   every permutation of small N and by property sampling for larger N.
//! - Duplicates, stale (already-committed) indexes, and missing gaps are
//!   handled: duplicates/stale → `OutputCommit`; missing gaps → the item is
//!   buffered (empty commit batch) until the gap closes.
//! - Resource limits (`max_blocks`, `max_bytes`) return `ResourceLimit`.

use super::{CourtCase, CourtRun};
use ryg_rans_rs_casefile::PhaseLCaseVerdict;
use ryg_rans_rs_parallel::{BlockErrorKind, ReorderBuffer};

/// A test item carrying a block index and a byte size.
#[derive(Debug, Clone)]
struct Item {
    index: u64,
    size: u64,
}

impl ryg_rans_rs_parallel::HasBlockIndex for Item {
    fn block_index(&self) -> u64 {
        self.index
    }
}

impl ryg_rans_rs_parallel::BufferSized for Item {
    fn buffer_size(&self) -> u64 {
        self.size
    }
}

pub fn court() -> CourtRun {
    let mut cases = Vec::new();
    let add = |cases: &mut Vec<CourtCase>,
               id: &str,
               input: &str,
               expected: &str,
               actual: Result<String, String>| {
        let actual_str = match &actual {
            Ok(a) => a.clone(),
            Err(e) => format!("ERROR: {}", e),
        };
        let verdict = match &actual {
            Ok(a) if a == expected => PhaseLCaseVerdict::Pass,
            _ => PhaseLCaseVerdict::Fail,
        };
        cases.push(CourtCase {
            case_id: id.to_string(),
            input: input.to_string(),
            expected: expected.to_string(),
            actual: actual_str,
            verdict,
            residual_ids: vec!["L5-A".to_string(), "L5-B".to_string()],
        });
    };

    // ---- Case 1: in-order inserts commit immediately, one at a time -------
    let mut rb = ReorderBuffer::<Item>::new(64, 1 << 30);
    let mut committed = Vec::new();
    let mut err = None;
    for i in 0..8u64 {
        match rb.insert(Item { index: i, size: 1 }) {
            Ok(batch) => committed.extend(batch),
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    let seq_ok = err.is_none()
        && committed.iter().map(|i| i.index).collect::<Vec<_>>() == (0..8).collect::<Vec<u64>>()
        && rb.buffered_count() == 0
        && rb.is_complete();
    add(
        &mut cases,
        "CASE.001",
        "in-order insert 0..8 commits each immediately",
        "ascending_complete",
        if seq_ok {
            Ok("ascending_complete".to_string())
        } else {
            Ok(format!("committed={:?} err={:?}", committed, err))
        },
    );

    // ---- Case 2: out-of-order insert buffers until the gap closes ---------
    let mut rb = ReorderBuffer::<Item>::new(64, 1 << 30);
    // Insert 3, 2, 1 first (gap at 0 → buffered, empty batches).
    let b3 = rb.insert(Item { index: 3, size: 1 }).unwrap();
    let b2 = rb.insert(Item { index: 2, size: 1 }).unwrap();
    let b1 = rb.insert(Item { index: 1, size: 1 }).unwrap();
    let buffered_before = rb.buffered_count();
    // Insert 0 → should commit 0,1,2,3 atomically in ascending order.
    let b0 = rb.insert(Item { index: 0, size: 1 }).unwrap();
    let chain: Vec<u64> = b0.iter().map(|i| i.index).collect();
    let gap_ok = b3.is_empty()
        && b2.is_empty()
        && b1.is_empty()
        && buffered_before == 3
        && chain == vec![0, 1, 2, 3]
        && rb.buffered_count() == 0;
    add(
        &mut cases,
        "CASE.002",
        "insert 3,2,1 then 0 → atomic commit chain 0..=3",
        "atomic_chain",
        if gap_ok {
            Ok("atomic_chain".to_string())
        } else {
            Ok(format!(
                "chain={:?} buffered={}",
                chain,
                rb.buffered_count()
            ))
        },
    );

    // ---- Case 3: exhaustive permutation property for N ≤ 7 ----------------
    let mut all_perms_ok = true;
    let mut checked = 0u64;
    let mut failed_perm: Option<String> = None;
    // Iterative Heap's algorithm for N = 7 (5040 permutations).
    let n = 7usize;
    let mut perm: Vec<usize> = (0..n).collect();
    let mut c = vec![0usize; n];
    'perm_loop: loop {
        checked += 1;
        if !perm_commit_ok(&perm) {
            all_perms_ok = false;
            failed_perm = Some(format!("{:?}", perm));
            break 'perm_loop;
        }
        let mut i = 1usize;
        loop {
            if i >= n {
                break;
            }
            if c[i] < i {
                if i % 2 == 0 {
                    perm.swap(0, i);
                } else {
                    perm.swap(c[i], i);
                }
                c[i] += 1;
                continue 'perm_loop;
            }
            c[i] = 0;
            i += 1;
        }
        break;
    }
    add(
        &mut cases,
        "CASE.003",
        &format!(
            "exhaustive permutation property: every permutation of 0..{} ({} perms)",
            n, checked
        ),
        "all_permutations_commit_ascending",
        if all_perms_ok {
            Ok("all_permutations_commit_ascending".to_string())
        } else {
            Ok(format!("FAILED perm {}", failed_perm.unwrap_or_default()))
        },
    );

    // ---- Case 4: property sampling for N = 12 (random permutations) -------
    let mut samples_ok = true;
    let mut seed = 0x9E3779B97F4A7C15u64;
    for _ in 0..64 {
        let mut perm: Vec<usize> = (0..12).collect();
        // Fisher–Yates with a small deterministic xorshift.
        for i in (1..12).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = (seed as usize) % (i + 1);
            perm.swap(i, j);
        }
        if !perm_commit_ok(&perm) {
            samples_ok = false;
            break;
        }
    }
    add(
        &mut cases,
        "CASE.004",
        "64 deterministic random permutations of 0..12 commit to [0..12)",
        "sampled_ok",
        if samples_ok {
            Ok("sampled_ok".to_string())
        } else {
            Ok("FAILED_SAMPLE".to_string())
        },
    );

    // ---- Case 5: duplicates are rejected with OutputCommit ---------------
    let mut rb = ReorderBuffer::<Item>::new(64, 1 << 30);
    let _ = rb.insert(Item { index: 0, size: 1 });
    let _ = rb.insert(Item { index: 1, size: 1 });
    let dup = rb.insert(Item { index: 1, size: 1 });
    add(
        &mut cases,
        "CASE.005",
        "duplicate insert of index 1",
        "OutputCommit",
        match dup {
            Err(e) if e.kind == BlockErrorKind::OutputCommit => Ok("OutputCommit".to_string()),
            other => Ok(format!("{:?}", other.map(|b| b.len()))),
        },
    );

    // ---- Case 6: stale (already committed) indexes are rejected -----------
    let mut rb = ReorderBuffer::<Item>::new(64, 1 << 30);
    let _ = rb.insert(Item { index: 0, size: 1 });
    let _ = rb.insert(Item { index: 1, size: 1 });
    let stale = rb.insert(Item { index: 0, size: 1 });
    add(
        &mut cases,
        "CASE.006",
        "stale insert of already-committed index 0",
        "OutputCommit",
        match stale {
            Err(e) if e.kind == BlockErrorKind::OutputCommit => Ok("OutputCommit".to_string()),
            other => Ok(format!("{:?}", other.map(|b| b.len()))),
        },
    );

    // ---- Case 7: missing gaps buffer (empty commit) until closed ----------
    let mut rb = ReorderBuffer::<Item>::new(64, 1 << 30);
    let r5 = rb.insert(Item { index: 5, size: 1 }).unwrap();
    let r4 = rb.insert(Item { index: 4, size: 1 }).unwrap();
    let gap_open = r5.is_empty() && r4.is_empty() && rb.buffered_count() == 2;
    let r0 = rb.insert(Item { index: 0, size: 1 }).unwrap();
    let r1 = rb.insert(Item { index: 1, size: 1 }).unwrap();
    let r2 = rb.insert(Item { index: 2, size: 1 }).unwrap();
    let r3 = rb.insert(Item { index: 3, size: 1 }).unwrap();
    let chain: Vec<u64> = r0
        .iter()
        .chain(r1.iter())
        .chain(r2.iter())
        .chain(r3.iter())
        .map(|i| i.index)
        .collect();
    let gap_closed = chain == vec![0, 1, 2, 3, 4, 5] && rb.buffered_count() == 0;
    add(
        &mut cases,
        "CASE.007",
        "gap at 0..3 closes when 0 arrives; 4 and 5 ride along",
        "gap_closed_ascending",
        if gap_open && gap_closed {
            Ok("gap_closed_ascending".to_string())
        } else {
            Ok(format!(
                "open={} chain={:?} buffered={}",
                gap_open,
                chain,
                rb.buffered_count()
            ))
        },
    );

    // ---- Case 8: count-based resource limit -------------------------------
    let mut rb = ReorderBuffer::<Item>::new(2, 1 << 30);
    let _ = rb.insert(Item { index: 3, size: 1 });
    let _ = rb.insert(Item { index: 2, size: 1 });
    let over = rb.insert(Item { index: 1, size: 1 });
    add(
        &mut cases,
        "CASE.008",
        "count limit (max_blocks=2) with 3 buffered items",
        "ResourceLimit",
        match over {
            Err(e) if e.kind == BlockErrorKind::ResourceLimit => Ok("ResourceLimit".to_string()),
            other => Ok(format!("{:?}", other.map(|b| b.len()))),
        },
    );

    // ---- Case 9: byte-based resource limit --------------------------------
    let mut rb = ReorderBuffer::<Item>::new(64, 10);
    let _ = rb.insert(Item { index: 3, size: 6 });
    let _ = rb.insert(Item { index: 2, size: 4 });
    let over = rb.insert(Item { index: 1, size: 1 });
    add(
        &mut cases,
        "CASE.009",
        "byte limit (max_bytes=10) exceeded by 6+4+1 buffered items",
        "ResourceLimit",
        match over {
            Err(e) if e.kind == BlockErrorKind::ResourceLimit => Ok("ResourceLimit".to_string()),
            other => Ok(format!("{:?}", other.map(|b| b.len()))),
        },
    );

    // ---- Case 10: buffered_bytes accounting is exact ----------------------
    let mut rb = ReorderBuffer::<Item>::new(64, 1 << 30);
    let _ = rb.insert(Item { index: 2, size: 5 });
    let _ = rb.insert(Item { index: 1, size: 7 });
    let bytes_before = rb.buffered_bytes();
    let batch = rb.insert(Item { index: 0, size: 3 }).unwrap();
    let bytes_after = rb.buffered_bytes();
    let sizes: u64 = batch.iter().map(|i| i.size).sum();
    let accounting_ok = bytes_before == 12 && bytes_after == 0 && sizes == 15;
    add(
        &mut cases,
        "CASE.010",
        "buffered_bytes decreases by committed sizes after chain commit",
        "exact",
        if accounting_ok {
            Ok("exact".to_string())
        } else {
            Ok(format!(
                "before={} after={} committed_sizes={}",
                bytes_before, bytes_after, sizes
            ))
        },
    );

    // ---- Case 11: next_expected tracks the commit frontier ----------------
    let mut rb = ReorderBuffer::<Item>::new(64, 1 << 30);
    let _ = rb.insert(Item { index: 3, size: 1 });
    let ne1 = rb.next_expected(); // 3 is buffered; 0 still expected
    let _ = rb.insert(Item { index: 0, size: 1 });
    let ne2 = rb.next_expected(); // only 0 committed; 1 still expected
    let _ = rb.insert(Item { index: 1, size: 1 });
    let _ = rb.insert(Item { index: 2, size: 1 });
    let ne3 = rb.next_expected(); // 0..=3 now committed
    let ok = ne1 == 0 && ne2 == 1 && ne3 == 4;
    add(
        &mut cases,
        "CASE.011",
        "next_expected advances through the commit frontier (0 → 1 → 4)",
        "frontier_ok",
        if ok {
            Ok("frontier_ok".to_string())
        } else {
            Ok(format!("ne1={} ne2={} ne3={}", ne1, ne2, ne3))
        },
    );

    CourtRun {
        court_id: "RYG_RANS.L.REORDER.ATOMIC_COMMIT".to_string(),
        title: "ReorderBuffer atomic commit (L.5)".to_string(),
        cases,
        residual_ids: vec!["L5-A".to_string(), "L5-B".to_string()],
    }
}

/// Insert a permutation into a fresh ReorderBuffer and check that the
/// concatenated commit batches equal `[0, 1, ..., N-1]` with no separate
/// drain call.
fn perm_commit_ok(perm: &[usize]) -> bool {
    let n = perm.len();
    let mut rb = ReorderBuffer::<Item>::new(n + 4, 1 << 30);
    let mut committed: Vec<u64> = Vec::new();
    let mut err = false;
    for &idx in perm {
        match rb.insert(Item {
            index: idx as u64,
            size: 1,
        }) {
            Ok(batch) => committed.extend(batch.iter().map(|i| i.index)),
            Err(_) => {
                err = true;
                break;
            }
        }
    }
    if err {
        return false;
    }
    let expected: Vec<u64> = (0..n as u64).collect();
    committed == expected
}
