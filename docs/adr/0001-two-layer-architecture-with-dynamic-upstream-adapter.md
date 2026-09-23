# ADR-0001 — Two-Layer Architecture with Dynamic Upstream Adapter

| Field | Value |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-07-25 |
| **Deciders** | Architecture review |

## Context

The MCP server exposes a curated intent layer (nine coaching tools) on top of the Intervals.icu API. The upstream API is large (~156 operations) and changes over time. Two conflicting approaches were proposed:

1. **Expose all upstream operations as MCP tools** (`candidate-04-wire-dynamic-runtime.md`) — violates the capability boundary, creates CRUD-mirror anti-pattern, and exposes raw upstream schema to LLM clients.
2. **Delete the dynamic OpenAPI runtime entirely** (`restore-intent-driven-mcp-tool-boundary.md`) — removes all production readers of `dispatch_openapi`, leaving ~3,400 LOC of dynamic infrastructure fully dead.

Both approaches are wrong. The intent layer must stay curated and stable; the dynamic runtime must stay alive and wired.

## Decision

Adopt a **two-layer architecture with a strangler-fig adapter**:

| Layer | Module | Role | Stability |
|---|---|---|---|
| **Intent layer** (`intents/`) | Public MCP contract | Nine curated coaching tools with stable schemas | Stable |
| **Dynamic upstream adapter** (`dynamic/`) | Private capability catalogue | Fetches, parses, caches live OpenAPI spec; dispatches HTTP calls for intent layer | Adapts |

The two layers connect via an `IntervalsClient` adapter:

- The typed `IntervalsClient` trait stays as the intent-side contract (stable, curated).
- The `DynamicClientAdapter` wraps `Arc<DynamicRuntime>` + `Arc<dyn IntervalsClient>` (typed fallback) and implements the full `IntervalsClient` trait.
- New upstream operations arrive via dynamic dispatch immediately; typed methods migrate incrementally (strangler fig).

### Dispatch semantics

| Condition | Outcome |
|---|---|
| Registry unavailable | `Ok(None)` — silent fallthrough to typed fallback |
| Operation unmapped | `Ok(None)` — silent fallthrough to typed fallback |
| Dispatch fails | `Err` — surface per this ADR |
| HTTP non-2xx | `Err(IntervalsError::from_status(...))` — preserves status so `is_not_found` / `is_auth_error` / `is_rate_limited` keep working |
| Decode fails | `Err(IntervalsError::from(serde_json::Error))` |

### Error handling

Dynamic errors surface (no silent fallback). The `intervals_icu_mcp_dynamic_dispatch_total{method, outcome}` metric records six outcomes: `dispatched`, `registry_unavailable`, `dispatch_error`, `http_error`, `decode_error`. The silent fallthrough path (registry unavailable or operation unmapped) is intentionally not recorded — it is the normal state during strangler-fig migration.

## Consequences

- **Positive:** The dynamic runtime is no longer dead code. It is wired through `DynamicClientAdapter`, env-var gated (`INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED`), and proven through a real call site (`get_athlete_training_plan`).
- **Positive:** New upstream operations can be exposed without touching the typed trait. Handlers that need an operation not in the trait use the dynamic adapter directly.
- **Positive:** The intent layer's nine-tool boundary is preserved. No upstream operation id is ever visible to MCP clients.
- **Negative:** Two code paths exist for each upstream operation (typed + dynamic). Migration is incremental, so some operations may never be typed. This is acceptable — the strangler fig is a migration pattern, not a mandate.

## Open Questions

1. **Mapping location:** Should operation-to-handler mappings live in code or in a config file? Current default: code (type-safe, compiled). Revisit after 2C if mapping churn becomes a problem.
2. **Error policy:** Should dispatch errors silently fall through to typed fallback? Current default: surface errors (per this ADR). Revisit if error rate is high during migration.
3. **Auto-generation:** Should `IntervalsClient` methods be auto-generated from the OpenAPI spec? Current default: no (manual, curated). Revisit after 2C.

## Verification Gate

ADR-0001 is verified when a handler uses an upstream operation not in the typed `IntervalsClient` trait, proving the adaptivity claim. This gate is closed by `DynamicClientAdapter::get_athlete_training_plan` (Phase 2C).
