# Reading the Custodian Commentary (N.11)

> The repository's source carries commentary at five levels.  This file
> explains what each level says, why it exists, and how to use it.
> The philosophy is in `docs/philosophy.md`; the layering rules in
> `docs/layers.md`.

## The five levels

| Level | Where | What it says | How to use it |
|-------|-------|--------------|---------------|
| Module | the `//!` block at the top of each `src/*.rs` | Purpose, History, Design, Alternatives, Invariants, Failure modes, Performance, Verification, Receipts, Tests, Future evolution, References | Read before anything in the module; it is the module's memory |
| Algorithm | `docs/papers/*` + the module docs of the algorithm's home | The mathematics, the stream formats, why the technique exists | Read before the code; the code assumes it |
| Function | `///` rustdoc on public functions | Purpose, inputs, outputs, invariants, safety, performance, failure modes, receipts, tests | Read before calling or modifying the function |
| Section | `//` annotations inside dense functions | What a block does, why, the rejected alternative, interaction with neighbours, evidence | Read when tracing a specific step |
| Line | `//` annotations on individual operations | Why this instruction, why the ordering, what breaks if changed, which invariant/receipt pins it | Read when modifying the operation |

## How the levels compose

A claim appears at the level where it is load-bearing and is referenced,
never restated, at the others (the never-duplicate rule).  Example — the
reciprocal bias:

* **Algorithm layer** (`docs/papers/0001-rans-design.md` §4): why the
  reciprocal exists, the Alverson bound, the exact bias.
* **Module layer** (core `lib.rs`): the surface constants and the proof
  table.
* **Function layer** (the `put_symbol` docs): the special-cased
  `freq == 1` case and why the general path cannot handle it.
* **Line layer**: the bias expression annotated with what breaks if
  changed (the oracle receipts).

## What a good annotation answers

1. **Why does this exist?** (the alternative it displaces)
2. **What invariant does it preserve?** (the load-bearing property)
3. **What breaks if changed?** (the failure mode)
4. **Which receipt pins it?** (the evidence)
5. **Which test detects a regression?** (the tripwire)

If an annotation answers none of these, it is syntax commentary and does
not belong (philosophy §9).

## Historical failure commentary

Where a line exists because of a past defect, the annotation says so:
original implementation → audit finding → root cause → correction →
invariant introduced → evidence.  The `docs/failures/` encyclopedia is the
index; the annotations are the local pointers.

## Practice for readers

* **Before modifying a function**, read its rustdoc, then its module's
  `//!`, then trace any `# Safety` section if it is unsafe.
* **Before changing an invariant**, find the annotation that names the
  receipt and the test; both must be updated or the seal fails.
* **Before "simplifying"**, read the line annotations around the code;
  the maintainer notes in `docs/education.md` name the tempting
  simplifications that are traps.
