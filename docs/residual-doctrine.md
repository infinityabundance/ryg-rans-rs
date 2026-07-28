# Residual Doctrine

**Project:** `ryg-rans-rs` — Rust port of `ryg_rans` by Fabian Giesen  
**Upstream commit:** `c9d162d996fd600315af9ae8eb89d832576cb32d`  
**Doctrine:** Bitstream parity — every Rust encoder/decoder must produce and consume byte-identical streams to the upstream C/C++ reference. Residual primacy — every observed diff is a first-class artifact until resolved.

---

## Residual Primacy

A **residual** is any observed difference between the Rust implementation and the upstream C/C++ reference at any level of the oracle court hierarchy — from individual arithmetic results to decoded output.

Residual primacy means residuals are the **primary engineering artifact** of the project. They are not merely bugs to be fixed; they are evidence that must be recorded, classified, and resolved before parity can be claimed. A surface without residuals is not `full` — it is merely `unexamined`.

### Principles

1. **Every diff is a residual.** If an oracle court detects any mismatch, a residual record must be created before the court run is considered complete. No exception for "minor" or "expected" diffs.

2. **Residuals persist.** Once created, a residual record is never deleted. Even after resolution, the record remains with status `"fixed"` for auditability and regression detection.

3. **No deleting failing cases.** If a casefile produces a residual, the casefile is retained. Deleting the casefile to make residuals disappear is forbidden. The correct response is to fix the implementation or document the residual as `"wontfix"` with justification.

4. **Resolution requires classification.** Before a residual can be marked `"fixed"` or `"wontfix"`, it must be classified by root cause.

---

## Residual Record

Residuals are stored as JSON in `reports/residuals/`. Each residual file represents one observed diff:

```json
{
    "schema_version": 1,
    "case_id": "byte-rans/freq-2/many-symbols",
    "court_id": "state-transition.byte-rans",
    "variant": "32-bit byte rANS (reciprocal)",
    "upstream_commit": "c9d162d996fd600315af9ae8eb89d832576cb32d",
    "class": "implementation",
    "severity": "S1",
    "status": "investigating"
}
```

### Fields

| Field | Description |
|---|---|
| `case_id` | Identifies the deterministic casefile that produced the diff |
| `court_id` | Identifies the oracle court that detected the diff |
| `variant` | Algorithmic variant (e.g., "32-bit byte rANS", "64-bit rANS", "SSE4.1 decoder") |
| `upstream_commit` | Always `c9d162d9` for this project |
| `class` | Root cause classification (see below) |
| `severity` | Impact severity: S0, S1, S2, or S3 |
| `status` | Lifecycle status: `"open"`, `"investigating"`, `"fixed"`, `"wontfix"` |

---

## Severity Levels

### S0 — Critical

Bitstream incompatibility at the cross-decode level. A Rust-encoded stream cannot be decoded by the upstream C/C++, or vice versa.

**Action required:** Must be resolved before the surface can be marked `full`. No release with S0 residuals.

**Examples:**
- Rust encoder produces a different byte sequence than upstream C.
- Rust decoder mis-decodes a valid upstream-encoded stream.

### S1 — Major

State-transition mismatch that does not affect cross-decode (e.g., internal state differs but final output matches). Indicates the implementation diverges from upstream in a way that may become S0 under different inputs.

**Action required:** Must be investigated and understood. May be promoted to S0 or demoted to S2 based on investigation.

**Examples:**
- Renormalization loop produces different intermediate states but same final state.
- Internal buffer positions differ during encoding but final output matches.

### S2 — Minor

Observable difference that does not affect bitstream correctness and has no plausible path to become S0. Usually matters for performance counting, statistics, or diagnostics.

**Action required:** Documented and understood. No release-blocking effect.

**Examples:**
- Different order of operations in reciprocal that produce same arithmetic result.
- Different error message text for malformed input.
- Performance counter differences.

### S3 — Informational

Not a real diff but recorded for completeness. Typically oracle or casefile artifacts.

**Action required:** None, but must be documented.

**Examples:**
- Floating-point differences in alias table construction that produce same integer results.
- Timing differences in performance measurement.
- Codegen differences that don't affect semantics.

---

## Classification Classes

| Class | Description |
|---|---|
| `implementation` | Bug or divergence in the Rust implementation |
| `oracle` | Bug in the oracle harness (trace extraction, comparison logic) |
| `casefile` | Bug in the test case (invalid parameters, wrong expected values) |
| `upstream` | Behavior in the upstream C/C++ that is undefined, unspecified, or platform-dependent |
| `design` | Intentional design divergence (documented in negative capabilities) |

---

## Residual Lifecycle

```
open ──→ investigating ──→ fixed
                            └──→ wontfix (with justification)
              ↑
         reopened (if regression detected)
```

### Transitions

1. **`open`**: Residual is newly created by an oracle court run. No investigation has occurred.

2. **`investigating`**: An engineer is actively working to determine the root cause. May include notes in the residual record.

3. **`fixed`**: The root cause was identified and corrected. The oracle court has been re-run and the residual no longer fires. The residual record is retained for audit.

4. **`wontfix`**: The residual is intentional or acceptable. Must include a justification referencing the negative capabilities ledger or a specific design decision.

5. **Regression**: If a previously `fixed` residual reappears (e.g., after a refactor), the status is reset to `investigating`.

---

## Verifying Residuals

The `cargo xtask residuals verify` command checks:

1. Every oracle court receipt lists all residuals it produced.
2. Every residual listed in a receipt has a corresponding file in `reports/residuals/`.
3. No residual file exists without a corresponding receipt (orphan detection).
4. All residuals have a non-empty status and severity.

---

## Interaction with the Gap Ledger

The gap ledger (`docs/gap-ledger.md`) tracks which algorithmic surfaces from the upstream are implemented (`full`) versus not yet implemented (`scaffold`). Residuals are separate: a surface can be implemented successfully (all state transitions match) but still have open residuals at the S2 or S3 level. A surface is not marked `full` until:

1. All required courts pass at S0 and S1 clearance.
2. All S0 and S1 residuals are resolved (status `"fixed"` or `"wontfix"`).
3. Cross-decode court passes in both directions.
