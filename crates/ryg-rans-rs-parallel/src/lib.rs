#![cfg_attr(not(feature = "affinity"), forbid(unsafe_code))]

//! # ryg-rans-rs-parallel — Deterministic Parallel Block Engine
//!
//! This crate requires `std`.  It coordinates thread execution, bounded queues,
//! cancellation, and deterministic error selection.
//!
//! It does not duplicate rANS algorithms.  It depends on `ryg-rans-rs-core` and,
//! optionally, on `ryg-rans-rs-simd` for SIMD-accelerated inner kernels.
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
