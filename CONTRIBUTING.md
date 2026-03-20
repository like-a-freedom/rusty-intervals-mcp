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

1. **Define the intent contract** in `INTENT_DRIVEN_SKILLS.md`:
   - Business purpose (outcome, not operation)
   - Input parameters (flattened, primitives only)
   - Output format (Markdown + guidance hints)
   - Business rules and validation

2. **Implement IntentHandler**:
   ```rust
   pub trait IntentHandler: Send + Sync {
       fn name(&self) -> &'static str;
       fn description(&self) -> &'static str;
       fn input_schema(&self) -> serde_json::Value;
       fn execute(&self, input: Value, client: Arc<dyn IntervalsClient>) -> Result<IntentOutput, IntentError>;
   }
   ```

3. **Follow key principles**:
   - **Outcomes, Not Operations**: One intent = one business outcome
   - **Flatten Arguments**: Only primitives at top level (no nested dicts)
   - **Business Identifiers**: Use `date`, `description` instead of `event_id`
   - **Tool Response as Prompt**: Include `suggestions` and `next_actions`
   - **Idempotency**: Require `idempotency_token` for mutating operations
   - **NO LLM inside**: Deterministic Rust logic only

4. **Add tests**:
   - Unit tests for validation and business rules
   - Integration tests for orchestration
   - Performance benchmarks (target: <3s avg latency)

### Modifying Existing Intents

- Preserve backward compatibility when possible
- Update `INTENT_DRIVEN_SKILLS.md` with changes
- Add migration notes to `CHANGELOG.md`
- Bump major version for breaking changes

### Token Efficiency

- Prefer 8 intents over 146 low-level tools
- Use compact responses by default
- Aggregate data instead of raw payloads
- Include guidance hints in all responses

## Development

- Run a single crate tests: `cargo test -p intervals_icu_client` or
  `cargo test -p intervals_icu_mcp --test e2e_http` for integration tests.
- Build release binary: `cargo build -p intervals_icu_mcp --release`

## Testing Intent Handlers

```rust
#[tokio::test]
async fn test_plan_training_intent() {
    let client = MockIntervalsClient::new();
    let handler = PlanTrainingHandler;
    
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-06-15",
        "focus": "aerobic_base",
        "idempotency_token": "test-token"
    });
    
    let result = handler.execute(input, Arc::new(client)).await.unwrap();
    
    assert!(!result.suggestions.is_empty());
    assert!(!result.next_actions.is_empty());
    assert!(result.metadata.events_created > 0);
}
```

## Releases & Binaries

Releases are published using GitHub Actions and produce pre-built binaries for
Linux (x86_64/aarch64), macOS (x86_64/aarch64) and Windows (x86_64).

When preparing a release manually:
1. Ensure all checks pass locally (format, clippy, tests).
2. Update `CHANGELOG.md` with intent changes and migration notes.
3. Update `INTENT_DRIVEN_SKILLS.md` if intent specifications changed.
4. Create a GitHub release (tag `vX.Y.Z`) — the workflow will produce and
   attach binary artifacts and checksums.

**Version numbering:**
- Major version (v2.0.0): Breaking changes to intent API
- Minor version (v2.1.0): New intents or features, backward compatible
- Patch version (v2.1.1): Bug fixes, no API changes

Note on container publishing

- The release workflow builds and pushes multi-architecture images to **GHCR** by
  default (using `GITHUB_TOKEN`). The image path is
  `ghcr.io/<your-org-or-user>/rusty-intervals-mcp` and tagged with the release
  tag and `latest` as applicable.

Security notes

- Do not store secrets in the repository. Use GitHub repository secrets for
  `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` (or a personal access token with
  the appropriate scope).
- GHCR publishing uses `GITHUB_TOKEN` and requires `packages: write` permission
  which the workflow already requests.

## Code style

- Keep changes focused and small.
- Use idiomatic Rust (prefer `?` operator, avoid `unwrap()` in non-tests).
- Add documentation comments for public types/functions.
- Run the strict format/lint/test gate before pushing.
- **Intent-specific**:
  - Document all 8 intents in `INTENT_DRIVEN_SKILLS.md`
  - Include `///` doc comments for public intent handlers
  - Preserve existing public behavior unless explicitly changed

---

If you have questions about project structure or design choices, prefer opening
an issue or discussion before a large design change.