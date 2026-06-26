/// Analysis Engine - Training performance analysis
///
/// Implements:
/// - Single workout analysis (grade_workout, HR drift, pace variance)
/// - Period summary
/// - Like-for-like comparison (compare_periods)
/// - Trend analysis (analyze_trend with 7d, 30d, 90d, 365d windows)
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// Workout metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkoutMetrics {
    pub duration_minutes: u32,
    pub distance_km: f32,
    pub elevation_gain_m: f32,
    pub avg_hr: Option<u32>,
    pub max_hr: Option<u32>,
    pub avg_power: Option<f32>,
    pub normalized_power: Option<f32>,
    pub tss: Option<f32>,
    pub avg_pace_min_per_km: Option<f32>,
    pub hr_drift_percent: Option<f32>,
    pub pace_variance_percent: Option<f32>,
}

/// Workout grade (A-F scale)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkoutGrade {
    A,
    B,
    C,
    D,
    F,
}

/// Period summary metrics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PeriodSummary {
    pub workout_count: u32,
    pub total_time_hours: f32,
    pub total_distance_km: f32,
    pub total_elevation_m: f32,
    pub avg_weekly_hours: f32,
    pub total_tss: f32,
    pub avg_tss_per_week: f32,
}

/// Trend insight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendInsight {
    pub metric: String,
    pub direction: TrendDirection,
    pub magnitude: f32,
    pub description: String,
}

/// Trend direction
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrendDirection {
    Increasing,
    Decreasing,
    Stable,
}

/// Like-for-like comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LikeForLikeComparison {
    pub period_a_label: String,
    pub period_b_label: String,
    pub metrics: Vec<MetricComparison>,
    pub summary: String,
}

/// Metric comparison
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricComparison {
    pub name: String,
    pub period_a_value: f32,
    pub period_b_value: f32,
    pub delta_absolute: f32,
    pub delta_percent: f32,
}

/// Trend analysis windows
#[must_use]
pub struct TrendWindows;

impl TrendWindows {
    pub const SHORT: i32 = 7; // 7 days
    pub const MEDIUM: i32 = 30; // 30 days
    pub const LONG: i32 = 90; // 90 days
    pub const YEAR: i32 = 365; // 365 days
}

/// Analysis Engine
#[must_use]
pub struct AnalysisEngine;

impl AnalysisEngine {
    /// Grade a workout based on completion and quality
    pub fn grade_workout(
        metrics: &WorkoutMetrics,
        planned: Option<&WorkoutMetrics>,
    ) -> WorkoutGrade {
        if let Some(planned) = planned {
            let duration_diff =
                (metrics.duration_minutes as i32 - planned.duration_minutes as i32).abs();
            let duration_percent = duration_diff as f32 / planned.duration_minutes as f32;

            if duration_percent < 0.05 {
                WorkoutGrade::A // Within 5% of planned
            } else if duration_percent < 0.10 {
                WorkoutGrade::B // Within 10%
            } else if duration_percent < 0.20 {
                WorkoutGrade::C // Within 20%
            } else if duration_percent < 0.30 {
                WorkoutGrade::D // Within 30%
            } else {
                WorkoutGrade::F // More than 30% off
            }
        } else {
            // No plan to compare against, grade based on HR drift and consistency
            if let Some(hr_drift) = metrics.hr_drift_percent {
                if hr_drift < 5.0 {
                    WorkoutGrade::A
                } else if hr_drift < 10.0 {
                    WorkoutGrade::B
                } else {
                    WorkoutGrade::C
                }
            } else {
                WorkoutGrade::B // Default grade
            }
        }
    }

    /// Calculate HR drift (cardiac drift during workout)
    pub fn calculate_hr_drift(_avg_hr: u32, first_half_avg: u32, second_half_avg: u32) -> f32 {
        if first_half_avg == 0 {
            return 0.0;
        }
        ((second_half_avg as f32 - first_half_avg as f32) / first_half_avg as f32) * 100.0
    }

    /// Calculate pace variance
    pub fn calculate_pace_variance(paces: &[f32]) -> f32 {
        if paces.is_empty() {
            return 0.0;
        }

        let avg = paces.iter().sum::<f32>() / paces.len() as f32;
        let variance = paces.iter().map(|p| (p - avg).powi(2)).sum::<f32>() / paces.len() as f32;

        (variance.sqrt() / avg) * 100.0
    }

    /// Compare two periods (like-for-like)
    pub fn compare_periods(
        period_a: &PeriodSummary,
        period_b: &PeriodSummary,
        label_a: &str,
        label_b: &str,
    ) -> LikeForLikeComparison {
        // Workout count
        let mut metrics = vec![MetricComparison {
            name: "Workouts".into(),
            period_a_value: period_a.workout_count as f32,
            period_b_value: period_b.workout_count as f32,
            delta_absolute: period_a.workout_count as f32 - period_b.workout_count as f32,
            delta_percent: Self::calc_percent_change(
                period_b.workout_count as f32,
                period_a.workout_count as f32,
            ),
        }];

        // Volume (hours)
        metrics.push(MetricComparison {
            name: "Volume (hours)".into(),
            period_a_value: period_a.total_time_hours,
            period_b_value: period_b.total_time_hours,
            delta_absolute: period_a.total_time_hours - period_b.total_time_hours,
            delta_percent: Self::calc_percent_change(
                period_b.total_time_hours,
                period_a.total_time_hours,
            ),
        });

        // Distance
        metrics.push(MetricComparison {
            name: "Distance (km)".into(),
            period_a_value: period_a.total_distance_km,
            period_b_value: period_b.total_distance_km,
            delta_absolute: period_a.total_distance_km - period_b.total_distance_km,
            delta_percent: Self::calc_percent_change(
                period_b.total_distance_km,
                period_a.total_distance_km,
            ),
        });

        // TSS
        metrics.push(MetricComparison {
            name: "TSS".into(),
            period_a_value: period_a.total_tss,
            period_b_value: period_b.total_tss,
            delta_absolute: period_a.total_tss - period_b.total_tss,
            delta_percent: Self::calc_percent_change(period_b.total_tss, period_a.total_tss),
        });

        // Generate summary
        let volume_change =
            Self::calc_percent_change(period_b.total_time_hours, period_a.total_time_hours);
        let summary = if volume_change.abs() <= 10.0 {
            format!(
                "Volume change: {:+.0}% - within normal range",
                volume_change
            )
        } else if volume_change > 10.0 {
            format!(
                "Volume increased by {:.0}% - monitor for overtraining",
                volume_change
            )
        } else {
            format!(
                "Volume decreased by {:.0}% - may indicate recovery",
                volume_change.abs()
            )
        };

        LikeForLikeComparison {
            period_a_label: label_a.into(),
            period_b_label: label_b.into(),
            metrics,
            summary,
        }
    }

    /// Analyze trends over time window
    pub fn analyze_trend(
        data_points: &[(NaiveDate, f32)],
        window_days: i32,
        metric_name: &str,
    ) -> Option<TrendInsight> {
        if data_points.len() < 2 {
            return None;
        }

        let now = chrono::Utc::now().date_naive();
        let cutoff = now - Duration::days(window_days as i64);

        let recent: Vec<&(NaiveDate, f32)> = data_points
            .iter()
            .filter(|(date, _)| *date >= cutoff)
            .collect();

        if recent.len() < 2 {
            return None;
        }

        // Calculate trend using simple linear regression
        let recent_vec: Vec<(NaiveDate, f32)> = recent.iter().map(|(d, v)| (*d, *v)).collect();
        let (slope, _) = Self::linear_regression(&recent_vec);

        let (direction, magnitude) = if slope > 0.05 {
            (TrendDirection::Increasing, slope)
        } else if slope < -0.05 {
            (TrendDirection::Decreasing, slope.abs())
        } else {
            (TrendDirection::Stable, slope.abs())
        };

        // Baseline = mean of recent values, used to express the slope as a
        // baseline-relative percentage per period instead of raw magnitude*100.
        let baseline: f32 =
            recent_vec.iter().map(|(_, v)| *v).sum::<f32>() / recent_vec.len() as f32;

        let description = Self::trend_description(metric_name, &direction, magnitude, baseline);

        Some(TrendInsight {
            metric: metric_name.into(),
            direction,
            magnitude,
            description,
        })
    }

    /// Calculate percentage change
    fn calc_percent_change(old_value: f32, new_value: f32) -> f32 {
        if old_value == 0.0 {
            0.0
        } else {
            ((new_value - old_value) / old_value) * 100.0
        }
    }

    /// Simple linear regression
    fn linear_regression(data: &[(NaiveDate, f32)]) -> (f32, f32) {
        let n = data.len() as f32;
        if n < 2.0 {
            return (0.0, 0.0);
        }

        let mut sum_x = 0.0;
        let mut sum_y = 0.0;
        let mut cross_product = 0.0;
        let mut sum_xx = 0.0;

        for (i, (_, y)) in data.iter().enumerate() {
            let x = i as f32;
            sum_x += x;
            sum_y += *y;
            cross_product += x * y;
            sum_xx += x * x;
        }

        let slope = (n * cross_product - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x);
        let intercept = (sum_y - slope * sum_x) / n;

        (slope, intercept)
    }

    /// Generate trend description.
    ///
    /// `magnitude` is the absolute slope (change per data point). `baseline` is
    /// the mean of the window's values. The percentage is computed relative to
    /// that baseline so the description reads e.g. "8.7% per period" rather than
    /// the nonsensical raw `magnitude * 100`.
    fn trend_description(
        metric: &str,
        direction: &TrendDirection,
        magnitude: f32,
        baseline: f32,
    ) -> String {
        let pct = if baseline.abs() > 0.0 {
            (magnitude / baseline) * 100.0
        } else {
            0.0
        };
        match direction {
            TrendDirection::Increasing => {
                format!("{} increasing by {:.1}% per period", metric, pct)
            }
            TrendDirection::Decreasing => {
                format!("{} decreasing by {:.1}% per period", metric, pct)
            }
            TrendDirection::Stable => {
                format!("{} stable (no significant change)", metric)
            }
        }
    }
}

/// Workout analysis insights generator
#[must_use]
pub struct WorkoutInsights;

impl WorkoutInsights {
    /// Generate insights from workout analysis
    pub fn generate(metrics: &WorkoutMetrics, grade: &WorkoutGrade) -> Vec<String> {
        let mut insights = Vec::new();

        // HR drift analysis
        if let Some(hr_drift) = metrics.hr_drift_percent {
            if hr_drift > 10.0 {
                insights
                    .push("High HR drift detected - may indicate fatigue or dehydration".into());
            } else if hr_drift > 5.0 {
                insights.push("Moderate HR drift - within acceptable range".into());
            } else {
                insights.push("Excellent HR stability - good aerobic fitness".into());
            }
        }

        // Pace variance
        if let Some(variance) = metrics.pace_variance_percent {
            if variance > 10.0 {
                insights.push("High pace variance - work on pacing consistency".into());
            } else {
                insights.push("Good pace consistency".into());
            }
        }

        // Grade-based insights
        match grade {
            WorkoutGrade::A => {
                insights.push("Excellent workout execution".into());
            }
            WorkoutGrade::B => {
                insights.push("Good workout, minor adjustments needed".into());
            }
            WorkoutGrade::C => {
                insights.push("Moderate execution - review pacing and effort".into());
            }
            _ => {
                insights.push("Workout execution below target - consider recovery".into());
            }
        }

        insights
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workout_grading() {
        let planned = WorkoutMetrics {
            duration_minutes: 60,
            ..Default::default()
        };

        let perfect = WorkoutMetrics {
            duration_minutes: 60,
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&perfect, Some(&planned)),
            WorkoutGrade::A
        ));

        let close = WorkoutMetrics {
            duration_minutes: 65,
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&close, Some(&planned)),
            WorkoutGrade::B
        ));
    }

    #[test]
    fn test_hr_drift_calculation() {
        let drift = AnalysisEngine::calculate_hr_drift(160, 155, 165);
        assert!(drift > 6.0);
        assert!(drift < 7.0);
    }

    #[test]
    fn test_percent_change() {
        let change = AnalysisEngine::calc_percent_change(100.0, 110.0);
        assert_eq!(change, 10.0);

        let decrease = AnalysisEngine::calc_percent_change(100.0, 90.0);
        assert_eq!(decrease, -10.0);
    }

    #[test]
    fn test_trend_analysis() {
        let now = chrono::Local::now().date_naive();
        let data = vec![
            (now - Duration::days(21), 100.0),
            (now - Duration::days(14), 105.0),
            (now - Duration::days(7), 110.0),
            (now, 115.0),
        ];

        let trend = AnalysisEngine::analyze_trend(&data, 30, "Volume");
        assert!(trend.is_some());
        let trend = trend.unwrap();
        assert!(matches!(trend.direction, TrendDirection::Increasing));
    }

    #[test]
    fn test_workout_grading_all_grades() {
        let planned = WorkoutMetrics {
            duration_minutes: 60,
            ..Default::default()
        };

        // Grade A: Within 5%
        let perfect = WorkoutMetrics {
            duration_minutes: 62,
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&perfect, Some(&planned)),
            WorkoutGrade::A
        ));

        // Grade B: Within 10%
        let close = WorkoutMetrics {
            duration_minutes: 65,
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&close, Some(&planned)),
            WorkoutGrade::B
        ));

        // Grade C: Within 20%
        let moderate = WorkoutMetrics {
            duration_minutes: 70,
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&moderate, Some(&planned)),
            WorkoutGrade::C
        ));

        // Grade D: Within 30%
        let off = WorkoutMetrics {
            duration_minutes: 75,
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&off, Some(&planned)),
            WorkoutGrade::D
        ));

        // Grade F: More than 30% off
        let fail = WorkoutMetrics {
            duration_minutes: 85,
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&fail, Some(&planned)),
            WorkoutGrade::F
        ));
    }

    #[test]
    fn test_workout_grading_no_plan() {
        let metrics = WorkoutMetrics {
            duration_minutes: 60,
            hr_drift_percent: Some(4.0),
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&metrics, None),
            WorkoutGrade::A
        ));

        let metrics_high_drift = WorkoutMetrics {
            duration_minutes: 60,
            hr_drift_percent: Some(8.0),
            ..Default::default()
        };
        assert!(matches!(
            AnalysisEngine::grade_workout(&metrics_high_drift, None),
            WorkoutGrade::B
        ));
    }

    #[test]
    fn test_pace_variance() {
        let consistent_paces = vec![5.0, 5.0, 5.0, 5.0, 5.0];
        let variance = AnalysisEngine::calculate_pace_variance(&consistent_paces);
        assert!(variance < 1.0);

        let variable_paces = vec![4.5, 5.0, 5.5, 6.0, 4.0];
        let variance = AnalysisEngine::calculate_pace_variance(&variable_paces);
        assert!(variance > 5.0);

        let empty: Vec<f32> = vec![];
        let variance = AnalysisEngine::calculate_pace_variance(&empty);
        assert_eq!(variance, 0.0);
    }

    #[test]
    fn test_like_for_like_comparison() {
        let period_a = PeriodSummary {
            workout_count: 14,
            total_time_hours: 28.75,
            total_distance_km: 245.0,
            total_tss: 4520.0,
            ..Default::default()
        };

        let period_b = PeriodSummary {
            workout_count: 12,
            total_time_hours: 24.5,
            total_distance_km: 210.0,
            total_tss: 3800.0,
            ..Default::default()
        };

        let comparison =
            AnalysisEngine::compare_periods(&period_a, &period_b, "February", "January");

        assert_eq!(comparison.period_a_label, "February");
        assert_eq!(comparison.period_b_label, "January");
        assert!(!comparison.metrics.is_empty());
        assert!(comparison.summary.contains("Volume"));
    }

    #[test]
    fn compare_periods_with_zero_b_period_yields_zero_percent_deltas() {
        let period_a = PeriodSummary {
            workout_count: 10,
            total_time_hours: 20.0,
            total_distance_km: 150.0,
            total_tss: 3000.0,
            ..Default::default()
        };
        let period_b = PeriodSummary::default();

        let comparison = AnalysisEngine::compare_periods(&period_a, &period_b, "A", "B");

        for metric in &comparison.metrics {
            assert_eq!(
                metric.delta_percent, 0.0,
                "delta_percent must be 0 when period_b value is 0 (got {} for {})",
                metric.delta_percent, metric.name
            );
        }
    }

    #[test]
    fn compare_periods_with_equal_periods_yields_zero_deltas() {
        let identical = PeriodSummary {
            workout_count: 8,
            total_time_hours: 16.0,
            total_distance_km: 120.0,
            total_tss: 2400.0,
            ..Default::default()
        };

        let comparison = AnalysisEngine::compare_periods(&identical, &identical, "Now", "Then");

        for metric in &comparison.metrics {
            assert_eq!(metric.delta_absolute, 0.0);
            assert_eq!(metric.delta_percent, 0.0);
        }
        assert!(comparison.summary.contains("within normal range"));
    }

    #[test]
    fn compare_periods_volume_increase_above_threshold_warns_overtraining() {
        let period_a = PeriodSummary {
            workout_count: 10,
            total_time_hours: 30.0,
            total_distance_km: 250.0,
            total_tss: 4500.0,
            ..Default::default()
        };
        let period_b = PeriodSummary {
            workout_count: 8,
            total_time_hours: 20.0,
            total_distance_km: 180.0,
            total_tss: 3000.0,
            ..Default::default()
        };

        let comparison = AnalysisEngine::compare_periods(&period_a, &period_b, "Now", "Then");

        assert!(comparison.summary.contains("increased"));
        assert!(comparison.summary.contains("overtraining"));
    }

    #[test]
    fn compare_periods_volume_decrease_below_threshold_suggests_recovery() {
        let period_a = PeriodSummary {
            workout_count: 6,
            total_time_hours: 10.0,
            total_distance_km: 80.0,
            total_tss: 1500.0,
            ..Default::default()
        };
        let period_b = PeriodSummary {
            workout_count: 12,
            total_time_hours: 25.0,
            total_distance_km: 200.0,
            total_tss: 4000.0,
            ..Default::default()
        };

        let comparison = AnalysisEngine::compare_periods(&period_a, &period_b, "Now", "Then");

        assert!(comparison.summary.contains("decreased"));
        assert!(comparison.summary.contains("recovery"));
    }

    #[test]
    fn compare_periods_preserves_label_order_in_result() {
        let period_a = PeriodSummary {
            workout_count: 5,
            total_time_hours: 10.0,
            total_distance_km: 80.0,
            total_tss: 1500.0,
            ..Default::default()
        };
        let period_b = PeriodSummary::default();

        let comparison =
            AnalysisEngine::compare_periods(&period_a, &period_b, "Custom A", "Custom B");

        assert_eq!(comparison.period_a_label, "Custom A");
        assert_eq!(comparison.period_b_label, "Custom B");
    }

    #[test]
    fn compare_periods_produces_four_metrics() {
        let period_a = PeriodSummary::default();
        let period_b = PeriodSummary::default();

        let comparison = AnalysisEngine::compare_periods(&period_a, &period_b, "A", "B");

        assert_eq!(comparison.metrics.len(), 4);
        assert_eq!(comparison.metrics[0].name, "Workouts");
        assert_eq!(comparison.metrics[1].name, "Volume (hours)");
        assert_eq!(comparison.metrics[2].name, "Distance (km)");
        assert_eq!(comparison.metrics[3].name, "TSS");
    }

    #[test]
    fn test_trend_directions() {
        use chrono::Duration;

        // Increasing trend
        let now = chrono::Local::now().date_naive();
        let increasing_data = vec![
            (now - Duration::days(21), 100.0),
            (now - Duration::days(14), 110.0),
            (now - Duration::days(7), 120.0),
            (now, 130.0),
        ];
        let trend = AnalysisEngine::analyze_trend(&increasing_data, 30, "Test");
        assert!(trend.is_some());
        assert!(matches!(
            trend.unwrap().direction,
            TrendDirection::Increasing
        ));

        // Decreasing trend
        let decreasing_data = vec![
            (now - Duration::days(21), 130.0),
            (now - Duration::days(14), 120.0),
            (now - Duration::days(7), 110.0),
            (now, 100.0),
        ];
        let trend = AnalysisEngine::analyze_trend(&decreasing_data, 30, "Test");
        assert!(trend.is_some());
        assert!(matches!(
            trend.unwrap().direction,
            TrendDirection::Decreasing
        ));

        // Stable trend - values very close together (slope < 0.05)
        let stable_data = vec![
            (now - Duration::days(21), 100.0),
            (now - Duration::days(14), 100.01),
            (now - Duration::days(7), 99.99),
            (now, 100.0),
        ];
        let trend = AnalysisEngine::analyze_trend(&stable_data, 30, "Test");
        assert!(trend.is_some());
        assert!(matches!(trend.unwrap().direction, TrendDirection::Stable));
    }

    #[test]
    fn test_trend_description_uses_baseline_relative_percentage() {
        use chrono::Duration;

        // [100, 110, 120, 130]: slope = 10/step, mean baseline = 115
        // Correct percentage per period = 10/115*100 ≈ 8.7%
        // Buggy behavior printed magnitude*100 = 1000%.
        let now = chrono::Utc::now().date_naive();
        let data = vec![
            (now - Duration::days(21), 100.0),
            (now - Duration::days(14), 110.0),
            (now - Duration::days(7), 120.0),
            (now, 130.0),
        ];
        let trend = AnalysisEngine::analyze_trend(&data, 30, "Volume").unwrap();
        assert!(
            trend.description.contains("8.7%"),
            "expected baseline-relative percentage, got: {}",
            trend.description
        );
        assert!(
            !trend.description.contains("1000%"),
            "description should not contain nonsensical magnitude*100, got: {}",
            trend.description
        );
    }

    #[test]
    fn test_workout_insights_generation() {
        let metrics = WorkoutMetrics {
            duration_minutes: 60,
            hr_drift_percent: Some(3.0),
            pace_variance_percent: Some(2.0),
            ..Default::default()
        };
        let grade = WorkoutGrade::A;

        let insights = WorkoutInsights::generate(&metrics, &grade);
        assert!(!insights.is_empty());
        assert!(
            insights
                .iter()
                .any(|i| i.contains("HR stability") || i.contains("Excellent"))
        );
    }

    // ========================================================================
    // WorkoutInsights Edge Cases Tests
    // ========================================================================

    #[test]
    fn test_workout_insights_high_hr_drift() {
        let metrics = WorkoutMetrics {
            duration_minutes: 60,
            hr_drift_percent: Some(12.0),
            ..Default::default()
        };
        let grade = WorkoutGrade::B;

        let insights = WorkoutInsights::generate(&metrics, &grade);
        assert!(!insights.is_empty());
        assert!(
            insights
                .iter()
                .any(|i| i.contains("High HR drift") || i.contains("fatigue"))
        );
    }

    #[test]
    fn test_workout_insights_moderate_hr_drift() {
        let metrics = WorkoutMetrics {
            duration_minutes: 60,
            hr_drift_percent: Some(7.0),
            ..Default::default()
        };
        let grade = WorkoutGrade::B;

        let insights = WorkoutInsights::generate(&metrics, &grade);
        assert!(!insights.is_empty());
        assert!(
            insights
                .iter()
                .any(|i| i.contains("Moderate") || i.contains("acceptable"))
        );
    }

    #[test]
    fn test_workout_insights_high_pace_variance() {
        let metrics = WorkoutMetrics {
            duration_minutes: 60,
            pace_variance_percent: Some(15.0),
            ..Default::default()
        };
        let grade = WorkoutGrade::B;

        let insights = WorkoutInsights::generate(&metrics, &grade);
        assert!(!insights.is_empty());
        assert!(
            insights
                .iter()
                .any(|i| i.contains("pace") || i.contains("consistency"))
        );
    }

    #[test]
    fn test_workout_insights_good_pace_consistency() {
        let metrics = WorkoutMetrics {
            duration_minutes: 60,
            pace_variance_percent: Some(3.0),
            ..Default::default()
        };
        let grade = WorkoutGrade::B;

        let insights = WorkoutInsights::generate(&metrics, &grade);
        assert!(!insights.is_empty());
        assert!(
            insights
                .iter()
                .any(|i| i.contains("Good pace") || i.contains("consistency"))
        );
    }

    #[test]
    fn test_workout_insights_low_grade() {
        let metrics = WorkoutMetrics::default();
        let grade = WorkoutGrade::D;

        let insights = WorkoutInsights::generate(&metrics, &grade);
        assert!(!insights.is_empty());
        assert!(
            insights
                .iter()
                .any(|i| i.contains("below target") || i.contains("recovery"))
        );
    }

    #[test]
    fn test_workout_insights_no_metrics() {
        let metrics = WorkoutMetrics::default();
        let grade = WorkoutGrade::C;

        let insights = WorkoutInsights::generate(&metrics, &grade);
        assert!(!insights.is_empty());
    }
}
