# Contributing

Thanks for helping improve Rusty-Intervals. This document explains how to run
local checks, tests, and how releases are produced.

## Local checks (pre-commit)

- Format: `cargo fmt --all -- --check`
- Lint: `cargo clippy --all-targets --all-features -- -D warnings`
- Tests: `cargo test --all-targets --all-features`

For `intervals_icu_client`, keep unit-level regression tests inline in the relevant source files. Use integration tests only when the behavior truly spans multiple public modules or requires a dedicated black-box harness.

When changing Intervals.icu client contract paths or verbs, also run the ignored live OpenAPI smoke test in `crates/intervals_icu_client/src/http_client.rs` to validate the client assumptions against `https://intervals.icu/api/v1/docs`.

We try to keep changes small, obvious and well-tested. Please add unit tests
for new functionality and run the full suite before opening PRs.

## Intent-Driven Development (v2.0+)

Starting with version 2.0, this project implements an **intent-driven architecture**. When contributing, follow these guidelines:

### Adding New Intents

1. **Define the intent contract**:
   - Business purpose (outcome, not operation)
   - Input parameters (flattened, primitives only)
   - Output format (Markdown + guidance hints)
   - Business rules and validation

2. **Implement [`IntentHandler`](crates/intervals_icu_mcp/src/intents/types.rs)**:
   follow an [existing handler](crates/intervals_icu_mcp/src/intents/handlers/plan_training.rs)
   for the async interface and business validation.

3. **Follow key principles**:
   - **Outcomes, Not Operations**: One intent = one business outcome
   - **Flatten Arguments**: Only primitives at top level (no nested dicts)
   - **Business Identifiers**: Use `date`, `description` instead of `event_id`
   - **Tool Response as Prompt**: Include `suggestions` and `next_actions`
   - **Idempotency**: Preserve the `idempotency_token` and durable replay contract of `plan_training` and `modify_training` (see [ADR 0012](docs/adr/0012-single-process-sqlite-state.md)).
   - **NO LLM inside**: Deterministic Rust logic only

4. **Add tests**:
   - Unit tests for validation and business rules
   - Integration tests for orchestration
   - Performance benchmarks (target: <3s avg latency)

### Modifying Existing Intents

- Preserve backward compatibility when possible
- Add migration notes to `CHANGELOG.md`
- Bump major version for breaking changes

### Token Efficiency

- Prefer the nine business intents over low-level upstream operations
- Use compact responses by default
- Aggregate data instead of raw payloads
- Include guidance hints in all responses

## Development

- Run a single crate tests: `cargo test -p intervals_icu_client` or
  `cargo test -p intervals_icu_mcp --test e2e_http` for integration tests.
- Build release binary: `cargo build -p intervals_icu_mcp --release`

## Testing Intent Handlers

See the [`plan_training` unit tests](crates/intervals_icu_mcp/src/intents/handlers/plan_training/tests.rs)
for input and outcome checks, and the [HTTP integration tests](crates/intervals_icu_mcp/tests/http_mode_integration.rs)
for authenticated calls and mutation replay.

## Releases & Binaries

Releases are published using GitHub Actions and produce pre-built binaries for
Linux (x86_64/aarch64), macOS (x86_64/aarch64) and Windows (x86_64).

The full step-by-step release guide — version numbering, checklist, tag/push,
GHCR publishing — lives in [`RELEASING.md`](RELEASING.md). In short:

Run the release quality gates, update `CHANGELOG.md`, and bump the server
version. Push the annotated tag, then create the GitHub release from it.
The release event triggers the build and publishing workflow; a tag push alone does not.

Security: do not store secrets in the repository; GHCR publishing uses
`GITHUB_TOKEN` and the workflow already requests `packages: write`.

## Code style

- Keep changes focused and small.
- Use idiomatic Rust (prefer `?` operator, avoid `unwrap()` in non-tests).
- Add documentation comments for public types/functions.
- Run the strict format/lint/test gate before pushing.
- **Intent-specific**:
  - Document all 9 intents
  - Include `///` doc comments for public intent handlers
  - Preserve existing public behavior unless explicitly changed

---

If you have questions about project structure or design choices, prefer opening
an issue or discussion before a large design change.