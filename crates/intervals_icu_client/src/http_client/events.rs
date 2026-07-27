//! Event endpoints — CRUD, bulk operations, and duplication.
//!
//! Covers the full lifecycle of `/athlete/{id}/events` plus the bulk-create
//! and bulk-delete variants, and the duplicate-events helper.

use crate::http_client::ReqwestIntervalsClient;
use crate::{IntervalsError, Result, ValidationError};
use chrono::{Duration, Utc};

impl ReqwestIntervalsClient {
    pub(crate) async fn fetch_create_event(&self, event: crate::Event) -> Result<crate::Event> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events"]);

        let mut ev = event;
        ev.start_date_local =
            Self::normalize_event_start(&ev.start_date_local).ok_or_else(|| {
                IntervalsError::Validation(ValidationError::InvalidFormat {
                    field: "start_date_local".to_string(),
                    value: format!("invalid start_date_local: {}", ev.start_date_local),
                })
            })?;

        let resp = self
            .execute_raw(self.request(reqwest::Method::POST, &url).json(&ev))
            .await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        Ok(resp.json().await?)
    }

    pub(crate) async fn fetch_event(&self, event_id: &str) -> Result<crate::Event> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", event_id]);
        let resp = self
            .execute_raw(self.request(reqwest::Method::GET, &url))
            .await?;
        if !resp.status().is_success() {
            return Err(self.error_from_response(resp).await);
        }
        let text = resp.text().await?;
        serde_json::from_str::<crate::Event>(&text).map_err(|e| {
            let body_snippet: String = text.chars().take(super::DECODE_BODY_SNIPPET_MAX).collect();
            IntervalsError::Decode {
                message: format!("decoding event: {e}"),
                snippet: body_snippet,
            }
        })
    }

    pub(crate) async fn fetch_delete_event(&self, event_id: &str) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", event_id]);
        self.execute_empty(self.request(reqwest::Method::DELETE, &url))
            .await
    }

    pub(crate) async fn fetch_events(
        &self,
        days_back: Option<i32>,
        limit: Option<u32>,
    ) -> Result<Vec<crate::Event>> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events"]);
        let mut builder = crate::utils::QueryBuilder::new();
        if let Some(d) = days_back {
            let today = Utc::now().date_naive();
            let oldest = today - Duration::days(i64::from(d));
            builder = builder.add("oldest", oldest).add("newest", today);
        }
        let pairs = builder.add_opt("limit", limit).build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();

        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    pub(crate) async fn fetch_bulk_create_events(
        &self,
        events: Vec<crate::Event>,
    ) -> Result<Vec<crate::Event>> {
        let url = format!(
            "{}/api/v1/athlete/{}/events/bulk",
            self.base_url, self.athlete_id
        );
        self.execute_json(self.request(reqwest::Method::POST, &url).json(&events))
            .await
    }

    pub(crate) async fn fetch_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events"]);
        let today = Utc::now().date_naive();
        let newest = today + Duration::days(i64::from(days_ahead.unwrap_or(7)));

        let pairs = crate::utils::QueryBuilder::new()
            .add("oldest", today)
            .add("newest", newest)
            .add_opt("limit", limit)
            .add_opt("category", category)
            .build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();

        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    pub(crate) async fn fetch_update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", event_id]);
        let normalized_fields = Self::normalize_event_update_fields(fields)?;
        self.execute_json(
            self.request(reqwest::Method::PUT, &url)
                .json(&normalized_fields),
        )
        .await
    }

    pub(crate) async fn fetch_bulk_delete_events(&self, event_ids: Vec<String>) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "events", "bulk-delete"]);
        let doomed: Vec<serde_json::Value> = event_ids
            .iter()
            .map(|id| {
                let parsed = id.parse::<i32>().map_err(|e| {
                    IntervalsError::Validation(ValidationError::InvalidFormat {
                        field: "event_id".to_string(),
                        value: format!("invalid event id '{id}': {e}"),
                    })
                })?;
                Ok(serde_json::json!({ "id": parsed }))
            })
            .collect::<Result<Vec<_>>>()?;
        self.execute_empty(self.request(reqwest::Method::PUT, &url).json(&doomed))
            .await
    }

    pub(crate) async fn fetch_duplicate_event(
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
        self.execute_json(self.request(reqwest::Method::POST, &url).json(&body))
            .await
    }
}
