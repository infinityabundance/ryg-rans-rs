//! # Allocation-instrumented model-cache measurement (Phase O.15)
//!
//! `cargo run -p ryg-rans-rs-bench --bin model_cache_alloc`
//!
//! Measures the *memory* effects of the model cache across the workload
//! classes, using a counting global allocator:
//!
//! ```text
//! disabled  cold  warm  hot-set  thrash  unique
//! ```
//!
//! for block sizes `4 KiB`, `16 KiB`, `64 KiB`, `256 KiB` at 8 workers.
//!
//! ## What is measured
//!
//! Per case, between two allocator snapshots around one decode of the
//! whole case:
//!
//! * allocations / allocated bytes / deallocations / deallocated bytes,
//! * cache metrics: builds started/completed (the instrumented packed-table
//!   construction count — every build constructs the packed table exactly
//!   once), hits, insertions, evictions, current/peak entries and bytes.
//!
//! ## Instrumentation honesty
//!
//! The packed-table construction count comes from the cache's instrumented
//! `builds_started` delta, never from inference about the data.  The
//! allocator counter wraps `std::alloc::System` and is exact for this
//! binary's process; it counts every allocation the whole process makes
//! (including the measurement scaffolding), so the *deltas* around the
//! timed decode are the meaningful numbers — identical scaffolding on both
//! sides cancels.
//!
//! ## What is NOT measured
//!
//! Not wall-clock time (that is the Criterion bench's job), and not cache
//! lookup timing (the `cache-timing` feature + the contention harness).
//!
//! The allocator is deliberately confined to this binary: ordinary library
//! consumers never link a diagnostic allocator.

use ryg_rans_rs_bench::common::cache_workload::{self, CacheMode};
use ryg_rans_rs_parallel::ParallelDecoder;
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

/// Counting allocator: exact allocation accounting for this process.
struct CountingAllocator {
    allocations: AtomicU64,
    allocated_bytes: AtomicU64,
    deallocations: AtomicU64,
    deallocated_bytes: AtomicU64,
}

impl CountingAllocator {
    const fn new() -> Self {
        Self {
            allocations: AtomicU64::new(0),
            allocated_bytes: AtomicU64::new(0),
            deallocations: AtomicU64::new(0),
            deallocated_bytes: AtomicU64::new(0),
        }
    }
}

// SAFETY: `System` is a sound global allocator; the counters are relaxed
// atomic accumulators that never affect the allocation result.  The
// allocator is only ever installed in this measurement binary (never in
// library consumers).
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            self.allocations.fetch_add(1, Ordering::Relaxed);
            self.allocated_bytes
                .fetch_add(layout.size() as u64, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        self.deallocations.fetch_add(1, Ordering::Relaxed);
        self.deallocated_bytes
            .fetch_add(layout.size() as u64, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let np = unsafe { System.realloc(ptr, layout, new_size) };
        if !np.is_null() && new_size > layout.size() {
            self.allocated_bytes
                .fetch_add((new_size - layout.size()) as u64, Ordering::Relaxed);
        }
        np
    }
}

#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();

#[derive(Debug, Clone, Copy, Default)]
struct AllocSnapshot {
    allocations: u64,
    allocated_bytes: u64,
    deallocations: u64,
    deallocated_bytes: u64,
}

fn alloc_snapshot() -> AllocSnapshot {
    AllocSnapshot {
        allocations: ALLOC.allocations.load(Ordering::Relaxed),
        allocated_bytes: ALLOC.allocated_bytes.load(Ordering::Relaxed),
        deallocations: ALLOC.deallocations.load(Ordering::Relaxed),
        deallocated_bytes: ALLOC.deallocated_bytes.load(Ordering::Relaxed),
    }
}

fn main() {
    let workers = 8usize;
    let sizes: [usize; 4] = [4096, 16384, 65536, 262144];
    let modes = [
        CacheMode::Disabled,
        CacheMode::Cold,
        CacheMode::Warm,
        CacheMode::HotSet,
        CacheMode::Thrash,
        CacheMode::Unique,
    ];

    println!(
        "{:<10} {:<9} {:<12} {:<15} {:<12} {:<18} {:<6} {:<6} {:<8} {:<8} {:<12} {:<12}",
        "mode",
        "size",
        "allocations",
        "allocated_bytes",
        "deallocs",
        "deallocated_bytes",
        "builds",
        "hits",
        "inserts",
        "evicts",
        "peak_entries",
        "peak_bytes"
    );

    for &mode in &modes {
        for &size in &sizes {
            let bc = cache_workload::blocks_per_case(size, mode);
            let (jobs, source) = match mode {
                CacheMode::Disabled | CacheMode::Cold | CacheMode::Warm => {
                    cache_workload::encode_case(size, bc, |_| Some(0))
                }
                CacheMode::HotSet => {
                    cache_workload::encode_case(size, bc, |i| Some((i % 16) as u32))
                }
                CacheMode::Thrash => {
                    cache_workload::encode_case(size, bc, |i| Some((i % 17) as u32))
                }
                CacheMode::Unique => {
                    cache_workload::encode_case(size, bc, |i| Some((i % 256) as u32))
                }
            };
            let source_sha = {
                use sha2::Digest;
                let mut h = sha2::Sha256::new();
                h.update(&source);
                format!("{:x}", h.finalize())
            };

            let cache = cache_workload::cache_for(mode);
            if mode == CacheMode::Warm {
                // Prewarm with the blocks' actual shared model (same route
                // as the bench's warm preflight).
                let first = &jobs[0].block_data;
                let model = first[104..104 + 1024].to_vec();
                cache
                    .get_or_build(8, 12, &model, None, || {
                        ryg_rans_rs_parallel::build_validated_model_artifacts(8, 12, &model)
                    })
                    .expect("prewarm");
            }
            let pre = cache.metrics();
            let a0 = alloc_snapshot();
            let decoder = ParallelDecoder::with_model_cache(
                cache_workload::config_for(workers),
                cache.clone(),
            );
            let decoded = decoder.decode_blocks(jobs).expect("decode");
            let a1 = alloc_snapshot();
            let post = cache.metrics();

            // Byte-exact verification before reporting (never report a
            // measurement whose decode was wrong).
            let mut out = Vec::with_capacity(source.len());
            for b in &decoded.blocks {
                out.extend_from_slice(&b.output);
            }
            {
                use sha2::Digest;
                let mut h = sha2::Sha256::new();
                h.update(&out);
                let out_sha = format!("{:x}", h.finalize());
                assert_eq!(
                    out_sha,
                    source_sha,
                    "decode output mismatch for {} {}",
                    mode.name(),
                    size
                );
            }

            let builds = post.builds_started - pre.builds_started;
            let hits = post.hits - pre.hits;
            let inserts = post.insertions - pre.insertions;
            let evicts = post.entry_evictions - pre.entry_evictions;
            println!(
                "{:<10} {:<9} {:<12} {:<15} {:<12} {:<18} {:<6} {:<6} {:<8} {:<8} {:<12} {:<12}",
                mode.name(),
                size,
                a1.allocations - a0.allocations,
                a1.allocated_bytes - a0.allocated_bytes,
                a1.deallocations - a0.deallocations,
                a1.deallocated_bytes - a0.deallocated_bytes,
                builds,
                hits,
                inserts,
                evicts,
                post.peak_entries,
                post.peak_bytes,
            );
        }
    }
    println!(
        "allocator totals: {} allocations, {} bytes allocated, {} deallocations, {} bytes deallocated",
        ALLOC.allocations.load(Ordering::Relaxed),
        ALLOC.allocated_bytes.load(Ordering::Relaxed),
        ALLOC.deallocations.load(Ordering::Relaxed),
        ALLOC.deallocated_bytes.load(Ordering::Relaxed),
    );
}
