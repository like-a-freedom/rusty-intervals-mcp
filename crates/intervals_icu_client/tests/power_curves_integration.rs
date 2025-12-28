use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_power_curves_normalizes_type_and_sends_curves() {
    let mock_server = MockServer::start().await;

    // Expect a GET with type=Run and curves=7d
    let m = Mock::given(method("GET"))
        .and(path("/api/v1/athlete/test_ath/power-curves"))
        .and(query_param("type", "Run"))
        .and(query_param("curves", "7d"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1);

    m.mount(&mock_server).await;

    let client = ReqwestIntervalsClient::new(
        &mock_server.uri(),
        "test_ath",
        SecretString::new("key".into()),
    );
    let res: serde_json::Value = client.get_power_curves(Some(7), "run").await.unwrap();
    assert_eq!(res, serde_json::json!({"ok": true}));
}
