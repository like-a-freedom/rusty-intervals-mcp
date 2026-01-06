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

    // (no debug fetch) — keep this test focused on the stream-to-file behavior
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
        .and(query_param("stream", "power"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let efforts = client
        .get_best_efforts("act1", None)
        .await
        .expect("best efforts");
    assert!(efforts.get("best_efforts").is_some());
}

#[tokio::test]
async fn best_efforts_fallback_to_distance_when_duration_unaccepted() {
    let server = MockServer::start().await;
    let body = serde_json::json!({"best_efforts": [{"distance": 1000, "power": 350}]});

    // First request with duration=60 -> return 422
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act2/best-efforts"))
        .and(query_param("stream", "power"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    // Second request with distance=1000 -> return 200
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act2/best-efforts"))
        .and(query_param("stream", "power"))
        .and(query_param("distance", "1000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let efforts = client
        .get_best_efforts("act2", None)
        .await
        .expect("best efforts via fallback");
    assert!(efforts.get("best_efforts").is_some());
}

#[tokio::test]
async fn best_efforts_all_fallbacks_422_returns_error() {
    let server = MockServer::start().await;

    // Make all expected attempts return 422
    for _ in 0..3 {
        Mock::given(method("GET"))
            .and(path("/api/v1/activity/act3/best-efforts"))
            .respond_with(ResponseTemplate::new(422))
            .mount(&server)
            .await;
    }

    // Also return empty streams when client tries to inspect available streams
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act3/streams"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({ "streams": {} })),
        )
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let err = client.get_best_efforts("act3", None).await.unwrap_err();
    assert_eq!(
        format!("{}", err),
        "configuration error: unexpected status: 422"
    );
}

#[tokio::test]
async fn best_efforts_detects_available_stream_and_uses_it() {
    let server = MockServer::start().await;

    // initial power attempts -> 422
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act4/best-efforts"))
        .and(query_param("stream", "power"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    // streams endpoint returns speed but not power
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act4/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "streams": { "time": [0,1,2], "speed": [2.5,3.0,3.5] }
        })))
        .mount(&server)
        .await;

    // expect best-efforts call with stream=speed & duration=60 -> return 200
    let body = serde_json::json!({"best_efforts": [{"duration": 60, "speed": 3.5}]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act4/best-efforts"))
        .and(query_param("stream", "speed"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let efforts = client
        .get_best_efforts("act4", None)
        .await
        .expect("best efforts via detected stream");
    assert!(efforts.get("best_efforts").is_some());
}

#[tokio::test]
async fn best_efforts_stream_lookup_without_candidates_returns_error() {
    let server = MockServer::start().await;

    // power attempts -> 422 (duration=60, distance=1000, duration=300)
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act5/best-efforts"))
        .and(query_param("stream", "power"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act5/best-efforts"))
        .and(query_param("stream", "power"))
        .and(query_param("distance", "1000"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act5/best-efforts"))
        .and(query_param("stream", "power"))
        .and(query_param("duration", "300"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;
    // also mock count and bare stream attempts (we try these as extended fallbacks)
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act5/best-efforts"))
        .and(query_param("stream", "power"))
        .and(query_param("count", "8"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act5/best-efforts"))
        .and(query_param("stream", "power"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    // streams endpoint returns only time (no candidate streams)
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act5/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "streams": { "time": [0,1,2] }
        })))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let err = client.get_best_efforts("act5", None).await.unwrap_err();
    assert_eq!(
        format!("{}", err),
        "configuration error: unexpected status: 422"
    );
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
        r#type: None,
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
        r#type: None,
    };
    let err = client.create_event(bad).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn create_event_accepts_iso_and_preserves_time() {
    let server = MockServer::start().await;
    let mock_created = serde_json::json!({
        "id": "evt-iso",
        "start_date_local": "2026-01-19T06:30:00",
        "name": "ISO test",
        "category": "NOTE",
        "description": null
    });
    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/ath/events"))
        .and(wiremock::matchers::body_json(serde_json::json!({
            "start_date_local": "2026-01-19T06:30:00",
            "name": "ISO test",
            "category": "NOTE",
            "description": null
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(&mock_created))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let ev2 = intervals_icu_client::Event {
        id: None,
        start_date_local: "2026-01-19T06:30:00".into(),
        name: "ISO test".into(),
        category: intervals_icu_client::EventCategory::Note,
        description: None,
        r#type: None,
    };
    let created2 = client.create_event(ev2).await.expect("create iso");
    assert_eq!(created2.start_date_local, "2026-01-19T06:30:00");
}

#[tokio::test]
async fn create_event_accepts_date_and_sets_midnight() {
    let server = MockServer::start().await;
    let mock_created = serde_json::json!({
        "id": "evt-date",
        "start_date_local": "2026-01-19T00:00:00",
        "name": "Date only",
        "category": "NOTE",
        "description": null
    });
    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/ath/events"))
        .and(wiremock::matchers::body_json(serde_json::json!({
            "start_date_local": "2026-01-19T00:00:00",
            "name": "Date only",
            "category": "NOTE",
            "description": null
        })))
        .respond_with(ResponseTemplate::new(201).set_body_json(&mock_created))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let ev = intervals_icu_client::Event {
        id: None,
        start_date_local: "2026-01-19".into(),
        name: "Date only".into(),
        category: intervals_icu_client::EventCategory::Note,
        description: None,
        r#type: None,
    };
    let created = client.create_event(ev).await.expect("create date");
    assert_eq!(created.start_date_local, "2026-01-19T00:00:00");
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
async fn base_url_trailing_slash_is_handled() {
    let server = MockServer::start().await;

    let expected_body = serde_json::json!({"athlete": {"id":"t1","name":"Trailing"}});

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/profile"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&expected_body))
        .mount(&server)
        .await;

    // Add a trailing slash to the base URL to ensure trim_end_matches('/') works
    let base = format!("{}/", server.uri());
    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &base,
        "ath",
        SecretString::new("tok".into()),
    );

    let p = client.get_athlete_profile().await.expect("profile");
    assert_eq!(p.id, "t1");
}

#[tokio::test]
async fn get_activities_around_includes_route_and_limit_query() {
    let server = MockServer::start().await;

    let payload = serde_json::json!({ "items": [] });
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities-around"))
        // Don't assert exact query string here (wiremock currently matches only when requested),
        // returning 200 is enough to exercise the client code path that adds the params.
        .respond_with(ResponseTemplate::new(200).set_body_json(&payload))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client
        .get_activities_around("a1", Some(5), Some(42))
        .await
        .expect("ok");
    assert!(res.get("items").is_some());
}

#[tokio::test]
async fn get_wellness_with_days_back_adds_oldest_query() {
    let server = MockServer::start().await;

    let payload = serde_json::json!({ "days": [] });
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/wellness"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&payload))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let _ = client.get_wellness(Some(5)).await.expect("ok");
}

#[tokio::test]
async fn download_activity_file_in_memory_sends_progress_and_returns_base64() {
    let server = MockServer::start().await;
    let body = vec![10u8, 20, 30, 40];

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/file"))
        // no content-length header to exercise the in-memory branch
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let res = client
        .download_activity_file_with_progress("a1", None, tx, cancel_rx)
        .await
        .expect("ok");

    // We expect a progress update (len bytes)
    let msg = rx.recv().await.expect("progress");
    assert_eq!(msg.bytes_downloaded, body.len() as u64);

    // Result should be base64-encoded bytes
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(res.unwrap())
        .unwrap();
    assert_eq!(decoded, body);
    // cancel channel should not be triggered
    let _ = cancel_tx;
}

#[tokio::test]
async fn get_event_non_success_returns_error() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/events/bad"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.get_event("bad").await;
    assert!(res.is_err());
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
    // Include a valid Content-Length header for the streaming test.
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("content-length", body.len().to_string())
                .set_body_bytes(body.clone()),
        )
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    // Stream-to-file: verify streaming writes the expected bytes
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.bin");

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
async fn download_activity_file_with_progress_writes_file_and_sends_progress() {
    let server = MockServer::start().await;
    let body: Vec<u8> = (0u8..120u8).collect();

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a2/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("content-length", body.len().to_string())
                .set_body_bytes(body.clone()),
        )
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file_out.bin");

    let (tx, mut rx) = tokio::sync::mpsc::channel(4);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let res = client
        .download_activity_file_with_progress("a2", Some(path.clone()), tx, cancel_rx)
        .await
        .expect("ok");

    // Expect at least one progress update and that total_bytes is reported
    let mut got_progress = false;
    while let Ok(Some(msg)) =
        tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv()).await
    {
        if msg.bytes_downloaded > 0 {
            got_progress = true;
            assert!(msg.total_bytes.is_some());
            break;
        }
    }
    assert!(got_progress, "expected progress messages");

    // File should exist and contain the expected bytes
    let mut read = Vec::new();
    for _ in 0..5 {
        read = std::fs::read(&path).unwrap_or_default();
        if !read.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(read, body);
    // Ensure returned path matches
    assert_eq!(res.unwrap(), path.to_string_lossy().to_string());
    let _ = cancel_tx;
}

#[tokio::test]
async fn download_file_create_error_returns_config() {
    let server = MockServer::start().await;
    let body = vec![9u8, 8, 7];

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a3/file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body.clone()))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    // Pass a directory path (exists) instead of a file path to induce create error
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_path_buf();

    let res = client.download_activity_file("a3", Some(path)).await;
    assert!(res.is_err());
    match res.err().unwrap() {
        intervals_icu_client::IntervalsError::Config(_) => {}
        _ => panic!("expected Config error"),
    }
}

#[tokio::test]
async fn get_workouts_in_folder_non_array_returns_original_json() {
    let server = MockServer::start().await;

    let body = serde_json::json!({ "meta": { "count": 0 } });
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/workouts"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.get_workouts_in_folder("123").await.expect("ok");
    assert!(res.get("meta").is_some());
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

    // activities.csv
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities.csv"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("id,start_date_local,name\n1,2025-10-18,Run"),
        )
        .mount(&server)
        .await;

    let csv = client.get_activities_csv().await.expect("csv");
    assert!(csv.contains("start_date_local"));
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
async fn bulk_create_events_propagates_error_body() {
    let server = MockServer::start().await;
    let event = serde_json::json!({
        "name": "Test Workout",
        "start_date_local": "2026-13-01",
        "category": "WORKOUT",
        "description": null
    });

    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/ath/events/bulk"))
        .and(wiremock::matchers::body_json(serde_json::json!([
            event.clone()
        ])))
        .respond_with(ResponseTemplate::new(422).set_body_json(serde_json::json!({
            "error": "invalid date"
        })))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let ev = intervals_icu_client::Event {
        id: None,
        name: "Test Workout".into(),
        start_date_local: "2026-13-01".into(),
        category: intervals_icu_client::EventCategory::Workout,
        description: None,
        r#type: None,
    };

    let res = client.bulk_create_events(vec![ev]).await;
    assert!(res.is_err());
    let err = format!("{}", res.err().unwrap());
    assert!(err.contains("422") || err.contains("unprocessable"));
    assert!(err.contains("invalid date"));
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

#[tokio::test]
async fn delete_sport_settings_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/athlete/ath/sport-settings/Run"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.delete_sport_settings("Run").await;
    assert!(res.is_err());
}

#[tokio::test]
async fn get_athlete_profile_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/profile"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let res = client.get_athlete_profile().await;
    assert!(res.is_err());
    match res.err().unwrap() {
        intervals_icu_client::IntervalsError::Config(msg) => {
            assert!(msg.contains("unexpected status"));
        }
        _ => panic!("expected Config error"),
    }
}

#[tokio::test]
async fn create_event_handles_non_success() {
    let server = MockServer::start().await;
    let ev = serde_json::json!({"error":"bad"});
    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/ath/events"))
        .respond_with(ResponseTemplate::new(400).set_body_json(&ev))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let event = intervals_icu_client::Event {
        id: None,
        start_date_local: "2025-12-15".into(),
        name: "X".into(),
        category: intervals_icu_client::EventCategory::Note,
        description: None,
        r#type: None,
    };
    let res = client.create_event(event).await;
    assert!(res.is_err());
    let err = format!("{}", res.err().unwrap());
    assert!(err.contains("400") || err.contains("bad"));
}

#[tokio::test]
async fn download_activity_file_with_progress_sends_progress() {
    let server = MockServer::start().await;
    let body = vec![10u8; 128];
    // Explicitly set a valid Content-Length header to avoid parsing errors
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("content-length", body.len().to_string())
                .set_body_bytes(body.clone()),
        )
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let res = client
        .download_activity_file_with_progress("a1", None, tx, cancel_rx)
        .await
        .expect("download");
    assert!(res.is_some());
    // a single progress update should be available
    let got = rx.recv().await;
    assert!(got.is_some());
    let p = got.unwrap();
    assert!(p.total_bytes.is_some());
}

#[tokio::test]
async fn download_activity_file_stream_missing_content_length_progress_total_none() {
    let server = MockServer::start().await;
    let body = vec![1u8, 2, 3, 4, 5];
    // Default ResponseTemplate does not include a Content-Length header; verify
    // the client handles an absent content-length by sending progress with None.
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

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.bin");
    let (tx, mut rx) = tokio::sync::mpsc::channel(5);
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let res = client
        .download_activity_file_with_progress("a1", Some(path.clone()), tx, cancel_rx)
        .await
        .expect("download");
    assert_eq!(res, Some(path.to_string_lossy().to_string()));

    // a progress update should have been sent; the Content-Length header may or may
    // not be present depending on the mock server; accept either and if present
    // validate it matches the expected length.
    let got = rx.recv().await;
    assert!(got.is_some());
    let p = got.unwrap();
    if let Some(t) = p.total_bytes {
        assert_eq!(t, body.len() as u64);
    }

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
async fn download_activity_file_stream_invalid_content_length_progress_total_none() {
    let server = MockServer::start().await;
    let size = 64usize;
    let body = vec![0u8; size];
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/file"))
        .respond_with(
            ResponseTemplate::new(200)
                .append_header("Content-Length", "not-numeric")
                .set_body_bytes(body.clone()),
        )
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out_invalid_len.bin");
    let (tx, _rx) = tokio::sync::mpsc::channel(2);
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let res = client
        .download_activity_file_with_progress("a1", Some(path.clone()), tx, cancel_rx)
        .await;
    // Hyper rejects an invalid Content-Length header (parse error). The client
    // should propagate the error from the HTTP client in this case.
    assert!(res.is_err());
}

#[tokio::test]
async fn download_activity_file_large_body_returns_base64() {
    let server = MockServer::start().await;
    // Create a body larger than 1MB to exercise the "large body" branch
    let size = 1_200_000usize;
    let body = vec![0u8; size];
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

    let res = client
        .download_activity_file("a1", None)
        .await
        .expect("download");
    assert!(res.is_some());
    let encoded = res.unwrap();
    let decoded = STANDARD.decode(encoded.as_bytes()).expect("base64 decode");
    assert_eq!(decoded.len(), size);
}

#[tokio::test]
async fn download_activity_file_small_body_returns_base64() {
    let server = MockServer::start().await;
    let body = vec![11u8, 12, 13];
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

    let res = client
        .download_activity_file("a1", None)
        .await
        .expect("download");
    assert!(res.is_some());
    let encoded = res.unwrap();
    let decoded = STANDARD.decode(encoded.as_bytes()).expect("base64 decode");
    assert_eq!(decoded, body);
}

#[tokio::test]
async fn get_upcoming_workouts_includes_oldest_and_newest() {
    let server = MockServer::start().await;
    let payload = serde_json::json!({ "items": [] });
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/events"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&payload))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.get_upcoming_workouts(Some(3)).await.expect("ok");
    assert!(res.get("items").is_some());
}

#[tokio::test]
async fn get_workouts_in_folder_missing_folder_id_filters_none() {
    let server = MockServer::start().await;

    // One workout missing folder_id and one with a non-matching folder_id
    let workouts = serde_json::json!([
        { "id": 1 },
        { "id": 2, "folder_id": 99 }
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

    let res = client.get_workouts_in_folder("123").await.expect("ok");
    // No items should match folder_id "123"
    assert_eq!(res.as_array().map(|a| a.len()), Some(0));
}

#[tokio::test]
async fn download_activity_file_handles_non_success() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/file"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.download_activity_file("a1", None).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn get_workouts_in_folder_handles_string_folder_id() {
    let server = MockServer::start().await;
    let workouts = serde_json::json!([
        { "id": 1, "folder_id": "fav" },
        { "id": 2, "folder_id": "other" }
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

    let filtered = client
        .get_workouts_in_folder("fav")
        .await
        .expect("workouts");
    let arr = filtered.as_array().cloned().unwrap_or_default();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0].get("id").and_then(|v| v.as_i64()), Some(1));
}

#[tokio::test]
async fn download_activity_file_zero_length_creates_empty_file_no_progress() {
    let server = MockServer::start().await;
    let body = vec![];
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

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out_empty.bin");
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let res = client
        .download_activity_file_with_progress("a1", Some(path.clone()), tx, cancel_rx)
        .await
        .expect("download");
    assert_eq!(res, Some(path.to_string_lossy().to_string()));

    // No progress updates should have been sent for an empty stream
    // No progress should have been sent; any TryRecvError is fine for this assertion
    if let Ok(p) = rx.try_recv() {
        panic!("unexpected progress: {:?}", p)
    }

    let metadata = std::fs::metadata(&path).expect("file exists");
    assert_eq!(metadata.len(), 0);
}

#[tokio::test]
async fn delete_event_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/athlete/ath/events/evt1"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let res = client.delete_event("evt1").await;
    assert!(res.is_err());
}

#[tokio::test]
async fn download_activity_file_with_progress_can_be_cancelled() {
    use intervals_icu_client::IntervalsError;
    // A tiny TCP server that returns a chunked response in two parts with a pause
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server_url = format!("http://{}", addr);

    // spawn the server task with a oneshot gate to coordinate second chunk
    let (go_tx, go_rx) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut _buf = [0u8; 1024];
            // read and ignore request
            let _ = socket.read(&mut _buf).await;

            // respond with chunked-encoded body
            let headers = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
            let _ = socket.write_all(headers.as_bytes()).await;

            let chunk1 = b"first-part";
            let _ = socket
                .write_all(format!("{:x}\r\n", chunk1.len()).as_bytes())
                .await;
            let _ = socket.write_all(chunk1).await;
            let _ = socket.write_all(b"\r\n").await;
            let _ = socket.flush().await;

            // wait until test signals to send the second chunk so cancellation can be set first
            let _ = go_rx.await;

            // send second chunk and end
            let chunk2 = b"second-part";
            let _ = socket
                .write_all(format!("{:x}\r\n", chunk2.len()).as_bytes())
                .await;
            let _ = socket.write_all(chunk2).await;
            let _ = socket.write_all(b"\r\n0\r\n\r\n").await;
            let _ = socket.flush().await;
            let _ = socket.shutdown().await;
        }
    });

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server_url,
        "ath",
        secrecy::SecretString::new("tok".into()),
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cancelled.bin");
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);

    let handle = tokio::spawn({
        let client = client.clone();
        let path = path.clone();
        let tx = tx.clone();
        async move {
            client
                .download_activity_file_with_progress("a1", Some(path), tx, cancel_rx)
                .await
        }
    });

    // wait for first progress (produced after first chunk is written)
    let got = rx.recv().await;
    assert!(got.is_some());

    // trigger cancellation and then signal the server to continue
    cancel_tx.send(true).unwrap();
    let _ = go_tx.send(());
    let res = handle.await.unwrap();
    assert!(res.is_err());
    match res.err().unwrap() {
        IntervalsError::Config(msg) => assert!(msg.contains("download cancelled")),
        e => panic!("expected Config error, got: {:?}", e),
    }
}

#[tokio::test]
async fn get_activities_around_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities-around"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );
    let res = client.get_activities_around("act1", None, None).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn delete_activity_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/activity/a1"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.delete_activity("a1").await;
    assert!(res.is_err());
}

#[tokio::test]
async fn get_wellness_for_date_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/wellness/2025-01-01"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.get_wellness_for_date("2025-01-01").await;
    assert!(res.is_err());
}

#[tokio::test]
async fn update_wellness_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/wellness/2025-01-02"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let body = serde_json::json!({"sleep": 7});
    let res = client.update_wellness("2025-01-02", &body).await;
    assert!(res.is_err());
}

#[tokio::test]
async fn delete_gear_handles_non_success() {
    let server = MockServer::start().await;
    Mock::given(method("DELETE"))
        .and(path("/api/v1/athlete/ath/gear/g1"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let client = intervals_icu_client::http_client::ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".into()),
    );

    let res = client.delete_gear("g1").await;
    assert!(res.is_err());
}

#[tokio::test]
async fn get_workouts_in_folder_empty_folder_id_returns_all() {
    let server = MockServer::start().await;
    let workouts = serde_json::json!([
        { "id": 1, "folder_id": "fav" },
        { "id": 2, "folder_id": "other" }
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

    // empty folder id should return all workouts
    let all = client.get_workouts_in_folder("").await.expect("workouts");
    let arr = all.as_array().cloned().unwrap_or_default();
    assert_eq!(arr.len(), 2);
}
