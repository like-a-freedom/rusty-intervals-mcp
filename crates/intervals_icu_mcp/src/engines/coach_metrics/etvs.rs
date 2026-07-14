use crate::domains::coach::EtvsMetrics;
use crate::engines::shared::aggregate_five_zone_seconds;
use intervals_icu_client::ActivitySummary;
use serde_json::Value;
use std::collections::HashMap;

const ETVS_ZONE_WEIGHTS: [f64; 5] = [1.0, 2.0, 3.0, 4.0, 5.0];
const ETVS_MODEL: &str = "intervals_icu_zones_linear_1_5_cap_v1";
const SECONDS_PER_MINUTE: f64 = 60.0;

#[must_use]
pub fn compute_etvs(
    zone_times: Option<&Value>,
    total_moving_seconds: Option<f64>,
) -> Option<EtvsMetrics> {
    let zone_seconds = aggregate_five_zone_seconds(zone_times?)?;
    let zone_minutes = zone_seconds.map(|seconds| seconds / SECONDS_PER_MINUTE);
    let score_weighted_minutes = zone_minutes
        .iter()
        .zip(ETVS_ZONE_WEIGHTS)
        .map(|(minutes, weight)| minutes * weight)
        .sum();
    let covered_seconds = zone_seconds.iter().sum::<f64>();
    let coverage_ratio = total_moving_seconds
        .filter(|seconds| *seconds > 0.0)
        .map(|seconds| covered_seconds / seconds);

    Some(EtvsMetrics {
        score_weighted_minutes,
        zone_minutes,
        coverage_ratio,
        activities_with_zone_data: 1,
        activities_total: 1,
        model: ETVS_MODEL.into(),
    })
}

#[must_use]
pub fn aggregate_period_etvs(
    activities: &[&ActivitySummary],
    details: &HashMap<String, Value>,
) -> Option<EtvsMetrics> {
    let total_moving_seconds = activities
        .iter()
        .filter_map(|activity| {
            activity.moving_time.map(f64::from).or_else(|| {
                details
                    .get(&activity.id)
                    .and_then(|detail| detail.get("moving_time"))
                    .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
            })
        })
        .filter(|seconds| *seconds > 0.0)
        .sum::<f64>();
    let mut zone_seconds = [0.0_f64; 5];
    let mut activities_with_zone_data = 0;

    for activity in activities {
        let Some(activity_zones) = details
            .get(&activity.id)
            .and_then(|detail| detail.get("icu_zone_times"))
            .and_then(aggregate_five_zone_seconds)
        else {
            continue;
        };
        activities_with_zone_data += 1;
        for (total, seconds) in zone_seconds.iter_mut().zip(activity_zones) {
            *total += seconds;
        }
    }

    if activities_with_zone_data == 0 {
        return None;
    }

    let zone_minutes = zone_seconds.map(|seconds| seconds / SECONDS_PER_MINUTE);
    let score_weighted_minutes = zone_minutes
        .iter()
        .zip(ETVS_ZONE_WEIGHTS)
        .map(|(minutes, weight)| minutes * weight)
        .sum();
    let covered_seconds = zone_seconds.iter().sum::<f64>();

    Some(EtvsMetrics {
        score_weighted_minutes,
        zone_minutes,
        coverage_ratio: (total_moving_seconds > 0.0)
            .then_some(covered_seconds / total_moving_seconds),
        activities_with_zone_data,
        activities_total: activities.len(),
        model: ETVS_MODEL.into(),
    })
}
