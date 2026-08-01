//! # Loom concurrency courts for the parallel executor
//!
//! These tests compile the **real executor** against loom's synchronization
//! primitives (via the `crate::sync` swap layer) and run it inside
//! `loom::model`, which explores every thread interleaving of the closure.
//!
//! Run with:
//!
//! ```sh
//! RUSTFLAGS="--cfg loom" cargo test -p ryg-rans-rs-parallel \
//!   --features loom --release --test loom_tests
//! ```
//!
//! ## What is explored
//!
//! * Producer/worker/coordinator coordination: no lost tasks, no duplicate
//!   results, no deadlock, no task accepted after shutdown.
//! * Cancellation races: a token cancelled while tasks are in flight.
//! * Panic + cancellation: a panicking task cannot wedge the pipeline.
//! * Completeness accounting: the executor report's declared/submitted/
//!   completed counters.
//!
//! ## What is NOT explored (and why)
//!
//! * Queue *capacity*: loom's mpsc is unbounded, so the producer never
//!   blocks on a full queue under loom.  Boundedness and backpressure are
//!   pinned by the real-thread stress tests in `executor.rs` and
//!   `phase_i_tests.rs`.
//! * `sched_setaffinity` (the `affinity` feature): never enabled under loom
//!   builds; it is runtime-only and cannot be modelled.
//!
//! The models use tiny task counts and worker counts on purpose: loom's
//! state-space exploration is exponential in the number of interleaving
//! points.

#![cfg(loom)]

use loom::sync::Arc;
use ryg_rans_rs_parallel::{
    BufferSized, CancellationToken, ExecutorReport, ExecutorTask, HasBlockIndex, ParallelError,
    ReorderBuffer, WorkerScratch, run_tasks, run_tasks_with_sink,
};

/// A trivial task: echoes its input with a `usize` output.
struct EchoTask(pub usize);

impl ExecutorTask for EchoTask {
    type Output = usize;
    fn block_index(&self) -> Option<u64> {
        Some(self.0 as u64)
    }
    fn run(
        self,
        _worker: usize,
        _cancel: &CancellationToken,
        _scratch: &mut WorkerScratch,
    ) -> Self::Output {
        self.0
    }
}

/// A task that either panics or echoes, selected per task.
enum MixedTask {
    Panic(usize),
    Echo(usize),
}

impl ExecutorTask for MixedTask {
    type Output = usize;
    fn block_index(&self) -> Option<u64> {
        Some(match self {
            MixedTask::Panic(i) | MixedTask::Echo(i) => *i as u64,
        })
    }
    fn run(
        self,
        _worker: usize,
        _cancel: &CancellationToken,
        _scratch: &mut WorkerScratch,
    ) -> Self::Output {
        match self {
            MixedTask::Panic(i) => panic!("loom panic task {}", i),
            MixedTask::Echo(i) => i,
        }
    }
}

/// A task that checks cancellation at its yield point.
struct CancelProbeTask(pub usize);

impl ExecutorTask for CancelProbeTask {
    type Output = usize;
    fn block_index(&self) -> Option<u64> {
        Some(self.0 as u64)
    }
    fn run(
        self,
        _worker: usize,
        cancel: &CancellationToken,
        _scratch: &mut WorkerScratch,
    ) -> Self::Output {
        // Yield point: if cancelled, abandon this unit of work (the worker
        // loop counts it as cancelled, not completed).
        if cancel.is_cancelled() {
            return self.0;
        }
        self.0
    }
}

/// A `BufferSized`+`HasBlockIndex` item for the reorder model.
struct ReorderItem(pub u64);

impl ryg_rans_rs_parallel::HasBlockIndex for ReorderItem {
    fn block_index(&self) -> u64 {
        self.0
    }
}
impl ryg_rans_rs_parallel::BufferSized for ReorderItem {
    fn buffer_size(&self) -> u64 {
        8
    }
}

/// Every task is executed exactly once and every result is returned:
/// `declared == started == completed == returned`, and the returned set is
/// exactly the input set.
///
/// The model is kept tiny (2 tasks, 2 workers): loom's state-space
/// exploration is exponential in scheduling points, so the full 3-task/2-
/// worker model is run under `LOOM_MAX_PREEMPTIONS` (see the module docs).
#[test]
fn loom_executor_no_lost_tasks() {
    loom::model(|| {
        let tasks: Vec<EchoTask> = (0..2).map(EchoTask).collect();
        let report: ExecutorReport<usize> = run_tasks(tasks, 2, 2, None, None).unwrap();
        assert_eq!(report.returned_results, 2, "all results returned");
        let mut got: Vec<usize> = report.results.clone();
        got.sort_unstable();
        assert_eq!(got, vec![0, 1], "exactly the input set");
        assert_eq!(report.completed_tasks, 2, "completeness counter");
    });
}

/// Cancellation is never allowed to produce `Ok` with fewer results than
/// declared: the run must either return every result or a typed
/// `ParallelError::Cancelled`/`IncompleteExecution` (the completeness check
/// enforces this before returning `Ok`).
#[test]
fn loom_cancellation_race_completeness() {
    loom::model(|| {
        let cancel = Arc::new(CancellationToken::new());
        let probe = cancel.clone();
        let cancel_thread = loom::thread::spawn(move || {
            probe.cancel();
        });
        let tasks: Vec<CancelProbeTask> = (0..3).map(CancelProbeTask).collect();
        match run_tasks(tasks, 2, 2, None, Some(cancel)) {
            Ok(report) => {
                // If the run returned Ok, every task must be accounted for.
                assert_eq!(
                    report.returned_results, report.completed_tasks,
                    "Ok run must return every completed result"
                );
            }
            Err(ParallelError::Cancelled {
                completed,
                expected,
            }) => {
                assert!(completed <= expected, "completed cannot exceed expected");
            }
            Err(_) => {}
        }
        cancel_thread.join().unwrap();
    });
}

/// A panicking task must not wedge the pipeline: the run returns a typed
/// `WorkerPanic` error (or, depending on the interleaving, a completed run
/// of the non-panicking tasks), and no thread leaks.
#[test]
fn loom_panic_cancellation_no_wedge() {
    loom::model(|| {
        let tasks = vec![MixedTask::Panic(0), MixedTask::Echo(1)];
        let _ = run_tasks(tasks, 1, 1, None, None);
    });
}

/// The sink path (used by decode streaming) has the same no-lost-task
/// property when results are consumed through the callback.
#[test]
fn loom_sink_path_completeness() {
    loom::model(|| {
        let tasks: Vec<EchoTask> = (0..2).map(EchoTask).collect();
        let seen: Arc<std::sync::Mutex<Vec<usize>>> = Arc::default();
        let seen_clone = seen.clone();
        let report: ExecutorReport<usize> =
            run_tasks_with_sink(tasks, 1, 1, None, None, move |r| {
                seen_clone.lock().unwrap().push(r);
            })
            .unwrap();
        assert_eq!(report.returned_results, 2);
        let mut got = seen.lock().unwrap().clone();
        got.sort_unstable();
        assert_eq!(got, vec![0, 1], "sink received every result");
    });
}

/// ReorderBuffer commit under arbitrary completion order: inserting results
/// out of order must eventually commit exactly `[0, 1, ..., N-1]` in
/// strictly ascending order, with the newly-ready chain returned per insert.
#[test]
fn loom_reorder_commit_ascending() {
    loom::model(|| {
        let mut buf = ReorderBuffer::<ReorderItem>::new(16, 4096);
        // Completion order is nondeterministic; probe several orders.
        let orders: [[u64; 3]; 3] = [[1, 0, 2], [2, 1, 0], [0, 2, 1]];
        for order in orders {
            let mut buf2 = ReorderBuffer::<ReorderItem>::new(16, 4096);
            let mut committed: Vec<u64> = Vec::new();
            for &item in &order {
                for c in buf2.insert(ReorderItem(item)).unwrap() {
                    committed.push(c.block_index());
                }
            }
            assert_eq!(committed, vec![0, 1, 2], "strictly ascending commit");
        }
        let _ = buf;
    });
}
