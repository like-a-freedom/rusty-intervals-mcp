# Domain Context — rusty_intervals_mcp

Single-context repo. This glossary is the canonical vocabulary for code, plans,
ADRs, and review output. When a term appears in an issue title, refactor
proposal, or test name, use the term as defined here.

## System shape

The server is an MCP (Model Context Protocol) bridge between an LLM host and the
Intervals.icu coaching API. Two transport modes (STDIO single-user, HTTP
multi-tenant) share one execution pipeline.

## Two-layer architecture

| Layer | Role | Stability |
|---|---|---|
| **Intent layer** (`intents/`) | Public MCP contract: nine curated coaching tools (`plan_training`, `analyze_training`, `modify_training`, `compare_periods`, `assess_recovery`, `manage_profile`, `manage_gear`, `analyze_race`, `track_progress`). `IntentRouter` is the only source of `rmcp::model::Tool` definitions. | Stable. Curated. Survives upstream API drift. |
| **Dynamic upstream adapter** (`dynamic/`) | Private capability catalogue: fetches, parses, caches the live OpenAPI spec from `/api/v1/docs`, exposes operation metadata, and dispatches HTTP calls for the intent layer. | Adapts. Reflects current upstream shape. Not visible to MCP clients. |

The two layers connect via an `IntervalsClient` adapter: the typed trait stays
as the intent-side contract; the dynamic runtime is the wire-side transport.
See ADR-0001.

## Vocabulary

### Architecture terms

- **Intent** — a coaching capability exposed to the LLM host. Has a curated
  name, input schema, output schema, and handler. Never derived from an
  upstream operation id.
- **Capability boundary** — the public MCP surface. Crossed only by
  `tools/list` and `tools/call`. Enforced by `IntentRouter`.
- **Upstream operation** — a single HTTP operation in the Intervals.icu API,
  identified by its OpenAPI `operationId`. Private. May be invoked by the
  intent layer through the dynamic adapter.
- **Dynamic adapter** — the private component that turns an upstream operation
  id into an actual HTTP call. Lives behind `IntervalsClient`, never in front.
- **Strangler fig** — the migration pattern: typed `IntervalsClient` methods
  and dynamic dispatch coexist; new endpoints arrive via dynamic dispatch
  immediately, typed methods migrate incrementally. See ADR-0001.
- **Render seam** — presentation is not engine logic. Engines never
  construct `ContentBlock`s; engine presentation lives in
  `engines/analyze_training/render/` (data flows through
  `engines/{single,compare,period}.rs` untouched). Intent handlers own
  theirs, including `intents/handlers/render/` and handler-local markdown.

### Auth and token terms

- **Token record** — the server-side record of one issued bearer token:
  `jti`, athlete, issue/expiry timestamps, revocation state, and the athlete's
  upstream credential encrypted at rest.
- **Token registry** — the repository of token records, keyed by `jti`. The
  single source of truth for revocation and credential resolution for newly
  issued JWTs (from `/auth` and the UI alike). See [ADR-0011](docs/adr/0011-server-side-token-registry.md).
- **Upstream credential** — an athlete's Intervals.icu API key. Stored encrypted
  in the token registry; never embedded in a newly issued JWT.
- **Credential rotation** — replacement of the upstream API key on the same
  token record, preserving JWT identity, expiry, and replay ownership. Available
  to Rust embedders, not via HTTP, the operator panel, or MCP.
- **Legacy token** — a JWT that carries its encrypted upstream credential in
  its claims, with or without a registry `jti`. Valid legacy claims can still
  supply credentials when no stored credential is available; a known revocation,
  athlete mismatch, or registry read failure is not bypassed.
- **Operator panel** — the `/ui` token-management surface. A trusted,
  network-protected operator view (like `/metrics`), not a multi-tenant
  authenticated surface. OAuth discovery and exchange are proposed, not implemented.

### Mutation safety terms

- **Mutation journal** — local SQLite record of mutation reservations and outcomes,
  scoped by upstream base URL, athlete, and idempotency token. Standard STDIO
  and HTTP share this storage model; HTTP also persists the token registry there.
  MCP and browser sessions are separate. See [ADR-0012](docs/adr/0012-single-process-sqlite-state.md).
- **Replay owner** — the verified JWT `jti`, or a digest of the exact JWT when
  it lacks a `jti`. A reissued JWT is a different owner; STDIO uses its configured
  upstream and athlete without a JWT owner.
- **Replay** — return a retained completed result for the same operation scope,
  request fingerprint, and owner without repeating the upstream write.
- **Unknown outcome** — a reserved write whose effect could not be confirmed.
  It stays blocked across restarts; there is no supported in-server resolution action.

### Metric-domain terms

- **Load** — training stress value for one activity. Canonical source:
  `icu_training_load` (alias: `training_load`, `icuTrainingLoad`). Never
  derived from `moving_time`. Tracked in
  `analysis_fetch::extract_activity_load`.
- **Coverage** — fraction of activities in a window that expose a usable load
  value. Surfaced as `activities_with_load / activities_total`.
- **eFTP / W' / pMax** — Intervals.icu's power-duration anchors. Surfaced via
  `EspePowerAnchors`; never re-modelled locally.
- **HRV / RHR baseline** — 60-day reference window, 7-day recent window,
  ln-transformed HRV, SWC = mean × CV × 0.5. Positions are neutral labels
  (Below / Within / Above), never medical claims.
- **Endurance evidence** — cycling-only, protocol-bound HR–power observations
  in matched 10-minute windows. Surfaced as observations with explicit
  availability reasons, never as composite scores or readiness labels. See
  `docs/METRIC_METHODS.md`.

### Module placement

| Concern | Module |
|---|---|
| Domain types (structs, enums) | `domains/` |
| Pure computation (no I/O) | `engines/` |
| I/O orchestration (fetch, decode, transform) | `engines/analysis_fetch/` |
| MCP intent handlers (validation + dispatch + render) | `intents/handlers/` |
| Presentation (markdown rendering) | `intents/handlers/render/` and `engines/analyze_training/render/` |
| Upstream HTTP transport | `intervals_icu_client::http_client` |
| Dynamic OpenAPI runtime | `dynamic/` |

## Out of vocabulary

Avoid: "service" (use **module**), "boundary" (use **seam** or **capability
boundary**), "API" for the MCP surface (use **contract** or **capability**),
"component" (use **module**).
