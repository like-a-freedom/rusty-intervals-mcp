//! Curves endpoints — power / HR / pace.
//!
//! Three thin wrappers around the private `get_curves` helper that lives in
//! [`http_client::mod`]. Each wrapper fixes the curve type (`power`, `hr`,
//! `pace`) and forwards the `days_back` / `sport` selection.

use crate::Result;
use crate::http_client::ReqwestIntervalsClient;

impl ReqwestIntervalsClient {
    pub(crate) async fn fetch_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.get_curves(days_back, sport, "power").await
    }

    pub(crate) async fn fetch_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.get_curves(days_back, sport, "hr").await
    }

    pub(crate) async fn fetch_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value> {
        self.get_curves(days_back, sport, "pace").await
    }
}
