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

SIGTERM / Ctrl-C trigger `axum::serve` graceful shutdown: the socket stops accepting new connections, in-flight requests drain to completion, and the exit is logged with `drain_ms`. There is no readiness flip — `/health` returns plain `ok` while the process is alive.

## Request correlation

The outermost middleware assigns every HTTP request a correlation id: a well-formed inbound `X-Request-ID` (max 128 visible ASCII chars) is adopted, otherwise a random 32-hex id is generated. The id is echoed back in the `X-Request-ID` response header and carried as `request_id=` in every log line produced for that request (auth refusals, `/auth` outcomes, tool calls).

Each request also produces one access-log line with `method`, `path`, `status`, `client_ip`, and `latency_ms`: `warn` for 5xx, `info` for 4xx, `debug` otherwise. `/health` and `/metrics` probes are silent to avoid log noise.

## Log-level policy

`warn` means an operator (or athlete) should look; `info` marks business events worth keeping forever; anything routine or per-request is `debug`/`trace`. In practice:
- `info`: auth issuance/refusal, tool-call audit (`outcome`, `response_bytes`), session start with `client_name`, startup/shutdown, config summary, cache refreshes that succeed.
- `warn`: 5xx responses, upstream intervals.icu failures (`transport`, `5xx`, `429`, `auth`, `circuit_open`), tool errors and slow calls (`> 10 s`), response serialization failures, rate-limit rejections, invalid config values falling back to defaults, mutex poisoning, panics.
- `debug`: routine per-request lines (access log on success, authenticated request), upstream 4xx/not-found, optional-data gaps (missing histograms), internal fallbacks and retries, engine fetch detail.

## Real client IP behind proxies

Docker NAT and reverse proxies hide the true client address in the TCP handshake (`ConnectInfo` shows the proxy/bridge, e.g. `172.26.0.x`). The server therefore resolves `client_ip` from `X-Forwarded-For` (right-most entry — the hop seen by the nearest proxy, so a fabricated left-most value cannot win) and `X-Real-IP` — but **only when the direct TCP peer is loopback/private/link-local**, i.e. a trusted local relay. A public direct peer is never trusted.

Operators must make the proxy send the header:
- nginx: `proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;` (or `X-Real-IP $remote_addr;`)
- Traefik and Caddy: sent by default.

Without such a proxy (plain `docker -p` port publishing), the container never sees the real client IP at all — use a reverse proxy or `network_mode: host`.

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
