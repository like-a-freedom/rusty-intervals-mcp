use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn best_efforts_detects_top_level_streams() {
    let server = MockServer::start().await;

    // initial power attempt -> 422
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act7/best-efforts"))
        .and(query_param("stream", "power"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    // streams endpoint returns speed as a top-level key
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act7/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "time": [0,1,2],
            "speed": [2.5,3.0,3.5]
        })))
        .mount(&server)
        .await;

    // expect best-efforts call with stream=speed & duration=60 -> return 200
    let body = serde_json::json!({"best_efforts": [{"duration": 60, "speed": 3.5}]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act7/best-efforts"))
        .and(query_param("stream", "speed"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = ReqwestIntervalsClient::new(&server.uri(), "ath", SecretString::new("tok".into()));
    let efforts: serde_json::Value = client
        .get_best_efforts("act7", None)
        .await
        .expect("best efforts via detected stream");
    assert!(efforts.get("best_efforts").is_some());
}

#[tokio::test]
async fn best_efforts_detects_streams_array_form() {
    let server = MockServer::start().await;

    // initial power attempt -> 422
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act8/best-efforts"))
        .and(query_param("stream", "power"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    // streams endpoint returns array with name fields
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act8/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "streams": [{"name":"time"}, {"name":"pace"}]
        })))
        .mount(&server)
        .await;

    // expect best-efforts call with stream=pace & duration=60 -> return 200
    let body = serde_json::json!({"best_efforts": [{"duration": 60, "pace": 200}]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act8/best-efforts"))
        .and(query_param("stream", "pace"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = ReqwestIntervalsClient::new(&server.uri(), "ath", SecretString::new("tok".into()));
    let efforts: serde_json::Value = client
        .get_best_efforts("act8", None)
        .await
        .expect("best efforts via detected stream");
    assert!(efforts.get("best_efforts").is_some());
}

#[tokio::test]
async fn best_efforts_detects_top_level_array_streams_form() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act10/best-efforts"))
        .and(query_param("stream", "power"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act10/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {"type": "time", "name": null, "data": [0, 1, 2]},
            {"type": "watts", "name": null, "data": [150, 200, 250]}
        ])))
        .mount(&server)
        .await;

    let body = serde_json::json!({"best_efforts": [{"duration": 60, "watts": 250}]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act10/best-efforts"))
        .and(query_param("stream", "watts"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = ReqwestIntervalsClient::new(&server.uri(), "ath", SecretString::new("tok".into()));
    let efforts: serde_json::Value = client
        .get_best_efforts("act10", None)
        .await
        .expect("best efforts via top-level array stream discovery");
    assert!(efforts.get("best_efforts").is_some());
}

#[tokio::test]
async fn best_efforts_prefers_watts_over_distance_and_annotates_stream() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act11/best-efforts"))
        .and(query_param("stream", "power"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act11/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "streams": { "time": [0,1,2], "distance": [0,10,20], "watts": [150,200,250] }
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act11/best-efforts"))
        .and(query_param("stream", "distance"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "efforts": [{"average": 6968.817, "duration": 60, "distance": null}]
        })))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act11/best-efforts"))
        .and(query_param("stream", "watts"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "efforts": [{"average": 303.51666, "duration": 60, "distance": null}]
        })))
        .mount(&server)
        .await;

    let client = ReqwestIntervalsClient::new(&server.uri(), "ath", SecretString::new("tok".into()));
    let efforts: serde_json::Value = client
        .get_best_efforts("act11", None)
        .await
        .expect("best efforts should prefer watts when available");

    assert_eq!(
        efforts.get("stream").and_then(serde_json::Value::as_str),
        Some("watts")
    );
    assert_eq!(
        efforts
            .get("efforts")
            .and_then(serde_json::Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|effort| effort.get("average"))
            .and_then(serde_json::Value::as_f64),
        Some(303.51666)
    );
}
