//! # Shared model-cache workload construction (Phase O.14/O.15)
//!
//! The Criterion bench (`benches/model_cache.rs`), the allocation
//! instrumented binary (`bin/model_cache_alloc.rs`), and the contention
//! harness (`bin/model_cache_contention.rs`) must all build the *same*
//! workload classes from the *same* construction code — a duplicated
//! construction path would let the measurements drift apart and invalidate
//! cross-references between the performance receipts.
//!
//! ## The dominant-symbol generator (why it exists)
//!
//! The encoder builds each block's model from the block's own bytes
//! (`ModelPolicy::PerBlock`), so the only way to make blocks share a model
//! is to make their bytes identical.  An earlier generator attempted to
//! skew by remapping symbols that already mapped to themselves — a no-op
//! that produced only 9 distinct models from 16 intended skews (measured
//! during Phase O.14).  The generator here forces a **dominant symbol**
//! (50% of bytes) per skew, so two skews always produce different
//! histograms and therefore different models.  Same skew + same seed →
//! byte-identical blocks → one shared model.
//!
//! ## Block-count policy
//!
//! `blocks_per_case` keeps each case's logical volume bounded (8–32
//! blocks), while the hot-set and thrash modes need enough blocks to
//! exercise their working sets: hot-set ≥ 16 (16 distinct models) and
//! thrash ≥ 18 (17 models against a 16-slot cache) so evictions are
//! provable at every block size.
//!
//! ## Honest mode semantics (Phase O.13)
//!
//! Every mode is labeled exactly by what it measures: `cold`/`warm` share
//! one model, `hot-set` cycles 16 models in a 64-slot cache (no evictions),
//! `thrash` cycles 17 models in a 16-slot cache (FIFO churn), `unique`
//! gives every block a distinct model (zero reuse), `disabled` bypasses
//! the cache entirely.  Nothing is presented as a naturally-occurring
//! distribution; the public-corpus group covers natural reuse.

use ryg_rans_rs_parallel::{
    CodecPolicy, DecodeBlockJob, EncodeBlockJob, ModelArtifactCache, ModelPolicy, ParallelConfig,
    ParallelEncoder, ThreadCount,
};
use std::num::NonZeroUsize;

/// Build a valid 1024-byte model whose symbol `s` has the given frequency
/// skew.  `skew == 0` yields the uniform256 model (all 16s).
///
/// NOTE: this vector is raw model *input material* for the construction
/// microbenchmarks; the e2e modes instead rely on the per-block histogram
/// of the block's own bytes (see [`encode_case`]).
pub fn model_bytes(skew: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024);
    for sym in 0..256u32 {
        let f: u32 = if skew == 0 {
            16
        } else if sym == skew % 256 {
            17
        } else if sym == (skew + 1) % 256 {
            15
        } else {
            16
        };
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

/// The cache mode's deterministic metric proof (Phase O.14).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheMode {
    Disabled,
    Cold,
    Warm,
    HotSet,
    Thrash,
    Unique,
}

impl CacheMode {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Cold => "cold",
            Self::Warm => "warm",
            Self::HotSet => "hot-set",
            Self::Thrash => "thrash",
            Self::Unique => "unique",
        }
    }
}

/// Block count per e2e case (see module docs for the hot-set/thrash floors).
pub fn blocks_per_case(size: usize, mode: CacheMode) -> usize {
    let base = match size {
        s if s <= 262_144 => 32,
        1_048_576 => 16,
        _ => 8,
    };
    match mode {
        CacheMode::HotSet => base.max(16),
        CacheMode::Thrash => base.max(18),
        _ => base,
    }
}

/// The cache configuration for a mode: thrash deliberately uses a 16-slot
/// cache (17 cycling models guarantee FIFO churn, Phase O.12-F); every
/// other cached mode uses 64 slots.
pub fn cache_for(mode: CacheMode) -> std::sync::Arc<ModelArtifactCache> {
    match mode {
        CacheMode::Disabled => ModelArtifactCache::disabled(),
        CacheMode::Thrash => ModelArtifactCache::bounded(16, 16 * 1024 * 1024),
        _ => ModelArtifactCache::bounded(64, 16 * 1024 * 1024),
    }
}

/// The decode configuration used by the e2e workloads.
pub fn config_for(workers: usize) -> ParallelConfig {
    ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(workers.max(1)).unwrap()),
        parallel_threshold_bytes: 0,
        // The workloads measure cache behavior, not the output memory bound:
        // raise the budgets so large sizes / many blocks never trip a
        // ResourceLimit during a decode.
        max_buffered_output_bytes: 4 << 30,
        max_buffered_input_bytes: 4 << 30,
        // Reorder window = max_in_flight.max(workers) + workers must exceed
        // the largest case (18 thrash blocks at 32 workers).
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
        ..Default::default()
    }
}

/// Encode `block_count` blocks of `size` bytes with the given model-skew
/// per block (`model_for_block(i)` selects the model; `None` → a natural
/// per-block model from distinct xorshift data).
///
/// Returns (decode jobs, concatenated source).  See the module docs for the
/// dominant-symbol rationale.
pub fn encode_case(
    size: usize,
    block_count: usize,
    model_for_block: impl Fn(usize) -> Option<u32>,
) -> (Vec<DecodeBlockJob>, Vec<u8>) {
    let mut jobs = Vec::with_capacity(block_count);
    let mut source = Vec::with_capacity(size * block_count);
    let config = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    for i in 0..block_count {
        let data = match model_for_block(i) {
            Some(skew) => {
                let mut d = Vec::with_capacity(size);
                let mut s = (skew as u64 + 1) | 1;
                for _ in 0..size {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    let r = s % 100;
                    let sym: u8 = if r < 50 {
                        (skew % 256) as u8 // dominant symbol: distinct per skew
                    } else if r < 55 {
                        ((skew + 1) % 256) as u8
                    } else {
                        (s % 256) as u8
                    };
                    d.push(sym);
                }
                d
            }
            None => {
                let mut d = Vec::with_capacity(size);
                let mut s = (i as u64 + 1) | 1;
                for _ in 0..size {
                    s ^= s << 13;
                    s ^= s >> 7;
                    s ^= s << 17;
                    d.push((s & 0xff) as u8);
                }
                d
            }
        };
        source.extend_from_slice(&data);
        jobs.push(EncodeBlockJob::new(
            i as u64,
            data,
            CodecPolicy::Auto,
            ModelPolicy::PerBlock,
            12,
        ));
    }
    let enc = ParallelEncoder::encode_blocks(jobs, &config).expect("encode");
    let decode_jobs: Vec<DecodeBlockJob> = enc
        .blocks
        .into_iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block,
        })
        .collect();
    (decode_jobs, source)
}
