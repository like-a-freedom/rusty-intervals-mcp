# HTTP Reliability

This document covers the mechanisms that keep the HTTP MCP server resilient under load, retries, and transient upstream failures.

## Rate limiting

### Per-athlete isolation

`/mcp` requests must authenticate before reaching the governor. Successful requests share a bucket with other JWTs for the same `athlete_id`; missing or rejected JWTs do not reach the bucket. `/auth` and `/ui` use separate buckets keyed by the direct TCP peer IP, so clients behind the same proxy may share those quotas.

### Configuration

| Env var | Default | Description |
|---------|---------|-------------|
| `MCP_RATE_LIMIT_PER_SECOND` | 5 | Seconds to replenish one request for an athlete in the current implementation |
| `MCP_RATE_LIMIT_BURST` | 15 | Burst capacity per athlete |

Despite the name, the value is passed to tower-governor's `per_second`, which takes a refill interval in seconds, not a request rate. Invalid values (zero, negative, non-numeric) log a warning and use defaults. `/auth` refills one request every second (burst 3); `/ui` refills one every two seconds (burst 5).

### Metrics

The `/mcp` governor emits `intervals_icu_mcp_rate_limited_total{scope="mcp"}` for its own rejections. `/auth` and `/ui` rejections appear in the general HTTP request counter with status `429`, not in the dedicated rate-limit counter.

## TCP keepalive

Configured via `socket2::TcpKeepalive` on the listening socket:

- Time before the first probe: `IDLE_TIMEOUT_SECONDS` (default 60 s, clamped to at least 1 s)
- Interval: 10 s
- Retries: OS default

Detects dead peers before the next request arrives, preventing silent stalls.

## Request timeout

The tower-http `TimeoutLayer` returns `408 Request Timeout` if `/auth` or `/mcp` does not produce a response within `REQUEST_TIMEOUT_SECONDS` (default 30 s). It does not time out an already-started response stream. There is no separate request inactivity timeout; `IDLE_TIMEOUT_SECONDS` controls TCP keepalive, not when to close a healthy idle connection.

## Body size limiting

The `/mcp` router sets Axum's `DefaultBodyLimit` from `MAX_HTTP_BODY_SIZE` (default 4 MiB), but rmcp reads POST bodies directly with its own default 4 MiB limit and returns `413 Payload Too Large` when it is exceeded. Changing `MAX_HTTP_BODY_SIZE` does not change that rmcp limit. `/auth` and UI forms use Axum's default extractor limits.

## Graceful shutdown

SIGTERM / Ctrl-C trigger `axum::serve` graceful shutdown: the socket stops accepting new connections, in-flight requests drain to completion, and the exit is logged with `drain_ms`. There is no readiness flip — `/health` returns plain `ok` while the process is alive.

## Request correlation

The application middleware adopts a trimmed inbound `X-Request-ID` of up to 128 ASCII letters, digits, dashes, underscores, or dots. Otherwise it generates a random 32-character hex ID. It echoes the ID in the response and passes it to handlers for correlation in access, auth, and tool logs. The outer path-prefix fallback does not run this middleware.

Access logs include `method`, `path`, `status`, `client_ip`, and `latency_ms`: `warn` for 5xx, `info` for 4xx, `debug` otherwise. Paths ending in `/health` or `/metrics` skip the access log but are still measured when they reach the application middleware.

## Log-level policy

`warn` means an operator (or athlete) should look; `info` marks business events worth keeping forever; anything routine or per-request is `debug`/`trace`. In practice:
- `info`: auth issuance and 4xx access logs, tool-call audit (`outcome`, `response_bytes`), session open/close with `client_name`, startup/shutdown, config summary, cache refreshes that succeed.
- `warn`: 5xx responses, JWT refusals, upstream intervals.icu failures (`transport`, `5xx`, `429`, `auth`, `circuit_open`), tool errors and slow calls (`> 10 s`), response serialization failures, rate-limit rejections, invalid config values falling back to defaults, mutex poisoning, panics.
- `debug`: routine per-request lines (access log on success, authenticated request), upstream 4xx/not-found, optional-data gaps (missing histograms), internal fallbacks and retries, engine fetch detail.

## Real client IP behind proxies

For logging, the server trusts forwarding headers only from a loopback, private/unique-local, or link-local TCP peer. It uses the rightmost parseable `X-Forwarded-For` address, then `X-Real-IP`, then the socket peer. There is no configurable trusted-proxy allowlist and no traversal of a proxy chain. If the nearest proxy sees another proxy, its address may be what gets logged. These headers do not affect the rate-limit keys.

Restrict direct access to the backend from networks treated as trusted. At the last trusted proxy, set `X-Forwarded-For` from a verified client address (for a single nginx edge proxy, `proxy_set_header X-Forwarded-For $remote_addr;`) rather than passing client-supplied headers unchanged. Depending on the platform and network mode, Docker port publishing may show a bridge or relay address instead of the original client.

`MCP_ALLOWED_HOSTS` configures rmcp's `Host` check at `/mcp`, not trusted peers or access to `/ui`. Set it to the comma-separated hostnames the proxy sends to rmcp; the default allows loopback hosts. With `MCP_PUBLIC_BASE_PATH`, pass the full request path through and use the prefix for health and metrics checks too.

## 429 recovery strategy for clients

When a client (e.g., Hermes) receives a 429:

1. **Back off** — do not retry immediately; respect `Retry-After` if present.
2. **Reconnect with jitter** — randomize reconnection delay to avoid thundering-herd.
3. **Use per-athlete limits** — each athlete has its own bucket; a single slow client cannot block others.

## Observability checklist

| Metric | What to watch |
|--------|---------------|
| `intervals_icu_mcp_rate_limited_total{scope="mcp"}` | `/mcp` governor rejections |
| `intervals_icu_mcp_http_requests_total{status="429"}` | HTTP 429s by route and method, including `/auth` and `/ui` |
| `intervals_icu_mcp_active_requests` | Requests still waiting for a response, not open SSE streams |
| `intervals_icu_mcp_token_verifications_total{status=~"invalid\|expired\|revoked\|registry_error"}` | Separate rejected credentials from token registry errors |
| `intervals_icu_mcp_operation_journal_unknown_operations`, `intervals_icu_mcp_operation_journal_oldest_unknown_age_seconds` | Unresolved writes that need upstream investigation, not automatic retries |
