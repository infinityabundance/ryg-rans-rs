//! # Synchronization primitives — std/loom swap layer
//!
//! The executor's concurrency courts run under [loom](https://crates.io/crates/loom),
//! which explores every thread interleaving of a `loom::model` closure.  Loom
//! can only explore synchronization it knows about, so the executor must be
//! compiled against `loom::sync`/`loom::thread` types when the `loom` cfg is
//! set, and against the real `std`/`crossbeam` types otherwise.
//!
//! This module is that swap layer.  Everything the executor touches goes
//! through it:
//!
//! | std build | loom build (`--cfg loom`) |
//! |-----------|----------------------------|
//! | `std::sync::Arc` | `loom::sync::Arc` |
//! | `std::sync::Mutex` | `loom::sync::Mutex` |
//! | `std::sync::atomic::*` | `loom::sync::atomic::*` |
//! | `std::thread` | `loom::thread` |
//! | `crossbeam_channel::bounded(cap)` | `loom::sync::mpsc::channel()` (unbounded) |
//!
//! ## Boundedness note
//!
//! Loom's mpsc is unbounded, so under `--cfg loom` the executor's queue
//! *capacity* is not exercised (the producer never blocks).  Loom models the
//! coordination *correctness*: no lost tasks, no deadlock, no lost wakeups,
//! cancellation and panic races, completeness accounting.  Queue
//! boundedness and backpressure are pinned separately by the real-thread
//! stress tests (`executor.rs` unit tests and `phase_i_tests.rs`).
//!
//! ## Safety
//!
//! This module is pure re-export/adaptation; it contains no `unsafe`.  The
//! `affinity` feature (libc) is never enabled under loom builds (the loom
//! test command must not pass it), so the crate's
//! `forbid(unsafe_code)`/`cfg_attr` split is unaffected.

#[cfg(loom)]
pub use loom::sync::{Arc, Mutex};
#[cfg(not(loom))]
pub use std::sync::{Arc, Mutex};

#[cfg(loom)]
pub use loom::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
#[cfg(not(loom))]
pub use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Condition variable: `std::sync::Condvar` at runtime, `loom::sync::Condvar`
/// under loom.  The model-artifact cache uses it for single-flight waiter
/// notification (Phase O.5); the executor's loom queue already uses it
/// directly.  `wait_timeout` is used by the cache so that cancelled waiters
/// can stop polling without waiting indefinitely on a build that may be
/// blocked behind a cancelled builder.
#[cfg(loom)]
pub use loom::sync::Condvar;
#[cfg(not(loom))]
pub use std::sync::Condvar;

/// Mutex guard type matching the active build (`loom::sync::MutexGuard`
/// under loom).  Named so cache code can spell its lock-guard type without a
/// cfg split.
#[cfg(loom)]
pub type MutexGuard<'a, T> = loom::sync::MutexGuard<'a, T>;
#[cfg(not(loom))]
pub type MutexGuard<'a, T> = std::sync::MutexGuard<'a, T>;

/// Timed condvar wait that normalizes the std/loom API difference.
///
/// Both `std::sync::Condvar::wait_timeout` and loom's return a `Result`
/// (poison).  The model-artifact cache's waiter loop uses the timeout to
/// poll its cancellation token.  Returns the re-acquired guard, or `None`
/// when the associated mutex is poisoned (the cache then abandons the wait
/// and bypasses — see `ModelArtifactCache::get_or_build`).
#[cfg(not(loom))]
pub fn wait_timeout<'a, T>(
    cv: &Condvar,
    guard: MutexGuard<'a, T>,
    dur: std::time::Duration,
) -> Option<MutexGuard<'a, T>> {
    cv.wait_timeout(guard, dur).ok().map(|(g, _)| g)
}

#[cfg(loom)]
pub fn wait_timeout<'a, T>(
    cv: &Condvar,
    guard: MutexGuard<'a, T>,
    dur: std::time::Duration,
) -> Option<MutexGuard<'a, T>> {
    cv.wait_timeout(guard, dur).ok().map(|(g, _)| g)
}

/// Thread abstraction: `std::thread` at runtime, `loom::thread` under loom.
#[cfg(loom)]
pub mod thread {
    pub use loom::thread::{Builder, JoinHandle};
}

#[cfg(not(loom))]
pub mod thread {
    pub use std::thread::{Builder, JoinHandle};
}

/// Spawn a named worker thread.
///
/// Loom's `Builder` has no `name`; names are a runtime-only concern, so they
/// are applied exclusively in the std build.
#[cfg(not(loom))]
pub fn spawn_worker<F, T>(
    name: &str,
    stack_size: Option<usize>,
    f: F,
) -> std::io::Result<thread::JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let mut builder = thread::Builder::new();
    builder = builder.name(name.to_string());
    if let Some(stack) = stack_size {
        builder = builder.stack_size(stack);
    }
    builder.spawn(f)
}

#[cfg(loom)]
pub fn spawn_worker<F, T>(
    _name: &str,
    _stack_size: Option<usize>,
    f: F,
) -> std::io::Result<thread::JoinHandle<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    thread::Builder::new().spawn(f)
}

/// Bounded channel abstraction: `crossbeam_channel::bounded` at runtime
/// (the real memory bound), loom's unbounded mpsc under loom (interleaving
/// exploration; see the module note on boundedness).
#[cfg(not(loom))]
pub mod channel {
    use std::sync::Arc;

    /// Crossbeam sender (already cheap to clone internally).
    #[derive(Debug)]
    pub struct Sender<T>(crossbeam_channel::Sender<T>);

    impl<T> Sender<T> {
        pub fn send(&self, t: T) -> Result<(), crossbeam_channel::SendError<T>> {
            self.0.send(t)
        }
    }

    impl<T> Clone for Sender<T> {
        fn clone(&self) -> Self {
            Sender(self.0.clone())
        }
    }

    /// Crossbeam receiver, shared through `Arc` for uniform clone semantics
    /// with the loom build.
    #[derive(Debug)]
    pub struct Receiver<T>(Arc<crossbeam_channel::Receiver<T>>);

    impl<T> Receiver<T> {
        pub fn recv(&self) -> Result<T, crossbeam_channel::RecvError> {
            self.0.recv()
        }
    }

    impl<T> Clone for Receiver<T> {
        fn clone(&self) -> Self {
            Receiver(self.0.clone())
        }
    }

    /// Create a bounded channel pair (the real memory bound).
    pub fn bounded<T>(cap: usize) -> (Sender<T>, Receiver<T>) {
        let (s, r) = crossbeam_channel::bounded(cap);
        (Sender(s), Receiver(Arc::new(r)))
    }
}

#[cfg(loom)]
pub mod channel {
    use crate::sync::{Arc, AtomicUsize, Mutex, Ordering};
    use loom::sync::Condvar;
    use std::collections::VecDeque;

    /// Multi-producer / multi-consumer queue modelled with loom's `Mutex` +
    /// `Condvar`.
    ///
    /// # Why not loom's `mpsc`
    ///
    /// Loom's mpsc is a thin wrapper over `std::sync::mpsc`: its receiver is
    /// `Send` but not `Sync`, so multiple workers sharing one receiver must
    /// hold a mutex across the blocking `recv` — a worker preempted inside
    /// `recv` then starves every other worker (a deadlock loom correctly
    /// reports).  The real executor uses crossbeam's `Receiver`, which is
    /// `Sync`, so many workers may block in `recv` simultaneously and a
    /// single send wakes one.  This queue reproduces those exact semantics
    /// with primitives loom can schedule.
    ///
    /// # Why the sender count lives under the mutex
    ///
    /// The wait condition is `queue empty && senders > 0`.  If the sender
    /// count were an atomic read outside the mutex, a consumer could pass
    /// the check and be preempted before registering its `wait`; the last
    /// sender's drop would then `notify_all` a condvar with no registered
    /// waiter — a permanently lost wakeup (loom caught exactly this).
    /// Holding the count, the queue, and the notify under one mutex makes
    /// the check and the wait registration atomic with respect to drop.
    #[derive(Debug)]
    pub struct Sender<T> {
        queue: Arc<Queue<T>>,
    }

    #[derive(Debug)]
    struct Queue<T> {
        inner: Mutex<Inner<T>>,
        wake: Condvar,
    }

    #[derive(Debug)]
    struct Inner<T> {
        items: VecDeque<T>,
        senders: usize,
    }

    impl<T> Sender<T> {
        pub fn send(&self, t: T) -> Result<(), T> {
            let mut inner = self.queue.inner.lock().expect("send lock");
            inner.items.push_back(t);
            self.queue.wake.notify_one();
            Ok(())
        }
    }

    impl<T> Clone for Sender<T> {
        fn clone(&self) -> Self {
            self.queue.inner.lock().expect("sender clone lock").senders += 1;
            Sender {
                queue: self.queue.clone(),
            }
        }
    }

    impl<T> Drop for Sender<T> {
        fn drop(&mut self) {
            let mut inner = self.queue.inner.lock().expect("sender drop lock");
            debug_assert!(inner.senders > 0);
            inner.senders -= 1;
            if inner.senders == 0 {
                // Last sender: wake every waiter so they observe disconnect.
                self.queue.wake.notify_all();
            }
        }
    }

    #[derive(Debug)]
    pub struct Receiver<T> {
        queue: Arc<Queue<T>>,
    }

    impl<T> Receiver<T> {
        pub fn recv(&self) -> Result<T, ()> {
            let mut inner = self.queue.inner.lock().expect("recv lock");
            while inner.items.is_empty() && inner.senders > 0 {
                // Wait releases the mutex; a spurious or lost wakeup simply
                // re-checks the loop condition.
                inner = self.queue.wake.wait(inner).expect("recv wait");
            }
            match inner.items.pop_front() {
                Some(v) => Ok(v),
                None => Err(()),
            }
        }
    }

    impl<T> Clone for Receiver<T> {
        fn clone(&self) -> Self {
            Receiver {
                queue: self.queue.clone(),
            }
        }
    }

    /// Create a channel pair.  The capacity argument is accepted for API
    /// compatibility; the loom model is unbounded.
    pub fn bounded<T>(_cap: usize) -> (Sender<T>, Receiver<T>) {
        let queue = Arc::new(Queue {
            inner: Mutex::new(Inner {
                items: VecDeque::new(),
                senders: 1,
            }),
            wake: Condvar::new(),
        });
        (
            Sender {
                queue: queue.clone(),
            },
            Receiver { queue },
        )
    }
}

// ---------------------------------------------------------------------------
// Loom model tests for the channel queue itself
// ---------------------------------------------------------------------------

#[cfg(all(loom, test))]
mod loom_channel_tests {
    use super::channel;

    /// Minimal: one producer, one consumer, one item.
    #[test]
    fn loom_channel_single_item() {
        loom::model(|| {
            let (tx, rx) = channel::bounded::<usize>(1);
            let p = loom::thread::spawn(move || {
                tx.send(7).unwrap();
                drop(tx);
            });
            let c = loom::thread::spawn(move || match rx.recv() {
                Ok(v) => assert_eq!(v, 7),
                Err(()) => panic!("expected the item"),
            });
            p.join().unwrap();
            c.join().unwrap();
        });
    }

    /// One producer, one consumer, two items: exercises the send-notify and
    /// drop-notify termination paths in a small state space.
    #[test]
    fn loom_channel_two_items_single_consumer() {
        loom::model(|| {
            let (tx, rx) = channel::bounded::<usize>(1);
            let p = loom::thread::spawn(move || {
                tx.send(0).unwrap();
                tx.send(1).unwrap();
                drop(tx);
            });
            let c = loom::thread::spawn(move || {
                let mut got = Vec::new();
                loop {
                    match rx.recv() {
                        Ok(v) => got.push(v),
                        Err(()) => break,
                    }
                }
                assert_eq!(got, vec![0, 1], "both items in order");
            });
            p.join().unwrap();
            c.join().unwrap();
        });
    }
}
