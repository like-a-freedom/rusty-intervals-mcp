//! Trailing CTL plateau detection.

use crate::domains::progress::{ChangepointResult, TrendState};

const PLATEAU_WINDOW_DAYS: usize = 28;
const BACKWARD_STEP_DAYS: usize = 7;
const CHECK_WINDOW_DAYS: usize = 14;
const DEFAULT_FLAT_SLOPE_PER_WEEK_THRESHOLD: f64 = 0.5;
const MIN_CTL_HISTORY_FOR_PERSONALIZATION: usize = 56;
const DAYS_PER_WEEK: usize = 7;
const MIN_WEEKS_FOR_PERSONALIZATION: usize = 8;
const MIN_SLOPE_VALUES: usize = 2;

pub fn linear_regression_slope(values: &[f64]) -> Option<f64> {
    if values.len() < MIN_SLOPE_VALUES {
        return None;
    }

    let n = values.len() as f64;
    let sum_x: f64 = (0..values.len()).map(|i| i as f64).sum();
    let sum_y: f64 = values.iter().sum();
    let sum_xy: f64 = values.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
    let sum_x2: f64 = (0..values.len()).map(|i| (i as f64).powi(2)).sum();

    let denominator = n * sum_x2 - sum_x * sum_x;
    if denominator.abs() < f64::EPSILON {
        return None;
    }

    Some((n * sum_xy - sum_x * sum_y) / denominator)
}

fn athlete_flat_slope_band(ctl_values: &[f64]) -> f64 {
    if ctl_values.len() < MIN_CTL_HISTORY_FOR_PERSONALIZATION {
        return DEFAULT_FLAT_SLOPE_PER_WEEK_THRESHOLD;
    }

    let weekly_means = ctl_values
        .chunks(DAYS_PER_WEEK)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| chunk.iter().sum::<f64>() / chunk.len() as f64)
        .collect::<Vec<_>>();

    if weekly_means.len() < MIN_WEEKS_FOR_PERSONALIZATION {
        return DEFAULT_FLAT_SLOPE_PER_WEEK_THRESHOLD;
    }

    let deltas = weekly_means
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .collect::<Vec<_>>();

    let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
    let variance = deltas
        .iter()
        .map(|delta| {
            let centered = delta - mean;
            centered * centered
        })
        .sum::<f64>()
        / deltas.len() as f64;

    variance.sqrt().max(DEFAULT_FLAT_SLOPE_PER_WEEK_THRESHOLD)
}

fn classify_trend(slope_per_week: f64, flat_band: f64) -> TrendState {
    if slope_per_week.abs() < flat_band {
        TrendState::Flat
    } else if slope_per_week > 0.0 {
        TrendState::Rising
    } else {
        TrendState::Declining
    }
}

fn trailing_flat_start(ctl_values: &[f64], flat_band: f64) -> usize {
    let mut start = ctl_values.len() - PLATEAU_WINDOW_DAYS;

    while start >= BACKWARD_STEP_DAYS {
        let candidate_start = start - BACKWARD_STEP_DAYS;
        let candidate_end = candidate_start + CHECK_WINDOW_DAYS;
        if candidate_end > ctl_values.len() {
            break;
        }

        let candidate = &ctl_values[candidate_start..candidate_end];
        let candidate_slope = linear_regression_slope(candidate).map(|s| s * DAYS_PER_WEEK as f64);
        let candidate_is_flat = candidate_slope
            .map(|slope| slope.abs() < flat_band)
            .unwrap_or(false);

        if candidate_is_flat {
            start = candidate_start;
        } else {
            break;
        }
    }

    start
}

pub fn detect_trailing_ctl_plateau(dates: &[String], ctl_values: &[f64]) -> ChangepointResult {
    if dates.len() != ctl_values.len() || ctl_values.len() < PLATEAU_WINDOW_DAYS {
        return ChangepointResult::unsupported();
    }

    let recent = &ctl_values[ctl_values.len() - PLATEAU_WINDOW_DAYS..];
    let flat_band = athlete_flat_slope_band(ctl_values);
    let trailing_slope_per_week =
        linear_regression_slope(recent).map(|slope| slope * DAYS_PER_WEEK as f64);
    let trend = trailing_slope_per_week
        .map(|slope| classify_trend(slope, flat_band))
        .unwrap_or_default();

    if trend != TrendState::Flat {
        return ChangepointResult {
            supported: true,
            plateau_detected: false,
            plateau_start_date: None,
            plateau_duration_days: None,
            trailing_slope_per_week,
            trend,
        };
    }

    let start_index = trailing_flat_start(ctl_values, flat_band);
    let duration_days = ctl_values.len() - start_index;

    ChangepointResult {
        supported: true,
        plateau_detected: duration_days >= PLATEAU_WINDOW_DAYS,
        plateau_start_date: dates.get(start_index).cloned(),
        plateau_duration_days: Some(duration_days),
        trailing_slope_per_week,
        trend,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dates(count: usize) -> Vec<String> {
        (0..count)
            .map(|day| format!("2026-01-{:02}", day + 1))
            .collect()
    }

    #[test]
    fn linear_regression_slope_returns_positive_for_rising_series() {
        let slope = linear_regression_slope(&[60.0, 61.0, 62.0, 63.0]).unwrap();
        assert!(slope > 0.0);
    }

    #[test]
    fn linear_regression_slope_returns_negative_for_falling_series() {
        let slope = linear_regression_slope(&[100.0, 90.0, 80.0, 70.0]).unwrap();
        assert!(slope < 0.0);
    }

    #[test]
    fn linear_regression_slope_returns_near_zero_for_constant_series() {
        let slope = linear_regression_slope(&[50.0, 50.0, 50.0, 50.0]).unwrap();
        assert!(slope.abs() < 1e-9);
    }

    #[test]
    fn linear_regression_slope_returns_none_for_single_value() {
        assert!(linear_regression_slope(&[42.0]).is_none());
    }

    #[test]
    fn linear_regression_slope_returns_none_for_empty_series() {
        let empty: &[f64] = &[];
        assert!(linear_regression_slope(empty).is_none());
    }

    #[test]
    fn linear_regression_slope_computes_exact_value_for_simple_pair() {
        // (0, 10), (1, 20) → slope = 10
        let slope = linear_regression_slope(&[10.0, 20.0]).unwrap();
        assert!((slope - 10.0).abs() < 1e-9);
    }

    #[test]
    fn detects_trailing_plateau_and_counts_only_flat_tail() {
        let dates = dates(56);
        let mut values = Vec::new();
        values.extend((0..28).map(|i| 55.0 + (i as f64 * 0.4)));
        values.extend((0..28).map(|_| 66.0));

        let result = detect_trailing_ctl_plateau(&dates, &values);
        assert!(result.supported);
        assert!(result.plateau_detected);
        assert_eq!(result.plateau_duration_days, Some(28));
        assert_eq!(result.plateau_start_date.as_deref(), Some("2026-01-29"));
        assert_eq!(result.trend, crate::domains::progress::TrendState::Flat);
    }

    #[test]
    fn rising_tail_is_not_reported_as_plateau() {
        let dates = dates(42);
        let values: Vec<f64> = (0..42).map(|i| 50.0 + (i as f64 * 0.3)).collect();

        let result = detect_trailing_ctl_plateau(&dates, &values);
        assert!(result.supported);
        assert!(!result.plateau_detected);
        assert_eq!(result.trend, crate::domains::progress::TrendState::Rising);
    }
}
