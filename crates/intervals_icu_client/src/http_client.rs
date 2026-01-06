use crate::{AthleteProfile, IntervalsClient, IntervalsError};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use futures_util::StreamExt;
use secrecy::{ExposeSecret, SecretString};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

#[derive(Clone, Debug)]
pub struct ReqwestIntervalsClient {
    base_url: String,
    athlete_id: String,
    api_key: SecretString,
    client: reqwest::Client,
}

impl ReqwestIntervalsClient {
    pub fn new(base_url: &str, athlete_id: impl Into<String>, api_key: SecretString) -> Self {
        let client = reqwest::Client::builder().build().expect("reqwest build");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            athlete_id: athlete_id.into(),
            api_key,
            client,
        }
    }

    async fn download_file(
        &self,
        url: String,
        output_path: Option<PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
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
        if bytes.len() <= 1024 * 1024 {
            return Ok(Some(STANDARD.encode(&bytes)));
        }
        Ok(Some(STANDARD.encode(&bytes)))
    }
}

impl ReqwestIntervalsClient {
    /// Map case-insensitive sport names to their canonical API form.
    pub fn normalize_sport(s: &str) -> String {
        // These values are taken from the intervals.icu API enum for sport `type`.
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
        // Fallback: capitalize first char and keep rest as-is (best-effort)
        if s.is_empty() {
            return s.to_string();
        }
        let mut chrs = s.chars();
        let first = chrs.next().unwrap().to_uppercase().collect::<String>();
        format!("{}{}", first, chrs.as_str())
    }
}

#[async_trait]
impl IntervalsClient for ReqwestIntervalsClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/profile",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                status
            )));
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
        if let Some(a) = payload.athlete {
            let id = a.id.unwrap_or_default();
            return Ok(AthleteProfile { id, name: a.name });
        }
        Err(IntervalsError::Config(
            "missing athlete profile data".into(),
        ))
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
        let oldest = if let Some(days) = days_back {
            today - Duration::days(days as i64)
        } else {
            today - Duration::days(7)
        };
        let newest = today;
        let mut pairs: Vec<(&str, String)> = vec![
            ("oldest", oldest.to_string()),
            ("newest", newest.to_string()),
        ];
        if let Some(limit) = limit {
            pairs.push(("limit", limit.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&qp)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        let acts: Vec<crate::ActivitySummary> = resp.json().await?;
        Ok(acts)
    }

    async fn create_event(&self, event: crate::Event) -> Result<crate::Event, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .post(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(&event)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(IntervalsError::Config(format!(
                "unexpected status: {} {}",
                status, body
            )));
        }
        let created: crate::Event = resp.json().await?;
        Ok(created)
    }

    async fn get_event(&self, event_id: &str) -> Result<crate::Event, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/{}",
            self.base_url, self.athlete_id, event_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        // Read body as text first so we can provide a helpful error message
        // when the returned JSON doesn't match the expected `Event` shape.
        let text = resp.text().await?;
        match serde_json::from_str::<crate::Event>(&text) {
            Ok(ev) => Ok(ev),
            Err(e) => {
                // Truncate body to avoid huge error messages
                let body_snippet: String = text.chars().take(512).collect();
                return Err(IntervalsError::Config(format!(
                    "decoding event: {} - body: {}",
                    e, body_snippet
                )));
            }
        }
    }

    async fn delete_event(&self, event_id: &str) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/{}",
            self.base_url, self.athlete_id, event_id
        );
        let resp = self
            .client
            .delete(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )))
        }
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
        let mut pairs: Vec<(&str, String)> = vec![];
        if let Some(d) = days_back {
            // API may support query params; use 'days_back' as example param
            pairs.push(("days_back", d.to_string()));
        }
        if let Some(l) = limit {
            pairs.push(("limit", l.to_string()));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&qp)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        let evs: Vec<crate::Event> = resp.json().await?;
        Ok(evs)
    }

    async fn bulk_create_events(
        &self,
        events: Vec<crate::Event>,
    ) -> Result<Vec<crate::Event>, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/bulk",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .post(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(&events)
            .send()
            .await?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(IntervalsError::Config(format!(
                "unexpected status: {} {}",
                status, body
            )));
        }
        let created: Vec<crate::Event> = resp.json().await?;
        Ok(created)
    }

    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}/streams", self.base_url, activity_id);
        let mut pairs: Vec<(&str, String)> = vec![];
        if let Some(s) = streams {
            pairs.push(("streams", s.join(",")));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&qp)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        let v = resp.json().await?;
        Ok(v)
    }

    async fn get_activity_intervals(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/intervals",
            self.base_url, activity_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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

        // If caller provided explicit options, treat them as authoritative
        if let Some(opts) = options {
            if let Some(s) = opts.stream.as_deref() {
                // require at least one of duration or distance per API contract
                if opts.duration.is_none() && opts.distance.is_none() {
                    return Err(IntervalsError::Config(
                        "missing duration or distance in best-efforts options".into(),
                    ));
                }
                let mut q: Vec<(&str, String)> = Vec::new();
                q.push(("stream", s.to_string()));
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

                let params = q;
                let resp = self
                    .client
                    .get(&url)
                    .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
                    .query(&params)
                    .send()
                    .await?;
                if !resp.status().is_success() {
                    return Err(IntervalsError::Config(format!(
                        "unexpected status: {}",
                        resp.status()
                    )));
                }
                return Ok(resp.json().await?);
            } else {
                return Err(IntervalsError::Config(
                    "missing stream in best-efforts options".into(),
                ));
            }
        }

        // Try a sequence of sensible default parameter combinations when callers
        // only provide the activity id. Some activities (or upstream validation)
        // may accept duration-based efforts, others distance-based; try both to
        // avoid surfacing 422 errors for the common quick-lookup case.
        let attempts = [
            vec![("stream", "power"), ("duration", "60")],
            vec![("stream", "power"), ("distance", "1000")],
            vec![("stream", "power"), ("duration", "300")],
        ];

        for params in attempts.iter() {
            let qp: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, *v)).collect();
            let resp = self
                .client
                .get(&url)
                .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
                .query(&qp)
                .send()
                .await?;

            if resp.status().is_success() {
                return Ok(resp.json().await?);
            }

            if resp.status().as_u16() != 422 {
                // Unexpected non-validation error -> return immediately
                return Err(IntervalsError::Config(format!(
                    "unexpected status: {}",
                    resp.status()
                )));
            }

            // otherwise it's a 422 -> try next fallback
        }

        // All fallbacks yielded 422 — attempt to detect available streams on the activity
        let streams_payload = self.get_activity_streams(activity_id, None).await;
        match streams_payload {
            Ok(json) => {
                // extract the stream keys if present at json.streams
                let mut available_streams: Vec<String> = Vec::new();
                // Streams may be returned as:
                // - top-level object with keys for each stream: { "time": [...], "power": [...] }
                // - nested under "streams": { "streams": { "time": [...], "speed": [...] } }
                // - array under "streams": { "streams": [ {"name": "power"}, {"name": "speed"} ] }
                if let Some(sv) = json.get("streams") {
                    if let Some(obj) = sv.as_object() {
                        for k in obj.keys() {
                            available_streams.push(k.clone());
                        }
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
                    // Top-level streams object: treat keys whose values are arrays/numbers as stream names
                    for (k, v) in obj.iter() {
                        if v.is_array() {
                            available_streams.push(k.clone());
                        }
                    }
                }

                // candidate stream preference
                let candidates = ["power", "speed", "pace", "distance", "hr", "watts"];

                // Build ordered list: candidate matches first, then remaining available streams
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
                    // try duration then distance for each available stream
                    let param_sets = [
                        vec![("stream", cand.as_str()), ("duration", "60")],
                        vec![("stream", cand.as_str()), ("distance", "1000")],
                        vec![("stream", cand.as_str()), ("duration", "300")],
                    ];
                    // Also try with `count=8` and without additional params in case upstream accepts defaults
                    let mut param_sets_extended: Vec<Vec<(&str, &str)>> = param_sets.to_vec();
                    param_sets_extended.push(vec![("stream", cand.as_str()), ("count", "8")]);
                    param_sets_extended.push(vec![("stream", cand.as_str())]);

                    for params in param_sets.iter().chain(param_sets_extended.iter()) {
                        let resp = self
                            .client
                            .get(&url)
                            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
                            .send()
                            .await?; // handled by building url earlier
                        if resp.status().is_success() {
                            return Ok(resp.json().await?);
                        }
                        // for debugging, capture body on 422 or 404 and continue trying
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
                            } else {
                                tracing::trace!(
                                    "best-efforts returned {} for stream={} params={:?} (no body)",
                                    status_code,
                                    cand,
                                    params
                                );
                            }
                            continue;
                        }
                        // Unexpected non-validation/non-404 error -> return immediately
                        return Err(IntervalsError::Config(format!(
                            "unexpected status: {}",
                            resp.status()
                        )));
                    }
                }

                // nothing worked
                Err(IntervalsError::Config("unexpected status: 422".into()))
            }
            Err(e) => {
                // couldn't fetch streams — if upstream returned 404 Not Found then
                // treat as "no streams" and return a config 422 to match the
                // validation semantics (caller expects 422 when no suitable params).
                if let IntervalsError::Config(msg) = &e
                    && (msg.contains("404") || msg.to_lowercase().contains("not found"))
                {
                    return Err(IntervalsError::Config("unexpected status: 422".into()));
                }
                // otherwise return the original error
                Err(e)
            }
        }
    }

    async fn get_activity_details(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<crate::ActivitySummary>, IntervalsError> {
        if query.trim().is_empty() {
            return Err(IntervalsError::Config("query must not be empty".into()));
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
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&qp)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        let acts: Vec<crate::ActivitySummary> = resp.json().await?;
        Ok(acts)
    }

    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, IntervalsError> {
        if query.trim().is_empty() {
            return Err(IntervalsError::Config("query must not be empty".into()));
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
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&qp)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/activities.csv",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.text().await?)
    }

    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(fields)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }

        let total = resp
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        // If output_path provided, stream to file and send progress
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
                        // check cancel flag and return error if cancelled
                        if *cancel_rx.borrow() {
                            return Err(IntervalsError::Config("download cancelled".into()));
                        }
                        continue;
                    }
                    c = stream.next() => c,
                };

                let Some(chunk) = chunk else {
                    break;
                };

                let bytes = chunk.map_err(IntervalsError::Http)?;
                file.write_all(&bytes)
                    .await
                    .map_err(|e| IntervalsError::Config(e.to_string()))?;
                downloaded = downloaded.saturating_add(bytes.len() as u64);
                // best-effort notify
                let _ = progress_tx.try_send(crate::DownloadProgress {
                    bytes_downloaded: downloaded,
                    total_bytes: total,
                });
                // check cancellation
                if *cancel_rx.borrow() {
                    return Err(IntervalsError::Config("download cancelled".into()));
                }
            }
            file.sync_all()
                .await
                .map_err(|e| IntervalsError::Config(e.to_string()))?;
            Ok(Some(path.to_string_lossy().to_string()))
        } else {
            // read into memory and send a single progress update
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
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
            let curve = format!("{}d", d);
            pairs.push(("curves", curve));
        }
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&qp)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_gap_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/gap-histogram",
            self.base_url, activity_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    // === Activities: Missing Methods ===

    async fn delete_activity(&self, activity_id: &str) -> Result<(), IntervalsError> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        let resp = self
            .client
            .delete(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(())
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
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&qp)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
        let resp = self
            .client
            .get(&url)
            .query(&pairs)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_power_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/power-histogram",
            self.base_url, activity_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_hr_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/hr-histogram",
            self.base_url, activity_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_pace_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/activity/{}/pace-histogram",
            self.base_url, activity_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    // === Fitness Summary ===

    async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> {
        // The API exposes fitness-related fields on the athlete object (GET /api/v1/athlete/{id}).
        // Historically there was a dedicated /fitness endpoint; use the athlete endpoint to avoid 404s.
        let url = format!("{}/api/v1/athlete/{}", self.base_url, self.athlete_id);
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        // Return raw JSON so callers can extract fitness/form/tsb fields.
        Ok(resp.json().await?)
    }

    // === Wellness ===

    async fn get_wellness(
        &self,
        days_back: Option<i32>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/wellness",
            self.base_url, self.athlete_id
        );
        let mut req = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()));
        if let Some(d) = days_back {
            let oldest = chrono::Utc::now()
                .checked_sub_signed(chrono::Duration::days(d as i64))
                .map(|dt| dt.format("%Y-%m-%d").to_string());
            if let Some(o) = oldest {
                let qp = [("oldest", o)];
                req = req.query(&qp);
            }
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/wellness/{}",
            self.base_url, self.athlete_id, date
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(data)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    // === Events/Calendar: Missing Methods ===

    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events",
            self.base_url, self.athlete_id
        );
        let newest = chrono::Utc::now()
            .checked_add_signed(chrono::Duration::days(days_ahead.unwrap_or(14) as i64))
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let oldest = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let pairs = [("oldest", oldest), ("newest", newest)];
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&pairs)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(fields)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/bulk-delete",
            self.base_url, self.athlete_id
        );
        let payload: Vec<serde_json::Value> = event_ids
            .into_iter()
            .map(|id| serde_json::json!({ "id": id }))
            .collect();

        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(&payload)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
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
        let payload = serde_json::json!({
            "eventIds": [event_id],
            "numCopies": num_copies.unwrap_or(1),
            "weeksBetween": weeks_between.unwrap_or(1),
        });

        let resp = self
            .client
            .post(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    // === Performance Curves ===

    async fn get_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/hr-curves",
            self.base_url, self.athlete_id
        );
        let mut pairs: Vec<(&str, String)> = vec![("type", sport.to_string())];
        if let Some(d) = days_back {
            let curve = format!("{}d", d);
            pairs.push(("curves", curve));
        }
        let resp = self
            .client
            .get(&url)
            .query(&pairs)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
        let mut pairs: Vec<(&str, String)> = vec![("type", sport.to_string())];
        if let Some(d) = days_back {
            let curve = format!("{}d", d);
            pairs.push(("curves", curve));
        }
        let resp = self
            .client
            .get(&url)
            .query(&pairs)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    // === Workout Library ===

    async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/folders",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn get_workouts_in_folder(
        &self,
        folder_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/workouts",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .get(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        let workouts: serde_json::Value = resp.json().await?;
        if folder_id.is_empty() {
            return Ok(workouts);
        }

        let target_id = folder_id.parse::<i64>().ok();
        if let Some(arr) = workouts.as_array() {
            let filtered: Vec<serde_json::Value> = arr
                .iter()
                .filter(|item| match (target_id, item.get("folder_id")) {
                    (Some(fid), Some(val)) => val.as_i64() == Some(fid),
                    (_, Some(val)) => val.as_str() == Some(folder_id),
                    _ => false,
                })
                .cloned()
                .collect();
            return Ok(serde_json::Value::Array(filtered));
        }

        Ok(workouts)
    }

    // === Gear Management ===

    async fn create_gear(
        &self,
        gear: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!("{}/api/v1/athlete/{}/gear", self.base_url, self.athlete_id);
        let resp = self
            .client
            .post(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(gear)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(fields)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn delete_gear(&self, gear_id: &str) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/gear/{}",
            self.base_url, self.athlete_id, gear_id
        );
        let resp = self
            .client
            .delete(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(())
    }

    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/gear/{}/reminder",
            self.base_url, self.athlete_id, gear_id
        );
        let resp = self
            .client
            .post(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(reminder)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
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
        let pairs = [
            ("reset", reset.to_string()),
            ("snoozeDays", snooze_days.to_string()),
        ];
        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&pairs)
            .json(fields)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    // === Sport Settings Management ===

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
        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .query(&pairs)
            .json(fields)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn apply_sport_settings(
        &self,
        sport_type: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings/{}/apply",
            self.base_url, self.athlete_id, sport_type
        );
        // Per API spec this is a PUT endpoint (apply settings). Use PUT to avoid 405 errors.
        let resp = self
            .client
            .put(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings",
            self.base_url, self.athlete_id
        );
        let resp = self
            .client
            .post(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .json(settings)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(resp.json().await?)
    }

    async fn delete_sport_settings(&self, sport_type: &str) -> Result<(), IntervalsError> {
        let url = format!(
            "{}/api/v1/athlete/{}/sport-settings/{}",
            self.base_url, self.athlete_id, sport_type
        );
        let resp = self
            .client
            .delete(&url)
            .basic_auth("API_KEY", Some(self.api_key.expose_secret()))
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(IntervalsError::Config(format!(
                "unexpected status: {}",
                resp.status()
            )));
        }
        Ok(())
    }
}
