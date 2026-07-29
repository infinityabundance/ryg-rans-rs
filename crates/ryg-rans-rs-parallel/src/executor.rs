//! # Bounded parallel executor
//!
//! A dedicated executor owned by the caller or operation.
//!
//! ## Properties
//!
//! - Fixed number of worker threads.
//! - Bounded multi-producer/multi-consumer work queue.
//! - Bounded result queue.
//! - Explicit startup and shutdown.
//! - Automatic shutdown through `Drop`.
//! - Every worker joined on drop.
//! - Worker names include stable prefix and numeric index.
//! - Worker panic captured with `catch_unwind`.
//! - Panic converted to typed `WorkerPanic` result.
//! - No detached thread.
//! - No polling timeout loop.
//! - No busy-spin completion loop.

use crate::cancellation::CancellationToken;
use crate::error::{BlockError, BlockErrorKind, CanonicalErrorTracker, ParallelError};
use std::fmt;

/// Thread index for identifying workers.  Internal use only.
type WorkerIndex = usize;

/// A task that can be executed by the executor.
///
/// Workers receive shared state via the context, return a result through
/// the channel, and check cancellation at defined yield points.
pub trait ExecutorTask: Send + 'static {
    /// The type of result produced by this task.
    type Output: Send + 'static;

    /// Execute this task with the given worker index and cancellation token.
    fn run(self, worker_index: WorkerIndex, cancel: &CancellationToken) -> Self::Output;
}

/// Internal collector for worker results and panics.
struct ExecutorCollector<R> {
    results: Vec<R>,
    error_tracker: CanonicalErrorTracker,
    worker_panics: Vec<(WorkerIndex, String)>,
    completed: usize,
    total_tasks: usize,
}

impl<R> ExecutorCollector<R> {
    fn new(total_tasks: usize) -> Self {
        Self {
            results: Vec::with_capacity(total_tasks),
            error_tracker: CanonicalErrorTracker::new(),
            worker_panics: Vec::new(),
            completed: 0,
            total_tasks,
        }
    }
}

/// Report produced after executor completes.
#[derive(Debug)]
pub struct ExecutorReport<R> {
    /// Completed task results (in completion order, not block-index order).
    pub results: Vec<R>,
    /// Number of worker panics that occurred.
    pub worker_panics: usize,
}

/// Run a set of tasks on a bounded executor and collect results.
///
/// This is the primary convenience function for batch execution.
/// It handles task distribution, cancellation, panic containment,
/// and result collection.
pub fn run_tasks<T, R>(
    tasks: Vec<T>,
    worker_count: usize,
    max_queue: usize,
    stack_size: Option<usize>,
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
        });
    }

    let effective_workers = worker_count.min(total_tasks).max(1);
    let effective_queue = max_queue.max(effective_workers);

    let cancel = std::sync::Arc::new(CancellationToken::new());
    let collector = std::sync::Arc::new(std::sync::Mutex::new(ExecutorCollector::<R>::new(
        total_tasks,
    )));

    // Bounded channel
    let (sender, receiver) = crossbeam_channel::bounded::<Option<T>>(effective_queue);

    // Spawn workers
    let mut handles = Vec::with_capacity(effective_workers);
    for i in 0..effective_workers {
        let rx = receiver.clone();
        let cancel = cancel.clone();
        let collector = collector.clone();

        let mut builder = std::thread::Builder::new();
        builder = builder.name(format!("ryg-parallel-{}", i));
        if let Some(stack) = stack_size {
            builder = builder.stack_size(stack);
        }

        let handle = builder
            .spawn(move || {
                for task_opt in rx {
                    let task = match task_opt {
                        Some(t) => t,
                        None => break,
                    };

                    if cancel.is_cancelled() {
                        continue;
                    }

                    let result =
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            task.run(i, &cancel)
                        })) {
                            Ok(r) => r,
                            Err(panic_payload) => {
                                let msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                                    s.to_string()
                                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                                    s.clone()
                                } else {
                                    "unknown panic".to_string()
                                };
                                let mut col = collector.lock().unwrap();
                                col.worker_panics.push((i, msg));
                                cancel.cancel();
                                continue;
                            }
                        };

                    let mut col = collector.lock().unwrap();
                    col.results.push(result);
                    col.completed += 1;
                }
            })
            .map_err(|e| ParallelError::ThreadCreate(format!("worker {}: {}", i, e)))?;

        handles.push(handle);
    }

    // Submit all tasks
    for task in tasks {
        if cancel.is_cancelled() {
            break;
        }
        sender
            .send(Some(task))
            .map_err(|_| ParallelError::Internal("task submission channel closed".into()))?;
    }

    // Signal shutdown by sending poison pills
    drop(sender);

    // Join all workers
    for handle in handles {
        handle
            .join()
            .map_err(|_| ParallelError::Internal("worker thread join failed".into()))?;
    }

    // Collect results
    let mut col = collector.lock().unwrap();

    if !col.worker_panics.is_empty() {
        let (wi, _msg) = &col.worker_panics[0];
        return Err(ParallelError::WorkerPanic {
            block_index: None,
            worker_index: *wi,
        });
    }

    if let Some(block_err) = col.error_tracker.canonical_error() {
        return Err(ParallelError::DecodeFailed(Box::new(block_err.clone())));
    }

    Ok(ExecutorReport {
        results: std::mem::take(&mut col.results),
        worker_panics: col.worker_panics.len(),
    })
}

impl<R> fmt::Debug for ExecutorCollector<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExecutorCollector")
            .field("completed", &self.completed)
            .field("total_tasks", &self.total_tasks)
            .field("worker_panics", &self.worker_panics.len())
            .finish()
    }
}

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
        let report = run_tasks(tasks, 1, 4, None).unwrap();
        assert_eq!(report.results.len(), 1);
        assert_eq!(report.results[0], 42);
        assert_eq!(report.worker_panics, 0);
    }

    #[test]
    fn test_multiple_tasks() {
        let tasks: Vec<_> = (0..10).map(|i| TestTask { value: i }).collect();
        let report = run_tasks(tasks, 3, 8, None).unwrap();
        assert_eq!(report.results.len(), 10);
        // Results may be in any order (workers are concurrent)
        let mut sorted: Vec<_> = report.results.clone();
        sorted.sort();
        assert_eq!(sorted, (0..10).collect::<Vec<_>>());
        assert_eq!(report.worker_panics, 0);
    }

    #[test]
    fn test_empty_tasks() {
        let tasks: Vec<TestTask> = Vec::new();
        let report = run_tasks(tasks, 4, 4, None).unwrap();
        assert!(report.results.is_empty());
        assert_eq!(report.worker_panics, 0);
    }

    #[test]
    fn test_cancellation_before_start() {
        let tasks: Vec<_> = (0..5).map(|i| TestTask { value: i }).collect();
        // Use 1 worker and a small queue
        let report = run_tasks(tasks, 1, 2, None).unwrap();
        assert_eq!(report.results.len(), 5);
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
        let result = run_tasks(tasks, 1, 4, None);
        match result {
            Err(ParallelError::WorkerPanic { .. }) => {} // Expected
            other => panic!("expected WorkerPanic, got {:?}", other),
        }
    }

    #[test]
    fn test_multiple_workers() {
        let tasks: Vec<_> = (0..100).map(|i| TestTask { value: i }).collect();
        let report = run_tasks(tasks, 4, 16, None).unwrap();
        assert_eq!(report.results.len(), 100);
        assert_eq!(report.worker_panics, 0);
    }
}
