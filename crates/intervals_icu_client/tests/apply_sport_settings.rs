use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn apply_sport_settings_uses_put() {
    let mock_server = MockServer::start().await;
    let sport = "RUNNING";
    let athlete = "ath";

    Mock::given(method("PUT"))
        .and(path(format!(
            "/api/v1/athlete/{}/sport-settings/{}/apply",
            athlete, sport
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status":"ok"})))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()));
    let res = client.apply_sport_settings(sport).await;
    assert!(res.is_ok());
    let v = res.unwrap();
    assert_eq!(v.get("status").and_then(|s| s.as_str()), Some("ok"));
}
