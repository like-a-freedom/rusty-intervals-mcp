//! Middleware layer for cross-cutting concerns.
//!
//! This module implements the **Indirection** GRASP principle by providing
//! a layer between MCP handlers and the client for:
//! - Logging and tracing
//! - Metrics collection
//! - Error handling and transformation
//! - Request/response validation

use std::sync::Arc;
use std::time::Instant;

use intervals_icu_client::{IntervalsClient, IntervalsError};
use tracing::debug;

/// Middleware wrapper for the IntervalsClient that adds cross-cutting concerns.
///
/// This implements the **Indirection** principle by:
/// - Decoupling business logic from logging/metrics concerns
/// - Providing a single place for cross-cutting behavior
/// - Allowing easy addition of new middleware layers
#[derive(Clone)]
pub struct LoggingMiddleware<C: IntervalsClient> {
    inner: Arc<C>,
}

impl<C: IntervalsClient> LoggingMiddleware<C> {
    /// Create a new logging middleware wrapper.
    pub fn new(client: C) -> Self {
        Self {
            inner: Arc::new(client),
        }
    }

    /// Execute a fallible operation with logging.
    async fn with_logging<F, Fut, T>(&self, operation: F, name: &str) -> Result<T, IntervalsError>
    where
        F: FnOnce(Arc<C>) -> Fut,
        Fut: std::future::Future<Output = Result<T, IntervalsError>>,
    {
        let start = Instant::now();
        debug!("Starting operation: {}", name);

        let result = operation(self.inner.clone()).await;

        let duration = start.elapsed();
        match &result {
            Ok(_) => {
                debug!(
                    "Operation completed successfully: {} in {:?}",
                    name, duration
                );
            }
            Err(e) => {
                debug!(
                    "Operation failed: {} in {:?} - error: {}",
                    name, duration, e
                );
            }
        }

        result
    }
}

#[async_trait::async_trait]
impl<C: IntervalsClient + 'static> IntervalsClient for LoggingMiddleware<C> {
    async fn get_athlete_profile(
        &self,
    ) -> Result<intervals_icu_client::AthleteProfile, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_athlete_profile().await },
            "get_athlete_profile",
        )
        .await
    }

    async fn get_recent_activities(
        &self,
        limit: Option<u32>,
        days_back: Option<i32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_recent_activities(limit, days_back).await },
            "get_recent_activities",
        )
        .await
    }

    async fn create_event(
        &self,
        event: intervals_icu_client::Event,
    ) -> Result<intervals_icu_client::Event, IntervalsError> {
        self.with_logging(
            |client| async move { client.create_event(event).await },
            "create_event",
        )
        .await
    }

    async fn get_event(
        &self,
        event_id: &str,
    ) -> Result<intervals_icu_client::Event, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_event(event_id).await },
            "get_event",
        )
        .await
    }

    async fn delete_event(&self, event_id: &str) -> Result<(), IntervalsError> {
        self.with_logging(
            |client| async move { client.delete_event(event_id).await },
            "delete_event",
        )
        .await
    }

    async fn get_events(
        &self,
        days_back: Option<i32>,
        limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_events(days_back, limit).await },
            "get_events",
        )
        .await
    }

    async fn bulk_create_events(
        &self,
        events: Vec<intervals_icu_client::Event>,
    ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
        let count = events.len();
        self.with_logging(
            |client| async move { client.bulk_create_events(events).await },
            &format!("bulk_create_events({} events)", count),
        )
        .await
    }

    async fn get_activity_streams(
        &self,
        activity_id: &str,
        streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_activity_streams(activity_id, streams).await },
            "get_activity_streams",
        )
        .await
    }

    async fn get_activity_intervals(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_activity_intervals(activity_id).await },
            "get_activity_intervals",
        )
        .await
    }

    async fn get_best_efforts(
        &self,
        activity_id: &str,
        options: Option<intervals_icu_client::BestEffortsOptions>,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_best_efforts(activity_id, options).await },
            "get_best_efforts",
        )
        .await
    }

    async fn get_activity_details(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_activity_details(activity_id).await },
            "get_activity_details",
        )
        .await
    }

    async fn search_activities(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
        self.with_logging(
            |client| async move { client.search_activities(query, limit).await },
            "search_activities",
        )
        .await
    }

    async fn search_activities_full(
        &self,
        query: &str,
        limit: Option<u32>,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.search_activities_full(query, limit).await },
            "search_activities_full",
        )
        .await
    }

    async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_activities_csv().await },
            "get_activities_csv",
        )
        .await
    }

    async fn update_activity(
        &self,
        activity_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.update_activity(activity_id, fields).await },
            "update_activity",
        )
        .await
    }

    async fn download_activity_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        self.with_logging(
            |client| async move {
                client
                    .download_activity_file(activity_id, output_path)
                    .await
            },
            "download_activity_file",
        )
        .await
    }

    async fn download_activity_file_with_progress(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
        progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
        cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, IntervalsError> {
        self.with_logging(
            |client| async move {
                client
                    .download_activity_file_with_progress(
                        activity_id,
                        output_path,
                        progress_tx,
                        cancel_rx,
                    )
                    .await
            },
            "download_activity_file_with_progress",
        )
        .await
    }

    async fn download_fit_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        self.with_logging(
            |client| async move { client.download_fit_file(activity_id, output_path).await },
            "download_fit_file",
        )
        .await
    }

    async fn download_gpx_file(
        &self,
        activity_id: &str,
        output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        self.with_logging(
            |client| async move { client.download_gpx_file(activity_id, output_path).await },
            "download_gpx_file",
        )
        .await
    }

    async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_gear_list().await },
            "get_gear_list",
        )
        .await
    }

    async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_sport_settings().await },
            "get_sport_settings",
        )
        .await
    }

    async fn get_power_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_power_curves(days_back, sport).await },
            "get_power_curves",
        )
        .await
    }

    async fn get_gap_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_gap_histogram(activity_id).await },
            "get_gap_histogram",
        )
        .await
    }

    async fn delete_activity(&self, activity_id: &str) -> Result<(), IntervalsError> {
        self.with_logging(
            |client| async move { client.delete_activity(activity_id).await },
            "delete_activity",
        )
        .await
    }

    async fn get_activities_around(
        &self,
        activity_id: &str,
        limit: Option<u32>,
        route_id: Option<i64>,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move {
                client
                    .get_activities_around(activity_id, limit, route_id)
                    .await
            },
            "get_activities_around",
        )
        .await
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
        self.with_logging(
            |client| async move {
                client
                    .search_intervals(
                        min_secs,
                        max_secs,
                        min_intensity,
                        max_intensity,
                        interval_type,
                        min_reps,
                        max_reps,
                        limit,
                    )
                    .await
            },
            "search_intervals",
        )
        .await
    }

    async fn get_power_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_power_histogram(activity_id).await },
            "get_power_histogram",
        )
        .await
    }

    async fn get_hr_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_hr_histogram(activity_id).await },
            "get_hr_histogram",
        )
        .await
    }

    async fn get_pace_histogram(
        &self,
        activity_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_pace_histogram(activity_id).await },
            "get_pace_histogram",
        )
        .await
    }

    async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_fitness_summary().await },
            "get_fitness_summary",
        )
        .await
    }

    async fn get_wellness(
        &self,
        days_back: Option<i32>,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_wellness(days_back).await },
            "get_wellness",
        )
        .await
    }

    async fn get_wellness_for_date(&self, date: &str) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_wellness_for_date(date).await },
            "get_wellness_for_date",
        )
        .await
    }

    async fn update_wellness(
        &self,
        date: &str,
        data: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.update_wellness(date, data).await },
            "update_wellness",
        )
        .await
    }

    async fn get_upcoming_workouts(
        &self,
        days_ahead: Option<u32>,
        limit: Option<u32>,
        category: Option<String>,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move {
                client
                    .get_upcoming_workouts(days_ahead, limit, category)
                    .await
            },
            "get_upcoming_workouts",
        )
        .await
    }

    async fn update_event(
        &self,
        event_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.update_event(event_id, fields).await },
            "update_event",
        )
        .await
    }

    async fn bulk_delete_events(&self, event_ids: Vec<String>) -> Result<(), IntervalsError> {
        let count = event_ids.len();
        self.with_logging(
            |client| async move { client.bulk_delete_events(event_ids).await },
            &format!("bulk_delete_events({} events)", count),
        )
        .await
    }

    async fn duplicate_event(
        &self,
        event_id: &str,
        num_copies: Option<u32>,
        weeks_between: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
        self.with_logging(
            |client| async move {
                client
                    .duplicate_event(event_id, num_copies, weeks_between)
                    .await
            },
            "duplicate_event",
        )
        .await
    }

    async fn get_hr_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_hr_curves(days_back, sport).await },
            "get_hr_curves",
        )
        .await
    }

    async fn get_pace_curves(
        &self,
        days_back: Option<i32>,
        sport: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_pace_curves(days_back, sport).await },
            "get_pace_curves",
        )
        .await
    }

    async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_workout_library().await },
            "get_workout_library",
        )
        .await
    }

    async fn get_workouts_in_folder(
        &self,
        folder_id: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.get_workouts_in_folder(folder_id).await },
            "get_workouts_in_folder",
        )
        .await
    }

    async fn create_folder(
        &self,
        folder: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.create_folder(folder).await },
            "create_folder",
        )
        .await
    }

    async fn update_folder(
        &self,
        folder_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.update_folder(folder_id, fields).await },
            "update_folder",
        )
        .await
    }

    async fn delete_folder(&self, folder_id: &str) -> Result<(), IntervalsError> {
        self.with_logging(
            |client| async move { client.delete_folder(folder_id).await },
            "delete_folder",
        )
        .await
    }

    async fn create_gear(
        &self,
        gear: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.create_gear(gear).await },
            "create_gear",
        )
        .await
    }

    async fn update_gear(
        &self,
        gear_id: &str,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.update_gear(gear_id, fields).await },
            "update_gear",
        )
        .await
    }

    async fn delete_gear(&self, gear_id: &str) -> Result<(), IntervalsError> {
        self.with_logging(
            |client| async move { client.delete_gear(gear_id).await },
            "delete_gear",
        )
        .await
    }

    async fn create_gear_reminder(
        &self,
        gear_id: &str,
        reminder: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.create_gear_reminder(gear_id, reminder).await },
            "create_gear_reminder",
        )
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
        self.with_logging(
            |client| async move {
                client
                    .update_gear_reminder(gear_id, reminder_id, reset, snooze_days, fields)
                    .await
            },
            "update_gear_reminder",
        )
        .await
    }

    async fn update_sport_settings(
        &self,
        sport_type: &str,
        recalc_hr_zones: bool,
        fields: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move {
                client
                    .update_sport_settings(sport_type, recalc_hr_zones, fields)
                    .await
            },
            "update_sport_settings",
        )
        .await
    }

    async fn apply_sport_settings(
        &self,
        sport_type: &str,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.apply_sport_settings(sport_type).await },
            "apply_sport_settings",
        )
        .await
    }

    async fn create_sport_settings(
        &self,
        settings: &serde_json::Value,
    ) -> Result<serde_json::Value, IntervalsError> {
        self.with_logging(
            |client| async move { client.create_sport_settings(settings).await },
            "create_sport_settings",
        )
        .await
    }

    async fn delete_sport_settings(&self, sport_type: &str) -> Result<(), IntervalsError> {
        self.with_logging(
            |client| async move { client.delete_sport_settings(sport_type).await },
            "delete_sport_settings",
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockClient;

    #[async_trait]
    impl IntervalsClient for MockClient {
        async fn get_athlete_profile(
            &self,
        ) -> Result<intervals_icu_client::AthleteProfile, IntervalsError> {
            Ok(intervals_icu_client::AthleteProfile {
                id: "test".to_string(),
                name: Some("Test".to_string()),
            })
        }

        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![])
        }

        async fn create_event(
            &self,
            _event: intervals_icu_client::Event,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            unimplemented!()
        }

        async fn get_event(
            &self,
            _event_id: &str,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            unimplemented!()
        }

        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_events(
            &self,
            _days_back: Option<i32>,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn bulk_create_events(
            &self,
            _events: Vec<intervals_icu_client::Event>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn get_activity_streams(
            &self,
            _activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_best_efforts(
            &self,
            _activity_id: &str,
            _options: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_activity_details(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn search_activities(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![])
        }

        async fn search_activities_full(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!([]))
        }

        async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
            Ok(String::new())
        }

        async fn update_activity(
            &self,
            _activity_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn download_activity_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn download_activity_file_with_progress(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
            _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
            _cancel_rx: tokio::sync::watch::Receiver<bool>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn download_fit_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn download_gpx_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, IntervalsError> {
            Ok(None)
        }

        async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_power_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_gap_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn delete_activity(&self, _activity_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_activities_around(
            &self,
            _activity_id: &str,
            _limit: Option<u32>,
            _route_id: Option<i64>,
        ) -> Result<serde_json::Value, IntervalsError> {
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
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_power_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_hr_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_pace_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_wellness(
            &self,
            _days_back: Option<i32>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_wellness_for_date(
            &self,
            _date: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_wellness(
            &self,
            _date: &str,
            _data: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_upcoming_workouts(
            &self,
            _days_ahead: Option<u32>,
            _limit: Option<u32>,
            _category: Option<String>,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_event(
            &self,
            _event_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn bulk_delete_events(&self, _event_ids: Vec<String>) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn duplicate_event(
            &self,
            _event_id: &str,
            _num_copies: Option<u32>,
            _weeks_between: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn get_hr_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_pace_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_workouts_in_folder(
            &self,
            _folder_id: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn create_folder(
            &self,
            _folder: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_folder(
            &self,
            _folder_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn delete_folder(&self, _folder_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn create_gear(
            &self,
            _gear: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_gear(
            &self,
            _gear_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn create_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder_id: &str,
            _reset: bool,
            _snooze_days: u32,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn update_sport_settings(
            &self,
            _sport_type: &str,
            _recalc_hr_zones: bool,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn apply_sport_settings(
            &self,
            _sport_type: &str,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn create_sport_settings(
            &self,
            _settings: &serde_json::Value,
        ) -> Result<serde_json::Value, IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_logging_middleware_wraps_client() {
        let client = MockClient;
        let middleware = LoggingMiddleware::new(client);

        let result = middleware.get_athlete_profile().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().id, "test");
    }
}
