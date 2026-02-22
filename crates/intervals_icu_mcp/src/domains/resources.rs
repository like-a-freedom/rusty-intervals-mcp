use rmcp::model::RawResource;

/// Return a `RawResource` descriptor for the athlete profile resource.
/// Kept small so `lib.rs` can delegate to it.
pub fn athlete_profile_resource() -> RawResource {
    let mut resource = RawResource::new("intervals-icu://athlete/profile", "Athlete Profile");
    resource.description = Some(
        "Complete athlete profile with current fitness metrics (fitness/fatigue/form/rampRate) and sport settings"
            .to_string(),
    );
    resource.mime_type = Some("application/json".to_string());
    resource
}

/// Build the textual JSON payload for the athlete profile resource by fetching
/// data from the provided `IntervalsClient`.
pub async fn build_athlete_profile_text(
    client: &dyn intervals_icu_client::IntervalsClient,
) -> Result<String, intervals_icu_client::IntervalsError> {
    let profile = client.get_athlete_profile().await?;
    let fitness = client.get_fitness_summary().await?;
    let sport_settings = client.get_sport_settings().await?;

    let text = format!(
        r#"{{
  "profile": {{
    "id": "{}",
    "name": {}
  }},
  "fitness": {},
  "sport_settings": {},
  "timestamp": "{}"
}}"#,
        profile.id,
        profile
            .name
            .as_ref()
            .map(|n| format!("\"{}\"", n))
            .unwrap_or_else(|| "null".to_string()),
        serde_json::to_string_pretty(&fitness).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string_pretty(&sport_settings).unwrap_or_else(|_| "[]".to_string()),
        chrono::Utc::now().to_rfc3339()
    );

    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[allow(dead_code)]
    struct MockClient;

    #[async_trait::async_trait]
    impl intervals_icu_client::IntervalsClient for MockClient {
        async fn get_athlete_profile(
            &self,
        ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError>
        {
            Ok(intervals_icu_client::AthleteProfile {
                id: "ath1".into(),
                name: Some("Tester".into()),
            })
        }
        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
        {
            unimplemented!()
        }
        async fn create_event(
            &self,
            _event: intervals_icu_client::Event,
        ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn get_event(
            &self,
            _event_id: &str,
        ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn delete_event(
            &self,
            _event_id: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn get_events(
            &self,
            _days_back: Option<i32>,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
        {
            unimplemented!()
        }
        async fn bulk_create_events(
            &self,
            _events: Vec<intervals_icu_client::Event>,
        ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
        {
            unimplemented!()
        }
        async fn get_activity_streams(
            &self,
            _activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn get_best_efforts(
            &self,
            _activity_id: &str,
            _options: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn get_activity_details(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn search_activities(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
        {
            unimplemented!()
        }
        async fn search_activities_full(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn get_activities_csv(&self) -> Result<String, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn update_activity(
            &self,
            _activity_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn download_activity_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }

        async fn download_activity_file_with_progress(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
            _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
            _cancel_rx: tokio::sync::watch::Receiver<bool>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }

        async fn download_fit_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            Ok(None)
        }

        async fn download_gpx_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            Ok(None)
        }

        async fn get_gear_list(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_sport_settings(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!([]))
        }

        async fn get_power_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_gap_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn delete_activity(
            &self,
            _activity_id: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }

        async fn get_activities_around(
            &self,
            _activity_id: &str,
            _limit: Option<u32>,
            _route_id: Option<i64>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn search_intervals(
            &self,
            _min_secs: u32,
            _max_secs: u32,
            _min_intensity: u32,
            _max_intensity: u32,
            _interval_type: Option<String>,
            _min_reps: Option<u32>,
            _max_reps: Option<u32>,
            _limit: Option<u32>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_power_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_hr_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_pace_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_fitness_summary(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_wellness(
            &self,
            _days_back: Option<i32>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!([]))
        }

        async fn get_wellness_for_date(
            &self,
            _date: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_wellness(
            &self,
            _date: &str,
            _data: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_upcoming_workouts(
            &self,
            _limit: Option<u32>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!([]))
        }

        async fn update_event(
            &self,
            _event_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn bulk_delete_events(
            &self,
            _ids: Vec<String>,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }

        async fn duplicate_event(
            &self,
            _event_id: &str,
            _num_copies: Option<u32>,
            _weeks_between: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
        {
            Ok(vec![])
        }

        async fn get_hr_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_pace_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_workout_library(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!([]))
        }

        async fn get_workouts_in_folder(
            &self,
            _folder_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!([]))
        }

        async fn create_folder(
            &self,
            _folder: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({"id": "f1", "name": "New Folder"}))
        }

        async fn update_folder(
            &self,
            _folder_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({"id": "f1", "name": "Updated Folder"}))
        }

        async fn delete_folder(
            &self,
            _folder_id: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }

        async fn create_gear(
            &self,
            _item: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_gear(
            &self,
            _gear_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn delete_gear(
            &self,
            _gear_id: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }

        async fn create_gear_reminder(
            &self,
            _gear_id: &str,
            _body: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder_id: &str,
            _enabled: bool,
            _interval_days: u32,
            _body: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_sport_settings(
            &self,
            _sport: &str,
            _apply: bool,
            _body: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn apply_sport_settings(
            &self,
            _sport: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn create_sport_settings(
            &self,
            _body: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn delete_sport_settings(
            &self,
            _sport: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn athlete_profile_resource_has_expected_metadata() {
        let r = athlete_profile_resource();
        assert_eq!(r.uri, "intervals-icu://athlete/profile");
        assert!(r.description.is_some());
        assert_eq!(r.mime_type.as_deref(), Some("application/json"));
    }

    #[tokio::test]
    async fn build_athlete_profile_text_includes_sections() {
        // Mock client returns simple fitness and sport settings
        struct FullMock;
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for FullMock {
            async fn get_athlete_profile(
                &self,
            ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError>
            {
                Ok(intervals_icu_client::AthleteProfile {
                    id: "A1".into(),
                    name: Some("X".into()),
                })
            }
            async fn get_recent_activities(
                &self,
                _limit: Option<u32>,
                _days_back: Option<i32>,
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                unimplemented!()
            }
            async fn create_event(
                &self,
                _event: intervals_icu_client::Event,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                unimplemented!()
            }
            async fn get_event(
                &self,
                _event_id: &str,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                unimplemented!()
            }
            async fn delete_event(
                &self,
                _event_id: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn get_events(
                &self,
                _days_back: Option<i32>,
                _limit: Option<u32>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                unimplemented!()
            }
            async fn bulk_create_events(
                &self,
                _events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                unimplemented!()
            }
            async fn get_activity_streams(
                &self,
                _activity_id: &str,
                _streams: Option<Vec<String>>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn get_activity_intervals(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn get_best_efforts(
                &self,
                _activity_id: &str,
                _options: Option<intervals_icu_client::BestEffortsOptions>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn get_activity_details(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn search_activities(
                &self,
                _query: &str,
                _limit: Option<u32>,
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                unimplemented!()
            }
            async fn search_activities_full(
                &self,
                _query: &str,
                _limit: Option<u32>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn update_activity(
                &self,
                _activity_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn download_activity_file(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                unimplemented!()
            }
            async fn download_fit_file(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(None)
            }
            async fn download_gpx_file(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(None)
            }
            async fn get_gear_list(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_power_curves(
                &self,
                _days_back: Option<i32>,
                _sport: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_gap_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn delete_activity(
                &self,
                _activity_id: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }
            async fn get_activities_around(
                &self,
                _activity_id: &str,
                _limit: Option<u32>,
                _route_id: Option<i64>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn search_intervals(
                &self,
                _min_secs: u32,
                _max_secs: u32,
                _min_intensity: u32,
                _max_intensity: u32,
                _interval_type: Option<String>,
                _min_reps: Option<u32>,
                _max_reps: Option<u32>,
                _limit: Option<u32>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_power_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_hr_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_pace_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_fitness_summary(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({"fitness": 45.5, "fatigue": 25.3, "form": 20.2, "rampRate": 1.5}))
            }
            async fn get_sport_settings(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!([{"sport":"Run","threshold":200}]))
            }
            async fn get_wellness(
                &self,
                _days_back: Option<i32>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!([]))
            }
            async fn get_wellness_for_date(
                &self,
                _date: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn update_wellness(
                &self,
                _date: &str,
                _data: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_upcoming_workouts(
                &self,
                _limit: Option<u32>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!([]))
            }
            async fn update_event(
                &self,
                _event_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn bulk_delete_events(
                &self,
                _ids: Vec<String>,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }
            async fn duplicate_event(
                &self,
                _event_id: &str,
                _num_copies: Option<u32>,
                _weeks_between: Option<u32>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(vec![])
            }
            async fn get_hr_curves(
                &self,
                _days_back: Option<i32>,
                _sport: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_pace_curves(
                &self,
                _days_back: Option<i32>,
                _sport: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn get_workout_library(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!([]))
            }
            async fn get_workouts_in_folder(
                &self,
                _folder_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!([]))
            }
            async fn create_folder(
                &self,
                _folder: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({"id": "f1", "name": "New Folder"}))
            }
            async fn update_folder(
                &self,
                _folder_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({"id": "f1", "name": "Updated Folder"}))
            }
            async fn delete_folder(
                &self,
                _folder_id: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }
            async fn create_gear(
                &self,
                _item: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn update_gear(
                &self,
                _gear_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn delete_gear(
                &self,
                _gear_id: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }
            async fn create_gear_reminder(
                &self,
                _gear_id: &str,
                _body: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn update_gear_reminder(
                &self,
                _gear_id: &str,
                _reminder_id: &str,
                _enabled: bool,
                _interval_days: u32,
                _body: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn update_sport_settings(
                &self,
                _sport: &str,
                _apply: bool,
                _body: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn apply_sport_settings(
                &self,
                _sport: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn create_sport_settings(
                &self,
                _body: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(json!({}))
            }
            async fn delete_sport_settings(
                &self,
                _sport: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }
        }

        let text = build_athlete_profile_text(&FullMock).await.expect("built");
        assert!(text.contains("\"profile\""));
        assert!(text.contains("\"fitness\""));
        assert!(text.contains("\"sport_settings\""));
    }
}
