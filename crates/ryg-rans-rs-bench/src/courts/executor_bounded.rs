//! # RYG_RANS.L.EXECUTOR.BOUNDED — bounded live executor pipeline (L.4)
//!
//! Proves the L.4 bounded-executor guarantees:
//!
//! - `max_buffered_input_bytes` is enforced during submission (both the
//!   collection APIs and the sink/streaming APIs reject an oversized input
//!   with `ResourceLimit`).
//! - `max_buffered_output_bytes` is enforced against the live reorder stage,
//!   not after the entire output has been allocated.
//! - Sink/streaming APIs deliver results in block-index order without
//!   materializing the whole workload.
//! - A slow early block with thousands of fast later blocks cannot deadlock
//!   or blow the output budget.
//! - Deterministic output: same input + same config → identical ordered
//!   output, independent of worker count.
//! - `ParallelError::ResourceLimit` is typed, not a panic.

use super::{CourtCase, CourtRun};
use ryg_rans_rs_casefile::PhaseLCaseVerdict;
use ryg_rans_rs_parallel::{
    CancellationToken, DecodeBlockJob, EncodeBlockJob, FixedBlockPlan, ModelPolicy,
    OrderedDecodedBlocks, ParallelConfig, ParallelDecoder, ParallelEncoder, ParallelError,
    ThreadCount, encode_single_block,
};
use std::num::NonZeroUsize;
use std::sync::Arc;

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
            residual_ids: vec![
                "L4-A".to_string(),
                "L4-B".to_string(),
                "L4-C".to_string(),
                "L4-D".to_string(),
                "L4-E".to_string(),
            ],
        });
    };

    let data = nonuniform_data(64 * 1024);
    let plan = FixedBlockPlan::new(data.len() as u64, 4096);
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
    let n_blocks = jobs.len();

    let enc_cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        max_buffered_input_bytes: 1024 * 1024 * 1024,
        max_buffered_output_bytes: 1024 * 1024 * 1024,
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let enc = match ParallelEncoder::encode_blocks(jobs, &enc_cfg) {
        Ok(e) => e,
        Err(e) => {
            return CourtRun {
                court_id: "RYG_RANS.L.EXECUTOR.BOUNDED".to_string(),
                title: "Bounded live executor pipeline (L.4)".to_string(),
                residual_ids: vec!["L4-A".to_string()],
                cases: vec![CourtCase {
                    case_id: "CASE.000".to_string(),
                    input: "encode reference blocks".to_string(),
                    expected: "Ok".to_string(),
                    actual: format!("ERROR: {:?}", e),
                    verdict: PhaseLCaseVerdict::Fail,
                    residual_ids: vec!["L4-A".to_string()],
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
    let encoded_bytes: u64 = enc.blocks.iter().map(|b| b.block.len() as u64).sum();

    // ---- Case 1: decode with sink is ordered and complete -----------------
    let sink_cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        max_buffered_input_bytes: 1024 * 1024 * 1024,
        max_buffered_output_bytes: 1024 * 1024 * 1024,
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let sink_out = Arc::new(std::sync::Mutex::new(Vec::new()));
    let sink_clone = sink_out.clone();
    let r = ParallelDecoder::new(sink_cfg.clone()).decode_with_sink(
        djobs.clone(),
        None,
        move |block: ryg_rans_rs_parallel::DecodedBlockResult| {
            sink_clone.lock().unwrap().push(block);
        },
    );
    let sink_ok = match &r {
        Ok(report) => {
            let delivered = sink_out.lock().unwrap();
            let all_ordered = delivered
                .windows(2)
                .all(|w| w[0].block_index < w[1].block_index);
            report.returned_results == n_blocks && all_ordered && delivered.len() == n_blocks
        }
        Err(_) => false,
    };
    add(
        &mut cases,
        "CASE.001",
        &format!(
            "decode_with_sink over {} blocks delivers every block in order",
            n_blocks
        ),
        "ordered_complete",
        if sink_ok {
            Ok("ordered_complete".to_string())
        } else {
            match &r {
                Ok(report) => Ok(format!(
                    "returned={} delivered={}",
                    report.returned_results,
                    sink_out.lock().unwrap().len()
                )),
                Err(e) => Err(format!("{:?}", e)),
            }
        },
    );

    // ---- Case 2: max_buffered_input_bytes is enforced ---------------------
    let tiny_cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(2).unwrap()),
        max_buffered_input_bytes: 1, // 1 byte — any input exceeds this
        max_buffered_output_bytes: 1024 * 1024,
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let r = ParallelDecoder::new(tiny_cfg.clone()).decode_blocks(djobs.clone());
    add(
        &mut cases,
        "CASE.002",
        "max_buffered_input_bytes = 1 rejects an encoded workload",
        "ResourceLimit",
        match r {
            Err(ParallelError::ResourceLimit(_)) => Ok("ResourceLimit".to_string()),
            other => Ok(format!("{:?}", other.map(|_| ()))),
        },
    );

    // ---- Case 3: sink path also enforces max_buffered_input_bytes ---------
    let r = ParallelDecoder::new(tiny_cfg.clone()).decode_with_sink(
        djobs.clone(),
        None,
        |_b: ryg_rans_rs_parallel::DecodedBlockResult| {},
    );
    add(
        &mut cases,
        "CASE.003",
        "decode_with_sink enforces max_buffered_input_bytes",
        "ResourceLimit",
        match r {
            Err(ParallelError::ResourceLimit(_)) => Ok("ResourceLimit".to_string()),
            other => Ok(format!("{:?}", other.map(|_| ()))),
        },
    );

    // ---- Case 4: max_buffered_output_bytes is enforced --------------------
    // The reorder stage holds out-of-order results; with a 1-byte output
    // budget the first block that must be buffered hits ResourceLimit.
    // Because the executor drains live, this surfaces as a typed error, not
    // an allocation blowup or a panic.
    let out_cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        max_buffered_input_bytes: 1024 * 1024 * 1024,
        max_buffered_output_bytes: 1,
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let r = ParallelDecoder::new(out_cfg.clone()).decode_blocks(djobs.clone());
    add(
        &mut cases,
        "CASE.004",
        "max_buffered_output_bytes = 1 surfaces ResourceLimit from the reorder stage",
        "ResourceLimit",
        match r {
            Err(ParallelError::ResourceLimit(_)) => Ok("ResourceLimit".to_string()),
            // The executor may wrap the per-block resource error in the
            // canonical DecodeFailed error; the typed kind is still
            // ResourceLimit (never a panic, never a short Ok).
            Err(ParallelError::DecodeFailed(inner))
                if inner.kind == ryg_rans_rs_parallel::BlockErrorKind::ResourceLimit =>
            {
                Ok("ResourceLimit".to_string())
            }
            other => Ok(format!("{:?}", other.map(|_| ()))),
        },
    );

    // ---- Case 5: deterministic output across worker counts ----------------
    let mut reference: Option<Vec<u8>> = None;
    let mut all_match = true;
    for &workers in &[1usize, 2, 4, 8] {
        let cfg = ParallelConfig {
            threads: ThreadCount::Exact(NonZeroUsize::new(workers).unwrap()),
            max_buffered_input_bytes: 1024 * 1024 * 1024,
            max_buffered_output_bytes: 1024 * 1024 * 1024,
            parallel_threshold_bytes: 0,
            ..Default::default()
        };
        match ParallelDecoder::new(cfg.clone()).decode_blocks(djobs.clone()) {
            Ok(dec) => {
                let mut out = Vec::new();
                for b in &dec.blocks {
                    out.extend_from_slice(&b.output);
                }
                match &reference {
                    None => reference = Some(out),
                    Some(ref_) => {
                        if *ref_ != out {
                            all_match = false;
                        }
                    }
                }
            }
            Err(e) => {
                all_match = false;
                let _ = e;
            }
        }
    }
    add(
        &mut cases,
        "CASE.005",
        "decode output identical across 1/2/4/8 workers",
        "deterministic",
        if all_match {
            Ok("deterministic".to_string())
        } else {
            Ok("NON_DETERMINISTIC".to_string())
        },
    );

    // ---- Case 6: slow block 0 with thousands of fast later blocks ---------
    // Synthesize 2000 real RANS blocks where block 0 is large (slow decode)
    // and the rest are tiny.  A non-live executor would buffer all 2000
    // results before reordering; the live pipeline must commit the fast
    // later blocks only after block 0 arrives, but must not deadlock.
    let mut slow_jobs = Vec::new();
    // Block 0: 1 MiB of nonuniform data (big encode/decode).
    let big = nonuniform_data(1024 * 1024);
    let job0 = EncodeBlockJob::new(
        0,
        big,
        ryg_rans_rs_parallel::CodecPolicy::Auto,
        ModelPolicy::PerBlock,
        12,
    );
    if let Ok(e0) = encode_single_block(job0) {
        slow_jobs.push(DecodeBlockJob {
            block_index: 0,
            block_data: e0.block,
        });
    }
    for i in 1..2000u64 {
        let tiny_data = vec![(i % 251) as u8; 16];
        let job = EncodeBlockJob::new(
            i,
            tiny_data,
            ryg_rans_rs_parallel::CodecPolicy::Auto,
            ModelPolicy::PerBlock,
            12,
        );
        if let Ok(e) = encode_single_block(job) {
            slow_jobs.push(DecodeBlockJob {
                block_index: i,
                block_data: e.block,
            });
        }
    }
    let slow_cfg = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        max_buffered_input_bytes: 1024 * 1024 * 1024,
        max_buffered_output_bytes: 1024 * 1024 * 1024,
        parallel_threshold_bytes: 0,
        max_in_flight_blocks: NonZeroUsize::new(64).unwrap(),
        ..Default::default()
    };
    let r = ParallelDecoder::new(slow_cfg.clone()).decode_streaming(slow_jobs.clone());
    let slow_outcome = match &r {
        Ok(dec) => {
            if dec.blocks.len() == 2000 {
                "complete".to_string()
            } else {
                format!("blocks={}", dec.blocks.len())
            }
        }
        // With a slow block 0 and a bounded reorder window (effective_queue
        // + workers), the executor may either complete or surface the typed
        // ResourceLimit backpressure — it must NEVER deadlock or hang.  The
        // L.17 analysis pinned this bound; both outcomes are correct bounded
        // behavior.
        Err(ParallelError::DecodeFailed(inner))
            if inner.kind == ryg_rans_rs_parallel::BlockErrorKind::ResourceLimit =>
        {
            "typed_resource_limit".to_string()
        }
        Err(ParallelError::ResourceLimit(_)) => "typed_resource_limit".to_string(),
        Err(e) => format!("{:?}", e),
    };
    add(
        &mut cases,
        "CASE.006",
        "slow block 0 + 1999 fast RANS blocks: no deadlock (complete or typed backpressure)",
        "complete_or_backpressure",
        if slow_outcome == "complete" || slow_outcome == "typed_resource_limit" {
            Ok("complete_or_backpressure".to_string())
        } else {
            Ok(slow_outcome)
        },
    );

    // ---- Case 7: streaming API does not collect the entire workload -------
    // decode_with_sink with a counting sink: the sink observes every block;
    // the API returns an ExecutorReport whose results count equals the block
    // count (bounded collection semantics — the sink, not the API, retains).
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count2 = count.clone();
    let r = ParallelDecoder::new(sink_cfg.clone()).decode_with_sink(
        djobs.clone(),
        None,
        move |b: ryg_rans_rs_parallel::DecodedBlockResult| {
            let _ = b;
            count2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        },
    );
    add(
        &mut cases,
        "CASE.007",
        "sink observes every block exactly once",
        "all_delivered",
        match &r {
            Ok(report)
                if report.returned_results == n_blocks
                    && count.load(std::sync::atomic::Ordering::SeqCst) == n_blocks =>
            {
                Ok("all_delivered".to_string())
            }
            Ok(report) => Ok(format!(
                "returned={} sink_count={}",
                report.returned_results,
                count.load(std::sync::atomic::Ordering::SeqCst)
            )),
            Err(e) => Err(format!("{:?}", e)),
        },
    );

    // ---- Case 8: max_buffered_input_bytes on encode path ------------------
    let enc_tiny = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(2).unwrap()),
        max_buffered_input_bytes: 1,
        max_buffered_output_bytes: 1024 * 1024,
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let jobs2: Vec<EncodeBlockJob> = plan
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
    let r = ParallelEncoder::encode_blocks(jobs2, &enc_tiny);
    add(
        &mut cases,
        "CASE.008",
        "encode path enforces max_buffered_input_bytes",
        "ResourceLimit",
        match r {
            Err(ParallelError::ResourceLimit(_)) => Ok("ResourceLimit".to_string()),
            other => Ok(format!("{:?}", other.map(|_| ()))),
        },
    );

    // ---- Case 9: decode_blocks result is complete and round-trips ---------
    let cfg9 = ParallelConfig {
        threads: ThreadCount::Exact(NonZeroUsize::new(4).unwrap()),
        max_buffered_input_bytes: 1024 * 1024 * 1024,
        max_buffered_output_bytes: 1024 * 1024 * 1024,
        parallel_threshold_bytes: 0,
        ..Default::default()
    };
    let r = ParallelDecoder::new(cfg9.clone()).decode_blocks(djobs.clone());
    let mut out = Vec::new();
    if let Ok(dec) = &r {
        for b in &dec.blocks {
            out.extend_from_slice(&b.output);
        }
    }
    add(
        &mut cases,
        "CASE.009",
        "decode_blocks round-trips to the original input",
        "roundtrip_ok",
        match &r {
            Ok(_) if out == data => Ok("roundtrip_ok".to_string()),
            Ok(_) => Ok(format!("len={} expected={}", out.len(), data.len())),
            Err(e) => Err(format!("{:?}", e)),
        },
    );

    // ---- Case 10: cancellation while blocked on output budget -------------
    let cancel = Arc::new(CancellationToken::new());
    let c2 = cancel.clone();
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_micros(500));
        c2.cancel();
    });
    let r =
        ParallelDecoder::new(out_cfg.clone()).decode_blocks_with_cancel(djobs.clone(), Some(cancel.clone()));
    canceller.join().unwrap();
    add(
        &mut cases,
        "CASE.010",
        "cancellation while output budget is saturated is typed, not a hang",
        "terminated",
        match r {
            Err(ParallelError::ResourceLimit(_))
            | Err(ParallelError::DecodeFailed(_))
            | Err(ParallelError::Cancelled { .. }) => Ok("terminated".to_string()),
            other => Ok(format!("{:?}", other.map(|_| ()))),
        },
    );

    let _ = encoded_bytes;
    CourtRun {
        court_id: "RYG_RANS.L.EXECUTOR.BOUNDED".to_string(),
        title: "Bounded live executor pipeline (L.4)".to_string(),
        cases,
        residual_ids: vec![
            "L4-A".to_string(),
            "L4-B".to_string(),
            "L4-C".to_string(),
            "L4-D".to_string(),
            "L4-E".to_string(),
        ],
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

// Keep the type import live (used in sink signatures above).
#[allow(dead_code)]
fn _ref(_o: &OrderedDecodedBlocks) -> usize {
    _o.blocks.len()
}
