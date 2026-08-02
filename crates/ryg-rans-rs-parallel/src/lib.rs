#![cfg_attr(not(feature = "affinity"), forbid(unsafe_code))]

//! # ryg-rans-rs-parallel — Deterministic Parallel Block Engine
//!
//! This crate requires `std`.  It coordinates thread execution, bounded queues,
//! cancellation, and deterministic error selection.
//!
//! It does not duplicate rANS algorithms.  It depends on `ryg-rans-rs-core` and,
//! optionally, on `ryg-rans-rs-simd` for SIMD-accelerated inner kernels.
//!
//! ## Purpose
//!
//! The crate answers one question: how do you encode/decode/verify many
//! independent blocks in parallel while keeping output order, error identity,
//! and peak memory deterministic and bounded by configuration?  The answer is
//! the bounded live executor (paper 0004): a producer thread, two bounded
//! channels, per-worker exclusive scratch, a live reorder commit, a shared
//! model cache, and a completeness-checked cancellation protocol.
//!
//! ## History
//!
//! Phase I created the first parallel engine; Phase L.4 rebuilt the executor
//! as a genuinely bounded live pipeline after an audit showed the original
//! materialise-then-reorder design could not honour `max_buffered_output_bytes`.
//! Phase L.3 added external cancellation and the completeness invariant;
//! L.5 made reorder commits atomic; L.6 wired (or removed) every configuration
//! field; L.7/L.8 wired scratch and the model cache into production paths;
//! L.9 made backend semantics exact.  See `docs/history/` and the ADRs.
//!
//! ## Design
//!
//! ```text
//! producer ─▶ bounded job channel ─▶ K workers (exclusive scratch) ─▶
//!   bounded result channel ─▶ coordinator ─▶ live ReorderBuffer commit
//! ```
//!
//! Determinism is by construction: `FixedBlockPlan` boundaries depend only on
//! input length; the reorder buffer commits ascending indexes; the canonical
//! error is the lowest block index; `effective_workers` is clamped and
//! reported.  Cancellation is cooperative and complete: the `ExecutorReport`
//! counts declared/submitted/started/completed/cancelled/skipped/returned,
//! and every public API re-asserts completeness at its own boundary
//! (`error::check_completeness`).
//!
//! ## Alternatives considered
//!
//! * Work-stealing schedulers — rejected: complicate deterministic error
//!   selection and boundedness (ADR-0004).
//! * A shared `ScratchPool` behind a mutex — rejected: a lock in the per-block
//!   path serializes workers (ADR-0015).
//! * Caching whole decode plans — rejected: a plan depends on runtime backend
//!   conditions; only model-derived artifacts are cached (ADR-0009).
//!
//! ## Invariants (frozen)
//!
//! 1. Same input + config → same output and same canonical error, independent
//!    of worker count, completion order, or schedule.
//! 2. Cancellation never returns `Ok` with fewer blocks than declared.
//! 3. Explicit backend requests execute exactly or return a typed error.
//! 4. `ParallelConfig` has no inert public field.
//! 5. Peak memory is bounded by configuration, not by workload.
//!
//! ## Failure modes
//!
//! * Worker panic — caught by `catch_unwind`, surfaced as `WorkerPanic`.
//! * Silent truncation — prevented by the completeness checks (an
//!   `IncompleteExecution` error is an internal bug, never a short `Ok`).
//! * Channel deadlock — prevented by the producer/coordinator split (a naive
//!   inline-submission design deadlocks; the loom courts model this).
//! * Config errors (affinity, stack size) — typed `Config` errors before any
//!   thread spawns; a stack below the platform minimum would otherwise abort
//!   the process on Linux.
//!
//! ## Performance
//!
//! The sealed numbers (run `phase-l-20260802b`) show a ~1.4× single-worker
//! overhead over the raw decode kernel, dominated by the mandatory dual
//! SHA-256 per block (payload + decoded-output integrity), not by scheduling.
//! The model cache removes per-block packed-table construction for repeated
//! models; scratch removes per-block allocation churn.
//!
//! ## Verification / Receipts / Tests
//!
//! The unit/property suite covers every config field (single-field tests),
//! reorder permutations, cancellation races, boundedness stress, scratch
//! allocation counts, cache equivalence, and backend compatibility.  The
//! Phase L courts seal the guarantees: `RYG_RANS.L.CANCEL.COMPLETENESS`,
//! `RYG_RANS.L.EXECUTOR.BOUNDED`, `RYG_RANS.L.REORDER.ATOMIC_COMMIT`,
//! `RYG_RANS.L.CONFIG.WIRING`, `RYG_RANS.L.SCRATCH.INTEGRATION`,
//! `RYG_RANS.L.MODEL_CACHE.INTEGRATION`, `RYG_RANS.L.BACKEND.EXPLICIT`,
//! and `RYG_RANS.L.PUBLIC_API.REACHABILITY`.
//!
//! ## Future evolution
//!
//! A true streaming input path (feeding blocks one at a time from a
//! non-seekable source) is the documented next step; the current streaming
//! API still materialises jobs.  Batch4 coordinator-level grouping is
//! planned behind the existing `BackendRequiresBatchContext` boundary.
//!
//! ## References
//!
//! `docs/papers/0004-parallel-engine.md` (architecture), `docs/adr/0004`,
//! `docs/adr/0005`, `docs/adr/0007`, `docs/adr/0009`, `docs/adr/0013`,
//! `docs/adr/0014`, `docs/adr/0015` (decisions), `docs/history/` (timeline),
//! `docs/glossary.md` (worker, task, in-flight, reorder buffering, committed
//! output).
//!
//! ## Module layout
//!
//! | Module | Description |
//! |--------|-------------|
//! | `config` | Thread count, queue bounds, backend policy, error policy |
//! | `error` | Typed errors — parallel-specific, deterministic selection |
//! | `executor` | Bounded worker pool with cancellation and panic containment |
//! | `cancellation` | Cooperative thread-safe cancellation token |
//! | `job` | Encode/decode/verify job types and results |
//! | `plan` | Fixed block planning — thread-count-independent boundaries |
//! | `reorder` | Bounded ordered result buffer — block-index-sequential commit |
//! | `encode` | Parallel per-block encoding with ordered write |
//! | `decode` | Parallel seekable and streaming decode |
//! | `verify` | Parallel container verification |
//! | `cache` | Shared immutable model/table cache |
//! | `resource` | Memory estimation and accounting |

mod affinity;
mod block;
mod cache;
mod cancellation;
mod config;
mod decode;
mod decode_plan;
mod encode;
mod error;
mod executor;
mod job;
mod plan;
mod reorder;
mod resource;
mod scratch;
mod sync;
mod verify;

pub use affinity::*;
pub use block::*;
pub use cache::*;
pub use cancellation::CancellationToken;
pub use config::*;
pub use decode::*;
pub use decode_plan::*;
pub use encode::*;
pub use error::*;
pub use executor::*;
pub use job::*;
pub use plan::*;
pub use reorder::*;
pub use resource::*;
pub use scratch::*;
pub use verify::*;
