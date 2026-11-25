# Architecture

## Crate layout

| Crate | Purpose |
|-------|---------|
| `intervals_icu_client` | HTTP client, retries, observability, API compatibility helpers |
| `intervals_icu_mcp` | MCP server, intent layer, dynamic OpenAPI runtime, resources, and tests |

## Startup and transports

The binary defaults to `MCP_TRANSPORT=stdio`: rmcp uses stdin/stdout and the handler uses the configured athlete ID and API key. The `http` transport serves stateful Streamable HTTP using per-request JWT credentials; it does not need an athlete or API key at startup. Both standard entry paths open the [SQLite state store](#durable-local-state) before serving requests.

## Layering (HTTP mode)

```text
TCP listener (socket2 keepalive)
  → optional MCP_PUBLIC_BASE_PATH mount (proxy passes path unchanged)
    → request-ID / access-log middleware
      → HTTP metrics middleware
        ├─ GET /              — redirect to the UI
        ├─ GET /health        — unauthenticated liveness
        ├─ GET /metrics       — Prometheus; optional separate bearer secret
        ├─ POST /auth         — 408 timeout → peer-IP governor → validate and issue JWT
        ├─ /ui routes         — peer-IP governor → token create/list/revoke and CSS
        └─ /mcp               — authenticated rmcp Streamable HTTP (see below)
```

With a prefix, out-of-prefix 404s and the trailing-slash prefix redirect use the outer fallback, outside the application's request-ID and metrics middleware. See [ADR-0010](adr/0010-public-base-path-prefix.md) and [HTTP reliability](HTTP_RELIABILITY.md) for proxy handling.

The `Mcp-Session-Id` lifecycle (create / initialize / close) is managed by
rmcp's StreamableHttpService with a `LoggingSessionManager` decorator
(`session.rs`) that logs session open/close with client identity and lifetime.

### `/mcp` sub-layers (inside the router)

```text
TimeoutLayer            — 408 if the route has not produced a response in time
DefaultBodyLimit        — MAX_HTTP_BODY_SIZE for Axum extractors (not rmcp's body limit)
Auth middleware         — verifies bearer JWT; inserts credentials and replay owner
GovernorLayer           — bucket keyed by the authenticated athlete
Extension(HttpBaseUrl)  — configured upstream base URL
rmcp StreamableHttpService — Host check, its own POST body limit, JSON-RPC and sessions
    ↓
Intent router           — validation and orchestration; journal for supported writes
Internal execution      — typed Intervals.icu client and optional dynamic dispatch
```

The MCP capability boundary is intentionally smaller than the internal execution layer:
`tools/list` returns exactly the nine curated intent tools. OpenAPI operation ids are
private implementation capabilities used by server-owned orchestration and are neither
listed nor accepted as `tools/call` names.

Authentication runs **before** the `/mcp` governor, so missing or rejected JWTs do not reach its rate limiter. `/auth` and `/ui` have separate peer-IP buckets; `/health` and `/metrics` use neither the MCP JWT middleware nor a governor. See [rate limiting](HTTP_RELIABILITY.md#rate-limiting) for actual refill intervals.

## Durable local state

Both transports require `MCP_STATE_DB_PATH` on writable local storage and a stable `MCP_STATE_ENCRYPTION_KEY` (32 bytes, 64 hex characters). HTTP also requires a stable `JWT_MASTER_KEY` (64 bytes, 128 hex characters). Use one server process per database, not a shared file across processes or hosts. Keep the database, WAL/SHM files, keys, and backups private: credentials and saved results are encrypted, but not all metadata is.

### Token registry

`POST /auth` and the token UI validate credentials against Intervals.icu before issuing a recorded JWT. The JWT carries `athlete_id` and `jti` plus standard claims, not the upstream key. The SQLite registry keyed by `jti` stores lifetime, revocation state, and the upstream key encrypted with a key derived from `JWT_MASTER_KEY`. Registry read errors fail authentication closed. Legacy JWTs carrying an encrypted credential may still verify with the same master key until they expire; this claims fallback does not import a registry or restore its revocation records. See [ADR-0011](adr/0011-server-side-token-registry.md) for the token model and [ADR-0012](adr/0012-single-process-sqlite-state.md) for persistence.

The browser session is in-memory and provides CSRF protection and a default athlete filter, not operator authentication or athlete isolation. Protect `/ui` and its subpaths at the proxy or restrict them to a private network. The API-key exchange is not an OAuth flow.

### Mutation journal

Durable replay covers non-dry-run writes with an `idempotency_token` in `plan_training` and `modify_training`, not all nine intents or all mutating intents. After local validation, the journal reserves the scoped key (normalized upstream URL, athlete, and token) before calling upstream. Completed results can replay for 24 hours to the original owner; HTTP ownership follows the JWT identity. Active operations left behind at restart become unknown. Unknown records block resubmission indefinitely. The SQLite v1-to-v2 schema upgrade preserves older unscoped rows; they block matching keys without replay while retained.

There is no JSON import for either the token registry or the operation history. Unset the obsolete registry environment variable and use a separate SQLite file; reissue registry-backed JWTs. Do not replace a legacy journal with an empty database and resume automatic retries: preserve the old files and reconcile outstanding writes upstream first. An existing non-SQLite file at the state path fails startup without importing or overwriting it.

## TCP keepalive

Configured via `socket2::TcpKeepalive` on the listening socket before binding:

| Parameter | Value |
|-----------|-------|
| Idle time | `IDLE_TIMEOUT_SECONDS` (default 60s) |
| Interval | 10 s (hard-coded) |
| Retries | OS default |

Detects dead connections early so clients don't silently stall.

## Graceful shutdown

`shutdown_signal` listens for SIGTERM / Ctrl-C and feeds `axum::serve(...).with_graceful_shutdown(...)`: the socket stops accepting new connections, in-flight requests finish, and the stop is logged with `drain_ms`. `/health` keeps returning `ok` for the whole process lifetime — there is no readiness flip.

## Observability

The standard HTTP server publishes Prometheus metrics at `GET /metrics`; STDIO does not serve a metrics endpoint. Representative series:

| Layer | Metric (prefix `intervals_icu_`) |
|-------|--------------------------------|
| HTTP requests | `mcp_http_requests_total{method,path,status}`, `mcp_http_request_duration_seconds{method,path}`, `mcp_active_requests` |
| Rate limit | `mcp_rate_limited_total{scope}` |
| Auth | `mcp_tokens_issued_total{source}` (`jwt` or `ui`), `mcp_token_verifications_total{status}`, `mcp_auth_failures_total{reason}` |
| Tool calls | `mcp_tool_calls_total{tool,status}` (`success` or `error`), `mcp_tool_duration_seconds{tool}` |
| MCP sessions and methods | `mcp_mcp_sessions_total{status}` (`started` or `ended`), `mcp_mcp_method_calls_total{method}` |
| Upstream intervals.icu | `client_upstream_requests_total{status}`, `client_upstream_errors_total{error_type}`, `client_upstream_request_duration_seconds`, `client_circuit_state` |
| Idempotency | `mcp_idempotency_total{result}` |
| Mutation journal | `mcp_operation_journal_errors_total`, `mcp_operation_journal_quota_rejections_total{quota}`, `mcp_operation_journal_unknown_operations`, `mcp_operation_journal_oldest_unknown_age_seconds` |
| Dynamic dispatch | `mcp_dynamic_dispatch_total{method, outcome}` |

HTTP metrics use the matched route template (or `unknown`), not the raw URL. The active gauge and latency histogram track requests until a response is produced, not the lifetime of a streaming response. UI actions also use `mcp_ui_action_total{action}` (without the `intervals_icu_` prefix).

## Testing strategy

- Unit tests near implementation (governor error handler, keepalive construction, rate-limit parsing)
- Integration tests via `cargo test -p intervals_icu_mcp --test metrics_tests`
- E2E HTTP test via `cargo test -p intervals_icu_mcp --test e2e_http`
- E2E STDIO and durable replay tests via `cargo test -p intervals_icu_mcp --test e2e_stdio --test http_mode_integration`

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
