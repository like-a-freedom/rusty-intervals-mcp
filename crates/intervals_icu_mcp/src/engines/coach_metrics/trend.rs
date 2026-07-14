use crate::domains::coach::{
    DecouplingMetrics, FitnessMetrics, TrendMetrics, VolumeMetrics, WorkoutMetricsContext,
};
use crate::engines::coach_metrics_constants::*;
use intervals_icu_client::ActivitySummary;
use serde_json::Value;
use std::collections::HashMap;

use super::helpers::percent_delta;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrendSnapshot {
    pub activity_count: usize,
    pub total_time_secs: i64,
    pub total_distance_m: f64,
    pub total_elevation_m: f64,
}

#[must_use]
pub fn build_trend_snapshot(
    activities: &[&ActivitySummary],
    details: &HashMap<String, Value>,
) -> TrendSnapshot {
    activities.iter().fold(
        TrendSnapshot {
            activity_count: activities.len(),
            total_time_secs: 0,
            total_distance_m: 0.0,
            total_elevation_m: 0.0,
        },
        |mut acc, activity| {
            if let Some(detail) = details.get(&activity.id).and_then(Value::as_object) {
                acc.total_time_secs += detail
                    .get("moving_time")
                    .and_then(Value::as_i64)
                    .unwrap_or(0);
                acc.total_distance_m += detail
                    .get("distance")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                acc.total_elevation_m += detail
                    .get("total_elevation_gain")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
            }
            acc
        },
    )
}

#[must_use]
pub fn derive_volume_metrics(
    window_days: i64,
    total_moving_time_secs: i64,
    total_distance_m: f64,
    total_elevation_gain_m: f64,
    activity_count: usize,
) -> VolumeMetrics {
    let weeks = (window_days as f64 / DAYS_PER_WEEK).max(WEEKS_FLOOR_MIN);
    let total_moving_time_hours = total_moving_time_secs as f64 / SECONDS_PER_HOUR;

    VolumeMetrics {
        activity_count,
        total_moving_time_secs,
        total_distance_m,
        total_elevation_gain_m,
        weekly_avg_hours: total_moving_time_hours / weeks,
        avg_activity_duration_secs: if activity_count > 0 {
            total_moving_time_secs as f64 / activity_count as f64
        } else {
            0.0
        },
        activities_per_week: activity_count as f64 / weeks,
    }
}

#[must_use]
pub fn interpret_fitness_metrics(
    ctl: Option<f64>,
    atl: Option<f64>,
    tsb: Option<f64>,
    ramp_rate: Option<f64>,
) -> FitnessMetrics {
    let load_state = tsb.map(|value| {
        if value > TSB_BALANCED_UPPER {
            "fresh".to_string()
        } else if value < TSB_LOAD_PRESSURE_THRESHOLD {
            "fatigued".to_string()
        } else {
            "balanced".to_string()
        }
    });

    FitnessMetrics {
        ctl,
        atl,
        tsb,
        load_state,
        ramp_rate,
    }
}
pub fn derive_trend_metrics(current: TrendSnapshot, previous: TrendSnapshot) -> TrendMetrics {
    TrendMetrics {
        activity_count_delta: Some(current.activity_count as i64 - previous.activity_count as i64),
        time_delta_pct: percent_delta(
            previous.total_time_secs as f64,
            current.total_time_secs as f64,
        ),
        distance_delta_pct: percent_delta(previous.total_distance_m, current.total_distance_m),
        elevation_delta_pct: percent_delta(previous.total_elevation_m, current.total_elevation_m),
    }
}

pub fn derive_workout_metrics_context(
    interval_count: Option<usize>,
    avg_hr: Option<f64>,
    avg_power: Option<f64>,
    efficiency_factor: Option<f64>,
    aerobic_decoupling: Option<DecouplingMetrics>,
    execution_notes: Vec<String>,
) -> WorkoutMetricsContext {
    WorkoutMetricsContext {
        interval_count,
        avg_hr,
        avg_power,
        efficiency_factor,
        aerobic_decoupling,
        execution_notes,
    }
}
