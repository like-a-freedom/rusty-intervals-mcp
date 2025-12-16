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

## [Unreleased]

## [0.3.0] - 2025-12-16
- Fix: `get_event` now returns a descriptive decoding error (includes a short snippet of the response body) when the API returns a payload that doesn't match the `Event` schema (e.g., when an `activity_id` is passed by mistake). See `docs/SPEC.md` and `docs/OPENAPI_EVENT_FIX.md` for details.
- Fix: include required `type` (sport) query parameter in `get_power_curves`, `get_hr_curves`, and `get_pace_curves` client methods and MCP tools to avoid 422 responses from the upstream API. Added tests asserting the `type` and `curves` query parameters are sent.
- Fix: event tools now accept numeric event IDs (as required by the Intervals.icu API) and normalize them to strings, preventing MCP parameter deserialization errors for `update_event`, `duplicate_event`, and `bulk_delete_events`.
- Fix: align activity search endpoints to `/activities/search` and `/activities/search-full` with required `query` validation across client, MCP tools, and tests.
- Fix: interval search now requires `minSecs`, `maxSecs`, `minIntensity`, and `maxIntensity`, and supports optional `type`, `minReps`, `maxReps`, and `limit` parameters; tests updated to cover new query payload.
- Fix: calendar bulk delete uses `PUT /events/bulk-delete` with `{id|external_id}` items; event duplication uses `POST /duplicate-events` with `numCopies` and `weeksBetween` and returns the created events list.
- Fix: gear reminder updates use singular endpoint `/gear/{gearId}/reminder/{reminderId}` and propagate `reset`/`snoozeDays` query flags; sport settings update requires `recalcHrZones`, and applying sport settings uses `PUT /sport-settings/{id}/apply` without `start_date`.
- Fix: workout library now reads folders via `/athlete/{id}/folders` and workouts via `/athlete/{id}/workouts`, filtering by `folder_id` client-side to match the published API.

## [0.3.1] - 2025-12-16
- Fix: `search_activities` and `search_activities_full` now validate that the `q` parameter is non-empty and forward it as `q` (the upstream API expects `q`, not `query`), preventing 422 Unprocessable Entity responses; tests added to assert `q` is sent.

## [0.3.2] - 2025-12-16
- Fix: Make integration `download_activity_file_returns_base64_and_writes_file` test robust against transient empty responses on CI by adding short retries and better diagnostics; prevents spurious CI failures. Updated CI to run the specific integration target with verbose output for clearer diagnostics.

## [0.3.3] - 2025-12-16
- Fix: Address clippy lint `expect_fun_call` in integration test by avoiding function calls inside `expect`. This cleans up the test and satisfies `cargo clippy` in CI.

## [0.3.4] - 2025-12-16
- Fix: Update CI to use `houseabsolute/actions-rust-cross@v1` (instead of `main`) for cross builds to avoid action resolution errors on the Windows runner; add a note and ensure workflows reference an explicit stable action tag.

## [0.3.5] - 2025-12-16
- Chore: Bump GitHub Actions to pinned, modern releases (actions/checkout@v6, docker actions v3/v5/v6 series, houseabsolute/actions-rust-cross@v1.0.5, softprops/action-gh-release@v2.5.0) to avoid 'unable to find version' errors and improve runner compatibility.

## [Unreleased]

- No changes yet.

## [0.1.0] - 2025-12-15
- Initial MCP-compatible Intervals.icu client in Rust.
- Auth via API key and athlete id, configurable base URL.
- Activity listing, streams, intervals, best-efforts, GAP histogram.
- Event CRUD with `start_date_local` and expanded categories.
- Gear, sport settings, power curves support.
- File download helper with streaming to disk.
- Integration tests using wiremock and runnable examples.
- Criterion benchmark for streaming download path.
