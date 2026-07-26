use std::collections::BTreeMap;

use chrono::NaiveDate;
use intervals_icu_client::ActivitySummary;

use crate::domains::load::{ComparableLoadSeries, LoadObservation, LoadSource};
use crate::engines::fetch_error::FetchError;
use crate::engines::shared::parse_activity_date;

/// Minimum wellness history to fetch for personal baseline calculation.
/// Requires 60 calendar days of observations to compute the reference window.
pub const PERSONAL_BASELINE_WINDOW_DAYS: i32 = 60;

/// Extract a load observation from an activity detail payload.
///
/// Alias priority: `icu_training_load` → `training_load`/`icuTrainingLoad` → `tss`
pub fn extract_activity_load(detail: Option<&serde_json::Value>) -> Option<LoadObservation> {
    let object = detail?.as_object()?;

    let (key, source) = object
        .get("icu_training_load")
        .map(|_| ("icu_training_load", LoadSource::IcuTrainingLoad))
        .or_else(|| {
            object
                .get("training_load")
                .map(|_| ("training_load", LoadSource::TrainingLoadAlias))
        })
        .or_else(|| {
            object
                .get("icuTrainingLoad")
                .map(|_| ("icuTrainingLoad", LoadSource::TrainingLoadAlias))
        })
        .or_else(|| object.get("tss").map(|_| ("tss", LoadSource::TssAlias)))?;

    let value = object.get(key).and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_i64().map(|n| n as f64))
            .or_else(|| v.as_str().and_then(|s| s.parse::<f64>().ok()))
    })?;

    LoadObservation::new(value, source)
}

/// Return the best available load for an activity without requiring its detail
/// endpoint. Activity summaries carry `training_load` for historical queries;
/// a loaded detail takes precedence when it exposes a more specific value.
pub fn activity_load(
    activity: &ActivitySummary,
    detail: Option<&serde_json::Value>,
) -> Option<LoadObservation> {
    extract_activity_load(detail).or_else(|| {
        activity.training_load.map(|value| LoadObservation {
            value: f64::from(value),
            source: LoadSource::ActivitySummaryTrainingLoad,
        })
    })
}

pub fn build_previous_window(
    current: &crate::domains::coach::AnalysisWindow,
) -> crate::domains::coach::AnalysisWindow {
    let days = current.window_days();
    let previous_end = current.start_date - chrono::Duration::days(1);
    let previous_start = previous_end - chrono::Duration::days(days - 1);
    crate::domains::coach::AnalysisWindow::new(previous_start, previous_end)
}

pub fn required_activity_window(request: &super::PeriodFetchRequest) -> (NaiveDate, NaiveDate) {
    let start = if request.include_comparison_window {
        build_previous_window(&request.window).start_date
    } else {
        request.window.start_date
    };
    (start, request.window.end_date)
}

pub fn activity_lookback_days(start: NaiveDate, today: NaiveDate) -> Result<i32, FetchError> {
    let days = (today - start).num_days().max(0);
    i32::try_from(days).map_err(|_| {
        FetchError::InvalidDateRange("Requested activity window is too large".to_string())
    })
}

pub fn build_daily_load_series(
    activities: &[&ActivitySummary],
    details: &std::collections::HashMap<String, serde_json::Value>,
    window: &crate::domains::coach::AnalysisWindow,
) -> ComparableLoadSeries {
    let mut daily_totals = std::collections::HashMap::<NaiveDate, f64>::new();
    let mut activities_total = 0usize;
    let mut activities_with_load = 0usize;
    let mut source_counts = BTreeMap::new();

    for activity in activities {
        if let Some(activity_date) = parse_activity_date(&activity.start_date_local) {
            activities_total += 1;
            if let Some(observation) = activity_load(activity, details.get(&activity.id)) {
                activities_with_load += 1;
                *source_counts.entry(observation.source).or_insert(0) += 1;
                *daily_totals.entry(activity_date).or_insert(0.0) += observation.value;
            }
        }
    }

    let mut current = window.start_date;
    let mut daily = Vec::with_capacity(window.window_days().max(0) as usize);
    while current <= window.end_date {
        daily.push((current, daily_totals.get(&current).copied().unwrap_or(0.0)));
        current += chrono::Duration::days(1);
    }

    ComparableLoadSeries {
        daily,
        activities_total,
        activities_with_load,
        source_counts,
    }
}
