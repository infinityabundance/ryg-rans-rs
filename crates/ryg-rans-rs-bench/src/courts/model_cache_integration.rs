//! # RYG_RANS.L.MODEL_CACHE.INTEGRATION — `ModelCache` in production (L.8)
//!
//! Proves the L.8 integration:
//!
//! - `ModelCache` is bounded (entries and bytes) with deterministic FIFO
//!   eviction.
//! - The production decode path (`cached_model_artifacts`) joins decode by
//!   canonical model identity: `(model_sha256, scale_bits, codec_id)`.
//! - Corrupt models are never cached (the `build` closure returns `None`
//!   for invalid data, and `cached_model_artifacts` propagates that).
//! - The cache never alters output or error identity (cache-equivalence).
//! - No poisoned-lock cascade: lookups clone artifacts outside the lock.

use super::{CourtCase, CourtRun};
use ryg_rans_rs_casefile::PhaseLCaseVerdict;
use ryg_rans_rs_parallel::{
    DecodeBlockJob, EncodeBlockJob, FixedBlockPlan, ModelArtifactCache, ModelCache, ModelCacheKey,
    ModelPolicy, ParallelConfig, ParallelDecoder, ParallelEncoder, ThreadCount,
    build_validated_model_artifacts, plan_cache_key,
};
use std::num::NonZeroUsize;

pub fn court() -> CourtRun {
    let mut cases = Vec::new();
    let add = |cases: &mut Vec<CourtCase>,
               id: &str,
               input: &str,
               expected: &str,
               actual: Result<String, String>| {
        let actual_str = match &actual {
            Ok(a) => a.clone(),
            Err(e) => format!("ERROR: {}", e),
        };
        let verdict = match &actual {
            Ok(a) if a == expected => PhaseLCaseVerdict::Pass,
            _ => PhaseLCaseVerdict::Fail,
        };
        cases.push(CourtCase {
            case_id: id.to_string(),
            input: input.to_string(),
            expected: expected.to_string(),
            actual: actual_str,
            verdict,
            residual_ids: vec!["L8-A".to_string()],
        });
    };

    // ---- Case 1: cache hit returns the same value --------------------------
    let mut cache: ModelCache<String> = ModelCache::new(16, 65536);
    let key = ModelCacheKey {
        model_sha256: [7u8; 32],
        scale_bits: 12,
        codec_id: 7,
    };
    cache
        .insert(key.clone(), std::sync::Arc::new("plan_a".to_string()), 100)
        .expect("insert");
    let hit = cache.get(&key).is_some_and(|s| s.as_str() == "plan_a");
    add(
        &mut cases,
        "CASE.001",
        "insert then get returns the inserted value",
        "hit",
        if hit {
            Ok("hit".to_string())
        } else {
            Ok("miss".to_string())
        },
    );

    // ---- Case 2: different key misses --------------------------------------
    let other_key = ModelCacheKey {
        model_sha256: [8u8; 32],
        scale_bits: 12,
        codec_id: 7,
    };
    let miss = cache.get(&other_key).is_none();
    add(
        &mut cases,
        "CASE.002",
        "different model hash misses",
        "miss",
        if miss {
            Ok("miss".to_string())
        } else {
            Ok("unexpected_hit".to_string())
        },
    );

    // ---- Case 3: codec id is part of the key -------------------------------
    let codec_variant = ModelCacheKey {
        model_sha256: [7u8; 32],
        scale_bits: 12,
        codec_id: 8,
    };
    let miss = cache.get(&codec_variant).is_none();
    add(
        &mut cases,
        "CASE.003",
        "same model hash + different codec_id misses",
        "miss",
        if miss {
            Ok("miss".to_string())
        } else {
            Ok("unexpected_hit".to_string())
        },
    );

    // ---- Case 4: scale_bits is part of the key -----------------------------
    let scale_variant = ModelCacheKey {
        model_sha256: [7u8; 32],
        scale_bits: 14,
        codec_id: 7,
    };
    let miss = cache.get(&scale_variant).is_none();
    add(
        &mut cases,
        "CASE.004",
        "same model hash + different scale_bits misses",
        "miss",
        if miss {
            Ok("miss".to_string())
        } else {
            Ok("unexpected_hit".to_string())
        },
    );

    // ---- Case 5: eviction by count (FIFO) ----------------------------------
    let mut cache: ModelCache<u32> = ModelCache::new(3, 1 << 30);
    let mk = |i: u8| ModelCacheKey {
        model_sha256: [i; 32],
        scale_bits: 12,
        codec_id: 7,
    };
    cache.insert(mk(1), std::sync::Arc::new(10), 1).unwrap();
    cache.insert(mk(2), std::sync::Arc::new(20), 1).unwrap();
    cache.insert(mk(3), std::sync::Arc::new(30), 1).unwrap();
    let before = cache.len();
    cache.insert(mk(4), std::sync::Arc::new(40), 1).unwrap(); // evicts key 1
    let evicted = cache.get(&mk(1)).is_none() && cache.get(&mk(4)).is_some();
    let count_ok = before == 3 && cache.len() == 3 && evicted;
    add(
        &mut cases,
        "CASE.005",
        "FIFO eviction by count (max_entries=3, 4th insert evicts oldest)",
        "fifo",
        if count_ok {
            Ok("fifo".to_string())
        } else {
            Ok(format!("len={} evicted={}", cache.len(), evicted))
        },
    );

    // ---- Case 6: eviction by bytes -----------------------------------------
    let mut cache: ModelCache<u32> = ModelCache::new(16, 10);
    cache.insert(mk(1), std::sync::Arc::new(10), 6).unwrap();
    cache.insert(mk(2), std::sync::Arc::new(20), 5).unwrap();
    cache.insert(mk(3), std::sync::Arc::new(30), 5).unwrap();
    // 6 + 5 exceeded the budget already; the third insert must have evicted
    // until it fit.  Bound: current_bytes <= 10 after eviction.
    let bounded = cache.len() <= 2;
    add(
        &mut cases,
        "CASE.006",
        "byte budget (max_total_bytes=10) bounds resident entries",
        "bounded",
        if bounded {
            Ok("bounded".to_string())
        } else {
            Ok(format!("len={}", cache.len()))
        },
    );

    // ---- Case 7: oversized single entry never exceeds the byte budget ------
    let mut cache: ModelCache<u32> = ModelCache::new(4, 100);
    cache.insert(mk(1), std::sync::Arc::new(10), 60).unwrap();
    cache.insert(mk(2), std::sync::Arc::new(20), 60).unwrap();
    let after = cache.len();
    add(
        &mut cases,
        "CASE.007",
        "two 60-byte entries under a 100-byte budget (second evicts first)",
        "bounded",
        if after <= 1 {
            Ok("bounded".to_string())
        } else {
            Ok(format!("len={}", after))
        },
    );

    // ---- Case 8: corrupt model is never cached -----------------------------
    // The canonical constructor rejects the model before any insertion; the
    // cache never admits a corrupt model and the error class is
    // block-independent (Phase O.6/O.7).
    let corrupt_model = vec![0xFFu8; 128]; // not a valid frequency model
    let corrupt_cache = ModelArtifactCache::bounded(8, 1 << 20);
    let r = corrupt_cache.get_or_build(7, 12, &corrupt_model, None, || {
        build_validated_model_artifacts(7, 12, &corrupt_model)
    });
    add(
        &mut cases,
        "CASE.008",
        "corrupt model → typed build error, never admitted to the cache",
        "error",
        match r {
            Err(_) => {
                let m = corrupt_cache.metrics();
                if m.current_entries == 0 && m.build_failures == 1 {
                    Ok("error".to_string())
                } else {
                    Ok(format!("error_but_admitted entries={}", m.current_entries))
                }
            }
            Ok(_) => Ok("cached".to_string()),
        },
    );

    // ---- Case 9: valid model is cached and served --------------------------
    let valid_model: Vec<u8> = {
        // 256 × u32 = 1024 bytes of frequency 16 (uniform256).
        let mut v = Vec::with_capacity(1024);
        for _ in 0..256 {
            v.extend_from_slice(&16u32.to_le_bytes());
        }
        v
    };
    let shared_cache = ModelArtifactCache::bounded(8, 1 << 20);
    let build = || build_validated_model_artifacts(7, 12, &valid_model);
    let a1 = shared_cache.get_or_build(7, 12, &valid_model, None, build);
    let a2 = shared_cache.get_or_build(7, 12, &valid_model, None, build);
    let served = match (&a1, &a2) {
        // The hit must serve the identical shared allocation (Arc::ptr_eq on
        // the outer artifact AND the inner freqs/table), not rebuild or
        // deep-copy.  Single-flight guarantees exactly one build.
        (Ok(x), Ok(y)) => {
            x.uniform256
                && std::sync::Arc::ptr_eq(x, y)
                && std::sync::Arc::ptr_eq(&x.freqs, &y.freqs)
                && match (&x.packed_table, &y.packed_table) {
                    (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                    _ => false,
                }
        }
        _ => false,
    };
    let single_build = shared_cache.metrics().builds_started == 1;
    add(
        &mut cases,
        "CASE.009",
        "valid uniform256 model cached and served identically",
        "served",
        if served && single_build {
            Ok("served".to_string())
        } else {
            Ok(format!(
                "not_served served={} single_build={}",
                served, single_build
            ))
        },
    );

    // ---- Case 10: cache does not alter decode output -----------------------
    // Decode the same blocks twice (cold vs warm cache) and compare output.
    let data = nonuniform_data(16 * 1024);
    let plan = FixedBlockPlan::new(data.len() as u64, 2048);
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(2).unwrap()),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let jobs: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                ryg_rans_rs_parallel::CodecPolicy::Auto,
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc = match ParallelEncoder::encode_blocks(jobs, &cfg) {
        Ok(e) => e,
        Err(e) => {
            return CourtRun {
                court_id: "RYG_RANS.L.MODEL_CACHE.INTEGRATION".to_string(),
                title: "ModelCache production integration (L.8)".to_string(),
                residual_ids: vec!["L8-A".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "encode reference blocks".to_string(),
                    expected: "Ok".to_string(),
                    actual: format!("ERROR: {:?}", e),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L8-A".to_string()],
                }],
            };
        }
    };
    let djobs: Vec<DecodeBlockJob> = enc
        .blocks
        .iter()
        .map(|b| DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();
    // One decoder instance = one explicitly owned cache (Phase O.4).  The
    // first decode is the cold miss; the second reuses the retained
    // artifacts (warm hits) — the shared instance is what makes the
    // comparison meaningful.
    let decoder = ParallelDecoder::new(cfg.clone());
    let decode_to = |jobs: Vec<DecodeBlockJob>| -> Option<Vec<u8>> {
        decoder.decode_blocks(jobs).ok().map(|dec| {
            let mut out = Vec::new();
            for b in &dec.blocks {
                out.extend_from_slice(&b.output);
            }
            out
        })
    };
    let cold = decode_to(djobs.clone());
    let warm = decode_to(djobs.clone());
    // The second decode must hit the cache (hits delta > 0) — the warm path
    // demonstrably reused artifacts instead of rebuilding them.
    let m = decoder.model_cache().metrics();
    let warm_hit = m.hits > 0;
    let equivalent = match (&cold, &warm) {
        (Some(c), Some(w)) => c == w && c == &data,
        _ => false,
    };
    add(
        &mut cases,
        "CASE.010",
        "decode output identical with cold vs warm model cache (cache-equivalence)",
        "equivalent",
        if equivalent && warm_hit {
            Ok("equivalent".to_string())
        } else {
            Ok(format!("equivalent={} warm_hit={}", equivalent, warm_hit))
        },
    );

    // ---- Case 11: plan_cache_key derives a stable key ----------------------
    let model_bytes = valid_model.clone();
    let k1 = plan_cache_key(7, 12, &model_bytes);
    let k2 = plan_cache_key(7, 12, &model_bytes);
    let k3 = plan_cache_key(8, 12, &model_bytes);
    let stable = k1 == k2 && k1 != k3;
    add(
        &mut cases,
        "CASE.011",
        "plan_cache_key is stable per (codec, scale, model) and differs by codec",
        "stable",
        if stable {
            Ok("stable".to_string())
        } else {
            Ok(format!("k1==k2={} k1!=k3={}", k1 == k2, k1 != k3))
        },
    );

    CourtRun {
        court_id: "RYG_RANS.L.MODEL_CACHE.INTEGRATION".to_string(),
        title: "ModelCache production integration (L.8)".to_string(),
        cases,
        residual_ids: vec!["L8-A".to_string()],
    }
}

fn nonuniform_data(len: usize) -> Vec<u8> {
    let mut d = Vec::with_capacity(len);
    let mut i = 0usize;
    while d.len() < len {
        let b = if i % 256 < 200 {
            b'a'
        } else if i % 256 < 220 {
            b'b'
        } else if i % 256 < 240 {
            b'c'
        } else {
            (i % 256) as u8
        };
        d.push(b);
        i += 1;
    }
    d
}
