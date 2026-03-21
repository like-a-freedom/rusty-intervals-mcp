#[allow(dead_code)]
mod test_helpers;

use intervals_icu_mcp::domains::resources::{athlete_profile_resource, build_athlete_profile_text};
use serde_json::Value;
use test_helpers::MockClient;

#[test]
fn athlete_profile_resource_exposes_expected_metadata() {
    let resource = athlete_profile_resource();

    assert_eq!(resource.uri, "intervals-icu://athlete/profile");
    assert_eq!(resource.name, "Athlete Profile");
    assert_eq!(resource.mime_type.as_deref(), Some("application/json"));

    let description = resource
        .description
        .expect("resource description should exist");
    assert!(description.contains("fitness metrics"));
    assert!(description.contains("sport settings"));
}

#[tokio::test]
async fn build_athlete_profile_text_returns_json_with_all_sections() {
    let text = build_athlete_profile_text(&MockClient)
        .await
        .expect("athlete profile resource should render");

    let value: Value = serde_json::from_str(&text).expect("resource payload should be valid json");

    assert_eq!(value["profile"]["id"], "test_athlete");
    assert_eq!(value["profile"]["name"], "Test Athlete");
    assert!(value.get("fitness").is_some());
    assert!(value.get("sport_settings").is_some());
    assert!(value["timestamp"].as_str().is_some());
}
