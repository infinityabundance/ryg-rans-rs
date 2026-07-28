# xtask

> Build system automation for ryg-rans-rs.

## Commands

- `cargo xtask check` — Run all pre-release gates (FFI check, unsafe ban, test count, docs drafts).
- `cargo xtask seal` — Full release seal (16 gates including SHA-256 chain verification, source freshness, dirty-tree check, Docker matrix evidence).
- `cargo xtask docker` — Run the full Docker VM matrix via `docker/bootstrap-docker.sh`.

## Seal Gate

The seal gate is the project's release mechanism. It enforces:

1. Dirty-tree: no uncommitted changes to covered source files
2. Workspace compilation
3. Core test suite (44 tests)
4. Parity model validity
5. Receipt existence and verdict
6. SHA-256 chain integrity (manifest→receipt, receipt→index, self-hash)
7. Source freshness (no covered files changed after `code_commit`)
8. Unsafe ban in core and casefile
9. Docker matrix evidence (10 jobs, all exit=0, `log_sha256` present)
