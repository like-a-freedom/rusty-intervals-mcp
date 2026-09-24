# intervals_icu_mcp

Minimal MCP server scaffold for Intervals.icu using the `rmcp` SDK.

Current status:
- Implements an `IntervalsMcpHandler` exposing 9 curated intents via `rmcp`:
	- `plan_training` — build a training plan for a date range
	- `analyze_training` — analyze recent training load and fitness
	- `modify_training` — create/adjust/delete planned workouts
	- `compare_periods` — compare two training periods
	- `assess_recovery` — assess readiness and recovery
	- `manage_profile` — read/update athlete profile
	- `manage_gear` — manage bikes and gear
	- `analyze_race` — analyze race performance
	- `track_progress` — track long-term progress and trends

Examples & usage

- Run the main binary in HTTP streamable mode:

```sh
export INTERVALS_ICU_API_KEY=...
export INTERVALS_ICU_ATHLETE_ID=...
export MCP_TRANSPORT=http
export MCP_HTTP_ADDRESS=127.0.0.1:3000
export JWT_MASTER_KEY=$(openssl rand -hex 64)
cargo run -p intervals_icu_mcp --bin intervals_icu_mcp
```

Or install the binary locally using `cargo install --locked --path` and run it directly:

```sh
# From inside the crate directory
cargo install --locked --path .
# Or from the repository root
cargo install --locked --path crates/intervals_icu_mcp

# The binary will be installed to $CARGO_HOME/bin (usually ~/.cargo/bin)
# Run the installed server:
export INTERVALS_ICU_API_KEY=...
export INTERVALS_ICU_ATHLETE_ID=...
export MCP_TRANSPORT=http   # omit or set to stdio for local child-process use
export MCP_HTTP_ADDRESS=127.0.0.1:3000
export JWT_MASTER_KEY=$(openssl rand -hex 64)
intervals_icu_mcp
```

(Use `--bin <name>` if you need to select a specific binary.)
- Run MCP server for stdio or other transports:

The crate supports RMCP transports (stdio, streamable HTTP). In stdio mode, the binary reads `INTERVALS_ICU_API_KEY` and `INTERVALS_ICU_ATHLETE_ID` directly from the environment. In HTTP mode, the server mounts `/auth`, `/health`, and the streamable MCP service at `/mcp` and requires `JWT_MASTER_KEY`. Setting `MCP_PUBLIC_BASE_PATH` (e.g. `/intervals`) serves all of these routes under that prefix for reverse-proxy pass-through deployments.

If you need a concrete stdio example, run an example from the RMCP SDK or see the `tests/e2e_stdio.rs` test for an example of launching the server as a child process.

Useful HTTP-mode environment variables:

- `MCP_HTTP_ADDRESS` (default `127.0.0.1:3000`)
- `MAX_HTTP_BODY_SIZE` (default `4194304` bytes)
- `REQUEST_TIMEOUT_SECONDS` (default `30`)
- `IDLE_TIMEOUT_SECONDS` (default `60`)
- `JWT_MASTER_KEY` (required, 64-byte hex key)
- `JWT_TTL_SECONDS` (default `7776000` = 90 days)

For containerized deployment, prefer the repository-level `Dockerfile` and `docker-compose.yml`; those artifacts are intended for HTTP streamable MCP, not stdio child-process usage.
- Running tests:

```sh
# Library unit tests
cargo test -p intervals_icu_mcp --lib

# E2E HTTP integration tests
cargo test -p intervals_icu_mcp --test e2e_http -- --nocapture
```

Notes
- The public MCP surface is the 9 curated intents listed above; dynamic OpenAPI operations stay internal and are never exposed as tool names.
