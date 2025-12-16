use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_fitness_summary_uses_athlete_path() {
    let mock_server = MockServer::start().await;
    let athlete = "ath";

    Mock::given(method("GET"))
        .and(path(format!("/api/v1/athlete/{}", athlete)))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "fitness": 12.3,
            "fatigue": 8.1,
            "form": 4.2
        })))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()));
    let res = client.get_fitness_summary().await;
    assert!(res.is_ok());
    let v = res.unwrap();
    assert_eq!(v.get("fitness").and_then(|f| f.as_f64()), Some(12.3));
}
