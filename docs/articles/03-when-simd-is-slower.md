# When SIMD Is Slower Than Scalar — and Why

*An engineering article.  Measured observations on Zen 5 (AMD Ryzen 7
9800X3D) for the rANS word-codec decode kernels: where the SIMD hierarchy
holds, where it inverts, and why gather choice is microarchitecture-
dependent.  Every claim below is traced to the sealed performance run
`evidence/performance/runs/phase-l-20260802e`.*

## Abstract

SIMD decode of interleaved rANS is usually faster than scalar — but not
always, and not for the reasons a naive analysis assumes.  This article
reports measured data for eight decode kernels, isolates the components
(hashing, allocation, scheduling) that dominate real pipelines, and
explains the microarchitectural mechanisms behind the observations.

## 1. The premise: interleaving enables SIMD

K interleaved rANS states are independent by construction: output byte `i`
reads lane `i mod K`, and no cross-lane dependency exists.  A vector of K
states advances with one mask, one gather, and K parallel multiplies.
The 16 KiB packed table (4096 slots × u32, each slot packing freq/bias/sym)
is L1-resident, so each step's gather hits cache.

## 2. The measured hierarchy

The sealed run shows the expected ordering on Zen 5 for throughput:
scalar < SSE4.1 < AVX2 < AVX-512, with the Uniform256 table-free path
competitive with the widest kernels because it removes the gather
entirely (pure arithmetic: `sym = slot >> 4`).

## 3. Why gather choice matters

Hardware gathers (`vpgatherdd`) have historically had high latency and
unpredictable throughput on some microarchitectures; manual gathers (a few
sequential loads assembled into a vector) have predictable throughput.
On Zen 5 the two are within a few percent for this workload — but the
comparative data is host-specific, which is exactly why both variants
exist and the planner/benchmark chooses.  Claiming "hardware gather is
faster" without host data is speculation; the repository records the host
in every receipt.

## 4. The counterintuitive part: when SIMD is not the bottleneck

In the parallel engine, the per-block work includes *two* mandatory
SHA-256 hashes (payload + decoded output) for integrity.  The measured
single-worker overhead over the raw kernel is ~1.4×, and almost all of it
is hashing, not scheduling or the kernels.  A "SIMD speedup" measured at
the kernel level does not translate 1:1 to a pipeline with integrity work:
the pipeline's wall time is bounded by the sum of decode + hashing, and
hashing is not vectorised in this design.

## 5. Component isolation (how we know)

The methodology separates decode-only, payload-SHA-256-only,
decoded-SHA-256-only, both-hashes, model reconstruction, plan
construction, cache lookup, queue scheduling, reorder commit, allocation,
and scratch reuse.  Attribution rule: never claim a scaling loss is
"memory bandwidth" or "cache" or "SMT" without a component measurement or
hardware-counter evidence for that specific claim.  (Hardware counters
are unavailable on this host — recorded as accepted limitation L17-C.)

## 6. Memory hierarchy and cache behaviour

The packed table is 16 KiB, 64-byte aligned, L1-resident.  Skewed models
cluster hot slots in cache; the K independent state registers give K
independent gather addresses, exposing memory-level parallelism.  The
table is built once per model (the model cache) so the hot path is stream
processing only.

## 7. Branch prediction and renorm

Renormalization is data-dependent (a state drops below `L` roughly every
`log_M(L)` symbols) but branch-mispredictable only when the branch
matters; the kernels use masks where possible.  The tail loops (0..K-1
remaining bytes) are short and covered by the differential tests at every
tail length.

## 8. Honesty rules

1. A benchmark measures this kernel, this host, this build
   (`-C target-cpu=native`); portable builds run the scalar reference.
2. No kernel claims superiority over an implementation with different
   integrity work.
3. A zero-throughput value is valid only for a latency-unit microbenchmark
   whose schema says so.

## 9. Conclusion

The SIMD hierarchy is real but conditional: the kernels' gains are real on
Zen 5, the gather choice is a wash on this host, and the pipeline's
bottleneck is integrity hashing.  The lesson for implementers: measure
components, record the host, and never infer a speedup from a kernel
benchmark alone.

## References

`docs/papers/0003-simd.md`, `docs/papers/0005-performance-methodology.md`,
`docs/performance/phase-l17-analysis.md`, `evidence/performance/runs/phase-l-20260802e/`.
