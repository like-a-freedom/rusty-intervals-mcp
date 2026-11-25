# intervals_icu_mcp

The Intervals.icu MCP server uses `rmcp` and exposes nine curated intents:

| Intent | Purpose |
|---|---|
| `plan_training` | Build a training plan for a date range |
| `analyze_training` | Analyze a workout or training period |
| `modify_training` | Create, adjust, or delete planned workouts and events |
| `compare_periods` | Compare two training periods |
| `assess_recovery` | Assess readiness and recovery |
| `manage_profile` | Read or update the athlete profile |
| `manage_gear` | Manage bikes and gear |
| `analyze_race` | Analyze race performance |
| `track_progress` | Track long-term progress and trends |

OpenAPI operations stay internal. Durable mutation replay is limited to `plan_training` and `modify_training` writes using `idempotency_token`; it does not apply to previews or profile and gear updates.

## Run the server

Both transports require [persistent local state](../../README.md#configure-persistent-local-state): `MCP_STATE_DB_PATH` points to a writable SQLite file and `MCP_STATE_ENCRYPTION_KEY` is a stable 32-byte key encoded as 64 hex characters. Use one process per database. The server does not import legacy JSON token or mutation history. [Review the cutover guidance](../../docs/ARCHITECTURE.md#durable-local-state) before replacing a file-backed installation.

From the repository root, run the binary with `cargo run -p intervals_icu_mcp --bin intervals_icu_mcp`, or install it with `cargo install --locked --path crates/intervals_icu_mcp` and run `intervals_icu_mcp`.

- **STDIO** (default): run as an MCP client's child process with `INTERVALS_ICU_API_KEY` and `INTERVALS_ICU_ATHLETE_ID` in the process environment. See the [root setup guide](../../README.md#stdio-mode-default) or the [STDIO integration tests](tests/e2e_stdio.rs).
- **Streamable HTTP**: set `MCP_TRANSPORT=http` and a stable `JWT_MASTER_KEY` (64 bytes encoded as 128 hex characters). The binary listens on `127.0.0.1:3000` unless `MCP_HTTP_ADDRESS` overrides it. It does not read an athlete ID or API key at startup: clients exchange their credentials at [`POST /auth`](../../README.md#authenticating-with-auth) and send the returned bearer JWT to `/mcp`.

HTTP also serves `/health`, `/metrics` (optionally protected by `PROMETHEUS_METRICS_TOKEN`), and the operator token UI at `/ui`. Restrict UI access at the proxy or on a private network. If you set `MCP_PUBLIC_BASE_PATH`, forward the prefix unchanged and include it in client and probe URLs. Set `MCP_ALLOWED_HOSTS` for the `Host` header rmcp receives.

See the [Docker guide](../../README.md#docker-and-remote-deployment) for container setup and [HTTP reliability notes](../../docs/HTTP_RELIABILITY.md) for limits, timeouts, and proxy behavior. The [standalone HTTP example](examples/servers_http.rs) only demonstrates `/health` and `/metrics`; it is not the MCP binary and does not exercise JWTs, SQLite, or Streamable HTTP.

## Tests

```sh
cargo test -p intervals_icu_mcp --lib
cargo test -p intervals_icu_mcp --test e2e_http --test e2e_stdio
```
