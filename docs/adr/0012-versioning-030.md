# ADR-0012 — Versioning: 0.3.0, a pre-1.0 minor per semver-checks

Status: Accepted

## Context
The v0.2.0 release was tagged and published.  Phase L.3 added a new
`AppError::Cancelled` variant to the CLI's public error enum — a breaking
change for exhaustive matches.  The L.22 decision process required running
`cargo semver-checks` and `cargo public-api` rather than blindly bumping.

## Problem
What is the correct next release version?

## Alternatives considered
1. Patch bump (0.2.1).
2. Pre-1.0 minor (0.3.0).
3. Keep 0.2.0 and re-tag.
4. A compatibility reset (1.0.0).

## Rejected alternatives
- (1) was rejected: semver-checks found a genuine breaking API change; a
  patch would misrepresent compatibility.
- (3) was rejected: the v0.2.0 tag already exists at an earlier commit;
  re-tagging rewrites history.
- (4) was rejected: the project is not claiming 1.0 stability yet.

## Decision
0.3.0 for the whole workspace (the version-consistency gate requires one
shared version across publishable crates).  The decision is documented in
the L.22 commit and this ADR; the v0.2.0 tag history is untouched.

## Tradeoffs
Gained: honest compatibility signalling; a clean tag.  Given up: a
version-label change touched `Cargo.lock`, which invalidated the
performance-evidence binding and required a full evidence regeneration at
the new version — the price of the lock-binding doctrine.

## Evidence
`cargo semver-checks check-release` output (1 breaking check: the CLI
`AppError::Cancelled` variant); `cargo public-api`; the L.22 commit; the
re-sealed run `phase-l-20260802b`.

## Future implications
The next breaking change forces 0.4.0.  If the API stabilises, a 1.0.0
reset would be a deliberate compatibility statement, not an accident.
