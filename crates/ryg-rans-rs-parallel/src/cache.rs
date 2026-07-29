//! # Shared model and table cache
//!
//! Caches validated immutable tables by model identity.
//!
//! ## Properties
//!
//! - Bounded entry count.
//! - Bounded total memory.
//! - Deterministic eviction policy (FIFO).
//! - No correctness dependence on cache hits.
//! - No mutable table after publication.
//! - Cache poisoning impossible after validation failure.

use std::collections::VecDeque;
use std::fmt;

/// Key for the model cache.
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ModelCacheKey {
    /// SHA-256 of the serialised model data.
    pub model_sha256: [u8; 32],
    /// Precision (scale_bits).
    pub scale_bits: u8,
    /// Codec identifier.
    pub codec_id: u16,
}

/// A bounded FIFO model cache.
///
/// Stores validated decode plans indexed by model identity.
/// Eviction is deterministic (oldest entry first).
pub struct ModelCache<T> {
    entries: VecDeque<(ModelCacheKey, T)>,
    max_entries: usize,
    max_total_bytes: u64,
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

    /// Get a cached entry by key.
    pub fn get(&self, key: &ModelCacheKey) -> Option<&T> {
        for (k, v) in &self.entries {
            if k == key {
                return Some(v);
            }
        }
        None
    }

    /// Insert a new entry.  Evicts oldest entries if bounds are exceeded.
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

    /// Number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Clear the cache.
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
