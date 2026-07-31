# Glossary — exact project terminology

Every document in this repository uses these terms with exactly these
meanings.  If a document uses a term differently, that document is wrong.

## Data model

* **Block** — one independently decodable unit of a container or parallel
  job: a header, optional model data, and a payload.  A block decodes to a
  fixed number of bytes (`uncompressed_length`).
* **Stream** — the canonical byte/word sequence produced by one codec for
  one block's payload.  "8-way stream" means the interleaved 8-state format
  produced by the 8-way encoder, not a byte stream of width 8.
* **Surface** — a named, evidence-tracked capability row in the README
  Evidence Status table (e.g. "32-bit byte rANS — division + reciprocal").
  A surface aggregates many behavioural receipts and one performance
  receipt.

## Codec and backend

* **Codec** — the stream format: number of states, renormalization unit,
  scale constraint.  Identified by a stable `codec_id` (see
  `container::codec`).  A codec does *not* pick the arithmetic
  implementation.
* **Backend** — the execution engine that decodes a stream: scalar table
  lookup, SSE4.1 SIMD, AVX2, AVX-512, batch.  One codec can be decoded by
  several backends.
* **Plan** — the validated decision about how to decode one block: which
  backend, which tables, under which execution conditions.  A plan is only
  valid for the exact codec, scale, model bytes, and capability context it
  was built for.
* **Requested backend** — the backend the caller asked for (explicitly or
  via policy).  Recorded in every execution result.
* **Executed backend** — the backend that actually ran.  With the exact
  backend contract, requested == executed or the call returns a typed
  error; silent substitution is prohibited.
* **Uniform256** — the specialized table-free Uniform256 decode path for
  models where every symbol's frequency is exactly `2^(scale_bits-8)`.
* **Batch4** — the 4-stream batched AVX2 decode backend.  It requires a
  coordinator-level batch context and is therefore not reachable through
  the one-block API (which returns a typed error).

## Evidence

* **Receipt** — a SHA-256-chained JSON artifact proving one claim: a
  behavioural receipt proves one observed behaviour; a performance receipt
  proves one measured surface.  A receipt carries a canonical self-hash.
* **Manifest** — the machine-readable inventory of a receipt's inputs:
  benchmark cases (performance) or court cases (behavioural), with hashes
  of every referenced artifact.
* **Residual** — a tracked, classified, unresolved-or-accepted discrepancy
  between observed and expected behaviour.  Residuals are never deleted;
  they are resolved or explicitly accepted with justification.
* **Seal** — the state of a claim after the complete seal gate passes:
  every receipt file exists, every hash verifies, every self-hash
  recomputes, every referenced artifact exists, and the working tree was
  clean at evidence time.
* **Behaviour seal** — seal of a behavioural surface (144 receipts at the
  Phase K baseline; the Phase L courts extend this total — the number is
  generated from the evidence index, never hardcoded).
* **Performance seal** — seal of a measured performance surface (10
  receipts).  "Sealed" means the measurement and its provenance are sealed,
  not that the implementation is fastest.
* **Preflight** — the structured verification record emitted by a
  benchmark case before timing: backend requested/executed, input/output
  hashes, words consumed, final states, thread counts.  Criterion timing is
  joined to preflight by exact benchmark ID.
* **Canonical output** — the byte-exact expected output of an operation
  under the pinned upstream format, asserted by parity courts.
* **Canonical error** — the deterministic error selected when multiple
  blocks fail: the lowest failing block index.  Independent of completion
  order and thread count.
* **Strict integrity** — the default verification policy: payload hash
  must match, decode must succeed, the stored decoded hash must be nonzero
  and match.  Zero/unset decoded hash fails with `DecodedHashMissing`;
  mismatch fails with `DecodedHashMismatch`.
* **Compatibility integrity** — the explicit opt-in policy
  (`AllowLegacyUnsetDecodedHash`): zero/unset decoded hashes are reported
  as `Unset` and do not fail solely for that reason; any nonzero mismatch
  still fails.

## Concurrency

* **Worker** — one OS thread in the executor's pool, with one exclusive
  `WorkerScratch` context.
* **Task** — one block's unit of work submitted to the executor.
* **In-flight** — submitted to a worker but not yet completed.
* **Reorder buffering** — holding completed-but-out-of-order results until
  their index becomes the next expected index.
* **Committed output** — results handed to the caller in strictly
  ascending block order by the coordinator.
* **Cancellation** — cooperative: a `CancellationToken` checked between
  units of work.  Cancellation returns `ParallelError::Cancelled {
  completed, expected }` and can never return `Ok` with fewer blocks than
  declared.

## Configuration

* **Worker count** — `requested_workers` (what the caller asked for) vs
  `effective_workers` (what actually ran).  `ExecutionMode::SequentialThresholdFallback`
  reports `effective_workers = 1` without spawning a pool.
* **In-flight bound** — `max_in_flight_blocks`: the job-queue capacity.
* **Input budget** — `max_buffered_input_bytes`: the bound on compressed
  input queued plus executing, enforced during submission.
* **Output budget** — `max_buffered_output_bytes`: the bound on
  completed-but-unordered output, enforced against the live reorder stage.
