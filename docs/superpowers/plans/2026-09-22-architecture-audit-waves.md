# Architecture Audit Waves — 2026-09-22

> **For AI agents:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to
> implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax.

**Goal:** Execute all seven audit candidates (C1–C7) from the 2026-09-22
architecture review in priority order, in three waves, restoring locality,
purging dangling functionality, gating the test seam, inverting validation
to the intent layer, and moving presentation behind the render seam.

**Architecture:** No new modules. Reuse existing seams:
`content::date` / `engines::shared::parse_activity_date` (C1/C2), delete
unwired surface (C3/C4), first cargo feature `test-support` (C5, ADR-0008),
validation at the intent handler (C7), `render/`-only `ContentBlock`
construction (C6, CONTEXT.md "Render seam").

**ADR allocation:** ADR-0008 (test-support gate), ADR-0009 (delete unwired
DTO catalogue + webhook path). CONTEXT.md gains the **Render seam** term.
No ADR for C1/C2/C4/C6/C7 — self-evident fixes, not trade-offs.

**Tech Stack:** Rust, cargo fmt/clippy/llvm-cov, existing helpers.

**Quality gate after every wave:**

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features --no-fail-fast
```

---

### Wave 1a (C1+C2): Date & sample-selection locality — Strong

- [x] Route all `start_date_local` parses through
  `engines::shared::parse_activity_date` (lenient datetime-aware):
  `intents/handlers/plan_training.rs` (~226, 277, 291, 360, 387),
  `intents/handlers/analyze_race.rs` (~219). **Live bug:** strict
  `%Y-%m-%d` drops real API rows (datetime format).
- [x] Add regression tests with datetime fixtures
  (`"2026-07-01T10:00:00"`) proving rows are no longer dropped.
- [x] Wire `content::date::validate_date_range` into hand-rolled
  `start > end` checks (`plan_training.rs`, engine compare validation —
  wherever they exist after C7).
- [x] Wire `content::date::parse_optional_date` where optional dates are
  hand-parsed (it is currently test-only). **Resolved:** grep found zero
  production hand-parse sites — the premise was wrong; the helper was
  deleted as dead code instead (audit intent: no unwired functionality).
- [x] Dedupe latest-by-date selection:
  `plan_training.rs` (~782–801) must reuse the canonical helper in
  `engines/coach_metrics/parse.rs` (or extract a shared pure helper if the
  shapes differ — one implementation, both callers).
- [x] Replace inlined sleep-bound assertions
  (`assess_recovery.rs` ~1227) with the shared
  `coach_metrics` plausibility/sleep helpers. **Verified N/A:** no
  inline duplicates exist — the helpers are already the single source.

**Files:** `plan_training.rs`, `analyze_race.rs`, `assess_recovery.rs`,
`engines/shared.rs`, `content/date.rs`, `engines/coach_metrics/parse.rs`.

### Wave 1b (C3+C4): Dangling purge — Strong (ADR-0009)

- [x] Delete `src/types.rs` (64/65 DTOs unused), `src/state.rs`,
  `src/event_id.rs`, `src/services.rs`, orphaned `src/tests.rs`.
- [x] Remove `mod`/`pub use` declarations in `lib.rs`; remove webhook
  fields/methods (`webhooks`, `webhook_secret`, `webhook_service`,
  `process_webhook`, `set_webhook_secret_value`) and their unit tests.
- [x] Remove webhook half of `tests/e2e_http.rs::e2e_webhook_and_profile`
  (keep profile coverage); fix `tests/main_initialization.rs` comment.
- [x] Rewrite crate README tool lists to the real 9 intents
  (`plan_training`, `analyze_training`, `modify_training`,
  `compare_periods`, `assess_recovery`, `manage_profile`, `manage_gear`,
  `analyze_race`, `track_progress`); remove 19 phantom tool names.
- [x] Verify: `grep -rn "DownloadStatus\|WebhookEvent\|EventId\|ObjectResult" src/`
  empty (except intentional leftovers, none expected).

### Wave 2a (C5): Test-support feature gate — Strong (ADR-0008)

- [x] Add `[features] test-support = []` to `intervals_icu_mcp/Cargo.toml`.
- [x] Gate `pub mod test_support;` with
  `#[cfg(any(test, feature = "test-support"))]`.
- [x] CI PR job: `cargo test --all --all-features --no-fail-fast`.
- [x] Update root `AGENTS.md` CI-compatible command to include
  `--all-features`.
- [x] Verify: release build compiles without the feature;
  `cargo test --all-features` green.

### Wave 2b (C7): Validation inversion + mock dedup — Worth exploring

- [x] Move required-field / date-range validation from
  `engines/analyze_training/compare.rs` into
  `intents/handlers/compare_periods.rs` (intent layer validates per
  CONTEXT.md); engine assumes validated input.
- [x] Add handler-level tests asserting validation errors (none exist
  today — engine tests must move/adapt with the code).
- [x] Delete `MockCoachClient` from `tests/coach_intents_integration.rs`;
  migrate all 73 tests to `test_support::mock::MockIntervalsClient`
  (comments already mark it "slated for removal in Phase 3B").
- [x] Remove the absent `with_recorded_calls()` reference.

### Wave 3 (C6): Markdown out of non-render engine files — Worth exploring

- [x] Per CONTEXT.md **Render seam**: migrate `ContentBlock::markdown`
  construction from `engines/analyze_training/{single,compare,period}.rs`
  into `engines/analyze_training/render/` (engines return data).
- [x] Unify `"n/a"` / percentage formatting on the shared helpers
  (`content::date::format_pct` and friends); delete hand-rolled variants.
  **Done:** all production `"n/a"` literals route through the
  `content::date::NA` constant (render.rs, single.rs, interval_analysis.rs,
  recovery_rows.rs); test literals pin the wire format and stay.
- [x] `render.rs` stays the presentation module (CONTEXT already sanctions
  it); only non-render engine files change.
- [x] Update engine tests that assert on markdown text to assert through
  the render seam or on returned data.

---

**Out of scope (recorded so they aren't re-suggested):** YAGNI trait stubs
kept per ADR-0005; dynamic adapter stays env-gated per ADR-0001; plan
backlog in `docs/superpowers/specs/` untouched (historical).

**Note:** `docs/superpowers/plans/` was emptied by an external process on
2026-09-22 (~23:24); historical plans were untracked (`.gitignore` had
`docs/`) and are unrecoverable via git. This plan is the first file in the
restored, now-tracked directory.
