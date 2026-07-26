//! Upstream adapter implementing [`IntervalsClient`] via the dynamic
//! OpenAPI runtime.
//!
//! See `docs/adr/0001-two-layer-architecture-with-dynamic-upstream-adapter.md`.
//!
//! # Strangler fig
//!
//! The adapter wraps a typed fallback (`ReqwestIntervalsClient`) and a
//! [`DynamicRuntime`]. For each trait method:
//!
//! 1. If an operation-id mapping exists for the method AND the registry is
//!    loadable, dispatch via [`DynamicRuntime::dispatch_openapi`] and decode
//!    the structured content into the trait's return type.
//! 2. Otherwise, fall through to the typed fallback.
//!
//! New endpoints arriving upstream can be exposed by adding one operation-id
//! mapping. Typed methods migrate to dynamic dispatch one at a time, each
//! behind its own tests. ADR-0001 defers the failure policy per operation;
//! this adapter surfaces dynamic errors (does not silently fall back) when
//! a mapping is present but the dispatch fails.

use std::sync::Arc;

use intervals_icu_client::domains;
use intervals_icu_client::{
    ActivitySummary, AthleteProfile, BestEffortsOptions, DownloadProgress, Event, IntervalsClient,
    IntervalsError, Result,
};
use rmcp::ErrorData;
use rmcp::model::JsonObject;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::warn;

use crate::dynamic::DynamicRuntime;

/// Adapter that implements [`IntervalsClient`] by dispatching through the
/// dynamic OpenAPI runtime when an operation mapping exists, and falling
/// back to a typed client otherwise.
///
/// Construct with [`DynamicClientAdapter::new`]. Wire into
/// [`crate::IntervalsMcpHandler`] via the existing `with_dynamic_runtime`
/// seam (Phase 2B).
pub struct DynamicClientAdapter {
    runtime: Arc<DynamicRuntime>,
    fallback: Arc<dyn IntervalsClient>,
}

impl DynamicClientAdapter {
    /// Wrap a typed fallback client with a dynamic runtime.
    #[must_use]
    pub fn new(runtime: Arc<DynamicRuntime>, fallback: Arc<dyn IntervalsClient>) -> Self {
        Self { runtime, fallback }
    }

    /// Fetch the athlete's training plan via the dynamic OpenAPI runtime.
    ///
    /// Maps to upstream operation `getAthletePlan`
    /// (`GET /api/v1/athlete/{id}/training-plan`). This endpoint is not
    /// part of the typed `IntervalsClient` trait; it is reachable only
    /// through the dynamic adapter, demonstrating the adaptivity claim
    /// from ADR-0001 Phase 2C: new upstream endpoints are usable without
    /// modifying the typed trait.
    ///
    /// Returns `IntervalsError::NotFound` when the dynamic registry has
    /// not been loaded or the operation id is missing from the spec —
    /// handler authors should treat this as "capability unavailable in
    /// this deployment" rather than as an upstream HTTP 404.
    ///
    /// # Errors
    ///
    /// - `IntervalsError::NotFound` — registry not loaded or operation id
    ///   missing from the live spec.
    /// - `IntervalsError::Api` / `Auth` / etc. — upstream returned an
    ///   error response; status is preserved for classification helpers.
    pub async fn get_athlete_training_plan(&self) -> Result<Value> {
        match self
            .try_dispatch_dynamic::<Value>("getAthleteTrainingPlan", None)
            .await?
        {
            Some(value) => Ok(value),
            None => Err(IntervalsError::NotFound(
                "getAthleteTrainingPlan not available in dynamic registry".to_string(),
            )),
        }
    }

    /// Try to satisfy a trait method via dynamic dispatch.
    ///
    /// Returns:
    /// - `Ok(Some(value))` when the operation was mapped and dispatched
    ///   successfully.
    /// - `Ok(None)` when the operation is not mapped or the registry is
    ///   unavailable; the caller should fall through to the typed path.
    /// - `Err(_)` when the operation was mapped but dispatch or decode
    ///   failed. ADR-0001 failure policy: surface, do not silently fall
    ///   back, so that wiring regressions are observable.
    async fn try_dispatch_dynamic<T>(
        &self,
        operation_id: &str,
        arguments: Option<&JsonObject>,
    ) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let registry = match self.runtime.ensure_registry().await {
            Ok(reg) => reg,
            Err(err) => {
                crate::metrics::record_dynamic_dispatch(operation_id, "registry_unavailable");
                warn!(
                    operation_id,
                    error = %err,
                    "dynamic registry unavailable; falling back to typed client"
                );
                return Ok(None);
            }
        };

        let Some(operation) = registry.operation(operation_id) else {
            // No mapping registered for this method. Silent fallthrough by
            // design; the vast majority of methods remain on the typed
            // path during the strangler-fig migration.
            return Ok(None);
        };

        let dispatch_result = self
            .runtime
            .dispatch_openapi(operation, arguments)
            .await
            .map_err(|err| {
                crate::metrics::record_dynamic_dispatch(operation_id, "dispatch_error");
                error_data_to_intervals_error(err, operation_id)
            })?;

        if dispatch_result.is_error == Some(true) {
            crate::metrics::record_dynamic_dispatch(operation_id, "http_error");
            return Err(structured_error_to_intervals_error(
                &dispatch_result,
                operation_id,
            ));
        }

        crate::metrics::record_dynamic_dispatch(operation_id, "dispatched");
        let value = dispatch_result.into_typed::<T>().map_err(|err| {
            crate::metrics::record_dynamic_dispatch(operation_id, "decode_error");
            IntervalsError::from(err)
        })?;
        Ok(Some(value))
    }
}

/// Map an `ErrorData` returned by `dispatch_openapi` onto `IntervalsError`.
///
/// `dispatch_openapi` only returns `Err(ErrorData)` for transport-level
/// failures (missing API key, malformed request, network error). HTTP error
/// responses with a body are returned as `Ok(CallToolResult)` with
/// `is_error = true`; those are handled separately in
/// [`structured_error_to_intervals_error`].
fn error_data_to_intervals_error(err: ErrorData, operation_id: &str) -> IntervalsError {
    IntervalsError::Config(intervals_icu_client::ConfigError::Other(format!(
        "dynamic dispatch failed for {operation_id}: {err}"
    )))
}

/// Map a tool-level error result (HTTP non-2xx) onto `IntervalsError`.
///
/// `dispatch_operation` wraps non-success responses as
/// `CallToolResult::structured_error({ status, content_type, body })`. We
/// preserve the upstream status code so existing error-classification
/// helpers (`is_not_found`, `is_auth_error`, `is_rate_limited`) keep
/// working.
fn structured_error_to_intervals_error(
    result: &rmcp::model::CallToolResult,
    operation_id: &str,
) -> IntervalsError {
    let payload = result.structured_content.as_ref().unwrap_or(&Value::Null);
    let status = payload
        .get("status")
        .and_then(Value::as_u64)
        .map_or(500u16, |s| s as u16);
    let body = payload
        .get("body")
        .map(|b| b.to_string())
        .or_else(|| payload.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("dynamic dispatch for {operation_id} returned HTTP {status}"));
    IntervalsError::from_status(status, body)
}

#[async_trait::async_trait]
#[allow(clippy::too_many_arguments)]
impl IntervalsClient for DynamicClientAdapter {
    // ── Mapped to dynamic dispatch ──────────────────────────────────────
    //
    // Each entry below is one operation migrating off the typed path under
    // the strangler-fig plan (ADR-0001). The mapping must match an
    // `operationId` in the live OpenAPI spec; verify with
    // `jq '.paths[].get.operationId' docs/intervals_icu_api.json`.

    /// Maps to OpenAPI `getAthlete` (`GET /api/v1/athlete/{id}`).
    ///
    /// Athlete upstream returns `WithSportSettings`; our `AthleteProfile`
    /// only carries `id` and `name`, and serde ignores unknown fields, so
    /// the dynamic decode is a strict subset of the upstream schema.
    async fn get_athlete_profile(&self) -> Result<AthleteProfile> {
        if let Some(profile) = self
            .try_dispatch_dynamic::<AthleteProfile>("getAthlete", None)
            .await?
        {
            return Ok(profile);
        }
        self.fallback.get_athlete_profile().await
    }

    // ── Delegate to typed fallback (unmapped) ───────────────────────────

    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<ActivitySummary>> {
        self.fallback.get_recent_activities(limit, days_back).await
    }

    async fn create_event(&self, event: Event) -> Result<Event> {
        self.fallback.create_event(event).await
    }

    async fn get_event(&self, event_id: &str) -> Result<Event> {
        self.fallback.get_event(event_id).await
    }

    async fn delete_event(&self, event_id: &str) -> Result<()> {
        self.fallback.delete_event(event_id).await
    }

    async fn get_events(&self, days_back: Option<i32>, limit: Option<u32>) -> Result<Vec<Event>> {
        self.fallback.get_events(days_back, limit).await
    }

    async fn bulk_create_events(&self, events: Vec<Event>) -> Result<Vec<Event>> {
        self.fallback.bulk_create_events(events).await
    }

    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value> {
        self.fallback
            .get_activity_streams(activity_id, streams)
            .await
    }

    async fn get_activity_intervals(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fallback.get_activity_intervals(activity_id).await
    }

    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<BestEffortsOptions>,
    ) -> Result<serde_json::Value> {
        self.fallback.get_best_efforts(activity_id, options).await
    }

    async fn get_activity_details(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fallback.get_activity_details(activity_id).await
    }

    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<ActivitySummary>> {
        self.fallback.search_activities(query, limit).await
    }

    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value> {
        self.fallback.search_activities_full(query, limit).await
    }

    async fn get_activities_csv(&self) -> Result<String> {
        self.fallback.get_activities_csv().await
    }

    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback.update_activity(activity_id, fields).await
    }

    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        self.fallback
            .download_activity_file(activity_id, output_path)
            .await
    }

    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<DownloadProgress>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>> {
        self.fallback
            .download_activity_file_with_progress(activity_id, output_path, progress_tx, cancel_rx)
            .await
    }

    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        self.fallback
            .download_fit_file(activity_id, output_path)
            .await
    }

    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        self.fallback
            .download_gpx_file(activity_id, output_path)
            .await
    }

    async fn get_gear_list(&self) -> Result<serde_json::Value> {
        self.fallback.get_gear_list().await
    }

    async fn get_sport_settings(&self) -> Result<domains::workout::SportSettings> {
        self.fallback.get_sport_settings().await
    }

    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.fallback.get_power_curves(days_back, sport).await
    }

    async fn get_gap_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fallback.get_gap_histogram(activity_id).await
    }

    async fn delete_activity(&self, activity_id: &str) -> Result<()> {
        self.fallback.delete_activity(activity_id).await
    }

    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value> {
        self.fallback
            .get_activities_around(activity_id, limit, route_id)
            .await
    }

    async fn search_intervals(
        &self,
        min_secs: u32,
        max_secs: u32,
        min_intensity: u32,
        max_intensity: u32,
        interval_type: Option<String>,
        min_reps: Option<u32>,
        max_reps: Option<u32>,
        limit: Option<u32>,
    ) -> Result<serde_json::Value> {
        self.fallback
            .search_intervals(
                min_secs,
                max_secs,
                min_intensity,
                max_intensity,
                interval_type,
                min_reps,
                max_reps,
                limit,
            )
            .await
    }

    async fn get_power_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fallback.get_power_histogram(activity_id).await
    }

    async fn get_hr_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fallback.get_hr_histogram(activity_id).await
    }

    async fn get_pace_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fallback.get_pace_histogram(activity_id).await
    }

    async fn get_fitness_summary(&self) -> Result<serde_json::Value> {
        self.fallback.get_fitness_summary().await
    }

    async fn get_wellness(&self, days_back: Option<i32>) -> Result<serde_json::Value> {
        self.fallback.get_wellness(days_back).await
    }

    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value> {
        self.fallback.get_wellness_for_date(date).await
    }

    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback.update_wellness(date, data).await
    }

    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value> {
        self.fallback
            .get_upcoming_workouts(days_ahead, limit, category)
            .await
    }

    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback.update_event(event_id, fields).await
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<()> {
        self.fallback.bulk_delete_events(event_ids).await
    }

    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<Event>> {
        self.fallback
            .duplicate_event(event_id, num_copies, weeks_between)
            .await
    }

    async fn get_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.fallback.get_hr_curves(days_back, sport).await
    }

    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.fallback.get_pace_curves(days_back, sport).await
    }

    async fn get_workout_library(&self) -> Result<Vec<domains::workout::WorkoutItem>> {
        self.fallback.get_workout_library().await
    }

    async fn get_workouts_in_folder(
        &self,
        folder_id: &str,
    ) -> Result<Vec<domains::workout::WorkoutItem>> {
        self.fallback.get_workouts_in_folder(folder_id).await
    }

    async fn create_folder(&self, folder: &serde_json::Value) -> Result<domains::workout::Folder> {
        self.fallback.create_folder(folder).await
    }

    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback.update_folder(folder_id, fields).await
    }

    async fn delete_folder(&self, folder_id: &str) -> Result<()> {
        self.fallback.delete_folder(folder_id).await
    }

    async fn create_gear(&self, gear: &serde_json::Value) -> Result<serde_json::Value> {
        self.fallback.create_gear(gear).await
    }

    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback.update_gear(gear_id, fields).await
    }

    async fn delete_gear(&self, gear_id: &str) -> Result<()> {
        self.fallback.delete_gear(gear_id).await
    }

    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback.create_gear_reminder(gear_id, reminder).await
    }

    async fn update_gear_reminder(
        &self,
        gear_id: &str,
        reminder_id: &str,
        reset: bool,
        snooze_days: u32,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback
            .update_gear_reminder(gear_id, reminder_id, reset, snooze_days, fields)
            .await
    }

    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback
            .update_sport_settings(sport_type, recalc_hr_zones, fields)
            .await
    }

    async fn apply_sport_settings(&self, sport_type: &str) -> Result<serde_json::Value> {
        self.fallback.apply_sport_settings(sport_type).await
    }

    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fallback.create_sport_settings(settings).await
    }

    async fn delete_sport_settings(&self, sport_type: &str) -> Result<()> {
        self.fallback.delete_sport_settings(sport_type).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dynamic::{DynamicRuntime, DynamicRuntimeConfig};
    use crate::test_support::mock::MockIntervalsClient;
    use serde_json::json;
    use std::sync::Arc;

    /// Build an adapter whose dynamic runtime points at a temp-file spec
    /// fixture containing the `getAthlete` operation, and whose fallback is
    /// a [`MockIntervalsClient`] whose `get_athlete_profile` returns a
    /// distinct sentinel profile.
    ///
    /// Returns the adapter plus a "dynamic profile" the wiremock-style
    /// fixture would resolve to, so individual tests can pick which path
    /// they expect to win.
    async fn adapter_with_spec(spec: &str) -> DynamicClientAdapter {
        let tmp = tempfile::NamedTempFile::new().expect("temp file");
        std::fs::write(tmp.path(), spec).expect("write spec");

        let runtime = DynamicRuntime::new(
            DynamicRuntimeConfig::builder()
                .spec_source(tmp.path().to_string_lossy().to_string())
                .athlete_id("i42")
                .api_key("test-key")
                .build(),
        );
        // Force a real load so ensure_registry succeeds for tests that
        // need it; subsequent ensure_registry calls hit the cache.
        let _ = runtime.ensure_registry().await;

        DynamicClientAdapter::new(Arc::new(runtime), Arc::new(MockIntervalsClient::default()))
    }

    /// Minimal OpenAPI spec exposing only the `getAthlete` GET operation.
    fn spec_with_get_athlete() -> String {
        json!({
            "openapi": "3.0.0",
            "info": { "title": "test", "version": "1.0" },
            "paths": {
                "/api/v1/athlete/{id}": {
                    "get": {
                        "operationId": "getAthlete",
                        "parameters": [{
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" }
                        }],
                        "responses": {
                            "200": { "description": "ok" }
                        }
                    }
                }
            }
        })
        .to_string()
    }

    #[tokio::test]
    async fn unmapped_method_delegates_to_fallback() {
        // No spec loaded, but `get_recent_activities` has no dynamic
        // mapping in the adapter, so the typed fallback must answer.
        let runtime = DynamicRuntime::new(DynamicRuntimeConfig::builder().build());
        let adapter =
            DynamicClientAdapter::new(Arc::new(runtime), Arc::new(MockIntervalsClient::default()));

        let activities = adapter.get_recent_activities(None, None).await.unwrap();
        // MockIntervalsClient::default returns an empty activities vec.
        assert!(activities.is_empty());
    }

    #[tokio::test]
    async fn mapped_method_falls_back_when_registry_unavailable() {
        // Spec path is intentionally bogus. ensure_registry will fail, the
        // adapter must fall through to the typed fallback rather than
        // erroring out — see ADR-0001 fallthrough rule.
        let runtime = DynamicRuntime::new(
            DynamicRuntimeConfig::builder()
                .spec_source("/nonexistent/spec.json")
                .build(),
        );
        let adapter =
            DynamicClientAdapter::new(Arc::new(runtime), Arc::new(MockIntervalsClient::default()));

        let profile = adapter.get_athlete_profile().await.unwrap();
        // MockIntervalsClient::default returns the "Test Athlete" sentinel.
        assert_eq!(profile.name.as_deref(), Some("Test Athlete"));
    }

    #[tokio::test]
    async fn mapped_method_dispatches_through_dynamic_when_registry_loaded() {
        // End-to-end: with a valid spec, the adapter should satisfy
        // `get_athlete_profile` via the dynamic runtime rather than the
        // typed fallback. With no reachable upstream the dispatch will
        // either fail at the transport layer or — if a live or mock
        // endpoint answers — return a structured HTTP result. Either way,
        // the response must NOT be the MockIntervalsClient sentinel
        // ("Test Athlete"), which would indicate the adapter silently
        // fell through instead of dispatching.
        //
        // Per ADR-0001 failure policy (surface, do not silently fall
        // back), both transport and HTTP errors propagate.
        let adapter = adapter_with_spec(&spec_with_get_athlete()).await;
        let result = adapter.get_athlete_profile().await;

        let err = result.expect_err("dynamic dispatch should surface, not fall back");
        // Accept either framing: transport error (no network) or HTTP
        // error (auth/4xx/5xx from upstream). The point of the test is
        // that the dynamic path was exercised; it must not be the
        // MockIntervalsClient's success sentinel.
        let msg = err.to_string();
        assert!(
            msg.contains("dynamic dispatch failed")
                || msg.contains("authentication error")
                || msg.contains("API error")
                || msg.contains("resource not found"),
            "expected dynamic-dispatch outcome, got: {msg}"
        );
    }

    #[test]
    fn structured_error_to_intervals_error_preserves_status() {
        use rmcp::model::CallToolResult;

        let result = CallToolResult::structured_error(json!({
            "status": 404,
            "content_type": "application/json",
            "body": "athlete not found"
        }));
        let err = super::structured_error_to_intervals_error(&result, "getAthlete");
        assert!(
            err.is_not_found(),
            "404 should map to NotFound, got {err:?}"
        );
    }

    #[test]
    fn structured_error_to_intervals_error_maps_auth_status() {
        use rmcp::model::CallToolResult;

        let result = CallToolResult::structured_error(json!({
            "status": 401,
            "body": "unauthorized"
        }));
        let err = super::structured_error_to_intervals_error(&result, "getAthlete");
        assert!(err.is_auth_error(), "401 should map to Auth, got {err:?}");
    }

    #[test]
    fn error_data_to_intervals_error_is_config_variant() {
        let err = super::error_data_to_intervals_error(
            ErrorData::invalid_params("missing api key", None),
            "getAthlete",
        );
        let msg = err.to_string();
        assert!(
            msg.contains("dynamic dispatch failed for getAthlete"),
            "expected operation_id framing, got: {msg}"
        );
    }

    // ── Phase 2C: adaptivity proof ──────────────────────────────────────
    //
    // `get_athlete_training_plan` is reachable only through the dynamic
    // adapter — there is no corresponding method on the typed
    // `IntervalsClient` trait. These tests verify ADR-0001's adaptivity
    // claim: upstream operations absent from the typed trait become
    // usable the moment the dynamic registry knows about them.

    #[tokio::test]
    async fn training_plan_unreachable_without_registry() {
        // Spec source points at a nonexistent path, so ensure_registry
        // cannot fall back to the bundled `docs/intervals_icu_api.json`.
        // The capability is reported as NotFound ("not available in this
        // deployment"). Crucially, this does NOT silently fall back to a
        // typed path that does not exist.
        let runtime = DynamicRuntime::new(
            DynamicRuntimeConfig::builder()
                .spec_source("/nonexistent/spec.json")
                .build(),
        );
        let adapter =
            DynamicClientAdapter::new(Arc::new(runtime), Arc::new(MockIntervalsClient::default()));

        let err = adapter
            .get_athlete_training_plan()
            .await
            .expect_err("should surface NotFound when registry unavailable");
        assert!(err.is_not_found(), "expected NotFound, got {err:?}");
    }

    #[tokio::test]
    async fn training_plan_dispatches_when_registry_has_operation() {
        // Spec includes `getAthleteTrainingPlan`. With the registry loaded,
        // the adapter attempts to dispatch to the real upstream. With no
        // upstream reachable from the test sandbox this fails (transport
        // or HTTP error), but the failure proves the capability is wired:
        // the adapter did not return "NotFound".
        let spec = json!({
            "openapi": "3.0.0",
            "info": { "title": "test", "version": "1.0" },
            "paths": {
                "/api/v1/athlete/{id}/training-plan": {
                    "get": {
                        "operationId": "getAthleteTrainingPlan",
                        "parameters": [{
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": { "type": "string" }
                        }],
                        "responses": { "200": { "description": "ok" } }
                    }
                }
            }
        })
        .to_string();
        let adapter = adapter_with_spec(&spec).await;
        let result = adapter.get_athlete_training_plan().await;

        let err = result
            .expect_err("dispatch should attempt a real call and surface the resulting error");
        // NotFound here would mean "operation missing" — that would be a
        // wiring bug. Any other error means the dispatch was attempted.
        assert!(
            !err.is_not_found(),
            "operation was in the spec; NotFound means wiring bug, got {err:?}"
        );
    }
}
