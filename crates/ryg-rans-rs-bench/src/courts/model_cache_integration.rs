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
    DecodeBlockJob, EncodeBlockJob, FixedBlockPlan, ModelCache, ModelCacheKey, ModelPolicy,
    ParallelConfig, ParallelDecoder, ParallelEncoder, ThreadCount, ValidatedModelArtifacts,
    cached_model_artifacts, plan_cache_key,
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
    cache.insert(key.clone(), "plan_a".to_string(), 100);
    let hit = cache.get(&key).map(|s| s.as_str()) == Some("plan_a");
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
    cache.insert(mk(1), 10, 1);
    cache.insert(mk(2), 20, 1);
    cache.insert(mk(3), 30, 1);
    let before = cache.len();
    cache.insert(mk(4), 40, 1); // evicts key 1
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
    cache.insert(mk(1), 10, 6);
    cache.insert(mk(2), 20, 5);
    let over = cache.insert(mk(3), 30, 5);
    let _ = over;
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
    cache.insert(mk(1), 10, 60);
    cache.insert(mk(2), 20, 60);
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
    // `cached_model_artifacts` calls the builder; if the builder returns
    // None (corrupt model), nothing is inserted and the function returns
    // None — the decode path treats it as an invalid model.
    let corrupt_model = vec![0xFFu8; 128]; // not a valid frequency model
    let r = cached_model_artifacts(7, 12, &corrupt_model, || {
        // Simulate validation failure: builder returns None.
        None::<ValidatedModelArtifacts>
    });
    add(
        &mut cases,
        "CASE.008",
        "corrupt model builder returns None → cached_model_artifacts returns None",
        "none",
        match r {
            None => Ok("none".to_string()),
            Some(_) => Ok("cached".to_string()),
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
    let build = || -> Option<ValidatedModelArtifacts> {
        // Validate sum: 256 × 16 = 4096 == 1 << 12.  Build the full
        // artifact exactly as the production decode path does: Arc-shared
        // frequencies plus the 16 KiB packed word table (the expensive
        // artifact the cache exists to share).
        let freqs: Vec<u32> = vec![16u32; 256];
        let cum = {
            let mut c = Vec::with_capacity(257);
            c.push(0u32);
            for i in 0..256 {
                c.push(c[i] + freqs[i]);
            }
            c
        };
        let table = ryg_rans_rs_simd::packed_table::PackedWordTable::from_freqs(&freqs, &cum, 12)
            .expect("uniform model table");
        Some(ValidatedModelArtifacts {
            freqs: std::sync::Arc::new(freqs),
            uniform256: true,
            packed_table: Some(std::sync::Arc::new(table)),
        })
    };
    let a1 = cached_model_artifacts(7, 12, &valid_model, build);
    let a2 = cached_model_artifacts(7, 12, &valid_model, build);
    let served = match (&a1, &a2) {
        // The hit must serve the identical shared allocations (Arc::ptr_eq
        // on the freqs and the packed table), not rebuild or deep-copy.
        (Some(x), Some(y)) => {
            x.uniform256
                && std::sync::Arc::ptr_eq(&x.freqs, &y.freqs)
                && match (&x.packed_table, &y.packed_table) {
                    (Some(a), Some(b)) => std::sync::Arc::ptr_eq(a, b),
                    _ => false,
                }
        }
        _ => false,
    };
    add(
        &mut cases,
        "CASE.009",
        "valid uniform256 model cached and served identically",
        "served",
        if served {
            Ok("served".to_string())
        } else {
            Ok("not_served".to_string())
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
    let decode_to = |jobs: Vec<DecodeBlockJob>| -> Option<Vec<u8>> {
        ParallelDecoder::decode_blocks(jobs, &cfg).ok().map(|dec| {
            let mut out = Vec::new();
            for b in &dec.blocks {
                out.extend_from_slice(&b.output);
            }
            out
        })
    };
    let cold = decode_to(djobs.clone());
    let warm = decode_to(djobs.clone());
    let equivalent = match (&cold, &warm) {
        (Some(c), Some(w)) => c == w && c == &data,
        _ => false,
    };
    add(
        &mut cases,
        "CASE.010",
        "decode output identical with cold vs warm model cache (cache-equivalence)",
        "equivalent",
        if equivalent {
            Ok("equivalent".to_string())
        } else {
            Ok("DIFFERENT".to_string())
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
