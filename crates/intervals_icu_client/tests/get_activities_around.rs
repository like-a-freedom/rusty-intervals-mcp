use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_activities_around_uses_activities_around_path() {
    let mock_server = MockServer::start().await;
    let athlete = "ath";

    Mock::given(method("GET"))
        .and(path(format!(
            "/api/v1/athlete/{}/activities-around",
            athlete
        )))
        .and(query_param("around", "a1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"before": [], "after": []})),
        )
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()));
    let res = client.get_activities_around("a1", Some(5)).await;
    assert!(res.is_ok());
}
