# ADR-0007: Upgrade `rmcp` from 2.x to 3.1

- **Status**: Accepted
- **Date**: 2026-08-02
- **Authors**: rusty_intervals_mcp maintainers

## Context

The Rust MCP SDK (`rmcp`) shipped two major releases in late July 2026:

| Version | Date | Notes |
|---|---|---|
| 3.0.0 | 2026-07-28 | Tracks the MCP `2026-07-28` protocol draft. Default negotiated protocol is still `V_2025_11_25` (legacy). |
| 3.1.0 | 2026-07-31 | Stabilizes the public server/client APIs over the beta line. |

The project currently depends on `rmcp = "2.2.0"` (production) and `rmcp = "2.1.0"`
(dev) in `crates/intervals_icu_mcp/Cargo.toml`. These pin us to the legacy release
line; new `rmcp` examples, CI fixtures, and ecosystem consumers assume the 3.x line.

Upstream maintains a [migration discussion (#969)][discussion], but the discussion
body is a stub (0 comments as of 2026-08-01) — the authoritative reference is the
[rmcp CHANGELOG][changelog], cross-referenced against docs.rs for 3.1.

### What changed in rmcp 3 that touches us

Verified against the published 3.1.0 crate (the compiler and the crate
source in `~/.cargo/registry/` are authoritative; docs.rs lagged on the
`ServerHandler` surface and misled the initial planning):

- **`ServerHandler` result types are wrapped.** `call_tool` now returns
  `Result<CallToolResponse, ErrorData>` and `read_resource` returns
  `Result<ReadResourceResponse, ErrorData>`. Both are `#[non_exhaustive]`
  enums with a `Complete(...)` variant plus the MRTR `InputRequired` /
  `Task` variants. `From<CallToolResult> for CallToolResponse` and
  `From<ReadResourceResult> for ReadResourceResponse` map to `Complete`.
  The migration wraps our existing results (always `Complete`); the method
  bodies themselves are unchanged.
- **`Tool` and `Resource` are now `#[non_exhaustive]`.** Direct field writes on
  _existing_ fields (`tool.annotations = ...`, `resource.description = ...`)
  still compile; the non-exhaustive attribute only prevents destructuring struct
  literals without `..`. Most of our code already uses the builder methods
  (`with_*`) where available.
- **New optional capabilities** — MRTR (Model-Round-Trip Request), Tasks,
  resource subscriptions/listen, OAuth, Discovery — ship with default
  implementations that do nothing. We do not adopt any of them, keeping the
  public MCP contract identical to the 2.x line.
- **Wire format unchanged** for legacy peers. Per `3.0.0-beta.2` ([#1038]),
  the new `resultType` discriminator is omitted when negotiating
  `V_2025_11_25` (the default). Existing MCP hosts continue to talk to us
  without protocol-level changes.
- **MSRV bumped to 1.88** in the rmcp workspace (`Cargo.toml`:
  `rust-version = "1.88"`, `edition = "2024"`). Our current
  `rust-toolchain.toml` declares `channel = "stable"` with no explicit
  `rust-version`; we need to declare 1.88 as our minimum.

### Migration guide §1.1 — confirmed by the published crate

The upstream guide's §1.1 describes wrapping `CallToolResult` in a new
`CallToolResponse` rich type. **This is exactly what `ServerHandler::call_tool`
returns in rmcp 3.1.0.** The published crate's `handler/server.rs` requires
`Result<CallToolResponse, McpError>`; the `From<CallToolResult>` impl maps
onto the `Complete` variant. The initial planning reading (docs.rs) claimed
`CallToolResult` was kept — the compiler is authoritative: the wrapper is
required, and using `Complete` keeps behavior identical for hosts that do not
opt into MRTR. The same applies to `ReadResourceResponse` for `read_resource`.

## Decision

1. **Bump `rmcp` to `"3"`** in both the production block (`server`, `macros`,
   `transport-streamable-http-server`, `transport-streamable-http-server-session`)
   and the dev-dependencies block (`client`, `transport-io`, `transport-child-process`)
   of `crates/intervals_icu_mcp/Cargo.toml`. A single `"3"` constraint avoids
   workspace-resolver conflicts and rides future 3.x patch releases.
2. **Declare `rust-version = "1.88"`** on the `intervals_icu_mcp` package. Keep
   `rust-toolchain.toml` at `channel = "stable"` so local dev rides the latest
   stable; the `rust-version` field is the MSRV contract for downstream
   consumers.
3. **Minimal production code changes** in `crates/intervals_icu_mcp/src/lib.rs`
   only: `call_tool` wraps its result via `CallToolResponse::from` (→
   `Complete`), `read_resource` wraps via `.into()` (→ `Complete`), and
   `list_tools` / `list_resources` use `ListToolsResult::with_all_items` /
   `ListResourcesResult::with_all_items` to cover the new `result_type`,
   `ttl_ms`, and `cache_scope` fields. Every other rmcp surface we use
   (`Tool::new`, `ToolAnnotations::new`, `Resource::new`, `serve_server`,
   `StreamableHttpService::new`, `LocalSessionManager::default()`) is
   unchanged.
4. **Do not adopt MRTR, Tasks, OAuth, Discovery, or resource
   subscriptions.** These are optional capabilities with default no-op impls;
   adopting any of them is a separate ADR.
5. **Live in `[Unreleased]`** in `CHANGELOG.md`. The previous `rmcp 1.8.0 →
   2.1.0` migration landed as the 2.19.1 patch (see `CHANGELOG.md`). A
   release/version bump is the user's call, not the migration's.

## Scope

| File | Change |
|---|---|
| `crates/intervals_icu_mcp/Cargo.toml` | `rmcp = "2.2.0"` → `"3"`, dev `rmcp = "2.1.0"` → `"3"`, add `rust-version = "1.88"` |
| `CHANGELOG.md` | Add `[Unreleased] / Changed` entry |
| Production source (`crates/intervals_icu_mcp/src/lib.rs`) | Wrap `call_tool` / `read_resource` results in `CallToolResponse` / `ReadResourceResponse` (always `Complete`); use `with_all_items` for paginated results |
| `crates/intervals_icu_client/**` | No change (never imports `rmcp`) |
| `rust-toolchain.toml` | No change (`channel = "stable"` already rides 1.88+) |

**Compile-failure fallback applied**: `ListToolsResult` and
`ListResourcesResult` gained required fields (`result_type`, `ttl_ms`,
`cache_scope`). We use the intended `with_all_items(items)` constructor,
which defaults `result_type` to `Some(ResultType::COMPLETE)`; the server
handler clears the field when responding to peers that negotiated a
pre-2026-07-28 protocol version. (`..Default::default()` was the backup
plan; `with_all_items` is the primary, constructor-based fix.)

## Non-changes

- No new MCP capabilities (MRTR, Tasks, OAuth, subscriptions, Discovery).
- No wire-format change for legacy peers (we negotiate `V_2025_11_25`).
- No public MCP contract change — tool names, input schemas, output shapes,
  and resource URIs are identical.
- No release/version bump of `intervals_icu_mcp` — left to the user's discretion.
- No changes to `crates/intervals_icu_client` (zero `rmcp` usage).

## Consequences

- **Positive**: lock-step with the rmcp ecosystem; future `cargo update` will
  not drag us across a major version; new rmcp examples and fixtures work
  without patches.
- **Positive**: wire-format compatibility with the existing set of MCP hosts is
  preserved (we negotiate `V_2025_11_25` by default, which suppresses the new
  `resultType` discriminator).
- **Neutral**: MSRV 1.88 is required for any downstream consumer building from
  source. The crate is published as a binary, so this only affects in-tree
  consumers.
- **Risk**: rmcp 3.x is a single-crate ecosystem update; the 3.0.0 → 3.1.0
  diff is small, but if a 3.x point release changes a trait again, we will need
  to adapt in lock-step. Mitigated by pinning to `"3"` (catches patches only)
  and by the short scope of our manual `ServerHandler` impl (~30 lines of
  method bodies).

## References

- [rmcp CHANGELOG][changelog] — authoritative source for 3.x changes
- [Migration discussion #969][discussion] — stub on the rendered page
  (0 comments as of 2026-08-01); consult the CHANGELOG for migration content
- [`ServerHandler` trait (docs.rs)](https://docs.rs/rmcp/latest/rmcp/handler/server/trait.ServerHandler.html)
- [`Tool` (docs.rs)](https://docs.rs/rmcp/latest/rmcp/model/struct.Tool.html)
- [`Resource` (docs.rs)](https://docs.rs/rmcp/latest/rmcp/model/struct.Resource.html)
- [`ToolAnnotations` (docs.rs)](https://docs.rs/rmcp/latest/rmcp/model/struct.ToolAnnotations.html)
- [`StreamableHttpService` (docs.rs)](https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/tower/struct.StreamableHttpService.html)
- Previous rmcp migration: `CHANGELOG.md` `## [2.19.1] - 2026-07-27` (1.8.0 → 2.1.0)

[changelog]: https://github.com/modelcontextprotocol/rust-sdk/blob/main/crates/rmcp/CHANGELOG.md
[discussion]: https://github.com/modelcontextprotocol/rust-sdk/discussions/969
