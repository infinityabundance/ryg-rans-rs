//! # Parallel configuration — thread count, queue bounds, backend, error policy
//!
//! All configuration is explicit.  No hidden global state, no ambient
//! environment variables consulted at runtime (except those directly
//! invoked by `std::thread::available_parallelism()`).
//!
//! ## Determinism invariant
//!
//! Thread count, backend selection, and SMT policy must **never** change
//! canonical output or error selection.  Two runs with differing thread
//! counts on the same input produce identical results (though possibly
//! at different speeds).  This invariant is enforced by the block planning
//! layer (which depends only on input length and block size) and the error
//! policy (which always selects the lowest-index failing block).
//!
//! ## Default value rationale
//!
//! | Field | Default | Rationale |
//! |-------|---------|-----------|
//! | `threads` | `AvailableParallelism` | No reason to limit parallelism by default; the OS knows its own topology. |
//! | `max_in_flight_blocks` | 16 | Balances pipeline depth against memory.  Fewer than 8 under-utilises workers; more than 64 risks excessive buffering with large blocks. |
//! | `max_buffered_input_bytes` | 256 MiB | Conservative for consumer hardware.  Fits within typical cgroup/container limits. |
//! | `max_buffered_output_bytes` | 512 MiB | Decoded data is typically 2–3× compressed size; this headroom prevents stalls. |
//! | `parallel_threshold_bytes` | 1 MiB | Below this, sequential processing is faster — thread spawn + queue overhead exceeds gains. |
//! | `affinity` | `None` | Affinity is a niche optimisation; wrong settings hurt NUMA performance. |
//! | `backend_policy` | `Portable` | Correctness-first: scalar kernels are the most thoroughly tested. |
//! | `error_policy` | `LowestBlockIndex` | The only policy that guarantees deterministic error reporting. |
//! | `worker_stack_size` | `None` | System default (typically 2 MiB on Linux) is adequate for rANS decode. |
//! | `disable_inner_batching` | `false` | Batching improves throughput; disabling is a debug escape hatch. |
//! | `disable_simd` | `false` | SIMD provides large speedups; disabling is for validation only. |
//! | `smt_policy` | `UseAllLogical` | SMT often improves throughput on mixed-workload decode pipelines. |
//!
//! ## BackendPolicy decision tree
//!
//! ```text
//! BackendPolicy::Portable
//!   └─ Always select Scalar8 or Scalar16 — no runtime feature detection.
//!      Maximum portability, minimum performance.
//!
//! BackendPolicy::ScalarPreferred
//!   └─ Like Portable but with an explicit intent signal.
//!      (Currently identical to Portable; reserved for future dispatch heuristics.)
//!
//! BackendPolicy::Auto
//!   └─ Conservative dispatch: scalar-first until multi-machine benchmarks
//!      establish architecture-specific crossover points.  Currently falls
//!      through to Portable behaviour.
//!
//! BackendPolicy::ModelAware
//!   └─ Inspect the symbol frequency model of each block.  For uniform-256
//!      distributions (all frequencies == 16), select the table-free 16-way
//!      kernel.  For skewed distributions, fall back to scalar.  This policy
//!      can select different backends for different blocks in the same run.
//!
//! BackendPolicy::Explicit(BackendId)
//!   └─ Override all heuristics.  The specified backend is used for every
//!      block.  Useful for benchmarking or targeting specific hardware.
//! ```
//!
//! ## Safety
//!
//! All configuration values are validated at executor start.  Invalid
//! combinations produce a `ParallelError::Config` before any worker
//! threads are spawned.  This is a fail-fast design — no partial work
//! to discard.

use std::num::NonZeroUsize;
use std::vec::Vec;

/// Top-level parallel engine configuration.
///
/// # Invariant
///
/// All fields are validated by `executor::validate_config()` before any
/// thread spawns.  At that point the config is frozen — no field changes
/// are visible once execution begins.
///
/// # Relationship to output
///
/// Only `error_policy` and `backend_policy` (indirectly via the decode
/// plan) can affect the bytes returned to the caller.  Thread count,
/// buffer sizes, affinity, SMT, and stack size affect *performance* and
/// *resource usage* but never *correctness* or *canonical error selection*.
#[derive(Debug, Clone)]
pub struct ParallelConfig {
    /// Number of worker threads to spawn in the executor pool.
    ///
    /// This controls concurrency, not block boundaries.  Two runs with
    /// different `threads` values produce identical output on the same
    /// input — only wall-clock time differs.
    pub threads: ThreadCount,
    /// Maximum number of blocks queued for processing (per operation direction).
    ///
    /// This is the primary backpressure control.  When in-flight blocks
    /// reach this limit, producers (the input reader or upstream stage)
    /// must wait before submitting more work.
    ///
    /// Too low: workers starve, throughput drops.
    /// Too high: memory grows proportionally; large blocks amplify the cost.
    pub max_in_flight_blocks: NonZeroUsize,
    /// Maximum total input bytes buffered across all in-flight blocks.
    ///
    /// A secondary backpressure signal: even if the block-count limit
    /// has not been reached, insertion is rejected if this byte budget
    /// would be exceeded.  This prevents a single enormous block from
    /// consuming the entire queue budget.
    pub max_buffered_input_bytes: u64,
    /// Maximum total decoded (or encoded) bytes buffered across all completed
    /// but uncommitted blocks.
    ///
    /// The reorder buffer holds finished results waiting for their turn in
    /// index order.  This cap prevents a slow early block from causing
    /// unbounded memory growth as later blocks finish out of order.
    pub max_buffered_output_bytes: u64,
    /// Minimum total input size (in bytes) before parallelism is engaged.
    ///
    /// Below this threshold, the executor falls back to sequential
    /// single-threaded processing on the calling thread.  The threshold
    /// is calibrated to break even against thread-spawn overhead
    /// (typically ~100–500 µs for pool activation).
    pub parallel_threshold_bytes: u64,
    /// CPU affinity policy (optional, best-effort optimisation).
    ///
    /// Affinity is **never** required for correctness.  On NUMA systems,
    /// `Compact` or `Spread` may improve cache locality.  On systems with
    /// aggressive frequency scaling, pinning can reduce latency jitter.
    /// Affinity failures are silently ignored (the executor logs a warning
    /// and continues).
    pub affinity: AffinityPolicy,
    /// Backend selection policy for inner decode kernels.
    ///
    /// Controls which implementation of the rANS decode inner loop is
    /// used.  See `BackendPolicy` enum docs for the full decision tree.
    pub backend_policy: BackendPolicy,
    /// Optional per-worker stack size, in bytes.
    ///
    /// `None` uses the Rust runtime default (typically 2 MiB on Linux).
    /// rANS decode uses minimal stack (~4–8 KiB per call frame), so the
    /// default is almost always sufficient.  Raise this only if the
    /// frequency model construction uses deep recursion.
    pub worker_stack_size: Option<usize>,
    /// Whether to disable all SIMD-accelerated decode kernels.
    ///
    /// When `true`, the execution planner will never select an SIMD
    /// decode plan, even if the `BackendPolicy` is `Explicit(simd_backend)`.
    /// An explicit SIMD request combined with `disable_simd = true` is a
    /// config conflict and is rejected with a typed error before execution.
    pub disable_simd: bool,
    /// SMT/hyper-threading policy for worker placement.
    ///
    /// Controls whether logical processors (SMT siblings) are counted
    /// when determining the effective worker count.  On CPUs with SMT,
    /// `UseAllLogical` will spawn up to 2× the physical core count,
    /// which may increase throughput for memory-bound workloads but
    /// can hurt compute-bound workloads due to resource contention.
    pub smt_policy: SmtPolicy,
    /// Integrity verification policy for decoded-output hashes.
    ///
    /// Defaults to [`IntegrityPolicy::Strict`]: a zero/unset stored
    /// decoded hash fails the block (`DecodedHashMissing`) and a nonzero
    /// mismatch fails with `DecodedHashMismatch`.  Set
    /// `AllowLegacyUnsetDecodedHash` only when verifying containers
    /// produced by encoders that predate decoded-hash storage.
    pub integrity_policy: IntegrityPolicy,
}

impl Default for ParallelConfig {
    fn default() -> Self {
        Self {
            threads: ThreadCount::AvailableParallelism,
            // 16 is a nonzero constant — the unwrap is provably infallible.
            max_in_flight_blocks: NonZeroUsize::new(16).unwrap(),
            max_buffered_input_bytes: 256 * 1024 * 1024, // 256 MiB
            max_buffered_output_bytes: 512 * 1024 * 1024, // 512 MiB
            parallel_threshold_bytes: 1024 * 1024,       // 1 MiB
            affinity: AffinityPolicy::None,
            backend_policy: BackendPolicy::Portable,
            worker_stack_size: None,
            disable_simd: false,
            smt_policy: SmtPolicy::UseAllLogical,
            integrity_policy: IntegrityPolicy::Strict,
        }
    }
}

/// How many worker threads to use in the executor pool.
///
/// # Clamping
///
/// The requested count is clamped at executor initialisation:
/// - Minimum: 1 (a count of 0 is a configuration error).
/// - Maximum: `total_blocks` (there is no point spawning more workers
///   than blocks to process).
///
/// Clamping is silent — it does not return an error.  The actual worker
/// count is readable from the executor report's `effective_workers`.
///
/// # Determinism guarantee
///
/// Different `ThreadCount` values produce identical output bytes.
/// They may produce different timing, different intermediate buffer
/// pressure, and (rarely) different non-canonical errors, but the
/// canonical result is invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCount {
    /// Use exactly N worker threads.
    ///
    /// The executor will attempt to spawn N workers.  If N exceeds
    /// the block count, the worker count is clamped to the block count.
    Exact(NonZeroUsize),
    /// Use `std::thread::available_parallelism()` to determine count.
    ///
    /// On Linux this reads `/sys/devices/system/cpu/possible` or
    /// `sched_getaffinity()`.  On other platforms it uses the OS
    /// equivalent.  Falls back to 1 if the call fails.
    AvailableParallelism,
}

/// CPU affinity policy for worker thread placement.
///
/// All affinity operations are best-effort.  If the underlying OS call
/// fails (e.g., permissions, invalid CPU index), the error is logged
/// but execution continues.  Affinity **never** affects correctness.
///
/// # NUMA considerations
///
/// - `Compact` is often best for single-socket systems: workers stay
///   close to their cache lines.
/// - `Spread` is often best for multi-socket systems: work is distributed
///   across NUMA domains, balancing memory bandwidth.
/// - `None` lets the kernel scheduler decide, which is usually optimal
///   for short-lived workloads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AffinityPolicy {
    /// Do not set any CPU affinity.
    ///
    /// The kernel scheduler is free to migrate workers across CPUs.
    None,
    /// Pack workers onto consecutive logical CPUs starting from CPU 0.
    ///
    /// Worker i is pinned to CPU (i % total_cpus).  This maximises
    /// cache reuse when workers process adjacent blocks.
    Compact,
    /// Spread workers evenly across available CPUs.
    ///
    /// Worker i is pinned to CPU ((i * stride) % total_cpus) where
    /// stride = total_cpus / worker_count.  This minimises contention
    /// on shared cache levels.
    Spread,
    /// Explicit CPU-index list.  Length must equal the effective worker count.
    ///
    /// Worker i is pinned to `cpus[i]`.  Validation occurs at executor
    /// start; mismatched lengths produce a `Config` error.
    Explicit(Vec<usize>),
}

/// Backend selection policy for inner decode kernels.
///
/// This enum controls the decision tree that maps a block's model
/// and available CPU features to a specific `BackendId`.  See the
/// module-level documentation for the full decision tree diagram.
///
/// # Performance vs portability spectrum
///
/// ```text
/// Portable ── ScalarPreferred ── Auto ── ModelAware ── Explicit
///  (safe)                                     (fast)     (risky)
/// ```
///
/// Moving right on this spectrum trades portability for potential
/// throughput gains.  `Explicit` bypasses all feature detection and
/// may produce illegal-instruction faults on unsupported hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendPolicy {
    /// Always select portable scalar kernels.
    ///
    /// No runtime CPU feature detection is performed.  Every block
    /// uses either `Scalar8` (codec 7) or `Scalar16` (codec 8).
    /// This is the safest policy for maximum compatibility.
    Portable,
    /// Prefer scalar kernels even when SIMD is available.
    ///
    /// Currently identical to `Portable`.  Future versions may use
    /// this to enable scatter-gather prefetch without full SIMD.
    ScalarPreferred,
    /// Auto-detect the best available backend using CPU feature flags.
    ///
    /// The current implementation is conservative: it falls through to
    /// scalar kernels.  Future releases will add AVX2 and AVX-512
    /// dispatch paths as multi-machine benchmarks establish crossover
    /// points.
    Auto,
    /// Use exactly the specified backend for every block.
    ///
    /// No feature detection, no fallback.  If the backend requires
    /// an instruction set extension not available on the host CPU,
    /// the process will crash with SIGILL.  This is intended for
    /// benchmarking on known hardware.
    Explicit(BackendId),
    /// Select backend per block based on frequency model analysis.
    ///
    /// Inspects each block's symbol distribution.  A uniform-256
    /// distribution (every frequency == 16) enables the table-free
    /// 16-way kernel, which is significantly faster.  Skewed
    /// distributions fall back to scalar.  Different blocks in
    /// the same run may use different backends.
    ModelAware,
}

/// Identifies a specific decode kernel or backend implementation.
///
/// # Variant semantics (16 variants)
///
/// | Variant | Width | ISA | Strategy |
/// |---------|-------|-----|----------|
/// | `RawCopy` | — | — | No decoding; memcpy-only passthrough for already-uncompressed data. |
/// | `RleFill` | — | — | Run-length encode fill: repeat a single symbol N times. |
/// | `Scalar8` | 8-way | Scalar | Word rANS, 8 lanes, portable C/Rust. The baseline for codec_id == 7. |
/// | `Scalar16` | 16-way | Scalar | Word rANS, 16 lanes, portable. The baseline for codec_id == 8. |
/// | `Sse41Interleaved8` | 8-way | SSE4.1 | Interleaved 8-way decode using SSE4.1 packed operations. |
/// | `Avx512VlInterleaved8` | 8-way | AVX-512 VL | 256-bit AVX-512VL interleaved 8-way. |
/// | `Avx512Interleaved16` | 16-way | AVX-512 | 512-bit AVX-512 interleaved 16-way. |
/// | `Avx512VlManualGather8` | 8-way | AVX-512 VL | Manual gather (explicit permute) 8-way on 256-bit. |
/// | `Avx512ManualGather16` | 16-way | AVX-512 | Manual gather 16-way on 512-bit. |
/// | `Avx512Vl2x8` | 2×8-on-16 | AVX-512 VL | Two 8-way streams interleaved into 16-way format. |
/// | `Avx512Batch4` | batch-4 | AVX-512 | Process 4 u16 words at once, 16 streams. |
/// | `Uniform256TableFree16` | 16-way | Scalar (opt.) | Table-free decode when all 256 frequencies are 16. Uses `slot / 16` arithmetic — no freq table needed. |
/// | `Avx2ManualGather8` | 8-way | AVX2 | Manual gather via VPERMD for 8-way decode. |
/// | `Avx2HardwareGather8` | 8-way | AVX2 | Hardware gather (VGATHERDPS) for 8-way decode. |
/// | `Avx2TwoBy8On16` | 2×8-on-16 | AVX2 | Two 8-way streams interleaved on 16-way format, AVX2. |
/// | `Avx2Uniform256TableFree16` | 16-way | AVX2 | Uniform256 table-free decode accelerated with AVX2. |
/// | `Avx2Batch4On16` | batch-4 | AVX2 | 4-at-once batch decode on 16-way format, AVX2. |
///
/// # Width terminology
///
/// - **N-way**: The inner loop processes N interleaved rANS streams
///   simultaneously.  Higher N gives more ILP but requires more
///   registers and a wider SIMD datapath.
/// - **batch-N**: Process N consecutive u16 words from each stream
///   in a single SIMD operation, reducing instruction-count per symbol.
/// - **2×8-on-16**: Decode two independent 8-way blocks but emit
///   their results in 16-way interleaved format for sequential output.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum BackendId {
    /// No decoding — memcpy-only passthrough.
    RawCopy,
    /// RLE fill from a single symbol value.
    RleFill,
    /// Scalar (portable) 8-way word rANS.
    Scalar8,
    /// Scalar (portable) 16-way word rANS.
    Scalar16,
    /// SSE4.1 interleaved 8-way.
    Sse41Interleaved8,
    /// AVX-512 VL (256-bit) interleaved 8-way.
    Avx512VlInterleaved8,
    /// AVX-512 (512-bit) interleaved 16-way.
    Avx512Interleaved16,
    /// AVX-512 VL manual gather 8-way.
    Avx512VlManualGather8,
    /// AVX-512 manual gather 16-way.
    Avx512ManualGather16,
    /// AVX-512 VL 2×8-on-16-way decode.
    Avx512Vl2x8,
    /// AVX-512 batch-4 decode.
    Avx512Batch4,
    /// Table-free 16-way decode for uniform-256 distributions.
    Uniform256TableFree16,
    /// AVX2 manual gather 8-way.
    Avx2ManualGather8,
    /// AVX2 hardware gather 8-way.
    Avx2HardwareGather8,
    /// AVX2 2×8-on-16-way decode.
    Avx2TwoBy8On16,
    /// AVX2 uniform-256 table-free 16-way decode.
    Avx2Uniform256TableFree16,
    /// AVX2 batch-4 on 16-way format.
    Avx2Batch4On16,
}

impl BackendId {
    /// Return a human-readable label for this backend.
    ///
    /// These labels are used in execution telemetry (`backend_counts`).
    /// and for diagnostic logging.  They are kebab-case, stable across
    /// releases, and should not be parsed programmatically (use the
    /// enum itself for dispatch).
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

/// Error selection policy for choosing among concurrent block failures.
///
/// # Determinism requirement
/// Error selection is **fixed** to the lowest failing block index.
///
/// Multiple blocks can fail concurrently.  Without a deterministic
/// selection policy, the returned error would depend on the order
/// workers finish, which is nondeterministic.  This would break the
/// core invariant of the parallel engine: same input → same output.
///
/// `LowestBlockIndex` is the **only** policy that guarantees
/// deterministic error reporting, implemented by `CanonicalErrorTracker`.
/// It is therefore not configurable — configuration theater with a
/// single option was removed in Phase L.

/// SMT/hyper-threading policy for determining effective worker count.
///
/// # Background
///
/// Simultaneous multithreading (SMT, a.k.a. Hyper-Threading) presents
/// each physical core as multiple logical processors.  Using all logical
/// processors can increase throughput for memory-bound workloads but
/// may decrease per-thread performance for compute-bound workloads due
/// to shared execution units.
///
/// # Application to thread count
///
/// This policy is consulted when `ThreadCount::AvailableParallelism`
/// is selected.  The raw count from the OS is adjusted:
///
/// - `UseAllLogical`: use the raw count directly.
/// - `PreferPhysicalEquivalent`: attempt to halve the count to
///    approximate physical core count.  Falls back `UseAllLogical`
///    if topology detection fails.
/// - `Explicit`: ignore OS count entirely and use a caller-specified
///    value (set via the `threads` field directly).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtPolicy {
    /// Count all logical processors, including SMT siblings.
    ///
    /// This maximises throughput for mixed-workload decode pipelines
    /// where memory latency is often the bottleneck.
    UseAllLogical,
    /// Attempt to count only one thread per physical core.
    ///
    /// If `available_parallelism()` returns 16 on an 8-core/16-thread
    /// CPU, this policy divides by 2 to get 8 workers.  Falls back
    /// to `UseAllLogical` if topology cannot be reliably detected.
    PreferPhysicalEquivalent,
    /// Explicit override — ignore OS topology entirely.
    ///
    /// The worker count is determined solely by `ThreadCount`.
    /// `AvailableParallelism` with this policy returns the raw OS
    /// count without SMT adjustment.
    Explicit,
}

/// Integrity verification policy.
///
/// Controls how decoded-output hashes are treated during verification.
///
/// - [`Strict`](IntegrityPolicy::Strict) (default for `verify`, CLI
///   verification, forensic courts, and evidence generation): a stored
///   decoded hash that is zero/unset fails the block with
///   `DecodedHashMissing`.  A stored nonzero decoded hash that does not
///   match the recomputed hash fails with `DecodedHashMismatch`.  Only a
///   matching nonzero decoded hash passes.
/// - [`AllowLegacyUnsetDecodedHash`](IntegrityPolicy::AllowLegacyUnsetDecodedHash):
///   compatibility mode for containers produced by older encoders that
///   did not store decoded hashes.  A zero/unset decoded hash is reported
///   as `Unset` but does not by itself fail the block.  A nonzero hash
///   mismatch still fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IntegrityPolicy {
    /// Default.  Fail on missing (zero) or mismatched decoded hashes.
    #[default]
    Strict,
    /// Allow legacy containers with unset (zero) decoded hashes.
    AllowLegacyUnsetDecodedHash,
}

/// Outcome of comparing a recomputed SHA-256 against a stored value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HashVerification {
    /// Recomputed hash equals the stored hash.
    Match,
    /// Recomputed hash differs from a nonzero stored hash.
    Mismatch,
    /// Stored hash is all zeros — no value was recorded.
    Unset,
    /// No hash was computed (e.g. decode failed before hashing).
    NotComputed,
}

/// Model construction policy — determines how symbol frequency models
/// are built for each block.
///
/// # Trade-offs
///
/// | Policy | Compression | Memory | Speed | Use case |
/// |--------|-------------|--------|-------|----------|
/// | `PerBlock` | Best | Highest | Slowest | Heterogeneous data, per-block specialisation |
/// | `Uniform` | Good (for similar data) | Shared | Fast | Many similar blocks (e.g., sensor data) |
/// | `External` | Caller-dependent | External | Fastest | Pre-analysed data, streaming |
/// | `Global` | Good | Single model | Fast | Homogeneous data, single histogram |
///
/// The model policy interacts with the `BackendPolicy`: a `Uniform256`
/// model enables the table-free 16-way decode kernel, which is ~2×
/// faster than the general scalar path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPolicy {
    /// Each block constructs its own frequency model from its raw data.
    ///
    /// This produces optimal compression for each block individually
    /// but requires per-block histogram construction, which is expensive
    /// for small blocks.
    PerBlock,
    /// All blocks share one frequency model derived from a single
    /// reference block.
    ///
    /// The reference block index is specified externally.  All other
    /// blocks reuse its model, saving histogram time.  Best when
    /// blocks contain similar data (e.g., consecutive frames in a
    /// video stream).
    Uniform,
    /// Use an externally supplied model for all blocks.
    ///
    /// The model is provided by the caller and must be validated
    /// before use.  This is the fastest path — no histogram work
    /// at all — and is appropriate when the data distribution is
    /// known a priori.
    External,
    /// Construct a single global model via deterministic histogram
    /// merge across all blocks.
    ///
    /// Each block computes its own histogram; then all histograms
    /// are merged deterministically.  This produces a single model
    /// that approximates the global distribution.  The merge order
    /// is fixed (ascending block index) to ensure determinism.
    Global,
}

/// Codec selection policy — determines which rANS variant to use.
///
/// Codec IDs map to specific rANS algorithm variants:
/// - 7: 8-way word rANS (scalar baseline)
/// - 8: 16-way word rANS (scalar baseline)
/// - Future: tANS, rCLFE, etc.
///
/// The codec determines the number of interleaved streams and the
/// format of the compressed data.  Different codecs are not
/// interoperable — data compressed with codec 7 must be decoded
/// with codec 7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodecPolicy {
    /// Automatically select the best codec based on data heuristics.
    ///
    /// The heuristic examines input size and entropy to choose
    /// between 8-way and 16-way encoding.  Small or low-entropy
    /// blocks favour 8-way (lower overhead); large or high-entropy
    /// blocks favour 16-way (higher throughput).
    Auto,
    /// Use an explicit codec ID.
    ///
    /// Must be a recognised codec ID (currently 7 or 8).
    /// Unknown IDs produce a `Config` error at validation time.
    Explicit(u16),
}
