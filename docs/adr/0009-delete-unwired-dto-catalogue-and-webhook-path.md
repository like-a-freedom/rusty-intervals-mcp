# ADR-0009 — Delete the Unwired DTO Catalogue and Webhook Path

| Field | Value |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-09-22 |
| **Deciders** | Architecture review (2026-09-22 waves) |

## Context

The 2026-09-22 audit found a capability-boundary surface with zero leverage:

- **`src/types.rs`** defines 65 public DTOs; **64 are referenced nowhere**
  outside the file itself (the 65th, `ObjectResult`, is used only by the
  webhook path). The names (`DownloadParams`, `CreateEventParams`,
  `PlanTrainingWeekParams`, …) mirror the 19 snake_case "tools" the crate
  README advertised but that never existed on the real surface — the real
  contract is the 9 curated intents (`IntentRouter`). Download behavior was
  already removed from production in commits `c5bb11a` / `6d56f7c`.
- **Webhook path**: `WebhookService` (`services.rs`), `WebhookEvent` /
  `DownloadState` / `DownloadStatus` (`state.rs`), `EventId` / `FolderId`
  (`event_id.rs`), plus `AppState` fields and
  `handler.process_webhook` / `set_webhook_secret_value`. There is **no
  `/webhook` route in production** `run_http_server`; the route exists only
  on a test-built router in `tests/e2e_http.rs`.
- **`src/tests.rs`**: orphaned (never declared in the module tree), would
  not compile if wired (duplicate `fn test_handler()`), and references a
  plan file that never existed in git.

### Considered options

1. **Wire the features (rejected).** Turning the DTOs/webhook into live
   capability is product work that re-opens the exact CRUD-mirror /
   raw-upstream-surface anti-pattern ADR-0001 rejected, and violates YAGNI
   for features with no current use case.
2. **Keep, marked `#[doc(hidden)]` (rejected).** Interface debt stays;
   dead vocabulary keeps inviting callers that can never be served.
3. **Delete (chosen).** The deletion test: deleting this surface
   concentrates nothing — no caller changes behavior, complexity only
   disappears. The documented interface (README) is rewritten to the real
   9-intent contract.

## Decision

**Delete everything unwired; the capability boundary is the 9 curated
intents.**

Removed: `src/types.rs` (whole module), `src/state.rs`,
`src/event_id.rs`, `src/services.rs`, the webhook fields/methods on
`AppState`/handler in `lib.rs`, their unit tests, the webhook half of
`e2e_webhook_and_profile`, the orphaned `src/tests.rs`, and the `pub use`
re-exports of `EventId`/`FolderId`/`DownloadState`/`DownloadStatus`/`WebhookEvent`/`types::*`.
README tool lists rewritten to the 9 intents.

Re-introducing any of this later is a product decision requiring its own
spec — not a resurrection of these types.

## Consequences

- **Positive:** `pub mod types` and the download/webhook vocabulary leave
  the public surface; README and code agree on the 9-intent contract.
- **Positive:** `lib.rs` loses webhook state (`webhooks` map,
  `webhook_secret`) and two dead pub methods.
- **Negative:** the e2e webhook round-trip test is deleted with the
  feature (its assertions only ever exercised a test-only router).
- **Negative:** if webhooks/downloads ever become real requirements, they
  start from a blank spec. That is the intended cost of YAGNI.
