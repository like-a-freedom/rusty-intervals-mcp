# HTTP Reliability

This document covers the mechanisms that keep the HTTP MCP server resilient under load, retries, and transient upstream failures.

## Rate limiting

### Per-athlete isolation

Governor rate limiting is keyed by `athlete_id` from the JWT (via `AthleteKeyExtractor`). This isolates each athlete's quota so one client's reconnection burst cannot exhaust another's allowance.

Fallback: if no JWT or no `athlete_id` claim, the governor falls back to client IP.

### Configuration

| Env var | Default | Description |
|---------|---------|-------------|
| `MCP_RATE_LIMIT_PER_SECOND` | 5 | Requests per second per athlete |
| `MCP_RATE_LIMIT_BURST` | 15 | Burst capacity per athlete |

Invalid values (zero, negative, non-numeric) log a warning and use defaults.

### Metrics

Governor's error handler calls `metrics::rate_limited("mcp")` on every 429, making rate limit events visible in Prometheus:

```
rate_limited_total{scope="mcp"}
```

## TCP keepalive

Configured via `socket2::TcpKeepalive` on the accepted TCP socket:

- Idle time: `IDLE_TIMEOUT_SECONDS` (default 60 s)
- Interval: 10 s
- Retries: OS default

Detects dead peers before the next request arrives, preventing silent stalls.

## Request inactivity timeout

`IdleTimeoutLayer` (tower-http) enforces `REQUEST_TIMEOUT_SECONDS` (default 30 s). If no bytes flow for the duration, the connection is dropped. This prevents zombie connections from consuming resources.

## Body size limiting

`DefaultBodyLimitLayer` caps request bodies at `MAX_HTTP_BODY_SIZE` (default 4 MiB). Oversized requests receive `413 Payload Too Large` without reading the body.

## Graceful shutdown

1. Readiness probe returns `{"ready": false}` — load balancers stop routing.
2. Tokio `CancellationToken` fires — in-flight requests finish draining.
3. Server socket closes.

Controlled by `IDLE_TIMEOUT_SECONDS` for the drain window.

## 429 recovery strategy for clients

When a client (e.g., Hermes) receives a 429:

1. **Back off** — do not retry immediately; respect `Retry-After` if present.
2. **Reconnect with jitter** — randomize reconnection delay to avoid thundering-herd.
3. **Use per-athlete limits** — each athlete has its own bucket; a single slow client cannot block others.

## Observability checklist

| Metric | What to watch |
|--------|---------------|
| `rate_limited_total{scope="mcp"}` | Spikes indicate client retry storms |
| `http_requests_total{status="429"}` | Same signal, different lens |
| `http_active_requests` | Should drop to 0 on shutdown |
| `token_verifications_total{status="error"}` | Auth failures under load |
