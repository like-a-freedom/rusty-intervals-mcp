// Placeholder test - real unit tests live inside the crate's internal test module (src/lib.rs)
#[test]
fn noop() {
    assert_eq!(2 + 2, 4);
}

#[tokio::test]
async fn process_webhook_happy_and_duplicate_paths() {
    // minimal mock client
    struct C;
    #[async_trait::async_trait]
    impl intervals_icu_client::IntervalsClient for C {
        async fn get_athlete_profile(&self) -> Result<intervals_icu_client::AthleteProfile, IntervalsError> { unimplemented!() }
        async fn get_recent_activities(&self, _limit: Option<u32>, _days_back: Option<u32>) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> { Ok(vec![]) }
        async fn create_event(&self, _event: intervals_icu_client::Event) -> Result<intervals_icu_client::Event, IntervalsError> { unimplemented!() }
        async fn get_event(&self, _event_id: &str) -> Result<intervals_icu_client::Event, IntervalsError> { unimplemented!() }
        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> { Ok(()) }
        async fn get_events(&self, _days_back: Option<u32>, _limit: Option<u32>) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> { Ok(vec![]) }
        async fn bulk_create_events(&self, _events: Vec<intervals_icu_client::Event>) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> { Ok(vec![]) }
        async fn get_activity_streams(&self, _activity_id: &str, _streams: Option<Vec<String>>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_activity_intervals(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_best_efforts(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_activity_details(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn search_activities(&self, _query: &str, _limit: Option<u32>) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> { Ok(vec![]) }
        async fn search_activities_full(&self, _query: &str, _limit: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!([])) }
        async fn update_activity(&self, _activity_id: &str, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn download_activity_file(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn download_fit_file(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn download_gpx_file(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_power_curves(&self, _days_back: Option<u32>, _sport: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_gap_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn download_activity_file_with_progress(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>, _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>, _cancel_rx: tokio::sync::watch::Receiver<bool>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn delete_activity(&self, _activity_id: &str) -> Result<(), IntervalsError> { Ok(()) }
        async fn get_activities_around(&self, _activity_id: &str, _limit: Option<u32>, _route_id: Option<i64>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn search_intervals(&self, _min_secs: u32, _max_secs: u32, _min_intensity: u32, _max_intensity: u32, _interval_type: Option<String>, _min_reps: Option<u32>, _max_reps: Option<u32>, _limit: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_power_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_hr_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_pace_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_wellness(&self, _days_back: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_wellness_for_date(&self, _date: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_wellness(&self, _date: &str, _data: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_upcoming_workouts(&self, _days_ahead: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_event(&self, _event_id: &str, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn bulk_delete_events(&self, _event_ids: Vec<String>) -> Result<(), IntervalsError> { Ok(()) }
        async fn duplicate_event(&self, _event_id: &str, _num_copies: Option<u32>, _weeks_between: Option<u32>) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> { Ok(vec![]) }
        async fn get_hr_curves(&self, _days_back: Option<u32>, _sport: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_pace_curves(&self, _days_back: Option<u32>, _sport: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_workouts_in_folder(&self, _folder_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn create_gear(&self, _gear: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_gear(&self, _gear_id: &str, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> { Ok(()) }
        async fn create_gear_reminder(&self, _gear_id: &str, _reminder: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_gear_reminder(&self, _gear_id: &str, _reminder_id: &str, _reset: bool, _snooze_days: u32, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_sport_settings(&self, _sport_type: &str, _recalc_hr_zones: bool, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn apply_sport_settings(&self, _sport_type: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn create_sport_settings(&self, _settings: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> { Ok(()) }
    }

    let handler = IntervalsMcpHandler::new(Arc::new(C));

    // missing secret -> error
    let res = handler.process_webhook("deadbeef", serde_json::json!({ "id": "x" })).await;
    assert!(res.is_err());

    handler.set_webhook_secret_value("s3cr3t").await;

    // prepare valid signature
    let payload = serde_json::json!({ "id": "evt1", "x": 1 });
    let body = serde_json::to_vec(&payload).unwrap();
    let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"s3cr3t").unwrap();
    mac.update(&body);
    let sig = hex::encode(mac.finalize().into_bytes());

    let r = handler.process_webhook(&sig, payload.clone()).await.expect("should succeed");
    assert_eq!(r.value.get("ok").and_then(|v| v.as_bool()), Some(true));

    // duplicate should return duplicate:true
    let r2 = handler.process_webhook(&sig, payload.clone()).await.expect("should return duplicate");
    assert_eq!(r2.value.get("duplicate").and_then(|v| v.as_bool()), Some(true));
}

#[tokio::test]
async fn cancel_download_paths() {
    struct C;
    #[async_trait::async_trait]
    impl intervals_icu_client::IntervalsClient for C {
        async fn get_athlete_profile(&self) -> Result<intervals_icu_client::AthleteProfile, IntervalsError> { unimplemented!() }
        async fn get_recent_activities(&self, _limit: Option<u32>, _days_back: Option<u32>) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> { Ok(vec![]) }
        // other fns omitted for brevity in test
        async fn create_event(&self, _event: intervals_icu_client::Event) -> Result<intervals_icu_client::Event, IntervalsError> { unimplemented!() }
        async fn get_event(&self, _event_id: &str) -> Result<intervals_icu_client::Event, IntervalsError> { unimplemented!() }
        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> { Ok(()) }
        async fn get_events(&self, _days_back: Option<u32>, _limit: Option<u32>) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> { Ok(vec![]) }
        async fn bulk_create_events(&self, _events: Vec<intervals_icu_client::Event>) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> { Ok(vec![]) }
        async fn get_activity_streams(&self, _activity_id: &str, _streams: Option<Vec<String>>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_activity_intervals(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_best_efforts(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_activity_details(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn search_activities(&self, _query: &str, _limit: Option<u32>) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> { Ok(vec![]) }
        async fn search_activities_full(&self, _query: &str, _limit: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!([])) }
        async fn update_activity(&self, _activity_id: &str, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn download_activity_file(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn download_fit_file(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn download_gpx_file(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn get_gear_list(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_sport_settings(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_power_curves(&self, _days_back: Option<u32>, _sport: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_gap_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn download_activity_file_with_progress(&self, _activity_id: &str, _output_path: Option<std::path::PathBuf>, _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>, _cancel_rx: tokio::sync::watch::Receiver<bool>) -> Result<Option<String>, IntervalsError> { Ok(None) }
        async fn delete_activity(&self, _activity_id: &str) -> Result<(), IntervalsError> { Ok(()) }
        async fn get_activities_around(&self, _activity_id: &str, _limit: Option<u32>, _route_id: Option<i64>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn search_intervals(&self, _min_secs: u32, _max_secs: u32, _min_intensity: u32, _max_intensity: u32, _interval_type: Option<String>, _min_reps: Option<u32>, _max_reps: Option<u32>, _limit: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_power_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_hr_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_pace_histogram(&self, _activity_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_fitness_summary(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_wellness(&self, _days_back: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_wellness_for_date(&self, _date: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_wellness(&self, _date: &str, _data: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_upcoming_workouts(&self, _days_ahead: Option<u32>) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_event(&self, _event_id: &str, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn bulk_delete_events(&self, _event_ids: Vec<String>) -> Result<(), IntervalsError> { Ok(()) }
        async fn duplicate_event(&self, _event_id: &str, _num_copies: Option<u32>, _weeks_between: Option<u32>) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> { Ok(vec![]) }
        async fn get_hr_curves(&self, _days_back: Option<u32>, _sport: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_pace_curves(&self, _days_back: Option<u32>, _sport: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_workout_library(&self) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn get_workouts_in_folder(&self, _folder_id: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn create_gear(&self, _gear: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_gear(&self, _gear_id: &str, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> { Ok(()) }
        async fn create_gear_reminder(&self, _gear_id: &str, _reminder: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_gear_reminder(&self, _gear_id: &str, _reminder_id: &str, _reset: bool, _snooze_days: u32, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn update_sport_settings(&self, _sport_type: &str, _recalc_hr_zones: bool, _fields: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn apply_sport_settings(&self, _sport_type: &str) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn create_sport_settings(&self, _settings: &serde_json::Value) -> Result<serde_json::Value, IntervalsError> { Ok(serde_json::json!({})) }
        async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> { Ok(()) }
    }

    let handler = IntervalsMcpHandler::new(Arc::new(C));

    // cancel an unknown download id
    let params = rmcp::handler::server::wrapper::Parameters(Box::new(crate::DownloadIdParam { download_id: "missing".into() }));
    let err = handler.cancel_download(params).await.unwrap_err();
    assert!(err.contains("not found"));

    // insert a canceller and download entry and then cancel
    let (tx, _rx) = watch::channel(false);
    {
        let mut canc = handler.cancel_senders.lock().await;
        canc.insert("d1".to_string(), tx);
    }
    {
        let mut dl = handler.downloads.lock().await;
        dl.insert(
            "d1".to_string(),
            crate::DownloadStatus { id: "d1".into(), activity_id: "a1".into(), state: crate::DownloadState::InProgress, bytes_downloaded: 0, total_bytes: None, path: None },
        );
    }

    let params_ok = rmcp::handler::server::wrapper::Parameters(Box::new(crate::DownloadIdParam { download_id: "d1".into() }));
    let ok = handler.cancel_download(params_ok).await.expect("cancel succeeds");
    assert_eq!(ok.value.get("cancelled").and_then(|v| v.as_bool()), Some(true));
    let dl = handler.downloads.lock().await;
    let s = dl.get("d1").unwrap();
    match s.state {
        crate::DownloadState::Cancelled => {}
        _ => panic!("expected cancelled"),
    }
}

#[test]
fn event_id_cow_works() {
    use intervals_icu_mcp::EventId;
    let i = EventId::Int(42);
    assert_eq!(i.as_cow(), "42");
    let s = EventId::Str("x".into());
    assert_eq!(s.as_cow(), "x");
}
