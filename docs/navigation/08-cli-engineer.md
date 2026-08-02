# 08 — CLI Engineer

**Purpose:** understand and extend the `ryg-rans` command: container
format, codec dispatch, integrity, exit codes, cancellation.

**Prerequisites:** `01-first-week.md`.

**Required papers:** 0001 §5 (renormalization), 0006 (integrity).

**Required ADRs:** 0006.

**Required source modules:** `crates/ryg-rans-rs-cli/src/` (`lib.rs`,
`ops/*.rs`, `container/*.rs`, `signal.rs`, `error.rs`, `exit.rs`,
`limits.rs`).

**Recommended reading order:**
1. `docs/container-format-v1.md` — the container spec.
2. The CLI README — commands, exit codes, cancellation.
3. The source modules above, in dependency order.
4. `docs/education.md` — the CLI maintainer notes.
5. The CLI integration tests (`tests/cli.rs`).

**Expected understanding:** the block-streaming container; the single
codec dispatcher (one truth for decode/inspect/verify); the strict
integrity policy; the stable exit codes 0–11; cooperative cancellation
(SIGINT/SIGTERM/`--timeout` at block boundaries).

**Estimated reading time:** 6–10 hours.

**Exercises:**
1. Explain why the dispatcher lives in one place (`ops::decode_block`).
2. Trace exit code 11 from signal to `ExitCode`.
3. Explain the `--no-default-features` unsafe posture (signals feature).

**Common misconceptions:**
- "Exit codes can be collapsed." Automation depends on the documented
  0–11 semantics.
- "The CLI uses the parallel engine." It does not; it is single-threaded
  streaming by design (documented in its README).

**Related evidence:** the CLI integration tests; the
`RYG_RANS.L.VERIFY.DECODED_HASH` court's CLI exit-code cases.

**Future reading:** `07-evidence-engineer.md`.
