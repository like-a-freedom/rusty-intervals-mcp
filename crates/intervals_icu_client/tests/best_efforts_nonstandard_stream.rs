use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn best_efforts_uses_nonstandard_stream_name_if_present() {
    let server = MockServer::start().await;

    // initial default power attempt -> 422
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act9/best-efforts"))
        .and(query_param("stream", "power"))
        .respond_with(ResponseTemplate::new(422))
        .mount(&server)
        .await;

    // streams endpoint returns `watts` as available stream
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act9/streams"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "streams": { "time": [0,1,2], "watts": [150,200,250] }
        })))
        .mount(&server)
        .await;

    // expect best-efforts call with stream=watts & duration=60 -> return 200
    let body = serde_json::json!({"best_efforts": [{"duration": 60, "watts": 250}]});
    Mock::given(method("GET"))
        .and(path("/api/v1/activity/act9/best-efforts"))
        .and(query_param("stream", "watts"))
        .and(query_param("duration", "60"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&body))
        .mount(&server)
        .await;

    let client = ReqwestIntervalsClient::new(&server.uri(), "ath", SecretString::new("tok".into()));
    let res: serde_json::Value = client
        .get_best_efforts("act9", None)
        .await
        .expect("best efforts via watts");
    assert!(res.get("best_efforts").is_some());
}
