use intervals_icu_client::{IntervalsClient, config::Config, http_client::ReqwestIntervalsClient};
use std::path::PathBuf;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::from_env()?;

    let activity_id = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("INTERVALS_ICU_ACTIVITY_ID").ok());

    let Some(activity_id) = activity_id else {
        eprintln!(
            "usage: cargo run -p intervals_icu_client --example download_activity_file -- <activity_id>"
        );
        eprintln!("or set INTERVALS_ICU_ACTIVITY_ID");
        return Ok(());
    };

    let output_path = std::env::var("INTERVALS_ICU_OUTPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("activity.bin"));

    let client = ReqwestIntervalsClient::new(&cfg.base_url, cfg.athlete_id.clone(), cfg.api_key);

    // Stream to disk to keep memory usage predictable
    client
        .download_activity_file(&activity_id, Some(output_path.clone()))
        .await
        .map_err(|e| format!("download failed: {}", e))?;

    println!("Saved activity {activity_id} to {}", output_path.display());
    Ok(())
}
