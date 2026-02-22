use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn get_fitness_summary_uses_athlete_path() {
    let mock_server = MockServer::start().await;
    let athlete = "ath";

    // API returns array of SummaryWithCats objects (most recent first)
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/v1/athlete/{}/athlete-summary.json",
            athlete
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "fitness": 12.3,
                "fatigue": 8.1,
                "form": 4.2,
                "rampRate": 1.5,
                "date": "2025-02-22"
            },
            {
                "fitness": 11.0,
                "fatigue": 7.0,
                "form": 4.0,
                "rampRate": 1.2,
                "date": "2025-02-21"
            }
        ])))
        .mount(&mock_server)
        .await;

    let client =
        ReqwestIntervalsClient::new(&mock_server.uri(), athlete, SecretString::new("key".into()));
    let res = client.get_fitness_summary().await;
    if let Err(ref e) = res {
        println!("Error: {:?}", e);
    }
    assert!(res.is_ok());
    let v = res.unwrap();
    // Response is array, check first element
    assert!(v.is_array());
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    let first = &arr[0];
    assert_eq!(first.get("fitness").and_then(|f| f.as_f64()), Some(12.3));
    assert_eq!(first.get("fatigue").and_then(|f| f.as_f64()), Some(8.1));
    assert_eq!(first.get("form").and_then(|f| f.as_f64()), Some(4.2));
    assert_eq!(first.get("rampRate").and_then(|f| f.as_f64()), Some(1.5));
}
