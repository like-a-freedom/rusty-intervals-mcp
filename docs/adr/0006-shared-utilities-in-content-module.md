# ADR-0006 — Shared Utilities Live in `crate::content`, Not `crate::intents`

| Field | Value |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-07-28 |
| **Deciders** | Architecture review |

## Context

The 2026-07-27 architecture audit (finding F-2; applied in commit `9f09eae`
— "refactor(mcp): move date/format helpers from intents::utils to
content::date (audit F-2)")
identified a DDD direction inversion: 14 pure date / format helpers lived
in `crates/intervals_icu_mcp/src/intents/utils.rs`, but five call sites in
`engines/` and `domains/` (the **lower** layers in the DDD sense) imported
them. A lower layer was naming a higher layer's utility module.

The pattern had already been corrected once. Commit `b512b5d` extracted
`ContentBlock`, `IntentError`, and `OutputMetadata` from `intents/types.rs`
into `crate::content`, with both `intents/` and `engines/` consuming the
new path. The `intents/utils.rs` helpers (date parsing, activity / event
filtering, data-availability rendering, percentage formatting) were missed
in that pass.

### Affected symbols

13 `pub` functions + 1 private function in `intents/utils.rs`:

| Symbol | Used by |
|---|---|
| `parse_date` | engines/analyze_training/{single,compare,period}.rs, intents/handlers/{plan_training,modify_training/actions}.rs |
| `parse_optional_date` | intents/handlers (via `parse_date`) |
| `filter_activities_by_date` | engines/analyze_training/single.rs, intents/handlers |
| `filter_activities_by_range` | engines/analyze_training/period.rs |
| `filter_activities_by_description` | intents/handlers/analyze_race.rs |
| `filter_events_by_date` | intents/handlers/modify_training.rs |
| `filter_events_by_range` | engines/analyze_training/period.rs, intents/handlers/modify_training.rs |
| `normalize_date_str` | domains/{events,wellness}.rs (events re-exports) |
| `normalize_event_start` | domains/events.rs (re-export) |
| `validate_date_range` | intents/handlers |
| `data_availability_block` | engines/analyze_training/{single,period}.rs, intents/handlers/analyze_race.rs |
| `format_pct` | engines/analyze_training/{compare,period}.rs |
| `resolve_relative_day_alias` (private) | `parse_date` (internal) |

### Existing layer assignment (per `CONTEXT.md` § Module placement)

| Concern | Module |
|---|---|
| Domain types | `domains/` |
| Pure computation (no I/O) | `engines/` |
| I/O orchestration | `engines/analysis_fetch/` |
| MCP intent handlers | `intents/handlers/` |
| Presentation | `intents/handlers/render/`, `engines/analyze_training/render/` |

`crate::content` already hosts cross-cutting types (`ContentBlock`,
`IntentError`, `OutputMetadata`) imported by both `engines/` and
`intents/`. The date / filter helpers are the same shape: pure functions
operating on plain data (`ActivitySummary`, `Event`, `NaiveDate`,
`Option<f64>`).

### Why this is a smell, not just a style preference

1. **Reading order.** A new contributor navigating `engines/analyze_training/single.rs`
   sees `use crate::intents::utils::...` and reasonably asks "why does
   the engine know about the intent layer?" The reverse direction would
   be normal; the forward direction is not.
2. **Refactor friction.** When `intents/utils.rs` is touched for an
   intent-handler concern, `engines/` and `domains/` recompile and have
   to be reasoned about. Pure date helpers should compile only against
   `chrono` and the client crate.
3. **No cycle today, but the precedent invites one.** As `intents/`
   grows, the temptation to add "one more helper" to `utils.rs` is high,
   and engines will keep importing from it. The audit caught the
   inversion now; this ADR prevents re-occurrence.

## Decision

**All shared, pure helpers live under `crate::content::*`, not
`crate::intents::utils`.**

### Layout

```
crates/intervals_icu_mcp/src/
├── content.rs              # ContentBlock, IntentError, OutputMetadata
│                           # (unchanged; conversion to mod.rs deferred)
└── intents.rs               # root module — no longer declares `pub mod utils`
```

The 13 `pub` helpers + `resolve_relative_day_alias` (private) move from
`intents/utils.rs` into **`crate::content::date`** as a sibling
`content/date.rs` module. The target module name is `date` because all
14 helpers operate on dates, date-ranges, and date-derived data
(filtering activities / events, formatting percentages that come from
time-windowed computations). Future shared helpers go into other
`content::*` siblings (`content::markdown`, `content::pagination`, etc.)
as the need arises.

### Downstream dependency note

`content::date` imports `crate::engines::shared::parse_activity_date`
(this is what `filter_activities_by_date` already does today). The
reverse direction (any `engines::` module importing from
`crate::intents::utils`) is removed. No new module cycle is created:

```
domains/        →  (no helpers)
engines/        →  content::date        ✓ allowed
content::date   →  engines::shared      ✓ allowed (single, narrow dep)
intents/handlers/ → content::date       ✓ allowed
```

`engines::shared::parse_activity_date` is `pub(crate)` and contains no
upward dependency, so the `content::date → engines::shared` edge stays
acyclic.

### Why delete `intents/utils.rs` instead of keeping a re-export shim

`intents::utils` is not reachable from outside the crate
(`intents::utils::*` is reachable via `pub use utils::*;` in `intents.rs`,
but the only consumers are the 9 internal call sites). Keeping a
re-export shim would:

- preserve a backward-compat path that has zero external callers
  (YAGNI);
- continue to invite new helpers to land under the wrong module
  (the very behavior this ADR prevents);
- cost 1 LOC and a confusing path that tells future readers "this used
  to live here, now it doesn't".

Cleaner to delete it and update all 9 import sites in one pass.

## Consequences

- **Positive:** Lower layers (`engines/`, `domains/`) no longer name
  higher-layer utility modules.
- **Positive:** `crate::content` becomes the single home for
  cross-cutting, pure helpers — consistent with how `ContentBlock` /
  `IntentError` already live there.
- **Positive:** Future shared helpers have an obvious place to land.
- **Positive:** `intents::utils` ceases to exist; no stale shim
  accumulates.
- **Negative:** Touches 9 import sites across 9 files. Mechanical
  change, no behavior delta.
- **Negative:** `content.rs` should be converted to `content/mod.rs`
  to make room for `content/date.rs`. This conversion is part of the
  same change; no separate ADR needed.

## Application

Applied to:

- `crates/intervals_icu_mcp/src/content.rs` → `src/content/mod.rs`
  (verbatim content + `pub mod date;` declaration).
- `crates/intervals_icu_mcp/src/content/date.rs` — new file, holds 13
  `pub fn`s + 1 private `fn` + 55 `#[test]` cases copied verbatim from
  `intents/utils.rs`.
- `crates/intervals_icu_mcp/src/intents/utils.rs` — deleted.
- `crates/intervals_icu_mcp/src/intents.rs` — line `pub mod utils;`
  removed; line `pub use utils::*;` removed.
- 9 import sites updated to `use crate::content::date::{...};`:
  - `engines/analyze_training/single.rs:34`
  - `engines/analyze_training/compare.rs:13`
  - `engines/analyze_training/period.rs:28`
  - `domains/events.rs:4` (was `pub use`; becomes internal `use` +
    re-export kept)
  - `domains/wellness.rs:34`
  - `intents/handlers/modify_training.rs:15`
  - `intents/handlers/analyze_race.rs:19`
  - `intents/handlers/plan_training.rs:18`
  - `intents/handlers/modify_training/actions.rs:13`

`domains/events.rs` retains `pub use` of `normalize_date_str` and
`normalize_event_start` (it was a re-export to its consumers); the
import path changes from `crate::intents::utils` to
`crate::content::date`. No consumer of `domains::events` is affected
because the re-export symbol names stay identical.

## Verification

After applying:

```sh
cargo fmt --all -- --check                                          # green
cargo clippy --all-targets --all-features -- -D warnings            # 0 warnings
cargo test --all-targets --all-features --no-fail-fast              # 2392 passed, 0 failed
grep -rn "intents::utils" crates/intervals_icu_mcp/src/              # empty
grep -rn "crate::intents::utils" crates/                             # empty
```

Test count must equal the audit baseline (2392). Any drift means a
helper was lost in the move.
