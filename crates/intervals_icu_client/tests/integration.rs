use base64::{Engine as _, engine::general_purpose::STANDARD};
use intervals_icu_client::{AthleteProfile, IntervalsClient};
use secrecy::SecretString;
use wiremock::matchers::{method, path};
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
    let input =
        serde_json::json!({"start_date_local":"2025-12-15","name":"Test","category":"NOTE"});
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
    let b64 = client
        .download_activity_file("a1", None)
        .await
        .expect("b64")
        .expect("some");
    assert_eq!(STANDARD.decode(&b64).unwrap(), body);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("out.bin");
    let res = client
        .download_activity_file("a1", Some(path.clone()))
        .await
        .expect("ok");
    assert!(res.is_none());
    let read = std::fs::read(path).unwrap();
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
    let c = client.get_power_curves(Some(30)).await.expect("curves");
    assert!(c.get("best").is_some());
    let h = client.get_gap_histogram("a1").await.expect("hist");
    assert!(h.get("bins").is_some());
}
