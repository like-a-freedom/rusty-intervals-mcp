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
