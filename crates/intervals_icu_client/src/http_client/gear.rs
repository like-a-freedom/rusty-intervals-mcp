//! Gear endpoints — list, CRUD, and reminders.
//!
//! Per the upstream API, gear lives at `/athlete/{id}/gear`. Reminders are
//! a sub-resource at `/athlete/{id}/gear/{gearId}/reminder`.

use crate::Result;
use crate::http_client::ReqwestIntervalsClient;

impl ReqwestIntervalsClient {
    pub(crate) async fn fetch_gear_list(&self) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear"]);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    pub(crate) async fn fetch_create_gear(
        &self,
        gear: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear"]);
        self.execute_json(self.request(reqwest::Method::POST, &url).json(gear))
            .await
    }

    pub(crate) async fn fetch_update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear", gear_id]);
        self.execute_json(self.request(reqwest::Method::PUT, &url).json(fields))
            .await
    }

    pub(crate) async fn fetch_delete_gear(&self, gear_id: &str) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear", gear_id]);
        self.execute_empty(self.request(reqwest::Method::DELETE, &url))
            .await
    }

    pub(crate) async fn fetch_create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "gear", gear_id, "reminder"]);
        self.execute_json(self.request(reqwest::Method::POST, &url).json(reminder))
            .await
    }

    pub(crate) async fn fetch_update_gear_reminder(
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
        self.execute_json(
            self.request(reqwest::Method::PUT, &url)
                .query(&qp)
                .json(fields),
        )
        .await
    }
}
