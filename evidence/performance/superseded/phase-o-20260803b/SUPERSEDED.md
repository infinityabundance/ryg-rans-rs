# SUPERSEDED — run phase-o-20260803b

This run is **superseded** (retained in full, never deleted).

**Reason:** the thrash mode proof's N-worker bound asserted `lookups ==
bc`, but a coalesced waiter whose key is FIFO-evicted between its
registration and its wake-up retries legitimately adds one lookup and
one rebuild (the documented evict-then-rebuild semantic).  The flake
rejected `model_cache/e2e/thrash/32-workers/262144-bytes/thrash`
(lookups=33, builds=18, evictions=2 for bc=32 — all correct cache
behavior), so the THRASH receipt carried 107 of 108 cases.

The fix (`83bf6af`) relaxes the N-worker bound to `lookups >= bc`; the
1-worker exact proof is unchanged.  The regenerated run
`phase-o-20260803c` carries the complete 108-case THRASH surface and
replaces this one as the active evidence.
