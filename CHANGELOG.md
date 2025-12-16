# Changelog

All notable changes to this project will be documented in this file.

## [0.2.0] - 2025-12-16
- Add release checklist and container packaging docs.
- Add GitHub Actions CI: run `cargo fmt`, `cargo clippy` (fail on warnings), and `cargo test` on PRs and pushes; add release build job that produces cross-platform binaries and checksums for Linux, macOS, and Windows.
- Add multi-architecture Docker image publishing to GitHub Container Registry (GHCR) on release using Docker Buildx and BuildKit (`ghcr.io/<org-or-user>/rusty-intervals`). Docker Hub publish option removed.
- Add `CONTRIBUTING.md` and `docs/REPOSITORY_STRUCTURE.md` to document development workflow, local checks, and repo layout.
- Remove non-actionable placeholder comments, add actionable TODOs where appropriate, and fix clippy warnings & formatting issues.
- Add unit tests asserting tool registration and update documented tool count to 53 across `README.md`, `docs/SPEC.md`, and `docs/IMPLEMENTATION_PLAN.md`.
- Ensure full test suite passes locally after changes.

## [Unreleased]

- Fix: `get_event` now returns a descriptive decoding error (includes a short snippet of the response body) when the API returns a payload that doesn't match the `Event` schema (e.g., when an `activity_id` is passed by mistake). See `docs/SPEC.md` and `docs/OPENAPI_EVENT_FIX.md` for details.
- Fix: include required `type` (sport) query parameter in `get_power_curves`, `get_hr_curves`, and `get_pace_curves` client methods and MCP tools to avoid 422 responses from the upstream API. Added tests asserting the `type` and `curves` query parameters are sent.

## [0.1.0] - 2025-12-15
- Initial MCP-compatible Intervals.icu client in Rust.
- Auth via API key and athlete id, configurable base URL.
- Activity listing, streams, intervals, best-efforts, GAP histogram.
- Event CRUD with `start_date_local` and expanded categories.
- Gear, sport settings, power curves support.
- File download helper with streaming to disk.
- Integration tests using wiremock and runnable examples.
- Criterion benchmark for streaming download path.
