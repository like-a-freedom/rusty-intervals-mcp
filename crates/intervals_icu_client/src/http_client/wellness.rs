//! Wellness endpoints.
//!
//! Daily wellness entries (`/athlete/{id}/wellness`) and the bulk update
//! variant (`/athlete/{id}/wellness-bulk`). `update_wellness_bulk` is kept on
//! the trait surface per ADR-0005 (real implementation, contract test, and
//! integration coverage all present).

use crate::Result;
use crate::http_client::ReqwestIntervalsClient;
use chrono::{Duration, Utc};

impl ReqwestIntervalsClient {
    pub(crate) async fn fetch_wellness(&self, days_back: Option<i32>) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness"]);
        let mut builder = crate::utils::QueryBuilder::new();
        if let Some(d) = days_back {
            let today = Utc::now().date_naive();
            let oldest = today - Duration::days(i64::from(d));
            builder = builder.add("oldest", oldest).add("newest", today);
        }
        let pairs = builder.build_owned();
        let qp: Vec<(&str, &str)> = pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();

        self.execute_json(self.request(reqwest::Method::GET, &url).query(&qp))
            .await
    }

    pub(crate) async fn fetch_wellness_for_date(&self, date: &str) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness", date]);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    pub(crate) async fn fetch_update_wellness(
        &self,
        date: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness", date]);
        self.execute_json(self.request(reqwest::Method::PUT, &url).json(payload))
            .await
    }

    pub(crate) async fn fetch_update_wellness_bulk(
        &self,
        entries: &[serde_json::Value],
    ) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "wellness-bulk"]);
        self.execute_empty(self.request(reqwest::Method::PUT, &url).json(entries))
            .await
    }
}
