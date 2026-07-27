//! Activity endpoints.
//!
//! Methods that operate on a single activity, the activity stream, the activity
//! store and search surface. Also owns two free helpers used by
//! [`get_best_efforts`](super::ReqwestIntervalsClient::get_best_efforts):
//! [`extract_available_streams`] and [`annotate_best_efforts_payload`].
//!
//! Submodule methods are named with a `fetch_` prefix to avoid a name
//! collision with the trait-method dispatchers in the
//! `impl IntervalsClient for ReqwestIntervalsClient` block in [`super`].

use crate::http_client::ReqwestIntervalsClient;
use crate::{ActivityMessage, BestEffortsOptions, IntervalsError, Result, ValidationError};
use chrono::{Duration, Utc};

impl ReqwestIntervalsClient {
    /// Fetch the recent activities list, optionally bounded by `days_back` and `limit`.
    pub(crate) async fn fetch_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<crate::ActivitySummary>> {
        let url = self.api_url(&["athlete", &self.athlete_id, "activities"]);
        let today = Utc::now().date_naive();
        let oldest = today - Duration::days(i64::from(days_back.unwrap_or(7)));

        let pairs = crate::utils::QueryBuilder::new()
            .add("oldest", oldest)
            .add("newest", today)
            .add_opt("limit", limit)
            .build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();

        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    /// Fetch the full JSON payload for a single activity by id.
    pub(crate) async fn fetch_activity_details(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Fetch the message thread attached to a single activity.
    pub(crate) async fn fetch_activity_messages(
        &self,
        activity_id: &str,
    ) -> Result<Vec<ActivityMessage>> {
        let url = format!("{}/api/v1/activity/{}/messages", self.base_url, activity_id);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Fetch a sampled activity stream. When `streams` is omitted, the server returns the default set.
    pub(crate) async fn fetch_activity_streams(
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
        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    /// Fetch the structured intervals of a single activity.
    pub(crate) async fn fetch_activity_intervals(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/intervals",
            self.base_url, activity_id
        );
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Fetch the best-efforts payload for an activity.
    ///
    /// The server accepts different combinations of `stream`, `duration`, and `distance`.
    /// When `options` is `Some`, validate that a `stream` plus a `duration` or `distance` is
    /// present, send the request, and annotate the response with the resolved stream.
    /// When `options` is `None`, try a fixed ladder of `(stream, duration|distance)` pairs and
    /// fall back to streams detected via [`fetch_activity_streams`].
    pub(crate) async fn fetch_best_efforts(
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
            let value = self
                .execute_json(self.request(reqwest::Method::GET, &url).query(&q))
                .await?;
            return Ok(annotate_best_efforts_payload(value, stream));
        }

        // Try default parameter combinations when no options provided
        let attempts = [
            vec![("stream", "power"), ("duration", "60")],
            vec![("stream", "power"), ("distance", "1000")],
            vec![("stream", "power"), ("duration", "300")],
        ];

        for params in &attempts {
            let qp: Vec<(&str, &str)> = params.iter().map(|(k, v)| (*k, *v)).collect();
            let resp = self
                .execute_raw(self.request(reqwest::Method::GET, &url).query(&qp))
                .await?;

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
        let streams_payload = self.fetch_activity_streams(activity_id, None).await;
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
                for s in &available_streams {
                    if !ordered_streams.contains(s) {
                        ordered_streams.push(s.clone());
                    }
                }

                for cand in &ordered_streams {
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
                        let resp = self
                            .execute_raw(self.request(reqwest::Method::GET, &url).query(&qp))
                            .await?;
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

    /// Search activities by free-text query, optionally bounded by `limit`.
    pub(crate) async fn fetch_search_activities(
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
        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    /// Search activities by free-text query and return the full JSON payload.
    pub(crate) async fn fetch_search_activities_full(
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
        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    /// Fetch the activities list as a CSV string.
    pub(crate) async fn fetch_activities_csv(&self) -> Result<String> {
        let url = format!(
            "{}/api/v1/athlete/{}/activities.csv",
            self.base_url, self.athlete_id
        );
        self.execute_text(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Update an activity in place. The server merges the supplied `fields` JSON.
    pub(crate) async fn fetch_update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_json(self.request(reqwest::Method::PUT, &url).json(fields))
            .await
    }

    /// Delete an activity by id.
    pub(crate) async fn fetch_delete_activity(&self, activity_id: &str) -> Result<()> {
        let url = format!("{}/api/v1/activity/{}", self.base_url, activity_id);
        self.execute_empty(self.request(reqwest::Method::DELETE, &url))
            .await
    }

    /// Fetch activities that occurred close to a given activity.
    pub(crate) async fn fetch_activities_around(
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
        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    /// Fetch the gap histogram for a single activity.
    pub(crate) async fn fetch_gap_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/gap-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Search intervals matching duration/intensity filters across the activity history.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn fetch_search_intervals(
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
        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    /// Fetch the power histogram for a single activity.
    pub(crate) async fn fetch_power_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/power-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Fetch the heart-rate histogram for a single activity.
    pub(crate) async fn fetch_hr_histogram(&self, activity_id: &str) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/hr-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    /// Fetch the pace histogram for a single activity.
    pub(crate) async fn fetch_pace_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/api/v1/activity/{}/pace-histogram",
            self.base_url, activity_id
        );
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }
}

/// Extract available stream names from a JSON response.
///
/// `pub(super)` so unit tests in the parent module can reach it via
/// `super::activities::extract_available_streams` without exporting it
/// beyond the crate.
pub(super) fn extract_available_streams(json: &serde_json::Value) -> Vec<String> {
    let mut available_streams = Vec::new();

    if let Some(sv) = json.get("streams") {
        if let Some(obj) = sv.as_object() {
            available_streams.extend(obj.keys().cloned());
        } else if let Some(arr) = sv.as_array() {
            for item in arr {
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
        for item in arr {
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
        for (k, v) in obj {
            if v.is_array() {
                available_streams.push(k.clone());
            }
        }
    }

    available_streams
}

/// Annotate a best-efforts payload with the `stream` it was fetched for.
///
/// `pub(super)` for the same reason as [`extract_available_streams`].
pub(super) fn annotate_best_efforts_payload(
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
