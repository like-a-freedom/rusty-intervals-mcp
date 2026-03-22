# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2025-12-16
- Add release checklist and container packaging docs.
- Add GitHub Actions CI: run `cargo fmt`, `cargo clippy` (fail on warnings), and `cargo test` on PRs and pushes; add release build job that produces cross-platform binaries and checksums for Linux, macOS, and Windows.
- Add multi-architecture Docker image publishing to GitHub Container Registry (GHCR) on release using Docker Buildx and BuildKit (`ghcr.io/<org-or-user>/rusty-intervals-mcp`). Docker Hub publish option removed.
- Add `CONTRIBUTING.md` and `docs/REPOSITORY_STRUCTURE.md` to document development workflow, local checks, and repo layout.
- Remove non-actionable placeholder comments, add actionable TODOs where appropriate, and fix clippy warnings & formatting issues.
- Add unit tests asserting tool registration and update documented tool count to 53 across `README.md`, `docs/SPEC.md`, and `docs/IMPLEMENTATION_PLAN.md`.
- Ensure full test suite passes locally after changes.

## [0.3.12] - 2025-12-17
- Chore: Bump crate versions to 0.3.12 and prepare release artifacts.
- Fix: Address clippy warnings and formatting; inline webhook tests and remove duplicate external test files.

## [1.0.0] - 2026-02-23
- Bump crate versions to 1.0.0 for initial stable release.

## [2.0.0] - 2026-03-07

### Changed
- Bump crate versions to 2.0.0 for both `intervals_icu_client` and `intervals_icu_mcp`.
- Major version bump reflects completed dynamic OpenAPI runtime alignment, athleteId auto-injection fix, and extensive test hardening.

## [2.1.0] - 2026-03-22

### Added
- **Prometheus metrics for MCP observability** (HTTP mode only). New `/metrics` endpoint exposes:
  - Upstream (intervals.icu) request duration, count, and error metrics
  - MCP protocol layer: tool calls, method calls, duration histograms
  - HTTP transport: request count, duration, active request gauge
  - Auth layer: token issuance, verification (valid/invalid/expired), auth failures
  - Active athletes gauge (no high-cardinality labels)
- Optional `PROMETHEUS_METRICS_TOKEN` for `/metrics` endpoint authentication
- `docs/METRICS.md`: full metrics SRS document with all metric definitions, alert examples, and design decisions
- `intervals_icu_client`: add `metrics` crate dependency and instrument upstream HTTP calls (`execute_json`, `execute_text`, `execute_empty`) with duration histograms and error counters
- `intervals_icu_mcp`: add `record_auth_failure`, `record_mcp_session`, `record_mcp_method_call` metric functions
- `intervals_icu_mcp`: add `endpoint` label to `rate_limited_total` metric
- `intervals_icu_mcp`: expand `token_verifications_total` labels to include `expired` status
- `intervals_icu_mcp`: add metrics integration tests (`tests/metrics_tests.rs`)
- `intervals_icu_client`: add spec-aligned weather config and athlete route client methods, plus `update_wellness_bulk` for the current wellness bulk endpoint.
- `intervals_icu_client`: add an ignored live OpenAPI smoke test in `src/http_client.rs` that validates critical client contracts against `https://intervals.icu/api/v1/docs`.
- `intervals_icu_mcp`: add dynamic registry regression coverage for current-spec athlete path placeholders, including `/api/v1/athlete/{athleteId}/sport-settings/{id}/apply`.
- `intervals_icu_mcp`: add deterministic runtime coverage for explicit OpenAPI source failures and successful loading/building from the checked-in fallback spec.

### Changed
- Move new unit-level contract regression coverage inline into `crates/intervals_icu_client/src/http_client.rs` and remove standalone test files for those checks.
- Document the strict workspace validation gate as `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-targets --all-features`.
- `intervals_icu_mcp`: treat `athleteId` as an auto-injected athlete path parameter in the dynamic OpenAPI runtime, but stop auto-injecting nested resource ids such as `/sport-settings/{id}`.
- `intervals_icu_mcp`: replace several production-adjacent test mocks that previously panicked via `unimplemented!()` with deterministic event stubs so MCP/client alignment regressions fail informatively.
- `intervals_icu_mcp`: intent tools now return schema-only MCP results (`structuredContent` + empty `content`) instead of duplicating the same payload as markdown text, improving token efficiency and matching the declared output contract.

### Added
- Token-efficiency features: `get_activity_streams` now supports `max_points` (downsample arrays), `summary` (return statistics instead of arrays), and `streams` filtering to reduce tokens when returning long time-series.
- `get_activity_details` gained `expand` (boolean) and `fields` (array) parameters. By default the MCP now returns a compact activity summary; set `expand=true` to fetch full payload when needed.
- Shortened tool descriptions and prompt templates to reduce token overhead in tool metadata and prompt injection.

### Fixed / Notes
- Tests added for sampling, summary stats, and compact activity extraction. All tests updated to reflect new API shapes and behavior.

### Breaking changes
- `get_activity_details` defaults to a compact summary (previously returned the full details by default). If your integrations rely on the full payload, update calls to `get_activity_details` to pass `{"expand": true}`.

## [Unreleased]

## [0.1.0] - 2025-12-15
- Initial MCP-compatible Intervals.icu client in Rust.
- Auth via API key and athlete id, configurable base URL.
- Activity listing, streams, intervals, best-efforts, GAP histogram.
- Event CRUD with `start_date_local` and expanded categories.
- Gear, sport settings, power curves support.
- File download helper with streaming to disk.
- Integration tests using wiremock and runnable examples.
- Criterion benchmark for streaming download path.
