//! # Model artifact cache — exact bounds, single-flight, explicit ownership
//!
//! ## Purpose
//!
//! The model cache memoises the *expensive, immutable, model-derived*
//! artifacts of the decode path — the validated frequency vector and (SIMD
//! builds) the 16 KiB packed word table — so that a model reused across many
//! blocks is parsed, validated, and table-built once instead of once per
//! block.  Backend selection deliberately happens *after* the lookup (see
//! [`ValidatedModelArtifacts`]), so a cached artifact is never reused under
//! incompatible execution conditions.
//!
//! ## Why this exists (Phase O)
//!
//! The pre-Phase-O cache (residuals `MODEL_CACHE.BOUND.1` … `MODEL_CACHE.
//! WORKLOAD.1`) was approximate in four load-bearing ways:
//!
//! 1. **Eviction byte accounting was wrong** (`BOUND.1`): eviction subtracted
//!    the *incoming* entry's size instead of the evicted entry's actual size,
//!    so `current_bytes` drifted whenever entry sizes differed.
//! 2. **Oversized entries were retained** (`BOUND.2`): an entry larger than
//!    the whole budget evicted everything and then was stored anyway,
//!    violating `current_bytes <= max_total_bytes`.
//! 3. **Zero capacity still admitted an entry** (`BOUND.3`): the eviction
//!    loop broke on an empty queue and the `push_back` then ran.
//! 4. **No single-flight, no unique-key guarantee, no metrics, no explicit
//!    owner** (`RACE.1`, `RACE.2`, `METRICS.1`, `CONTENTION.1`, `PERF.1`,
//!    `AVAILABILITY.1`): a process-global `OnceLock<Mutex<ModelCache>>` made
//!    cold/warm measurement ambiguous and let concurrent cold misses build
//!    the packed table N times.
//!
//! This module replaces that design with one whose resource invariants are
//! mathematically true after every public operation:
//!
//! ```text
//! current_entries == number of retained ready entries
//! current_bytes  == sum(accounted_bytes of every retained ready entry)
//! current_entries <= max_entries
//! current_bytes  <= max_total_bytes
//! ```
//!
//! ## Design overview
//!
//! * [`ModelCache`] — the exact-accounting FIFO core (`HashMap` for O(1)
//!   key lookup + `VecDeque` for deterministic insertion order).  Insertion
//!   is two-phase (plan, then execute) so that no state mutation happens on
//!   an arithmetic overflow path.
//! * [`ModelArtifactCache`] — the explicitly owned, thread-safe cache
//!   facade with per-key **single-flight** construction: concurrent cold
//!   requests for one key perform exactly one build; the other callers wait
//!   on a condition variable and receive the same `Arc` artifact.
//! * [`build_validated_model_artifacts`] — the *single canonical artifact
//!   constructor*.  Both the cached path and the cache-bypass path call it,
//!   so cache-disabled and cached decodes cannot drift on validation or
//!   construction.
//! * [`ModelCacheMetricsSnapshot`] — authoritative counters; the only way
//!   production cache behavior is observable.
//!
//! ## Ownership (Phase O.4)
//!
//! There is **no process-global cache**.  A [`ModelArtifactCache`] is
//! constructed explicitly ([`ModelArtifactCache::bounded`] or
//! [`ModelArtifactCache::disabled`]) and handed to a [`crate::decode::
//! ParallelDecoder`].  Cold runs create a fresh cache; warm runs reuse a
//! known instance; tests inject tiny budgets; applications isolate tenants.
//! Cache lifetime is explicit — see ADR-0016.
//!
//! ## Failure transparency (Phase O.6)
//!
//! The cache is a performance optimisation.  A cache-internal failure (lock
//! poisoning, accounting invariant violation) is **never** reported as a
//! malformed model: it is recorded in the metrics (`uncached_fallbacks`),
//! the cache is bypassed, and the artifact is built directly with the same
//! [`build_validated_model_artifacts`] constructor.  Model invalidity
//! ([`ModelArtifactBuildError`]) and cache unavailability
//! ([`ModelCacheError`]) remain distinguishable.
//!
//! ## Panic and cancellation policy (Phase O.5)
//!
//! * A builder panic is caught at the cache boundary ([`std::panic::
//!   catch_unwind`]), converted to [`ModelArtifactBuildError::Panicked`],
//!   the `Building` marker is removed, all waiters are notified, and no
//!   partial artifact is published.  A panic can never leave a permanent
//!   `Building` entry.
//! * A cancelled waiter stops waiting (via `wait_timeout` polling of the
//!   cancellation token) without corrupting the shared build; the build
//!   itself, once started, runs to completion and publishes on success.
//! * Failed builds are **retryable**: the `Building` marker is removed and
//!   the next request retries.  This is cheap because validation runs before
//!   the expensive table build, so a permanently invalid model fails fast.
//!
//! ## Eviction policy (Phase O.17)
//!
//! FIFO, with replacement semantics: re-inserting an existing key removes
//! the old entry (exact byte subtraction) and inserts the new one at the
//! back.  FIFO is retained unless shadow simulation shows another policy has
//! material end-to-end value (ADR-0017); the shadow simulation is in
//! `bench`'s cache courts.

use std::collections::{HashMap, VecDeque};

use crate::cancellation::CancellationToken;
use crate::sync::{Arc, Condvar, Mutex};

// ---------------------------------------------------------------------------
// ModelCacheKey
// ---------------------------------------------------------------------------

/// Composite identity of a cached model artifact.
///
/// # Fields
///
/// * `model_sha256` — SHA-256 of the *exact serialised model bytes* stored
///   in the block header (not a parsed representation).  Any byte difference
///   produces a different key; identical bytes (e.g. every Uniform256 block)
///   produce the same key.
/// * `scale_bits` — the rANS precision.  Two models at different precisions
///   are different artifacts even with identical frequencies.
/// * `codec_id` — the codec (7 = 8-way word, 8 = 16-way word, …).  The same
///   model bytes used by different codecs are different artifacts.
///
/// # Key completeness (Phase O.7)
///
/// Every field that influences the cached artifacts (frequencies, uniform256
/// flag, packed table) is represented: the exact model bytes (via the hash),
/// the scale, and the codec.  Backend policy is deliberately *outside* the
/// key: the cached artifacts are backend-independent, and backend selection
/// happens after the lookup ([`ValidatedModelArtifacts`] docs).
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ModelCacheKey {
    /// SHA-256 hash of the serialised model data.
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
    /// parsed representation — so the key is byte-exact.
    pub fn from_model(codec_id: u16, scale_bits: u8, model_data: &[u8]) -> Self {
        Self {
            model_sha256: crate::encode::sha256(model_data),
            scale_bits,
            codec_id,
        }
    }
}

// ---------------------------------------------------------------------------
// Typed outcomes and errors
// ---------------------------------------------------------------------------

/// Typed result of a cache insertion (Phase O.2).
///
/// Stringly-typed outcomes are prohibited; every insertion decision is an
/// enum the caller must match exhaustively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheInsertOutcome {
    /// The key was absent and is now retained.
    Inserted,
    /// The key was already retained; the old entry was replaced (exact byte
    /// accounting) and the new value is retained at the FIFO back.
    Replaced,
    /// The cache is disabled (`max_entries == 0` or `max_total_bytes == 0`);
    /// nothing was retained.
    RejectedDisabled,
    /// `entry_bytes > max_total_bytes`: the artifact is valid but cannot be
    /// admitted.  **Nothing is evicted** — evicting useful entries merely to
    /// discover the new object still cannot fit is a net loss (Phase O.2).
    RejectedOversized {
        /// The accounted size of the rejected artifact.
        entry_bytes: u64,
        /// The cache's total byte budget.
        max_total_bytes: u64,
    },
}

/// Typed cache-internal failure (Phase O.6).
///
/// These are failures of the *cache*, never of the model.  The owner policy
/// converts them into a metric + uncached build, preserving decode
/// semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCacheError {
    /// The cache mutex is poisoned or a condvar wait failed.  The cache state
    /// cannot be trusted; the caller must bypass the cache.
    Synchronization,
    /// The byte/entry accounting invariant is violated or the arithmetic
    /// overflowed.  Unreachable under correct operation; the owner self-heals
    /// by clearing the cache and bypasses.
    AccountingInvariant,
    /// Any other internal inconsistency.
    InternalState,
}

/// Block-independent model artifact construction error (Phase O.5/O.7).
///
/// Errors are **block-independent**: they describe the model, not the block,
/// so concurrent waiters of a single-flight build can receive the same
/// semantic error class without the cache ever caching a block-specific
/// error.  The caller (e.g. [`crate::decode::decode_single_block`]) maps the
/// error to the current block index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelArtifactBuildError {
    /// The model does not encode exactly 256 symbols.
    InvalidFrequencyCount,
    /// The 256 frequencies do not sum to `1 << scale_bits`.
    InvalidFrequencySum,
    /// The scale is outside the supported range (e.g. `>= 32` where the
    /// `1 << scale` shift would overflow).
    UnsupportedScale,
    /// The packed word table could not be constructed (e.g. a symbol's
    /// frequency exceeds 4095, or the scale is not 12).
    PackedTableConstruction,
    /// The builder closure panicked.  Caught at the cache boundary; never a
    /// permanent `Building` state.
    Panicked,
    /// The waiting caller was cancelled before the shared build completed.
    /// The shared build, if in progress, continues for the other waiters.
    Cancelled,
}

impl core::fmt::Display for ModelArtifactBuildError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::InvalidFrequencyCount => write!(f, "model does not encode 256 symbols"),
            Self::InvalidFrequencySum => {
                write!(f, "model frequencies do not sum to 1 << scale_bits")
            }
            Self::UnsupportedScale => write!(f, "scale_bits outside the supported range"),
            Self::PackedTableConstruction => write!(f, "packed word table construction failed"),
            Self::Panicked => write!(f, "model artifact builder panicked"),
            Self::Cancelled => write!(f, "cache wait cancelled"),
        }
    }
}

// ---------------------------------------------------------------------------
// The exact-accounting FIFO core
// ---------------------------------------------------------------------------

/// One retained cache entry with its exact accounted size (Phase O.1).
///
/// # Why the value is `Arc<T>`
///
/// The owner hands out clones of the same `Arc` on hits and publishes the
/// same `Arc` to all single-flight waiters.  `Arc` is the *sharing* unit;
/// the cache never copies the expensive artifact itself.
///
/// The map key (`HashMap<ModelCacheKey, CacheEntry<T>>`) is the
/// authoritative identity; the entry deliberately does not duplicate it (a
/// duplicated key field would be dead storage in production builds).
struct CacheEntry<T> {
    /// The shared artifact.
    value: Arc<T>,
    /// The exact number of accounted bytes this entry contributes to
    /// `current_bytes`.  Set once at insertion; every eviction/replacement
    /// subtracts exactly this number.
    accounted_bytes: u64,
}

/// A bounded, FIFO-evicted cache with **exact** byte and count accounting.
///
/// # Representation
///
/// `HashMap<ModelCacheKey, CacheEntry<T>>` gives O(1) key lookup (the
/// pre-Phase-O linear scan was replaced — the O(N) `get` was kept only
/// because the cache was assumed small, without measurement); a
/// `VecDeque<ModelCacheKey>` gives deterministic FIFO insertion order.
///
/// # Invariants (after every successful public operation)
///
/// ```text
/// map.len() == fifo.len()                       (queue/map set equality)
/// every key appears at most once in fifo         (unique keys)
/// current_bytes == sum(entry.accounted_bytes)
/// current_entries <= max_entries
/// current_bytes <= max_total_bytes
/// ```
///
/// # Two-phase insertion
///
/// [`ModelCache::insert`] first *plans* the eviction set without mutating
/// anything (all arithmetic checked), then *executes* it.  An arithmetic
/// overflow therefore rejects the insertion with
/// [`ModelCacheError::AccountingInvariant`] without leaving partially
/// mutated state.
pub struct ModelCache<T> {
    /// Key → entry.  O(1) lookup; the single source of retained truth.
    map: HashMap<ModelCacheKey, CacheEntry<T>>,
    /// FIFO insertion order.  Front = oldest.  Contains exactly the keys of
    /// `map` (set equality is a tested invariant).
    fifo: VecDeque<ModelCacheKey>,
    /// Maximum number of entries before eviction kicks in.
    max_entries: usize,
    /// Maximum total accounted bytes before eviction kicks in.
    max_total_bytes: u64,
    /// Exact sum of `accounted_bytes` over retained entries.
    current_bytes: u64,
    /// Number of entries evicted (count bound + byte bound).
    entry_evictions: u64,
    /// Total accounted bytes evicted.
    byte_evictions: u64,
}

impl<T> ModelCache<T> {
    /// Create a new exact-accounting cache.
    ///
    /// `max_entries == 0` or `max_total_bytes == 0` disables the cache
    /// (Phase O.2): no insertion is ever admitted and no entry is retained.
    pub fn new(max_entries: usize, max_total_bytes: u64) -> Self {
        Self {
            map: HashMap::new(),
            fifo: VecDeque::new(),
            max_entries,
            max_total_bytes,
            current_bytes: 0,
            entry_evictions: 0,
            byte_evictions: 0,
        }
    }

    /// Whether the cache is disabled (Phase O.2).
    ///
    /// ```text
    /// max_entries == 0 OR max_total_bytes == 0 → disabled
    /// ```
    #[inline]
    pub fn is_disabled(&self) -> bool {
        self.max_entries == 0 || self.max_total_bytes == 0
    }

    /// Look up a cached entry by key.
    ///
    /// O(1) via the `HashMap`.  Returns a cloned `Arc` so the caller never
    /// borrows through the cache.
    pub fn get(&self, key: &ModelCacheKey) -> Option<Arc<T>> {
        self.map.get(key).map(|e| e.value.clone())
    }

    /// Number of retained entries.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether no entry is retained.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The exact total accounted bytes of retained entries.
    pub fn current_bytes(&self) -> u64 {
        self.current_bytes
    }

    /// Configured maximum entries.
    pub fn max_entries(&self) -> usize {
        self.max_entries
    }

    /// Configured maximum total bytes.
    pub fn max_total_bytes(&self) -> u64 {
        self.max_total_bytes
    }

    /// Cumulative eviction counters since construction or last reset.
    pub fn eviction_stats(&self) -> (u64, u64) {
        (self.entry_evictions, self.byte_evictions)
    }

    /// Snapshot the retained entries as `(key, accounted_bytes)` pairs
    /// (Phase O.1 ground truth for courts and soak runs).
    ///
    /// Available in tests and with the `cache-timing` feature so an
    /// *external* verifier (court, proptest shadow model, soak loop) can
    /// recompute the retained byte sum independently and compare it with
    /// [`ModelCache::current_bytes`] — the exact-accounting cross-check the
    /// mission requires.  Production decode never needs it.
    #[cfg(any(test, feature = "cache-timing"))]
    pub fn retained_entries(&self) -> Vec<(ModelCacheKey, u64)> {
        self.map
            .iter()
            .map(|(k, e)| (k.clone(), e.accounted_bytes))
            .collect()
    }

    /// Insert or replace an entry (Phase O.1/O.2/O.3).
    ///
    /// # Semantics
    ///
    /// 1. Disabled cache → [`CacheInsertOutcome::RejectedDisabled`], no
    ///    state change.
    /// 2. `entry_bytes > max_total_bytes` → [`CacheInsertOutcome::
    ///    RejectedOversized`], **no state change** — useful entries are not
    ///    evicted for an artifact that still cannot fit.
    /// 3. Key already present → the old entry is removed first (exact byte
    ///    subtraction), then the new entry is inserted as if fresh.  The
    ///    outcome is [`CacheInsertOutcome::Replaced`].  The same key can
    ///    never occupy two slots.
    /// 4. Over budget (count or bytes) → the oldest entries are evicted
    ///    (front of the FIFO, subtracting each evicted entry's exact bytes)
    ///    until the new entry fits.
    ///
    /// # Arithmetic
    ///
    /// All byte arithmetic is checked.  An overflow on the *final* addition
    /// (unreachable under correct accounting) returns
    /// [`ModelCacheError::AccountingInvariant`] with **no state mutation**:
    /// the insertion is rejected rather than corrupting the counters.
    /// Saturating arithmetic is deliberately *not* used: saturation would
    /// hide an accounting defect, and the mission forbids concealing
    /// violations.
    pub fn insert(
        &mut self,
        key: ModelCacheKey,
        value: Arc<T>,
        entry_bytes: u64,
    ) -> Result<CacheInsertOutcome, ModelCacheError> {
        // ---- Phase 0: disable / oversized rejection (no mutation) --------
        if self.is_disabled() {
            return Ok(CacheInsertOutcome::RejectedDisabled);
        }
        if entry_bytes > self.max_total_bytes {
            return Ok(CacheInsertOutcome::RejectedOversized {
                entry_bytes,
                max_total_bytes: self.max_total_bytes,
            });
        }

        // ---- Phase 1: replacement removal (exact subtraction) -------------
        let was_present = self.map.contains_key(&key);
        if was_present {
            let old = self
                .map
                .remove(&key)
                .ok_or(ModelCacheError::AccountingInvariant)?;
            self.current_bytes = self
                .current_bytes
                .checked_sub(old.accounted_bytes)
                .ok_or(ModelCacheError::AccountingInvariant)?;
            // Keys are unique in the queue; `retain` removes the one entry.
            // O(N) on the replacement path only — single-flight makes
            // replacement rare (an explicit re-insert), and N is bounded by
            // max_entries.  Documented, measured choice (ADR-0016): lookup
            // stays O(1); the queue is never scanned on the hit path.
            self.fifo.retain(|k| k != &key);
        }

        // ---- Phase 2: plan the eviction set without mutating -------------
        // The queue is NOT mutated while planning; an index advances past
        // already-planned victims (re-reading `fifo.front()` every iteration
        // would plan the same victim twice — a defect caught by the
        // mixed-size FIFO eviction test).
        let mut projected_bytes = self.current_bytes;
        let mut projected_len = self.fifo.len();
        let mut to_evict: Vec<ModelCacheKey> = Vec::new();
        let mut front_idx = 0usize;
        loop {
            let fits = projected_len < self.max_entries
                && projected_bytes
                    .checked_add(entry_bytes)
                    .is_some_and(|c| c <= self.max_total_bytes);
            if fits {
                break;
            }
            let Some(front) = self.fifo.get(front_idx) else {
                // Unreachable: entry_bytes <= max_total_bytes and
                // max_entries >= 1 (not disabled) guarantee an empty cache
                // fits.  Guarded defensively; an empty queue here would mean
                // the disable/oversized checks above were bypassed, which is
                // an internal error.
                return Err(ModelCacheError::InternalState);
            };
            let ev = self
                .map
                .get(front)
                .ok_or(ModelCacheError::AccountingInvariant)?;
            projected_bytes = projected_bytes
                .checked_sub(ev.accounted_bytes)
                .ok_or(ModelCacheError::AccountingInvariant)?;
            projected_len -= 1;
            front_idx += 1;
            to_evict.push(front.clone());
        }

        // ---- Phase 3: execute evictions (exact bytes) ----------------------
        for k in &to_evict {
            let ev = self
                .map
                .remove(k)
                .ok_or(ModelCacheError::AccountingInvariant)?;
            self.current_bytes = self
                .current_bytes
                .checked_sub(ev.accounted_bytes)
                .ok_or(ModelCacheError::AccountingInvariant)?;
            // `to_evict` was built in FIFO order (front_idx advanced); each
            // pop_front removes the next planned victim.
            let popped = self
                .fifo
                .pop_front()
                .ok_or(ModelCacheError::AccountingInvariant)?;
            debug_assert_eq!(popped, *k, "eviction plan must match queue order");
            self.entry_evictions += 1;
            self.byte_evictions += ev.accounted_bytes;
        }

        // ---- Phase 4: insert -------------------------------------------------
        self.current_bytes = self
            .current_bytes
            .checked_add(entry_bytes)
            .ok_or(ModelCacheError::AccountingInvariant)?;
        self.map.insert(
            key.clone(),
            CacheEntry {
                value,
                accounted_bytes: entry_bytes,
            },
        );
        self.fifo.push_back(key);
        Ok(if was_present {
            CacheInsertOutcome::Replaced
        } else {
            CacheInsertOutcome::Inserted
        })
    }

    /// Remove every retained entry, resetting byte/count accounting and the
    /// eviction counters.
    pub fn clear(&mut self) {
        self.map.clear();
        self.fifo.clear();
        self.current_bytes = 0;
        self.entry_evictions = 0;
        self.byte_evictions = 0;
    }

    /// Independently recompute the invariants and return them for tests and
    /// soak runs.  This is the ground-truth cross-check: the counters must
    /// equal a fresh sum over the retained entries.
    #[cfg(any(test, feature = "cache-timing"))]
    pub fn invariant_check(&self) -> Result<(), String> {
        if self.map.len() != self.fifo.len() {
            return Err(format!(
                "map/fifo length mismatch: map={} fifo={}",
                self.map.len(),
                self.fifo.len()
            ));
        }
        let mut recomputed = 0u64;
        for (_k, e) in &self.map {
            recomputed = recomputed
                .checked_add(e.accounted_bytes)
                .ok_or_else(|| "recomputed byte sum overflow".to_string())?;
        }
        if recomputed != self.current_bytes {
            return Err(format!(
                "byte accounting drift: recomputed={} current_bytes={}",
                recomputed, self.current_bytes
            ));
        }
        // Every map key must appear exactly once in the FIFO.
        let mut seen: std::collections::HashSet<&ModelCacheKey> = std::collections::HashSet::new();
        for k in &self.fifo {
            if !self.map.contains_key(k) {
                return Err("fifo key not in map".into());
            }
            if !seen.insert(k) {
                return Err("duplicate key in fifo".into());
            }
        }
        if self.map.len() > self.max_entries {
            return Err("entry count exceeds max_entries".into());
        }
        if self.current_bytes > self.max_total_bytes {
            return Err("byte total exceeds max_total_bytes".into());
        }
        Ok(())
    }
}

impl<T: core::fmt::Debug> core::fmt::Debug for ModelCache<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ModelCache")
            .field("entries", &self.map.len())
            .field("max_entries", &self.max_entries)
            .field("current_bytes", &self.current_bytes)
            .field("max_total_bytes", &self.max_total_bytes)
            .finish()
    }
}

// ---------------------------------------------------------------------------
// The cached artifact type
// ---------------------------------------------------------------------------

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
/// once per unique model instead of once per block.  The whole artifact is
/// additionally wrapped in an `Arc` by the cache (`CacheEntry.value`), so
/// hits and single-flight waiters share one allocation.
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
    /// `None` when the model cannot build a word table (e.g. `scale_bits !=
    /// 12`): the decode executor then fails with the same `Model` error it
    /// would have produced building the table itself, so caching never
    /// changes error identity.
    #[cfg(feature = "simd")]
    pub packed_table: Option<Arc<ryg_rans_rs_simd::packed_table::PackedWordTable>>,
}

/// The artifact plus its exact accounted size (Phase O.7).
///
/// `accounted_bytes` is the number the cache tracks.  It is computed by the
/// single canonical constructor so the cached and uncached paths can never
/// disagree about what an entry costs.
#[derive(Debug, Clone)]
pub struct BuiltModelArtifacts {
    /// The validated, shareable artifacts.
    pub artifacts: ValidatedModelArtifacts,
    /// Exact accounted bytes of the entry (frequencies + packed table +
    /// fixed overhead).
    pub accounted_bytes: u64,
}

/// Fixed per-entry accounting overhead (Arc header, box header, map/queue
/// node amortisation).  Deliberately small and constant; the dominant terms
/// are the 1 KiB frequency vector and the 16 KiB packed table.
const ARTIFACT_FIXED_OVERHEAD: u64 = 64;

/// Build the validated model artifacts — the **single canonical
/// constructor** (Phase O.7).
///
/// Both the cached path and the cache-bypass path of [`ModelArtifactCache`]
/// call this function; there is no separate uncached construction logic that
/// could drift.
///
/// # Steps
///
/// 1. `model_data` length 0 → uniform model (`freq[s] = (1 << scale) / 256`
///    for all 256 symbols); length 1024 → parse 256 × u32 LE; anything else
///    → [`ModelArtifactBuildError::InvalidFrequencyCount`].
/// 2. `scale_bits < 32` checked before any `1 << scale` shift; otherwise
///    [`ModelArtifactBuildError::UnsupportedScale`].  (A shift of `>= 32`
///    would panic in debug builds — untrusted header input must never reach
///    a shift.)
/// 3. Frequencies must sum to exactly `1 << scale_bits` →
///    [`ModelArtifactBuildError::InvalidFrequencySum`].
/// 4. Uniform256 detection: `scale == 12` and every frequency == 16.
/// 5. SIMD builds: cumulative frequencies and the packed word table →
///    [`ModelArtifactBuildError::PackedTableConstruction`] on failure.
/// 6. Exact accounted bytes: `freqs.len() * 4 + ARTIFACT_FIXED_OVERHEAD`
///    plus the packed table's `entries * size_of::<u32>()` when present.
///
/// # Error identity
///
/// The error class is block-independent: the caller maps it to the current
/// block index.  A corrupt model is never admitted to the cache (the build
/// fails before insertion).
pub fn build_validated_model_artifacts(
    _codec_id: u16,
    scale_bits: u8,
    model_data: &[u8],
) -> Result<BuiltModelArtifacts, ModelArtifactBuildError> {
    // `codec_id` is deliberately *not* read here: the cached artifacts
    // (frequencies, uniform256 flag, packed table) are codec-independent,
    // which is exactly why backend/codec policy stays outside the cache key
    // for artifact reuse (Phase O.7).  The caller still passes it so the
    // single constructor signature mirrors the key derivation inputs — the
    // key (which *does* include codec_id) is computed by
    // `ModelArtifactCache::get_or_build` before this runs.
    // ---- 1. Parse frequencies ----------------------------------------------
    let freqs: Vec<u32> = match model_data.len() {
        0 => {
            let total = checked_scale_total(scale_bits)?;
            let uniform_freq = total / 256;
            vec![uniform_freq; 256]
        }
        1024 => model_data
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
        _ => return Err(ModelArtifactBuildError::InvalidFrequencyCount),
    };

    // ---- 2. Validate symbol count ------------------------------------------
    if freqs.len() != 256 {
        return Err(ModelArtifactBuildError::InvalidFrequencyCount);
    }

    // ---- 3. Validate scale (checked shift) and normalized sum -------------
    let expected_total = checked_scale_total(scale_bits)?;
    let sum: u64 = freqs.iter().map(|&f| f as u64).sum();
    if sum != expected_total as u64 {
        return Err(ModelArtifactBuildError::InvalidFrequencySum);
    }

    // ---- 4. Uniform256 detection -------------------------------------------
    let uniform256 = scale_bits == 12 && freqs.iter().all(|&f| f == 16);

    // ---- 5. Packed word table (SIMD builds) --------------------------------
    #[cfg(feature = "simd")]
    let packed_table = {
        // Cumulative frequencies: cum[0] = 0, cum[i+1] = cum[i] + freqs[i].
        // The sum is validated == 1 << scale <= 2^31 (scale < 32), so each
        // cumulative value fits u32 — but the addition is checked anyway so
        // a future scale change cannot introduce a panic on untrusted input.
        let mut cum = Vec::with_capacity(257);
        cum.push(0u32);
        for i in 0..256 {
            let next = cum[i]
                .checked_add(freqs.get(i).copied().unwrap_or(0))
                .ok_or(ModelArtifactBuildError::PackedTableConstruction)?;
            cum.push(next);
        }
        ryg_rans_rs_simd::packed_table::PackedWordTable::from_freqs(&freqs, &cum, scale_bits as u32)
            .map(Arc::new)
            .map_err(|_| ModelArtifactBuildError::PackedTableConstruction)?
    };

    // ---- 6. Exact accounted bytes -------------------------------------------
    let mut accounted_bytes = (freqs.len() as u64)
        .checked_mul(4)
        .and_then(|b| b.checked_add(ARTIFACT_FIXED_OVERHEAD))
        .ok_or(ModelArtifactBuildError::PackedTableConstruction)?;
    #[cfg(feature = "simd")]
    {
        let table_bytes = (packed_table.as_slice().len() as u64)
            .checked_mul(std::mem::size_of::<u32>() as u64)
            .ok_or(ModelArtifactBuildError::PackedTableConstruction)?;
        accounted_bytes = accounted_bytes
            .checked_add(table_bytes)
            .ok_or(ModelArtifactBuildError::PackedTableConstruction)?;
    }

    Ok(BuiltModelArtifacts {
        artifacts: ValidatedModelArtifacts {
            freqs: Arc::new(freqs),
            uniform256,
            #[cfg(feature = "simd")]
            packed_table: Some(packed_table),
        },
        accounted_bytes,
    })
}

/// `1u32 << scale_bits` with the shift overflow guard (scale must be < 32).
///
/// Untrusted header input controls `scale_bits`; an unchecked `1u32 << s`
/// panics for `s >= 32` in debug builds.  The typed
/// [`ModelArtifactBuildError::UnsupportedScale`] is the canonical error.
fn checked_scale_total(scale_bits: u8) -> Result<u32, ModelArtifactBuildError> {
    u32::checked_shl(1u32, scale_bits as u32).ok_or(ModelArtifactBuildError::UnsupportedScale)
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// Authoritative cache behavior snapshot (Phase O.8).
///
/// All counters are monotonic (or snapshot-level `current_*` values).
/// Counters are incremented under the cache-state mutex, so they are
/// thread-safe by construction and can never affect cache correctness.
/// Production code must not depend on metrics for correctness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModelCacheMetricsSnapshot {
    /// Total lookup attempts (cache-enabled only; disabled runs count
    /// `disabled_bypasses` instead).
    pub lookups: u64,
    /// Lookups that found a retained artifact.
    pub hits: u64,
    /// Lookups that found no retained artifact.
    pub misses: u64,
    /// Single-flight constructions started (includes retries after failure).
    pub builds_started: u64,
    /// Constructions that completed and produced a publishable artifact.
    pub builds_completed: u64,
    /// Constructions that failed (build error or panic).
    pub build_failures: u64,
    /// Callers that waited for an in-progress same-key build (single-flight
    /// coalescing).
    pub coalesced_waiters: u64,
    /// Artifacts admitted to the cache.
    pub insertions: u64,
    /// Existing keys replaced.
    pub replacements: u64,
    /// Entries evicted (count bound + byte bound).
    pub entry_evictions: u64,
    /// Total accounted bytes evicted.
    pub byte_evictions: u64,
    /// Artifacts rejected as larger than `max_total_bytes` (not retained).
    pub oversized_rejections: u64,
    /// Requests served while the cache was disabled (direct uncached build).
    pub disabled_bypasses: u64,
    /// Cache-internal failures that forced a direct uncached build.
    pub uncached_fallbacks: u64,
    /// Currently retained ready entries.
    pub current_entries: usize,
    /// Peak retained ready entries.
    pub peak_entries: usize,
    /// Currently retained accounted bytes.
    pub current_bytes: u64,
    /// Peak retained accounted bytes.
    pub peak_bytes: u64,
}

impl ModelCacheMetricsSnapshot {
    /// The O.8 invariant `hits + misses == lookups`.
    pub fn invariant_hit_miss_sum(&self) -> bool {
        self.hits.wrapping_add(self.misses) == self.lookups
    }

    /// The O.8 invariant `builds_completed + build_failures <= builds_started`.
    pub fn invariant_build_accounting(&self) -> bool {
        self.builds_completed.saturating_add(self.build_failures) <= self.builds_started
    }
}

// ---------------------------------------------------------------------------
// Timing instrumentation (Phase O.16) — behind `cache-timing`
// ---------------------------------------------------------------------------

/// Cumulative timing snapshot for contention analysis (Phase O.16).
///
/// Only available with the `cache-timing` feature.  The counters are
/// best-effort *diagnostics*: they live in atomic cells outside the
/// cache-state mutex (so recording them can never deadlock or perturb the
/// very lock they measure) and production code must never depend on them
/// for correctness.  They are monotonic since cache construction;
/// `ModelArtifactCache::clear` deliberately does NOT reset them (clear
/// resets behavior counters; timing stays cumulative so a soak run can
/// still see whole-run contention).
///
/// # What each counter means
///
/// * `lock_acquires` / `lock_wait_ns` — the cache-state mutex acquisition
///   attempts routed through [`ModelArtifactCache::lock_state`] and the
///   cumulative time spent blocked acquiring it.  This is the "lookup lock
///   wait time" of O.16: if it grows with worker count, the global mutex is
///   the serialization point.
/// * `artifact_builds` / `artifact_build_ns` — the single-flight builder
///   closure executions and their cumulative duration (run OUTSIDE the
///   mutex, so this is not lock time).
/// * `single_flight_waits` / `single_flight_wait_ns` — callers that
///   registered as waiters and the cumulative time they spent on the
///   condition variable before waking (hit, failure-retry, or cancellation).
/// * `lookup_calls` / `lookup_ns` — every `get_or_build` call and its total
///   duration (the caller-visible latency, including lock + build + wait).
#[cfg(all(feature = "cache-timing", not(loom)))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModelCacheTimingSnapshot {
    /// Successful (non-poisoned) `lock_state` acquisitions.
    pub lock_acquires: u64,
    /// Cumulative nanoseconds blocked acquiring the cache-state mutex.
    pub lock_wait_ns: u64,
    /// Builder closure invocations (single-flight builder role).
    pub artifact_builds: u64,
    /// Cumulative nanoseconds inside builder closures.
    pub artifact_build_ns: u64,
    /// Callers that waited for an in-progress same-key build.
    pub single_flight_waits: u64,
    /// Cumulative nanoseconds spent on the condvar as a waiter.
    pub single_flight_wait_ns: u64,
    /// `get_or_build` invocations.
    pub lookup_calls: u64,
    /// Cumulative nanoseconds across every `get_or_build` invocation.
    pub lookup_ns: u64,
}

/// Atomic timing cells owned by the cache, outside the mutex (Phase O.16).
///
/// `std::sync::atomic` is deliberate: under `--cfg loom` this type is
/// compiled out (`not(loom)`), so the loom build never carries `Instant`
/// or std atomics that loom cannot model.
#[cfg(all(feature = "cache-timing", not(loom)))]
#[derive(Debug, Default)]
struct ModelArtifactCacheTiming {
    lock_acquires: std::sync::atomic::AtomicU64,
    lock_wait_ns: std::sync::atomic::AtomicU64,
    artifact_builds: std::sync::atomic::AtomicU64,
    artifact_build_ns: std::sync::atomic::AtomicU64,
    single_flight_waits: std::sync::atomic::AtomicU64,
    single_flight_wait_ns: std::sync::atomic::AtomicU64,
    lookup_calls: std::sync::atomic::AtomicU64,
    lookup_ns: std::sync::atomic::AtomicU64,
}

#[cfg(all(feature = "cache-timing", not(loom)))]
impl ModelArtifactCacheTiming {
    fn snapshot(&self) -> ModelCacheTimingSnapshot {
        use std::sync::atomic::Ordering;
        ModelCacheTimingSnapshot {
            lock_acquires: self.lock_acquires.load(Ordering::Relaxed),
            lock_wait_ns: self.lock_wait_ns.load(Ordering::Relaxed),
            artifact_builds: self.artifact_builds.load(Ordering::Relaxed),
            artifact_build_ns: self.artifact_build_ns.load(Ordering::Relaxed),
            single_flight_waits: self.single_flight_waits.load(Ordering::Relaxed),
            single_flight_wait_ns: self.single_flight_wait_ns.load(Ordering::Relaxed),
            lookup_calls: self.lookup_calls.load(Ordering::Relaxed),
            lookup_ns: self.lookup_ns.load(Ordering::Relaxed),
        }
    }
}

// ---------------------------------------------------------------------------
// The explicitly owned cache with single-flight construction
// ---------------------------------------------------------------------------

/// Per-key single-flight state (Phase O.5).
///
/// `Building { waiters }` means a builder is constructing the artifact
/// *outside* the cache-state lock; `waiters` counts callers blocked on the
/// condition variable for this key.  There is deliberately no `Ready` state
/// here: a completed build is published into the ready cache and the marker
/// is removed, so the ready cache remains the *single* source of retained
/// truth (O.3 — one retained entry per key).
#[derive(Debug, Clone, Copy)]
enum KeyState {
    /// A builder is running; `waiters` callers are waiting on the condvar.
    Building { waiters: usize },
}

/// The mutex-guarded cache state.
struct CacheState {
    /// Ready (retained) artifacts — the exact-accounting FIFO core.
    ready: ModelCache<ValidatedModelArtifacts>,
    /// In-progress single-flight builds keyed by model identity.
    in_flight: HashMap<ModelCacheKey, KeyState>,
    /// Whether this cache instance is disabled at construction time.
    disabled: bool,
    /// Monotonic behavior counters.
    metrics: ModelCacheMetricsSnapshot,
}

/// The explicitly owned, thread-safe model artifact cache (Phase O.4).
///
/// # Construction
///
/// * [`ModelArtifactCache::bounded(max_entries, max_total_bytes)`] — a real
///   cache with exact bounds.
/// * [`ModelArtifactCache::disabled()`] — a zero-capacity cache that retains
///   nothing, performs no insertion, and serves every request by direct
///   construction (the semantic baseline).
///
/// # Single-flight (Phase O.5)
///
/// [`ModelArtifactCache::get_or_build`] guarantees that N concurrent cold
/// requests for one key perform exactly **one** construction; N-1 callers
/// wait on the condition variable and receive the same `Arc` artifact.  The
/// expensive model parse/validation/table build runs *outside* the global
/// cache-state lock (the lock is held only for state transitions: check,
/// register, publish).
///
/// # Failure transparency (Phase O.6)
///
/// A cache-internal failure (poisoned lock, accounting invariant violation)
/// is recorded as `uncached_fallbacks` and the artifact is built directly
/// with the same [`build_validated_model_artifacts`] constructor — decode
/// semantics are preserved and cache failure is never misreported as a
/// malformed model.
#[derive(Debug)]
pub struct ModelArtifactCache {
    state: Mutex<CacheState>,
    wake: Condvar,
    /// Contention timing cells (Phase O.16), outside the mutex.
    #[cfg(all(feature = "cache-timing", not(loom)))]
    timing: ModelArtifactCacheTiming,
}

/// How often a waiting caller re-checks the cache state when no notification
/// arrived.  Notifications (publish/failure) wake waiters immediately; the
/// poll interval exists so a cancelled waiter can stop waiting promptly.
const WAIT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

impl ModelArtifactCache {
    /// Create a bounded cache with exact entry and byte limits.
    ///
    /// `max_entries == 0` or `max_total_bytes == 0` produces a disabled
    /// cache (identical behavior to [`ModelArtifactCache::disabled`]).
    pub fn bounded(max_entries: usize, max_total_bytes: u64) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(CacheState {
                ready: ModelCache::new(max_entries, max_total_bytes),
                in_flight: HashMap::new(),
                disabled: max_entries == 0 || max_total_bytes == 0,
                metrics: ModelCacheMetricsSnapshot::default(),
            }),
            wake: Condvar::new(),
            #[cfg(all(feature = "cache-timing", not(loom)))]
            timing: ModelArtifactCacheTiming::default(),
        })
    }

    /// Create a disabled cache: retains nothing, inserts nothing, serves
    /// every request by direct construction.
    pub fn disabled() -> Arc<Self> {
        Self::bounded(0, 0)
    }

    /// Whether this cache instance is disabled.
    pub fn is_disabled(&self) -> bool {
        self.state.lock().map(|g| g.disabled).unwrap_or(true) // poisoned → treat as disabled (bypass policy)
    }

    /// Look up or build the model artifacts for one block (Phase O.5).
    ///
    /// # Single-flight contract
    ///
    /// Exactly one of N concurrent same-key callers runs `build`; the others
    /// wait and receive the same `Arc<ValidatedModelArtifacts>`.  If the
    /// builder fails, all waiters are released, the key is retryable, and
    /// the same block-independent error class is returned to every caller.
    ///
    /// # Cancellation
    ///
    /// A caller whose `cancel` token fires while waiting stops waiting and
    /// returns [`ModelArtifactBuildError::Cancelled`]; the shared build (if
    /// any) continues for the other waiters.  A build that has already
    /// started runs to completion and publishes on success (a started build
    /// is never abandoned mid-flight — documented semantic, Phase O.5).
    ///
    /// # Panic
    ///
    /// A panic inside `build` is caught, converted to
    /// [`ModelArtifactBuildError::Panicked`], the in-flight marker is
    /// removed, and all waiters are released.  No permanent `Building`
    /// state is possible.
    ///
    /// # Cache failures
    ///
    /// A poisoned lock or accounting violation records `uncached_fallbacks`
    /// and builds directly (same constructor).  It is never reported as a
    /// model error.
    pub fn get_or_build(
        &self,
        codec_id: u16,
        scale_bits: u8,
        model_data: &[u8],
        cancel: Option<&CancellationToken>,
        build: impl FnOnce() -> Result<BuiltModelArtifacts, ModelArtifactBuildError>,
    ) -> Result<Arc<ValidatedModelArtifacts>, ModelArtifactBuildError> {
        #[cfg(all(feature = "cache-timing", not(loom)))]
        {
            use std::sync::atomic::Ordering as AOrd;
            let t0 = std::time::Instant::now();
            let r = self.get_or_build_inner(codec_id, scale_bits, model_data, cancel, build);
            self.timing.lookup_calls.fetch_add(1, AOrd::Relaxed);
            self.timing
                .lookup_ns
                .fetch_add(t0.elapsed().as_nanos() as u64, AOrd::Relaxed);
            return r;
        }
        #[cfg(not(all(feature = "cache-timing", not(loom))))]
        {
            self.get_or_build_inner(codec_id, scale_bits, model_data, cancel, build)
        }
    }

    /// The single-flight implementation (Phase O.5).
    ///
    /// See [`ModelArtifactCache::get_or_build`] for the full contract; the
    /// public wrapper only adds the O.16 timing envelope so the inner
    /// control flow stays free of instrumentation.
    fn get_or_build_inner(
        &self,
        codec_id: u16,
        scale_bits: u8,
        model_data: &[u8],
        cancel: Option<&CancellationToken>,
        build: impl FnOnce() -> Result<BuiltModelArtifacts, ModelArtifactBuildError>,
    ) -> Result<Arc<ValidatedModelArtifacts>, ModelArtifactBuildError> {
        let key = ModelCacheKey::from_model(codec_id, scale_bits, model_data);

        loop {
            // ---- State check / registration under the lock -----------------
            let mut state = match self.lock_state() {
                Ok(g) => g,
                Err(_sync_err) => {
                    // Cache unavailable: record the diagnostic, bypass, build
                    // directly with the same constructor.  Never a Model error.
                    self.note_bypass();
                    return self.build_uncached(build);
                }
            };

            if state.disabled {
                state.metrics.disabled_bypasses = state.metrics.disabled_bypasses.saturating_add(1);
                drop(state);
                return self.build_uncached(build);
            }

            state.metrics.lookups = state.metrics.lookups.saturating_add(1);

            // Hit path: the artifact is retained.
            if let Some(artifact) = state.ready.get(&key) {
                state.metrics.hits = state.metrics.hits.saturating_add(1);
                return Ok(artifact);
            }

            // Miss path.  Use the entry API so exactly one caller becomes the
            // builder (atomic under the mutex — the classic single-flight
            // registration).
            match state.in_flight.entry(key.clone()) {
                std::collections::hash_map::Entry::Vacant(v) => {
                    // We are the builder.  Register and release the lock so
                    // the expensive construction runs outside it.
                    v.insert(KeyState::Building { waiters: 0 });
                    state.metrics.builds_started = state.metrics.builds_started.saturating_add(1);
                    state.metrics.misses = state.metrics.misses.saturating_add(1);
                    drop(state);

                    // A builder cancelled before construction starts yields
                    // the build to the next caller: the marker is removed and
                    // the waiters are released; one of them takes over
                    // (documented "another waiter takes over" semantic,
                    // Phase O.5).  A build that has already started runs to
                    // completion — it is never abandoned mid-flight.
                    if cancel.is_some_and(|c| c.is_cancelled()) {
                        if let Ok(mut g) = self.state.lock() {
                            g.in_flight.remove(&key);
                        }
                        self.wake.notify_all();
                        return Err(ModelArtifactBuildError::Cancelled);
                    }

                    // ---- Build OUTSIDE the lock (O.5 requirement) ---------
                    // The builder may panic; catch it so no permanent
                    // Building state survives.
                    #[cfg(all(feature = "cache-timing", not(loom)))]
                    let built = {
                        use std::sync::atomic::Ordering as AOrd;
                        let bt = std::time::Instant::now();
                        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(build));
                        self.timing.artifact_builds.fetch_add(1, AOrd::Relaxed);
                        self.timing
                            .artifact_build_ns
                            .fetch_add(bt.elapsed().as_nanos() as u64, AOrd::Relaxed);
                        r
                    };
                    #[cfg(not(all(feature = "cache-timing", not(loom))))]
                    let built = std::panic::catch_unwind(std::panic::AssertUnwindSafe(build));

                    // ---- Publish under the lock -----------------------------
                    let guard = match self.lock_state() {
                        Ok(g) => g,
                        Err(_) => {
                            // The cache became unavailable while we built.
                            // The artifact is still valid — return it
                            // uncached (never a model error).
                            self.note_bypass();
                            let artifact = match built {
                                Ok(Ok(b)) => Arc::new(b.artifacts),
                                Ok(Err(e)) => return Err(e),
                                Err(_) => return Err(ModelArtifactBuildError::Panicked),
                            };
                            return Ok(artifact);
                        }
                    };
                    let mut state = guard;
                    state.in_flight.remove(&key);

                    match built {
                        Ok(Ok(built_artifacts)) => {
                            let artifact = Arc::new(built_artifacts.artifacts);
                            let outcome = state.ready.insert(
                                key.clone(),
                                artifact.clone(),
                                built_artifacts.accounted_bytes,
                            );
                            match outcome {
                                Ok(CacheInsertOutcome::Inserted) => {
                                    state.metrics.insertions =
                                        state.metrics.insertions.saturating_add(1);
                                }
                                Ok(CacheInsertOutcome::Replaced) => {
                                    state.metrics.replacements =
                                        state.metrics.replacements.saturating_add(1);
                                }
                                Ok(CacheInsertOutcome::RejectedDisabled) => {
                                    state.metrics.disabled_bypasses =
                                        state.metrics.disabled_bypasses.saturating_add(1);
                                }
                                Ok(CacheInsertOutcome::RejectedOversized {
                                    entry_bytes,
                                    max_total_bytes: _,
                                }) => {
                                    // The artifact is valid but cannot be
                                    // admitted.  The caller still receives it
                                    // for the current decode (Phase O.2).
                                    state.metrics.oversized_rejections =
                                        state.metrics.oversized_rejections.saturating_add(1);
                                    let _ = entry_bytes;
                                }
                                Err(ModelCacheError::AccountingInvariant) => {
                                    // Self-heal: the core cannot be trusted.
                                    state.ready.clear();
                                    state.metrics.uncached_fallbacks =
                                        state.metrics.uncached_fallbacks.saturating_add(1);
                                }
                                Err(ModelCacheError::Synchronization)
                                | Err(ModelCacheError::InternalState) => {
                                    state.metrics.uncached_fallbacks =
                                        state.metrics.uncached_fallbacks.saturating_add(1);
                                }
                            }
                            state.metrics.builds_completed =
                                state.metrics.builds_completed.saturating_add(1);
                            // Peak tracking.
                            if state.ready.len() > state.metrics.peak_entries {
                                state.metrics.peak_entries = state.ready.len();
                            }
                            if state.ready.current_bytes() > state.metrics.peak_bytes {
                                state.metrics.peak_bytes = state.ready.current_bytes();
                            }
                            // Copy the core's eviction counters into the
                            // metrics (they are cumulative since clear).
                            let (ev, ev_bytes) = state.ready.eviction_stats();
                            state.metrics.entry_evictions = ev;
                            state.metrics.byte_evictions = ev_bytes;
                            state.metrics.current_entries = state.ready.len();
                            state.metrics.current_bytes = state.ready.current_bytes();
                            self.wake.notify_all();
                            return Ok(artifact);
                        }
                        Ok(Err(e)) => {
                            state.metrics.build_failures =
                                state.metrics.build_failures.saturating_add(1);
                            state.metrics.current_entries = state.ready.len();
                            state.metrics.current_bytes = state.ready.current_bytes();
                            self.wake.notify_all();
                            return Err(e);
                        }
                        Err(_panic) => {
                            state.metrics.build_failures =
                                state.metrics.build_failures.saturating_add(1);
                            state.metrics.current_entries = state.ready.len();
                            state.metrics.current_bytes = state.ready.current_bytes();
                            self.wake.notify_all();
                            return Err(ModelArtifactBuildError::Panicked);
                        }
                    }
                }
                std::collections::hash_map::Entry::Occupied(mut o) => {
                    // Someone else is building (or already registered as a
                    // waiter).  Register as a waiter and wait on the condvar.
                    #[cfg(all(feature = "cache-timing", not(loom)))]
                    let wait_t0 = std::time::Instant::now();
                    let waiters = match o.get() {
                        KeyState::Building { waiters } => *waiters,
                    };
                    o.insert(KeyState::Building {
                        waiters: waiters.saturating_add(1),
                    });
                    state.metrics.coalesced_waiters =
                        state.metrics.coalesced_waiters.saturating_add(1);
                    let mut state = state;

                    // ---- Wait loop (releases the lock) ----------------------
                    #[cfg(all(feature = "cache-timing", not(loom)))]
                    let record_wait = |self_: &Self| {
                        use std::sync::atomic::Ordering as AOrd;
                        self_.timing.single_flight_waits.fetch_add(1, AOrd::Relaxed);
                        self_
                            .timing
                            .single_flight_wait_ns
                            .fetch_add(wait_t0.elapsed().as_nanos() as u64, AOrd::Relaxed);
                    };
                    loop {
                        // Poison (unreachable in practice — critical sections
                        // are short and panic-free) abandons the wait: the
                        // outer loop's lock attempt then fails and the
                        // bypass path builds directly with the still-owned
                        // `build` closure.
                        let Some(g) =
                            crate::sync::wait_timeout(&self.wake, state, WAIT_POLL_INTERVAL)
                        else {
                            #[cfg(all(feature = "cache-timing", not(loom)))]
                            record_wait(self);
                            break;
                        };
                        state = g;
                        // Wake reasons: published (ready hit), builder
                        // finished without publishing (retry), or timeout.
                        if let Some(artifact) = state.ready.get(&key) {
                            self.finish_wait(&mut state, &key);
                            state.metrics.hits = state.metrics.hits.saturating_add(1);
                            #[cfg(all(feature = "cache-timing", not(loom)))]
                            record_wait(self);
                            return Ok(artifact);
                        }
                        if !state.in_flight.contains_key(&key) {
                            // Builder finished without publishing (failure or
                            // panic).  This waiter retries → becomes a
                            // builder via the outer loop (retry policy).
                            self.finish_wait(&mut state, &key);
                            #[cfg(all(feature = "cache-timing", not(loom)))]
                            record_wait(self);
                            break;
                        }
                        if cancel.is_some_and(|c| c.is_cancelled()) {
                            self.finish_wait(&mut state, &key);
                            #[cfg(all(feature = "cache-timing", not(loom)))]
                            record_wait(self);
                            return Err(ModelArtifactBuildError::Cancelled);
                        }
                    }
                }
            }
        }
    }

    /// Decrement a waiter registration, removing the in-flight marker when
    /// it reaches zero.  `waiters` is a diagnostic metric only — it never
    /// gates resource accounting, so saturation is the documented semantic
    /// and cannot hide an invariant violation.
    fn finish_wait(&self, state: &mut CacheState, key: &ModelCacheKey) {
        if let std::collections::hash_map::Entry::Occupied(mut o) =
            state.in_flight.entry(key.clone())
        {
            match o.get() {
                KeyState::Building { waiters } => {
                    if *waiters <= 1 {
                        o.remove();
                    } else {
                        o.insert(KeyState::Building {
                            waiters: waiters - 1,
                        });
                    }
                }
            }
        }
    }

    /// Take the cache-state lock, mapping poison to a typed sync failure.
    ///
    /// The caller treats a sync failure as "cache unavailable" → bypass
    /// (Phase O.6).  The lock is never held across user code (builds run
    /// outside it), so poisoning is unreachable in practice; the bypass path
    /// exists so even that unreachable case cannot corrupt decode semantics.
    fn lock_state(&self) -> Result<crate::sync::MutexGuard<'_, CacheState>, ModelCacheError> {
        #[cfg(all(feature = "cache-timing", not(loom)))]
        {
            use std::sync::atomic::Ordering as AOrd;
            let wt = std::time::Instant::now();
            let r = self
                .state
                .lock()
                .map_err(|_| ModelCacheError::Synchronization);
            self.timing.lock_acquires.fetch_add(1, AOrd::Relaxed);
            self.timing
                .lock_wait_ns
                .fetch_add(wt.elapsed().as_nanos() as u64, AOrd::Relaxed);
            return r;
        }
        #[cfg(not(all(feature = "cache-timing", not(loom))))]
        {
            self.state
                .lock()
                .map_err(|_| ModelCacheError::Synchronization)
        }
    }

    /// Record an uncached-fallback diagnostic (cache unavailable path).
    fn note_bypass(&self) {
        if let Ok(mut g) = self.state.lock() {
            g.metrics.uncached_fallbacks = g.metrics.uncached_fallbacks.saturating_add(1);
        }
    }

    /// Build the artifact directly with the canonical constructor, bypassing
    /// the cache entirely (disabled, poisoned, or accounting-failure paths).
    ///
    /// This is the *same* function the cache would call on a miss — there is
    /// no separate construction logic (Phase O.6/O.7).
    fn build_uncached(
        &self,
        build: impl FnOnce() -> Result<BuiltModelArtifacts, ModelArtifactBuildError>,
    ) -> Result<Arc<ValidatedModelArtifacts>, ModelArtifactBuildError> {
        #[cfg(all(feature = "cache-timing", not(loom)))]
        {
            use std::sync::atomic::Ordering as AOrd;
            let bt = std::time::Instant::now();
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(build));
            self.timing.artifact_builds.fetch_add(1, AOrd::Relaxed);
            self.timing
                .artifact_build_ns
                .fetch_add(bt.elapsed().as_nanos() as u64, AOrd::Relaxed);
            match r {
                Ok(Ok(b)) => Ok(Arc::new(b.artifacts)),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(ModelArtifactBuildError::Panicked),
            }
        }
        #[cfg(not(all(feature = "cache-timing", not(loom))))]
        {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(build)) {
                Ok(Ok(b)) => Ok(Arc::new(b.artifacts)),
                Ok(Err(e)) => Err(e),
                Err(_) => Err(ModelArtifactBuildError::Panicked),
            }
        }
    }

    /// Current metrics snapshot.
    ///
    /// Metrics are best-effort diagnostics: if the lock is poisoned the
    /// snapshot reports the last known-good values plus a `Synchronization`
    /// marker is *not* representable in the snapshot, so the caller should
    /// treat a poisoned cache as bypassed (see [`ModelArtifactCache::
    /// is_disabled`]).
    pub fn metrics(&self) -> ModelCacheMetricsSnapshot {
        match self.state.lock() {
            Ok(g) => g.metrics,
            Err(_) => ModelCacheMetricsSnapshot::default(),
        }
    }

    /// Cumulative contention timing snapshot (Phase O.16).
    ///
    /// Only available with the `cache-timing` feature; see
    /// [`ModelCacheTimingSnapshot`] for the counter semantics.  In builds
    /// without the feature the method does not exist (there is no zero
    /// snapshot to report — a caller that compiles timing reads is
    /// statically told the feature is off).
    #[cfg(all(feature = "cache-timing", not(loom)))]
    pub fn timing(&self) -> ModelCacheTimingSnapshot {
        self.timing.snapshot()
    }

    /// Reset the retained entries and the behavior counters.
    ///
    /// In-flight single-flight builds are *not* interrupted: their builders
    /// publish into the (cleared) cache when they finish.  This is the
    /// documented clear/shutdown semantic (Phase O.9 test 9).
    pub fn clear(&self) {
        if let Ok(mut g) = self.state.lock() {
            g.ready.clear();
            g.metrics = ModelCacheMetricsSnapshot::default();
        }
    }
}

/// Reference-counting wrapper so `ModelArtifactCache` can be shared cheaply
/// and `Debug`-printed without exposing internals.
impl core::fmt::Debug for CacheState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CacheState")
            .field("disabled", &self.disabled)
            .field("ready_entries", &self.ready.len())
            .field("in_flight", &self.in_flight.len())
            .field("current_bytes", &self.ready.current_bytes())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::Arc as SyncArc;

    fn key(seed: u8) -> ModelCacheKey {
        ModelCacheKey::from_model(7, 12, &[seed; 4])
    }

    fn arc_val(v: u32) -> Arc<u32> {
        Arc::new(v)
    }

    /// The O.1 exact-accounting core: sizes 1 B .. 1 MiB and u64-boundary
    /// adjacent values; after every operation the test independently
    /// recomputes the retained sum and compares it with the counter.
    #[test]
    fn exact_accounting_mixed_sizes() {
        let mut c: ModelCache<u32> = ModelCache::new(64, 1 << 30);
        let sizes: [u64; 8] = [
            1,
            99,
            100,
            101,
            1024,
            16 * 1024,
            1024 * 1024,
            u64::MAX - 1000, // boundary-adjacent; rejected (oversized)
        ];
        for (i, s) in sizes.iter().enumerate() {
            let k = key(i as u8);
            let out = c.insert(k, arc_val(i as u32), *s).unwrap();
            if *s > c.max_total_bytes() {
                assert_eq!(
                    out,
                    CacheInsertOutcome::RejectedOversized {
                        entry_bytes: *s,
                        max_total_bytes: c.max_total_bytes()
                    }
                );
            } else {
                assert_eq!(out, CacheInsertOutcome::Inserted);
            }
            c.invariant_check().expect("invariants after insert");
        }
        assert!(c.len() <= c.max_entries());
    }

    /// O.1: exact-size entry equal to the total budget is admitted; one byte
    /// larger is rejected.
    #[test]
    fn exact_budget_boundary() {
        let mut c: ModelCache<u32> = ModelCache::new(4, 100);
        assert_eq!(
            c.insert(key(0), arc_val(0), 100).unwrap(),
            CacheInsertOutcome::Inserted
        );
        c.invariant_check().unwrap();
        assert_eq!(
            c.insert(key(1), arc_val(1), 101).unwrap(),
            CacheInsertOutcome::RejectedOversized {
                entry_bytes: 101,
                max_total_bytes: 100
            }
        );
        // The rejected insert must not have evicted the admitted entry.
        assert_eq!(c.len(), 1);
        assert_eq!(c.current_bytes(), 100);
        c.invariant_check().unwrap();
    }

    /// O.2: zero entry capacity and zero byte capacity both disable.
    #[test]
    fn zero_capacity_disables() {
        for (m, b) in [(0usize, 1024u64), (8, 0)] {
            let mut c: ModelCache<u32> = ModelCache::new(m, b);
            assert!(c.is_disabled());
            assert_eq!(
                c.insert(key(0), arc_val(0), 10).unwrap(),
                CacheInsertOutcome::RejectedDisabled
            );
            assert_eq!(c.len(), 0);
            assert_eq!(c.current_bytes(), 0);
            c.invariant_check().unwrap();
        }
    }

    /// O.3: duplicate-key insertion replaces, never duplicates.
    #[test]
    fn duplicate_key_replaces() {
        let mut c: ModelCache<u32> = ModelCache::new(8, 1024);
        let k = key(3);
        assert_eq!(
            c.insert(k.clone(), arc_val(1), 100).unwrap(),
            CacheInsertOutcome::Inserted
        );
        assert_eq!(
            c.insert(k.clone(), arc_val(2), 200).unwrap(),
            CacheInsertOutcome::Replaced
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c.current_bytes(), 200);
        assert_eq!(c.get(&k).map(|v| *v), Some(2));
        c.invariant_check().unwrap();
    }

    /// O.3: mixed-size FIFO eviction with exact byte accounting.
    #[test]
    fn mixed_size_fifo_eviction() {
        let mut c: ModelCache<u32> = ModelCache::new(3, 1000);
        for (i, s) in [(0u8, 300u64), (1, 300), (2, 300)] {
            assert_eq!(
                c.insert(key(i), arc_val(i as u32), s).unwrap(),
                CacheInsertOutcome::Inserted
            );
        }
        c.invariant_check().unwrap();
        // A 500-byte entry needs 900+500 = 1400 bytes but the budget is 1000,
        // so the two oldest 300-byte entries are evicted (not one): the byte
        // bound and the count bound are both enforced exactly.
        assert_eq!(
            c.insert(key(3), arc_val(3), 500).unwrap(),
            CacheInsertOutcome::Inserted
        );
        assert_eq!(c.len(), 2);
        assert_eq!(c.current_bytes(), 300 + 500);
        assert!(c.get(&key(0)).is_none(), "oldest evicted");
        assert!(c.get(&key(1)).is_none(), "second-oldest evicted");
        assert!(c.get(&key(2)).is_some());
        assert!(c.get(&key(3)).is_some());
        c.invariant_check().unwrap();
        let (ev, ev_bytes) = c.eviction_stats();
        assert_eq!(ev, 2);
        assert_eq!(ev_bytes, 600);
    }

    /// O.1: the eviction-first design makes byte-overflow unreachable from
    /// well-formed inputs — an entry is never admitted when the sum would
    /// overflow, because the eviction plan runs first and the oversized
    /// guard rejects `entry_bytes > max_total_bytes` before any arithmetic.
    /// The checked arithmetic in [`ModelCache::insert`] is the defensive
    /// guard for that (unreachable) invariant violation; this test pins the
    /// boundary behavior: a u64::MAX budget admits a u64::MAX entry, and a
    /// second one evicts the first (exact accounting, no overflow panic).
    #[test]
    fn u64_boundary_accounting() {
        let mut c: ModelCache<u32> = ModelCache::new(2, u64::MAX);
        assert_eq!(
            c.insert(key(0), arc_val(0), u64::MAX).unwrap(),
            CacheInsertOutcome::Inserted
        );
        assert_eq!(c.current_bytes(), u64::MAX);
        // A second u64::MAX entry cannot coexist: the plan evicts the first
        // (0 + u64::MAX fits), so the outcome is a clean replacement-by-
        //-eviction, never an overflow panic and never a violated bound.
        assert_eq!(
            c.insert(key(1), arc_val(1), u64::MAX).unwrap(),
            CacheInsertOutcome::Inserted
        );
        assert_eq!(c.len(), 1);
        assert_eq!(c.current_bytes(), u64::MAX);
        assert!(c.get(&key(0)).is_none());
        assert!(c.get(&key(1)).is_some());
        c.invariant_check().unwrap();
    }

    /// O.1: clear resets everything exactly.
    #[test]
    fn clear_resets() {
        let mut c: ModelCache<u32> = ModelCache::new(8, 1024);
        c.insert(key(0), arc_val(0), 100).unwrap();
        c.insert(key(1), arc_val(1), 100).unwrap();
        c.clear();
        assert_eq!(c.len(), 0);
        assert_eq!(c.current_bytes(), 0);
        assert_eq!(c.eviction_stats(), (0, 0));
        c.invariant_check().unwrap();
    }

    /// O.7: the canonical constructor rejects corrupt models and builds
    /// valid ones with exact accounted bytes.
    #[test]
    fn canonical_constructor_validation() {
        // Empty model at scale 12 → uniform freq 16 → valid.
        let b = build_validated_model_artifacts(7, 12, &[]).unwrap();
        assert_eq!(b.artifacts.freqs.len(), 256);
        assert!(b.artifacts.uniform256);
        // Exact accounted bytes: 1 KiB frequencies + fixed overhead, plus the
        // 16 KiB packed table (4096 x u32) on SIMD builds.
        let mut expected = 256 * 4 + ARTIFACT_FIXED_OVERHEAD;
        #[cfg(feature = "simd")]
        {
            expected += 4096 * 4;
        }
        assert_eq!(b.accounted_bytes, expected);

        // Truncated model (not 0, not 1024).
        assert_eq!(
            build_validated_model_artifacts(7, 12, &[0u8; 100]).unwrap_err(),
            ModelArtifactBuildError::InvalidFrequencyCount
        );
        // Scale >= 32 → checked-shift rejection, never a panic.
        assert_eq!(
            build_validated_model_artifacts(7, 40, &[]).unwrap_err(),
            ModelArtifactBuildError::UnsupportedScale
        );
        // Sum mismatch.
        let mut bad = vec![0u8; 1024];
        bad[0] = 1;
        assert_eq!(
            build_validated_model_artifacts(7, 12, &bad).unwrap_err(),
            ModelArtifactBuildError::InvalidFrequencySum
        );
    }

    /// O.7: key completeness — mutating each key input independently misses.
    #[test]
    fn key_completeness() {
        // A valid uniform256 model: 256 symbols x freq 16 (sum 4096 = 1<<12),
        // serialized as 256 x u32 LE (NOT [16u8; 1024], whose u32 words are
        // 0x10101010 — a subtle byte-order trap this test documents).
        let mut model = Vec::with_capacity(1024);
        for _ in 0..256 {
            model.extend_from_slice(&16u32.to_le_bytes());
        }
        let base = build_validated_model_artifacts(7, 12, &model).unwrap();
        let k_base = ModelCacheKey::from_model(7, 12, &model);
        let k_other_codec = ModelCacheKey::from_model(8, 12, &model);
        let k_other_scale = ModelCacheKey::from_model(7, 11, &model);
        let mut other = model.clone();
        other[0] ^= 1; // mutate one model byte → different hash
        let k_other_bytes = ModelCacheKey::from_model(7, 12, &other);
        assert_ne!(k_base, k_other_codec);
        assert_ne!(k_base, k_other_scale);
        assert_ne!(k_base, k_other_bytes);
        // Sanity: the artifacts themselves are key-independent (backend
        // policy lives outside the key — the artifacts are shared across
        // policies by design).
        let _ = base.artifacts.freqs.len();
    }

    /// O.4/O.8: disabled cache builds directly, retains nothing, and counts
    /// disabled bypasses.
    #[test]
    fn disabled_cache_builds_directly() {
        let cache = ModelArtifactCache::disabled();
        let built = cache
            .get_or_build(7, 12, &[], None, || {
                build_validated_model_artifacts(7, 12, &[])
            })
            .unwrap();
        assert_eq!(built.freqs.len(), 256);
        assert!(built.uniform256);
        let m = cache.metrics();
        assert_eq!(m.disabled_bypasses, 1);
        assert_eq!(m.current_entries, 0);
        assert_eq!(m.current_bytes, 0);
        assert_eq!(m.lookups, 0, "disabled cache never performs lookups");
    }

    /// O.4/O.8: a bounded cache returns the same artifact on hit (Arc
    /// identity) without a second build.
    #[test]
    fn bounded_cache_hit_shares_arc() {
        let cache = ModelArtifactCache::bounded(8, 1 << 20);
        let builds = SyncArc::new(std::sync::atomic::AtomicUsize::new(0));
        let build_fn = || {
            builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            build_validated_model_artifacts(7, 12, &[])
        };
        let a1 = cache.get_or_build(7, 12, &[], None, &build_fn).unwrap();
        let a2 = cache.get_or_build(7, 12, &[], None, &build_fn).unwrap();
        assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(SyncArc::ptr_eq(&a1, &a2));
        let m = cache.metrics();
        assert_eq!(m.lookups, 2);
        assert_eq!(m.hits, 1);
        assert_eq!(m.misses, 1);
        assert_eq!(m.builds_started, 1);
        assert_eq!(m.builds_completed, 1);
        assert_eq!(m.insertions, 1);
        assert!(m.invariant_hit_miss_sum());
        assert!(m.invariant_build_accounting());
    }

    /// O.6: cache-disabled and cached paths return identical outputs and
    /// semantic errors (both call the same constructor).
    #[test]
    fn disabled_and_cached_error_equivalence() {
        let disabled = ModelArtifactCache::disabled();
        let cached = ModelArtifactCache::bounded(8, 1 << 20);
        let bad = [0u8; 100]; // invalid length → InvalidFrequencyCount
        let e_disabled = disabled.get_or_build(7, 12, &bad, None, || {
            build_validated_model_artifacts(7, 12, &bad)
        });
        let e_cached = cached.get_or_build(7, 12, &bad, None, || {
            build_validated_model_artifacts(7, 12, &bad)
        });
        let d_err = e_disabled.err();
        assert_eq!(d_err, e_cached.err());
        assert_eq!(d_err, Some(ModelArtifactBuildError::InvalidFrequencyCount));
        // A corrupt model is never admitted.
        assert_eq!(cached.metrics().current_entries, 0);
        assert_eq!(cached.metrics().build_failures, 1);
    }

    /// O.5: a builder panic is caught, converts to a typed error, releases
    /// waiters, and leaves no permanent Building state.
    #[test]
    fn builder_panic_is_typed_and_retryable() {
        let cache = ModelArtifactCache::bounded(8, 1 << 20);
        let panic_fn = || -> Result<BuiltModelArtifacts, ModelArtifactBuildError> {
            panic!("deliberate builder panic");
        };
        let e1 = cache.get_or_build(7, 12, &[], None, panic_fn);
        assert_eq!(e1.err(), Some(ModelArtifactBuildError::Panicked));
        // The key is retryable: the next request can succeed.
        let ok = cache
            .get_or_build(7, 12, &[], None, || {
                build_validated_model_artifacts(7, 12, &[])
            })
            .unwrap();
        assert_eq!(ok.freqs.len(), 256);
        let m = cache.metrics();
        assert_eq!(m.builds_started, 2);
        assert_eq!(m.build_failures, 1);
        assert_eq!(m.builds_completed, 1);
    }

    /// O.2: oversized artifacts bypass the cache but are still delivered for
    /// the current decode.
    #[test]
    fn oversized_bypasses_but_delivers() {
        let cache = ModelArtifactCache::bounded(8, 100); // tiny budget
        let artifact = cache
            .get_or_build(7, 12, &[], None, || {
                build_validated_model_artifacts(7, 12, &[])
            })
            .unwrap();
        assert_eq!(artifact.freqs.len(), 256);
        let m = cache.metrics();
        assert_eq!(m.oversized_rejections, 1);
        assert_eq!(m.current_entries, 0, "oversized never retained");
        assert_eq!(m.current_bytes, 0);
        // The valid artifact is still usable for the current decode.
        assert!(artifact.uniform256);
    }
}

// ---------------------------------------------------------------------------
// Phase O.9 — concurrency courts (real threads) and property tests
// ---------------------------------------------------------------------------

/// O.5/O.9: N concurrent cold requests for one key perform exactly one
/// construction, and every caller receives the same `Arc` artifact.
#[test]
fn concurrent_same_key_cold_burst_single_build() {
    const N: usize = 8;
    let cache = ModelArtifactCache::bounded(16, 1 << 20);
    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = Vec::new();
    for _ in 0..N {
        let cache = cache.clone();
        let builds = builds.clone();
        handles.push(std::thread::spawn(move || {
            let a = cache
                .get_or_build(7, 12, &[], None, || {
                    builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    build_validated_model_artifacts(7, 12, &[])
                })
                .expect("build must succeed");
            crate::sync::Arc::as_ptr(&a) as usize
        }));
    }
    let mut ptrs = Vec::new();
    for h in handles {
        ptrs.push(h.join().expect("worker join"));
    }
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(
        ptrs.iter().all(|&p| p == ptrs[0]),
        "all callers must receive the same Arc artifact"
    );
    let m = cache.metrics();
    assert_eq!(m.builds_started, 1);
    // Coalescing is schedule-dependent: threads that arrive after the build
    // is published take the hit fast path and never register as waiters.
    // The load-bearing single-flight facts are `builds == 1` and the shared
    // Arc; the waiter count is bounded, not exact.
    assert!(m.coalesced_waiters <= N as u64 - 1);
    assert!(m.invariant_build_accounting());
}

/// O.5: different-key concurrent requests build each key exactly once and
/// never confuse keys.
#[test]
fn concurrent_different_keys_each_build_once() {
    const N: usize = 8;
    let cache = ModelArtifactCache::bounded(64, 1 << 20);
    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut handles = Vec::new();
    for i in 0..N {
        let cache = cache.clone();
        let builds = builds.clone();
        handles.push(std::thread::spawn(move || {
            // Each thread uses a *valid* model that still hashes differently:
            // freq[sym_a] = 17, freq[sym_b] = 15, the rest 16 → sum 4096.
            // (A naive `[i as u8; 1024]` would be an invalid model — every
            // u32 word would be a huge frequency and the sum would not be
            // 1 << scale — a byte-order trap worth documenting.)
            let mut model = Vec::with_capacity(1024);
            for sym in 0..256u32 {
                let f: u32 = if sym == i as u32 * 31 % 256 {
                    17
                } else if sym == (i as u32 * 31 + 1) % 256 {
                    15
                } else {
                    16
                };
                model.extend_from_slice(&f.to_le_bytes());
            }
            let a = cache
                .get_or_build(7, 12, &model, None, || {
                    builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    build_validated_model_artifacts(7, 12, &model)
                })
                .expect("build must succeed");
            a.freqs.len()
        }));
    }
    for h in handles {
        assert_eq!(h.join().expect("worker join"), 256);
    }
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), N);
    let m = cache.metrics();
    assert_eq!(m.insertions, N as u64);
    assert_eq!(m.current_entries, N as usize);
}

/// O.5: a concurrent burst where the builder fails: every caller receives
/// the same typed error, nothing is admitted, and a later retry succeeds.
#[test]
fn concurrent_build_failure_same_error_then_retry() {
    let cache = ModelArtifactCache::bounded(16, 1 << 20);
    let bad = [0u8; 100]; // invalid length
    let mut handles = Vec::new();
    for _ in 0..4 {
        let cache = cache.clone();
        handles.push(std::thread::spawn(move || {
            let r = cache.get_or_build(7, 12, &bad, None, || {
                build_validated_model_artifacts(7, 12, &bad)
            });
            r.err()
        }));
    }
    for h in handles {
        assert_eq!(
            h.join().expect("worker join"),
            Some(ModelArtifactBuildError::InvalidFrequencyCount)
        );
    }
    let m = cache.metrics();
    assert_eq!(m.current_entries, 0, "corrupt model never admitted");
    assert!(m.build_failures >= 1);
    // Retry policy: a later request with a valid model succeeds.
    let ok = cache
        .get_or_build(7, 12, &[], None, || {
            build_validated_model_artifacts(7, 12, &[])
        })
        .expect("retry must succeed");
    assert_eq!(ok.freqs.len(), 256);
}

/// O.5: a cancelled *builder* yields the build to the next caller
/// ("another waiter takes over", documented semantic).
///
/// Deterministic scenario: the single cancelled caller becomes the builder
/// (no competitor), observes its own pre-cancelled token before
/// construction starts, removes the in-flight marker, and returns
/// `Cancelled`.  The next (uncancelled) caller builds and receives the
/// artifact.  Exactly one construction succeeds.
#[test]
fn cancelled_builder_yields_to_next_caller() {
    let cache = ModelArtifactCache::bounded(16, 1 << 20);
    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let cancelled = std::sync::Arc::new(crate::cancellation::CancellationToken::new());
    cancelled.cancel();
    let build_fn = || {
        builds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        build_validated_model_artifacts(7, 12, &[])
    };
    let e = cache
        .get_or_build(7, 12, &[], Some(&cancelled), build_fn)
        .err();
    assert_eq!(e, Some(ModelArtifactBuildError::Cancelled));
    let a = cache
        .get_or_build(7, 12, &[], None, build_fn)
        .expect("successor builds");
    assert_eq!(a.freqs.len(), 256);
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);
    let m = cache.metrics();
    assert_eq!(m.current_entries, 1);
}

/// O.5: a cancelled *waiter* stops waiting without corrupting the shared
/// build; the builder (uncancelled) completes and publishes.
///
/// Deterministic scenario: the builder's closure signals `started` *after*
/// the in-flight marker is set, then sleeps; the main thread — guaranteed
/// to find the marker and register as a waiter — cancels its token and
/// observes `Cancelled` on the next wait poll, long before the builder
/// publishes.  The build still completes exactly once and is published.
#[test]
fn cancelled_waiter_stops_waiting_build_completes() {
    use std::sync::mpsc;
    let cache = ModelArtifactCache::bounded(16, 1 << 20);
    let builds = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (tx, rx) = mpsc::channel::<()>();

    let cache_b = cache.clone();
    let builds_b = builds.clone();
    let builder = std::thread::spawn(move || {
        cache_b
            .get_or_build(7, 12, &[], None, || {
                builds_b.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                tx.send(()).expect("started signal");
                std::thread::sleep(std::time::Duration::from_millis(120));
                build_validated_model_artifacts(7, 12, &[])
            })
            .expect("builder succeeds")
    });

    // The marker is guaranteed set once `started` fires (the marker is
    // inserted before the build closure runs).
    rx.recv().expect("started");

    // Main becomes a waiter (the marker is definitely present).
    let cancel = std::sync::Arc::new(crate::cancellation::CancellationToken::new());
    let cache_w = cache.clone();
    let cancel_w = cancel.clone();
    let waiter = std::thread::spawn(move || {
        cache_w
            .get_or_build(7, 12, &[], Some(&cancel_w), || {
                build_validated_model_artifacts(7, 12, &[])
            })
            .unwrap_err()
    });

    // Give the waiter time to register and block in the condvar wait, then
    // cancel.  The 10 ms wait poll picks it up long before the builder's
    // 120 ms sleep ends.
    std::thread::sleep(std::time::Duration::from_millis(30));
    cancel.cancel();

    let e = waiter.join().expect("waiter join");
    assert_eq!(e, ModelArtifactBuildError::Cancelled);
    let a = builder.join().expect("builder join");
    assert_eq!(a.freqs.len(), 256);
    assert_eq!(builds.load(std::sync::atomic::Ordering::SeqCst), 1);
    let m = cache.metrics();
    assert_eq!(m.builds_completed, 1);
    assert_eq!(
        m.current_entries, 1,
        "the build is published for future callers"
    );
}

// ---------------------------------------------------------------------------
// Phase O.9 — property tests (proptest)
// ---------------------------------------------------------------------------

/// O.9: randomized mixed-size insert/replace sequences must keep the cache's
/// counters equal to an independently recomputed shadow model after every
/// operation.
#[cfg(test)]
mod cache_proptests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::VecDeque;

    fn key(seed: u8) -> ModelCacheKey {
        ModelCacheKey::from_model(7, 12, &[seed; 4])
    }

    fn arc_val(v: u32) -> Arc<u32> {
        Arc::new(v)
    }

    proptest! {
        #[test]
        fn counters_match_shadow_model(
            ops in proptest::collection::vec(
                (any::<u8>(), any::<u64>(), any::<bool>()),
                1..300,
            ),
            max_entries in 1usize..17,
            max_bytes in 16u64..8192,
        ) {
            let mut c: ModelCache<u32> = ModelCache::new(max_entries, max_bytes);
            // Shadow model: (key, bytes) pairs in FIFO order.
            let mut shadow: VecDeque<(ModelCacheKey, u64)> = VecDeque::new();
            let mut shadow_bytes = 0u64;

            for (seed, size, replace) in ops {
                let k = key(seed);
                let sz = size % (max_bytes + 64); // mixes admissible + oversized
                let _out = c.insert(k.clone(), arc_val(seed as u32), sz);
                if replace {
                    // Re-insert the same key: exercises the replacement path
                    // (exact byte subtraction + FIFO re-queue) under the
                    // shadow model's remove-then-push semantics.
                    let _out2 = c.insert(k.clone(), arc_val(seed as u32), sz);
                }

                // Recompute the shadow independently.
                if sz > max_bytes || max_entries == 0 || max_bytes == 0 {
                    // rejected / disabled: shadow unchanged
                } else {
                    // replacement: remove old entry for the same key
                    if let Some(pos) = shadow.iter().position(|(sk, _)| *sk == k) {
                        let (_, old_sz) = shadow.remove(pos).unwrap();
                        shadow_bytes -= old_sz;
                    }
                    // evict FIFO until it fits
                    while (shadow.len() >= max_entries
                        || shadow_bytes + sz > max_bytes)
                        && !shadow.is_empty()
                    {
                        let (_, old_sz) = shadow.pop_front().unwrap();
                        shadow_bytes -= old_sz;
                    }
                    shadow.push_back((k.clone(), sz));
                    shadow_bytes += sz;
                }

                // Exact invariants: counters == independent recomputation.
                c.invariant_check().expect("invariants after every op");
                assert_eq!(c.len(), shadow.len(), "entry count vs shadow");
                assert_eq!(c.current_bytes(), shadow_bytes, "byte total vs shadow");
                // Retained key set equality.
                for (sk, _) in &shadow {
                    assert!(c.get(sk).is_some(), "shadow key must be retained");
                }
                assert!(c.current_bytes() <= max_bytes);
                assert!(c.len() <= max_entries);
            }
        }
    }
}
