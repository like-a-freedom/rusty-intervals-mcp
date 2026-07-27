//! HTTP client implementation for the Intervals.icu API.
//!
//! This module provides a reqwest-based implementation of the [`IntervalsClient`](crate::IntervalsClient) trait.
//!
//! The implementation is split across several submodules grouped by API
//! surface: [`activities`], [`athlete`], [`events`], [`downloads`],
//! [`wellness`], [`curves`], [`gear`], [`sport`], and
//! [`weather_routes`]. The `mod.rs` here owns the [`ReqwestIntervalsClient`]
//! struct, transport plumbing (auth, circuit breaker, error mapping), and
//! cross-cutting helpers.
//!
//! The weather/route submodule methods are named with a `fetch_` prefix to
//! avoid a name collision with the trait-method dispatchers in the
//! `impl IntervalsClient for ReqwestIntervalsClient` block below.

pub(crate) mod activities;
pub(crate) mod athlete;
pub(crate) mod curves;
pub(crate) mod downloads;
pub(crate) mod events;
pub(crate) mod gear;
pub(crate) mod sport;
pub(crate) mod weather_routes;
pub(crate) mod wellness;

use crate::circuit_breaker::CircuitBreaker;
use crate::{
    ActivityMessage, AthleteProfile, BestEffortsOptions, IntervalsError, Result, TransportError,
    ValidationError,
};
use ::metrics::{counter, histogram};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use std::path::PathBuf;
use std::sync::Arc;

// ADR-0003: single conversion site from the HTTP transport to the domain error.
// Every `reqwest::Error` produced in this module reaches `IntervalsError` via
// these two impls — the rest of the codebase never sees `reqwest::Error`.
// The chain is: reqwest::Error -> TransportError -> IntervalsError.
impl From<reqwest::Error> for TransportError {
    fn from(e: reqwest::Error) -> Self {
        TransportError {
            message: e.to_string(),
            is_timeout: e.is_timeout(),
            is_connect: e.is_connect(),
        }
    }
}

// Convenience: let `?` on a `Result<_, reqwest::Error>` auto-convert to
// `Result<_, IntervalsError>` inside this module. Outside `http_client.rs`
// callers must use `TransportError::from` explicitly so the transport detail
// doesn't leak back into the rest of the crate.
impl From<reqwest::Error> for IntervalsError {
    fn from(e: reqwest::Error) -> Self {
        Self::Transport(TransportError::from(e))
    }
}

/// Maximum number of characters to keep in error body snippets.
///
/// Truncation is codepoint-safe via `chars().take(N)`. A multi-byte UTF-8
/// character (e.g., `é`, emoji) is never split mid-byte. However, a grapheme
/// cluster (e.g., ZWJ-joined emoji like 👨‍👩‍👧‍👦) may be split, leaving a trailing
/// incomplete cluster. Acceptable for diagnostic-only error output.
const ERROR_BODY_MAX: usize = 256;
/// Maximum number of characters to keep when decoding an unexpected response body.
///
/// Codepoint-safe via `chars().take(N)`; grapheme clusters may be split
/// (see `ERROR_BODY_MAX` for rationale). Diagnostic-only.
pub(super) const DECODE_BODY_SNIPPET_MAX: usize = 512;
use tokio::io::AsyncWriteExt;

/// Client for the Intervals.icu API using reqwest.
#[derive(Clone)]
pub struct ReqwestIntervalsClient {
    base_url: String,
    athlete_id: String,
    api_key: SecretString,
    client: reqwest::Client,
    circuit_breaker: Arc<CircuitBreaker>,
}

impl std::fmt::Debug for ReqwestIntervalsClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReqwestIntervalsClient")
            .field("base_url", &self.base_url)
            .field("athlete_id", &self.athlete_id)
            .field("circuit_breaker", &self.circuit_breaker)
            .finish_non_exhaustive()
    }
}

impl ReqwestIntervalsClient {
    /// Create a new client instance.
    ///
    /// # Arguments
    /// * `base_url` - The base URL of the Intervals.icu API (e.g., `<https://intervals.icu>`)
    /// * `athlete_id` - The athlete ID for authentication
    /// * `api_key` - The API key for authentication
    pub fn new(
        base_url: &str,
        athlete_id: impl Into<String>,
        api_key: SecretString,
    ) -> Result<Self> {
        let client = reqwest::Client::builder().build().map_err(|e| {
            IntervalsError::Config(crate::ConfigError::Other(format!(
                "failed to build HTTP client: {e}"
            )))
        })?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            athlete_id: athlete_id.into(),
            api_key,
            client,
            circuit_breaker: Arc::new(CircuitBreaker::default()),
        })
    }

    /// Build an API URL from path segments.
    ///
    /// # Arguments
    /// * `segments` - Path segments (e.g., `&["athlete", athlete_id, "events"]`)
    ///
    /// # Returns
    /// Full URL like `https://intervals.icu/api/v1/athlete/i123/events`
    fn api_url(&self, segments: &[&str]) -> String {
        let mut url = format!("{}/api/v1", self.base_url);
        for segment in segments {
            url.push('/');
            url.push_str(segment);
        }
        url
    }

    /// Build an authenticated request with the given HTTP method.
    ///
    /// Replaces the previous `get_request`/`post_request`/`put_request`/`delete_request`
    /// quartet — all four were identical except for the verb, and callers had to know
    /// which `reqwest::Method` to pick. Single entry point reduces duplication and
    /// makes future auth changes (e.g. switching from Basic to Bearer) one-line.
    fn request(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        self.client
            .request(method, url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
    }

    /// Execute a request and return the raw response.
    ///
    /// Handles circuit breaker, timing, metrics, and transport errors.
    /// The caller is responsible for interpreting the response body.
    async fn execute_raw(&self, request: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        if !self.circuit_breaker.allow_request() {
            return Err(IntervalsError::Api(crate::error::ApiError::new(
                503,
                "circuit breaker open — upstream is unavailable",
                "",
            )));
        }

        let start = std::time::Instant::now();
        let resp = request.send().await;
        let duration = start.elapsed().as_secs_f64();

        let resp = match resp {
            Ok(r) => {
                self.circuit_breaker.record_success();
                r
            }
            Err(e) => {
                self.circuit_breaker.record_failure();
                histogram!("intervals_icu_client_upstream_request_duration_seconds")
                    .record(duration);
                return Err(IntervalsError::Transport(e.into()));
            }
        };

        let status = resp.status().as_u16();

        histogram!("intervals_icu_client_upstream_request_duration_seconds").record(duration);
        counter!(
            "intervals_icu_client_upstream_requests_total",
            "status" => status.to_string()
        )
        .increment(1);

        Ok(resp)
    }

    /// Execute a request and expect a JSON response.
    async fn execute_json<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        let resp = self.execute_raw(request).await?;
        self.handle_response(resp).await.inspect_err(|e| {
            Self::record_upstream_error(e);
        })
    }

    /// Execute a request and expect a text response.
    async fn execute_text(&self, request: reqwest::RequestBuilder) -> Result<String> {
        let resp = self.execute_raw(request).await?;
        if !resp.status().is_success() {
            let err = self.error_from_response(resp).await;
            Self::record_upstream_error(&err);
            return Err(err);
        }
        Ok(resp.text().await?)
    }

    /// Execute a request with no expected response body.
    async fn execute_empty(&self, request: reqwest::RequestBuilder) -> Result<()> {
        let resp = self.execute_raw(request).await?;
        if !resp.status().is_success() {
            let err = self.error_from_response(resp).await;
            Self::record_upstream_error(&err);
            return Err(err);
        }
        Ok(())
    }

    /// Record an upstream error metric based on error type.
    fn record_upstream_error(err: &IntervalsError) {
        let error_type = Self::upstream_error_type(err);
        counter!(
            "intervals_icu_client_upstream_errors_total",
            "error_type" => error_type
        )
        .increment(1);
    }

    fn upstream_error_type(err: &IntervalsError) -> &'static str {
        match err {
            IntervalsError::Transport(_) if err.is_timeout() => "timeout",
            IntervalsError::Transport(_) if err.is_connect() => "network",
            IntervalsError::Transport(_) => "network",
            IntervalsError::Auth(_) => "auth",
            IntervalsError::NotFound(_) => "not_found",
            IntervalsError::Api(api) if api.status >= 500 => "5xx",
            IntervalsError::Api(api) if api.status >= 400 => "4xx",
            _ => "other",
        }
    }

    /// Handle a response, converting status codes to appropriate errors.
    async fn handle_response<T: serde::de::DeserializeOwned>(
        &self,
        resp: reqwest::Response,
    ) -> Result<T> {
        let status = resp.status();
        if !status.is_success() {
            return Err(self.error_from_response(resp).await);
        }
        Ok(resp.json::<T>().await?)
    }

    /// Extract error information from a failed response.
    async fn error_from_response(&self, resp: reqwest::Response) -> IntervalsError {
        let status = resp.status().as_u16();
        let body = resp.text().await.unwrap_or_default();
        Self::error_from_response_parts(status, &body)
    }

    fn error_from_response_parts(status: u16, body: &str) -> IntervalsError {
        let body_snippet = Self::truncate_error_body(body);
        IntervalsError::from_status(status, body_snippet)
    }

    fn truncate_error_body(body: &str) -> String {
        body.chars().take(ERROR_BODY_MAX).collect()
    }

    /// Download a file from a URL, optionally saving to disk.
    async fn download_file(
        &self,
        url: String,
        output_path: Option<PathBuf>,
    ) -> Result<Option<String>> {
        let resp = self
            .execute_raw(self.request(reqwest::Method::GET, &url))
            .await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }

        if let Some(path) = output_path {
            let mut stream = resp.bytes_stream();
            let mut file = tokio::fs::File::create(&path).await?;
            while let Some(chunk) = stream.next().await {
                let bytes = chunk
                    .map_err(TransportError::from)
                    .map_err(IntervalsError::from)?;
                file.write_all(&bytes).await?;
            }
            file.sync_all().await?;
            return Ok(None);
        }

        let bytes = resp.bytes().await?;
        Ok(Some(STANDARD.encode(&bytes)))
    }
}

impl ReqwestIntervalsClient {
    /// Map case-insensitive sport names to their canonical API form.
    #[must_use]
    pub fn normalize_sport(s: &str) -> String {
        const SPORTS: &[&str] = &[
            "Ride",
            "Run",
            "Swim",
            "WeightTraining",
            "Hike",
            "Walk",
            "AlpineSki",
            "BackcountrySki",
            "Badminton",
            "Canoeing",
            "Crossfit",
            "EBikeRide",
            "EMountainBikeRide",
            "Elliptical",
            "Golf",
            "GravelRide",
            "TrackRide",
            "Handcycle",
            "HighIntensityIntervalTraining",
            "Hockey",
            "IceSkate",
            "InlineSkate",
            "Kayaking",
            "Kitesurf",
            "MountainBikeRide",
            "NordicSki",
            "OpenWaterSwim",
            "Padel",
            "Pilates",
            "Pickleball",
            "Racquetball",
            "Rugby",
            "RockClimbing",
            "RollerSki",
            "Rowing",
            "Sail",
            "Skateboard",
            "Snowboard",
            "Snowshoe",
            "Soccer",
            "Squash",
            "StairStepper",
            "StandUpPaddling",
            "Surfing",
            "TableTennis",
            "Tennis",
            "TrailRun",
            "Transition",
            "Velomobile",
            "VirtualRide",
            "VirtualRow",
            "VirtualRun",
            "VirtualSki",
            "WaterSport",
            "Wheelchair",
            "Windsurf",
            "Workout",
            "Yoga",
            "Other",
        ];
        let lowered = s.to_lowercase();
        for &c in SPORTS {
            if c.to_lowercase() == lowered {
                return c.to_string();
            }
        }
        // Fallback: capitalize first character
        if s.is_empty() {
            return s.to_string();
        }
        let mut chrs = s.chars();
        let first = chrs
            .next()
            .unwrap_or('X')
            .to_uppercase()
            .collect::<String>();
        format!("{}{}", first, chrs.as_str())
    }

    /// Normalize `start_date_local` for events: preserve time when provided;
    /// if only date is given, set time to 00:00:00.
    fn normalize_event_start(s: &str) -> Option<String> {
        crate::utils::normalize_event_start(s)
    }

    /// Fetch activity curves of a given type (power, hr, pace) for a sport.
    async fn get_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
        curve_type: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/athlete/{}/activity-{}-curves",
            self.base_url, self.athlete_id, curve_type
        );
        let today = chrono::Utc::now().date_naive();
        let oldest = if let Some(days) = days_back {
            today - chrono::Duration::days(i64::from(days))
        } else {
            today - chrono::Duration::days(90)
        };

        let pairs = crate::utils::QueryBuilder::new()
            .add("ext", "")
            .add("oldest", oldest.to_string())
            .add("newest", today.to_string())
            .add("type", sport)
            .build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    fn normalize_event_update_fields(fields: &serde_json::Value) -> Result<serde_json::Value> {
        let Some(mut object) = fields.as_object().cloned() else {
            return Ok(fields.clone());
        };

        if let Some(start_date_local) = object
            .get("start_date_local")
            .and_then(serde_json::Value::as_str)
        {
            let normalized = Self::normalize_event_start(start_date_local).ok_or_else(|| {
                IntervalsError::Validation(ValidationError::InvalidFormat {
                    field: "start_date_local".to_string(),
                    value: format!("invalid start_date_local: {start_date_local}"),
                })
            })?;
            object.insert(
                "start_date_local".to_string(),
                serde_json::Value::String(normalized),
            );
        }

        Ok(serde_json::Value::Object(object))
    }
}

// ============================================================================
// Athlete profile implementation lives in [`http_client::athlete`].
// Dispatchers below keep the trait surface intact.
// ============================================================================

#[async_trait::async_trait]
impl crate::IntervalsClient for ReqwestIntervalsClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile> {
        self.fetch_athlete_profile().await
    }

    // Weather and routes — real implementations live in
    // [`http_client::weather_routes`]. Dispatchers below keep the trait
    // surface intact without duplicating the URL/query logic.

    async fn get_weather_config(&self) -> Result<serde_json::Value> {
        self.fetch_weather_config().await
    }

    async fn update_weather_config(&self, config: &serde_json::Value) -> Result<serde_json::Value> {
        self.fetch_update_weather_config(config).await
    }

    async fn list_routes(&self) -> Result<serde_json::Value> {
        self.fetch_list_routes().await
    }

    async fn get_route(&self, route_id: i64, include_path: bool) -> Result<serde_json::Value> {
        self.fetch_route(route_id, include_path).await
    }

    async fn update_route(
        &self,
        route_id: i64,
        route: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_route(route_id, route).await
    }

    async fn get_route_similarity(
        &self,
        route_id: i64,
        other_id: i64,
    ) -> Result<serde_json::Value> {
        self.fetch_route_similarity(route_id, other_id).await
    }

    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<crate::ActivitySummary>> {
        self.fetch_recent_activities(limit, days_back).await
    }

    async fn get_activity_details(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fetch_activity_details(activity_id).await
    }

    async fn get_activity_messages(&self, activity_id: &str) -> Result<Vec<ActivityMessage>> {
        self.fetch_activity_messages(activity_id).await
    }

    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value> {
        self.fetch_activity_streams(activity_id, streams).await
    }

    async fn get_activity_intervals(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fetch_activity_intervals(activity_id).await
    }

    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<BestEffortsOptions>,
    ) -> Result<serde_json::Value> {
        self.fetch_best_efforts(activity_id, options).await
    }

    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<crate::ActivitySummary>> {
        self.fetch_search_activities(query, limit).await
    }

    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value> {
        self.fetch_search_activities_full(query, limit).await
    }

    async fn get_activities_csv(&self) -> Result<String> {
        self.fetch_activities_csv().await
    }

    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_activity(activity_id, fields).await
    }

    async fn delete_activity(&self, activity_id: &str) -> Result<()> {
        self.fetch_delete_activity(activity_id).await
    }

    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value> {
        self.fetch_activities_around(activity_id, limit, route_id)
            .await
    }

    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        self.fetch_download_activity_file(activity_id, output_path)
            .await
    }

    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<crate::DownloadProgress>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>> {
        self.fetch_download_activity_file_with_progress(
            activity_id,
            output_path,
            progress_tx,
            cancel_rx,
        )
        .await
    }

    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        self.fetch_download_fit_file(activity_id, output_path).await
    }

    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        self.fetch_download_gpx_file(activity_id, output_path).await
    }

    async fn get_gap_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fetch_gap_histogram(activity_id).await
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
        self.fetch_search_intervals(
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
        self.fetch_power_histogram(activity_id).await
    }

    async fn get_hr_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fetch_hr_histogram(activity_id).await
    }

    async fn get_pace_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        self.fetch_pace_histogram(activity_id).await
    }

    async fn create_event(&self, event: crate::Event) -> Result<crate::Event> {
        self.fetch_create_event(event).await
    }

    async fn get_event(&self, event_id: &str) -> Result<crate::Event> {
        self.fetch_event(event_id).await
    }

    async fn delete_event(&self, event_id: &str) -> Result<()> {
        self.fetch_delete_event(event_id).await
    }

    async fn get_events(
        &self,
        days_back: Option<i32>,
        limit: Option<u32>,
    ) -> Result<Vec<crate::Event>> {
        self.fetch_events(days_back, limit).await
    }

    async fn bulk_create_events(&self, events: Vec<crate::Event>) -> Result<Vec<crate::Event>> {
        self.fetch_bulk_create_events(events).await
    }

    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value> {
        self.fetch_upcoming_workouts(days_ahead, limit, category)
            .await
    }

    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_event(event_id, fields).await
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<()> {
        self.fetch_bulk_delete_events(event_ids).await
    }

    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<crate::Event>> {
        self.fetch_duplicate_event(event_id, num_copies, weeks_between)
            .await
    }

    async fn get_fitness_summary(&self) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/athlete/{}/athlete-summary.json",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.fetch_power_curves(days_back, sport).await
    }

    async fn get_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.fetch_hr_curves(days_back, sport).await
    }

    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.fetch_pace_curves(days_back, sport).await
    }

    async fn get_gear_list(&self) -> Result<serde_json::Value> {
        self.fetch_gear_list().await
    }

    async fn create_gear(&self, gear: &serde_json::Value) -> Result<serde_json::Value> {
        self.fetch_create_gear(gear).await
    }

    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_gear(gear_id, fields).await
    }

    async fn delete_gear(&self, gear_id: &str) -> Result<()> {
        self.fetch_delete_gear(gear_id).await
    }

    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_create_gear_reminder(gear_id, reminder).await
    }

    async fn update_gear_reminder(
        &self,
        gear_id: &str,
        reminder_id: &str,
        reset: bool,
        snooze_days: u32,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_gear_reminder(gear_id, reminder_id, reset, snooze_days, fields)
            .await
    }

    async fn get_wellness(&self, days_back: Option<i32>) -> Result<serde_json::Value> {
        self.fetch_wellness(days_back).await
    }

    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value> {
        self.fetch_wellness_for_date(date).await
    }

    async fn update_wellness(
        &self,
        date: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_wellness(date, payload).await
    }

    async fn update_wellness_bulk(&self, entries: &[serde_json::Value]) -> Result<()> {
        self.fetch_update_wellness_bulk(entries).await
    }

    async fn get_workout_library(&self) -> Result<Vec<crate::domains::workout::WorkoutItem>> {
        self.fetch_workout_library().await
    }

    async fn get_workouts_in_folder(
        &self,
        folder_id: &str,
    ) -> Result<Vec<crate::domains::workout::WorkoutItem>> {
        self.fetch_workouts_in_folder(folder_id).await
    }

    async fn create_folder(
        &self,
        folder: &serde_json::Value,
    ) -> Result<crate::domains::workout::Folder> {
        self.fetch_create_folder(folder).await
    }

    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_folder(folder_id, fields).await
    }

    async fn delete_folder(&self, folder_id: &str) -> Result<()> {
        self.fetch_delete_folder(folder_id).await
    }

    async fn get_sport_settings(&self) -> Result<crate::domains::workout::SportSettings> {
        self.fetch_sport_settings().await
    }

    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_update_sport_settings(sport_type, recalc_hr_zones, fields)
            .await
    }

    async fn apply_sport_settings(&self, sport_type: &str) -> Result<serde_json::Value> {
        self.fetch_apply_sport_settings(sport_type).await
    }

    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.fetch_create_sport_settings(settings).await
    }

    async fn delete_sport_settings(&self, sport_type: &str) -> Result<()> {
        self.fetch_delete_sport_settings(sport_type).await
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        IntervalsClient, IntervalsError, ValidationError, http_client::ReqwestIntervalsClient,
    };
    use serde_json::json;

    #[test]
    fn normalize_sport_capitalizes_correctly() {
        assert_eq!(ReqwestIntervalsClient::normalize_sport("run"), "Run");
        assert_eq!(ReqwestIntervalsClient::normalize_sport("RIDE"), "Ride");
        assert_eq!(
            ReqwestIntervalsClient::normalize_sport("MountainBikeRide"),
            "MountainBikeRide"
        );
    }

    #[test]
    fn normalize_event_update_fields_expands_date_only() {
        let normalized = ReqwestIntervalsClient::normalize_event_update_fields(&json!({
            "start_date_local": "2026-03-16",
            "name": "Tempo Run"
        }))
        .expect("date-only update fields should normalize");

        assert_eq!(
            normalized
                .get("start_date_local")
                .and_then(serde_json::Value::as_str),
            Some("2026-03-16T00:00:00")
        );
        assert_eq!(
            normalized.get("name").and_then(serde_json::Value::as_str),
            Some("Tempo Run")
        );
    }

    #[test]
    fn normalize_event_update_fields_rejects_invalid_date() {
        let err = ReqwestIntervalsClient::normalize_event_update_fields(&json!({
            "start_date_local": "not-a-date",
            "name": "Tempo Run"
        }))
        .expect_err("invalid date should be rejected");

        assert!(matches!(
            err,
            IntervalsError::Validation(ValidationError::InvalidFormat { field, value })
                if field == "start_date_local" && value.contains("not-a-date")
        ));
    }

    #[test]
    fn resolve_sport_settings_id_from_flat_array_matches_type() {
        let settings = crate::domains::workout::SportSettings {
            sports: vec![crate::domains::workout::SportSetting {
                id: Some(1783043),
                types: Some(vec!["Run".into(), "VirtualRun".into(), "TrailRun".into()]),
                ..Default::default()
            }],
            age: None,
            weight: None,
        };

        let resolved =
            ReqwestIntervalsClient::resolve_sport_settings_id_from_settings(&settings, "run")
                .expect("sport type should resolve from settings");

        assert_eq!(resolved, "1783043");
    }

    #[test]
    fn resolve_sport_settings_id_from_nested_sports_matches_name() {
        let settings = crate::domains::workout::SportSettings {
            sports: vec![crate::domains::workout::SportSetting {
                id: Some(-7),
                name: Some("MountainBikeRide".into()),
                ..Default::default()
            }],
            age: None,
            weight: None,
        };

        let resolved = ReqwestIntervalsClient::resolve_sport_settings_id_from_settings(
            &settings,
            "mountainbikeride",
        )
        .expect("sport name should resolve from settings");

        assert_eq!(resolved, "-7");
    }

    #[test]
    fn resolve_sport_settings_id_from_single_object_matches_name() {
        let settings = crate::domains::workout::SportSettings {
            sports: vec![crate::domains::workout::SportSetting {
                id: Some(42),
                name: Some("Swim".into()),
                ..Default::default()
            }],
            age: None,
            weight: None,
        };

        let resolved =
            ReqwestIntervalsClient::resolve_sport_settings_id_from_settings(&settings, "swim")
                .expect("single sport settings object should resolve by name");

        assert_eq!(resolved, "42");
    }

    #[test]
    fn resolve_sport_settings_id_from_settings_rejects_unknown_sport() {
        let settings = crate::domains::workout::SportSettings {
            sports: vec![crate::domains::workout::SportSetting {
                id: Some(1783043),
                types: Some(vec!["Run".into(), "VirtualRun".into(), "TrailRun".into()]),
                ..Default::default()
            }],
            age: None,
            weight: None,
        };

        let err =
            ReqwestIntervalsClient::resolve_sport_settings_id_from_settings(&settings, "PogoStick")
                .expect_err("unknown sport should fail resolution");

        assert!(matches!(
            err,
            IntervalsError::Validation(ValidationError::InvalidFormat { field, value })
                if field == "sport_type" && value.contains("PogoStick")
        ));
    }

    #[test]
    fn error_from_response_parts_maps_not_found() {
        let err = ReqwestIntervalsClient::error_from_response_parts(404, "missing activity");

        assert!(matches!(err, IntervalsError::NotFound(body) if body == "missing activity"));
    }

    #[test]
    fn error_from_response_parts_maps_auth() {
        let err = ReqwestIntervalsClient::error_from_response_parts(403, "forbidden");

        assert!(matches!(err, IntervalsError::Auth(body) if body == "forbidden"));
    }

    #[test]
    fn error_from_response_parts_maps_validation() {
        let err = ReqwestIntervalsClient::error_from_response_parts(422, "bad payload");

        assert!(matches!(
            err,
            IntervalsError::Validation(ValidationError::InvalidFormat { field, value })
                if field == "request" && value == "bad payload"
        ));
    }

    #[test]
    fn error_from_response_parts_truncates_long_body() {
        let long_body = "é".repeat(300);
        let err = ReqwestIntervalsClient::error_from_response_parts(500, &long_body);

        match err {
            IntervalsError::Api(api) => {
                assert_eq!(api.status, 500);
                assert_eq!(api.message.chars().count(), super::ERROR_BODY_MAX);
                assert_eq!(api.raw_body.chars().count(), super::ERROR_BODY_MAX);
                assert_eq!(api.message, "é".repeat(super::ERROR_BODY_MAX));
                assert_eq!(api.raw_body, "é".repeat(super::ERROR_BODY_MAX));
            }
            other => panic!("expected API error, got {other:?}"),
        }
    }

    #[test]
    fn upstream_error_type_classifies_auth() {
        let err = IntervalsError::Auth("forbidden".to_string());

        assert_eq!(ReqwestIntervalsClient::upstream_error_type(&err), "auth");
    }

    #[test]
    fn upstream_error_type_classifies_not_found() {
        let err = IntervalsError::NotFound("missing".to_string());

        assert_eq!(
            ReqwestIntervalsClient::upstream_error_type(&err),
            "not_found"
        );
    }

    #[test]
    fn upstream_error_type_classifies_5xx_and_4xx_api_errors() {
        let server_err = IntervalsError::from_status(500, "boom");
        let client_err = IntervalsError::from_status(429, "slow down");

        assert_eq!(
            ReqwestIntervalsClient::upstream_error_type(&server_err),
            "5xx"
        );
        assert_eq!(
            ReqwestIntervalsClient::upstream_error_type(&client_err),
            "4xx"
        );
    }

    #[test]
    fn upstream_error_type_classifies_validation_as_other() {
        let err = IntervalsError::Validation(ValidationError::InvalidFormat {
            field: "request".to_string(),
            value: "bad payload".to_string(),
        });

        assert_eq!(ReqwestIntervalsClient::upstream_error_type(&err), "other");
    }

    #[test]
    fn extract_available_streams_reads_stream_object_keys() {
        let payload = json!({
            "streams": {
                "watts": [100, 120],
                "distance": [10.0, 20.0]
            }
        });

        let mut streams = super::activities::extract_available_streams(&payload);
        streams.sort();

        assert_eq!(streams, vec!["distance".to_string(), "watts".to_string()]);
    }

    #[test]
    fn extract_available_streams_reads_stream_array_name_or_type() {
        let payload = json!({
            "streams": [
                {"name": "watts"},
                {"type": "distance"}
            ]
        });

        let mut streams = super::activities::extract_available_streams(&payload);
        streams.sort();

        assert_eq!(streams, vec!["distance".to_string(), "watts".to_string()]);
    }

    #[test]
    fn extract_available_streams_reads_top_level_array_variants() {
        let payload = json!([
            {"name": "watts"},
            {"type": "distance"},
            {"name": ""},
            {"other": "ignored"}
        ]);

        let mut streams = super::activities::extract_available_streams(&payload);
        streams.sort();

        assert_eq!(streams, vec!["distance".to_string(), "watts".to_string()]);
    }

    #[test]
    fn extract_available_streams_reads_top_level_object_arrays() {
        let payload = json!({
            "watts": [100, 120],
            "distance": [1.0, 2.0],
            "meta": {"ignored": true}
        });

        let mut streams = super::activities::extract_available_streams(&payload);
        streams.sort();

        assert_eq!(streams, vec!["distance".to_string(), "watts".to_string()]);
    }

    #[test]
    fn annotate_best_efforts_payload_adds_missing_stream() {
        let payload = json!({"best_efforts": [{"duration": 60, "power": 300}]});

        let annotated = super::activities::annotate_best_efforts_payload(payload, Some("power"));

        assert_eq!(
            annotated.get("stream").and_then(serde_json::Value::as_str),
            Some("power")
        );
    }

    #[test]
    fn annotate_best_efforts_payload_preserves_existing_stream() {
        let payload = json!({
            "stream": "distance",
            "best_efforts": [{"distance": 1000, "power": 300}]
        });

        let annotated = super::activities::annotate_best_efforts_payload(payload, Some("power"));

        assert_eq!(
            annotated.get("stream").and_then(serde_json::Value::as_str),
            Some("distance")
        );
    }

    #[tokio::test]
    async fn circuit_breaker_open_returns_synthetic_503() {
        use crate::circuit_breaker::CircuitBreaker;
        use std::sync::Arc;
        use std::time::Duration;

        let cb = Arc::new(CircuitBreaker::new(1, Duration::from_secs(60)));
        cb.record_failure();
        assert!(!cb.allow_request());

        let inner: reqwest::Client = reqwest::Client::builder().build().unwrap();
        let client = super::ReqwestIntervalsClient {
            base_url: "http://localhost".into(),
            athlete_id: "999".into(),
            api_key: secrecy::SecretString::new("test-key".to_string().into_boxed_str()),
            client: inner,
            circuit_breaker: cb,
        };

        let request = client.client.get("http://localhost/events");
        let result = client.execute_raw(request).await;
        match result {
            Err(IntervalsError::Api(api_err)) => {
                assert_eq!(api_err.status, 503);
            }
            other => panic!("expected 503 Api error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_best_efforts_rejects_missing_stream() {
        use crate::BestEffortsOptions;
        use crate::circuit_breaker::CircuitBreaker;
        use std::sync::Arc;

        let inner: reqwest::Client = reqwest::Client::builder().build().unwrap();
        let client = super::ReqwestIntervalsClient {
            base_url: "http://localhost".into(),
            athlete_id: "999".into(),
            api_key: secrecy::SecretString::new("test-key".to_string().into_boxed_str()),
            client: inner,
            circuit_breaker: Arc::new(CircuitBreaker::default()),
        };

        // Missing stream should fail before any HTTP request.
        let opts = BestEffortsOptions {
            stream: None,
            duration: Some(120),
            distance: None,
            count: None,
            min_value: None,
            exclude_intervals: None,
            start_index: None,
            end_index: None,
        };
        let result = client.get_best_efforts("act_1", Some(opts)).await;
        match result {
            Err(IntervalsError::Validation(ValidationError::InvalidFormat { field, .. })) => {
                assert_eq!(field, "stream");
            }
            other => panic!("expected Validation error for stream, got: {other:?}"),
        }

        // Missing duration AND distance should fail before any HTTP request.
        let opts = BestEffortsOptions {
            stream: Some("watts".to_string()),
            duration: None,
            distance: None,
            count: None,
            min_value: None,
            exclude_intervals: None,
            start_index: None,
            end_index: None,
        };
        let result = client.get_best_efforts("act_1", Some(opts)).await;
        match result {
            Err(IntervalsError::Validation(ValidationError::InvalidFormat { field, .. })) => {
                assert_eq!(field, "duration/distance");
            }
            other => panic!("expected Validation error for duration/distance, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn bulk_delete_events_rejects_invalid_id() {
        use crate::circuit_breaker::CircuitBreaker;
        use std::sync::Arc;

        let inner: reqwest::Client = reqwest::Client::builder().build().unwrap();
        let client = super::ReqwestIntervalsClient {
            base_url: "http://localhost".into(),
            athlete_id: "999".into(),
            api_key: secrecy::SecretString::new("test-key".to_string().into_boxed_str()),
            client: inner,
            circuit_breaker: Arc::new(CircuitBreaker::default()),
        };

        // Non-numeric ID should fail during parsing before any HTTP request.
        let result = client.bulk_delete_events(vec!["abc".to_string()]).await;
        match result {
            Err(IntervalsError::Validation(ValidationError::InvalidFormat { field, .. })) => {
                assert_eq!(field, "event_id");
            }
            other => panic!("expected Validation error for event_id, got: {other:?}"),
        }
    }
}
