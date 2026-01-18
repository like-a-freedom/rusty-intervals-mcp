# intervals_icu_mcp

Minimal MCP server scaffold for Intervals.icu using the `rmcp` SDK.

Current status:
- Implements an `IntervalsMcpHandler` with many tools exposed via `rmcp` including:
	- Athlete/profile: `get_athlete_profile`
	- Activities: `get_recent_activities`, `get_activity_details`, `search_activities`, `update_activity`
	- Events: `get_events`, `create_event`, `get_event`, `delete_event`, `bulk_create_events`
	- Streams & intervals: `get_activity_streams`, `get_activity_intervals`, `get_best_efforts`
	- Files: `start_download`, `get_download_status`, `list_downloads`, `cancel_download` (progress & cancellation supported)
	- Webhooks: `receive_webhook` (HMAC verification + dedupe) and a programmatic `process_webhook`

Examples & usage

- Run HTTP server (example binary `server`):

```sh
export INTERVALS_ICU_API_KEY=...
export INTERVALS_ICU_ATHLETE_ID=...
cargo run -p intervals_icu_mcp --bin server
```

Or install the binary locally using `cargo install --path` and run it directly:

```sh
# From inside the crate directory
cargo install --path .
# Or from the repository root
cargo install --path crates/intervals_icu_mcp

# The binary will be installed to $CARGO_HOME/bin (usually ~/.cargo/bin)
# Run the installed server:
export INTERVALS_ICU_API_KEY=...
export INTERVALS_ICU_ATHLETE_ID=...
intervals_icu_mcp
```

(Use `--bin <name>` if you need to select a specific binary.)
- Run MCP server for stdio or other transports:

The crate supports RMCP transports (stdio, streamable HTTP). To run the server in a stdio/child-process mode, use the SDK examples (see top-level `examples/` or the RMCP SDK examples) or run the compiled binary as a child process. For HTTP usage, the server mounts the RMCP service at `/mcp`.

If you need a concrete stdio example, run an example from the RMCP SDK or see the `tests/e2e_stdio.rs` test for an example of launching the server as a child process.
- Running tests:

```sh
# Library unit tests
cargo test -p intervals_icu_mcp --lib

# E2E HTTP integration tests
cargo test -p intervals_icu_mcp --test e2e_http -- --nocapture
```

Notes
- To exercise webhooks: set the HMAC secret via the `set_webhook_secret` tool (or `set_webhook_secret_value` programmatically) and POST to `/webhook` with header `x-signature` containing the hex HMAC-SHA256 of the JSON body.
- The `start_download` tool kicks off a download job and returns a `download_id`. Use `get_download_status`/`list_downloads` to track progress and `cancel_download` to abort.
