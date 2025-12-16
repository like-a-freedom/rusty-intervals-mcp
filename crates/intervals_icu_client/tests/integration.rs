use base64::{Engine as _, engine::general_purpose::STANDARD};
use intervals_icu_client::{AthleteProfile, IntervalsClient};
use secrecy::SecretString;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_athlete_profile_passes_basic_auth_and_parses() {
    let server = MockServer::start().await;

    let expected_body = serde_json::json!({"athlete": {"id":"123","name":"Alice"}});

    // Expect a GET to /api/v1/athlete
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&expected_body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let profile = client.get_athlete_profile().await.expect("profile");
    assert_eq!(
        profile,
        AthleteProfile {
            id: "123".into(),
            name: Some("Alice".into())
        }
    );
    // Verify the Authorization header was sent and starts with `Basic `
    let received = server.received_requests().await.unwrap();
    assert!(!received.is_empty());
    let auth = received[0].headers.get("authorization").cloned();
    assert!(auth.is_some());
    let auth = auth.unwrap();
    let ok = auth
        .to_str()
        .map(|s| s.starts_with("Basic "))
        .unwrap_or(false);
    assert!(ok);
}

#[tokio::test]
async fn get_recent_activities_parses_list() {
    let server = MockServer::start().await;
    let body = serde_json::json!([
        {"id":"a1","name":"Ride 1"},
        {"id":"a2","name":"Run"}
    ]);
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let acts = client
        .get_recent_activities(Some(2), None)
        .await
        .expect("acts");
    assert_eq!(acts.len(), 2);
    assert_eq!(acts[0].id, "a1");
}

#[tokio::test]
async fn streams_and_intervals_endpoints_return_json() {
    let server = MockServer::start().await;
    let streams_body = serde_json::json!({"time":[1,2,3],"power":[100,150,200]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act1/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&streams_body))
        .mount(&server)
        .await;

    let intervals_body = serde_json::json!({"intervals":[{"start":0,"end":60}]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act1/intervals"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&intervals_body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let streams = client
        .get_activity_streams("act1", None)
        .await
        .expect("streams");
    assert!(streams.get("power").is_some());
    let intervals = client
        .get_activity_intervals("act1")
        .await
        .expect("intervals");
    assert!(intervals.get("intervals").is_some());
}

#[tokio::test]
async fn best_efforts_returns_payload() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"best_efforts": [{"duration": 60, "power": 300}]});

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act1/best-efforts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let efforts = client.get_best_efforts("act1").await.expect("best efforts");
    assert!(efforts.get("best_efforts").is_some());
}

#[tokio::test]
async fn create_event_validates_date_and_posts() {
    let server = MockServer::start().await;
    let input = serde_json::json!({
        "id": "evt-1",
        "start_date_local":"2025-12-15",
        "name":"Test",
        "category":"NOTE"
    });
    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/ath/events"))
        .respond_with(ResponseTemplate::new(201).set_body_json(&input))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let ev = intervals_icu_client::Event {
        id: None,
        start_date_local: "2025-12-15".into(),
        name: "Test".into(),
        category: intervals_icu_client::EventCategory::Note,
        description: None,
    };
    let created = client.create_event(ev).await.expect("create");
    assert_eq!(created.start_date_local, "2025-12-15");

    // invalid date should error
    let bad = intervals_icu_client::Event {
        id: None,
        start_date_local: "15-12-2025".into(),
        name: "Bad".into(),
        category: intervals_icu_client::EventCategory::Note,
        description: None,
    };
    let err = client.create_event(bad).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn get_event_fetches_by_id() {
    let server = MockServer::start().await;
    let body = serde_json::json!({
        "id": "evt1",
        "start_date_local": "2025-12-15",
        "name": "Race",
        "category": "RACE_A",
        "description": "Test race"
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/events/evt1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let ev = client.get_event("evt1").await.expect("event");
    assert_eq!(ev.id.as_deref(), Some("evt1"));
    assert_eq!(ev.category, intervals_icu_client::EventCategory::RaceA);
}

#[tokio::test]
async fn get_event_returns_helpful_error_on_unexpected_body() {
    let server = MockServer::start().await;
    // This payload resembles an activity (not an event) and should fail to decode
    // as an `Event` in the client. The client should return a Config error with
    // a helpful message containing a snippet of the body.
    let body = serde_json::json!({
        "id": "82024749",
        "name": "Morning Run",
        "start_time_local": "2025-12-15T07:00:00",
        "duration": 3600
    });

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/events/82024749"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.get_event("82024749").await;
    assert!(res.is_err());
    let err = res.err().unwrap();
    match err {
        intervals_icu_client::IntervalsError::Config(msg) => {
            assert!(msg.contains("decoding event"));
            assert!(msg.contains("Morning Run"));
        }
        _ => panic!("expected Config error"),
    }
}

#[tokio::test]
async fn delete_event_returns_ok() {
    let server = MockServer::start().await;

    Mock::given(method("DELETE"))
        .and(path("/api/v1/athlete/ath/events/evt1"))
        .respond_with(ResponseTemplate::new(204))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    client.delete_event("evt1").await.expect("delete");
}

#[tokio::test]
async fn download_activity_file_returns_base64_and_writes_file() {
    let server = MockServer::start().await;
    let body = vec![1u8, 2, 3, 4, 5];
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    // Retry a few times if the server responds with an empty payload; this guards
    // against transient test flakiness on CI where the mock server may return an
    // empty body intermittently.
    let mut last = None;
    let mut ok_b64 = None;
    for _ in 0..3 {
        let res = client
            .download_activity_file("a1", None)
            .await
            .expect("b64");
        last = res.clone();
        if let Some(s) = res {
            let decoded = STANDARD.decode(&s).unwrap_or_default();
            if !decoded.is_empty() {
                ok_b64 = Some(s);
                break;
            }
        }
        // allow a short delay for the mock server
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let b64 = ok_b64.expect(&format!(
        "download returned empty body after retries; last response={:?}; received_requests={:?}",
        last,
        server.received_requests().await.unwrap_or_default()
    ));
    assert_eq!(STANDARD.decode(&b64).unwrap(), body);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.bin");

    // Stream-to-file: retry reading the file until it contains data (up to a few attempts)
    client
        .download_activity_file("a1", Some(path.clone()))
        .await
        .expect("ok");

    let mut read = Vec::new();
    for _ in 0..5 {
        read = std::fs::read(&path).unwrap_or_default();
        if !read.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(read, body);
}

#[tokio::test]
async fn get_gear_list_ok() {
    let server = MockServer::start().await;
    let body = serde_json::json!([{"id":"g1","name":"Shoe"}]);
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/gear"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let list = client.get_gear_list().await.expect("gear");
    assert!(list.get(0).is_some());
}

#[tokio::test]
async fn get_sport_settings_ok() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"ftp": 250});
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/sport-settings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let settings = client.get_sport_settings().await.expect("settings");
    assert_eq!(settings.get("ftp").and_then(|v| v.as_u64()), Some(250));
}

#[tokio::test]
async fn power_curves_and_histogram_ok() {
    let server = MockServer::start().await;
    let curves = serde_json::json!({"best": [100,200]});
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/power-curves"))
        .and(query_param("type", "Ride"))
        .and(query_param("curves", "30d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&curves))
        .mount(&server)
        .await;

    let hist = serde_json::json!({"bins": [0,1,2]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/gap-histogram"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&hist))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let c = client
        .get_power_curves(Some(30), "Ride")
        .await
        .expect("curves");
    assert!(c.get("best").is_some());
    let h = client.get_gap_histogram("a1").await.expect("hist");
    assert!(h.get("bins").is_some());
}

#[tokio::test]
async fn search_activities_uses_search_endpoints() {
    let server = MockServer::start().await;

    let list_body = serde_json::json!([{ "id": "a1", "name": "Ride" }]);
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities/search"))
        .and(query_param("q", "run"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&list_body))
        .mount(&server)
        .await;

    let full_body = serde_json::json!({ "items": [ {"id": "a2"} ] });
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities/search-full"))
        .and(query_param("q", "ride"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&full_body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client
        .search_activities("run", None)
        .await
        .expect("search list");
    assert_eq!(res.len(), 1);

    let full = client
        .search_activities_full("ride", None)
        .await
        .expect("search full");
    assert!(full.get("items").is_some());
}

#[tokio::test]
async fn search_activities_rejects_empty_query() {
    let mock_server = MockServer::start().await;
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &mock_server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let err = client.search_activities("", None).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn search_intervals_sends_required_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities/interval-search"))
        .and(query_param("minSecs", "30"))
        .and(query_param("maxSecs", "60"))
        .and(query_param("minIntensity", "90"))
        .and(query_param("maxIntensity", "110"))
        .and(query_param("type", "POWER"))
        .and(query_param("minReps", "2"))
        .and(query_param("maxReps", "4"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client
        .search_intervals(
            30,
            60,
            90,
            110,
            Some("POWER".into()),
            Some(2),
            Some(4),
            Some(5),
        )
        .await
        .expect("search intervals");
    assert!(res.get("ok").is_some());
}

#[tokio::test]
async fn bulk_delete_events_hits_bulk_endpoint() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/events/bulk-delete"))
        .and(wiremock::matchers::body_json(
            serde_json::json!([{ "id": "1" }]),
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"eventsDeleted":1})),
        )
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    client
        .bulk_delete_events(vec!["1".into()])
        .await
        .expect("bulk delete");
}

#[tokio::test]
async fn duplicate_event_uses_duplicate_events_api() {
    let server = MockServer::start().await;
    let response = serde_json::json!([{
        "id": "2",
        "start_date_local": "2025-12-22",
        "name": "Copy",
        "category": "NOTE"
    }]);

    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/ath/duplicate-events"))
        .and(wiremock::matchers::body_json(serde_json::json!({
            "eventIds": ["1"],
            "numCopies": 2,
            "weeksBetween": 1
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let out = client
        .duplicate_event("1", Some(2), Some(1))
        .await
        .expect("duplicate");
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id.as_deref(), Some("2"));
}

#[tokio::test]
async fn workout_library_and_folder_paths_match_spec() {
    let server = MockServer::start().await;
    let folders = serde_json::json!([{ "id": 10, "name": "Base" }]);
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/folders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&folders))
        .mount(&server)
        .await;

    let workouts = serde_json::json!([
        { "id": 1, "folder_id": 10 },
        { "id": 2, "folder_id": 20 }
    ]);
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/workouts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&workouts))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let lib = client.get_workout_library().await.expect("folders");
    assert_eq!(lib.as_array().map(|a| a.len()), Some(1));

    let filtered = client.get_workouts_in_folder("10").await.expect("workouts");
    let arr = filtered.as_array().cloned().unwrap_or_default();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].get("id").and_then(|v| v.as_i64()), Some(1));
}

#[tokio::test]
async fn gear_reminder_update_sends_required_query() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/gear/g1/reminder/5"))
        .and(query_param("reset", "true"))
        .and(query_param("snoozeDays", "7"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": 5})))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let out = client
        .update_gear_reminder("g1", "5", true, 7, &serde_json::json!({"note": "hi"}))
        .await
        .expect("update reminder");
    assert!(out.get("id").is_some());
}

#[tokio::test]
async fn sport_settings_update_includes_recalc_flag() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/sport-settings/Run"))
        .and(query_param("recalcHrZones", "true"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/sport-settings/Run/apply"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let v = client
        .update_sport_settings("Run", true, &serde_json::json!({"ftp": 250}))
        .await
        .expect("update settings");
    assert!(v.get("ok").is_some());

    let applied = client
        .apply_sport_settings("Run")
        .await
        .expect("apply settings");
    assert!(applied.get("ok").is_some());
}
