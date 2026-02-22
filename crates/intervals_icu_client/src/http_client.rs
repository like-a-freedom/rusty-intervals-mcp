//! HTTP client implementation for the Intervals.icu API.
//!
//! This module provides a reqwest-based implementation of the [`IntervalsClient`](crate::IntervalsClient) trait.

use crate::{AthleteProfile, IntervalsClient, IntervalsError};
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
    ) -> Result<T, IntervalsError> {
        let resp = request.send().await?;
        self.handle_response(resp).await
    }

    /// Execute a request and expect a text response.
    async fn execute_text(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<String, IntervalsError> {
        let resp = request.send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        Ok(resp.text().await?)
    }

    /// Execute a request with no expected response body.
    async fn execute_empty(&self, request: reqwest::RequestBuilder) -> Result<(), IntervalsError> {
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
    ) -> Result<T, IntervalsError> {
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
            422 => IntervalsError::InvalidInput(body_snippet),
            _ => IntervalsError::from_status(status, body_snippet),
        }
    }

    /// Download a file from a URL, optionally saving to disk.
    async fn download_file(
        &self,
        url: String,
        output_path: Option<PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        let resp = self.get_request(&url).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }

        if let Some(path) = output_path {
            let mut stream = resp.bytes_stream();
            let mut file = tokio::fs::File::create(&path)
                .await
                .map_err(|e| IntervalsError::Config(e.to_string()))?;
            while let Some(chunk) = stream.next().await {
                let bytes = chunk.map_err(IntervalsError::Http)?;
                file.write_all(&bytes)
                    .await
                    .map_err(|e| IntervalsError::Config(e.to_string()))?;
            }
            file.sync_all()
                .await
                .map_err(|e| IntervalsError::Config(e.to_string()))?;
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
        let first = chrs.next().unwrap().to_uppercase().collect::<String>();
        format!("{}{}", first, chrs.as_str())
    }

    /// Normalize start_date_local for events: preserve time when provided;
    /// if only date is given, set time to 00:00:00.
    fn normalize_event_start(s: &str) -> Option<String> {
        if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
            return Some(format!("{}T00:00:00", s));
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Some(dt.naive_local().format("%Y-%m-%dT%H:%M:%S").to_string());
        }
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Some(ndt.format("%Y-%m-%dT%H:%M:%S").to_string());
        }
        None
    }
}

#[async_trait]
impl IntervalsClient for ReqwestIntervalsClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/profile",
            self.base_url, self.athlete_id
        );
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
            .ok_or_else(|| IntervalsError::Config("missing athlete profile data".into()))
    }

    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<crate::ActivitySummary>, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/activities",
            self.base_url, self.athlete_id
        );
        let today = Utc::now().date_naive();
        let oldest = today - Duration::days(days_back.unwrap_or(7) as i64);

        let mut pairs: Vec<(&str, String)> = vec![
            ("oldest", oldest.to_string()),
            ("newest", today.to_string()),
        ];
        if let Some(limit) = limit {
            pairs.push(("limit", limit.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();

        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn create_event(&self, event: crate::Event) -> Result<crate::Event, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events",
            self.base_url, self.athlete_id
        );

        let mut ev = event;
        ev.start_date_local =
            Self::normalize_event_start(&ev.start_date_local).ok_or_else(|| {
                IntervalsError::InvalidInput(format!(
                    "invalid start_date_local: {}",
                    ev.start_date_local
                ))
            })?;

        let resp = self.post_request(&url).json(&ev).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        Ok(resp.json().await?)
    }

    async fn get_event(&self, event_id: &str) -> Result<crate::Event, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/{}",
            self.base_url, self.athlete_id, event_id
        );
        let resp = self.get_request(&url).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        // Read body as text first so we can provide a helpful error message
        // when the returned JSON doesn't match the expected `Event` shape.
        let text = resp.text().await?;
        serde_json::from_str::<crate::Event>(&text).map_err(|e| {
            let body_snippet: String = text.chars().take(512).collect();
            IntervalsError::Config(format!("decoding event: {} - body: {}", e, body_snippet))
        })
    }

    async fn delete_event(&self, event_id: &str) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/{}",
            self.base_url, self.athlete_id, event_id
        );
        self.execute_empty(self.delete_request(&url)).await
    }

    async fn get_events(
        &self,
        days_back: Option<i32>,
        limit: Option<u32>,
    ) -> Result<Vec<crate::Event>, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(d) = days_back {
            pairs.push(("days_back", d.to_string()));
        }
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn bulk_create_events(
        &self,
        events: Vec<crate::Event>,
    ) -> Result<Vec<crate::Event>, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/bulk",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.post_request(&url).json(&events))
            .await
    }

    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}/streams", self.base_url, activity_id);
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(s) = streams {
            pairs.push(("streams", s.join(",")));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_activity_intervals(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/intervals",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<crate::BestEffortsOptions>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/best-efforts",
            self.base_url, activity_id
        );

        if let Some(opts) = options {
            if opts.stream.is_none() {
                return Err(IntervalsError::InvalidInput(
                    "missing stream in best-efforts options".into(),
                ));
            }
            if opts.duration.is_none() && opts.distance.is_none() {
                return Err(IntervalsError::InvalidInput(
                    "missing duration or distance in best-efforts options".into(),
                ));
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

            return self.execute_json(self.get_request(&url).query(&q)).await;
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
                return Ok(resp.json().await?);
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
                let candidates = ["power", "speed", "pace", "distance", "hr", "watts"];

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
                            return Ok(resp.json().await?);
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

                Err(IntervalsError::InvalidInput(
                    "no suitable best efforts parameters found".into(),
                ))
            }
            Err(e) => {
                if let IntervalsError::NotFound(_) = &e {
                    return Err(IntervalsError::InvalidInput(
                        "activity has no streams".into(),
                    ));
                }
                Err(e)
            }
        }
    }

    async fn get_activity_details(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_json(self.get_request(&url)).await
    }

    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<crate::ActivitySummary>, IntervalsError> {
        if query.trim().is_empty() {
            return Err(IntervalsError::InvalidInput(
                "query must not be empty".into(),
            ));
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
    ) -> Result<serde_json::Value, IntervalsError> {
        if query.trim().is_empty() {
            return Err(IntervalsError::InvalidInput(
                "query must not be empty".into(),
            ));
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

    async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
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
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_json(self.put_request(&url).json(fields)).await
    }

    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}/file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<crate::DownloadProgress>,
        mut cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, IntervalsError> {
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
                .map_err(|e| IntervalsError::Config(e.to_string()))?;
            let mut downloaded: u64 = 0;

            loop {
                let chunk = tokio::select! {
                    biased;
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            return Err(IntervalsError::Config("download cancelled".into()));
                        }
                        continue;
                    }
                    c = stream.next() => c,
                };

                let Some(chunk) = chunk else { break };

                let bytes = chunk.map_err(IntervalsError::Http)?;
                file.write_all(&bytes)
                    .await
                    .map_err(|e| IntervalsError::Config(e.to_string()))?;
                downloaded = downloaded.saturating_add(bytes.len() as u64);

                let _ = progress_tx.try_send(crate::DownloadProgress {
                    bytes_downloaded: downloaded,
                    total_bytes: total,
                });

                if *cancel_rx.borrow() {
                    return Err(IntervalsError::Config("download cancelled".into()));
                }
            }

            file.sync_all()
                .await
                .map_err(|e| IntervalsError::Config(e.to_string()))?;
            Ok(Some(path.to_string_lossy().to_string()))
        } else {
            let bytes = resp.bytes().await?;
            let len = bytes.len() as u64;
            let _ = progress_tx.try_send(crate::DownloadProgress {
                bytes_downloaded: len,
                total_bytes: Some(len),
            });
            Ok(Some(STANDARD.encode(&bytes)))
        }
    }

    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}/fit-file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}/gpx-file", self.base_url, activity_id);
        self.download_file(url, output_path).await
    }

    async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/athlete/{}/gear", self.base_url, self.athlete_id);
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/power-curves",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![("type", Self::normalize_sport(sport))];
        if let Some(d) = days_back {
            pairs.push(("curves", format!("{}d", d)));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_gap_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/gap-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn delete_activity(&self, activity_id: &str) -> Result<(), IntervalsError> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_empty(self.delete_request(&url)).await
    }

    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/activities-around",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![("activity_id", activity_id.to_string())];
        if let Some(lim) = limit {
            pairs.push(("limit", lim.to_string()));
        }
        if let Some(r) = route_id {
            pairs.push(("route_id", r.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
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
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/activities/interval-search",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![
            ("minSecs", min_secs.to_string()),
            ("maxSecs", max_secs.to_string()),
            ("minIntensity", min_intensity.to_string()),
            ("maxIntensity", max_intensity.to_string()),
        ];
        if let Some(kind) = interval_type {
            pairs.push(("type", kind));
        }
        if let Some(reps) = min_reps {
            pairs.push(("minReps", reps.to_string()));
        }
        if let Some(reps) = max_reps {
            pairs.push(("maxReps", reps.to_string()));
        }
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        self.execute_json(self.get_request(&url).query(&pairs))
            .await
    }

    async fn get_power_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/power-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_hr_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/hr-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_pace_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/pace-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> {
        // Use athlete-summary endpoint which returns fitness/fatigue/form/rampRate data
        // Returns array of SummaryWithCats objects, most recent first
        let url = format!(
            "{}/api/v1/athlete/{}/athlete-summary.json",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_wellness(
        &self,
        days_back: Option<i32>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/wellness",
            self.base_url, self.athlete_id
        );
        let mut req = self.get_request(&url);
        if let Some(d) = days_back {
            let oldest = Utc::now()
                .checked_sub_signed(Duration::days(d as i64))
                .map(|dt| dt.format("%Y-%m-%d").to_string());
            if let Some(o) = oldest {
                req = req.query(&[("oldest", o)]);
            }
        }
        self.execute_json(req).await
    }

    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/wellness/{}",
            self.base_url, self.athlete_id, date
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/wellness/{}",
            self.base_url, self.athlete_id, date
        );
        self.execute_json(self.put_request(&url).json(data)).await
    }

    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events",
            self.base_url, self.athlete_id
        );
        let mut req = self.get_request(&url);
        if let Some(d) = days_ahead {
            req = req.query(&[("days_ahead", d.to_string())]);
        }
        self.execute_json(req).await
    }

    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/{}",
            self.base_url, self.athlete_id, event_id
        );
        self.execute_json(self.put_request(&url).json(fields)).await
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/bulk-delete",
            self.base_url, self.athlete_id
        );
        // API expects array of objects with id field
        let body: Vec<serde_json::Value> = event_ids
            .into_iter()
            .map(|id| serde_json::json!({ "id": id }))
            .collect();
        let resp = self.put_request(&url).json(&body).send().await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        // Response body is ignored (API may return success indicator)
        Ok(())
    }

    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<crate::Event>, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/duplicate-events",
            self.base_url, self.athlete_id
        );
        let mut body = serde_json::json!({
            "eventIds": [event_id]
        });
        if let Some(obj) = body.as_object_mut() {
            if let Some(n) = num_copies {
                obj.insert("numCopies".to_string(), serde_json::json!(n));
            }
            if let Some(w) = weeks_between {
                obj.insert("weeksBetween".to_string(), serde_json::json!(w));
            }
        }
        self.execute_json(self.post_request(&url).json(&body)).await
    }

    async fn get_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/hr-curves",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![("type", Self::normalize_sport(sport))];
        if let Some(d) = days_back {
            pairs.push(("curves", format!("{}d", d)));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/pace-curves",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![("type", Self::normalize_sport(sport))];
        if let Some(d) = days_back {
            pairs.push(("curves", format!("{}d", d)));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        self.execute_json(self.get_request(&url).query(&qp)).await
    }

    async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/folders",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.get_request(&url)).await
    }

    async fn get_workouts_in_folder(
        &self,
        folder_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/workouts",
            self.base_url, self.athlete_id
        );
        let result = self
            .execute_json::<serde_json::Value>(self.get_request(&url))
            .await?;

        // Client-side filtering when folder_id is provided
        if folder_id.is_empty() {
            return Ok(result);
        }

        if let Some(arr) = result.as_array() {
            let filtered: Vec<serde_json::Value> = arr
                .iter()
                .filter(|item| {
                    item.get("folder_id")
                        .and_then(|v| {
                            // Handle both string and numeric folder_id
                            v.as_str()
                                .map(|s| s.to_string())
                                .or_else(|| v.as_i64().map(|i| i.to_string()))
                        })
                        .map(|s| s == folder_id)
                        .unwrap_or(false)
                })
                .cloned()
                .collect();
            Ok(serde_json::Value::Array(filtered))
        } else {
            Ok(result)
        }
    }

    async fn create_folder(
        &self,
        folder: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/folders",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.post_request(&url).json(folder))
            .await
    }

    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/folders/{}",
            self.base_url, self.athlete_id, folder_id
        );
        self.execute_json(self.put_request(&url).json(fields)).await
    }

    async fn delete_folder(&self, folder_id: &str) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/folders/{}",
            self.base_url, self.athlete_id, folder_id
        );
        self.execute_empty(self.delete_request(&url)).await
    }

    async fn create_gear(
        &self,
        gear: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/athlete/{}/gear", self.base_url, self.athlete_id);
        self.execute_json(self.post_request(&url).json(gear)).await
    }

    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/gear/{}",
            self.base_url, self.athlete_id, gear_id
        );
        self.execute_json(self.put_request(&url).json(fields)).await
    }

    async fn delete_gear(&self, gear_id: &str) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/gear/{}",
            self.base_url, self.athlete_id, gear_id
        );
        self.execute_empty(self.delete_request(&url)).await
    }

    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/gear/{}/reminders",
            self.base_url, self.athlete_id, gear_id
        );
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
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/gear/{}/reminder/{}",
            self.base_url, self.athlete_id, gear_id, reminder_id
        );
        let pairs = vec![
            ("reset", reset.to_string()),
            ("snoozeDays", snooze_days.to_string()),
        ];
        self.execute_json(self.put_request(&url).query(&pairs).json(fields))
            .await
    }

    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings/{}",
            self.base_url, self.athlete_id, sport_type
        );
        let pairs = [("recalcHrZones", recalc_hr_zones.to_string())];
        self.execute_json(self.put_request(&url).query(&pairs).json(fields))
            .await
    }

    async fn apply_sport_settings(
        &self,
        sport_type: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings/{}/apply",
            self.base_url, self.athlete_id, sport_type
        );
        self.execute_json(self.put_request(&url)).await
    }

    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.post_request(&url).json(settings))
            .await
    }

    async fn delete_sport_settings(&self, sport_type: &str) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings/{}",
            self.base_url, self.athlete_id, sport_type
        );
        self.execute_empty(self.delete_request(&url)).await
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
    } else if let Some(obj) = json.as_object() {
        for (k, v) in obj.iter() {
            if v.is_array() {
                available_streams.push(k.clone());
            }
        }
    }

    available_streams
}

#[cfg(test)]
mod tests {
    use crate::http_client::ReqwestIntervalsClient;
    use secrecy::SecretString;

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
    fn normalize_event_start_accepts_date_only() {
        let result = ReqwestIntervalsClient::normalize_event_start("2025-12-15");
        assert_eq!(result.unwrap(), "2025-12-15T00:00:00");
    }

    #[test]
    fn normalize_event_start_preserves_datetime() {
        let result = ReqwestIntervalsClient::normalize_event_start("2025-12-15T10:30:00");
        assert_eq!(result.unwrap(), "2025-12-15T10:30:00");
    }

    #[test]
    fn normalize_event_start_rejects_invalid() {
        let result = ReqwestIntervalsClient::normalize_event_start("not-a-date");
        assert!(result.is_none());
    }
}
