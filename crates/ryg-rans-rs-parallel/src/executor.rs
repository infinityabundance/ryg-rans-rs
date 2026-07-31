//! # Bounded parallel executor
//!
//! A dedicated, bounded-memory executor owned by the caller or operation.
//! This executor is the core parallelism primitive for all RYGRANS parallel
//! operations (encode, decode, verify).
//!
//! ## Architecture
//!
//! The executor is built around a **bounded job channel** (`crossbeam_channel::bounded`):
//!
//! - The coordinator submits tasks into the bounded channel.
//! - Worker threads dequeue tasks, execute them, and write results into a
//!   **shared `Mutex<Vec>` collector**.
//! - After all tasks are submitted, the sender is dropped, workers receive
//!   `None` (the sentinel), break their receive loop, and are joined.
//!
//! ## Why a Mutex collector instead of a bounded result channel?
//!
//! An earlier design used a **bounded result channel** for worker output.
//! This caused a **deadlock** scenario:
//!
//! 1. The coordinator is blocked trying to send a task into the full job channel.
//! 2. All workers are blocked trying to send results into the full result channel.
//! 3. Nobody is draining the result channel (there is no dedicated drainer thread).
//!    → Deadlock.
//!
//! The fix: use a **`Mutex<Vec>`** for result collection.  Workers lock the
//! mutex, push their result, and unlock — the lock is held for a tiny duration
//! (just the `push` call).  Boundedness is still guaranteed by the job channel:
//! at most `effective_queue` tasks can be in-flight, and thus at most
//! `effective_queue` result entries can be outstanding at any time.
//!
//! ## Properties
//!
//! - Fixed number of worker threads (no dynamic scaling).
//! - Bounded multi-producer/multi-consumer work queue.
//! - Mutex-based result collection (not bounded, but implicitly bounded by
//!   job channel capacity).
//! - Explicit startup and shutdown.
//! - Automatic shutdown through `Drop` (channel drop causes worker termination).
//! - Every worker joined before `run_tasks` returns.
//! - Worker names include stable prefix `"ryg-parallel-N"` and numeric index.
//! - Worker panic captured with `catch_unwind`.
//! - Panic converted to typed `WorkerPanic` result that includes the block
//!   index if known (via `ExecutorTask::block_index`).
//! - No detached threads.
//! - No polling timeout loops.
//! - No busy-spin completion loops.
//!
//! ## Cancellation design
//!
//! The executor supports two cancellation modes:
//!
//! - **External cancellation**: The caller may pass a shared `CancellationToken`.
//!   Workers check it at defined yield points (before starting each task).
//!   The coordinator also checks it before submitting each task.  If cancelled,
//!   no new work is started and in-progress work may abort at checkpoints.
//! - **Internal cancellation**: If no external token is provided, an internal
//!   token is created.  If a worker panics, the internal token is cancelled,
//!   signalling all other workers to stop.
//!
//! In both cases, all workers are still joined (no abandoned threads).
//!
//! ## Error selection
//!
//! After all workers complete, the executor checks for panics.  If any panics
//! occurred, it selects the **lowest-index** panic (by `block_index`) and
//! returns `ParallelError::WorkerPanic`.  This is the canonical error: only
//! the first failing block's error is surfaced to the caller.
//!
//! ## Worker lifecycle
//!
//! 1. **Spawn** — Each worker is spawned with `std::thread::Builder`, named
//!    `"ryg-parallel-{i}"`, and optionally given a custom stack size.
//! 2. **Receive loop** — Workers call `rx.iter()` on their clone of the job
//!    receiver.  They receive `Some(task)` for real work, `None` to break.
//! 3. **Execute** — `catch_unwind(AssertUnwindSafe(|| task.run(i, &cancel)))`.
//!    On success, the result is pushed to the Mutex collector.  On panic, the
//!    panic message is extracted, pushed as an `Err`, and cancellation is
//!    triggered (internal token).
//! 4. **Join** — After the sender is dropped and all workers break their loop,
//!    the coordinator joins every handle.

use crate::cancellation::CancellationToken;
use crate::error::ParallelError;
use std::fmt;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Thread index for identifying workers.  Internal use only.
///
/// A `usize` in the range `[0, worker_count)`.  Passed to each task's
/// `run` method so the task can identify which lane it is executing on
/// (useful for diagnostics and backend selection).
type WorkerIndex = usize;

/// A task that can be executed by the executor.
///
/// This is the trait that any parallelisable work item must implement.
/// Workers receive a task via the bounded channel, execute its `run` method,
/// and the coordinator collects the result.
///
/// ## Lifecycle
///
/// 1. The coordinator creates `T: ExecutorTask` values and submits them to
///    the bounded job channel via `run_tasks`.
/// 2. A worker dequeues the task and calls `task.run(worker_index, &cancel)`
///    inside a `catch_unwind` guard.
/// 3. The returned `Output` value is pushed into the shared Mutex result
///    collector.
/// 4. If `run` panics, `block_index()` is called to attribute the panic to
///    a specific block (if available).
///
/// ## Safety
///
/// `Send + 'static` bounds ensure the task can be moved across threads and
/// does not borrow thread-local state.
pub trait ExecutorTask: Send + 'static {
    /// The type of result produced by this task.
    type Output: Send + 'static;

    /// Execute this task with the given worker index and cancellation token.
    fn run(self, worker_index: WorkerIndex, cancel: &CancellationToken) -> Self::Output;

    /// Return the block index for this task, if known.
    ///
    /// Used for panic reporting so the coordinator can identify which
    /// block caused a worker panic.  Return `None` if the block index
    /// is not available (e.g. for non-block tasks).
    fn block_index(&self) -> Option<u64> {
        None
    }
}

/// Report produced after executor completes.
///
/// Contains all task results collected from workers, along with a count of
/// any worker panics that occurred during execution.  Results are in
/// **completion order** (not block-index order).  The coordinator may need
/// to sort or reorder them (e.g. via `ReorderBuffer` for encoding results).
///
/// ## Error handling
///
/// - If `worker_panics > 0`, the `run_tasks` function returns
///   `Err(ParallelError::WorkerPanic)` instead of an `ExecutorReport`.
///   The panic count in the report is only meaningful when no panic errors
///   were returned (e.g. when the caller chooses to ignore panics, which
///   `run_tasks` does not currently support).
///
/// ## Panic attribution
///
/// Panicked results are stored internally as `Err((worker_index, panic_msg, block_index))`.
/// The `block_index` comes from `ExecutorTask::block_index()`, allowing the
/// coordinator to identify which block's task caused the panic.
pub struct ExecutorReport<R> {
    /// Completed task results (in completion order, not block-index order).
    pub results: Vec<R>,
    /// Number of worker panics that occurred.
    pub worker_panics: usize,
    /// Number of worker threads that were actually created.
    /// This is clamped to `[1, min(requested, total_tasks)]`.
    /// Use this in benchmark evidence to prove the intended thread
    /// count was actually executed.
    pub effective_workers: usize,
    /// Total number of tasks declared by the caller.
    pub declared_tasks: usize,
    /// Number of tasks actually submitted to the bounded queue.
    /// May be less than `declared_tasks` when cancellation interrupts
    /// submission.
    pub submitted_tasks: usize,
    /// Number of tasks that began execution on a worker.
    pub started_tasks: usize,
    /// Number of tasks that produced a result (completed).
    pub completed_tasks: usize,
    /// Number of tasks skipped because cancellation was observed
    /// before execution began.
    pub cancelled_tasks: usize,
    /// Number of results returned in `results`.
    pub returned_results: usize,
    /// Whether the run was cancelled (external token or panic-triggered).
    pub cancelled: bool,
}

/// Run a set of tasks on a bounded executor and collect results.
///
/// This is the primary convenience function for batch execution.
/// It handles task distribution, cancellation, panic containment,
/// and result collection.
///
/// # Parameters
///
/// * `tasks` — the tasks to execute.
/// * `worker_count` — number of worker threads to spawn.  Clamped to
///   `[1, min(worker_count, total_tasks)]` so we never spawn more workers
///   than tasks.
/// * `max_queue` — maximum capacity of the bounded job queue.  Clamped to
///   at least `effective_workers`.
/// * `stack_size` — optional per-worker stack size (passed to
///   `std::thread::Builder::stack_size`).
/// * `external_cancel` — optional external cancellation token.  If provided,
///   tasks check this token for cancellation.  If not provided, an internal
///   token is used that is only cancelled on worker panic.
///
/// # Boundedness — current limitations
///
/// **The job channel is bounded** to `effective_queue` slots.  This bounds
/// the number of tasks in-flight at any time, bounding peak memory for
/// active tasks.
///
/// **The result collector is NOT bounded end-to-end.** All completed
/// results are accumulated in a `Mutex<Vec<R>>` and reordering happens
/// only after every worker joins.  This means that for an N-block workload,
/// all N results may be resident simultaneously in the collector.
///
/// Current memory shape:
/// ```text
/// all input tasks (Vec<T>)          — allocated upfront
/// + bounded job queue               — effective_queue slots
/// + all completed results (Mutex)   — unbounded, all N results
/// + final ordered Vec               — all N results
/// = peak memory ~ 2× N results + bounded queue
/// ```
///
/// A future version should implement a **coordinator drain loop** that
/// consumes results from a bounded result channel while submitting new
/// jobs, enabling true end-to-end boundedness at the cost of more complex
/// coordinator logic.  See the `decode_streaming()` documentation for
/// additional context on the streaming architecture.
///
/// # Cancellation
///
/// - Before submitting each task, the coordinator checks `cancel.is_cancelled()`.
/// - Before starting each task, each worker checks `cancel.is_cancelled()`.
/// - If a worker panics and no external token was provided, the internal
///   token is cancelled, broadcasting to all workers.
///
/// # Panic safety
///
/// Workers execute inside `catch_unwind(AssertUnwindSafe(...))`.  If a worker
/// panics:
/// 1. The panic message is extracted (as `&str` or `String`).
/// 2. The block index is obtained via `task_block_index(&task)`.
/// 3. The panic is recorded as `Err((worker_index, msg, block_index))` in the
///    collector.
/// 4. The cancellation token is triggered.
/// 5. All other workers that check cancellation will see the cancellation and
///    stop processing (they drain the job channel but skip task execution).
/// 6. After joining, the canonical error is the **lowest-index panic**
///    (by `block_index`, with `None` treated as `u64::MAX`).
///
/// Returns `ParallelError::WorkerPanic` with the worker index and block index
/// of the lowest-index panic.
///
/// # Empty tasks
///
/// If `tasks` is empty, returns immediately with an empty `ExecutorReport`.
/// No workers are spawned.
pub fn run_tasks<T, R>(
    tasks: Vec<T>,
    worker_count: usize,
    max_queue: usize,
    stack_size: Option<usize>,
    external_cancel: Option<Arc<CancellationToken>>,
) -> Result<ExecutorReport<R>, ParallelError>
where
    T: ExecutorTask<Output = R> + Send + 'static,
    R: Send + 'static,
{
    let total_tasks = tasks.len();
    if total_tasks == 0 {
        return Ok(ExecutorReport {
            results: Vec::new(),
            worker_panics: 0,
            effective_workers: 0,
            declared_tasks: 0,
            submitted_tasks: 0,
            started_tasks: 0,
            completed_tasks: 0,
            cancelled_tasks: 0,
            returned_results: 0,
            cancelled: false,
        });
    }

    let effective_workers = worker_count.min(total_tasks).max(1);
    let effective_queue = max_queue.max(effective_workers);

    // Create the shared cancellation token.
    // If the caller provided one, we use that.  Otherwise we create an internal one.
    let cancel = external_cancel.unwrap_or_else(|| Arc::new(CancellationToken::new()));

    // Shared result collector (protected by a mutex).
    let collector = Arc::new(Mutex::new(Vec::<
        Result<R, (WorkerIndex, String, Option<u64>)>,
    >::with_capacity(total_tasks)));

    // Bounded job channel.
    let (job_sender, job_receiver) = crossbeam_channel::bounded::<Option<T>>(effective_queue);

    // Per-worker atomic counters.
    let started = Arc::new(AtomicUsize::new(0));
    let completed = Arc::new(AtomicUsize::new(0));
    let cancelled_tasks = Arc::new(AtomicUsize::new(0));

    // Spawn workers
    let mut handles = Vec::with_capacity(effective_workers);
    for i in 0..effective_workers {
        let rx = job_receiver.clone();
        let cancel = cancel.clone();
        let collector = collector.clone();
        let started = started.clone();
        let completed = completed.clone();
        let cancelled_tasks = cancelled_tasks.clone();

        let mut builder = std::thread::Builder::new();
        builder = builder.name(format!("ryg-parallel-{}", i));
        if let Some(stack) = stack_size {
            builder = builder.stack_size(stack);
        }

        let handle = builder
            .spawn(move || {
                for task_opt in rx {
                    if cancel.is_cancelled() {
                        cancelled_tasks.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }

                    let task = match task_opt {
                        Some(t) => t,
                        None => break,
                    };

                    let block_index_for_panic = task_block_index(&task);
                    started.fetch_add(1, Ordering::Relaxed);

                    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        task.run(i, &cancel)
                    })) {
                        Ok(r) => {
                            completed.fetch_add(1, Ordering::Relaxed);
                            let mut col = collector.lock().unwrap();
                            col.push(Ok(r));
                        }
                        Err(panic_payload) => {
                            let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                                s.to_string()
                            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                                s.clone()
                            } else {
                                "unknown panic".to_string()
                            };
                            completed.fetch_add(1, Ordering::Relaxed);
                            let mut col = collector.lock().unwrap();
                            col.push(Err((i, msg, block_index_for_panic)));
                            cancel.cancel();
                        }
                    }
                }
            })
            .map_err(|e| ParallelError::ThreadCreate(format!("worker {}: {}", i, e)))?;

        handles.push(handle);
    }
    drop(job_receiver); // workers hold their own clones

    // Submit all tasks through the bounded channel.
    let mut submitted = 0usize;
    for task in tasks {
        if cancel.is_cancelled() {
            break;
        }
        if job_sender.send(Some(task)).is_err() {
            break;
        }
        submitted += 1;
    }
    // Signal shutdown by dropping the sender.
    drop(job_sender);

    // Join all workers.
    for handle in handles {
        let _ = handle.join();
    }

    let was_cancelled = cancel.is_cancelled();
    let started_count = started.load(Ordering::Relaxed);
    let completed_count = completed.load(Ordering::Relaxed);
    let cancelled_count = cancelled_tasks.load(Ordering::Relaxed);

    // Collect results from the shared mutex.
    let mut col = collector.lock().unwrap();
    let mut results = Vec::with_capacity(total_tasks);
    let mut panic_errors: Vec<(WorkerIndex, String, Option<u64>)> = Vec::new();

    for item in col.drain(..) {
        match item {
            Ok(r) => results.push(r),
            Err(e) => panic_errors.push(e),
        }
    }

    // Check for panics first (highest priority)
    if !panic_errors.is_empty() {
        let lowest_panic = panic_errors
            .iter()
            .min_by_key(|(_, _, block_idx)| block_idx.unwrap_or(u64::MAX));
        if let Some((wi, _msg, block_idx)) = lowest_panic {
            return Err(ParallelError::WorkerPanic {
                block_index: *block_idx,
                worker_index: *wi,
            });
        }
    }

    // Completeness invariant: cancellation must never return Ok with
    // fewer results than declared.  If the run was cancelled, return a
    // Cancelled error carrying the counts.  If it was not cancelled but
    // results are short, that is silent truncation — an internal bug.
    if was_cancelled && results.len() != total_tasks {
        return Err(ParallelError::Cancelled {
            completed: results.len(),
            expected: total_tasks,
        });
    }
    if !was_cancelled && results.len() != total_tasks {
        return Err(ParallelError::IncompleteExecution {
            completed: results.len(),
            expected: total_tasks,
        });
    }

    let returned_results = results.len();

    Ok(ExecutorReport {
        results,
        worker_panics: panic_errors.len(),
        effective_workers,
        declared_tasks: total_tasks,
        submitted_tasks: submitted,
        started_tasks: started_count,
        completed_tasks: completed_count,
        cancelled_tasks: cancelled_count,
        returned_results,
        cancelled: was_cancelled,
    })
}

/// Extract the block index from a task, if available.
///
/// Uses the `block_index()` method on the `ExecutorTask` trait.
fn task_block_index<T: ExecutorTask>(task: &T) -> Option<u64> {
    task.block_index()
}

impl<R> fmt::Debug for ExecutorReport<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorReport")
            .field("results", &self.results.len())
            .field("worker_panics", &self.worker_panics)
            .field("effective_workers", &self.effective_workers)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct TestTask {
        value: u32,
    }

    impl ExecutorTask for TestTask {
        type Output = u32;

        fn run(self, _worker_index: usize, _cancel: &CancellationToken) -> u32 {
            self.value
        }
    }

    #[test]
    fn test_single_task() {
        let tasks = vec![TestTask { value: 42 }];
        let report = run_tasks(tasks, 1, 4, None, None).unwrap();
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0], 42);
        assert_eq!(report.worker_panics, 0);
    }

    #[test]
    fn test_multiple_tasks() {
        let tasks: Vec<_> = (0..10).map(|i| TestTask { value: i }).collect();
        let report = run_tasks(tasks, 3, 8, None, None).unwrap();
        assert_eq!(report.results.len(), 10);
        let mut sorted: Vec<_> = report.results.clone();
        sorted.sort();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
        assert_eq!(report.worker_panics, 0);
    }

    #[test]
    fn test_empty_tasks() {
        let tasks: Vec<TestTask> = Vec::new();
        let report = run_tasks(tasks, 4, 4, None, None).unwrap();
        assert!(report.results.is_empty());
        assert_eq!(report.worker_panics, 0);
    }

    struct PanicTask;

    impl ExecutorTask for PanicTask {
        type Output = u32;

        fn run(self, _worker_index: usize, _cancel: &CancellationToken) -> u32 {
            panic!("test panic");
        }
    }

    #[test]
    fn test_worker_panic_containment() {
        let tasks = vec![PanicTask, PanicTask];
        let result = run_tasks(tasks, 1, 4, None, None);
        match result {
            Err(ParallelError::WorkerPanic { .. }) => {} // Expected
            other => panic!("expected WorkerPanic, got {:?}", other),
        }
    }

    #[test]
    fn test_multiple_workers() {
        let tasks: Vec<_> = (0..100).map(|i| TestTask { value: i }).collect();
        let report = run_tasks(tasks, 4, 16, None, None).unwrap();
        assert_eq!(report.results.len(), 100);
        assert_eq!(report.worker_panics, 0);
    }

    #[test]
    fn test_external_cancellation() {
        let cancel = Arc::new(CancellationToken::new());
        let cancel_clone = cancel.clone();

        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(10));
            cancel_clone.cancel();
        });

        let tasks: Vec<_> = (0..50).map(|i| TestTask { value: i }).collect();
        let _report = run_tasks(tasks, 2, 8, None, Some(cancel)).unwrap();
        handle.join().unwrap();
    }
}
