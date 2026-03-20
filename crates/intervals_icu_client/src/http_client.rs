//! HTTP client implementation for the Intervals.icu API.
//!
//! This module provides a reqwest-based implementation of the [`IntervalsClient`](crate::IntervalsClient) trait.

use crate::traits::{
    ActivityService, AthleteService, EventService, FitnessService, GearService, RouteService,
    SportSettingsService, WeatherService, WellnessService, WorkoutService,
};
use crate::{
    ActivityMessage, AthleteProfile, BestEffortsOptions, IntervalsError, Result, ValidationError,
};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// Client for the Intervals.icu API using reqwest.
#[derive(Clone, Debug)]
pub struct ReqwestIntervalsClient {
    base_url: String,
    athlete_id: String,
    api_key: SecretString,
    client: reqwest::Client,
}

impl ReqwestIntervalsClient {
    /// Create a new client instance.
    ///
    /// # Arguments
    /// * `base_url` - The base URL of the Intervals.icu API (e.g., "https://intervals.icu")
    /// * `athlete_id` - The athlete ID for authentication
    /// * `api_key` - The API key for authentication
    pub fn new(base_url: &str, athlete_id: impl Into<String>, api_key: SecretString) -> Self {
        let client = reqwest::Client::builder()
            .build()
            .expect("reqwest client build should not fail");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            athlete_id: athlete_id.into(),
            api_key,
            client,
        }
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

    /// Build query parameters from a vector of (key, String) pairs.
    ///
    /// This is a helper to avoid repeating the conversion from Vec<(&str, String)> to Vec<(&str, &str)>.
    fn build_query<'a>(&'a self, params: &'a [(&str, String)]) -> Vec<(&'a str, &'a str)> {
        params.iter().map(|(k, v)| (*k, v.as_str())).collect()
    }

    async fn resolve_sport_settings_id(&self, sport_type_or_id: &str) -> Result<String> {
        if !sport_type_or_id.is_empty() && sport_type_or_id.chars().all(|c| c.is_ascii_digit()) {
            return Ok(sport_type_or_id.to_string());
        }

        let normalized = Self::normalize_sport(sport_type_or_id);
        let payload = <Self as SportSettingsService>::get_sport_settings(self).await?;

        let candidates = if let Some(items) = payload.as_array() {
            items
                .iter()
                .filter_map(serde_json::Value::as_object)
                .collect::<Vec<_>>()
        } else if let Some(object) = payload.as_object() {
            if let Some(items) = object.get("sports").and_then(serde_json::Value::as_array) {
                items
                    .iter()
                    .filter_map(serde_json::Value::as_object)
                    .collect::<Vec<_>>()
            } else {
                vec![object]
            }
        } else {
            Vec::new()
        };

        candidates
            .into_iter()
            .find(|entry| {
                entry
                    .get("types")
                    .and_then(serde_json::Value::as_array)
                    .map(|types| {
                        types
                            .iter()
                            .any(|value| value.as_str() == Some(normalized.as_str()))
                    })
                    .unwrap_or(false)
                    || entry.get("name").and_then(serde_json::Value::as_str)
                        == Some(normalized.as_str())
            })
            .and_then(|entry| {
                entry.get("id").and_then(|value| {
                    value
                        .as_i64()
                        .map(|id| id.to_string())
                        .or_else(|| value.as_str().map(ToString::to_string))
                })
            })
            .ok_or_else(|| {
                IntervalsError::Validation(ValidationError::InvalidFormat {
                    field: "sport_type".to_string(),
                    value: format!(
                        "could not resolve sport settings id for sport type '{}'",
                        sport_type_or_id
                    ),
                })
            })
    }

    /// Build an authenticated GET request.
    fn get_request(&self, url: &str) -> reqwest::RequestBuilder {
        self.client
            .get(url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
    }

    /// Build an authenticated POST request.
    fn post_request(&self, url: &str) -> reqwest::RequestBuilder {
        self.client
            .post(url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
    }

    /// Build an authenticated PUT request.
    fn put_request(&self, url: &str) -> reqwest::RequestBuilder {
        self.client
            .put(url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
    }

    /// Build an authenticated DELETE request.
    fn delete_request(&self, url: &str) -> reqwest::RequestBuilder {
        self.client
            .delete(url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
    }

    /// Execute a request and expect a JSON response.
    async fn execute_json<T: serde::de::DeserializeOwned>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        let resp = request.send().await?;
        self.handle_response(resp).await
    }

    /// Execute a request and expect a text response.
    async fn execute_text(&self, request: reqwest::RequestBuilder) -> Result<String> {
        let resp = request.send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        Ok(resp.text().await?)
    }

    /// Execute a request with no expected response body.
    async fn execute_empty(&self, request: reqwest::RequestBuilder) -> Result<()> {
        let resp = request.send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        Ok(())
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
        let body_snippet: String = body.chars().take(256).collect();

        match status {
            404 => IntervalsError::NotFound(body_snippet),
            401 | 403 => IntervalsError::Auth(body_snippet),
            422 => IntervalsError::Validation(ValidationError::InvalidFormat {
                field: "request".to_string(),
                value: body_snippet,
            }),
            _ => IntervalsError::from_status(status, body_snippet),
        }
    }

    /// Download a file from a URL, optionally saving to disk.
    async fn download_file(
        &self,
        url: String,
        output_path: Option<PathBuf>,
    ) -> Result<Option<String>> {
        let resp = self.get_request(&url).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }

        if let Some(path) = output_path {
            let mut stream = resp.bytes_stream();
            let mut file = tokio::fs::File::create(&path)
                .await
                .map_err(|e| IntervalsError::Config(crate::ConfigError::Other(e.to_string())))?;
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(IntervalsError::Http)?;
                file.write_all(&bytes).await.map_err(|e| {
                    IntervalsError::Config(crate::ConfigError::Other(e.to_string()))
                })?;
            }
            file.sync_all()
                .await
                .map_err(|e| IntervalsError::Config(crate::ConfigError::Other(e.to_string())))?;
            return Ok(None);
        }

        let bytes = resp.bytes().await?;
        Ok(Some(STANDARD.encode(&bytes)))
    }
}

impl ReqwestIntervalsClient {
    /// Map case-insensitive sport names to their canonical API form.
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

    /// Normalize start_date_local for events: preserve time when provided;
    /// if only date is given, set time to 00:00:00.
    fn normalize_event_start(s: &str) -> Option<String> {
        crate::utils::normalize_event_start(s)
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
                    value: format!("invalid start_date_local: {}", start_date_local),
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
// Service Trait Implementations
// ============================================================================

#[async_trait]
impl AthleteService for ReqwestIntervalsClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile> {
        let url = self.api_url(&["athlete", &self.athlete_id, "profile"]);
        let resp = self.get_request(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(self.error_from_response(resp).await);
        }

        #[derive(serde::Deserialize)]
        struct ProfilePayload {
            athlete: Option<ProfileAthlete>,
        }
        #[derive(serde::Deserialize)]
        struct ProfileAthlete {
            id: Option<String>,
            name: Option<String>,
        }

        let payload: ProfilePayload = resp.json().await?;
        payload
            .athlete
            .map(|a| AthleteProfile {
                id: a.id.unwrap_or_default(),
                name: a.name,
            })
            .ok_or_else(|| {
                IntervalsError::Config(crate::ConfigError::Other(
                    "missing athlete profile data".to_string(),
                ))
            })
    }
}

#[async_trait]
impl ActivityService for ReqwestIntervalsClient {
    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<crate::ActivitySummary>> {
        let url = self.api_url(&["athlete", &self.athlete_id, "activities"]);
        let today = Utc::now().date_naive();
        let oldest = today - Duration::days(days_back.unwrap_or(7) as i64);

        let mut pairs: Vec<(&str, String)> = vec![
            ("oldest", oldest.to_string()),
            ("newest", today.to_string()),
        ];
        if let Some(limit) = limit {
            pairs.push(("limit", limit.to_string()));
        }

        self.execute_json(self.get_request(&url).query(&self.build_query(&pairs)))
            .await
    }

    async fn get_activity_details(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_activity_messages(&self, activity_id: &str) -> Result<Vec<ActivityMessage>> {
        let url = format!("{}/api/v1/activity/{}/messages", self.base_url, activity_id);
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/activity/{}/streams", self.base_url, activity_id);
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(s) = streams {
            pairs.push(("streams", s.join(",")));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_activity_intervals(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/intervals",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<BestEffortsOptions>,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/best-efforts",
            self.base_url, activity_id
        );

        if let Some(opts) = options {
            if opts.stream.is_none() {
                return Err(IntervalsError::Validation(ValidationError::InvalidFormat {
                    field: "stream".to_string(),
                    value: "missing stream in best-efforts options".to_string(),
                }));
            }
            if opts.duration.is_none() && opts.distance.is_none() {
                return Err(IntervalsError::Validation(ValidationError::InvalidFormat {
                    field: "duration/distance".to_string(),
                    value: "missing duration or distance in best-efforts options".to_string(),
                }));
            }

            let mut q: Vec<(&str, String)> = Vec::new();
            if let Some(s) = opts.stream.as_deref() {
                q.push(("stream", s.to_string()));
            }
            if let Some(dur) = opts.duration {
                q.push(("duration", dur.to_string()));
            }
            if let Some(dist) = opts.distance {
                q.push(("distance", dist.to_string()));
            }
            if let Some(cnt) = opts.count {
                q.push(("count", cnt.to_string()));
            }
            if let Some(minv) = opts.min_value {
                q.push(("minValue", minv.to_string()));
            }
            if let Some(ex) = opts.exclude_intervals {
                q.push((
                    "excludeIntervals",
                    (if ex { "true" } else { "false" }).to_string(),
                ));
            }
            if let Some(si) = opts.start_index {
                q.push(("startIndex", si.to_string()));
            }
            if let Some(ei) = opts.end_index {
                q.push(("endIndex", ei.to_string()));
            }

            let stream = opts.stream.as_deref();
            let value = self.execute_json(self.get_request(&url).query(&q)).await?;
            return Ok(annotate_best_efforts_payload(value, stream));
        }

        // Try default parameter combinations when no options provided
        let attempts = [
            vec![("stream", "power"), ("duration", "60")],
            vec![("stream", "power"), ("distance", "1000")],
            vec![("stream", "power"), ("duration", "300")],
        ];

        for params in attempts.iter() {
            let qp: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, *v)).collect();
            let resp = self.get_request(&url).query(&qp).send().await?;

            if resp.status().is_success() {
                let value = resp.json().await?;
                let stream = params
                    .iter()
                    .find(|(key, _)| *key == "stream")
                    .map(|(_, value)| *value);
                return Ok(annotate_best_efforts_payload(value, stream));
            }

            if resp.status().as_u16() != 422 {
                return Err(self.error_from_response(resp).await);
            }
        }

        // All fallbacks yielded 422 — attempt to detect available streams
        let streams_payload = self.get_activity_streams(activity_id, None).await;
        match streams_payload {
            Ok(json) => {
                let available_streams = extract_available_streams(&json);
                let candidates = [
                    "power",
                    "watts",
                    "hr",
                    "heartrate",
                    "pace",
                    "speed",
                    "distance",
                ];

                let mut ordered_streams: Vec<String> = Vec::new();
                for &cand in &candidates {
                    if available_streams.contains(&cand.to_string()) {
                        ordered_streams.push(cand.to_string());
                    }
                }
                for s in available_streams.iter() {
                    if !ordered_streams.contains(s) {
                        ordered_streams.push(s.clone());
                    }
                }

                for cand in ordered_streams.iter() {
                    let param_sets = [
                        vec![("stream", cand.as_str()), ("duration", "60")],
                        vec![("stream", cand.as_str()), ("distance", "1000")],
                        vec![("stream", cand.as_str()), ("duration", "300")],
                    ];
                    let mut param_sets_extended: Vec<Vec<(&str, &str)>> = param_sets.to_vec();
                    param_sets_extended.push(vec![("stream", cand.as_str()), ("count", "8")]);
                    param_sets_extended.push(vec![("stream", cand.as_str())]);

                    for params in param_sets.iter().chain(param_sets_extended.iter()) {
                        let qp: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, *v)).collect();
                        let resp = self.get_request(&url).query(&qp).send().await?;
                        if resp.status().is_success() {
                            let value = resp.json().await?;
                            let stream = params
                                .iter()
                                .find(|(key, _)| *key == "stream")
                                .map(|(_, value)| *value);
                            return Ok(annotate_best_efforts_payload(value, stream));
                        }
                        let status_code = resp.status().as_u16();
                        if status_code == 422 || status_code == 404 {
                            if let Ok(text) = resp.text().await {
                                tracing::trace!(
                                    "best-efforts returned {} for stream={} params={:?} body={}",
                                    status_code,
                                    cand,
                                    params,
                                    text
                                );
                            }
                            continue;
                        }
                        return Err(self.error_from_response(resp).await);
                    }
                }

                Err(IntervalsError::Validation(ValidationError::InvalidFormat {
                    field: "parameters".to_string(),
                    value: "no suitable best efforts parameters found".to_string(),
                }))
            }
            Err(e) => {
                if let IntervalsError::NotFound(_) = &e {
                    return Err(IntervalsError::Validation(ValidationError::InvalidFormat {
                        field: "activity".to_string(),
                        value: "activity has no streams".to_string(),
                    }));
                }
                Err(e)
            }
        }
    }

    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<crate::ActivitySummary>> {
        if query.trim().is_empty() {
            return Err(IntervalsError::Validation(ValidationError::InvalidFormat {
                field: "query".to_string(),
                value: "query must not be empty".to_string(),
            }));
        }
        let url = format!(
            "{}/api/v1/athlete/{}/activities/search",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![("q", query.to_string())];
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value> {
        if query.trim().is_empty() {
            return Err(IntervalsError::Validation(ValidationError::InvalidFormat {
                field: "query".to_string(),
                value: "query must not be empty".to_string(),
            }));
        }
        let url = format!(
            "{}/api/v1/athlete/{}/activities/search-full",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![("q", query.to_string())];
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_activities_csv(&self) -> Result<String> {
        let url = format!(
            "{}/api/v1/athlete/{}/activities.csv",
            self.base_url, self.athlete_id
        );
        self.execute_text(self.get_request(&url)).await
    }

    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_json(self.put_request(&url).json(fields)).await
    }

    async fn delete_activity(&self, activity_id: &str) -> Result<()> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_empty(self.delete_request(&url)).await
    }

    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "activities-around"]);
        let mut pairs: Vec<(&str, String)> = Vec::new();
        pairs.push(("activity_id", activity_id.to_string()));
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        if let Some(r) = route_id {
            pairs.push(("route_id", r.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<crate::DownloadProgress>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/file", self.base_url, activity_id);
        let resp = self.get_request(&url).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }

        let total = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        if let Some(path) = output_path {
            let mut stream = resp.bytes_stream();
            let mut file = tokio::fs::File::create(&path)
                .await
                .map_err(|e| IntervalsError::Config(crate::ConfigError::Other(e.to_string())))?;
            let mut downloaded: u64 = 0;

            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            return Err(IntervalsError::Config(crate::ConfigError::Other("download cancelled".to_string())));
                        }
                        continue;
                    }
                    c = stream.next() => c,
                };

                let Some(chunk) = chunk else { break };

                let bytes = chunk.map_err(IntervalsError::Http)?;
                file.write_all(&bytes).await.map_err(|e| {
                    IntervalsError::Config(crate::ConfigError::Other(e.to_string()))
                })?;
                downloaded = downloaded.saturating_add(bytes.len() as u64);

                let _ = progress_tx.try_send(crate::DownloadProgress {
                    bytes_downloaded: downloaded,
                    total_bytes: total,
                });

                if *cancel_rx.borrow() {
                    return Err(IntervalsError::Config(crate::ConfigError::Other(
                        "download cancelled".to_string(),
                    )));
                }
            }

            file.sync_all()
                .await
                .map_err(|e| IntervalsError::Config(crate::ConfigError::Other(e.to_string())))?;
            Ok(Some(path.to_string_lossy().to_string()))
        } else {
            let mut stream = resp.bytes_stream();
            let mut downloaded: u64 = 0;
            let mut all_bytes = Vec::new();

            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(IntervalsError::Http)?;
                downloaded = downloaded.saturating_add(bytes.len() as u64);
                all_bytes.extend_from_slice(&bytes);

                let _ = progress_tx.try_send(crate::DownloadProgress {
                    bytes_downloaded: downloaded,
                    total_bytes: total,
                });

                if *cancel_rx.borrow() {
                    return Err(IntervalsError::Config(crate::ConfigError::Other(
                        "download cancelled".to_string(),
                    )));
                }
            }

            Ok(Some(STANDARD.encode(&all_bytes)))
        }
    }

    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/fit-file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        let url = format!("{}/api/v1/activity/{}/gpx-file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    async fn get_gap_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/gap-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
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
        let url = self.api_url(&["athlete", &self.athlete_id, "activities", "interval-search"]);
        let pairs = crate::utils::QueryBuilder::new()
            .add("minSecs", min_secs)
            .add("maxSecs", max_secs)
            .add("minIntensity", min_intensity)
            .add("maxIntensity", max_intensity)
            .add_opt("type", interval_type.as_ref())
            .add_opt("minReps", min_reps)
            .add_opt("maxReps", max_reps)
            .add_opt("limit", limit)
            .build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_power_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/power-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_hr_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/hr-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_pace_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/pace-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }
}

#[async_trait]
impl EventService for ReqwestIntervalsClient {
    async fn create_event(&self, event: crate::Event) -> Result<crate::Event> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events"]);

        let mut ev = event;
        ev.start_date_local =
            Self::normalize_event_start(&ev.start_date_local).ok_or_else(|| {
                IntervalsError::Validation(ValidationError::InvalidFormat {
                    field: "start_date_local".to_string(),
                    value: format!("invalid start_date_local: {}", ev.start_date_local),
                })
            })?;

        let resp = self.post_request(&url).json(&ev).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        Ok(resp.json().await?)
    }

    async fn get_event(&self, event_id: &str) -> Result<crate::Event> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", event_id]);
        let resp = self.get_request(&url).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        let text = resp.text().await?;
        serde_json::from_str::<crate::Event>(&text).map_err(|e| {
            let body_snippet: String = text.chars().take(512).collect();
            IntervalsError::Config(crate::ConfigError::Other(format!(
                "decoding event: {} - body: {}",
                e, body_snippet
            )))
        })
    }

    async fn delete_event(&self, event_id: &str) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", event_id]);
        self.execute_empty(self.delete_request(&url)).await
    }

    async fn get_events(
        &self,
        days_back: Option<i32>,
        limit: Option<u32>,
    ) -> Result<Vec<crate::Event>> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events"]);
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(d) = days_back {
            let today = Utc::now().date_naive();
            let oldest = today - Duration::days(d as i64);
            pairs.push(("oldest", oldest.to_string()));
            pairs.push(("newest", today.to_string()));
        }
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        self.execute_json(self.get_request(&url).query(&self.build_query(&pairs)))
            .await
    }

    async fn bulk_create_events(&self, events: Vec<crate::Event>) -> Result<Vec<crate::Event>> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/bulk",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.post_request(&url).json(&events))
            .await
    }

    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events"]);
        let mut pairs: Vec<(&str, String)> = Vec::new();
        let today = Utc::now().date_naive();
        let newest = today + Duration::days(days_ahead.unwrap_or(7) as i64);
        pairs.push(("oldest", today.to_string()));
        pairs.push(("newest", newest.to_string()));
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        if let Some(c) = category {
            pairs.push(("category", c));
        }
        self.execute_json(self.get_request(&url).query(&self.build_query(&pairs)))
            .await
    }

    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", event_id]);
        let normalized_fields = Self::normalize_event_update_fields(fields)?;
        self.execute_json(self.put_request(&url).json(&normalized_fields))
            .await
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", "bulk-delete"]);
        let doomed: Vec<serde_json::Value> = event_ids
            .iter()
            .map(|id| serde_json::json!({ "id": id.parse::<i32>().unwrap_or(0) }))
            .collect();
        self.execute_empty(self.put_request(&url).json(&doomed))
            .await
    }

    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<crate::Event>> {
        let url = self.api_url(&["athlete", &self.athlete_id, "duplicate-events"]);
        let body = serde_json::json!({
            "eventIds": [event_id],
            "numCopies": num_copies,
            "weeksBetween": weeks_between
        });
        self.execute_json(self.post_request(&url).json(&body)).await
    }
}

#[async_trait]
impl FitnessService for ReqwestIntervalsClient {
    async fn get_fitness_summary(&self) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/athlete/{}/athlete-summary.json",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/athlete/{}/activity-power-curves",
            self.base_url, self.athlete_id
        );
        let today = chrono::Utc::now().date_naive();
        let oldest = if let Some(days) = days_back {
            today - chrono::Duration::days(days as i64)
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
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/athlete/{}/activity-hr-curves",
            self.base_url, self.athlete_id
        );
        let today = chrono::Utc::now().date_naive();
        let oldest = if let Some(days) = days_back {
            today - chrono::Duration::days(days as i64)
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
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/athlete/{}/activity-pace-curves",
            self.base_url, self.athlete_id
        );
        let today = chrono::Utc::now().date_naive();
        let oldest = if let Some(days) = days_back {
            today - chrono::Duration::days(days as i64)
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
        self.execute_json(self.get_request(&url).query(&qp)).await
    }
}

#[async_trait]
impl GearService for ReqwestIntervalsClient {
    async fn get_gear_list(&self) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear"]);
        self.execute_json(self.get_request(&url)).await
    }

    async fn create_gear(&self, gear: &serde_json::Value) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear"]);
        self.execute_json(self.post_request(&url).json(gear)).await
    }

    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear", gear_id]);
        self.execute_json(self.put_request(&url).json(fields)).await
    }

    async fn delete_gear(&self, gear_id: &str) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear", gear_id]);
        self.execute_empty(self.delete_request(&url)).await
    }

    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear", gear_id, "reminder"]);
        self.execute_json(self.post_request(&url).json(reminder))
            .await
    }

    async fn update_gear_reminder(
        &self,
        gear_id: &str,
        reminder_id: &str,
        reset: bool,
        snooze_days: u32,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "gear",
            gear_id,
            "reminder",
            reminder_id,
        ]);
        let pairs = crate::utils::QueryBuilder::new()
            .add("reset", reset)
            .add("snoozeDays", snooze_days)
            .build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.put_request(&url).query(&qp).json(fields))
            .await
    }
}

#[async_trait]
impl WellnessService for ReqwestIntervalsClient {
    async fn get_wellness(&self, days_back: Option<i32>) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness"]);
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(d) = days_back {
            let today = Utc::now().date_naive();
            let oldest = today - Duration::days(d as i64);
            pairs.push(("oldest", oldest.to_string()));
            pairs.push(("newest", today.to_string()));
        }
        self.execute_json(self.get_request(&url).query(&self.build_query(&pairs)))
            .await
    }

    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness", date]);
        self.execute_json(self.get_request(&url)).await
    }

    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness", date]);
        self.execute_json(self.put_request(&url).json(data)).await
    }

    async fn update_wellness_bulk(&self, entries: &[serde_json::Value]) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness-bulk"]);
        self.execute_empty(self.put_request(&url).json(entries))
            .await
    }
}

#[async_trait]
impl WeatherService for ReqwestIntervalsClient {
    async fn get_weather_config(&self) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "weather-config"]);
        self.execute_json(self.get_request(&url)).await
    }

    async fn update_weather_config(&self, config: &serde_json::Value) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "weather-config"]);
        self.execute_json(self.put_request(&url).json(config)).await
    }
}

#[async_trait]
impl RouteService for ReqwestIntervalsClient {
    async fn list_routes(&self) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "routes"]);
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_route(&self, route_id: i64, include_path: bool) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "routes", &route_id.to_string()]);
        let pairs = crate::utils::QueryBuilder::new()
            .add("includePath", include_path)
            .build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn update_route(
        &self,
        route_id: i64,
        route: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "routes", &route_id.to_string()]);
        self.execute_json(self.put_request(&url).json(route)).await
    }

    async fn get_route_similarity(
        &self,
        route_id: i64,
        other_id: i64,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "routes",
            &route_id.to_string(),
            "similarity",
            &other_id.to_string(),
        ]);
        self.execute_json(self.get_request(&url)).await
    }
}

#[async_trait]
impl WorkoutService for ReqwestIntervalsClient {
    async fn get_workout_library(&self) -> Result<serde_json::Value> {
        // API returns folders, plans and workouts together
        let url = self.api_url(&["athlete", &self.athlete_id, "folders"]);
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_workouts_in_folder(&self, _folder_id: &str) -> Result<serde_json::Value> {
        // API doesn't have a direct endpoint - return all folders and let client filter
        // For now, return the full library response
        self.get_workout_library().await
    }

    async fn create_folder(&self, folder: &serde_json::Value) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "folders"]);
        self.execute_json(self.post_request(&url).json(folder))
            .await
    }

    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "folders", folder_id]);
        self.execute_json(self.put_request(&url).json(fields)).await
    }

    async fn delete_folder(&self, folder_id: &str) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "folders", folder_id]);
        self.execute_empty(self.delete_request(&url)).await
    }
}

#[async_trait]
impl SportSettingsService for ReqwestIntervalsClient {
    async fn get_sport_settings(&self) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "sport-settings"]);
        self.execute_json(self.get_request(&url)).await
    }

    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let sport_settings_id = self.resolve_sport_settings_id(sport_type).await?;
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "sport-settings",
            &sport_settings_id,
        ]);
        let mut body = fields.clone();
        if let Some(obj) = body.as_object_mut() {
            obj.insert(
                "recalc_hr_zones".to_string(),
                serde_json::json!(recalc_hr_zones),
            );
        }
        self.execute_json(self.put_request(&url).json(&body)).await
    }

    async fn apply_sport_settings(&self, sport_type: &str) -> Result<serde_json::Value> {
        let sport_settings_id = self.resolve_sport_settings_id(sport_type).await?;
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "sport-settings",
            &sport_settings_id,
            "apply",
        ]);
        self.execute_json(self.put_request(&url)).await
    }

    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "sport-settings"]);
        self.execute_json(self.post_request(&url).json(settings))
            .await
    }

    async fn delete_sport_settings(&self, sport_type: &str) -> Result<()> {
        let sport_settings_id = self.resolve_sport_settings_id(sport_type).await?;
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "sport-settings",
            &sport_settings_id,
        ]);
        self.execute_empty(self.delete_request(&url)).await
    }
}

#[async_trait::async_trait]
impl crate::IntervalsClient for ReqwestIntervalsClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile> {
        <Self as AthleteService>::get_athlete_profile(self).await
    }

    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<crate::ActivitySummary>> {
        <Self as ActivityService>::get_recent_activities(self, limit, days_back).await
    }

    async fn create_event(&self, event: crate::Event) -> Result<crate::Event> {
        <Self as EventService>::create_event(self, event).await
    }

    async fn get_event(&self, event_id: &str) -> Result<crate::Event> {
        <Self as EventService>::get_event(self, event_id).await
    }

    async fn delete_event(&self, event_id: &str) -> Result<()> {
        <Self as EventService>::delete_event(self, event_id).await
    }

    async fn get_events(
        &self,
        days_back: Option<i32>,
        limit: Option<u32>,
    ) -> Result<Vec<crate::Event>> {
        <Self as EventService>::get_events(self, days_back, limit).await
    }

    async fn bulk_create_events(&self, events: Vec<crate::Event>) -> Result<Vec<crate::Event>> {
        <Self as EventService>::bulk_create_events(self, events).await
    }

    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_activity_streams(self, activity_id, streams).await
    }

    async fn get_activity_intervals(&self, activity_id: &str) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_activity_intervals(self, activity_id).await
    }

    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<crate::BestEffortsOptions>,
    ) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_best_efforts(self, activity_id, options).await
    }

    async fn get_activity_details(&self, activity_id: &str) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_activity_details(self, activity_id).await
    }

    async fn get_activity_messages(&self, activity_id: &str) -> Result<Vec<ActivityMessage>> {
        <Self as ActivityService>::get_activity_messages(self, activity_id).await
    }

    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<crate::ActivitySummary>> {
        <Self as ActivityService>::search_activities(self, query, limit).await
    }

    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value> {
        <Self as ActivityService>::search_activities_full(self, query, limit).await
    }

    async fn get_activities_csv(&self) -> Result<String> {
        <Self as ActivityService>::get_activities_csv(self).await
    }

    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as ActivityService>::update_activity(self, activity_id, fields).await
    }

    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        <Self as ActivityService>::download_activity_file(self, activity_id, output_path).await
    }

    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<crate::DownloadProgress>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>> {
        <Self as ActivityService>::download_activity_file_with_progress(
            self,
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
        <Self as ActivityService>::download_fit_file(self, activity_id, output_path).await
    }

    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>> {
        <Self as ActivityService>::download_gpx_file(self, activity_id, output_path).await
    }

    async fn get_gear_list(&self) -> Result<serde_json::Value> {
        <Self as GearService>::get_gear_list(self).await
    }

    async fn get_sport_settings(&self) -> Result<serde_json::Value> {
        <Self as SportSettingsService>::get_sport_settings(self).await
    }

    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        <Self as FitnessService>::get_power_curves(self, days_back, sport).await
    }

    async fn get_gap_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_gap_histogram(self, activity_id).await
    }

    async fn delete_activity(&self, activity_id: &str) -> Result<()> {
        <Self as ActivityService>::delete_activity(self, activity_id).await
    }

    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_activities_around(self, activity_id, limit, route_id).await
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
        <Self as ActivityService>::search_intervals(
            self,
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
        <Self as ActivityService>::get_power_histogram(self, activity_id).await
    }

    async fn get_hr_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_hr_histogram(self, activity_id).await
    }

    async fn get_pace_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        <Self as ActivityService>::get_pace_histogram(self, activity_id).await
    }

    async fn get_fitness_summary(&self) -> Result<serde_json::Value> {
        <Self as FitnessService>::get_fitness_summary(self).await
    }

    async fn get_wellness(&self, days_back: Option<i32>) -> Result<serde_json::Value> {
        <Self as WellnessService>::get_wellness(self, days_back).await
    }

    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value> {
        <Self as WellnessService>::get_wellness_for_date(self, date).await
    }

    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as WellnessService>::update_wellness(self, date, data).await
    }

    async fn update_wellness_bulk(&self, entries: &[serde_json::Value]) -> Result<()> {
        <Self as WellnessService>::update_wellness_bulk(self, entries).await
    }

    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value> {
        <Self as EventService>::get_upcoming_workouts(self, days_ahead, limit, category).await
    }

    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as EventService>::update_event(self, event_id, fields).await
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<()> {
        <Self as EventService>::bulk_delete_events(self, event_ids).await
    }

    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<crate::Event>> {
        <Self as EventService>::duplicate_event(self, event_id, num_copies, weeks_between).await
    }

    async fn get_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        <Self as FitnessService>::get_hr_curves(self, days_back, sport).await
    }

    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        <Self as FitnessService>::get_pace_curves(self, days_back, sport).await
    }

    async fn get_workout_library(&self) -> Result<serde_json::Value> {
        <Self as WorkoutService>::get_workout_library(self).await
    }

    async fn get_workouts_in_folder(&self, folder_id: &str) -> Result<serde_json::Value> {
        <Self as WorkoutService>::get_workouts_in_folder(self, folder_id).await
    }

    async fn create_folder(&self, folder: &serde_json::Value) -> Result<serde_json::Value> {
        <Self as WorkoutService>::create_folder(self, folder).await
    }

    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as WorkoutService>::update_folder(self, folder_id, fields).await
    }

    async fn delete_folder(&self, folder_id: &str) -> Result<()> {
        <Self as WorkoutService>::delete_folder(self, folder_id).await
    }

    async fn create_gear(&self, gear: &serde_json::Value) -> Result<serde_json::Value> {
        <Self as GearService>::create_gear(self, gear).await
    }

    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as GearService>::update_gear(self, gear_id, fields).await
    }

    async fn delete_gear(&self, gear_id: &str) -> Result<()> {
        <Self as GearService>::delete_gear(self, gear_id).await
    }

    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as GearService>::create_gear_reminder(self, gear_id, reminder).await
    }

    async fn update_gear_reminder(
        &self,
        gear_id: &str,
        reminder_id: &str,
        reset: bool,
        snooze_days: u32,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as GearService>::update_gear_reminder(
            self,
            gear_id,
            reminder_id,
            reset,
            snooze_days,
            fields,
        )
        .await
    }

    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as SportSettingsService>::update_sport_settings(
            self,
            sport_type,
            recalc_hr_zones,
            fields,
        )
        .await
    }

    async fn apply_sport_settings(&self, sport_type: &str) -> Result<serde_json::Value> {
        <Self as SportSettingsService>::apply_sport_settings(self, sport_type).await
    }

    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as SportSettingsService>::create_sport_settings(self, settings).await
    }

    async fn delete_sport_settings(&self, sport_type: &str) -> Result<()> {
        <Self as SportSettingsService>::delete_sport_settings(self, sport_type).await
    }

    async fn get_weather_config(&self) -> Result<serde_json::Value> {
        <Self as WeatherService>::get_weather_config(self).await
    }

    async fn update_weather_config(&self, config: &serde_json::Value) -> Result<serde_json::Value> {
        <Self as WeatherService>::update_weather_config(self, config).await
    }

    async fn list_routes(&self) -> Result<serde_json::Value> {
        <Self as RouteService>::list_routes(self).await
    }

    async fn get_route(&self, route_id: i64, include_path: bool) -> Result<serde_json::Value> {
        <Self as RouteService>::get_route(self, route_id, include_path).await
    }

    async fn update_route(
        &self,
        route_id: i64,
        route: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        <Self as RouteService>::update_route(self, route_id, route).await
    }

    async fn get_route_similarity(
        &self,
        route_id: i64,
        other_id: i64,
    ) -> Result<serde_json::Value> {
        <Self as RouteService>::get_route_similarity(self, route_id, other_id).await
    }
}

/// Extract available stream names from a JSON response.
fn extract_available_streams(json: &serde_json::Value) -> Vec<String> {
    let mut available_streams = Vec::new();

    if let Some(sv) = json.get("streams") {
        if let Some(obj) = sv.as_object() {
            available_streams.extend(obj.keys().cloned());
        } else if let Some(arr) = sv.as_array() {
            for item in arr.iter() {
                if let Some(obj) = item.as_object() {
                    if let Some(name) = obj.get("name").and_then(|n| n.as_str()) {
                        available_streams.push(name.to_string());
                    } else if let Some(t) = obj.get("type").and_then(|n| n.as_str()) {
                        available_streams.push(t.to_string());
                    }
                }
            }
        }
    } else if let Some(arr) = json.as_array() {
        for item in arr.iter() {
            if let Some(obj) = item.as_object() {
                if let Some(name) = obj.get("name").and_then(|n| n.as_str())
                    && !name.is_empty()
                {
                    available_streams.push(name.to_string());
                } else if let Some(t) = obj.get("type").and_then(|n| n.as_str()) {
                    available_streams.push(t.to_string());
                }
            }
        }
    } else if let Some(obj) = json.as_object() {
        for (k, v) in obj.iter() {
            if v.is_array() {
                available_streams.push(k.clone());
            }
        }
    }

    available_streams
}

fn annotate_best_efforts_payload(
    mut value: serde_json::Value,
    stream: Option<&str>,
) -> serde_json::Value {
    let Some(stream) = stream else {
        return value;
    };

    if let Some(obj) = value.as_object_mut()
        && !obj.contains_key("stream")
    {
        obj.insert("stream".to_string(), serde_json::json!(stream));
    }

    value
}

#[cfg(test)]
mod tests {
    use crate::{IntervalsClient, http_client::ReqwestIntervalsClient};
    use chrono::{Duration, Utc};
    use secrecy::SecretString;
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const LIVE_OPENAPI_SPEC_URL: &str = "https://intervals.icu/api/v1/docs";

    async fn fetch_live_openapi_spec() -> serde_json::Value {
        reqwest::Client::new()
            .get(LIVE_OPENAPI_SPEC_URL)
            .send()
            .await
            .expect("fetch live OpenAPI spec")
            .error_for_status()
            .expect("live OpenAPI status")
            .json()
            .await
            .expect("parse live OpenAPI spec")
    }

    fn spec_operation<'a>(
        spec: &'a serde_json::Value,
        path_name: &str,
        method_name: &str,
    ) -> &'a serde_json::Value {
        spec.get("paths")
            .and_then(|paths| paths.get(path_name))
            .and_then(|path_item| path_item.get(method_name))
            .unwrap_or_else(|| panic!("missing {method_name} {path_name} in live spec"))
    }

    fn assert_query_param(
        spec: &serde_json::Value,
        path_name: &str,
        method_name: &str,
        param_name: &str,
    ) {
        let params = spec_operation(spec, path_name, method_name)
            .get("parameters")
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("missing parameters for {method_name} {path_name}"));
        assert!(
            params.iter().any(|param| {
                param.get("in").and_then(serde_json::Value::as_str) == Some("query")
                    && param.get("name").and_then(serde_json::Value::as_str) == Some(param_name)
            }),
            "missing query param {param_name} for {method_name} {path_name}"
        );
    }

    #[tokio::test]
    async fn client_new_and_basic() {
        let client =
            ReqwestIntervalsClient::new("http://localhost", "ath", SecretString::new("key".into()));
        let _ = client;
    }

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

    #[tokio::test]
    async fn get_activities_around_uses_activities_around_path() {
        let mock_server = MockServer::start().await;
        let athlete = "ath";

        Mock::given(method("GET"))
            .and(path(format!(
                "/api/v1/athlete/{}/activities-around",
                athlete
            )))
            .and(query_param("activity_id", "a1"))
            .and(query_param("limit", "5"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        let client = ReqwestIntervalsClient::new(
            &mock_server.uri(),
            athlete,
            SecretString::new("key".into()),
        );
        let res = client.get_activities_around("a1", Some(5), None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn apply_sport_settings_uses_put() {
        let mock_server = MockServer::start().await;
        let sport = "Run";
        let athlete = "ath";

        Mock::given(method("GET"))
            .and(path(format!("/api/v1/athlete/{}/sport-settings", athlete)))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": 1783043, "types": ["Run", "VirtualRun", "TrailRun"]}
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path(format!(
                "/api/v1/athlete/{}/sport-settings/{}/apply",
                athlete, 1783043
            )))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"status":"ok"})),
            )
            .mount(&mock_server)
            .await;

        let client = ReqwestIntervalsClient::new(
            &mock_server.uri(),
            athlete,
            SecretString::new("key".into()),
        );
        let res = client.apply_sport_settings(sport).await;
        assert!(res.is_ok());
        let v = res.unwrap();
        assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
    }

    #[tokio::test]
    async fn get_wellness_translates_days_back_to_oldest_and_newest() {
        let mock_server = MockServer::start().await;
        let athlete = "ath";
        let today = Utc::now().date_naive();
        let oldest = today - Duration::days(5);

        Mock::given(method("GET"))
            .and(path(format!("/api/v1/athlete/{}/wellness", athlete)))
            .and(query_param("oldest", oldest.to_string()))
            .and(query_param("newest", today.to_string()))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .mount(&mock_server)
            .await;

        let client = ReqwestIntervalsClient::new(
            &mock_server.uri(),
            athlete,
            SecretString::new("key".into()),
        );
        let res = client.get_wellness(Some(5)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_events_translates_days_back_to_oldest_and_newest() {
        let mock_server = MockServer::start().await;
        let athlete = "ath";
        let today = Utc::now().date_naive();
        let oldest = today - Duration::days(7);

        Mock::given(method("GET"))
            .and(path(format!("/api/v1/athlete/{}/events", athlete)))
            .and(query_param("oldest", oldest.to_string()))
            .and(query_param("newest", today.to_string()))
            .and(query_param("limit", "3"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {
                    "id": "evt-1",
                    "start_date_local": today.to_string(),
                    "name": "Workout",
                    "category": "WORKOUT"
                }
            ])))
            .mount(&mock_server)
            .await;

        let client = ReqwestIntervalsClient::new(
            &mock_server.uri(),
            athlete,
            SecretString::new("key".into()),
        );
        let res = client.get_events(Some(7), Some(3)).await;
        assert!(res.is_ok());
        assert_eq!(res.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn download_fit_file_uses_fit_file_endpoint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/activity/a1/fit-file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![1u8, 2, 3]))
            .mount(&mock_server)
            .await;

        let client =
            ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()));

        let res = client.download_fit_file("a1", None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn download_gpx_file_uses_gpx_file_endpoint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/activity/a1/gpx-file"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(vec![4u8, 5, 6]))
            .mount(&mock_server)
            .await;

        let client =
            ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()));

        let res = client.download_gpx_file("a1", None).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn create_gear_reminder_uses_singular_reminder_endpoint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/api/v1/athlete/ath/gear/g1/reminder"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id":"g1"})))
            .mount(&mock_server)
            .await;

        let client =
            ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()));

        let res = client
            .create_gear_reminder("g1", &serde_json::json!({"note":"check chain"}))
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_wellness_bulk_uses_bulk_endpoint() {
        let mock_server = MockServer::start().await;

        Mock::given(method("PUT"))
            .and(path("/api/v1/athlete/ath/wellness-bulk"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let client =
            ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()));
        let res = client
            .update_wellness_bulk(&[serde_json::json!({"id": "2026-03-01", "sleepSecs": 28800})])
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn weather_config_uses_spec_endpoints() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/athlete/ath/weather-config"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"provider": "yr.no"})),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/api/v1/athlete/ath/weather-config"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"provider": "open-meteo"})),
            )
            .mount(&mock_server)
            .await;

        let client =
            ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()));

        let current = client.get_weather_config().await.expect("weather config");
        assert_eq!(
            current.get("provider").and_then(|v| v.as_str()),
            Some("yr.no")
        );

        let updated = client
            .update_weather_config(&serde_json::json!({"provider": "open-meteo"}))
            .await
            .expect("update weather config");
        assert_eq!(
            updated.get("provider").and_then(|v| v.as_str()),
            Some("open-meteo")
        );
    }

    #[tokio::test]
    async fn routes_use_current_spec_paths() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/api/v1/athlete/ath/routes"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"route": {"id": 11}, "count": 4}
            ])))
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/athlete/ath/routes/11"))
            .and(query_param("includePath", "true"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": 11, "name": "Lunch Loop"})),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("PUT"))
            .and(path("/api/v1/athlete/ath/routes/11"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": 11, "name": "Updated Loop"})),
            )
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path("/api/v1/athlete/ath/routes/11/similarity/12"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"similarity": 0.97})),
            )
            .mount(&mock_server)
            .await;

        let client =
            ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()));

        let routes = client.list_routes().await.expect("list routes");
        assert_eq!(routes.as_array().map(Vec::len), Some(1));

        let route = client.get_route(11, true).await.expect("get route");
        assert_eq!(route.get("id").and_then(|v| v.as_i64()), Some(11));

        let updated = client
            .update_route(11, &serde_json::json!({"name": "Updated Loop"}))
            .await
            .expect("update route");
        assert_eq!(
            updated.get("name").and_then(|v| v.as_str()),
            Some("Updated Loop")
        );

        let similarity = client
            .get_route_similarity(11, 12)
            .await
            .expect("route similarity");
        assert_eq!(
            similarity.get("similarity").and_then(|v| v.as_f64()),
            Some(0.97)
        );
    }

    #[tokio::test]
    #[ignore = "hits the live Intervals.icu OpenAPI endpoint"]
    async fn live_openapi_spec_contract_smoke() {
        let spec = fetch_live_openapi_spec().await;

        spec_operation(&spec, "/api/v1/activity/{id}/fit-file", "get");
        spec_operation(&spec, "/api/v1/activity/{id}/gpx-file", "get");
        spec_operation(
            &spec,
            "/api/v1/athlete/{id}/gear/{gearId}/reminder/{reminderId}",
            "put",
        );
        spec_operation(&spec, "/api/v1/athlete/{id}/weather-config", "get");
        spec_operation(&spec, "/api/v1/athlete/{id}/weather-config", "put");
        spec_operation(&spec, "/api/v1/athlete/{id}/routes", "get");
        spec_operation(&spec, "/api/v1/athlete/{id}/routes/{route_id}", "get");
        spec_operation(&spec, "/api/v1/athlete/{id}/routes/{route_id}", "put");
        spec_operation(
            &spec,
            "/api/v1/athlete/{id}/routes/{route_id}/similarity/{other_id}",
            "get",
        );
        spec_operation(&spec, "/api/v1/athlete/{id}/wellness-bulk", "put");
        spec_operation(
            &spec,
            "/api/v1/athlete/{athleteId}/sport-settings/{id}/apply",
            "put",
        );

        assert_query_param(
            &spec,
            "/api/v1/athlete/{id}/activities-around",
            "get",
            "activity_id",
        );
        assert_query_param(
            &spec,
            "/api/v1/athlete/{id}/activities-around",
            "get",
            "route_id",
        );
        assert_query_param(
            &spec,
            "/api/v1/athlete/{id}/events{format}",
            "get",
            "oldest",
        );
        assert_query_param(
            &spec,
            "/api/v1/athlete/{id}/events{format}",
            "get",
            "newest",
        );
        assert_query_param(&spec, "/api/v1/athlete/{id}/wellness{ext}", "get", "oldest");
        assert_query_param(&spec, "/api/v1/athlete/{id}/wellness{ext}", "get", "newest");
        assert_query_param(
            &spec,
            "/api/v1/athlete/{id}/routes/{route_id}",
            "get",
            "includePath",
        );
    }
}
