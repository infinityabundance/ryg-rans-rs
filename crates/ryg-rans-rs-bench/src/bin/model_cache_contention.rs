//! # Cache contention measurement harness (Phase O.16)
//!
//! `cargo run -p ryg-rans-rs-bench --bin model_cache_contention`
//!
//! Measures the synchronization cost of the model cache across the worker
//! matrix using the `cache-timing` instrumentation built into
//! [`ryg_rans_rs_parallel::ModelArtifactCache`]:
//!
//! * `lock_wait_ns` — cumulative time blocked acquiring the cache-state
//!   mutex (the "lookup lock wait time"); if it grows with worker count the
//!   global mutex is the serialization point.
//! * `artifact_build_ns` — cumulative time inside builder closures (runs
//!   OUTSIDE the mutex by design, so this is not lock time).
//! * `single_flight_wait_ns` — cumulative time waiters spend on the
//!   condvar for an in-progress same-key build.
//! * `lookup_ns` — cumulative time across every `get_or_build` call (the
//!   caller-visible latency).
//! * `coalesced_waiters` — how many callers were deduplicated by
//!   single-flight.
//!
//! ## What the numbers prove
//!
//! A global lock is not automatically a defect; it becomes one only when
//! measurement shows material serialization (lock wait growing
//! superlinearly with workers, or lookup latency dominated by lock
//! waits).  This harness produces the evidence; the Phase O.19 report
//! draws the conclusion, and ADR-0017 records the eviction/synchronization
//! decision.
//!
//! The workload classes mirror the Criterion bench: cold (1 shared model),
//! hot-set (16 models, 64-slot cache), thrash (17 models, 16-slot cache),
//! unique (per-block models).  `coalesced_waiters` is scheduler-dependent
//! under concurrency and is reported as measured.

use ryg_rans_rs_bench::common::cache_workload::{self, CacheMode};
use ryg_rans_rs_parallel::ParallelDecoder;

fn main() {
    let workers: [usize; 6] = [1, 2, 4, 8, 16, 32];
    let modes = [
        CacheMode::Cold,
        CacheMode::HotSet,
        CacheMode::Thrash,
        CacheMode::Unique,
    ];
    let sizes: [usize; 2] = [4096, 262144];

    println!(
        "{:<8} {:<9} {:<10} {:<12} {:<12} {:<12} {:<14} {:<14} {:<14} {:<12}",
        "mode",
        "size",
        "workers",
        "lock_acquires",
        "lock_wait_ns",
        "build_ns",
        "sf_wait_ns",
        "lookup_ns",
        "lookups",
        "coalesced"
    );

    for &mode in &modes {
        for &size in &sizes {
            let bc = cache_workload::blocks_per_case(size, mode);
            let (jobs, _source) = match mode {
                CacheMode::Cold => cache_workload::encode_case(size, bc, |_| Some(0)),
                CacheMode::HotSet => {
                    cache_workload::encode_case(size, bc, |i| Some((i % 16) as u32))
                }
                CacheMode::Thrash => {
                    cache_workload::encode_case(size, bc, |i| Some((i % 17) as u32))
                }
                CacheMode::Unique => {
                    cache_workload::encode_case(size, bc, |i| Some((i % 256) as u32))
                }
                CacheMode::Disabled | CacheMode::Warm => unreachable!(),
            };
            for &w in &workers {
                let cache = cache_workload::cache_for(mode);
                let decoder =
                    ParallelDecoder::with_model_cache(cache_workload::config_for(w), cache.clone());
                let t0 = cache.timing();
                let m0 = cache.metrics();
                let decoded = decoder.decode_blocks(jobs.clone()).expect("decode");
                let t1 = cache.timing();
                let m1 = cache.metrics();
                let _ = decoded;
                println!(
                    "{:<8} {:<9} {:<10} {:<12} {:<12} {:<12} {:<14} {:<14} {:<14} {:<12}",
                    mode.name(),
                    size,
                    w,
                    t1.lock_acquires - t0.lock_acquires,
                    t1.lock_wait_ns - t0.lock_wait_ns,
                    t1.artifact_build_ns - t0.artifact_build_ns,
                    t1.single_flight_wait_ns - t0.single_flight_wait_ns,
                    t1.lookup_ns - t0.lookup_ns,
                    m1.lookups - m0.lookups,
                    m1.coalesced_waiters - m0.coalesced_waiters,
                );
            }
        }
    }
}
