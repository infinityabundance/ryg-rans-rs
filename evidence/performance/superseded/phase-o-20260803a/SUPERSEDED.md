# SUPERSEDED — run phase-o-20260803a

This run is **superseded** (retained in full, never deleted).

**Reason:** the benchmark's hot-set mode proof still asserted an exact
hit count (`hits == bc - 16`) that is scheduler-dependent under
Design-A accounting (MODEL_CACHE.METRICS.2): a hot-set block whose
lookup runs before its model is published is a coalesced MISS, not a
hit.  Four hot-set cases (8/16/32 workers on small blocks) were
therefore rejected at preflight and are missing from the
`RYG_RANS.PERF.CACHE.THRASH` receipt (104 executed of 108 declared).

The fix (`8e1b6fe`) makes the hot-set proof worker-aware: exact counts
at 1 worker, deterministic bounds at N workers.  The regenerated run
`phase-o-20260803b` carries the complete 108-case THRASH surface and
replaces this one as the active evidence.

Bindings of this run remain valid as far as they go (same implementation
commit, same host, same workload cache); they are simply incomplete for
the thrash/hot-set surface.  See the gap ledger Phase O section for the
residual note.
