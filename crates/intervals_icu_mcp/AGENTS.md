# AGENTS.md

Purpose: crate-level operating manual for coding agents working in `crates/intervals_icu_mcp`.

## Scope and precedence

- This file applies to everything under `crates/intervals_icu_mcp/`.
- It complements the repository-root `AGENTS.md`.
- If instructions conflict, follow the closest file in the directory tree (this one for crate files).

## Fast validation commands (crate-focused)

- Format (workspace-safe): `cargo fmt --all -- --check`
- Lint (strict): `cargo clippy -p intervals_icu_mcp --all-targets --all-features -- -D warnings`
- Unit/lib tests: `cargo test -p intervals_icu_mcp --lib`
- Integration tests: `cargo test -p intervals_icu_mcp --tests --all-features`
- HTTP E2E: `cargo test -p intervals_icu_mcp --test e2e_http -- --nocapture`
- STDIO E2E: `cargo test -p intervals_icu_mcp --test e2e_stdio -- --nocapture`

## Crate mission

`intervals_icu_mcp` is the MCP server crate that:
- implements `IntervalsMcpHandler` (RMCP `ServerHandler`),
- dynamically builds tool definitions from OpenAPI (`dynamic` module),
- dispatches OpenAPI operations via `IntervalsClient`,
- exposes prompts/resources for training analytics workflows,
- supports HTTP/stdio MCP usage.

## Important files and modules

- `src/lib.rs` — MCP handler, prompt/resource routing, dynamic dispatch entry.
- `src/main.rs` — CLI/server startup path.
- `src/dynamic/` — OpenAPI parsing, registry generation, dispatch runtime.
- `src/compact.rs` — compact response shaping/token-efficiency behavior.
- `src/domains/` — domain-specific prompt/resource/tool helpers (including `progress.rs` for progress tracking domain types).
- `src/engines/` — deterministic analytics: `changepoint.rs` (CTL plateau detection), `progress_tracking.rs` (progress report assembly, TID drift, hypotheses), `coach_metrics.rs` (load/HRV helpers), and many more.
- `src/intents/handlers/` — 9 intent handlers including `track_progress.rs` (progress tracking) and `render/progress.rs` (progress report markdown rendering).
- `tests/e2e_http.rs` and `tests/e2e_stdio.rs` — end-to-end contract tests.
- `tests/dispatch_tests.rs` and `tests/dynamic_registry_tests.rs` — dynamic tool/runtime correctness.
- `tests/progress_tracking_integration.rs` — progress engine-level integration tests.

## Implementation rules

- Preserve dynamic OpenAPI-first behavior; avoid reintroducing hardcoded static tool maps.
- Keep MCP contract stable (`list_tools`, `call_tool`, prompts/resources behavior).
- Prefer additive changes with backward compatibility in tool argument handling.
- Use `?` and typed errors; do not use `unwrap()`/`expect()` in non-test code.
- Keep token-efficiency features (`compact`, field filtering, summary pathways) intact unless explicitly changing behavior.

## Testing expectations

- Any change in dynamic parsing/dispatch must update or add tests in:
  - `tests/dynamic_registry_tests.rs`,
  - `tests/dispatch_tests.rs`,
  - and relevant e2e tests when wire behavior changes.
- Any prompt/resource routing change in `src/lib.rs` should include coverage for the updated route or argument handling.
- Never remove failing assertions to make tests pass.

## Environment and runtime notes

- Required runtime vars for real API calls:
  - `INTERVALS_ICU_API_KEY`
  - `INTERVALS_ICU_ATHLETE_ID`
- Dynamic registry tuning vars:
  - `INTERVALS_ICU_OPENAPI_SPEC`
  - `INTERVALS_ICU_SPEC_REFRESH_SECS`
- Logging:
  - `RUST_LOG` (preferred)

## Code Navigation Rules

ALWAYS use octocode MCP tools before reading files directly:
- Use `semantic_search` to find relevant code by meaning
- Use `view_signatures` to understand file structure
- Use `graphrag` to explore dependencies between files
- Use `structural_search` for AST-level pattern search (replaces grep/rg)

NEVER:
- Run grep, rg, find to locate code — use semantic_search instead
- Read entire files to understand structure — use view_signatures instead
- Guess file locations — use graphrag overview first

WORKFLOW for any task:
1. graphrag overview → understand project structure
2. semantic_search → find relevant files
3. view_signatures → inspect structure of found files
4. Read only specific sections if needed

## Boundaries

### Always

- Read current code paths before refactoring (`src/lib.rs` + relevant `src/dynamic/*`).
- Run strict lint and relevant tests before finalizing.
- Keep changes minimal and focused on the requested behavior.

### Ask first

- Changes to public MCP behavior or wire-format semantics.
- New dependencies in `Cargo.toml`.
- Major changes to prompt set, resource URIs, or transport behavior.

### Never

- Hardcode secrets or log API keys/tokens.
- Bypass failing checks by suppressing warnings globally.
- Replace dynamic registry architecture with a static tool list.
- Modify unrelated workspace crates/files “while here”.
