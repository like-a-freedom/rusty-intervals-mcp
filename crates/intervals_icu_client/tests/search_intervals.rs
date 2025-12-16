use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn search_intervals_calls_interval_search_path() {
    let mock_server = MockServer::start().await;
    let athlete = "ath";

    Mock::given(method("GET"))
        .and(path(format!(
            "/api/v1/athlete/{}/activities/interval-search",
            athlete
        )))
        .and(query_param("minSecs", "30"))
        .and(query_param("maxSecs", "120"))
        .and(query_param("minIntensity", "70"))
        .and(query_param("maxIntensity", "90"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([{"id":"a1"}])))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()));
    let res = client
        .search_intervals(Some(30), Some(120), Some(70), Some(90), None)
        .await;
    assert!(res.is_ok());
    let v = res.unwrap();
    assert!(v.as_array().is_some());
}
