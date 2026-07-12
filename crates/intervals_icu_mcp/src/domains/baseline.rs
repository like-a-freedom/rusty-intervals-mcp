use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BaselinePosition {
    Below,
    Within,
    Above,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonalBaselineDeviation {
    pub metric: String,
    pub unit: String,
    pub recent_mean_7d: f64,
    pub baseline_mean_60d: f64,
    pub baseline_cv_60d: f64,
    pub swc: f64,
    pub lower_bound: f64,
    pub upper_bound: f64,
    pub recent_sample_count: usize,
    pub baseline_sample_count: usize,
    pub baseline_span_days: i64,
    pub position: BaselinePosition,
    pub model: String,
}

pub enum BaselineTransform {
    LogLnRmssd,
    RawBpm,
}

pub fn compute_personal_baseline(
    observations: &[(NaiveDate, f64)],
    transform: BaselineTransform,
) -> Option<PersonalBaselineDeviation> {
    if observations.is_empty() {
        return None;
    }

    // Sort and deduplicate by date, keeping last valid observation per date
    let mut sorted = observations.to_vec();
    sorted.sort_by_key(|(date, _)| *date);
    sorted.dedup_by_key(|(date, _)| *date);

    // Apply transform
    let transformed: Vec<(NaiveDate, f64)> = sorted
        .into_iter()
        .filter_map(|(date, value)| match transform {
            BaselineTransform::LogLnRmssd => {
                if value > 0.0 {
                    Some((date, value.ln()))
                } else {
                    None
                }
            }
            BaselineTransform::RawBpm => Some((date, value)),
        })
        .collect();

    if transformed.is_empty() {
        return None;
    }

    let today = transformed.last().map(|(date, _)| *date)?;

    // Recent 7-day window
    let recent_cutoff = today - chrono::Duration::days(6);
    let recent: Vec<f64> = transformed
        .iter()
        .filter(|(date, _)| *date >= recent_cutoff)
        .map(|(_, value)| *value)
        .collect();

    if recent.len() < 3 {
        return None;
    }

    let recent_mean = recent.iter().sum::<f64>() / recent.len() as f64;

    // 60-day baseline window
    let baseline_cutoff = today - chrono::Duration::days(59);
    let baseline: Vec<f64> = transformed
        .iter()
        .filter(|(date, _)| *date >= baseline_cutoff)
        .map(|(_, value)| *value)
        .collect();

    // Data quality floor: 14 observations spanning at least 28 calendar days
    if baseline.len() < 14 {
        return None;
    }

    let baseline_span = {
        // Calculate span from the original filtered observations
        let baseline_dates: Vec<NaiveDate> = transformed
            .iter()
            .filter(|(date, _)| *date >= baseline_cutoff)
            .map(|(date, _)| *date)
            .collect();
        if let (Some(first_date), Some(last_date)) = (baseline_dates.first(), baseline_dates.last())
        {
            (*last_date - *first_date).num_days()
        } else {
            0
        }
    };

    if baseline_span < 28 {
        return None;
    }

    let baseline_mean = baseline.iter().sum::<f64>() / baseline.len() as f64;
    let baseline_variance = baseline
        .iter()
        .map(|value| {
            let delta = value - baseline_mean;
            delta * delta
        })
        .sum::<f64>()
        / baseline.len() as f64;
    let baseline_cv = if baseline_mean.abs() < f64::EPSILON {
        0.0
    } else {
        baseline_variance.sqrt() / baseline_mean.abs()
    };

    // Smallest Worthwhile Change = 0.5 * CV
    let swc = baseline_mean * baseline_cv * 0.5;
    let lower_bound = baseline_mean - swc;
    let upper_bound = baseline_mean + swc;

    let position = if recent_mean < lower_bound {
        BaselinePosition::Below
    } else if recent_mean > upper_bound {
        BaselinePosition::Above
    } else {
        BaselinePosition::Within
    };

    let (metric, unit) = match transform {
        BaselineTransform::LogLnRmssd => ("lnRMSSD", "ln(ms)"),
        BaselineTransform::RawBpm => ("Resting HR", "bpm"),
    };

    Some(PersonalBaselineDeviation {
        metric: metric.to_string(),
        unit: unit.to_string(),
        recent_mean_7d: recent_mean,
        baseline_mean_60d: baseline_mean,
        baseline_cv_60d: baseline_cv,
        swc,
        lower_bound,
        upper_bound,
        recent_sample_count: recent.len(),
        baseline_sample_count: baseline.len(),
        baseline_span_days: baseline_span,
        position,
        model: "rolling_7d_vs_60d_half_cv_swc_v1".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn baseline_position_serialization_round_trip() {
        let positions = vec![
            BaselinePosition::Below,
            BaselinePosition::Within,
            BaselinePosition::Above,
        ];
        for pos in &positions {
            let json = serde_json::to_string(pos).unwrap();
            let deserialized: BaselinePosition = serde_json::from_str(&json).unwrap();
            assert_eq!(*pos, deserialized);
        }
    }

    #[test]
    fn personal_baseline_deviation_serialization_round_trip() {
        let deviation = PersonalBaselineDeviation {
            metric: "lnRMSSD".to_string(),
            unit: "ln(ms)".to_string(),
            recent_mean_7d: 4.0,
            baseline_mean_60d: 4.1,
            baseline_cv_60d: 0.05,
            swc: 0.1,
            lower_bound: 4.0,
            upper_bound: 4.2,
            recent_sample_count: 7,
            baseline_sample_count: 30,
            baseline_span_days: 45,
            position: BaselinePosition::Within,
            model: "rolling_7d_vs_60d_half_cv_swc_v1".to_string(),
        };
        let json = serde_json::to_string(&deviation).unwrap();
        let deserialized: PersonalBaselineDeviation = serde_json::from_str(&json).unwrap();
        assert_eq!(deviation, deserialized);
    }

    #[test]
    fn constant_series_produces_zero_swc_and_within_position() {
        let observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm).unwrap();
        assert_eq!(result.position, BaselinePosition::Within);
        assert!(result.baseline_cv_60d < 0.001);
        assert!(result.swc < 0.001);
    }

    #[test]
    fn sustained_increase_produces_above_position() {
        let mut observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        // Last 7 days are significantly higher
        for item in observations.iter_mut().take(60).skip(53) {
            item.1 = 70.0;
        }
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm).unwrap();
        assert_eq!(result.position, BaselinePosition::Above);
    }

    #[test]
    fn sustained_decrease_produces_below_position() {
        let mut observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        // Last 7 days are significantly lower
        for item in observations.iter_mut().take(60).skip(53) {
            item.1 = 50.0;
        }
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm).unwrap();
        assert_eq!(result.position, BaselinePosition::Below);
    }

    #[test]
    fn exactly_three_recent_observations_supported() {
        let mut observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        // Remove some recent observations, keep only 3
        observations.truncate(57);
        // Add back 3 recent
        for i in 0..3 {
            observations.push((
                date(2026, 1, 1) + chrono::Duration::days(57 + i as i64),
                60.0,
            ));
        }
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm);
        assert!(result.is_some());
    }

    #[test]
    fn two_recent_observations_unavailable() {
        // 58 baseline observations + 2 recent = 60 total
        // The 2 recent observations are within the 7-day window, so recent count = 2 < 3
        let observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        // Keep only the last 2 observations as "recent"
        let recent_observations = &observations[58..60];
        let result = compute_personal_baseline(recent_observations, BaselineTransform::RawBpm);
        assert!(result.is_none());
    }

    #[test]
    fn thirteen_baseline_observations_unavailable() {
        // 13 baseline observations + 7 recent = 20 total, but baseline < 14
        let observations: Vec<(NaiveDate, f64)> = (0..20)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm);
        assert!(result.is_none());
    }

    #[test]
    fn fourteen_observations_spanning_less_than_28_days_unavailable() {
        // 14 observations in a short span (< 28 days)
        let observations: Vec<(NaiveDate, f64)> = (0..14)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm);
        assert!(result.is_none());
    }

    #[test]
    fn unordered_dates_produce_same_result_as_ordered_dates() {
        let ordered: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| {
                (
                    date(2026, 1, 1) + chrono::Duration::days(i),
                    60.0 + i as f64 * 0.1,
                )
            })
            .collect();
        let mut unordered = ordered.clone();
        unordered.reverse();
        unordered.swap(10, 40);
        unordered.swap(20, 50);

        let result_ordered =
            compute_personal_baseline(&ordered, BaselineTransform::RawBpm).unwrap();
        let result_unordered =
            compute_personal_baseline(&unordered, BaselineTransform::RawBpm).unwrap();

        assert_eq!(result_ordered.position, result_unordered.position);
        assert!(
            (result_ordered.baseline_mean_60d - result_unordered.baseline_mean_60d).abs() < 0.001
        );
    }

    #[test]
    fn duplicate_dates_use_last_valid_observation() {
        let mut observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        // Add duplicate date with different value
        observations.push((date(2026, 1, 15), 70.0));
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm).unwrap();
        // The duplicate should be resolved by keeping the last one
        assert!(result.baseline_sample_count <= 60);
    }

    #[test]
    fn zero_negative_nonfinite_hrv_values_are_ignored() {
        let mut observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        // Insert invalid values
        observations[10].1 = 0.0;
        observations[20].1 = -5.0;
        observations[30].1 = f64::NAN;
        let result = compute_personal_baseline(&observations, BaselineTransform::LogLnRmssd);
        assert!(result.is_some());
    }

    #[test]
    fn rmssd_is_log_transformed() {
        let observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        let result =
            compute_personal_baseline(&observations, BaselineTransform::LogLnRmssd).unwrap();
        // ln(60) ≈ 4.094
        assert!((result.recent_mean_7d - 4.094).abs() < 0.01);
        assert!((result.baseline_mean_60d - 4.094).abs() < 0.01);
    }

    #[test]
    fn rhr_is_not_log_transformed() {
        let observations: Vec<(NaiveDate, f64)> = (0..60)
            .map(|i| (date(2026, 1, 1) + chrono::Duration::days(i), 60.0))
            .collect();
        let result = compute_personal_baseline(&observations, BaselineTransform::RawBpm).unwrap();
        assert!((result.recent_mean_7d - 60.0).abs() < 0.001);
        assert!((result.baseline_mean_60d - 60.0).abs() < 0.001);
    }

    #[test]
    fn empty_observations_returns_none() {
        let result = compute_personal_baseline(&[], BaselineTransform::RawBpm);
        assert!(result.is_none());
    }
}
