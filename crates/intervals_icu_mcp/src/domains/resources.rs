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
    fn activity_stream_resources_returns_three_uris() {
        let resources = activity_stream_resources();
        assert_eq!(resources.len(), 3);
        assert!(resources.iter().any(|r| r.uri.contains("power")));
        assert!(resources.iter().any(|r| r.uri.contains("hr")));
        assert!(resources.iter().any(|r| r.uri.contains("pace")));
    }
}
