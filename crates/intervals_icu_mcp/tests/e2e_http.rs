use reqwest::Client;
use std::sync::Arc;

use intervals_icu_client::AthleteProfile;
use intervals_icu_client::IntervalsClient;

struct LocalMockClient;

#[async_trait::async_trait]
impl IntervalsClient for LocalMockClient {
    async fn get_athlete_profile(
        &self,
    ) -> Result<AthleteProfile, intervals_icu_client::IntervalsError> {
        Ok(AthleteProfile {
            id: "me".into(),
            name: Some("Test".into()),
        })
    }
    async fn get_recent_activities(
        &self,
        _limit: Option<u32>,
        _days_back: Option<i32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![])
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
        Ok(())
    }
    async fn get_events(
        &self,
        _days_back: Option<i32>,
        _limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }
    async fn bulk_create_events(
        &self,
        _events: Vec<intervals_icu_client::Event>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
        Ok(vec![])
    }
    async fn get_activity_streams(
        &self,
        _activity_id: &str,
        _streams: Option<Vec<String>>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn get_activity_intervals(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn get_best_efforts(
        &self,
        _activity_id: &str,
        _options: Option<intervals_icu_client::BestEffortsOptions>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn get_activity_details(
        &self,
        _activity_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn search_activities(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
    {
        Ok(vec![])
    }
    async fn search_activities_full(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!([]))
    }
    async fn get_activities_csv(&self) -> Result<String, intervals_icu_client::IntervalsError> {
        Ok("id,start_date_local,name\n1,2025-10-18,Run".into())
    }
    async fn update_activity(
        &self,
        _activity_id: &str,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn download_activity_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
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
        Ok(serde_json::json!({}))
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
    async fn download_activity_file_with_progress(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
        _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
        _cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
        Ok(None)
    }

    // New methods
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
        Ok(serde_json::json!({}))
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
        _days_ahead: Option<u32>,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
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
        _event_ids: Vec<String>,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
    async fn duplicate_event(
        &self,
        _event_id: &str,
        _num_copies: Option<u32>,
        _weeks_between: Option<u32>,
    ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError> {
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
        Ok(serde_json::json!({}))
    }
    async fn get_workouts_in_folder(
        &self,
        _folder_id: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
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
        _gear: &serde_json::Value,
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
        _reminder: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn update_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder_id: &str,
        _reset: bool,
        _snooze_days: u32,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn update_sport_settings(
        &self,
        _sport_type: &str,
        _recalc_hr_zones: bool,
        _fields: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn apply_sport_settings(
        &self,
        _sport_type: &str,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn create_sport_settings(
        &self,
        _settings: &serde_json::Value,
    ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
        Ok(serde_json::json!({}))
    }
    async fn delete_sport_settings(
        &self,
        _sport_type: &str,
    ) -> Result<(), intervals_icu_client::IntervalsError> {
        Ok(())
    }
}

#[tokio::test]
async fn e2e_webhook_and_profile() {
    // build client and handler
    let client: Arc<dyn IntervalsClient> = Arc::new(LocalMockClient);
    let handler = intervals_icu_mcp::IntervalsMcpHandler::new(client.clone());
    handler.set_webhook_secret_value("s3cr3t").await;

    // build app (reuse server construction)
    let session = std::sync::Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let handler_for_factory = handler.clone();
    let mcp_service = rmcp::transport::streamable_http_server::tower::StreamableHttpService::new(
        move || -> Result<_, std::io::Error> { Ok(handler_for_factory.clone()) },
        session,
        rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig::default(),
    );

    let client_clone = client.clone();
    let handler_clone = handler.clone();
    let app = axum::Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route(
            "/athlete/profile",
            axum::routing::get(move || {
                let client = client_clone.clone();
                async move {
                    let p = client.get_athlete_profile().await.unwrap();
                    axum::Json(serde_json::json!({ "id": p.id, "name": p.name }))
                }
            }),
        )
        .route(
            "/webhook",
            axum::routing::post(move |axum::Json(payload): axum::Json<serde_json::Value>| {
                let handler = handler_clone.clone();
                async move {
                    // delegate to handler.process_webhook with a dummy signature
                    let _ = handler.process_webhook("deadbeef", payload).await;
                    axum::Json(serde_json::json!({ "ok": true }))
                }
            }),
        )
        .nest_service("/mcp", mcp_service);

    // bind to ephemeral port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = axum::serve(listener, app.into_make_service());
    let _sv = tokio::spawn(async move {
        server.await.ok();
    });

    let http = Client::new();

    // profile
    let res = http
        .get(format!("http://{}/athlete/profile", addr))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());

    // webhook - with bad signature should fail
    let res = http
        .post(format!("http://{}/webhook", addr))
        .json(&serde_json::json!({ "id": "x" }))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
}
