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
AllowedHostsLayer  — DNS rebinding guard
    ↓
Request ID layer   — per-request tracing span
    ↓
Metrics layers     — request counters, active-gauge, duration histogram
    ↓
Body limit layer   — MAX_HTTP_BODY_SIZE
    ↓
CORS layer
    ↓
GracefulShutdownLayer
    ↓
Router (axum)
  ├─ GET  /health          — healthz
  ├─ GET  /metrics         — Prometheus (optional Bearer token)
  ├─ POST /auth            — exchange ICu key for JWT
  ├─ GET  /ui              — token management UI
  └─ POST /mcp             — JSON-RPC endpoint
```

### `/mcp` sub-layers (inside the router)

```text
AuthMiddleware          — extracts JWT, resolves athlete_id
GovernorLayer           — per-athlete rate limiting (AthleteKeyExtractor)
IdleTimeoutLayer        — request inactivity timeout (REQUEST_TIMEOUT_SECONDS)
McpJsonRpcService       — JSON-RPC processing, session management
    ↓
Intent Router           — validation, idempotency, orchestration
Internal Execution Layer— dynamic OpenAPI runtime, ICu client
```

The MCP capability boundary is intentionally smaller than the internal execution layer:
`tools/list` returns exactly the nine curated intent tools. OpenAPI operation ids are
private implementation capabilities used by server-owned orchestration and are neither
listed nor accepted as `tools/call` names.

Key design: auth runs **before** governor so `athlete_id` is available for rate-limit keying. Unauthenticated endpoints (`/health`, `/metrics`) are separate routes and skip both layers.

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

`GracefulShutdownLayer` listens for SIGTERM / Ctrl-C and drains in-flight requests using tokio's cooperative cancellation. The readiness probe flips to `ready=false` so load balancers stop routing new traffic before the socket closes.

## Observability

All layers publish counters and histograms to Prometheus (`GET /metrics`):

| Layer | What it tracks |
|-------|----------------|
| Request counter | `http_requests_total{path, status}` |
| Active gauge | `http_active_requests` (up/down per handler) |
| Duration histogram | `http_request_duration_seconds_bucket{path}` |
| Rate limit | `rate_limited_total{scope}` (incremented on 429) |
| Auth | `tokens_issued_total`, `token_verifications_total{status}` |
| Tool calls | `tool_calls_total{tool}`, `tool_duration_seconds{tool}` |

## Testing strategy

- Unit tests near implementation (governor error handler, keepalive construction, rate-limit parsing)
- Integration tests via `cargo test -p intervals_icu_mcp --test metrics_tests`
- E2E HTTP test via `cargo test -p intervals_icu_mcp --test e2e_http`
- Idempotency flakiness note: `test_idempotency_entry_is_expired` is timing-dependent; if it flakes, use `cargo test -- --retries 3` locally

## Evidence-Qualified Coaching Metrics

### Load Provenance Pipeline

All load extraction flows through `extract_activity_load` in `analysis_fetch.rs`:

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
