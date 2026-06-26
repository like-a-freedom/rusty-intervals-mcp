use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{Duration, Utc};
use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LIVE_OPENAPI_SPEC_URL: &str = "https://intervals.icu/api/v1/docs";

async fn fetch_live_openapi_spec() -> serde_json::Value {
    reqwest::Client::new()
        .get(LIVE_OPENAPI_SPEC_URL)
        .send()
        .await
        .expect("fetch live OpenAPI spec")
        .error_for_status()
        .expect("live OpenAPI status")
        .json()
        .await
        .expect("parse live OpenAPI spec")
}

fn spec_operation<'a>(
    spec: &'a serde_json::Value,
    path_name: &str,
    method_name: &str,
) -> &'a serde_json::Value {
    spec.get("paths")
        .and_then(|paths| paths.get(path_name))
        .and_then(|path_item| path_item.get(method_name))
        .unwrap_or_else(|| panic!("missing {method_name} {path_name} in live spec"))
}

fn assert_query_param(
    spec: &serde_json::Value,
    path_name: &str,
    method_name: &str,
    param_name: &str,
) {
    let params = spec_operation(spec, path_name, method_name)
        .get("parameters")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("missing parameters for {method_name} {path_name}"));
    assert!(
        params.iter().any(|param| {
            param.get("in").and_then(serde_json::Value::as_str) == Some("query")
                && param.get("name").and_then(serde_json::Value::as_str) == Some(param_name)
        }),
        "missing query param {param_name} for {method_name} {path_name}"
    );
}

#[tokio::test]
async fn get_activities_around_uses_activities_around_path() {
    let mock_server = MockServer::start().await;
    let athlete = "ath";
    let expected = serde_json::json!([
        {"id": "a-prev", "name": "Warmup Ride"}
    ]);

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/activities-around"))
        .and(query_param("activity_id", "a1"))
        .and(query_param("limit", "5"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&expected))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()))
            .expect("new");
    let res = client
        .get_activities_around("a1", Some(5), None)
        .await
        .expect("activities around response");
    assert_eq!(res, expected);
}

#[tokio::test]
async fn apply_sport_settings_uses_put() {
    let mock_server = MockServer::start().await;
    let sport = "Run";
    let athlete = "ath";

    Mock::given(method("GET"))
        .and(path(format!("/api/v1/athlete/{athlete}/sport-settings")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"id": 1_783_043, "types": ["Run", "VirtualRun", "TrailRun"]}
        ])))
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/sport-settings/1783043/apply"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status":"ok"})))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()))
            .expect("new");
    let res = client.apply_sport_settings(sport).await;
    assert!(res.is_ok());
    let v = res.unwrap();
    assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
}

#[tokio::test]
async fn get_wellness_translates_days_back_to_oldest_and_newest() {
    let mock_server = MockServer::start().await;
    let athlete = "ath";
    let before = Utc::now().date_naive();

    Mock::given(method("GET"))
        .and(path(format!("/api/v1/athlete/{athlete}/wellness")))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()))
            .expect("new");
    let res = client
        .get_wellness(Some(5))
        .await
        .expect("wellness response");
    let after = Utc::now().date_naive();

    assert_eq!(res, serde_json::json!([]));

    let requests = mock_server
        .received_requests()
        .await
        .expect("received requests should be available");
    assert_eq!(requests.len(), 1);

    let query: std::collections::HashMap<String, String> = requests[0]
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let newest = query.get("newest").expect("newest query param");
    let oldest = query.get("oldest").expect("oldest query param");

    let before_newest = before.to_string();
    let after_newest = after.to_string();
    let before_oldest = (before - Duration::days(5)).to_string();
    let after_oldest = (after - Duration::days(5)).to_string();

    assert!(
        (newest == &before_newest && oldest == &before_oldest)
            || (newest == &after_newest && oldest == &after_oldest),
        "unexpected wellness date window: oldest={oldest}, newest={newest}"
    );
}

#[tokio::test]
async fn get_events_translates_days_back_to_oldest_and_newest() {
    let mock_server = MockServer::start().await;
    let athlete = "ath";
    let before = Utc::now().date_naive();
    let event_day = before.to_string();

    Mock::given(method("GET"))
        .and(path(format!("/api/v1/athlete/{athlete}/events")))
        .and(query_param("limit", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "id": "evt-1",
                "start_date_local": event_day,
                "name": "Workout",
                "category": "WORKOUT"
            }
        ])))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()))
            .expect("new");
    let res = client
        .get_events(Some(7), Some(3))
        .await
        .expect("events response");
    let after = Utc::now().date_naive();

    assert_eq!(res.len(), 1);
    assert_eq!(res[0].id.as_deref(), Some("evt-1"));

    let requests = mock_server
        .received_requests()
        .await
        .expect("received requests should be available");
    assert_eq!(requests.len(), 1);

    let query: std::collections::HashMap<String, String> = requests[0]
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let newest = query.get("newest").expect("newest query param");
    let oldest = query.get("oldest").expect("oldest query param");

    let before_newest = before.to_string();
    let after_newest = after.to_string();
    let before_oldest = (before - Duration::days(7)).to_string();
    let after_oldest = (after - Duration::days(7)).to_string();

    assert!(
        (newest == &before_newest && oldest == &before_oldest)
            || (newest == &after_newest && oldest == &after_oldest),
        "unexpected event date window: oldest={oldest}, newest={newest}"
    );
}

#[tokio::test]
async fn download_fit_file_uses_fit_file_endpoint() {
    let mock_server = MockServer::start().await;
    let bytes = vec![1u8, 2, 3];

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/fit-file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()))
            .expect("new");

    let res = client
        .download_fit_file("a1", None)
        .await
        .expect("fit file download");
    assert_eq!(res, Some(STANDARD.encode(bytes)));
}

#[tokio::test]
async fn download_gpx_file_uses_gpx_file_endpoint() {
    let mock_server = MockServer::start().await;
    let bytes = vec![4u8, 5, 6];

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/a1/gpx-file"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(bytes.clone()))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()))
            .expect("new");

    let res = client
        .download_gpx_file("a1", None)
        .await
        .expect("gpx file download");
    assert_eq!(res, Some(STANDARD.encode(bytes)));
}

#[tokio::test]
async fn create_gear_reminder_uses_singular_reminder_endpoint() {
    let mock_server = MockServer::start().await;
    let expected = serde_json::json!({"id":"g1"});

    Mock::given(method("POST"))
        .and(path("/api/v1/athlete/ath/gear/g1/reminder"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&expected))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()))
            .expect("new");

    let res = client
        .create_gear_reminder("g1", &serde_json::json!({"note":"check chain"}))
        .await
        .expect("gear reminder create response");
    assert_eq!(res, expected);
}

#[tokio::test]
async fn update_wellness_bulk_uses_bulk_endpoint() {
    let mock_server = MockServer::start().await;

    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/wellness-bulk"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()))
            .expect("new");
    let res = client
        .update_wellness_bulk(&[serde_json::json!({"id": "2026-03-01", "sleepSecs": 28800})])
        .await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn weather_config_uses_spec_endpoints() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/weather-config"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"provider": "yr.no"})),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/weather-config"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"provider": "open-meteo"})),
        )
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()))
            .expect("new");

    let current = client.get_weather_config().await.expect("weather config");
    assert_eq!(
        current.get("provider").and_then(|v| v.as_str()),
        Some("yr.no")
    );

    let updated = client
        .update_weather_config(&serde_json::json!({"provider": "open-meteo"}))
        .await
        .expect("update weather config");
    assert_eq!(
        updated.get("provider").and_then(|v| v.as_str()),
        Some("open-meteo")
    );
}

#[tokio::test]
async fn routes_use_current_spec_paths() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/routes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"route": {"id": 11}, "count": 4}
        ])))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/routes/11"))
        .and(query_param("includePath", "true"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id": 11, "name": "Lunch Loop"})),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("PUT"))
        .and(path("/api/v1/athlete/ath/routes/11"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"id": 11, "name": "Updated Loop"})),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/athlete/ath/routes/11/similarity/12"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"similarity": 0.97})),
        )
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), "ath", SecretString::new("key".into()))
            .expect("new");

    let routes = client.list_routes().await.expect("list routes");
    assert_eq!(routes.as_array().map(Vec::len), Some(1));

    let route = client.get_route(11, true).await.expect("get route");
    assert_eq!(
        route.get("id").and_then(serde_json::Value::as_i64),
        Some(11)
    );

    let updated = client
        .update_route(11, &serde_json::json!({"name": "Updated Loop"}))
        .await
        .expect("update route");
    assert_eq!(
        updated.get("name").and_then(|v| v.as_str()),
        Some("Updated Loop")
    );

    let similarity = client
        .get_route_similarity(11, 12)
        .await
        .expect("route similarity");
    assert_eq!(
        similarity
            .get("similarity")
            .and_then(serde_json::Value::as_f64),
        Some(0.97)
    );
}

#[tokio::test]
#[ignore = "hits the live Intervals.icu OpenAPI endpoint"]
async fn live_openapi_spec_contract_smoke() {
    let spec = fetch_live_openapi_spec().await;

    spec_operation(&spec, "/api/v1/activity/{id}/fit-file", "get");
    spec_operation(&spec, "/api/v1/activity/{id}/gpx-file", "get");
    spec_operation(
        &spec,
        "/api/v1/athlete/{id}/gear/{gearId}/reminder/{reminderId}",
        "put",
    );
    spec_operation(&spec, "/api/v1/athlete/{id}/weather-config", "get");
    spec_operation(&spec, "/api/v1/athlete/{id}/weather-config", "put");
    spec_operation(&spec, "/api/v1/athlete/{id}/routes", "get");
    spec_operation(&spec, "/api/v1/athlete/{id}/routes/{route_id}", "get");
    spec_operation(&spec, "/api/v1/athlete/{id}/routes/{route_id}", "put");
    spec_operation(
        &spec,
        "/api/v1/athlete/{id}/routes/{route_id}/similarity/{other_id}",
        "get",
    );
    spec_operation(&spec, "/api/v1/athlete/{id}/wellness-bulk", "put");
    spec_operation(
        &spec,
        "/api/v1/athlete/{athleteId}/sport-settings/{id}/apply",
        "put",
    );

    assert_query_param(
        &spec,
        "/api/v1/athlete/{id}/activities-around",
        "get",
        "activity_id",
    );
    assert_query_param(
        &spec,
        "/api/v1/athlete/{id}/activities-around",
        "get",
        "route_id",
    );
    assert_query_param(
        &spec,
        "/api/v1/athlete/{id}/events{format}",
        "get",
        "oldest",
    );
    assert_query_param(
        &spec,
        "/api/v1/athlete/{id}/events{format}",
        "get",
        "newest",
    );
    assert_query_param(&spec, "/api/v1/athlete/{id}/wellness{ext}", "get", "oldest");
    assert_query_param(&spec, "/api/v1/athlete/{id}/wellness{ext}", "get", "newest");
    assert_query_param(
        &spec,
        "/api/v1/athlete/{id}/routes/{route_id}",
        "get",
        "includePath",
    );
}
