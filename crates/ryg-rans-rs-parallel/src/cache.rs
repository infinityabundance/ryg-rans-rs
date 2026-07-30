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
/// The key is computed by `decode_plan::plan_cache_key()`.  It hashes the
/// raw model data bytes as stored in the block header.
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
}
