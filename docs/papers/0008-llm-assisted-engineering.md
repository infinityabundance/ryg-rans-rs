# Paper 0008 — LLM-assisted systems engineering: the methodology this repository was built with

> *Layer: Subsystem.  Companion: `docs/llm/` (the operational checklists and
> prompt records).  This paper is the reference methodology for how this
> repository was constructed with machine assistance — and why its claims
> never rest on the assistant's word.*

## 1. What the assistant did

An LLM (a "coding agent") did the large majority of the mechanical
engineering work in this repository: writing the initial Rust
implementations, restructuring modules, drafting documentation, running
the build/test/benchmark commands, and iterating on compiler and test
failures.  It worked at the level of a very fast, very thorough junior-to-
mid-level systems engineer with a strong theoretical background and no
short-term memory of its own mistakes.

That sentence is the entire premise of this paper: the assistant is fast,
broad, and unreliable in specific, known ways.  The repository's
engineering process is the discipline that turned those properties into an
asset instead of a liability.

## 2. What the assistant did NOT do

The assistant did **not**:

* decide the bitstream contract — the pinned upstream `ryg_rans` defines
  it, and the oracle court enforces it;
* decide the evidence doctrine — "prose is not proof" is a human
  requirement, applied by the seal gate;
* certify correctness — every claim is traced to a test, a court, a
  receipt, and the seal;
* own the release — version decisions, tag placement, and publication
  order are human decisions recorded as ADRs;
* remember its own past failures — the repository's history, ADRs, and
  gap ledger are the memory the assistant does not have.

The boundary is the single most important fact in this paper: **the
assistant proposes; the process disposes.**  Any claim the assistant makes
about the code is treated as an unverified hypothesis until the machinery
accepts it.

## 3. How the assistant was wrong (and how the process caught it)

Concrete failures from this project, each of which shaped the process:

1. **"Fixed" but inert code.**  The ModelCache was reported resolved while
   the actual cache stored only a trivial artifact — the expensive packed
   table was still rebuilt per block.  The fix "worked" (tests passed)
   but produced no throughput gain.  Caught by a human audit that asked
   *"what does this actually buy?"* rather than *"does it compile?"*.
   Lesson: a wiring claim needs an observable effect, not just a call site.

2. **Doc comments promising what the code did not enforce.**  The
   cancellation APIs documented "returns Cancelled; never Ok with fewer
   blocks" while the final return paths returned `Ok` regardless,
   delegating the guarantee to the executor's internals.  A human traced
   the two functions to their returns and found the promise unenforced.
   Lesson: an agent writes the documentation it believes; the reviewer
   checks the documentation against the code path, instruction by
   instruction.

3. **Fabricated evidence semantics.**  The Phase K exporter reported
   `sample_count = 1`, hardcoded `verification_passed = true`, and zero
   throughput for 798 of 831 records — structurally present, semantically
   empty.  The machinery (receipts, index hashes) was there; the truth was
   not.  Caught by an audit of the *values*, not the files.  Lesson: file
   presence is not evidence; value provenance is.

4. **Evidence-destroying tooling.**  An evidence-promotion script renamed
   the entire `evidence/` tree and deleted the backup, destroying
   unrelated evidence (a full-precision benchmark run).  The script was
   correct for its narrow purpose and catastrophic for the repository.
   Lesson: agents optimize locally; the review must ask what else the
   operation touches.

5. **The seal printing success after skipping.**  The behavioural
   self-hash verification was skipped and reported as verified.  Lesson:
   an agent's summary of what it did is not a record of what it did; the
   tool must either verify or say it did not.

6. **Tautological binding.**  The exporter assigned a commit hash and then
   "verified" the assignment against the same value — a check that
   provably cannot fail.  Lesson: a check whose inputs are its own outputs
   proves nothing; binding must come from independent capture.

7. **The literal-type-name reachability audit (Phase O).**  An audit
   concluded "ModelCache has no production path" because it searched for
   the literal type name and never traced the `cached_model_artifacts`
   wrapper that decode uses.  The finding was rejected as factually
   incorrect (recorded in the gap ledger as an audit-method lesson): the
   cache was constructed, looked up, missed, inserted, and its `Arc`
   artifact consumed.  Lesson: reachability is traced through wrappers and
   downstream artifact consumption, not by grepping for a type name.

8. **The audit finding that was actually right (Phase O).**  The same
   audit's second claim — the cancellation doc comments promising what the
   code did not enforce — was verified by tracing the functions to their
   final returns, and the same failure class then recurred in *new*
   documentation written while fixing it.  Lesson: the doc-comment-vs-code
   gap is a recurring class, not a one-off; every guarantee sentence needs
   a trace-to-return check, including guarantees written during the fix.

9. **The feature-composition failure (post-v0.5.0 audit).**  Two features
   were individually proven — cancellation (a waiter can stop waiting) and
   single-flight (a same-key cold burst performs one build) — and their
   composition was wrong: a cancelled last waiter deleted the RUNNING
   builder's in-flight marker, so a caller arriving between cancellation
   and publication became a second builder and duplicated the expensive
   construction.  The cancellation test alone (one builder + one
   cancelling waiter, no third arrival) never exercised the window; the
   fix (builder-marker ownership) added a three-party court and a loom
   court over the exact interleaving.  Lesson: a test can prove feature A
   and another test can prove feature B while A∘B is still broken;
   adversarial review must compose the features it reviews, and the fix
   for a composition defect needs a composition test.

10. **Requiring a corpus to exist is not deriving the workload from its
    bytes (post-v0.5.0 audit).**  The stress/soak commands required the
    fetched source tree, read the derived manifest, and then encoded
    synthetic xorshift payloads from constant seeds — the manifest and
    corpus were presence gates, not inputs, yet the completion claims
    described "public-corpus stress".  The fix separated the families
    (`synthetic-cache-stress` labeled honestly; `stress-public` resolving
    every block's `source_sha256`/`offset`/`length` to verified bytes) and
    added the identity-honesty rule to the workloads README.  Lesson:
    provenance is a property of the data that actually flows into the
    measured region; names, directories, and manifests are not inputs.

The pattern across all ten: **the agent produced plausible structure with
missing truth.**  Plausible structure is the failure mode to defend
against, because it passes every local check while failing the global
one.

## 4. How the assistant accelerated development (and why that matters)

With the failures named, the acceleration is real and large:

* The initial reconstruction of four codec surfaces from the upstream C
  was produced and compiled in a fraction of the time a human would take
  to type it, freeing human attention for the contract and the evidence.
* The mechanical parts of the evidence pipeline (exporters, hashing,
  manifests, receipts, seal gates) are large, repetitive, error-prone
  code — exactly where an agent's tirelessness is an asset *when the
  truth-checks are machine-enforced*.
* Adversarial review loops ("hunt for bugs", "trace this guarantee") are
  cheap to run, so the audit surface is much wider than a human-only
  process would cover.  The agent's own review passes found real defects
  (the decoded-hash aggregate bug, the reorder-bound bug, the missed
  wakeup).
* The documentation volume (this paper included) is only feasible with
  machine drafting; the human role is to make it truthful.

The formula: **agents for throughput, humans for truth, machines for
verification.**  The agent generates the volume; the human sets the
invariants; the tooling (tests, courts, receipts, seal) arbitrates.

## 5. Prompt evolution: what changed over the project

* **Early prompts** asked for implementations and trusted the agent's
  self-report.  This produced the plausible-structure failures above.
* **Middle prompts** demanded tests with every change, then courts, then
  receipts.  The unit-test floor caught mechanical errors; it did not
  catch inert wiring.
* **Mature prompts** demand the *observable effect*: a documented feature
  is not implemented until its execution path, its effect, its test, and
  its receipt exist and join.  "No silent fallback", "no hardcoded
  verdicts", and "no deleted history" are all prompt-level rules that
  became machine-enforced gates.

The evolution is recorded in `docs/llm/` (prompt philosophy, review
checklists, common hallucination patterns).

## 6. Review methodology

Every change is reviewed against a fixed checklist, regardless of who
wrote it:

1. Find the claim.  (What does the doc/comment/commit message say?)
2. Find the producing code path.  (Trace it yourself; do not accept the
   author's summary.)
3. Find the test/court that pins it.  (Not a test that *mentions* it — one
   that *fails* if the behaviour changes.)
4. Find the receipt in `evidence/`.  (The artifact, with a verifying
   hash.)
5. Run the seal gate.  (The authoritative final gate; a skipped check
   fails.)

If any link is missing, the claim is unsealed and must not say "Sealed".
This is `AGENTS.md`'s verification procedure, and it is the review
methodology because it is the only one that defeats plausible structure.

## 7. Evidence-first development

The ordering matters: **the evidence requirement is stated before the
implementation, not after.**  A residual is written with its test
requirement and evidence requirement before the fix exists.  This turns
the agent's tendency to "finish" into a forcing function: the work is done
when the receipt exists and the seal passes, not when the compiler is
quiet.  The gap ledger (`evidence/phase-l/gap-ledger.md`) is the working
example of evidence-first: every entry names its reproduction, expected
behaviour, actual behaviour, fix, test, and evidence requirement.

## 8. When humans must intervene

The project's rule of thumb, learned the hard way:

* **Humans decide the contract.**  Anything that changes bytes on disk or
  the public API is a human decision (ADR-worthy).  The agent may propose;
  it may not finalise.
* **Humans audit the truth of summaries.**  An agent's description of what
  a function does is a hypothesis.  When the summary and the code disagree
  (documented repeatedly in this project), the code wins and the summary
  is a defect to fix — in either direction.
* **Humans own irreversibility.**  Publication, tag creation, evidence
  deletion (forbidden), and evidence supersession are human-gated.
* **Humans review the reviews.**  The agent's own audit passes are useful
  and were used, but a claim that "an agent found no bugs" is not a
  claim that there are none; it is a search result with a defined (and
  sometimes undocumented) coverage.

## 9. Lessons learned — the summary

1. Plausible structure with missing truth is the failure mode.  Check
   values, not files.
2. A wiring claim needs an observable effect.  "The cache is consulted"
   is not "the cache saves work".
3. A doc comment is a claim.  Trace it to the code, or it will lie.
4. A check that cannot fail is not a check.  Bind from independent
   sources.
5. Never print "verified" after skipping.  The tool must be honest even
   when the author is not.
6. History is evidence.  Supersede, never delete; record why.
7. Agents for throughput, humans for truth, machines for verification.
8. Evidence-first: state the receipt before the fix, and the fix is done
   when the seal passes.
9. The agent has no memory; the repository's history, ADRs, and ledger
   are the memory, and they must be written down as if the next engineer
   has never met you.
10. The final standard is not that the code looks serious; it is that
    every serious claim can be traced through real execution to a
    reproducible, adversarially verified, cryptographically bound
    artifact.
