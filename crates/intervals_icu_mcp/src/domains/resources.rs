use rmcp::model::RawResource;

/// Return a `RawResource` descriptor for the athlete profile resource.
/// Kept small so `lib.rs` can delegate to it.
pub fn athlete_profile_resource() -> RawResource {
    let mut resource = RawResource::new("intervals-icu://athlete/profile", "Athlete Profile");
    resource.description = Some(
        "Athlete profile data including: profile (id, name), fitness metrics (ctl, atl, tsb, rampRate), \
         sport settings (ftp, lthr, max_hr, power_zones, hr_zones, pace_zones). \
         Returns JSON with fields: profile.id, profile.name, fitness.*, sport_settings[]."
            .to_string(),
    );
    resource.mime_type = Some("application/json".to_string());
    resource
}

/// Build the textual JSON payload for the athlete profile resource by fetching
/// data from the provided `IntervalsClient`.
pub async fn build_athlete_profile_text(
    client: &dyn intervals_icu_client::IntervalsClient,
) -> Result<String, intervals_icu_client::IntervalsError> {
    let profile = client.get_athlete_profile().await?;
    let fitness = client.get_fitness_summary().await?;
    let sport_settings = client.get_sport_settings().await?;

    let text = format!(
        r#"{{
  "profile": {{
    "id": "{}",
    "name": {}
  }},
  "fitness": {},
  "sport_settings": {},
  "timestamp": "{}"
}}"#,
        profile.id,
        profile
            .name
            .as_ref()
            .map(|n| format!("\"{}\"", n))
            .unwrap_or_else(|| "null".to_string()),
        serde_json::to_string_pretty(&fitness).unwrap_or_else(|_| "{}".to_string()),
        serde_json::to_string_pretty(&sport_settings).unwrap_or_else(|_| "[]".to_string()),
        chrono::Utc::now().to_rfc3339()
    );

    Ok(text)
}

// =============================================================================
// P4.3 — MCP Resources for Streaming Time-Series Data
// =============================================================================

const MAX_DOWNSAMPLE_POINTS: usize = 200;

/// Return resource descriptors for activity stream resources.
pub fn activity_stream_resources() -> Vec<RawResource> {
    let mut resources = Vec::new();

    let mut power = RawResource::new(
        "activity://{activity_id}/streams/power",
        "Activity Power Stream",
    );
    power.description = Some(
        "Power time-series data for a specific activity. URI template: activity://{activity_id}/streams/power. \
         Returns downsampled power data (max 200 points) as JSON array."
            .to_string(),
    );
    power.mime_type = Some("application/json".to_string());
    resources.push(power);

    let mut hr = RawResource::new(
        "activity://{activity_id}/streams/hr",
        "Activity Heart Rate Stream",
    );
    hr.description = Some(
        "Heart rate time-series data for a specific activity. URI template: activity://{activity_id}/streams/hr. \
         Returns downsampled HR data (max 200 points) as JSON array."
            .to_string(),
    );
    hr.mime_type = Some("application/json".to_string());
    resources.push(hr);

    let mut pace = RawResource::new(
        "activity://{activity_id}/streams/pace",
        "Activity Pace Stream",
    );
    pace.description = Some(
        "Pace/velocity time-series data for a specific activity. URI template: activity://{activity_id}/streams/pace. \
         Returns downsampled pace data (max 200 points) as JSON array."
            .to_string(),
    );
    pace.mime_type = Some("application/json".to_string());
    resources.push(pace);

    resources
}

/// Downsample a numeric stream to at most `max_points` points.
/// Uses uniform sampling: keeps every Nth point.
pub fn downsample_stream(data: &[f64], max_points: usize) -> Vec<f64> {
    if data.len() <= max_points {
        return data.to_vec();
    }
    let step = data.len() / max_points;
    let mut result = Vec::with_capacity(max_points);
    let mut i = 0;
    while i < data.len() && result.len() < max_points {
        result.push(data[i]);
        i += step;
    }
    result
}

/// Resolve a stream URI and return the downsampled data.
/// Supports: activity://{id}/streams/power, activity://{id}/streams/hr, activity://{id}/streams/pace
pub async fn resolve_stream_resource(
    uri: &str,
    client: &dyn intervals_icu_client::IntervalsClient,
) -> Result<String, String> {
    let parts: Vec<&str> = uri.split('/').collect();
    if parts.len() < 5 {
        return Err(format!("Invalid resource URI: {uri}"));
    }

    let activity_id = parts[2];
    let stream_type = parts[4];

    let stream_key = match stream_type {
        "power" => "watts",
        "hr" => "heartrate",
        "pace" => "velocity_smooth",
        _ => return Err(format!("Unknown stream type: {stream_type}")),
    };

    let streams = client
        .get_activity_streams(activity_id, None)
        .await
        .map_err(|e| format!("Failed to fetch streams: {e}"))?;

    let Some(stream_obj) = streams.as_object() else {
        return Err("Streams response is not an object".to_string());
    };

    let Some(stream_data) = stream_obj.get(stream_key).and_then(|v| v.as_array()) else {
        return Err(format!(
            "Stream type '{stream_type}' not found in activity streams"
        ));
    };

    let numeric: Vec<f64> = stream_data
        .iter()
        .filter_map(|v| v.as_f64().or_else(|| v.as_i64().map(|n| n as f64)))
        .collect();

    let downsampled = downsample_stream(&numeric, MAX_DOWNSAMPLE_POINTS);

    serde_json::to_string_pretty(&downsampled).map_err(|e| format!("Serialization error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::mock::MockIntervalsClient;
    use intervals_icu_client::AthleteProfile;
    use serde_json::json;

    // ── downsample_stream edge cases ──

    #[test]
    fn downsample_returns_all_when_under_limit() {
        let data = vec![1.0, 2.0, 3.0];
        let result = downsample_stream(&data, 200);
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn downsample_reduces_large_stream() {
        let data: Vec<f64> = (0..1000).map(|i| i as f64).collect();
        let result = downsample_stream(&data, 200);
        assert_eq!(result.len(), 200);
        assert!((result[0] - 0.0).abs() < 0.01);
    }

    #[test]
    fn downsample_empty() {
        let data: Vec<f64> = vec![];
        let result = downsample_stream(&data, 200);
        assert!(result.is_empty());
    }

    #[test]
    fn downsample_exactly_at_limit() {
        let data: Vec<f64> = (0..200).map(|i| i as f64).collect();
        let result = downsample_stream(&data, 200);
        assert_eq!(result.len(), 200);
        assert!((result[0] - 0.0).abs() < 0.01);
        assert!((result[199] - 199.0).abs() < 0.01);
    }

    #[test]
    fn downsample_single_element() {
        let data = vec![42.0];
        let result = downsample_stream(&data, 200);
        assert_eq!(result.len(), 1);
        assert!((result[0] - 42.0).abs() < 0.01);
    }

    #[test]
    fn downsample_exact_multiple() {
        let data: Vec<f64> = (0..400).map(|i| i as f64).collect();
        let result = downsample_stream(&data, 200);
        assert_eq!(result.len(), 200);
        assert!((result[0] - 0.0).abs() < 0.01);
        assert!((result[1] - 2.0).abs() < 0.01);
    }

    #[test]
    fn downsample_exactly_one_below_limit() {
        let data: Vec<f64> = (0..199).map(|i| i as f64).collect();
        let result = downsample_stream(&data, 200);
        assert_eq!(result.len(), 199);
    }

    // ── activity_stream_resources ──

    #[test]
    fn activity_stream_resources_returns_three_uris() {
        let resources = activity_stream_resources();
        assert_eq!(resources.len(), 3);
        assert!(resources.iter().any(|r| r.uri.contains("power")));
        assert!(resources.iter().any(|r| r.uri.contains("hr")));
        assert!(resources.iter().any(|r| r.uri.contains("pace")));
    }

    #[test]
    fn activity_stream_resources_all_have_descriptions_and_mime() {
        let resources = activity_stream_resources();
        for r in &resources {
            assert!(
                r.description.is_some(),
                "resource {} missing description",
                r.uri
            );
            assert_eq!(
                r.mime_type,
                Some("application/json".into()),
                "resource {} missing mime type",
                r.uri
            );
        }
    }

    #[test]
    fn activity_stream_resources_uri_templates() {
        let resources = activity_stream_resources();
        for r in &resources {
            assert!(
                r.uri.contains("{activity_id}"),
                "URI doesn't contain template: {}",
                r.uri
            );
        }
    }

    // ── athlete_profile_resource ──

    #[test]
    fn athlete_profile_resource_basic() {
        let resource = athlete_profile_resource();
        assert_eq!(resource.uri, "intervals-icu://athlete/profile");
        assert_eq!(resource.name, "Athlete Profile");
        assert!(resource.description.is_some());
        assert!(
            resource
                .description
                .as_ref()
                .unwrap()
                .contains("Athlete profile data")
        );
        assert_eq!(resource.mime_type, Some("application/json".to_string()));
    }

    // ── resolve_stream_resource error cases ──

    #[tokio::test]
    async fn resolve_stream_uri_too_short() {
        let client = MockIntervalsClient::default();
        let err = resolve_stream_resource("activity://a1/streams", &client)
            .await
            .unwrap_err();
        assert!(err.contains("Invalid resource URI"));
    }

    #[tokio::test]
    async fn resolve_stream_uri_unknown_type() {
        let client = MockIntervalsClient::default();
        let err = resolve_stream_resource("activity://a1/streams/unknown", &client)
            .await
            .unwrap_err();
        assert!(err.contains("Unknown stream type"));
    }

    #[tokio::test]
    async fn resolve_stream_not_an_object() {
        let client = MockIntervalsClient::builder().with_streams(json!([1, 2, 3]));
        let err = resolve_stream_resource("activity://a1/streams/power", &client)
            .await
            .unwrap_err();
        assert!(err.contains("not an object"));
    }

    #[tokio::test]
    async fn resolve_stream_type_not_found() {
        let client = MockIntervalsClient::builder().with_streams(json!({"heartrate": [100, 110]}));
        let err = resolve_stream_resource("activity://a1/streams/power", &client)
            .await
            .unwrap_err();
        assert!(err.contains("not found"));
    }

    #[tokio::test]
    async fn resolve_stream_empty_object() {
        let client = MockIntervalsClient::builder().with_streams(json!({}));
        let err = resolve_stream_resource("activity://a1/streams/power", &client)
            .await
            .unwrap_err();
        assert!(err.contains("not found"));
    }

    // ── resolve_stream_resource success cases ──

    #[tokio::test]
    async fn resolve_stream_power_success() {
        let client = MockIntervalsClient::builder()
            .with_streams(json!({"watts": [100, 200, 300, 400, 500]}));
        let result = resolve_stream_resource("activity://a1/streams/power", &client)
            .await
            .unwrap();
        let parsed: Vec<f64> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 5);
        assert!((parsed[0] - 100.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn resolve_stream_hr_success() {
        let client =
            MockIntervalsClient::builder().with_streams(json!({"heartrate": [120, 130, 140]}));
        let result = resolve_stream_resource("activity://a1/streams/hr", &client)
            .await
            .unwrap();
        let parsed: Vec<f64> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 3);
        assert!((parsed[0] - 120.0).abs() < 0.01);
    }

    #[tokio::test]
    async fn resolve_stream_pace_success() {
        let client = MockIntervalsClient::builder()
            .with_streams(json!({"velocity_smooth": [3.5, 4.0, 4.5]}));
        let result = resolve_stream_resource("activity://a1/streams/pace", &client)
            .await
            .unwrap();
        let parsed: Vec<f64> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 3);
        assert!((parsed[0] - 3.5).abs() < 0.01);
    }

    #[tokio::test]
    async fn resolve_stream_large_downsampled() {
        let watts: Vec<i64> = (0..1000).collect();
        let client = MockIntervalsClient::builder().with_streams(json!({"watts": watts}));
        let result = resolve_stream_resource("activity://a1/streams/power", &client)
            .await
            .unwrap();
        let parsed: Vec<f64> = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed.len(), 200);
    }

    #[tokio::test]
    async fn resolve_stream_empty_numeric_data() {
        let client = MockIntervalsClient::builder().with_streams(json!({"watts": []}));
        let result = resolve_stream_resource("activity://a1/streams/power", &client)
            .await
            .unwrap();
        assert_eq!(result, "[]");
    }

    // ── build_athlete_profile_text ──

    #[tokio::test]
    async fn build_athlete_profile_text_success() {
        let client = MockIntervalsClient::builder()
            .with_athlete_profile(AthleteProfile {
                id: "athlete-1".into(),
                name: Some("Test Athlete".into()),
            })
            .with_fitness_summary(json!({"ctl": 80, "atl": 60, "tsb": 20}))
            .with_sport_settings(json!([{"sport": "cycling", "ftp": 250}]));
        let text = build_athlete_profile_text(&client).await.unwrap();
        assert!(text.contains("athlete-1"), "should contain athlete id");
        assert!(text.contains("Test Athlete"), "should contain athlete name");
        assert!(text.contains("ctl"), "should contain fitness data");
        assert!(text.contains("ftp"), "should contain sport settings");
    }

    #[tokio::test]
    async fn build_athlete_profile_text_no_name() {
        let client = MockIntervalsClient::builder()
            .with_athlete_profile(AthleteProfile {
                id: "athlete-2".into(),
                name: None,
            })
            .with_fitness_summary(json!({"ctl": 50}))
            .with_sport_settings(json!([]));
        let text = build_athlete_profile_text(&client).await.unwrap();
        assert!(text.contains("athlete-2"));
        assert!(text.contains("null"), "name should be null when None");
    }

    #[tokio::test]
    async fn build_athlete_profile_text_null_fitness() {
        let client = MockIntervalsClient::builder()
            .with_athlete_profile(AthleteProfile {
                id: "athlete-3".into(),
                name: None,
            })
            .with_fitness_summary(json!(null))
            .with_sport_settings(json!([]));
        let text = build_athlete_profile_text(&client).await.unwrap();
        assert!(text.contains("athlete-3"));
    }
}
