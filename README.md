# Intent-Driven, Token-Efficient Intervals.icu MCP Server

A high-performance **Rust MCP server for Intervals.icu** designed around one idea: an LLM should interact with a **small, semantically rich coaching interface**, not a raw pile of endpoint wrappers.

[![.github/workflows/ci.yml](https://github.com/like-a-freedom/rusty-intervals-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/like-a-freedom/rusty-intervals-mcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/like-a-freedom/rusty-intervals-mcp?label=release)](https://github.com/like-a-freedom/rusty-intervals-mcp/releases)
[![codecov](https://codecov.io/gh/like-a-freedom/rusty-intervals-mcp/graph/badge.svg?token=I47UV16VY5)](https://codecov.io/gh/like-a-freedom/rusty-intervals-mcp)
![Rust](https://img.shields.io/badge/rust-stable-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)

> **Public contract:** 9 high-level intents + 1 resource  
> **Internal execution layer:** dynamic OpenAPI runtime that stays aligned with Intervals.icu  
> **Design goal:** respect the agent's context window and return decision-ready coaching context

## Table of Contents

- [Why this project exists](#why-this-project-exists)
- [What makes it different](#what-makes-it-different)
- [Public MCP surface](#public-mcp-surface)
- [Architecture at a glance](#architecture-at-a-glance)
- [Why it is token-efficient](#why-it-is-token-efficient)
- [Quick start](#quick-start)
- [VS Code / Copilot setup](#vs-code--copilot-setup)
- [Claude Desktop setup](#claude-desktop-setup)
- [Example asks](#example-asks)
- [Deterministic coaching analytics](#deterministic-coaching-analytics)
- [Observability & Metrics](#observability--metrics)
- [Runtime configuration](#runtime-configuration)
- [Development](#development)
- [Docker and remote deployment](#docker-and-remote-deployment)
- [License](#license)
- [Disclaimer](#disclaimer)

## Why this project exists

The original foundation of this project was strong: build tool behavior dynamically from the live Intervals.icu OpenAPI spec so the MCP server does not drift as the upstream API evolves.

That solved the **maintenance** problem.

It did **not** solve the **agent UX** problem.

Exposing one tool per API endpoint creates the exact failure mode modern MCP design tries to avoid:

- too many tools loaded into context
- too much low-level API detail exposed to the model
- too many multi-step orchestration burdens pushed onto the LLM
- more chances for bad tool selection, invalid arguments, and wasted tokens

This project now takes a different approach:

- keep the **dynamic OpenAPI layer** internally, where it belongs
- expose a **capability-level intent surface** to the LLM
- return **structured, guidance-driven outputs** instead of raw payloads
- compute important coaching metrics on the server, not in the model's head

In other words: **dynamic under the hood, curated at the boundary**.

## What makes it different

### 1. Intent-driven public interface

The LLM sees **9 high-level intents** such as `analyze_training` or `modify_training`, not dozens of endpoint-shaped tools.

### 2. Dynamic OpenAPI runtime retained internally

This is not a hand-maintained wrapper that goes stale. The server still loads the Intervals.icu OpenAPI spec dynamically and uses it as the execution layer behind the intent orchestration.

Those endpoint operations are private implementation capabilities: they are never returned
by MCP `tools/list` and cannot be invoked by their operation ids through `tools/call`.

### 3. Token-efficiency by default

Responses are designed for LLMs:

- structured and compact
- pre-filtered
- pre-aggregated
- guidance-rich

### 4. Deterministic coaching analytics

Read-only coaching intents use a deterministic pipeline to compute metrics from a growing library of engines:

**Progress Tracking:** CTL plateau detection via changepoint analysis with athlete-aware flat-band estimation, TID drift (Shannon entropy deltas over rolling 4-week windows), lnRMSSD rollup with trend slope and coefficient of variation, and evidence-weighted coaching hypotheses (volume, intensity distribution, recovery) with confidence ranking and recommended interventions.

**Performance Intelligence:** eFTP/W′/pMax extraction (ESPE), W′ depletion (WDRM), signed aerobic decoupling (ISDM) with durability state, Z2 HR stability.

**Coaching Intelligence:** Neural Density Load Index (NDLI) with green/amber/red thresholds, adaptation state & curve profile classification, HRV multi-domain (ratio, trend slope, recovery quality index), race readiness scoring (5-factor model).

**Ultra-Sport Extensions:** Heat stress context, terrain context (terrain index, VAM), nutrition demand model (carb/protein), training intensity distribution (TID) classifier.

**Strategic Capabilities:** Banister TSB forecast with fatigue class projection, power curve 2-window comparison (delta per anchor, rotation index, system status), ultra-specific context tokens (back-to-back load, vert/week, longest-run ratio, elevation specificity), adaptive decision engine (ADE) with operational state and risk level.

**Load management:** ACWR context, monotony, strain, fatigue index, stress tolerance, durability index, recovery interpretation, stream-derived execution signals.

### Evidence-Qualified Coaching Metrics

All coaching metrics include provenance disclosure and use neutral statistical positions rather than medical or readiness claims.

**Training Load Provenance:**
- Load scores are extracted from `icu_training_load` → `training_load`/`icuTrainingLoad` → `tss` (in priority order)
- Activities without API load data are excluded from totals and explicitly disclosed
- `moving_time` is never used as a load proxy
- Coverage percentage (loaded/total activities) is rendered in all affected responses

**HRV/RHR Personal Baselines:**
- HRV is ln-transformed (lnRMSSD) before baseline calculation
- Baseline uses a 60-day reference window; recent value uses a 7-day window
- Smallest Worthwhile Change (SWC) = mean × CV × 0.5
- Positions are neutral: Below / Within / Above personal range
- Insufficient data states exactly which requirement failed (recent samples, baseline samples, or date span)

The separate recovery index uses recent HRV and RHR relative to their historical averages when both are available; otherwise it displays the raw HRV/RHR ratio. The raw ratio is not compared with baseline-relative thresholds and is not used to calculate fatigue index. These heuristic indices are not diagnoses. Period analysis currently does not populate wellness for its load-management calculation, so its fatigue index is unavailable even when wellness history exists.

**Critical Power Diagnostics:**
- Reports sample count, duration coverage, R², RMSE, CP standard error, and W′ standard error
- "Valid" means mathematically identifiable and finite; it does not imply physiological validation
- No arbitrary R² threshold is used to reject or accept a fit

**Deferrals (not implemented):**
- Submaximal response classification
- Load-response classifier
- W′ recovery kinetics
- Metabolic score
- HR-TRIMP
- New readiness score
- ETVS remains a separate metric

### 5. Safer mutation flows

Mutating intents are designed for agents:

- business identifiers instead of opaque system-first flows
- `dry_run` previews for risky changes
- `idempotency_token` replay protection for `plan_training` and `modify_training` writes only, not previews or profile and gear updates

When a mutation is cancelled, panics, loses its response, or returns an error after it may have reached Intervals.icu, its outcome is uncertain. Retries with the same idempotency token are blocked rather than dispatched again. Local validation for `plan_training` and `modify_training` runs before the journal reserves an operation.

The standard HTTP and STDIO server modes use a local SQLite journal. The operation key includes the normalized upstream base URL, athlete ID, and idempotency token. Older SQLite entries without upstream and athlete scope block the same key without replaying their result for as long as the old entry is retained. Changing the configured athlete or upstream does not make those keys safe to repeat. The journal commits a reservation before invoking the mutation handler. It stores hashes rather than raw operation tokens, fingerprints, or owners. An uncertain operation remains in the database indefinitely; process restarts do not make it retryable. A confirmed result is encrypted and retained for 24 hours, then the same token can be treated as a new operation. Do not reuse tokens for separate user actions.

The journal currently enforces fixed capacity limits: 16 active operations and 128 unknown outcomes per tenant, up to 2,048 active or unexpired completed records per tenant, and 1,024 active or unknown records globally. When a limit is reached, a new operation is rejected before its handler runs; replay of an existing completed result remains available. Unknown records are never evicted to free space.

In authenticated HTTP mode, only the JWT owner recorded with the operation can replay its saved result. The owner is the JWT `jti`, or a digest of the exact JWT for legacy tokens without a `jti`. Reissuing a JWT creates a different owner: it cannot replay the previous result, and the same operation key remains blocked if its outcome is uncertain. Rotating an upstream API key without changing the JWT owner does not change operation ownership. A saved result is not an authorization grant to a different JWT.

This guarantee applies to the standard single-process server using one persistent local database. It does not provide cross-process or multi-host coordination. Handlers or runtimes assembled by library users without the standard startup wiring may use non-durable idempotency protection. The database, SQLite WAL/SHM sidecars, and backups contain unencrypted metadata and encrypted saved results; the token registry's upstream API keys retain their existing AES-GCM encryption. Keep the database and backups private, and preserve `MCP_STATE_ENCRYPTION_KEY` across restarts so saved results remain readable. Unknown outcomes cannot be cleared automatically. There is currently no supported operator tool to resolve them; investigate Intervals.icu out of band before taking any separate action, and do not delete journal records to make a retry pass.

If you embedded the old file-backed JSON idempotency cache, keep it in place until you have checked outstanding operations upstream. There is no automatic cutover from that cache to SQLite: pointing the server at the JSON file fails startup and leaves the file unchanged. Do not point the server at a fresh database and resume automatic retries from the old cache. The JSON token registry is not imported either: unset the obsolete `MCP_TOKEN_REGISTRY_PATH` before starting either transport, use a separate SQLite file, and reissue registry-backed JWTs. Older self-contained JWTs carrying the encrypted credential may still work until expiry with the same master key; this fallback does not carry over JSON revocations.

### 6. Rust-first operational profile

- single binary
- fast startup
- strong type safety
- good fit for local MCP, containers, and remote HTTP deployments

## Public MCP surface

The public MCP contract is intentionally small and stable.

### Intents

| Intent | Purpose | Mutating | Example ask |
|---|---|---|---:|---|
| `plan_training` | Create training plans across any horizon | ✅ | “Build me a 12-week 50K plan” |
| `analyze_training` | Analyze a single workout or a training period | ❌ | “Analyze yesterday’s workout” |
| `modify_training` | Move, edit, create, or delete workouts and events | ✅ | “Move Saturday’s workout to Sunday” |
| `compare_periods` | Compare two blocks of training | ❌ | “Compare this month vs last month” |
| `assess_recovery` | Assess readiness, recovery, and red flags | ❌ | “Am I ready for intensity tomorrow?” |
| `manage_profile` | View or update thresholds, zones, and profile settings | ✅ | “Update my threshold values from a lab test” |
| `manage_gear` | List, add, or retire gear | ✅ | “How much mileage is on my shoes?” |
| `analyze_race` | Post-race analysis and follow-up guidance | ❌ | “How did my 50K go?” |
| `track_progress` | Detect plateaus, surface TID drift, and rank coaching hypotheses | ❌ | “Why have I stopped improving?” |

### Resource

| Resource | Purpose |
|---|---|
| `intervals-icu://athlete/profile` | Ongoing athlete context including profile and fitness-related information |

### Public contract rules

- **Names are outcome-oriented**, not endpoint-oriented.
- **Arguments are flattened** so agents do not have to invent nested structures.
- **Successful intent results use structured MCP output** via `structuredContent`.
- **Intent tool calls avoid duplicating the same payload into text `content`**, reducing token waste.
- **Error and partial states are guidance-driven**, so the model is told what to do next.

## Architecture at a glance

```text
LLM Host (VS Code / Claude / Cursor / other MCP client)
            |
            | calls one high-level intent
            v
  +-----------------------------+
  | Intent Layer                |
  | 9 public coaching intents   |
  +-----------------------------+
            |
            v
  +-----------------------------+
  | Intent Router               |
  | validation + idempotency    |
  | orchestration + rendering   |
  +-----------------------------+
            |
            v
  +-----------------------------+
  | Internal Execution Layer    |
  | dynamic OpenAPI runtime     |
  | Intervals client            |
  +-----------------------------+
            |
            v
        Intervals.icu API
```

### Layering philosophy

This README describes the project as a **capability-level MCP server**:

- the LLM interacts with **goals**
- the server handles **orchestration**
- the OpenAPI runtime remains the **internal product/component layer**

That separation is the core design decision behind the current architecture.

## Why it is token-efficient

This server is designed to reduce both **static tool metadata cost** and **dynamic response cost**.

### Smaller tool surface

Instead of flooding the model with endpoint-shaped tools, the server exposes only the intent surface that matters most in real coaching workflows.

### Compact outputs

Responses are shaped for actionability:

- summaries before detail
- decision-ready metrics before raw JSON
- markdown tables and structured content instead of schema dumps
- selective enrichment only when it changes the decision

### Server-side computation

The server computes important metrics and interpretations itself, including portions of:

- readiness context
- fatigue and load guidance (Fatigue Index, Stress Tolerance, Durability Index)
- stream-aware execution signals
- training period summaries

This keeps the model focused on reasoning with the result rather than reconstructing the result.

### Guidance-driven follow-up

Intent responses include `suggestions` and `next_actions` so the host model knows how to continue without trial-and-error tool calling.

## Quick start

### Prerequisites

- **Rust stable** with Cargo, or
- **Docker**

### Get your Intervals.icu credentials

1. Open <https://intervals.icu/settings>
2. Scroll to the **Developer** section
3. Create an API key
4. Copy your API key
5. Note your athlete ID from your profile URL (format: `i123456`)

### Install and run the MCP server

The server supports both **STDIO** (for local MCP clients) and **HTTP** (for remote clients) transport modes via the `MCP_TRANSPORT` environment variable.

```sh
git clone https://github.com/like-a-freedom/rusty-intervals-mcp.git
cd rusty-intervals-mcp

cargo install --locked --path crates/intervals_icu_mcp
```

The binary reads its process environment; it does not automatically load a `.env` file. Use the settings below for direct startup.

#### Configure persistent local state

Both transports need a SQLite path on local disk and a stable encryption key. Create the directory and generate the key file once; keep the directory private and back up the key with the database.

```sh
umask 077
mkdir -p "$HOME/.local/share/intervals-icu-mcp"
chmod 700 "$HOME/.local/share/intervals-icu-mcp"
export MCP_STATE_DB_PATH="$HOME/.local/share/intervals-icu-mcp/state.sqlite3"
# On an existing installation, restore a missing key from backup instead of replacing it.
if [ ! -s "$MCP_STATE_DB_PATH.key" ] && [ ! -e "$MCP_STATE_DB_PATH" ]; then
  openssl rand -hex 32 > "$MCP_STATE_DB_PATH.key"
fi
export MCP_STATE_ENCRYPTION_KEY="$(cat "$MCP_STATE_DB_PATH.key")"
```

#### STDIO mode (default)

For local MCP clients like VS Code Copilot or Claude Desktop, pass both state settings and the credentials in the server process environment:

```sh
export INTERVALS_ICU_API_KEY=your_api_key_here
export INTERVALS_ICU_ATHLETE_ID=i123456
export MCP_TRANSPORT=stdio  # optional: stdio is the default
intervals_icu_mcp
```

#### HTTP mode

For remote MCP clients or when running as a service, keep both state settings above in the server process environment. Generate a JWT master key once with `openssl rand -hex 64`, store it privately, and set `JWT_MASTER_KEY` to that same value on each start. HTTP mode does not need an athlete ID or API key in the server environment.

```sh
# JWT_MASTER_KEY and both SQLite state settings must already be exported.
export MCP_TRANSPORT=http
export MCP_HTTP_ADDRESS=127.0.0.1:3000  # optional: default is 127.0.0.1:3000

export REQUEST_TIMEOUT_SECONDS=30        # optional: deadline to produce an HTTP response
export IDLE_TIMEOUT_SECONDS=60           # optional: time before the first TCP keepalive probe
intervals_icu_mcp
```

The MCP endpoint is available at `http://<address>/mcp`.

#### Authenticating with `/auth`

Exchange your Intervals.icu API key for a JWT:

```sh
curl -s -X POST http://127.0.0.1:3000/auth \
  -H "Content-Type: application/json" \
  -d '{"api_key": "your_api_key_here", "athlete_id": "i123456"}'
```

Response:

```json
{
  "token": "<jwt>",
  "expires_in": 7776000,
  "athlete_id": "i123456"
}
```

Configure your Streamable HTTP MCP client with the `/mcp` URL and the header `Authorization: Bearer <jwt>` using the returned token. The client initializes the MCP session before making tool calls. The API-key exchange is not OAuth; the client must support supplying the bearer JWT (or use a compatible intermediary).

Current HTTP security/runtime notes:
- `/auth` is rate-limited separately for brute-force protection (1 req/sec, burst size 3).
- `/mcp` authenticates first, then rate-limits by athlete. Missing or rejected JWTs do not reach the limiter. See [HTTP rate limiting](docs/HTTP_RELIABILITY.md#rate-limiting) for the refill intervals and separate peer-IP buckets.
- TCP keepalive is enabled on the server socket using `IDLE_TIMEOUT_SECONDS` as the keepalive idle time.
- `JWT_MASTER_KEY` supplies separate derived keys for JWT signing and upstream credential encryption. Keep it stable across restarts.
- Container deployments are intended for **Streamable HTTP MCP**. STDIO is for local child-process integrations.

### Token lifetime
- `/auth`: defaults to 90 days (7,776,000 seconds), configurable via `JWT_TTL_SECONDS`.
- Browser UI: defaults to 30 days; the form allows 1–3,650 days.

## Token Management Web UI

The server includes a minimal browser-based token management UI at `/ui` when running in HTTP mode. It is useful for ad‑hoc token creation without `curl`.

| Endpoint | Method | Description |
|---|---|---|
| `/ui` | GET | Token creation form |
| `/ui/token` | POST | Create a JWT token (form body: `athlete_id`, `api_key`) |
| `/ui/tokens` | GET | List issued tokens (optional `?athlete=` filter; `?sort=`, `?order=`) |
| `/ui/revoke/{jti}` | POST | Revoke a token by its JTI |

Endpoint paths above assume the default root deployment. With `MCP_PUBLIC_BASE_PATH=/intervals` they live under `/intervals/ui…`, the session cookie is scoped to `Path=/intervals/ui`, and all generated links/redirects carry the prefix.

The UI is server-rendered and uses the same SQLite-backed token registry as `/auth`. Its peer-IP bucket replenishes one request every two seconds (burst 5). Session cookies are `Secure` by default; set `MCP_COOKIE_SECURE=false` only for local plain-HTTP testing.

**Restrict access to `/ui` and its subpaths** at the proxy or on a private network. The UI has no operator login of its own. The browser session provides CSRF protection for forms, not authorization to list or revoke records.

**Revocation scope:** every token the server issues — via `/auth` or the UI — is recorded in the server-side **token registry**, keyed by its `jti`, and can be revoked. The registry is the source of truth for revocation and for the athlete's upstream credential (stored encrypted at rest; the credential never travels inside the JWT). The token list shows the whole registry, filterable by athlete with `?athlete=`; the browser session only supplies a default filter. Token records live in the SQLite database configured by `MCP_STATE_DB_PATH`.

Rust embedders can use `AppState::rotate_upstream_api_key` to replace the encrypted upstream credential on an existing, non-revoked JWT without changing its owner or expiration. Validate the replacement key for the token's athlete before calling. New requests authenticated with that JWT see the new key; requests already authenticated can still use the old one. The standard HTTP and MCP interfaces do not expose this method.

## VS Code / Copilot setup

For local development, the simplest VS Code MCP configuration is:

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "cargo",
      "args": [
        "run",
        "--manifest-path",
        "/absolute/path/to/rusty_intervals_mcp/Cargo.toml",
        "-p",
        "intervals_icu_mcp",
        "--bin",
        "intervals_icu_mcp"
      ],
      "env": {
        "INTERVALS_ICU_API_KEY": "your_api_key_here",
        "INTERVALS_ICU_ATHLETE_ID": "i123456",
        "MCP_STATE_DB_PATH": "/absolute/private/path/state.sqlite3",
        "MCP_STATE_ENCRYPTION_KEY": "replace_with_your_existing_64_hex_character_key"
      }
    }
  }
}
```

After restarting VS Code, try asking:

```text
@intervals-icu Analyze yesterday's workout
@intervals-icu Build me a 12-week trail ultra plan
@intervals-icu How is my recovery this week?
```

## Claude Desktop setup

Add a local MCP entry pointing at the stdio binary:

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "/ABSOLUTE/PATH/TO/intervals_icu_mcp",
      "env": {
        "INTERVALS_ICU_API_KEY": "your_api_key_here",
        "INTERVALS_ICU_ATHLETE_ID": "i123456",
        "MCP_STATE_DB_PATH": "/absolute/private/path/state.sqlite3",
        "MCP_STATE_ENCRYPTION_KEY": "replace_with_your_existing_64_hex_character_key"
      }
    }
  }
}
```

Keep the API key and state encryption key out of versioned client configuration. A launcher script can read them from a private environment file or secret manager before starting the server.

## Example asks

The best way to use this MCP server is to ask for **outcomes**, not API mechanics.

### Planning

- “Create a 10-week build toward a half marathon”
- “Plan next week around four available training days”
- “Build a recovery week after my race”

### Workout and period analysis

- “Analyze yesterday’s threshold workout”
- “Summarize my training for February”
- “Show interval insights from my Tuesday session”
- “What workouts are planned later this week?”
- “Show ETVS for yesterday’s workout”
- “Compare ETVS between these two training blocks”

`ETVS` is an additive zone-weighted volume score reported in `weighted min`.
It uses the versioned `intervals_icu_zones_linear_1_5_cap_v1` model: configured
Intervals.icu zones Z1–Z4 receive weights 1–4, while Z5–Z7 share weight 5.
Responses include zone-data coverage. This product metric is not labeled Edwards
TRIMP because Intervals.icu zone provenance does not guarantee Edwards’ original
five HRmax zones, and it is not a replacement for training load/TSS.

### Recovery and performance management

- “How is my recovery looking over the last 7 days?”
- “Am I ready for intensity tomorrow?”
- “Compare this month with last month” — `compare_periods` supports arbitrary like-for-like windows, including YoY and quarter-over-quarter comparisons
- “Why have I stopped improving?” — `track_progress` detects trailing plateau over 4–24 weeks, surfaces load context, TID drift, HRV trend, and ranks possible root causes
- “Is my training volume stalled?” — `track_progress` identifies plateau onset date, duration, and trailing slope, with recommendations for gradual volume increase
- “Analyze my last workout” — `analyze_training` with `target_type: "single"` for one workout; use `target_type: "period"` with `period_start` and `period_end` for an explicit period

Historical period fetches are uncapped at the MCP layer. Detail-derived metrics (HR, zones, TSS, load) may report partial availability if upstream detail calls fail for some activities.

### Calendar changes

- “Move Saturday’s long run to Sunday”
- “Create a 45-minute recovery run for Wednesday”
- “Preview deleting workouts next week before applying it”

### Profile and gear

- “Show my current running thresholds”
- “Update my profile using my new lab thresholds”
- “How much life is left in my shoes?”

## Deterministic coaching analytics

The read-only coaching layer is intentionally **deterministic**.

It follows this pipeline:

```text
Fetch → Audit → Compute → Interpret → Render
```

### What this means in practice

- the server gathers relevant activities, wellness, events, and profile context
- it checks data availability and degraded-data scenarios
- it computes metrics and summaries in Rust
- it interprets those metrics with explicit rules
- it renders compact, guidance-rich output for the host model

### Where it shows up most strongly

#### `track_progress`

- CTL plateau detection from wellness history using linear regression slope across trailing 28-day window with backward step search
- athlete-aware flat-band personalisation when 8+ weeks of CTL history exist
- load context: ACWR ratio and state (underloaded/productive/watch/overreaching), monotony, strain
- HRV context: baseline-relative ratio, trend state (suppressed/within_range), lnRMSSD 7-day rollup with mean, CV, and trend slope
- TID drift analysis: weekly 3-zone distributions grouped by ISO week, Shannon entropy delta between the latest two adjacent 4-week windows, drift classification (stable/converging/polarizing), dominant zone identification; requires zone data in each of the 8 weeks
- evidence-weighted coaching hypotheses: volume hypothesis with ACWR and monotony signals, intensity distribution hypothesis with TID drift and monotony, recovery hypothesis with HRV suppression and ACWR state
- explicit warnings when data is insufficient for specific sub-analyses
- trailing plateau detection over 4–24 weeks; directs YoY analysis to `compare_periods`

#### `compare_periods`

- arbitrary like-for-like window comparisons including YoY and quarter-over-quarter
- uncapped historical activity fetches at the MCP layer
- detail-derived metrics (HR, zones, TSS, load) may report partial availability if upstream detail calls fail
- requested metrics rendered per-period with delta summaries

Detailed period analysis may show Endurance Performance Evidence for cycling power data. It compares matched, steady 10-minute HR–power windows and, when a qualifying long ride exists, matched early and late windows. It reports observations, physiological context (bilingual explanation of what the signal may indicate), and data eligibility; it is not a readiness, recovery, or durability score. Summary analysis does not fetch historical streams for this section. Expect an additional 3–5 seconds of API latency for historical stream retrieval in detailed mode. See `docs/METRIC_METHODS.md` for the protocol and explicit non-goals.

#### `analyze_training`

- single-workout deep dives: ESPE anchors (eFTP, W′, pMax), WDRM, ISDM with durability state, Z2 HR stability, terrain context (index, VAM), nutrition demand (carb/protein), curve profile classification (endurance/punchy/speed)
- one workout or one explicit period; period calls use `period_start` and `period_end`
- period analysis: heat stress context, TID model (pyramidal/threshold/polarized), NDLI (green/amber/red), power curve comparison (2-window deltas with rotation index), ultra-specific tokens (back-to-back load, vert/week), load management (ACWR, monotony, strain)
- interval-aware, stream-aware, and histogram analysis modes
- planned workout and calendar event visibility in period windows
- explicit data-availability reporting

#### `assess_recovery`

- readiness framing with activity-specific verdict (easy/intensity/long/race)
- ADE system state assessment (LoadAccepting/RecoveryPriority with risk level and active flags: maladaptation risk, functional overreach, load pressure, loaded taper)
- personal-baseline-aware HRV multi-domain interpretation (ratio, trend slope, recovery quality index)
- recovery-first guidance and red-flag detection

#### `analyze_race`

- post-race execution review with efficiency factor and aerobic decoupling
- 5-factor race readiness scoring (score/100 with tier: ready/monitor/caution/not_ready)
- performance, strategy, and recovery analysis modes
- comparison-to-plan behavior when a matching calendar event exists

### Why deterministic matters

- **repeatable** — same input, same output
- **testable** — rules can be unit and integration tested
- **explainable** — alerts have evidence
- **token-efficient** — the model receives interpretations, not just raw numbers

## Observability & Metrics

In HTTP mode, the server exposes Prometheus-format metrics at `/metrics`. These provide visibility into upstream API health, MCP protocol usage, auth lifecycle, and active athlete tracking.

### Endpoint

`GET /metrics` — returns metrics in Prometheus text format.

Optional authentication via `PROMETHEUS_METRICS_TOKEN` env var:
- Set a non-empty value to protect the endpoint and send `Authorization: Bearer <token>` when scraping. This is a separate secret, not an MCP JWT.
- If unset or empty: the endpoint is not protected by a scrape secret.

### Key metrics groups

| Group | Example metrics |
|-------|-----------------|
| **Upstream API** | `intervals_icu_client_upstream_requests_total{status}`, `intervals_icu_client_upstream_errors_total{error_type}`, `intervals_icu_client_upstream_request_duration_seconds` |
| **MCP Protocol** | `intervals_icu_mcp_tool_calls_total{tool,status}`, `intervals_icu_mcp_tool_duration_seconds{tool}`, `intervals_icu_mcp_mcp_method_calls_total{method}` |
| **HTTP Transport** | `intervals_icu_mcp_http_requests_total{method,path,status}`, `intervals_icu_mcp_http_request_duration_seconds{method,path}`, `intervals_icu_mcp_active_requests` |
| **Auth & Security** | `intervals_icu_mcp_tokens_issued_total{source}`, `intervals_icu_mcp_token_verifications_total{status}`, `intervals_icu_mcp_auth_failures_total{reason}` |
| **Active Usage** | `intervals_icu_mcp_active_athletes` (gauge, no athlete ID label) |
| **Mutation Journal** | `intervals_icu_mcp_operation_journal_errors_total`, `intervals_icu_mcp_operation_journal_quota_rejections_total{quota}`, `intervals_icu_mcp_operation_journal_unknown_operations`, `intervals_icu_mcp_operation_journal_oldest_unknown_age_seconds` |

The journal metrics count storage errors and rejected reservations, and expose the total number and oldest age of unresolved unknown outcomes. The `quota` label uses only fixed values (`active_per_tenant`, `unknown_per_tenant`, `unresolved_global`, `completed_per_tenant`); tenant, token and operation identifiers are never metric labels. The unknown count and oldest timestamp are refreshed from SQLite at startup and after an operation becomes unknown. Each scrape calculates the age from that timestamp, so the gauge keeps advancing when the server is idle.

### Example Prometheus queries

```promql
# Recorded upstream errors per second, by type
sum by (error_type) (rate(intervals_icu_client_upstream_errors_total[5m]))

# p95 tool latency, by tool
histogram_quantile(0.95, sum by (le, tool) (rate(intervals_icu_mcp_tool_duration_seconds_bucket[5m])))

# Active athletes
intervals_icu_mcp_active_athletes
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PROMETHEUS_METRICS_TOKEN` | unset | Optional Bearer token for `/metrics` endpoint |

### Grafana dashboard

A prebuilt Grafana dashboard for these metrics is available at:

- [Grafana dashboard JSON](examples/grafana/intervals_icu_mcp_observability_dashboard.json)

Import it into Grafana, bind the `DS_PROMETHEUS` datasource input, and use the built-in variables (`Job`, `Instance`, `Path`, `Tool`) to drill from service-wide health down to specific routes or MCP tools.

The example dashboard needs query updates for the current upstream metric prefix (`intervals_icu_client_`) and rate-limit label (`scope`, not `endpoint`). It does not include the mutation journal gauges. For protected `/metrics` endpoints, configure your Prometheus scrape job with the same bearer token value as `PROMETHEUS_METRICS_TOKEN`.

### vmagent scrape example

An example VictoriaMetrics `vmagent` scrape configuration is available at:

- [vmagent scrape YAML](examples/vmagent/intervals_icu_mcp_metrics.yml)

The example includes both:

- a public `/metrics` target
- a protected `/metrics` target using `authorization.credentials_file`

Use the job that matches your deployment. If the server is mounted under a prefix, set `metrics_path` to the prefixed URL path. For remote production scrapes, use TLS and prefer `credentials_file` over inline bearer tokens.

## Architecture & Reliability

- [Runtime architecture](docs/ARCHITECTURE.md) — layering, transports, and middleware
- [HTTP reliability](docs/HTTP_RELIABILITY.md) — rate limiting, keepalive, graceful shutdown, and 429 recovery

## Runtime configuration

Use the tables below and [persistent state setup](#configure-persistent-local-state) for current environment settings. The repository's `.env.example` has legacy JSON persistence comments and does not list the required SQLite settings.

In multi-tenant HTTP mode, typed and dynamic upstream calls share connections
and a circuit breaker per base URL and athlete. The transport cache holds up to
128 entries and no user API keys; credentials are resolved for each request.
Active transports and runtimes are not evicted. An idle open breaker becomes
eligible for eviction after its cooldown; eviction resets its state, including
any failures below the opening threshold. If no cache entry is eligible for
eviction, a request needing a new entry fails rather than creating an independent
transport. HTTP 5xx responses count as breaker failures; responses below 500
reset the consecutive failure count. Manually assembled `DynamicRuntime` instances and handlers built around a custom `IntervalsClient` do not automatically get this shared transport.

When dynamic dispatch is enabled, mapped calls use the request's credentials
and base URL. The OpenAPI registry is cached separately per base URL (up to 128
entries), and concurrent cold loads for a cached runtime share one fetch. After
a failed cold load, retries are suppressed for one second and mapped calls fall
back to the typed client.

### Required environment variables

| Variable | Description |
|---|---|
| `MCP_STATE_DB_PATH` | Both modes: path to a writable local SQLite file for persistent state. Use one server process per database. |
| `MCP_STATE_ENCRYPTION_KEY` | Both modes: stable 32-byte key as 64 hex characters for encrypting saved operation results. |
| `INTERVALS_ICU_API_KEY` | STDIO only: Intervals.icu API key. |
| `INTERVALS_ICU_ATHLETE_ID` | STDIO only: athlete ID such as `i123456`. |
| `JWT_MASTER_KEY` | HTTP only: stable 64-byte key as 128 hex characters for JWT signing and upstream API-key encryption. |

### Common optional environment variables

| Variable | Default | Description |
|---|---|---|
| `INTERVALS_ICU_BASE_URL` | `https://intervals.icu` | Base URL for the upstream API |
| `INTERVALS_ICU_OPENAPI_SPEC` | unset | Explicit OpenAPI source (HTTP(S) URL or local file) |
| `INTERVALS_ICU_SPEC_REFRESH_SECS` | `300` | Refresh cadence for the cached OpenAPI runtime |
| `INTERVALS_ICU_DYNAMIC_DISPATCH_ENABLED` | unset | When set to `1`/`true`, wraps the typed upstream client in `DynamicClientAdapter` so mapped `IntervalsClient` methods can dispatch through the live OpenAPI runtime. The adapter-only `get_athlete_training_plan` method is not called by a production handler. See ADR-0001. Default off — no behaviour change. |
| `RUST_LOG` | `info` | Log level filter (standard `RUST_LOG` directives); noisy internals pinned to `warn` |
| `RUST_LOG_FORMAT` | text | Set to `json` for structured one-JSON-object-per-line records; anything else gives dense single-line text with UTC millisecond timestamps |
| `RUST_LOG_COLOR` | auto | ANSI colors in text mode: `always`/`never` (or `1`/`0`); unset follows stderr TTY detection |
| `MCP_TRANSPORT` | `stdio` | Transport mode: `stdio` or `http` |
| `MCP_HTTP_ADDRESS` | `127.0.0.1:3000` | Listen address for HTTP mode |
| `MAX_HTTP_BODY_SIZE` | `4194304` | Axum extractor limit on the `/mcp` router; does not change rmcp's separate 4 MiB POST limit |
| `REQUEST_TIMEOUT_SECONDS` | `30` | Time to produce a response on `/auth` and `/mcp`; does not limit an open response stream |
| `IDLE_TIMEOUT_SECONDS` | `60` | TCP idle time before the first keepalive probe |
| `MCP_RATE_LIMIT_PER_SECOND` | `5` | Despite the name, seconds to replenish one request per athlete at `/mcp` |
| `MCP_RATE_LIMIT_BURST` | `15` | Per-athlete burst capacity for `/mcp` |
| `JWT_TTL_SECONDS` | `7776000` | Lifetime for JWTs issued by `/auth`, in seconds |
| `MCP_COOKIE_SECURE` | `true` | Marks UI session cookies `Secure`; `false` or `0` disables it for local HTTP |
| `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | Allowed `Host` names for rmcp at `/mcp`, not a trusted-proxy setting |
| `MCP_PUBLIC_BASE_PATH` | unset | Public path prefix for reverse-proxy pass-through (e.g. `/intervals`); empty = serve at the domain root. See [Path prefix](#path-prefix-multiple-services-per-domain) |

### OpenAPI runtime behavior

If `INTERVALS_ICU_OPENAPI_SPEC` is **unset**, the runtime:

1. fetches `${INTERVALS_ICU_BASE_URL}/api/v1/docs`
2. builds the internal registry dynamically
3. keeps a cached version in memory
4. falls back to a local `docs/intervals_icu_api.json` at the build-time source-tree path if remote loading fails and that file exists; it is not tracked, embedded, or bundled in the container images

If `INTERVALS_ICU_OPENAPI_SPEC` is **set explicitly**, that source becomes authoritative and failures are surfaced instead of silently switching to a different spec.

This gives the project the best of both worlds:

- **live compatibility** with the upstream API
- **local spec override** for development, testing, or deployments that mount a spec file

## Development

This repository is a Cargo workspace with two main crates:

| Path | Purpose |
|---|---|
| `crates/intervals_icu_client` | HTTP client, retries, observability, API compatibility helpers |
| `crates/intervals_icu_mcp` | MCP server, intent layer, dynamic runtime, resources, and tests |

### Recommended verification commands

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

### Useful development commands

```sh
# Client examples
cargo run -p intervals_icu_client --example basic_client
cargo run -p intervals_icu_client --example list_recent_activities

# MCP stdio mode (default)
cargo run -p intervals_icu_mcp

# MCP http mode
MCP_TRANSPORT=http cargo run -p intervals_icu_mcp
```

### Testing notes

The codebase includes:

- unit tests around intent handlers and runtime behavior
- mocked HTTP tests for OpenAPI and MCP behavior
- integration tests across the workspace
- ignored live contract checks for selected upstream compatibility cases

## Docker and remote deployment

Docker packaging in this repository is for the **HTTP streamable MCP** transport. For local STDIO clients such as VS Code or Claude Desktop, run the binary directly instead of containerizing it.

This repo keeps two Dockerfiles on purpose:

- `Dockerfile` — used for local development and `docker compose`; it builds the Rust binary inside the image.
- `Dockerfile.release` — used by the GitHub release workflow; it packages prebuilt Linux binaries into a slim runtime image.

### Build the container

```sh
docker build -t rusty-intervals:latest .
```

This uses the default `Dockerfile` and compiles the project inside the builder stage.

### Run locally with environment variables

#### HTTP mode

The server requires a persistent local SQLite database and runs as a non-root user. Create a private bind-mount directory and run the container with your host UID/GID so it can write there:

```sh
mkdir -p ./mcp-state
chmod 700 ./mcp-state
export MCP_UID="$(id -u)"
export MCP_GID="$(id -g)"
```

Generate `JWT_MASTER_KEY` and `MCP_STATE_ENCRYPTION_KEY` once, store them in a secret manager, and export the same values on every restart. Do not regenerate them when restarting: `JWT_MASTER_KEY` signs JWTs and encrypts stored upstream credentials; `MCP_STATE_ENCRYPTION_KEY` encrypts saved operation results.

```sh
docker run --rm \
  --user "$MCP_UID:$MCP_GID" \
  --mount "type=bind,source=$PWD/mcp-state,target=/var/lib/intervals-mcp" \
  -p 127.0.0.1:3000:3000 \
  -e MCP_TRANSPORT=http \
  -e MCP_HTTP_ADDRESS=0.0.0.0:3000 \
  -e JWT_MASTER_KEY \
  -e MCP_STATE_DB_PATH=/var/lib/intervals-mcp/state.sqlite3 \
  -e MCP_STATE_ENCRYPTION_KEY \
  rusty-intervals:latest
```

The container exposes:

- `GET /health` for liveness checks
- `POST /auth` to exchange Intervals.icu credentials for a JWT
- streamable MCP at `/mcp`
- `GET /metrics` for Prometheus metrics (HTTP mode only)
- `/ui` for token administration (restrict operator access)

### Docker Compose

`docker-compose.yml` is preconfigured for HTTP mode and builds from the default `Dockerfile`. It binds `./data` to `/data` for the SQLite database and maps the container process to `MCP_UID:MCP_GID` so it can write to that directory. Set these to the host user's IDs (for example, `export MCP_UID="$(id -u)"` and `export MCP_GID="$(id -g)"`), and provide the required secrets in your shell or Compose `.env` file. Keep both keys stable across restarts and backups. Use local persistent storage, not a shared filesystem or a network mount.

Create the host directory before starting Compose. It must be writable by the UID/GID configured for the container; if another user owns an existing directory, correct its ownership before starting.

```sh
mkdir -p ./data
chmod 700 ./data
docker compose up -d
```

At minimum, set these variables before launch:

- `JWT_MASTER_KEY`
- `MCP_STATE_ENCRYPTION_KEY`

The database path defaults to `/data/state.sqlite3`. Back up the database using SQLite's backup mechanism, or stop the server and copy the database together with any `-wal` and `-shm` sidecars. Preserve both encryption/signing keys with the backup. A restore must use the same `JWT_MASTER_KEY` and `MCP_STATE_ENCRYPTION_KEY`.

### Remote MCP clients

The [remote client JSON](examples/mcp_remote.json) illustrates the endpoint URL only. It lacks the required `Authorization: Bearer <jwt>` header. Add that header using your host's Streamable HTTP configuration syntax and keep the JWT out of version control.

### Release image packaging

The GitHub release workflow builds the multi-arch container image from `Dockerfile.release` after compiling Linux `amd64` and `arm64` binaries in CI. That Dockerfile is meant for release packaging, not for a standalone local `docker build`.

For production deployment:

- place the server behind TLS
- restrict `/ui` and its subpaths to operators at the proxy or gateway; MCP bearer authentication does not protect the UI
- keep liveness and scrape endpoints private when possible, and configure a separate scrape secret if needed
- do not publish the backend directly to the public internet

### Path prefix (multiple services per domain)

To host several MCP servers on one domain (`https://mcp.like-a-freedom.ru/memory`, `…/intervals`, …):

1. Set the env var on this service: `MCP_PUBLIC_BASE_PATH=/intervals`.
2. Configure the reverse proxy for **prefix pass-through — do NOT rewrite the path**:
   - Pangolin: target `Path=/intervals`, `Match=prefix`, Path Rewriting **off** (the backend expects `/intervals/...`).
   - Other proxies: forward the request path unchanged to this container.
3. Point clients at the prefixed endpoints:
   - MCP: `https://mcp.like-a-freedom.ru/intervals/mcp`
     (if the client appends `/mcp` itself, give it `https://mcp.like-a-freedom.ru/intervals`)
   - UI: `https://mcp.like-a-freedom.ru/intervals/ui`
4. Set `MCP_ALLOWED_HOSTS=mcp.like-a-freedom.ru` (public hostname, anti-DNS-rebinding).

Behavior notes when a prefix is set:

- The domain root is **freed**: `/`, `/health`, `/mcp`, `/ui` at the host root return 404, so they cannot collide with other services on the same domain.
- Health/metrics move under the prefix: `/intervals/health`, `/intervals/metrics`. If you do not want them public, simply do not publish those paths at the proxy.
- Direct container healthchecks (bypassing the proxy) must request `{prefix}/health`, or run local/dev instances without `MCP_PUBLIC_BASE_PATH`.
- The same image serves root and any prefix — switching is a restart with a different env value, no rebuild.

## License

MIT — see [`LICENSE`](LICENSE).

## Disclaimer

This project is not affiliated with, endorsed by, or sponsored by Intervals.icu. All product names, logos, and brands are the property of their respective owners.
