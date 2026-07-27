//! Athlete profile endpoint.
//!
//! Wraps `GET /athlete/{id}/profile`. The `ProfilePayload` and
//! `ProfileAthlete` intermediate types live here too — they exist purely to
//! peel the upstream envelope off the response body before shaping the
//! public [`AthleteProfile`] return value.

use crate::http_client::ReqwestIntervalsClient;
use crate::{AthleteProfile, IntervalsError, Result};

#[derive(serde::Deserialize)]
pub(super) struct ProfilePayload {
    pub(super) athlete: Option<ProfileAthlete>,
}

#[derive(serde::Deserialize)]
pub(super) struct ProfileAthlete {
    pub(super) id: Option<String>,
    pub(super) name: Option<String>,
}

impl ReqwestIntervalsClient {
    pub(crate) async fn fetch_athlete_profile(&self) -> Result<AthleteProfile> {
        let url = self.api_url(&["athlete", &self.athlete_id, "profile"]);
        let resp = self
            .execute_raw(self.request(reqwest::Method::GET, &url))
            .await?;
        let status = resp.status();
        if !status.is_success() {
            return Err(self.error_from_response(resp).await);
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
