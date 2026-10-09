# Architecture

## Crate layout

| Crate | Purpose |
|-------|---------|
| `intervals_icu_client` | HTTP client, retries, observability, API compatibility helpers |
| `intervals_icu_mcp` | MCP server, intent layer, dynamic OpenAPI runtime, resources, and tests |

## Layering (HTTP mode)

```text
Client request
    ↓
TCP accept (socket2 keepalive)
    ↓
Request-ID middleware  — adopts/echoes X-Request-ID, resolves real client_ip
                         (X-Forwarded-For / X-Real-IP behind trusted proxies),
                         per-request access log
    ↓
Router (axum)
  ├─ [optional] nest MCP_PUBLIC_BASE_PATH — public prefix pass-through (ADR-0010);
  │             trailing `{prefix}/` and out-of-prefix paths → fallback (redirect / 404)
  ├─ GET  /health          — healthz (unauthenticated)
  ├─ GET  /metrics         — Prometheus (unauthenticated)
  ├─ POST /auth            — exchange ICu key for JWT; record the token in the registry (governor 1 rps + timeout)
  ├─ GET  /ui, /ui/token…  — token management UI over the registry (governor 2 rps)
  └─ POST /mcp             — JSON-RPC endpoint (see sub-layers)
```

The `Mcp-Session-Id` lifecycle (create / initialize / close) is managed by
rmcp's StreamableHttpService with a `LoggingSessionManager` decorator
(`session.rs`) that logs session open/close with client identity and lifetime.

### `/mcp` sub-layers (inside the router)

```text
Extension(HttpBaseUrl)
GovernorLayer           — per-athlete rate limiting (keyed by athlete_id from the JWT)
IdleTimeoutLayer        — request inactivity timeout (REQUEST_TIMEOUT_SECONDS)
AuthMiddleware          — verifies JWT, resolves credentials from the token registry by `jti`, rejects revoked tokens, resolves athlete_id + request_id into extensions
DefaultBodyLimit        — MAX_HTTP_BODY_SIZE
TimeoutLayer            — 408 on overall request timeout
McpJsonRpcService       — rmcp StreamableHttpService; allowed-host check lives here
    ↓
Intent Router           — validation, idempotency, orchestration
Internal Execution Layer— dynamic OpenAPI runtime, ICu client
```

The MCP capability boundary is intentionally smaller than the internal execution layer:
`tools/list` returns exactly the nine curated intent tools. OpenAPI operation ids are
private implementation capabilities used by server-owned orchestration and are neither
listed nor accepted as `tools/call` names.

Key design: auth runs **before** governor so `athlete_id` is available for rate-limit keying. Unauthenticated endpoints (`/health`, `/metrics`) are separate routes and skip both layers.

### Token registry (ADR-0011)

The `token_registry` module is the single source of truth for issued tokens, keyed by `jti`. It holds each token's athlete, lifetime, revocation state, and the athlete's upstream credential **encrypted at rest**; the JWT carries only identity (`athlete_id` + `jti`) and never the credential. Both `/auth` and the UI write to it, the `AuthMiddleware` reads it, and it persists to `MCP_TOKEN_REGISTRY_PATH` when set (in-memory otherwise, with a startup warning). Legacy tokens that still embed their credential in the claims stay verifiable until they expire.

## Rate limiting

Governor enforces a per-athlete token bucket:

| Config | Default | Env var |
|--------|---------|---------|
| Rate (req/s) | 5 | `MCP_RATE_LIMIT_PER_SECOND` |
| Burst | 15 | `MCP_RATE_LIMIT_BURST` |

The key is `athlete_id` from the JWT. Fallback (unauthenticated or missing claim) uses client IP.

Governor's error handler calls `metrics::rate_limited("mcp")` so every 429 is visible in Prometheus.

## TCP keepalive

Configured via `socket2::TcpKeepalive` on the server socket before `into_make_service()`:

| Parameter | Value |
|-----------|-------|
| Idle time | `IDLE_TIMEOUT_SECONDS` (default 60s) |
| Interval | 10 s (hard-coded) |
| Retries | OS default |

Detects dead connections early so clients don't silently stall.

## Graceful shutdown

`shutdown_signal` listens for SIGTERM / Ctrl-C and feeds `axum::serve(...).with_graceful_shutdown(...)`: the socket stops accepting new connections, in-flight requests finish, and the stop is logged with `drain_ms`. `/health` keeps returning `ok` for the whole process lifetime — there is no readiness flip.

## Observability

All layers publish counters and histograms to Prometheus (`GET /metrics`):

| Layer | Metric (prefix `intervals_icu_`) |
|-------|--------------------------------|
| HTTP requests | `mcp_http_requests_total{path, status}`, `mcp_http_request_duration_seconds`, `mcp_active_requests` |
| Rate limit | `mcp_rate_limited_total{scope}` |
| Auth | `mcp_tokens_issued_total{source}`, `mcp_token_verifications_total{status}`, `mcp_auth_failures_total` |
| Tool calls | `mcp_tool_calls_total{tool, success}`, `mcp_tool_duration_seconds{tool}` |
| MCP sessions | `mcp_mcp_sessions_total{status}` (started / ended), `mcp_mcp_method_calls_total` |
| Upstream intervals.icu | `client_upstream_requests_total{status}`, `client_upstream_errors_total{error_type}`, `client_upstream_request_duration_seconds`, `client_circuit_state` |
| Idempotency | `mcp_idempotency_total{result}` |
| Dynamic dispatch | `mcp_dynamic_dispatch_total{method, outcome}` |

(mod.rs files are not used; the layout is `<name>.rs` + `<name>/` — Rust 2018 pathless style.)

## Testing strategy

- Unit tests near implementation (governor error handler, keepalive construction, rate-limit parsing)
- Integration tests via `cargo test -p intervals_icu_mcp --test metrics_tests`
- E2E HTTP test via `cargo test -p intervals_icu_mcp --test e2e_http`
- Idempotency flakiness note: `test_idempotency_entry_is_expired` is timing-dependent; if it flakes, use `cargo test -- --retries 3` locally

## Evidence-Qualified Coaching Metrics

### Load Provenance Pipeline

All load extraction flows through `extract_activity_load` in `engines/analysis_fetch/load.rs`:

1. **Alias priority:** `icu_training_load` → `training_load`/`icuTrainingLoad` → `tss`
2. **Activity summary fallback:** `training_load` field on `ActivitySummary` (lower priority)
3. **No duration fallback:** `moving_time` is never used as a load proxy
4. **Coverage disclosure:** `ComparableLoadSeries` tracks `activities_with_load` / `activities_total` and source counts

### HRV/RHR Personal Baselines

Computed in `domains/baseline.rs`:

- **Window:** 60-day reference baseline, 7-day recent window
- **Transform:** HRV is ln-transformed (lnRMSSD) before comparison
- **SWC formula:** `SWC = mean × CV × 0.5`
- **Positions:** Below / Within / Above (neutral; no medical claims)
- **Data quality:** Requires ≥3 recent observations, ≥14 baseline observations spanning ≥28 days

### Critical Power Diagnostics

Computed in `engines/cp_regression.rs`:

- **Model:** 2-parameter CP model: P(t) = CP + W'/t
- **Valid:** Mathematically identifiable, finite, positive CP/W' (no R² threshold)
- **Diagnostics:** sample count, duration range, R², RMSE, max residual, CP/W' standard errors
- **No validation:** Reports fit quality, not physiological accuracy

### Endurance Evidence Pipeline

Cycling-only evidence-gated endurance metrics. The pipeline lives inside `analyze_training` period detail:

```
bounded historical ride details/streams (≤ 12 recent + ≤ 12 reference, 90-day lookback)
  → strict MetricStreams parser (engines/metric_streams.rs)
  → two-pointer sliding-window collector (CONTROL_WINDOW_S=600s, stride=60s)
  → pure endurance_evidence engine (engines/endurance_evidence.rs)
  → CoachContext.endurance_evidence (Option<EnduranceEvidenceMetrics>)
  → analyze_training detailed-period renderer
```

The fetch layer (`analysis_fetch::collect_endurance_evidence`) is bounded, best-effort, and never fails the period analysis — degraded runs surface explicit `InsufficientCandidateSessions` and a partial-data warning. Summary mode skips both the fetch and the renderer to guarantee zero extra HTTP traffic and zero output surface.

### Rendered Sections

- `Training Load Data Quality`: source counts, coverage percentage, excluded activities
- `Personal Baseline`: HRV/RHR with neutral position labels and model version
- `CP Model Diagnostics`: sample count, coverage, R², RMSE, standard errors
- `Endurance Performance Evidence — Cycling Power Protocol`: raw 10-minute HR–power windows and matched early/late prolonged-ride windows, with explanatory context (no readiness/durability scoring)
