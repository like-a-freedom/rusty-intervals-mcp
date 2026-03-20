/// Analysis Engine - Training performance analysis
///
/// Implements:
/// - Single workout analysis
/// - Period summary
/// - Like-for-like comparison
/// - Trend analysis (7d, 30d, 90d, 365d)
use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

/// Analysis types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnalysisType {
    SingleWorkout,
    PeriodSummary,
    LikeForLike,
    TrendAnalysis,
}

/// Single workout analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkoutAnalysis {
    pub workout_id: String,
    pub name: String,
    pub date: NaiveDate,
    pub metrics: WorkoutMetrics,
    pub grade: WorkoutGrade,
    pub insights: Vec<String>,
}

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

/// Period analysis result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodAnalysis {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
    pub summary: PeriodSummary,
    pub zone_distribution: ZoneDistribution,
    pub trends: Vec<TrendInsight>,
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

/// Zone distribution for period
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneDistribution {
    pub z1_percent: f32,
    pub z2_percent: f32,
    pub z3_percent: f32,
    pub z4_percent: f32,
    pub z5_percent: f32,
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
pub struct TrendWindows;

impl TrendWindows {
    pub const SHORT: i32 = 7; // 7 days
    pub const MEDIUM: i32 = 30; // 30 days
    pub const LONG: i32 = 90; // 90 days
    pub const YEAR: i32 = 365; // 365 days
}

/// Analysis Engine
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
        period_a_label: &str,
        period_b_label: &str,
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
            period_a_label: period_a_label.into(),
            period_b_label: period_b_label.into(),
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

        let now = chrono::Local::now().date_naive();
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

        let description = Self::trend_description(metric_name, &direction, magnitude);

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
        let mut sum_xy = 0.0;
        let mut sum_xx = 0.0;

        for (i, (_, y)) in data.iter().enumerate() {
            let x = i as f32;
            sum_x += x;
            sum_y += *y;
            sum_xy += x * y;
            sum_xx += x * x;
        }

        let slope = (n * sum_xy - sum_x * sum_y) / (n * sum_xx - sum_x * sum_x);
        let intercept = (sum_y - slope * sum_x) / n;

        (slope, intercept)
    }

    /// Generate trend description
    fn trend_description(metric: &str, direction: &TrendDirection, magnitude: f32) -> String {
        match direction {
            TrendDirection::Increasing => {
                format!(
                    "{} increasing by {:.1}% per period",
                    metric,
                    magnitude * 100.0
                )
            }
            TrendDirection::Decreasing => {
                format!(
                    "{} decreasing by {:.1}% per period",
                    metric,
                    magnitude * 100.0
                )
            }
            TrendDirection::Stable => {
                format!("{} stable (no significant change)", metric)
            }
        }
    }
}

/// Workout analysis insights generator
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
        // Use recent dates (within 30 days of current date)
        let data = vec![
            (NaiveDate::from_ymd_opt(2026, 2, 10).unwrap(), 100.0),
            (NaiveDate::from_ymd_opt(2026, 2, 17).unwrap(), 105.0),
            (NaiveDate::from_ymd_opt(2026, 2, 24).unwrap(), 110.0),
            (NaiveDate::from_ymd_opt(2026, 3, 3).unwrap(), 115.0),
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
    fn test_trend_directions() {
        use chrono::Duration;

        // Increasing trend
        let now = NaiveDate::from_ymd_opt(2026, 3, 4).unwrap();
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
    // AnalysisType Tests
    // ========================================================================

    #[test]
    fn test_analysis_type_variants() {
        let single = AnalysisType::SingleWorkout;
        let period = AnalysisType::PeriodSummary;
        let like_for_like = AnalysisType::LikeForLike;
        let trend = AnalysisType::TrendAnalysis;

        assert_ne!(single, period);
        assert_ne!(like_for_like, trend);
    }

    #[test]
    fn test_analysis_type_clone() {
        let original = AnalysisType::TrendAnalysis;
        let cloned = original.clone();
        assert_eq!(format!("{:?}", original), format!("{:?}", cloned));
    }

    #[test]
    fn test_analysis_type_debug() {
        let single = AnalysisType::SingleWorkout;
        let debug = format!("{:?}", single);
        assert!(debug.contains("SingleWorkout"));
    }

    // ========================================================================
    // WorkoutMetrics Tests
    // ========================================================================

    #[test]
    fn test_workout_metrics_default() {
        let metrics = WorkoutMetrics::default();
        assert_eq!(metrics.duration_minutes, 0);
        assert_eq!(metrics.distance_km, 0.0);
        assert_eq!(metrics.elevation_gain_m, 0.0);
        assert!(metrics.avg_hr.is_none());
        assert!(metrics.max_hr.is_none());
        assert!(metrics.avg_power.is_none());
        assert!(metrics.normalized_power.is_none());
        assert!(metrics.tss.is_none());
        assert!(metrics.avg_pace_min_per_km.is_none());
        assert!(metrics.hr_drift_percent.is_none());
        assert!(metrics.pace_variance_percent.is_none());
    }

    #[test]
    fn test_workout_metrics_clone() {
        let original = WorkoutMetrics {
            duration_minutes: 90,
            distance_km: 15.5,
            elevation_gain_m: 250.0,
            avg_hr: Some(155),
            max_hr: Some(178),
            avg_power: Some(250.0),
            normalized_power: Some(265.0),
            tss: Some(85.0),
            avg_pace_min_per_km: Some(5.5),
            hr_drift_percent: Some(4.5),
            pace_variance_percent: Some(3.2),
        };
        let cloned = original.clone();

        assert_eq!(cloned.duration_minutes, original.duration_minutes);
        assert_eq!(cloned.distance_km, original.distance_km);
        assert_eq!(cloned.avg_hr, original.avg_hr);
        assert_eq!(cloned.tss, original.tss);
    }

    #[test]
    fn test_workout_metrics_partial_fields() {
        let metrics = WorkoutMetrics {
            duration_minutes: 45,
            distance_km: 8.0,
            ..Default::default()
        };

        assert_eq!(metrics.duration_minutes, 45);
        assert_eq!(metrics.distance_km, 8.0);
        assert!(metrics.avg_hr.is_none());
    }

    #[test]
    fn test_workout_metrics_debug() {
        let metrics = WorkoutMetrics {
            duration_minutes: 60,
            ..Default::default()
        };
        let debug = format!("{:?}", metrics);
        assert!(debug.contains("WorkoutMetrics"));
        assert!(debug.contains("60"));
    }

    // ========================================================================
    // WorkoutGrade Tests
    // ========================================================================

    #[test]
    fn test_workout_grade_all_variants() {
        let grades = [
            WorkoutGrade::A,
            WorkoutGrade::B,
            WorkoutGrade::C,
            WorkoutGrade::D,
            WorkoutGrade::F,
        ];

        for i in 0..grades.len() {
            for j in 0..grades.len() {
                if i == j {
                    assert_eq!(grades[i], grades[j]);
                } else {
                    assert_ne!(grades[i], grades[j]);
                }
            }
        }
    }

    #[test]
    fn test_workout_grade_clone() {
        let original = WorkoutGrade::A;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_workout_grade_debug() {
        let grade = WorkoutGrade::B;
        let debug = format!("{:?}", grade);
        assert!(debug.contains("B"));
    }

    #[test]
    fn test_workout_grade_equality() {
        assert_eq!(WorkoutGrade::A, WorkoutGrade::A);
        assert_eq!(WorkoutGrade::F, WorkoutGrade::F);
        assert_ne!(WorkoutGrade::A, WorkoutGrade::B);
        assert_ne!(WorkoutGrade::C, WorkoutGrade::D);
    }

    // ========================================================================
    // ZoneDistribution Tests
    // ========================================================================

    #[test]
    fn test_zone_distribution_clone() {
        let original = ZoneDistribution {
            z1_percent: 0.60,
            z2_percent: 0.25,
            z3_percent: 0.10,
            z4_percent: 0.03,
            z5_percent: 0.02,
        };
        let cloned = original.clone();

        assert_eq!(cloned.z1_percent, original.z1_percent);
        assert_eq!(cloned.z2_percent, original.z2_percent);
        assert_eq!(cloned.z3_percent, original.z3_percent);
        assert_eq!(cloned.z4_percent, original.z4_percent);
        assert_eq!(cloned.z5_percent, original.z5_percent);
    }

    #[test]
    fn test_zone_distribution_debug() {
        let zone = ZoneDistribution {
            z1_percent: 0.80,
            z2_percent: 0.15,
            z3_percent: 0.05,
            z4_percent: 0.0,
            z5_percent: 0.0,
        };
        let debug = format!("{:?}", zone);
        assert!(debug.contains("ZoneDistribution"));
    }

    #[test]
    fn test_zone_distribution_sum() {
        let zone = ZoneDistribution {
            z1_percent: 0.60,
            z2_percent: 0.25,
            z3_percent: 0.10,
            z4_percent: 0.03,
            z5_percent: 0.02,
        };
        let sum =
            zone.z1_percent + zone.z2_percent + zone.z3_percent + zone.z4_percent + zone.z5_percent;
        assert!((sum - 1.0).abs() < 0.01);
    }

    // ========================================================================
    // TrendInsight Tests
    // ========================================================================

    #[test]
    fn test_trend_insight_clone() {
        let original = TrendInsight {
            metric: "Volume".into(),
            direction: TrendDirection::Increasing,
            magnitude: 5.5,
            description: "Volume is increasing".into(),
        };
        let cloned = original.clone();

        assert_eq!(cloned.metric, original.metric);
        assert_eq!(cloned.direction, original.direction);
        assert_eq!(cloned.magnitude, original.magnitude);
        assert_eq!(cloned.description, original.description);
    }

    #[test]
    fn test_trend_insight_debug() {
        let insight = TrendInsight {
            metric: "HR".into(),
            direction: TrendDirection::Stable,
            magnitude: 0.01,
            description: "HR stable".into(),
        };
        let debug = format!("{:?}", insight);
        assert!(debug.contains("TrendInsight"));
        assert!(debug.contains("HR"));
    }

    #[test]
    fn test_trend_insight_with_all_directions() {
        let base = TrendInsight {
            metric: "Test".into(),
            direction: TrendDirection::Increasing,
            magnitude: 1.0,
            description: "Test".into(),
        };

        let increasing = TrendInsight {
            direction: TrendDirection::Increasing,
            ..base.clone()
        };
        let decreasing = TrendInsight {
            direction: TrendDirection::Decreasing,
            ..base.clone()
        };
        let stable = TrendInsight {
            direction: TrendDirection::Stable,
            ..base.clone()
        };

        assert_ne!(increasing.direction, decreasing.direction);
        assert_ne!(increasing.direction, stable.direction);
        assert_ne!(decreasing.direction, stable.direction);
    }

    // ========================================================================
    // TrendDirection Tests
    // ========================================================================

    #[test]
    fn test_trend_direction_all_variants() {
        let increasing = TrendDirection::Increasing;
        let decreasing = TrendDirection::Decreasing;
        let stable = TrendDirection::Stable;

        assert_ne!(increasing, decreasing);
        assert_ne!(increasing, stable);
        assert_ne!(decreasing, stable);
    }

    #[test]
    fn test_trend_direction_clone() {
        let original = TrendDirection::Increasing;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_trend_direction_debug() {
        let dir = TrendDirection::Decreasing;
        let debug = format!("{:?}", dir);
        assert!(debug.contains("Decreasing"));
    }

    #[test]
    fn test_trend_direction_equality() {
        assert_eq!(TrendDirection::Increasing, TrendDirection::Increasing);
        assert_eq!(TrendDirection::Stable, TrendDirection::Stable);
        assert_ne!(TrendDirection::Increasing, TrendDirection::Decreasing);
    }

    // ========================================================================
    // MetricComparison Tests
    // ========================================================================

    #[test]
    fn test_metric_comparison_clone() {
        let original = MetricComparison {
            name: "Volume".into(),
            period_a_value: 100.0,
            period_b_value: 110.0,
            delta_absolute: 10.0,
            delta_percent: 10.0,
        };
        let cloned = original.clone();

        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.period_a_value, original.period_a_value);
        assert_eq!(cloned.period_b_value, original.period_b_value);
        assert_eq!(cloned.delta_absolute, original.delta_absolute);
        assert_eq!(cloned.delta_percent, original.delta_percent);
    }

    #[test]
    fn test_metric_comparison_debug() {
        let comparison = MetricComparison {
            name: "TSS".into(),
            period_a_value: 500.0,
            period_b_value: 550.0,
            delta_absolute: 50.0,
            delta_percent: 10.0,
        };
        let debug = format!("{:?}", comparison);
        assert!(debug.contains("MetricComparison"));
        assert!(debug.contains("TSS"));
    }

    #[test]
    fn test_metric_comparison_negative_delta() {
        let comparison = MetricComparison {
            name: "Distance".into(),
            period_a_value: 200.0,
            period_b_value: 180.0,
            delta_absolute: -20.0,
            delta_percent: -10.0,
        };

        assert!(comparison.delta_absolute < 0.0);
        assert!(comparison.delta_percent < 0.0);
    }

    // ========================================================================
    // LikeForLikeComparison Tests
    // ========================================================================

    #[test]
    fn test_like_for_like_comparison_clone() {
        let original = LikeForLikeComparison {
            period_a_label: "Period A".into(),
            period_b_label: "Period B".into(),
            metrics: vec![MetricComparison {
                name: "Test".into(),
                period_a_value: 100.0,
                period_b_value: 110.0,
                delta_absolute: 10.0,
                delta_percent: 10.0,
            }],
            summary: "Summary".into(),
        };
        let cloned = original.clone();

        assert_eq!(cloned.period_a_label, original.period_a_label);
        assert_eq!(cloned.period_b_label, original.period_b_label);
        assert_eq!(cloned.metrics.len(), original.metrics.len());
        assert_eq!(cloned.summary, original.summary);
    }

    #[test]
    fn test_like_for_like_comparison_debug() {
        let comparison = LikeForLikeComparison {
            period_a_label: "Feb".into(),
            period_b_label: "Jan".into(),
            metrics: vec![],
            summary: "Volume increased".into(),
        };
        let debug = format!("{:?}", comparison);
        assert!(debug.contains("LikeForLikeComparison"));
        assert!(debug.contains("Feb"));
    }

    #[test]
    fn test_like_for_like_comparison_empty_metrics() {
        let comparison = LikeForLikeComparison {
            period_a_label: "A".into(),
            period_b_label: "B".into(),
            metrics: vec![],
            summary: "No metrics".into(),
        };

        assert!(comparison.metrics.is_empty());
        assert_eq!(comparison.period_a_label, "A");
    }

    // ========================================================================
    // WorkoutAnalysis Tests
    // ========================================================================

    #[test]
    fn test_workout_analysis_clone() {
        let original = WorkoutAnalysis {
            workout_id: "123".into(),
            name: "Morning Run".into(),
            date: NaiveDate::from_ymd_opt(2026, 3, 5).unwrap(),
            metrics: WorkoutMetrics::default(),
            grade: WorkoutGrade::A,
            insights: vec!["Good workout".into()],
        };
        let cloned = original.clone();

        assert_eq!(cloned.workout_id, original.workout_id);
        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.date, original.date);
        assert_eq!(cloned.grade, original.grade);
        assert_eq!(cloned.insights.len(), original.insights.len());
    }

    #[test]
    fn test_workout_analysis_debug() {
        let analysis = WorkoutAnalysis {
            workout_id: "456".into(),
            name: "Test Workout".into(),
            date: NaiveDate::from_ymd_opt(2026, 3, 6).unwrap(),
            metrics: WorkoutMetrics::default(),
            grade: WorkoutGrade::B,
            insights: vec![],
        };
        let debug = format!("{:?}", analysis);
        assert!(debug.contains("WorkoutAnalysis"));
        assert!(debug.contains("Test Workout"));
    }

    // ========================================================================
    // PeriodAnalysis Tests
    // ========================================================================

    #[test]
    fn test_period_analysis_clone() {
        let original = PeriodAnalysis {
            start_date: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
            summary: PeriodSummary::default(),
            zone_distribution: ZoneDistribution {
                z1_percent: 0.8,
                z2_percent: 0.15,
                z3_percent: 0.05,
                z4_percent: 0.0,
                z5_percent: 0.0,
            },
            trends: vec![],
        };
        let cloned = original.clone();

        assert_eq!(cloned.start_date, original.start_date);
        assert_eq!(cloned.end_date, original.end_date);
        assert_eq!(
            cloned.zone_distribution.z1_percent,
            original.zone_distribution.z1_percent
        );
        assert_eq!(cloned.trends.len(), original.trends.len());
    }

    #[test]
    fn test_period_analysis_debug() {
        let analysis = PeriodAnalysis {
            start_date: NaiveDate::from_ymd_opt(2026, 2, 1).unwrap(),
            end_date: NaiveDate::from_ymd_opt(2026, 2, 28).unwrap(),
            summary: PeriodSummary::default(),
            zone_distribution: ZoneDistribution {
                z1_percent: 1.0,
                z2_percent: 0.0,
                z3_percent: 0.0,
                z4_percent: 0.0,
                z5_percent: 0.0,
            },
            trends: vec![],
        };
        let debug = format!("{:?}", analysis);
        assert!(debug.contains("PeriodAnalysis"));
    }

    // ========================================================================
    // PeriodSummary Tests
    // ========================================================================

    #[test]
    fn test_period_summary_default() {
        let summary = PeriodSummary::default();
        assert_eq!(summary.workout_count, 0);
        assert_eq!(summary.total_time_hours, 0.0);
        assert_eq!(summary.total_distance_km, 0.0);
        assert_eq!(summary.total_elevation_m, 0.0);
        assert_eq!(summary.avg_weekly_hours, 0.0);
        assert_eq!(summary.total_tss, 0.0);
        assert_eq!(summary.avg_tss_per_week, 0.0);
    }

    #[test]
    fn test_period_summary_clone() {
        let original = PeriodSummary {
            workout_count: 20,
            total_time_hours: 45.5,
            total_distance_km: 350.0,
            total_elevation_m: 5000.0,
            avg_weekly_hours: 11.375,
            total_tss: 6000.0,
            avg_tss_per_week: 1500.0,
        };
        let cloned = original.clone();

        assert_eq!(cloned.workout_count, original.workout_count);
        assert_eq!(cloned.total_time_hours, original.total_time_hours);
        assert_eq!(cloned.total_tss, original.total_tss);
    }

    #[test]
    fn test_period_summary_debug() {
        let summary = PeriodSummary {
            workout_count: 15,
            ..Default::default()
        };
        let debug = format!("{:?}", summary);
        assert!(debug.contains("PeriodSummary"));
        assert!(debug.contains("15"));
    }

    // ========================================================================
    // TrendWindows Constants Tests
    // ========================================================================

    #[test]
    fn test_trend_windows_constants() {
        // Verify all trend window constants have expected values
        assert_eq!(TrendWindows::SHORT, 7);
        assert_eq!(TrendWindows::MEDIUM, 30);
        assert_eq!(TrendWindows::LONG, 90);
        assert_eq!(TrendWindows::YEAR, 365);
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
