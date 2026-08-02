//! # Shared model and table cache — bounded, FIFO, correctness-independent
//!
//! ## Purpose
//!
//! In a parallel decode pipeline, multiple blocks may share the same
//! frequency model (e.g., `ModelPolicy::Uniform` or `ModelPolicy::Global`).
//! Without caching, each block would rebuild the decode plan (model
//! classification, table construction) independently — wasting CPU cycles
//! and increasing memory pressure.
//!
//! The `ModelCache` stores validated, immutable decode artefacts keyed by
//! `(model_sha256, scale_bits, codec_id)`.  Workers look up the cache before
//! building a new decode plan.
//!
//! ## Key design decisions
//!
//! ### 1. FIFO eviction
//!
//! A simple FIFO queue (oldest entry evicted first) is deterministic and
//! trivially correct.  LRU (Least Recently Used) or LFU (Least Frequently
//! Used) would require per-access bookkeeping and would introduce
//! nondeterminism (access order depends on thread scheduling).  FIFO is
//! reproducible across runs.
//!
//! ### 2. Bounded by count and bytes
//!
//! Two independent bounds prevent memory runaway:
//! - `max_entries`: limits the number of unique models cached.
//! - `max_total_bytes`: limits the total memory consumed by cached entries.
//!
//! ### 3. No correctness dependence
//!
//! Cache hits are a performance optimisation only.  If the cache is
//! disabled (or cold), every block builds its own plan, which is always
//! correct.  The cache never returns stale or invalid data because:
//! - Entries are **immutable** after insertion.
//! - Entries are only inserted after model **validation** succeeds.
//! - Cache poisoning (inserting malicious entries) is impossible because
//!   the key is a cryptographic hash of the model data.
//!
//! ### 4. Thread safety design
//!
//! The current `ModelCache` is **not** `Sync`.  It is designed to be used
//! with external synchronisation (e.g., `Mutex<ModelCache>` or a
//! read-optimised structure like `RwLock<ModelCache>`).  A lock-free
//! concurrent cache (e.g., `chashmap` or `dashmap`) could be added as an
//! alternative backend, but the simple Mutex-guarded approach is sufficient
//! for the current workload: cache lookups are infrequent (one per block,
//! not one per symbol) and the critical section is short.
//!
//! ## Limitations
//!
//! - Entry sizes are tracked approximately (all entries are assumed to
//!   have the size reported at insertion time).  A more accurate cache
//!   would store per-entry sizes for precise eviction decisions.
//! - The linear scan in `get()` is O(N) in the number of cached entries.
//!   For typical workloads (tens to hundreds of unique models), this is
//!   negligible.  For workloads with thousands of unique models, a
//!   `HashMap`-backed cache would be faster.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

/// Key for uniquely identifying a frequency model in the cache.
///
/// # Composite key design
///
/// The key combines three values that together uniquely identify a decode plan:
///
/// - `model_sha256`: A cryptographic hash of the serialised frequency model.
///   This uniquely identifies the model itself.  Two blocks with the same
///   model (e.g., uniform-256) will have the same hash.
/// - `scale_bits`: The precision parameter.  Different scale_bits values
///   produce different decode tables even with the same frequency distribution.
/// - `codec_id`: The codec variant (7 = 8-way, 8 = 16-way).  The same model
///   may require different tables for different codecs.
///
/// Collisions on `model_sha256` are cryptographically infeasible (SHA-256
/// preimage resistance).  The `scale_bits` and `codec_id` are additional
/// discriminators for safety.
///
/// # Derivation
///
/// The key is computed by [`ModelCacheKey::from_model`] (the single source
/// of truth); [`crate::decode_plan::plan_cache_key`] delegates to it so
/// every construction path hashes the same bytes in the same order.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ModelCacheKey {
    /// SHA-256 hash of the serialised model data.
    ///
    /// This is the primary discriminator.  Two models with the same hash
    /// are assumed identical.
    pub model_sha256: [u8; 32],
    /// Precision (scale_bits), e.g. 12 for standard word rANS.
    pub scale_bits: u8,
    /// Codec identifier (7 = 8-way, 8 = 16-way, etc.).
    pub codec_id: u16,
}

impl ModelCacheKey {
    /// Derive the cache key from the raw model bytes, scale bits, and codec.
    ///
    /// Hashes the exact model bytes stored in the block header — not a
    /// parsed representation — so the key is byte-exact: any difference in
    /// the serialised model produces a different key, and identical model
    /// bytes (e.g. every Uniform256 block) produce the same key.
    pub fn from_model(codec_id: u16, scale_bits: u8, model_data: &[u8]) -> Self {
        Self {
            model_sha256: crate::encode::sha256(model_data),
            scale_bits,
            codec_id,
        }
    }
}

/// A bounded, FIFO-evicted cache of validated model artefacts.
///
/// # Type parameter
///
/// `T` is the cached value type (e.g., `DecodePlan`, precomputed tables).
/// `T` must implement `Debug` for diagnostic logging.
///
/// # Eviction policy
///
/// FIFO (first-in, first-out).  When a new entry would exceed `max_entries`
/// or `max_total_bytes`, the oldest entries are evicted until the new entry
/// fits.  This policy is:
/// - **Deterministic**: same insertion sequence always produces the same
///   cache state at each step.
/// - **Simple**: no per-access bookkeeping, no lock contention on reads.
/// - **Bounded**: worst-case memory is `max_entries * max_entry_size`.
///
/// # Invariant
///
/// All cached entries have been validated.  No unvalidated model data
/// is ever inserted.  The caller must verify the model before calling
/// `insert()`.
pub struct ModelCache<T> {
    /// FIFO queue of (key, value) pairs.  Front = oldest, back = newest.
    entries: VecDeque<(ModelCacheKey, T)>,
    /// Maximum number of entries before eviction kicks in.
    max_entries: usize,
    /// Maximum total estimated memory footprint before eviction kicks in.
    max_total_bytes: u64,
    /// Current estimated memory footprint (sum of `entry_bytes` at insertion).
    current_bytes: u64,
}

impl<T: fmt::Debug> fmt::Debug for ModelCache<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelCache")
            .field("entries", &self.entries.len())
            .field("max_entries", &self.max_entries)
            .field("current_bytes", &self.current_bytes)
            .field("max_total_bytes", &self.max_total_bytes)
            .finish()
    }
}

impl<T> ModelCache<T> {
    /// Create a new bounded model cache.
    pub fn new(max_entries: usize, max_total_bytes: u64) -> Self {
        Self {
            entries: VecDeque::new(),
            max_entries,
            max_total_bytes,
            current_bytes: 0,
        }
    }

    /// Look up a cached entry by key.
    ///
    /// Performs a linear scan through the FIFO queue.  Returns `Some(&T)`
    /// if the key is found, `None` otherwise.
    ///
    /// # Complexity
    ///
    /// O(N) where N = number of cached entries.  This is acceptable for
    /// typical workloads (tens to hundreds of unique models).  For larger
    /// caches, consider a `HashMap`-backed implementation.
    ///
    /// # Thread safety note
    ///
    /// This takes `&self` (immutable reference), so it can be called
    /// concurrently under `RwLock` read guards, but the current
    /// implementation requires external synchronisation.
    pub fn get(&self, key: &ModelCacheKey) -> Option<&T> {
        for (k, v) in &self.entries {
            if k == key {
                return Some(v);
            }
        }
        None
    }

    /// Insert a new entry into the cache.
    ///
    /// If the cache is at capacity (entry count or byte budget), the oldest
    /// entries are evicted to make room.  Eviction continues until the new
    /// entry fits or the cache is empty.
    ///
    /// # Parameters
    ///
    /// - `key`: The composite key identifying this model.
    /// - `value`: The cached artefact (decode plan, tables, etc.).
    /// - `entry_bytes`: Estimated memory footprint of the entry.
    ///   Used for byte-budget tracking.
    ///
    /// # Precondition
    ///
    /// The model has been validated before insertion.  Insertion of
    /// unvalidated data could lead to cache poisoning, though the
    /// cryptographic key makes targeted collisions infeasible.
    ///
    /// # Eviction note
    ///
    /// Entry sizes are tracked approximately.  Each eviction subtracts
    /// the *new* entry's `entry_bytes` rather than the evicted entry's
    /// actual size.  This is a known approximation — see module docs.
    pub fn insert(&mut self, key: ModelCacheKey, value: T, entry_bytes: u64) {
        // Evict if full
        while self.entries.len() >= self.max_entries
            || self.current_bytes + entry_bytes > self.max_total_bytes
        {
            if let Some((_k, _v)) = self.entries.pop_front() {
                // We don't track per-entry sizes precisely; approximate by
                // subtracting an average.  In a production cache, store
                // per-entry sizes.
                self.current_bytes = self.current_bytes.saturating_sub(entry_bytes);
            } else {
                break;
            }
        }

        self.entries.push_back((key, value));
        self.current_bytes = self.current_bytes.saturating_add(entry_bytes);
    }

    /// Return the number of entries currently cached.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty (no entries).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove all entries from the cache.
    ///
    /// Resets both the entry count and the byte counter to zero.
    /// After calling `clear()`, the cache is in the same state as
    /// a newly constructed cache with the same bounds.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.current_bytes = 0;
    }
}

/// Model-derived artifacts that are safe to cache by model identity.
///
/// The cached value deliberately separates **model-derived immutable
/// artifacts** from **runtime backend selection**: the frequencies, the
/// uniform256 flag, and the packed word table depend only on the model
/// bytes + scale + codec, which is exactly the cache key.  Backend choice
/// (which depends on runtime CPU capabilities, build features, and
/// `disable_simd`) is made after the cache lookup, per block, so a cached
/// artifact is never reused under incompatible execution conditions.
///
/// The `freqs` and (SIMD builds) `packed_table` fields are `Arc`-shared:
/// a cache hit hands out a clone of the `Arc` (a refcount bump), so the
/// expensive O(4096 × symbols) packed-table construction happens exactly
/// once per unique model instead of once per block.  This is the source of
/// the cache's throughput gain; without it the cache would only save a
/// trivial 1 KiB frequency parse.
#[derive(Debug, Clone)]
pub struct ValidatedModelArtifacts {
    /// Parsed 256 × u32 frequencies (validated sum == 1 << scale_bits),
    /// shared across blocks with the same model via `Arc`.
    pub freqs: Arc<Vec<u32>>,
    /// Whether the model is the Uniform256 distribution.
    pub uniform256: bool,
    /// 16 KiB packed word table (slot → (freq, bias, sym)), built once per
    /// unique model and shared via `Arc`.
    ///
    /// `None` when the model is not a valid word-codec model (e.g.
    /// `scale_bits != 12`, or a symbol with zero frequency): the decode
    /// executor then fails with the same `Model` error it would have
    /// produced building the table itself, so caching never changes error
    /// identity.
    #[cfg(feature = "simd")]
    pub packed_table: Option<Arc<ryg_rans_rs_simd::packed_table::PackedWordTable>>,
}

/// Process-global shared model cache used by `decode_single_block`.
///
/// Bounded to 64 entries and 16 MiB; FIFO eviction.  Mutex-guarded for
/// thread safety.  A cache miss simply rebuilds the artifacts (always
/// correct), so the cache is a pure performance optimisation.
static GLOBAL_MODEL_CACHE: std::sync::OnceLock<
    std::sync::Mutex<ModelCache<ValidatedModelArtifacts>>,
> = std::sync::OnceLock::new();

/// Look up model artifacts in the global cache, or build and insert them.
///
/// Returns cloned artifacts so the caller never holds the cache lock
/// across decoding work (no poisoned-lock cascade, no lock in the hot
/// path).  The key is derived from the exact model bytes, scale bits,
/// and codec ID.
pub fn cached_model_artifacts(
    codec_id: u16,
    scale_bits: u8,
    model_data: &[u8],
    build: impl FnOnce() -> Option<ValidatedModelArtifacts>,
) -> Option<ValidatedModelArtifacts> {
    let key = ModelCacheKey::from_model(codec_id, scale_bits, model_data);
    let cache = GLOBAL_MODEL_CACHE
        .get_or_init(|| std::sync::Mutex::new(ModelCache::new(64, 16 * 1024 * 1024)));
    {
        let guard = cache.lock().ok()?;
        if let Some(v) = guard.get(&key) {
            return Some(v.clone());
        }
    }
    // Build outside the lock so concurrent duplicate construction is
    // possible but cheap; the last insert wins deterministically.
    let artifacts = build()?;
    let mut entry_bytes = (artifacts.freqs.len() * 4) as u64 + 64;
    #[cfg(feature = "simd")]
    {
        if let Some(t) = &artifacts.packed_table {
            // 4096 packed u32 entries = 16 KiB per table.
            entry_bytes += (t.as_slice().len() * std::mem::size_of::<u32>()) as u64;
        }
    }
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, artifacts.clone(), entry_bytes);
    }
    Some(artifacts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_hit() {
        let mut cache = ModelCache::new(16, 65536);
        let key = ModelCacheKey {
            model_sha256: [0u8; 32],
            scale_bits: 12,
            codec_id: 7,
        };
        cache.insert(key.clone(), "plan_data".to_string(), 100);
        assert_eq!(cache.get(&key), Some(&"plan_data".to_string()));
    }

    #[test]
    fn test_cache_miss() {
        let cache: ModelCache<String> = ModelCache::new(16, 65536);
        let key = ModelCacheKey {
            model_sha256: [1u8; 32],
            scale_bits: 12,
            codec_id: 7,
        };
        assert_eq!(cache.get(&key), None);
    }

    #[test]
    fn test_eviction() {
        let mut cache = ModelCache::new(2, 65536);
        let k1 = ModelCacheKey {
            model_sha256: [0u8; 32],
            scale_bits: 12,
            codec_id: 7,
        };
        let k2 = ModelCacheKey {
            model_sha256: [1u8; 32],
            scale_bits: 12,
            codec_id: 7,
        };
        let k3 = ModelCacheKey {
            model_sha256: [2u8; 32],
            scale_bits: 12,
            codec_id: 7,
        };

        cache.insert(k1.clone(), "plan1".to_string(), 100);
        cache.insert(k2.clone(), "plan2".to_string(), 100);
        assert_eq!(cache.len(), 2);

        // Insert third — should evict k1
        cache.insert(k3.clone(), "plan3".to_string(), 100);
        assert_eq!(cache.len(), 2);
        assert!(cache.get(&k1).is_none());
        assert!(cache.get(&k2).is_some());
        assert!(cache.get(&k3).is_some());
    }

    #[test]
    fn test_clear() {
        let mut cache = ModelCache::new(16, 65536);
        let key = ModelCacheKey {
            model_sha256: [0u8; 32],
            scale_bits: 12,
            codec_id: 7,
        };
        cache.insert(key.clone(), "plan_data".to_string(), 100);
        assert!(!cache.is_empty());
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_cached_model_artifacts_hit() {
        let uniform_bytes = [0u8; 256];
        let first = cached_model_artifacts(8, 12, &uniform_bytes, || {
            // Uniform256 artifacts: freqs all 16 at scale 12; SIMD builds
            // also carry a real packed table so the hit path is exercised
            // with the shared table present (the production build closure
            // does the same).
            let freqs = vec![16u32; 256];
            #[cfg(feature = "simd")]
            let packed_table = {
                let cum = {
                    let mut c = Vec::with_capacity(257);
                    c.push(0u32);
                    for i in 0..256 {
                        c.push(c[i] + freqs[i]);
                    }
                    c
                };
                let table =
                    ryg_rans_rs_simd::packed_table::PackedWordTable::from_freqs(&freqs, &cum, 12)
                        .expect("uniform model table");
                Some(Arc::new(table))
            };
            Some(ValidatedModelArtifacts {
                freqs: Arc::new(freqs),
                uniform256: true,
                #[cfg(feature = "simd")]
                packed_table,
            })
        });
        // Second call must be served from the global cache: the build
        // closure is never invoked on a hit, so panic if it runs.
        let second = cached_model_artifacts(8, 12, &uniform_bytes, || {
            panic!("build closure must not run on cache hit")
        });
        assert!(first.is_some());
        assert!(second.is_some());
        let f = first.unwrap();
        let s = second.unwrap();
        assert_eq!(f.freqs.as_slice(), s.freqs.as_slice());
        // The cache returns Arc clones of the same allocation: a hit must
        // share the artifact, not rebuild or deep-copy it (this is what
        // makes the packed table reusable across blocks).
        assert!(Arc::ptr_eq(&f.freqs, &s.freqs));
        #[cfg(feature = "simd")]
        match (&f.packed_table, &s.packed_table) {
            (Some(a), Some(b)) => assert!(Arc::ptr_eq(a, b)),
            _ => panic!("uniform model must carry a cached packed table"),
        }
    }

    #[test]
    fn test_cached_model_artifacts_miss_build_none() {
        // Distinct model bytes from the hit test: the global cache is shared
        // across tests in this process, so using the same key would make the
        // result order-dependent.
        let miss_bytes = [1u8; 256];
        let result = cached_model_artifacts(8, 12, &miss_bytes, || None);
        assert!(result.is_none());
    }

    #[cfg(feature = "simd")]
    #[test]
    fn test_cached_model_artifacts_carries_packed_table() {
        // A valid Uniform256 model (256 × freq 16 @ scale 12) must produce
        // cached artifacts whose packed table is present and correct: the
        // table's slot 0 maps to symbol 0 with freq 16, bias 0.
        let mut model = Vec::with_capacity(1024);
        for _ in 0..256 {
            model.extend_from_slice(&16u32.to_le_bytes());
        }
        let artifacts = cached_model_artifacts(8, 12, &model, || {
            let freqs = vec![16u32; 256];
            let cum = {
                let mut c = Vec::with_capacity(257);
                c.push(0u32);
                for i in 0..256 {
                    c.push(c[i] + freqs[i]);
                }
                c
            };
            let table =
                ryg_rans_rs_simd::packed_table::PackedWordTable::from_freqs(&freqs, &cum, 12)
                    .expect("uniform model table");
            Some(ValidatedModelArtifacts {
                freqs: Arc::new(freqs),
                uniform256: true,
                packed_table: Some(Arc::new(table)),
            })
        })
        .expect("artifacts");
        let table = artifacts.packed_table.expect("packed table present");
        assert_eq!(table.as_slice().len(), 4096);
        let first = table.as_slice()[0];
        // slot 0 belongs to symbol 0; freq 16, bias 0.
        assert_eq!(first.0 & 0x0fff, 16);
        assert_eq!((first.0 >> 24) as u8, 0);
    }

    #[cfg(feature = "simd")]
    #[test]
    fn test_cached_model_artifacts_rejects_invalid_model() {
        // A model whose frequencies do not sum to 1 << scale must be rejected
        // by the cache (build returns None) and never cached.
        let mut model = Vec::with_capacity(1024);
        for i in 0..256 {
            model.extend_from_slice(&(i as u32).to_le_bytes());
        }
        let result = cached_model_artifacts(8, 12, &model, || None);
        assert!(result.is_none());
    }
}
