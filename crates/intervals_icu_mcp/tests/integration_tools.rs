//! Integration tests for all MCP tools using real Intervals.icu API.
//!
//! These tests require valid API credentials set via environment variables:
//! - `INTERVALS_ICU_BASE_URL` (default: https://intervals.icu)
//! - `INTERVALS_ICU_ATHLETE_ID`
//! - `INTERVALS_ICU_API_KEY`
//!
//! Run with: `cargo test --test integration_tools -- --ignored`
//! Or set env vars and run: `cargo test --test integration_tools`

use intervals_icu_client::IntervalsClient;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use std::env;
use std::sync::Arc;

/// Get test configuration from environment
fn test_config() -> TestConfig {
    let base_url =
        env::var("INTERVALS_ICU_BASE_URL").unwrap_or_else(|_| "https://intervals.icu".to_string());
    let athlete_id =
        env::var("INTERVALS_ICU_ATHLETE_ID").expect("INTERVALS_ICU_ATHLETE_ID must be set");
    let api_key = env::var("INTERVALS_ICU_API_KEY").expect("INTERVALS_ICU_API_KEY must be set");

    TestConfig {
        base_url,
        athlete_id,
        api_key,
    }
}

struct TestConfig {
    base_url: String,
    athlete_id: String,
    api_key: String,
}

/// Create a test client from environment config
fn create_client() -> Arc<ReqwestIntervalsClient> {
    let config = test_config();
    Arc::new(ReqwestIntervalsClient::new(
        &config.base_url,
        &config.athlete_id,
        SecretString::new(config.api_key.into()),
    ))
}

// ============================================================================
// Athlete Profile Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_athlete_profile() {
    let client = create_client();
    let result = client.get_athlete_profile().await;

    assert!(
        result.is_ok(),
        "get_athlete_profile failed: {:?}",
        result.err()
    );
    let profile = result.unwrap();

    assert!(!profile.id.is_empty(), "athlete ID should not be empty");
    println!(
        "Athlete profile: id={}, name={:?}",
        profile.id, profile.name
    );
}

// ============================================================================
// Recent Activities Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_recent_activities() {
    let client = create_client();
    let result = client.get_recent_activities(Some(5), None).await;

    assert!(
        result.is_ok(),
        "get_recent_activities failed: {:?}",
        result.err()
    );
    let activities = result.unwrap();

    println!("Retrieved {} activities", activities.len());
    for activity in &activities {
        println!(
            "  - {}: {}",
            activity.id,
            activity.name.as_deref().unwrap_or("unnamed")
        );
    }
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_recent_activities_with_days_back() {
    let client = create_client();
    let result = client.get_recent_activities(Some(10), Some(7)).await;

    assert!(
        result.is_ok(),
        "get_recent_activities with days_back failed: {:?}",
        result.err()
    );
    let activities = result.unwrap();

    println!("Retrieved {} activities from last 7 days", activities.len());
}

// ============================================================================
// Fitness Summary Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_fitness_summary() {
    let client = create_client();
    let result = client.get_fitness_summary().await;

    assert!(
        result.is_ok(),
        "get_fitness_summary failed: {:?}",
        result.err()
    );
    let summary = result.unwrap();

    println!("Fitness summary: {}", summary);

    // Should be an array with fitness metrics
    assert!(summary.is_array(), "Fitness summary should be an array");
    let arr = summary.as_array().unwrap();
    if !arr.is_empty() {
        let first = &arr[0];
        assert!(
            first.get("fitness").is_some()
                || first.get("fatigue").is_some()
                || first.get("form").is_some(),
            "Fitness summary should contain at least one of: fitness, fatigue, form"
        );
    }
}

// ============================================================================
// Wellness Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_wellness() {
    let client = create_client();
    let result = client.get_wellness(Some(7)).await;

    assert!(result.is_ok(), "get_wellness failed: {:?}", result.err());
    let wellness = result.unwrap();

    println!("Wellness data: {}", wellness);
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_wellness_for_date() {
    let client = create_client();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let result = client.get_wellness_for_date(&today).await;

    assert!(
        result.is_ok(),
        "get_wellness_for_date failed: {:?}",
        result.err()
    );
    let wellness = result.unwrap();

    println!("Wellness for {}: {}", today, wellness);
}

// ============================================================================
// Events Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_events() {
    let client = create_client();
    let result = client.get_events(Some(30), Some(20)).await;

    assert!(result.is_ok(), "get_events failed: {:?}", result.err());
    let events = result.unwrap();

    println!("Retrieved {} events", events.len());
    for event in &events {
        let id = event.id.as_deref().unwrap_or("unknown");
        println!("  - {}: {}", id, event.name);
    }
}

// ============================================================================
// Activity Details Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_activity_details() {
    let client = create_client();

    // First get a recent activity ID
    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result {
        if !acts.is_empty() {
            let activity_id = &acts[0].id;

            let result = client.get_activity_details(activity_id).await;

            assert!(
                result.is_ok(),
                "get_activity_details failed: {:?}",
                result.err()
            );
            let details = result.unwrap();

            println!("Activity {} details: {}", activity_id, details);
        } else {
            println!("No activities found to test get_activity_details");
        }
    }
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_activity_streams() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let result = client
            .get_activity_streams(
                activity_id,
                Some(vec!["time".to_string(), "distance".to_string()]),
            )
            .await;

        assert!(
            result.is_ok(),
            "get_activity_streams failed: {:?}",
            result.err()
        );
        let streams = result.unwrap();

        println!("Activity {} streams: {}", activity_id, streams);
    }
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_activity_intervals() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let result = client.get_activity_intervals(activity_id).await;

        assert!(
            result.is_ok(),
            "get_activity_intervals failed: {:?}",
            result.err()
        );
        let intervals = result.unwrap();

        println!("Activity {} intervals: {}", activity_id, intervals);
    }
}

// ============================================================================
// Power Curves Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_power_curves() {
    let client = create_client();
    let result = client.get_power_curves(Some(90), "Ride").await;

    assert!(
        result.is_ok(),
        "get_power_curves failed: {:?}",
        result.err()
    );
    let curves = result.unwrap();

    println!("Power curves: {}", curves);
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_hr_curves() {
    let client = create_client();
    let result = client.get_hr_curves(Some(90), "Run").await;

    assert!(result.is_ok(), "get_hr_curves failed: {:?}", result.err());
    let curves = result.unwrap();

    println!("HR curves for Run: {}", curves);
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_pace_curves() {
    let client = create_client();
    let result = client.get_pace_curves(Some(90), "Run").await;

    assert!(result.is_ok(), "get_pace_curves failed: {:?}", result.err());
    let curves = result.unwrap();

    println!("Pace curves for Run: {}", curves);
}

// ============================================================================
// Gear List Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_gear_list() {
    let client = create_client();
    let result = client.get_gear_list().await;

    assert!(result.is_ok(), "get_gear_list failed: {:?}", result.err());
    let gear = result.unwrap();

    println!("Gear list: {}", gear);
}

// ============================================================================
// Sport Settings Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_sport_settings() {
    let client = create_client();
    let result = client.get_sport_settings().await;

    assert!(
        result.is_ok(),
        "get_sport_settings failed: {:?}",
        result.err()
    );
    let settings = result.unwrap();

    println!("Sport settings: {}", settings);
}

// ============================================================================
// Upcoming Workouts Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_upcoming_workouts() {
    let client = create_client();
    let result = client.get_upcoming_workouts(Some(14)).await;

    assert!(
        result.is_ok(),
        "get_upcoming_workouts failed: {:?}",
        result.err()
    );
    let workouts = result.unwrap();

    println!("Upcoming workouts: {}", workouts);
}

// ============================================================================
// Search Activities Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_search_activities() {
    let client = create_client();
    let result = client.search_activities("run", Some(5)).await;

    assert!(
        result.is_ok(),
        "search_activities failed: {:?}",
        result.err()
    );
    let activities = result.unwrap();

    println!("Search results for 'run': {} activities", activities.len());
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_search_activities_full() {
    let client = create_client();
    let result = client.search_activities_full("run", Some(5)).await;

    assert!(
        result.is_ok(),
        "search_activities_full failed: {:?}",
        result.err()
    );
    let activities = result.unwrap();

    println!("Search activities full: {}", activities);
}

// ============================================================================
// Activities Around Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_activities_around() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let result = client
            .get_activities_around(activity_id, Some(5), None)
            .await;

        assert!(
            result.is_ok(),
            "get_activities_around failed: {:?}",
            result.err()
        );
        let around = result.unwrap();

        println!("Activities around {}: {}", activity_id, around);
    }
}

// ============================================================================
// Best Efforts Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_best_efforts() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let options = intervals_icu_client::BestEffortsOptions {
            stream: Some("power".to_string()),
            duration: Some(300),
            distance: None,
            count: None,
            min_value: None,
            exclude_intervals: None,
            start_index: None,
            end_index: None,
        };

        let result = client.get_best_efforts(activity_id, Some(options)).await;

        assert!(
            result.is_ok(),
            "get_best_efforts failed: {:?}",
            result.err()
        );
        let efforts = result.unwrap();

        println!("Best efforts for {}: {}", activity_id, efforts);
    }
}

// ============================================================================
// Workout Library Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_workout_library() {
    let client = create_client();
    let result = client.get_workout_library().await;

    assert!(
        result.is_ok(),
        "get_workout_library failed: {:?}",
        result.err()
    );
    let library = result.unwrap();

    println!("Workout library: {}", library);
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_workouts_in_folder() {
    let client = create_client();

    // Try with a sample folder ID (may not exist for all users)
    let result = client.get_workouts_in_folder("default").await;

    // This might fail if folder doesn't exist, which is OK
    match result {
        Ok(workouts) => println!("Workouts in folder: {}", workouts),
        Err(e) => println!("Folder not found or error (expected): {}", e),
    }
}

// ============================================================================
// Histogram Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_gap_histogram() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let result = client.get_gap_histogram(activity_id).await;

        assert!(
            result.is_ok(),
            "get_gap_histogram failed: {:?}",
            result.err()
        );
        let histogram = result.unwrap();

        println!("Activity {} GAP histogram: {}", activity_id, histogram);
    }
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_power_histogram() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let result = client.get_power_histogram(activity_id).await;

        assert!(
            result.is_ok(),
            "get_power_histogram failed: {:?}",
            result.err()
        );
        let histogram = result.unwrap();

        println!("Activity {} power histogram: {}", activity_id, histogram);
    }
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_hr_histogram() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let result = client.get_hr_histogram(activity_id).await;

        assert!(
            result.is_ok(),
            "get_hr_histogram failed: {:?}",
            result.err()
        );
        let histogram = result.unwrap();

        println!("Activity {} HR histogram: {}", activity_id, histogram);
    }
}

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_pace_histogram() {
    let client = create_client();

    let acts_result = client.get_recent_activities(Some(1), Some(90)).await;

    if let Ok(acts) = acts_result
        && !acts.is_empty()
    {
        let activity_id = &acts[0].id;

        let result = client.get_pace_histogram(activity_id).await;

        assert!(
            result.is_ok(),
            "get_pace_histogram failed: {:?}",
            result.err()
        );
        let histogram = result.unwrap();

        println!("Activity {} pace histogram: {}", activity_id, histogram);
    }
}

// ============================================================================
// Search Intervals Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_search_intervals() {
    let client = create_client();
    let result = client
        .search_intervals(60, 600, 1, 5, None, None, None, Some(10))
        .await;

    assert!(
        result.is_ok(),
        "search_intervals failed: {:?}",
        result.err()
    );
    let intervals = result.unwrap();

    println!("Search intervals: {}", intervals);
}

// ============================================================================
// Activities CSV Tests
// ============================================================================

#[tokio::test]
#[ignore = "requires real API credentials"]
async fn integration_get_activities_csv() {
    let client = create_client();
    let result = client.get_activities_csv().await;

    assert!(
        result.is_ok(),
        "get_activities_csv failed: {:?}",
        result.err()
    );
    let csv = result.unwrap();

    println!(
        "Activities CSV (first 200 chars): {}",
        &csv[..csv.len().min(200)]
    );
}
