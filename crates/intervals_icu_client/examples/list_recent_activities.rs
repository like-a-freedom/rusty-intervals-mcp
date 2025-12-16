use intervals_icu_client::{IntervalsClient, config::Config, http_client::ReqwestIntervalsClient};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;
    let client = ReqwestIntervalsClient::new(&cfg.base_url, cfg.athlete_id.clone(), cfg.api_key);

    let limit = std::env::var("INTERVALS_ICU_LIMIT")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(5);

    let activities = client
        .get_recent_activities(Some(limit), None)
        .await
        .map_err(|e| format!("failed to fetch activities: {}", e))?;

    if activities.is_empty() {
        println!("No recent activities returned (check date range or credentials)");
        return Ok(());
    }

    println!("Recent activities (limit {}):", limit);
    for a in activities {
        let name = a.name.unwrap_or_else(|| "(no name)".to_string());
        println!("- {} — {}", a.id, name);
    }

    Ok(())
}
