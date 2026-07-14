//! Endurance evidence fetch — collects historical ride candidates for
//! endurance evidence computation. Extracted from `analysis_fetch.rs`
//! to separate I/O orchestration from domain-specific fetch logic.

use intervals_icu_client::{ActivitySummary, IntervalsClient};
use serde_json::Value;

use crate::engines::shared::parse_activity_date;

use super::analysis_fetch::{
    FetchedAnalysisData, PeriodFetchRequest, normalize_streams_payload, value_is_empty,
};

/// How many calendar days back to scan when collecting historical ride
/// candidates for endurance evidence. Reference cohort spans 15–90 days;
/// recent cohort spans 0–14 days. The constant is the maximum backstop.
pub const ENDURANCE_EVIDENCE_LOOKBACK_DAYS: i32 = 90;

/// Cap on per-cohort candidate count to bound downstream work.
pub const ENDURANCE_EVIDENCE_MAX_RECENT_CANDIDATES: usize = 12;
pub const ENDURANCE_EVIDENCE_MAX_REFERENCE_CANDIDATES: usize = 12;

/// Minimum moving time (seconds) for an activity detail to be considered
/// for endurance evidence. Shorter rides have no chance of containing
/// eligible 600s control windows.
pub const ENDURANCE_EVIDENCE_MIN_MOVING_TIME_S: i64 = 1800;

/// Collect historical ride candidates for endurance evidence computation.
///
/// Fetches activities from the last 90 days, splits them into recent
/// (0–14 days) and reference (15–90 days) cohorts, fetches details and
/// streams for qualifying rides, and stores the results in `fetched`.
pub async fn collect_endurance_evidence(
    client: &dyn IntervalsClient,
    request: &PeriodFetchRequest,
    fetched: &mut FetchedAnalysisData,
) {
    let requested = match client
        .get_recent_activities(None, Some(ENDURANCE_EVIDENCE_LOOKBACK_DAYS))
        .await
    {
        Ok(value) => value,
        Err(_) => {
            fetched
                .fetch_warnings
                .push("endurance evidence partial: failed to list historical activities".into());
            return;
        }
    };

    // Sort by parsed date descending, ties broken by activity_id so the
    // selection is deterministic for callers with multiple rides on the
    // same date.
    let mut sorted = requested;
    sorted.sort_by(|left, right| {
        let date_left = parse_activity_date(&left.start_date_local);
        let date_right = parse_activity_date(&right.start_date_local);
        date_right
            .cmp(&date_left)
            .then_with(|| right.id.cmp(&left.id))
    });

    let mut recent: Vec<ActivitySummary> = Vec::new();
    let mut reference: Vec<ActivitySummary> = Vec::new();
    for activity in sorted {
        let Some(date) = parse_activity_date(&activity.start_date_local) else {
            continue;
        };
        let age = (request.window.end_date - date).num_days();
        if (0..=14).contains(&age) && recent.len() < ENDURANCE_EVIDENCE_MAX_RECENT_CANDIDATES {
            recent.push(activity);
        } else if (15..=90).contains(&age)
            && reference.len() < ENDURANCE_EVIDENCE_MAX_REFERENCE_CANDIDATES
        {
            reference.push(activity);
        }
    }

    let mut profile_activities = recent;
    profile_activities.append(&mut reference);
    if profile_activities.is_empty() {
        return;
    }

    let mut rejected_details: Vec<String> = Vec::new();
    let mut retained_ids: Vec<String> = Vec::new();
    for activity in profile_activities.iter() {
        if fetched.activity_details.contains_key(&activity.id) {
            retained_ids.push(activity.id.clone());
            continue;
        }
        match client.get_activity_details(&activity.id).await {
            Ok(detail) => {
                let is_ride = detail
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|value| value == "Ride");
                let moving_time = detail
                    .get("moving_time")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                if is_ride && moving_time >= ENDURANCE_EVIDENCE_MIN_MOVING_TIME_S as f64 {
                    fetched.activity_details.insert(activity.id.clone(), detail);
                    retained_ids.push(activity.id.clone());
                }
            }
            Err(error) => {
                tracing::info!(
                    "Endurance evidence detail fetch failed for {}: {}",
                    activity.id,
                    error
                );
                rejected_details.push(activity.id.clone());
            }
        }
    }

    if !retained_ids.is_empty() {
        let mut rejected_streams: Vec<String> = Vec::new();
        let mut succeeded: std::collections::HashMap<String, Value> =
            std::collections::HashMap::new();

        // Sequential stream retrieval. Each `get_activity_streams` call
        // blocks until it completes before starting the next one.
        // Concurrency is not needed here because the candidate set is
        // bounded (≤ 24 rides) and each fetch is cheap; true concurrent
        // retrieval would require `&dyn IntervalsClient: Send` which is
        // not the case today.
        for id in retained_ids.iter() {
            match client.get_activity_streams(id, None).await {
                Ok(payload) => {
                    let normalized = normalize_streams_payload(payload);
                    if !value_is_empty(&normalized) {
                        succeeded.insert(id.clone(), normalized);
                    }
                }
                Err(error) => {
                    tracing::info!(
                        "Endurance evidence stream fetch failed for {}: {}",
                        id,
                        error
                    );
                    rejected_streams.push(id.clone());
                }
            }
        }

        fetched.endurance_profile_streams.extend(succeeded);

        if !rejected_details.is_empty() || !rejected_streams.is_empty() {
            fetched.fetch_warnings.push(format!(
                "endurance evidence partial: {} candidate details and {} ride streams unavailable",
                rejected_details.len(),
                rejected_streams.len()
            ));
        }
    } else if !rejected_details.is_empty() {
        fetched.fetch_warnings.push(format!(
            "endurance evidence partial: {} candidate details and 0 ride streams unavailable",
            rejected_details.len()
        ));
    }

    fetched.endurance_profile_activities = profile_activities;
}
