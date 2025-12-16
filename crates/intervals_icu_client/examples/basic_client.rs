use intervals_icu_client::{IntervalsClient, config::Config, http_client::ReqwestIntervalsClient};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Example: expects INTERVALS_ICU_API_KEY in env
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("config error: {}", e);
            return Ok(());
        }
    };
    let client = ReqwestIntervalsClient::new(&cfg.base_url, cfg.athlete_id.clone(), cfg.api_key);
    let profile = client.get_athlete_profile().await?;
    println!(
        "Athlete: {} ({})",
        profile.name.unwrap_or_default(),
        profile.id
    );
    Ok(())
}
