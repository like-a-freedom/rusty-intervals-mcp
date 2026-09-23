# ADR-0002 — No Module-Level `#![allow(dead_code)]`

| Field | Value |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-07-25 |
| **Deciders** | Architecture review |

## Context

Module-level `#![allow(dead_code)]` annotations suppress all dead-code warnings for an entire module, masking genuinely dead symbols and making it impossible to distinguish intentional retention from accidental dead code. The `endurance_evidence.rs` module had a module-level `#![allow(dead_code)]` at line 30, hiding unknown dead fields.

## Decision

**Ban module-level `#![allow(dead_code)]`.** Replace with item-level `#[allow(dead_code)]` annotations that include a justification comment explaining why the item is retained.

### Triage policy for dead symbols

Every dead symbol must be triaged:

| Outcome | Action |
|---|---|
| **Wire** | The symbol is needed by a caller that doesn't exist yet. Add the caller or wire the symbol into the data flow. |
| **Delete** | The symbol is genuinely unused (YAGNI). Delete it and its assignment. |
| **Retain** | The symbol mirrors an upstream schema field that may be populated in the future, or is a test fixture. Add `#[allow(dead_code)]` with a justification comment. |

### Justification comment format

```rust
/// Retained because <reason>. <What would break if deleted?>
#[allow(dead_code)]
```

## Consequences

- **Positive:** Dead code is visible. Each `#[allow(dead_code)]` has a human-readable reason.
- **Positive:** The compiler surfaces genuinely dead symbols instead of hiding them.
- **Positive:** Future maintainers can audit each allowance independently.
- **Negative:** Slightly more verbose. Mitigated by the justification comment format.

## Application

Applied to:

- `engines/endurance_evidence.rs` — module-level allow removed; four item-level allows added with justifications:
  - `Gap` / `HrSegment` test fixtures (deserialise-only, schema mirror)
  - `ControlWindow.session_date` — deleted (YAGNI, never read)
- `src/test_support.rs` — `with_pace_histogram` builder ergonomic completeness
- `tests/coach_intents_integration.rs` — `MockCoachClient` counters/methods slated for Phase 3B mock-consolidation removal

## Verification

`cargo clippy --all-targets --all-features -- -D warnings` must pass with zero warnings after applying this ADR.
