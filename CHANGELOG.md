# Changelog

All notable changes to this project will be documented in this file.

## [2.23.1] - 2026-09-22

### Fixed
- Fitness parsing from multi-day arrays resolves the latest day by date instead of taking the first object, so stale CTL/ATL/TSB can no longer pin when the API order puts an older day first (fully undated payloads keep the first object).
- Resting-HR and HRV sensor artifacts are now filtered like implausible sleep: dropouts (0), negatives, RHR outside 20–130 bpm, and HRV above 500 ms no longer corrupt recent averages, rolling baselines, personal-baseline observations, the lnRMSSD rollup input, or the training-plan snapshot.
- Rolling HRV/RHR baselines require at least 7 samples: shorter windows left deviation, ratio, and recovery-index denominators to single-day noise.
- Recovery Quality Index clamps the HRV ratio into the shared component band, so a glitchy-baseline day can no longer explode the score (e.g. 30x ratio displaying as "12.60").
- TSB band edge parity: exactly -10.0 renders Balanced, matching `interpret_fitness_metrics` (fatigued only strictly below -10).
- Personal-baseline duplicates resolve last-wins (later syncs correct earlier ones); the resting-HR arm drops non-positive/non-finite observations like the HRV arm.
- `wellness_days_count` counts days with data instead of raw rows, so empty entries no longer inflate it.

## [2.23.0] - 2026-09-22

### Changed
- Internal crypto helper refactor: `fill_random` now returns a fixed-size array (`fill_random::<12>()`), removing the zero-initialized AES-GCM nonce buffer from `encrypt_api_key` that CodeQL flagged as a hard-coded cryptographic value. No behavior change — the nonce is still filled by the OS CSPRNG.

## [2.22.1] - 2026-09-22

### Fixed
- Recovery table no longer fabricates readings for missing metrics (`0.0 hrs / Poor`): absent sleep, resting HR, HRV, and TSB now render as `n/a`, and zero sleep/RHR/HRV is treated as missing (0.0 remains a legitimate balanced TSB).
- Training-plan wellness snapshot resolves the latest entry by date instead of array position, so newest-first API responses no longer yield the oldest day; it shares the sleep pipeline (constants, plausibility filter, legacy `sleep` key) with the wellness parser.
- Recovery Quality Index stays `None` when sleep is entirely implausible instead of silently defaulting to neutral 7h.
- Progress-tracking baselines accept the canonical resting-HR key set (`resting_hr_bpm`, `avgSleepingHR`).
- Entry ordering is no longer defeated by a single dateless row: dated entries sort oldest-first, dateless ones sort first.

## [2.22.0] - 2026-09-22

### Fixed
- Inflated sleep metrics (e.g. "Sleep = 12.8 h/day"): wellness sleep values outside the plausible 1.0–12.0 h range are now excluded from `avg_sleep_hours`; when every sample is an artifact the field is `None` instead of a bogus average.
- Wellness date resolution: entries from the real Intervals.icu API carry the day in `id`, not `date` — `extract_wellness_observations`, `extract_ctl_series`, `extract_hrv_series`, and the `track_progress` personal-baseline extraction now fall back to `id`.
- Wellness entry order is now normalized oldest-first before splitting the recent window from the baseline, so newest-first API responses no longer skew recent averages.
- `plan_training` wellness snapshot now reads `sleepSecs`/`sleep_secs`/`sleep_hours` (converting seconds > 24 to hours) instead of only the normalized `sleep` key.

### Added
- Regression tests for implausible-sleep filtering (including the 12.0/12.1 h boundary), `id`-as-date handling in CTL/HRV series and personal baselines, entry-order normalization, `sleepSecs` snapshot parsing, and the missing-sleep recovery row rendering.

## [2.21.0] - 2026-08-02

### Changed
- Migrate `rmcp` from 2.x to 3.x (major version bump). `ServerHandler::call_tool` / `read_resource` now return `CallToolResponse` / `ReadResourceResponse` (always the `Complete` variant), and `ListToolsResult` / `ListResourcesResult` are built with `with_all_items` for the new `result_type` / `ttl_ms` / `cache_scope` fields.
- Bump MSRV to 1.88 (declared by rmcp 3.x). `rust-version = "1.88"` added to `intervals_icu_mcp/Cargo.toml`.
- Wire format unchanged for existing MCP clients: the server still negotiates `V_2025_11_25` by default, so no changes to tool/resource schemas or response shapes are required on the client side.

## [2.20.0] - 2026-07-28

### Added
- Comprehensive unit tests for `adaptation.rs` (18.92% → 100%), `heat.rs` (52.73% → 100%), `test_support.rs` (63.28% → 98.97%), `metrics.rs` (50.32% → 84.71%)
- Unit tests for `AthleteTracker`, `init_prometheus_recorder`, and all `record_*` metrics functions
- Unit tests for `content_text` helper and all MockIntervalsClient scenario constructors
- Unit tests for `clone_intervals_error` covering all error variants (Transport, Config, Validation, Io, Cancelled, Decode)
- Unit tests for `IntervalsMcpHandler` methods: `new_multi_tenant`, `webhook_service`, `process_webhook`, `preload_dynamic_registry`
- Fix: `e2e_stdio` test now explicitly sets `MCP_TRANSPORT=stdio` to prevent inheriting host env

### Fixed
- `e2e_stdio` integration test hung when `MCP_TRANSPORT=http` was set in the host environment

### Changed
- Documentation cleanup: removed stale references to non-existent `INTENT_DRIVEN_SKILLS.md` and `docs/OBSERVABILITY_SRS.md`
- Documentation cleanup: fixed intent count from 8 to 9 in CONTRIBUTING.md
- Removed completed planning artifacts: `.scratch/`, `.superpowers/`, superseded superpowers plans
- Updated `docs/agents/issue-tracker.md` to reflect GitHub Issues (replaced `.scratch/` workflow)

## [2.19.1] - 2026-07-27

### Changed
- Migrate `rmcp` from 1.8.0 to 2.1.0 (major version bump).
- Replace `RawResource` with `Resource` (renamed in rmcp 2.0).
- Remove unused `AnnotateAble` trait import and `.no_annotation()` calls (removed in rmcp 2.0).
- Update all workspace dependencies to latest compatible versions via `cargo update`.

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

## [2.13.0] - 2026-07-04

### Fixed
- Corrected historical activity fetch bounds: `fetch_period_data` now anchors lookback to today (not window width), removes the internal 200-activity cap, and filters summaries to the requested date window before fetching details.
- Detail requests are now scoped to the required current/previous windows; partial failures produce explicit `fetch_warnings` with exact failed/succeeded counts.
- `analyze_training` period calls with `target_type="period"` now reject mistaken `start_date`/`end_date` usage with actionable guidance naming `period_start`/`period_end`.
- `compare_periods` validates both date ranges before network I/O and fetches independent periods concurrently via `tokio::try_join!`.
- Empty-period guidance in `analyze_training` now states the uncapped query succeeded and recommends checking the range or using `compare_periods`, instead of speculative device-sync advice.

### Added
- Integration regressions for historical YoY comparison (Q2 2025 vs Q2 2026), high-volume period (250 activities), and partial-detail failure scenarios.
- Intent-routing documentation: `track_progress` limited to trailing 4–24 weeks; `compare_periods` handles YoY and quarter-over-quarter; `analyze_training` reserves `period_start`/`period_end` for `target_type="period"`.

## [Unreleased]

### Added
- **Progress tracking engine**: new `track_progress` MCP intent that detects trailing CTL plateaus via changepoint analysis (linear regression + athlete-aware flat-band personalization), summarizes load-management context (ACWR, monotony, strain), surfaces TID drift (Shannon entropy delta over rolling 4-week windows, drift classification, dominant zone), computes lnRMSSD 7-day rollup, and emits evidence-weighted coaching hypotheses (volume, intensity distribution, recovery) with confidence scores.
- New domain types in `domains/progress.rs`: `TrendState`, `TidDriftState`, `HypothesisDomain`, `ChangepointResult`, `LnRmssdRollup`, `TidDriftMetrics`, `ProgressHypothesis`, `ProgressReport`.
- New changepoint engine in `engines/changepoint.rs`: trailing CTL plateau detection with athlete-aware flat-band estimation, backward step search, and linear regression slope.
- New progress orchestration engine in `engines/progress_tracking.rs`: weekly zone distribution grouping, TID drift computation, hypothesis ranking, progress report assembly.
- New `track_progress` MCP handler in `intents/handlers/track_progress.rs` with wellness and activity fetching.
- New progress report renderer in `intents/handlers/render/progress.rs` with plateau detection, load context, HRV context, lnRMSSD rollup, TID drift, hypotheses, and warnings sections.
- CTL/HRV extraction helpers in `coach_metrics.rs`: `extract_ctl_series`, `extract_hrv_series`, `compute_lnrmssd_rollup`, `compute_tid_entropy`.
- Wiring fix: `compute_heat_metrics_7d` now called in `analyze_training` period analysis path (previously always returned None).
- Wiring fix: wellness context (HRV, sleep, RHR, HRV ratio, recovery index) now rendered in `analyze_race` output (previously fetched and parsed but discarded).
- Wiring fix: `hrv_trend_slope`, `recovery_quality_index`, and `hrv_suppression_flag` now rendered in `assess_recovery` metrics table.
- Wiring fix: ESPE derived metrics (`aerobic_durability`, `durability_gradient`, `balance_score`, `vo2_reserve_ratio`) now rendered in `render_espe_section` (previously computed but hidden).
- All analytical MCP outputs now include inline metric explanations (parenthetical context for monotony, strain, stress tolerance, fatigue index, WDRM, NDLI, ISDM signed decoupling, EF halves, eFTP, W′, pMax, efficiency factor, HRV ratio, recovery index, lnRMSSD, TID entropy).

### Changed
- Major magic number refactor across `changepoint.rs`, `progress_tracking.rs`, and `track_progress.rs`: 17 hardcoded literals extracted to named constants.
- `track_progress` tool description expanded per MCP design skill guidelines (when to use, when NOT to use, argument descriptions, return shape).
- Renderer test and handler test strengthened to verify all output sections.

## [0.1.0] - 2025-12-15
- Initial MCP-compatible Intervals.icu client in Rust.
- Auth via API key and athlete id, configurable base URL.
- Activity listing, streams, intervals, best-efforts, GAP histogram.
- Event CRUD with `start_date_local` and expanded categories.
- Gear, sport settings, power curves support.
- File download helper with streaming to disk.
- Integration tests using wiremock and runnable examples.
- Criterion benchmark for streaming download path.
