//! # Parallel configuration — thread count, queue bounds, backend, error policy
//!
//! All configuration is explicit. No hidden global state.
//! Thread count never affects canonical output.

use std::num::NonZeroUsize;
use std::vec::Vec;

/// Top-level parallel engine configuration.
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Number of worker threads.
    pub threads: ThreadCount,
    /// Maximum blocks queued for processing (per direction).
    pub max_in_flight_blocks: NonZeroUsize,
    /// Maximum total input bytes buffered across all in-flight blocks.
    pub max_buffered_input_bytes: u64,
    /// Maximum total decoded bytes buffered across all completed blocks.
    pub max_buffered_output_bytes: u64,
    /// Minimum input size before parallelism is considered.
    pub parallel_threshold_bytes: u64,
    /// CPU affinity policy (optional optimisation).
    pub affinity: AffinityPolicy,
    /// Backend selection policy.
    pub backend_policy: BackendPolicy,
    /// Error selection policy (must be deterministic).
    pub error_policy: ErrorPolicy,
    /// Optional per-worker stack size. `None` = system default.
    pub worker_stack_size: Option<usize>,
    /// Whether to skip inner SIMD batching.
    pub disable_inner_batching: bool,
    /// Whether to skip SIMD decoding entirely.
    pub disable_simd: bool,
    /// SMT/hyper-threading policy.
    pub smt_policy: SmtPolicy,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            threads: ThreadCount::AvailableParallelism,
            max_in_flight_blocks: NonZeroUsize::new(16).unwrap(),
            max_buffered_input_bytes: 256 * 1024 * 1024, // 256 MiB
            max_buffered_output_bytes: 512 * 1024 * 1024, // 512 MiB
            parallel_threshold_bytes: 1024 * 1024,       // 1 MiB
            affinity: AffinityPolicy::None,
            backend_policy: BackendPolicy::Portable,
            error_policy: ErrorPolicy::LowestBlockIndex,
            worker_stack_size: None,
            disable_inner_batching: false,
            disable_simd: false,
            smt_policy: SmtPolicy::UseAllLogical,
        }
    }
}

/// How many worker threads to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCount {
    /// Exactly N threads.
    Exact(NonZeroUsize),
    /// Use `std::thread::available_parallelism()`.
    AvailableParallelism,
}

/// CPU affinity policy (best-effort, never affects correctness).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AffinityPolicy {
    /// No affinity set.
    None,
    /// Pack workers onto consecutive physical/logical CPUs.
    Compact,
    /// Spread workers across available CPUs.
    Spread,
    /// Explicit CPU-index list. Length must equal worker count.
    Explicit(Vec<usize>),
}

/// Backend selection policy for inner decode kernels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendPolicy {
    /// Only portable scalar kernels.
    Portable,
    /// Prefer scalar even when SIMD is available.
    ScalarPreferred,
    /// Auto-detect best available backend.
    Auto,
    /// Use exactly the specified backend.
    Explicit(BackendId),
    /// Select backend per block based on model analysis.
    ModelAware,
}

/// Identifies a specific decode kernel/backend.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BackendId {
    RawCopy,
    RleFill,
    Scalar8,
    Scalar16,
    Sse41Interleaved8,
    Avx512VlInterleaved8,
    Avx512Interleaved16,
    Avx512VlManualGather8,
    Avx512ManualGather16,
    Avx512Vl2x8,
    Avx512Batch4,
    Uniform256TableFree16,
    Avx2ManualGather8,
    Avx2HardwareGather8,
    Avx2TwoBy8On16,
    Avx2Uniform256TableFree16,
    Avx2Batch4On16,
}

impl BackendId {
    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::RawCopy => "raw-copy",
            Self::RleFill => "rle-fill",
            Self::Scalar8 => "scalar-8way",
            Self::Scalar16 => "scalar-16way",
            Self::Sse41Interleaved8 => "sse41-interleaved-8way",
            Self::Avx512VlInterleaved8 => "avx512vl-interleaved-8way",
            Self::Avx512Interleaved16 => "avx512-interleaved-16way",
            Self::Avx512VlManualGather8 => "avx512vl-manual-gather-8way",
            Self::Avx512ManualGather16 => "avx512-manual-gather-16way",
            Self::Avx512Vl2x8 => "avx512vl-2x8",
            Self::Avx512Batch4 => "avx512-batch4",
            Self::Uniform256TableFree16 => "uniform256-tablefree-16way",
            Self::Avx2ManualGather8 => "avx2-manual-gather-8way",
            Self::Avx2HardwareGather8 => "avx2-hardware-gather-8way",
            Self::Avx2TwoBy8On16 => "avx2-2x8-on16",
            Self::Avx2Uniform256TableFree16 => "avx2-uniform256-tablefree-16way",
            Self::Avx2Batch4On16 => "avx2-batch4-on16",
        }
    }
}

/// Error selection policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorPolicy {
    /// Always return the error from the lowest failing block index.
    /// This is the only deterministic policy.
    LowestBlockIndex,
}

/// SMT/hyper-threading policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtPolicy {
    /// Use all logical processors (SMT siblings included).
    UseAllLogical,
    /// Attempt to use only one thread per physical core.
    /// Falls back to `UseAllLogical` if topology cannot be detected.
    PreferPhysicalEquivalent,
    /// Explicit override — use this many workers regardless.
    Explicit,
}

/// Model construction policy per block or global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPolicy {
    /// Each block builds its own frequency model from its data.
    PerBlock,
    /// All blocks share one model derived from a single reference block.
    /// The reference block index is specified.
    Uniform,
    /// An externally supplied model is used for all blocks.
    External,
    /// A global model is constructed via deterministic histogram merge.
    Global,
}

/// Codec selection policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecPolicy {
    /// Use the best codec for the data (heuristic).
    Auto,
    /// Explicit codec ID.
    Explicit(u16),
}
