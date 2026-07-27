//! Compile-time API surface contract.
//!
//! This file acts as a static guard: if a method disappears from
//! `ReqwestIntervalsClient`'s `IntervalsClient` impl, this file fails to
//! compile. That makes accidental method loss during refactoring (e.g., the
//! `http_client.rs` split into `http_client/{activities,events,...}.rs`)
//! impossible to miss in CI.
//!
//! Each test constructs a `ReqwestIntervalsClient` and invokes a single
//! trait method through the dynamic `&dyn IntervalsClient`. Because trait
//! dispatch erases static method identities, this would not actually catch
//! a missing method on its own — instead the test module statically calls
//! each method via concrete `ReqwestIntervalsClient` reference, so the
//! compiler verifies the impl block is complete.

#![allow(dead_code, unused_imports)]

use base64::engine::general_purpose::STANDARD as BASE64;
use chrono::{Duration as ChronoDuration, Utc};
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use intervals_icu_client::{
    BestEffortsOptions, IntervalsClient,
    domains::workout::{Folder, SportSettings, WorkoutItem},
};
use secrecy::SecretString;

/// Build a client targeting an unreachable URL — no network call ever happens
/// in these tests because the assertions only need the method surface, not
/// the response.
fn make_client() -> ReqwestIntervalsClient {
    ReqwestIntervalsClient::new("http://127.0.0.1:1", "ath", SecretString::new("k".into()))
        .expect("client construction must not fail in tests")
}

#[tokio::test]
async fn api_surface_smoke_does_not_block() {
    // Sanity check: the smoke test only confirms the client is constructable.
    // The real value of this file is the static dispatch below.
    let _client = make_client();
}

#[tokio::test]
async fn api_surface_invoke_athlete_profile() {
    let client = make_client();
    // We invoke via the concrete type — this is a compile-time assertion
    // that the method exists on `ReqwestIntervalsClient`. Network is unreachable,
    // so we expect a transport error; we don't care about its content.
    let _ = client.get_athlete_profile().await;
}

#[tokio::test]
async fn api_surface_invoke_recent_activities() {
    let client = make_client();
    let _ = client.get_recent_activities(Some(5), Some(7)).await;
}

#[tokio::test]
async fn api_surface_invoke_activity_methods() {
    let client = make_client();
    let _ = client.get_activity_details("a1").await;
    let _ = client.get_activity_messages("a1").await;
    let _ = client
        .get_activity_streams("a1", Some(vec!["watts".into()]))
        .await;
    let _ = client.get_activity_intervals("a1").await;
    let _ = client
        .get_best_efforts(
            "a1",
            Some(BestEffortsOptions {
                stream: None,
                duration: None,
                distance: None,
                count: None,
                min_value: None,
                exclude_intervals: None,
                start_index: None,
                end_index: None,
            }),
        )
        .await;
    let _ = client.search_activities("ride", Some(10)).await;
    let _ = client.search_activities_full("ride", Some(10)).await;
    let _ = client.get_activities_csv().await;
    let _ = client.update_activity("a1", &serde_json::json!({})).await;
    let _ = client.delete_activity("a1").await;
    let _ = client.get_activities_around("a1", Some(5), None).await;
    let _ = client.get_gap_histogram("a1").await;
    let _ = client
        .search_intervals(60, 600, 50, 200, None, None, None, Some(20))
        .await;
    let _ = client.get_power_histogram("a1").await;
    let _ = client.get_hr_histogram("a1").await;
    let _ = client.get_pace_histogram("a1").await;
    let _ = client.get_fitness_summary().await;
}

#[tokio::test]
async fn api_surface_invoke_events() {
    let client = make_client();
    let event = intervals_icu_client::Event {
        id: None,
        start_date_local: "2026-03-16".into(),
        name: "t".into(),
        category: intervals_icu_client::EventCategory::Workout,
        description: None,
        r#type: None,
    };
    let _ = client.create_event(event).await;
    let _ = client.get_event("e1").await;
    let _ = client.delete_event("e1").await;
    let _ = client.get_events(Some(7), Some(20)).await;
    let _ = client.bulk_create_events(vec![]).await;
    let _ = client.get_upcoming_workouts(Some(7), Some(10), None).await;
    let _ = client.update_event("e1", &serde_json::json!({})).await;
    let _ = client
        .bulk_delete_events(vec!["1".into(), "2".into()])
        .await;
    let _ = client.duplicate_event("e1", Some(2), Some(1)).await;
}

#[tokio::test]
async fn api_surface_invoke_downloads() {
    let client = make_client();
    let _ = client.download_activity_file("a1", None).await;
    let (tx, _rx) = tokio::sync::mpsc::channel(8);
    let (_cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let _ = client
        .download_activity_file_with_progress("a1", None, tx, cancel_rx)
        .await;
    let _ = client.download_fit_file("a1", None).await;
    let _ = client.download_gpx_file("a1", None).await;
}

#[tokio::test]
async fn api_surface_invoke_curves() {
    let client = make_client();
    let _ = client.get_power_curves(Some(90), "Run").await;
    let _ = client.get_hr_curves(Some(90), "Run").await;
    let _ = client.get_pace_curves(Some(90), "Run").await;
}

#[tokio::test]
async fn api_surface_invoke_wellness() {
    let client = make_client();
    let _ = client.get_wellness(Some(7)).await;
    let _ = client.get_wellness_for_date("2026-03-16").await;
    let _ = client
        .update_wellness("2026-03-16", &serde_json::json!({}))
        .await;
    let _ = client.update_wellness_bulk(&[]).await;
}

#[tokio::test]
async fn api_surface_invoke_gear() {
    let client = make_client();
    let _ = client.get_gear_list().await;
    let _ = client.create_gear(&serde_json::json!({})).await;
    let _ = client.update_gear("g1", &serde_json::json!({})).await;
    let _ = client.delete_gear("g1").await;
    let _ = client
        .create_gear_reminder("g1", &serde_json::json!({}))
        .await;
    let _ = client
        .update_gear_reminder("g1", "r1", false, 7, &serde_json::json!({}))
        .await;
}

#[tokio::test]
async fn api_surface_invoke_workouts_and_sport() {
    let client = make_client();
    let _ = client.get_workout_library().await;
    let _ = client.get_workouts_in_folder("f1").await;
    let _ = client
        .create_folder(&serde_json::json!({"name": "f"}))
        .await;
    let _ = client.update_folder("f1", &serde_json::json!({})).await;
    let _ = client.delete_folder("f1").await;
    let _ = client.get_sport_settings().await;
    let _ = client
        .update_sport_settings("Run", false, &serde_json::json!({}))
        .await;
    let _ = client.apply_sport_settings("Run").await;
    let _ = client.create_sport_settings(&serde_json::json!({})).await;
    let _ = client.delete_sport_settings("Run").await;
}

#[tokio::test]
async fn api_surface_invoke_weather_routes_yagni() {
    // Per ADR-0005: these are YAGNI candidates. The trait defaults return
    // `ConfigError::Unsupported { method }`. This test verifies the methods
    // exist on the public surface so future promotion is mechanical.
    let client = make_client();
    let _ = client.get_weather_config().await;
    let _ = client.update_weather_config(&serde_json::json!({})).await;
    let _ = client.list_routes().await;
    let _ = client.get_route(1, false).await;
    let _ = client.update_route(1, &serde_json::json!({})).await;
    let _ = client.get_route_similarity(1, 2).await;
}
