# AGENTS.md

Purpose: operating manual for coding agents working in `rusty-intervals-mcp`.

## Quick commands (run these first)

- Format check: `cargo fmt --all -- --check`
- Lint (warnings are errors): `cargo clippy --all-targets --all-features -- -D warnings`
- Full tests: `cargo test --all-targets --all-features`
- CI-compatible tests: `cargo test --all --all-features --no-fail-fast`

## Project snapshot

- Language: Rust; toolchain is `channel = "stable"` in `rust-toolchain.toml`
  (currently 1.99.0; `rust-version` in the mcp manifest tracks the stable
  in use)
- Workspace crates:
  - `crates/intervals_icu_client` — HTTP client, circuit breaker, upstream
    metrics + failure logging, API compatibility helpers
  - `crates/intervals_icu_mcp` — MCP server, 9 intent handlers (including
    `track_progress`), dynamic OpenAPI tool registry, prompts/resources,
    request correlation (`request_id` / real client IP) and session
    lifecycle logging (`session.rs`)
- Important docs:
  - `README.md` — setup, runtime usage, token-efficiency behavior, env vars
    (incl. `RUST_LOG_FORMAT` / `RUST_LOG_COLOR`)
  - `docs/ARCHITECTURE.md` — runtime architecture, layering, metrics list
  - `docs/HTTP_RELIABILITY.md` — rate limiting, keepalive, graceful shutdown,
    request correlation, real client IP behind proxies, log-level policy
  - `RELEASING.md` — release checklist (annotated `vX.Y.Z — title` tags)
  - `CONTEXT.md` + `docs/adr/` — domain vocabulary and decisions
  - `CONTRIBUTING.md` — contribution and local checks
  - `.github/workflows/ci.yml` — authoritative CI checks
- **`Cargo.lock` is gitignored** — never stage it.

## How to work in this repo

1. Read relevant docs and existing code paths before editing.
2. Prefer small, surgical changes over broad refactors.
3. Preserve existing public behavior unless the task explicitly asks to change it.
4. After edits, run quality gates (fmt, clippy, tests).
5. If tests fail, fix root cause instead of weakening checks.

## Build/test targets by intent

- Entire workspace (default):
  - `cargo fmt --all -- --check`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo test --all-targets --all-features`
- MCP crate only:
  - `cargo test -p intervals_icu_mcp --lib`
  - `cargo test -p intervals_icu_mcp --test e2e_http -- --nocapture`
- Client crate only:
  - `cargo test -p intervals_icu_client --tests`

## Code style and conventions

- Use idiomatic Rust:
  - Prefer `?` for error propagation.
  - Avoid `unwrap()`/`expect()` in non-test code.
  - Keep functions focused and composable.
- Keep APIs and naming explicit; avoid ambiguous abbreviations.
- Add/maintain `///` docs for public types/functions.
- Use trait-based abstractions for testability (e.g., `IntervalsClient`).
- Follow existing module boundaries instead of introducing parallel architecture.
- **Modules use the pathless layout**: `<name>.rs` next to `<name>/`.
  `mod.rs` files are legacy here — allowed only inside `tests/*/mod.rs`.
- Logging: structured `key=value`, UTC-millis timestamps; levels follow the
  policy in `docs/HTTP_RELIABILITY.md` (info = business/audit events,
  warn = operator-actionable failures, debug = routine per-request detail).

### Good pattern example

```rust
pub async fn load_registry(client: &dyn IntervalsClient) -> Result<DynamicRegistry, IntervalsError> {
    let spec = client.fetch_openapi_spec().await?;
    DynamicRegistry::from_openapi(&spec)
}
```

### Avoid

```rust
pub async fn load_registry(client: &dyn IntervalsClient) -> DynamicRegistry {
    let spec = client.fetch_openapi_spec().await.unwrap();
    DynamicRegistry::from_openapi(&spec).unwrap()
}
```

## Testing expectations

- Add or update tests for behavior you change.
- Prefer unit tests near implementation; use integration/e2e tests for cross-module behavior.
- Do not delete failing tests to make CI green.
- Do not silently weaken assertions without explicit task requirement.

## Security and secrets

- Never commit secrets, API keys, or tokens.
- Treat `.env` as sensitive local configuration.
- Do not print secret values in logs, docs, tests, or commit messages.
- If a task needs new env vars, document placeholders in `.env.example`.

## Git and change hygiene

- Keep commits and diffs minimal and task-scoped.
- Avoid unrelated formatting churn.
- Preserve file structure and naming conventions already used in the workspace.
- When behavior changes, update relevant docs in `README.md` or `docs/`.
- Never touch `Cargo.lock` (gitignored) or other local-only ignored files
  (sections of `docs/`, `.env*`).
- Never stage or revert another actor's uncommitted in-progress work.

## Boundaries

### Always

- Run formatting, lint, and tests before finalizing.
- Use only llvm-cov for coverage; do not switch Tarpallin or other tools.
- Reuse existing patterns from `crates/intervals_icu_client` and `crates/intervals_icu_mcp`.
- Prefer retrieval-led reasoning: consult repo docs/code over assumptions.

### Ask first

- Adding/removing dependencies.
- Changing public API shape or wire formats.
- Modifying CI/release workflows, Docker publishing, or deployment behavior.
- Large architectural refactors spanning both crates.

### Never

- Commit `.env` secrets or private credentials.
- Bypass lint/tests by muting warnings or deleting tests.
- Rewrite major subsystems when a small fix suffices.
- Edit unrelated files “while here”.
## Agent skills

### Issue tracker

Issues and specs are tracked in GitHub Issues. See `docs/agents/issue-tracker.md`.

### Triage labels

Five canonical roles mapped to labels with strings matching their names. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context — one `CONTEXT.md` + `docs/adr/` at the repo root. See `docs/agents/domain.md`.

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
