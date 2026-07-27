//! Sport settings and workout library endpoints.
//!
//! Covers `/athlete/{id}/sport-settings` (CRUD + apply) and
//! `/athlete/{id}/folders` (workout library). The `resolve_sport_settings_id`
//! helper also lives here — it is only meaningful in the context of the
//! sport-settings workflow.

use crate::http_client::ReqwestIntervalsClient;
use crate::{IntervalsError, Result};

impl ReqwestIntervalsClient {
    async fn resolve_sport_settings_id(&self, sport_type_or_id: &str) -> Result<String> {
        if !sport_type_or_id.is_empty() && sport_type_or_id.chars().all(|c| c.is_ascii_digit()) {
            return Ok(sport_type_or_id.to_string());
        }

        let settings = self.fetch_sport_settings().await?;

        Self::resolve_sport_settings_id_from_settings(&settings, sport_type_or_id)
    }

    pub(crate) fn resolve_sport_settings_id_from_settings(
        settings: &crate::domains::workout::SportSettings,
        sport_type_or_id: &str,
    ) -> Result<String> {
        let normalized = Self::normalize_sport(sport_type_or_id);

        for entry in &settings.sports {
            let matches_type = entry
                .types
                .as_ref()
                .is_some_and(|types| types.iter().any(|t| t == &normalized));
            let matches_name = entry.name.as_deref() == Some(&normalized);

            if matches_type || matches_name {
                return entry.id.map(|id| id.to_string()).ok_or_else(|| {
                    IntervalsError::Validation(crate::ValidationError::InvalidFormat {
                        field: "sport_settings_id".to_string(),
                        value: "sport entry has no id".to_string(),
                    })
                });
            }
        }

        Err(IntervalsError::Validation(
            crate::ValidationError::InvalidFormat {
                field: "sport_type".to_string(),
                value: format!("unknown sport_type: {sport_type_or_id}"),
            },
        ))
    }

    pub(crate) async fn fetch_workout_library(
        &self,
    ) -> Result<Vec<crate::domains::workout::WorkoutItem>> {
        // API returns folders, plans and workouts together as a flat array
        let url = self.api_url(&["athlete", &self.athlete_id, "folders"]);
        self.execute_json(self.request(reqwest::Method::GET, &url))
            .await
    }

    pub(crate) async fn fetch_workouts_in_folder(
        &self,
        _folder_id: &str,
    ) -> Result<Vec<crate::domains::workout::WorkoutItem>> {
        // API doesn't have a direct endpoint - return all folders and let client filter
        // For now, return the full library response
        self.fetch_workout_library().await
    }

    pub(crate) async fn fetch_create_folder(
        &self,
        folder: &serde_json::Value,
    ) -> Result<crate::domains::workout::Folder> {
        let url = self.api_url(&["athlete", &self.athlete_id, "folders"]);
        self.execute_json(self.request(reqwest::Method::POST, &url).json(folder))
            .await
    }

    pub(crate) async fn fetch_update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "folders", folder_id]);
        self.execute_json(self.request(reqwest::Method::PUT, &url).json(fields))
            .await
    }

    pub(crate) async fn fetch_delete_folder(&self, folder_id: &str) -> Result<()> {
        let url = self.api_url(&["athlete", &self.athlete_id, "folders", folder_id]);
        self.execute_empty(self.request(reqwest::Method::DELETE, &url))
            .await
    }

    pub(crate) async fn fetch_sport_settings(
        &self,
    ) -> Result<crate::domains::workout::SportSettings> {
        let url = self.api_url(&["athlete", &self.athlete_id, "sport-settings"]);
        let value: serde_json::Value = self
            .execute_json(self.request(reqwest::Method::GET, &url))
            .await?;
        let raw = serde_json::to_string(&value).unwrap_or_default();
        let body_snippet: String = raw.chars().take(super::DECODE_BODY_SNIPPET_MAX).collect();
        crate::domains::workout::SportSettings::from_value(&value).ok_or_else(|| {
            IntervalsError::Decode {
                message: "failed to parse sport settings response".to_string(),
                snippet: body_snippet,
            }
        })
    }

    pub(crate) async fn fetch_update_sport_settings(
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
        self.execute_json(self.request(reqwest::Method::PUT, &url).json(&body))
            .await
    }

    pub(crate) async fn fetch_apply_sport_settings(
        &self,
        sport_type: &str,
    ) -> Result<serde_json::Value> {
        let sport_settings_id = self.resolve_sport_settings_id(sport_type).await?;
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "sport-settings",
            &sport_settings_id,
            "apply",
        ]);
        self.execute_json(self.request(reqwest::Method::PUT, &url))
            .await
    }

    pub(crate) async fn fetch_create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = self.api_url(&["athlete", &self.athlete_id, "sport-settings"]);
        self.execute_json(self.request(reqwest::Method::POST, &url).json(settings))
            .await
    }

    pub(crate) async fn fetch_delete_sport_settings(&self, sport_type: &str) -> Result<()> {
        let sport_settings_id = self.resolve_sport_settings_id(sport_type).await?;
        let url = self.api_url(&[
            "athlete",
            &self.athlete_id,
            "sport-settings",
            &sport_settings_id,
        ]);
        self.execute_empty(self.request(reqwest::Method::DELETE, &url))
            .await
    }
}
