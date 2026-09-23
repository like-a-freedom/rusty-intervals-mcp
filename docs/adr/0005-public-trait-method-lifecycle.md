# ADR-0005 — Public Trait Method Lifecycle

| Field | Value |
|---|---|
| **Status** | Implemented (revised scope, 2026-07-27) |
| **Date** | 2026-07-27 |
| **Deciders** | Architecture audit + post-plan cross-check |

## Context

The `IntervalsClient` trait (`lib.rs:127-346`) has 50+ methods. The original
architecture audit (F-3 / F-18) flagged 8 of
them as YAGNI stubs returning `Err(ConfigError::Unsupported { .. })` defaults.

After cross-checking `docs/superpowers/plans/` (31 files), the deletion scope
proved to be **incorrect**:

1. **`update_wellness_bulk`** — listed as "YAGNI, no production caller" in the
   audit, but the codebase contains:
   - A real implementation in `http_client.rs:1284-1288` (PUT
     `/athlete/{id}/wellness-bulk`)
   - A contract test at
     `crates/intervals_icu_client/tests/http_client_contract.rs:286-299`
   - A mock impl + dedicated test at
     `crates/intervals_icu_mcp/src/test_support.rs:1989-1993, 3032-3037`
   This is a **fully wired, tested method** — not YAGNI. The audit missed it.

2. **`get_activity_messages`** — listed as YAGNI with "Err default body", but
   `http_client.rs:536-540` already implements it against
   `/api/v1/activity/{id}/messages`. The audit's framing was stale; production
   call site at `engines/analysis_fetch/fetch.rs:405` (`.unwrap_or_default()`)
   still deserves the silent-error fix described in the original ADR, but the
   method itself stays.

3. **Six weather/route methods** (`get_weather_config`, `update_weather_config`,
   `list_routes`, `get_route`, `update_route`, `get_route_similarity`) — audit
   correct: stubs in trait, real implementations in `http_client.rs:1290+`,
   zero production callers, only `test_support.rs` mock impls + a
   `_ = client.list_routes()...` smoke block in `intents/router.rs:1071-1075`.

4. **No plan documents these methods.** `grep -E
   'weather_config|list_routes|get_route|update_route|route_similarity|wellness_bulk'
   docs/superpowers/plans/*.md` returns **zero matches** across all 31 plan
   files. The YAGNI verdict was an inference from grep, not a recorded decision.

5. **Precedent warns against deletion.** `2026-07-05-gap-analysis-deleted-code.md`
   documents that code removed as "dead" during the 2026-07-04 audit
   (`AdaptationState`, `parameterized_load`, `compute_taper_efficiency`) was
   later found to be required by plans P1.2 / P3.1 — and had to be restored.
   Same risk applies here: deleting `update_wellness_bulk` would remove a
   working method with a contract test and an integration test.

## Decision

**Establish a lifecycle policy for `IntervalsClient` trait methods, but do not
delete the 6 weather/route methods.**

### Inclusion criteria (unchanged from original ADR)

A method earns a place on `IntervalsClient` when:
1. It is called by at least one production code path (engine or intent
   handler), OR
2. It is called by at least one integration/E2E test that exercises a real
   code path, OR
3. It mirrors an upstream endpoint that is explicitly planned for use
   (documented in a spec under `docs/superpowers/specs/`).

Methods that do not meet these criteria must NOT live on the public trait
**as primary public API**. They may live on the trait under `#[doc(hidden)]`
plus an explicit YAGNI-candidate note.

### Revised disposition (corrections in **bold**)

| Method | Disposition | Rationale |
|--------|------------|-----------|
| `get_activity_messages` | **Keep + fix silent caller** | Already implemented in `http_client.rs:536`. Original ADR's "Err default body" claim was wrong. Open follow-up: replace `.unwrap_or_default()` at `engines/analysis_fetch/fetch.rs:405` with explicit match — out of scope for this ADR. |
| `update_wellness_bulk` | **Keep** | Has working implementation, contract test, mock test. Not YAGNI. Original ADR's "no production caller" claim was wrong. |
| `get_weather_config` | **Hide via `#[doc(hidden)]`** | Genuinely YAGNI today. Mark YAGNI-candidate so it does not appear in rendered docs. Re-promote if a real caller appears. |
| `update_weather_config` | **Hide via `#[doc(hidden)]`** | Same. |
| `list_routes` | **Hide via `#[doc(hidden)]`** | Same. `router.rs:1071-1075` smoke block remains in place — it is `_ = …` and harmless. |
| `get_route` | **Hide via `#[doc(hidden)]`** | Same. |
| `update_route` | **Hide via `#[doc(hidden)]`** | Same. |
| `get_route_similarity` | **Hide via `#[doc(hidden)]`** | Same. |

### Annotation pattern

```rust
/// Update wellness entries in bulk (PUT /athlete/{id}/wellness-bulk).
///
/// Production callers: none today. Method exists on the public trait for
/// parity with the upstream Intervals.icu API.
///
/// Per ADR-0005, this is a **YAGNI candidate**: if a real caller appears,
/// drop the `#[doc(hidden)]` annotation. Until then it stays out of rendered
/// docs but remains on the trait surface for mock impls and contract tests.
#[doc(hidden)]
async fn update_wellness_bulk(&self, _entries: &[serde_json::Value]) -> Result<()> {
    Err(IntervalsError::Config(ConfigError::Unsupported {
        method: "update_wellness_bulk",
    }))
}
```

### Lifecycle rule for future methods (unchanged)

When a new upstream endpoint needs client access:
1. **Start in `DynamicClientAdapter`** (dynamic dispatch) — no trait change.
2. When the endpoint stabilizes and has ≥1 real caller, **add to
   `IntervalsClient`** with a real implementation in `ReqwestIntervalsClient`.
3. Never add a method with only a `ConfigError::Other("not implemented")`
   default — but a stub returning `ConfigError::Unsupported { method }` is
   acceptable **iff** it is annotated `#[doc(hidden)]` with the ADR-0005
   YAGNI-candidate note.

## Consequences

- **Positive:** Six methods stay available for mock/contract-test purposes
  without polluting rendered `cargo doc` output. Library consumers do not see
  them as "supported" methods.
- **Positive:** `update_wellness_bulk` and `get_activity_messages` keep
  their working implementations, contract tests, and integration coverage.
- **Positive:** Future endpoint additions follow the documented lifecycle —
  no risk of silent YAGNI growth.
- **Positive:** No production behavior changes; no callers updated; no
  contract tests removed; no risk of breaking external library consumers.
- **Negative:** Trait still has 50+ methods. The visual surface stays large,
  but rendered docs now show only the supported set (43+1 = 44 — plus the
  6 hidden ones are discoverable via source but not via docs.rs).
- **Negative:** The original audit's "remove from trait" recommendation is
  **not applied**. This is an explicit downgrade of remediation scope based
  on plan cross-check.

## Verification

- `cargo test --all-targets --all-features` — passes (no test changes).
- `cargo doc --no-deps -p intervals_icu_client` then
  `rg 'fn (get_weather_config|update_weather_config|list_routes|get_route|update_route|get_route_similarity)' target/doc/intervals_icu_client/*.html`
  — zero matches in rendered docs (confirmed hidden).
- `cargo doc --no-deps -p intervals_icu_client --document-hidden-items`
  — six methods **do** appear with their YAGNI-candidate notes.
- `cargo clippy --all-targets --all-features -- -D warnings` — clean.
- `cargo fmt --all -- --check` — clean.

## Open follow-ups (out of scope)

1. Replace `.unwrap_or_default()` at
   `crates/intervals_icu_mcp/src/engines/analysis_fetch/fetch.rs:405` with
   explicit match on `IntervalsError::Transport` (transport is now isolated
   via ADR-0003). Tracked separately.
2. Consider periodic audit (quarterly) of `#[doc(hidden)]` methods to
   re-evaluate whether any should be promoted to public.
