//! # RYG_RANS.L.BACKEND.EXPLICIT — exact backend semantics (L.9)
//!
//! Proves the L.9 exact-backend contract:
//!
//! - An explicit backend request executes **exactly** that backend or
//!   returns a typed error — never a silent scalar substitution.
//! - Format compatibility is enforced at plan time:
//!   `8-way ↔ codec 7`, `16-way ↔ codec 8`, `Uniform256 ↔ validated
//!   Uniform256 model`, `RAW ↔ RAW kind`, `RLE ↔ RLE kind`.
//! - Batch backends require a coordinator batch context; the one-block
//!   plan API returns `BackendRequiresBatchContext`.
//! - `disable_simd` + explicit SIMD → `BackendUnavailable` (config conflict).
//! - On success, `plan_backend == backend` (no substitution).

use super::{CourtCase, CourtRun};
use ryg_rans_rs_casefile::PhaseLCaseVerdict;
use ryg_rans_rs_parallel::{
    BackendId, BackendPolicy, BlockErrorKind, CodecPolicy, EncodeBlockJob, FixedBlockPlan,
    ModelPolicy, ParallelConfig, ParallelDecoder, ParallelEncoder, ThreadCount, create_decode_plan,
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
            residual_ids: vec!["L9-A".to_string(), "L9-B".to_string(), "L9-C".to_string()],
        });
    };

    // Build a real 8-way (codec 7) encoded block and a 16-way (codec 8) one.
    let data = nonuniform_data(8 * 1024);
    let plan = FixedBlockPlan::new(data.len() as u64, 4096);
    let cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    // Codec 7 (interleaved8)
    let jobs7: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Explicit(7),
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc7 = match ParallelEncoder::encode_blocks(jobs7, &cfg) {
        Ok(e) => e,
        Err(e) => {
            return CourtRun {
                court_id: "RYG_RANS.L.BACKEND.EXPLICIT".to_string(),
                title: "Exact backend semantics (L.9)".to_string(),
                residual_ids: vec!["L9-A".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "encode codec-7 blocks".to_string(),
                    expected: "Ok".to_string(),
                    actual: format!("ERROR: {:?}", e),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L9-A".to_string()],
                }],
            };
        }
    };
    let djobs7: Vec<ryg_rans_rs_parallel::DecodeBlockJob> = enc7
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // Codec 8 (interleaved16)
    let jobs8: Vec<EncodeBlockJob> = plan
        .ranges
        .iter()
        .map(|r| {
            let s = r.input_offset as usize;
            EncodeBlockJob::new(
                r.block_index,
                data[s..s + r.length as usize].to_vec(),
                CodecPolicy::Explicit(8),
                ModelPolicy::PerBlock,
                12,
            )
        })
        .collect();
    let enc8 = match ParallelEncoder::encode_blocks(jobs8, &cfg) {
        Ok(e) => e,
        Err(e) => {
            return CourtRun {
                court_id: "RYG_RANS.L.BACKEND.EXPLICIT".to_string(),
                title: "Exact backend semantics (L.9)".to_string(),
                residual_ids: vec!["L9-A".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "encode codec-8 blocks".to_string(),
                    expected: "Ok".to_string(),
                    actual: format!("ERROR: {:?}", e),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L9-A".to_string()],
                }],
            };
        }
    };
    let djobs8: Vec<ryg_rans_rs_parallel::DecodeBlockJob> = enc8
        .blocks
        .iter()
        .map(|b| ryg_rans_rs_parallel::DecodeBlockJob {
            block_index: b.block_index,
            block_data: b.block.clone(),
        })
        .collect();

    // ---- Case 1: explicit SSE4.1 on codec 7 executes exactly --------------
    let cfg_sse = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        backend_policy: BackendPolicy::Explicit(BackendId::Sse41Interleaved8),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let r = ParallelDecoder::new(cfg_sse.clone()).decode_blocks(djobs7.clone());
    let sse_ok = match &r {
        Ok(dec) => dec
            .blocks
            .iter()
            .all(|b| b.backend == BackendId::Sse41Interleaved8 && b.plan_backend == b.backend),
        Err(e) => match e {
            ryg_rans_rs_parallel::ParallelError::DecodeFailed(inner)
                if inner.kind == BlockErrorKind::BackendUnavailable =>
            {
                // Host lacks SSE4.1 (unlikely on x86-64, but typed = honest)
                true
            }
            _ => false,
        },
    };
    add(
        &mut cases,
        "CASE.001",
        "Explicit(Sse41Interleaved8) on codec 7 executes exactly or typed error",
        "exact_or_unavailable",
        if sse_ok {
            Ok("exact_or_unavailable".to_string())
        } else {
            Ok(format!("{:?}", r.map(|d| d.blocks.len())))
        },
    );

    // ---- Case 2: explicit 8-way on codec 8 → format mismatch --------------
    // A codec-8 (16-way) block decoded with an explicit 8-way backend must
    // be rejected at plan time with BackendFormatMismatch — never silently
    // executed.
    let cfg_wrong = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        backend_policy: BackendPolicy::Explicit(BackendId::Sse41Interleaved8),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let r = ParallelDecoder::new(cfg_wrong.clone()).decode_blocks(djobs8.clone());
    let mismatch = match &r {
        Err(ryg_rans_rs_parallel::ParallelError::DecodeFailed(inner))
            if inner.kind == BlockErrorKind::BackendFormatMismatch =>
        {
            true
        }
        _ => false,
    };
    add(
        &mut cases,
        "CASE.002",
        "Explicit 8-way backend on a codec-8 (16-way) block → BackendFormatMismatch",
        "format_mismatch",
        if mismatch {
            Ok("format_mismatch".to_string())
        } else {
            Ok(format!("{:?}", r.map(|d| d.blocks.len())))
        },
    );

    // ---- Case 3: explicit 16-way on codec 7 → format mismatch -------------
    let cfg_wrong16 = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        backend_policy: BackendPolicy::Explicit(BackendId::Avx512Interleaved16),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let r = ParallelDecoder::new(cfg_wrong16.clone()).decode_blocks(djobs7.clone());
    let mismatch = match &r {
        Err(ryg_rans_rs_parallel::ParallelError::DecodeFailed(inner))
            if inner.kind == BlockErrorKind::BackendFormatMismatch =>
        {
            true
        }
        _ => false,
    };
    add(
        &mut cases,
        "CASE.003",
        "Explicit 16-way backend on a codec-7 (8-way) block → BackendFormatMismatch",
        "format_mismatch",
        if mismatch {
            Ok("format_mismatch".to_string())
        } else {
            Ok(format!("{:?}", r.map(|d| d.blocks.len())))
        },
    );

    // ---- Case 4: explicit scalar16 on codec 8 executes exactly ------------
    let cfg_s16 = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(1).unwrap()),
        backend_policy: BackendPolicy::Explicit(BackendId::Scalar16),
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let r = ParallelDecoder::new(cfg_s16.clone()).decode_blocks(djobs8.clone());
    let exact = match &r {
        Ok(dec) => dec
            .blocks
            .iter()
            .all(|b| b.backend == BackendId::Scalar16 && b.plan_backend == b.backend),
        Err(_) => false,
    };
    add(
        &mut cases,
        "CASE.004",
        "Explicit(Scalar16) on codec 8 executes exactly",
        "exact",
        if exact {
            Ok("exact".to_string())
        } else {
            Ok(format!("{:?}", r.map(|d| d.blocks.len())))
        },
    );

    // ---- Case 5: batch backend via one-block plan → requires batch context
    let r = create_decode_plan(
        7,
        12,
        &[0u8; 1024],
        BackendPolicy::Explicit(BackendId::Avx512Batch4),
        false,
        0,
        0,
    );
    add(
        &mut cases,
        "CASE.005",
        "create_decode_plan(Explicit(Avx512Batch4)) → BackendRequiresBatchContext",
        "batch_context",
        match r {
            Err(e) if e.kind == BlockErrorKind::BackendRequiresBatchContext => {
                Ok("batch_context".to_string())
            }
            other => Ok(format!("{:?}", other.map(|p| p.backend_id()))),
        },
    );

    // ---- Case 6: RawCopy on a non-RAW block → format mismatch -------------
    let r = create_decode_plan(
        7,
        12,
        &[0u8; 1024],
        BackendPolicy::Explicit(BackendId::RawCopy),
        false,
        0, // RANS kind, not RAW
        0,
    );
    add(
        &mut cases,
        "CASE.006",
        "Explicit(RawCopy) on a RANS block → BackendFormatMismatch",
        "format_mismatch",
        match r {
            Err(e) if e.kind == BlockErrorKind::BackendFormatMismatch => {
                Ok("format_mismatch".to_string())
            }
            other => Ok(format!("{:?}", other.map(|p| p.backend_id()))),
        },
    );

    // ---- Case 7: Uniform256 explicit on a non-uniform model → mismatch ----
    let non_uniform_model = {
        let mut v = Vec::with_capacity(1024);
        for i in 0..256u32 {
            v.extend_from_slice(&(if i == 0 { 17u32 } else { 15u32 }).to_le_bytes());
        }
        v
    };
    let r = create_decode_plan(
        8,
        12,
        &non_uniform_model,
        BackendPolicy::Explicit(BackendId::Uniform256TableFree16),
        false,
        0,
        0,
    );
    add(
        &mut cases,
        "CASE.007",
        "Explicit(Uniform256TableFree16) on a non-uniform model → BackendFormatMismatch",
        "format_mismatch",
        match r {
            Err(e) if e.kind == BlockErrorKind::BackendFormatMismatch => {
                Ok("format_mismatch".to_string())
            }
            other => Ok(format!("{:?}", other.map(|p| p.backend_id()))),
        },
    );

    // ---- Case 8: Uniform256 explicit on a valid uniform model → plan -------
    let uniform_model: Vec<u8> = {
        let mut v = Vec::with_capacity(1024);
        for _ in 0..256 {
            v.extend_from_slice(&16u32.to_le_bytes());
        }
        v
    };
    let r = create_decode_plan(
        8,
        12,
        &uniform_model,
        BackendPolicy::Explicit(BackendId::Uniform256TableFree16),
        false,
        0,
        0,
    );
    add(
        &mut cases,
        "CASE.008",
        "Explicit(Uniform256TableFree16) on a valid uniform256 model → plan",
        "planned",
        match r {
            Ok(_) => Ok("planned".to_string()),
            Err(e) => Err(format!("{:?}", e)),
        },
    );

    // ---- Case 9: disable_simd + explicit SIMD → BackendUnavailable --------
    let r = create_decode_plan(
        7,
        12,
        &[0u8; 1024],
        BackendPolicy::Explicit(BackendId::Sse41Interleaved8),
        true, // disable_simd
        0,
        0,
    );
    add(
        &mut cases,
        "CASE.009",
        "disable_simd=true + Explicit(SIMD) → BackendUnavailable (config conflict)",
        "unavailable",
        match r {
            Err(e) if e.kind == BlockErrorKind::BackendUnavailable => Ok("unavailable".to_string()),
            other => Ok(format!("{:?}", other.map(|p| p.backend_id()))),
        },
    );

    // ---- Case 10: ModelAware selects Uniform256 on a uniform model --------
    let r = create_decode_plan(
        8,
        12,
        &uniform_model,
        BackendPolicy::ModelAware,
        false,
        0,
        0,
    );
    add(
        &mut cases,
        "CASE.010",
        "ModelAware on uniform256 model selects the table-free kernel",
        "Uniform256TableFree16",
        match r {
            Ok(plan) if plan.backend_id() == BackendId::Uniform256TableFree16 => {
                Ok("Uniform256TableFree16".to_string())
            }
            Ok(plan) => Ok(format!("{:?}", plan.backend_id())),
            Err(e) => Err(format!("{:?}", e)),
        },
    );

    // ---- Case 11: Portable never selects SIMD ------------------------------
    let r = create_decode_plan(8, 12, &uniform_model, BackendPolicy::Portable, false, 0, 0);
    add(
        &mut cases,
        "CASE.011",
        "Portable policy selects scalar even for uniform256",
        "scalar",
        match r {
            Ok(plan) => {
                let id = plan.backend_id();
                if id == BackendId::Scalar16 || id == BackendId::Scalar8 {
                    Ok("scalar".to_string())
                } else {
                    Ok(format!("{:?}", id))
                }
            }
            Err(e) => Err(format!("{:?}", e)),
        },
    );

    CourtRun {
        court_id: "RYG_RANS.L.BACKEND.EXPLICIT".to_string(),
        title: "Exact backend semantics and format compatibility (L.9)".to_string(),
        cases,
        residual_ids: vec!["L9-A".to_string(), "L9-B".to_string(), "L9-C".to_string()],
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
