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
