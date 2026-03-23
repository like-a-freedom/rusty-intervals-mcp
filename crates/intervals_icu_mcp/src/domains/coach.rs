//! Shared deterministic coaching analytics contract.
//!
//! This module defines the internal schema used by existing read-only intents
//! (`analyze_training`, `assess_recovery`, `analyze_race`, and later
//! `compare_periods`) to exchange structured analytical context before
//! rendering intent-specific `IntentOutput` responses.

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AnalysisKind {
    TrainingSingle,
    TrainingPeriod,
    RecoveryAssessment,
    RaceAnalysis,
    PeriodComparison,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnalysisWindow {
    pub start_date: NaiveDate,
    pub end_date: NaiveDate,
}

impl AnalysisWindow {
    pub fn new(start_date: NaiveDate, end_date: NaiveDate) -> Self {
        Self {
            start_date,
            end_date,
        }
    }

    pub fn window_days(&self) -> i64 {
        (self.end_date - self.start_date).num_days() + 1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoachMeta {
    pub analysis_kind: AnalysisKind,
    pub window_start: NaiveDate,
    pub window_end: NaiveDate,
    pub window_days: i64,
    pub generated_at: DateTime<Utc>,
    pub data_sources: Vec<String>,
}

impl CoachMeta {
    pub fn new(analysis_kind: AnalysisKind, window: &AnalysisWindow) -> Self {
        Self {
            analysis_kind,
            window_start: window.start_date,
            window_end: window.end_date,
            window_days: window.window_days(),
            generated_at: Utc::now(),
            data_sources: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct DataAudit {
    pub activities_available: bool,
    pub wellness_available: bool,
    pub fitness_available: bool,
    pub intervals_available: bool,
    pub streams_available: bool,
    pub degraded_mode_reasons: Vec<String>,
}

impl DataAudit {
    /// Returns true if all data sources are available
    pub fn all_available(&self) -> bool {
        self.activities_available
            && self.wellness_available
            && self.fitness_available
            && self.degraded_mode_reasons.is_empty()
    }

    /// Returns a summary of data availability status
    pub fn availability_summary(&self) -> Vec<(&'static str, bool)> {
        vec![
            ("Activities", self.activities_available),
            ("Wellness", self.wellness_available),
            ("Fitness", self.fitness_available),
            ("Intervals", self.intervals_available),
            ("Streams", self.streams_available),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CoachAlertSeverity {
    Info,
    Caution,
    Priority,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CoachAlert {
    pub severity: CoachAlertSeverity,
    pub code: String,
    pub title: String,
    pub evidence: Vec<String>,
    pub section: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct CoachGuidance {
    pub findings: Vec<String>,
    pub suggestions: Vec<String>,
    pub next_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct VolumeMetrics {
    pub activity_count: usize,
    pub total_moving_time_secs: i64,
    pub total_distance_m: f64,
    pub total_elevation_gain_m: f64,
    pub weekly_avg_hours: f64,
    pub avg_activity_duration_secs: f64,
    pub activities_per_week: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct FitnessMetrics {
    pub ctl: Option<f64>,
    pub atl: Option<f64>,
    pub tsb: Option<f64>,
    pub load_state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WellnessMetrics {
    pub avg_sleep_hours: Option<f64>,
    pub avg_resting_hr: Option<f64>,
    pub avg_hrv: Option<f64>,
    pub hrv_baseline: Option<f64>,
    pub resting_hr_baseline: Option<f64>,
    pub hrv_deviation_pct: Option<f64>,
    pub hrv_trend_state: Option<String>,
    pub recovery_index: Option<f64>,
    pub wellness_days_count: usize,
    pub avg_mood: Option<f64>,
    pub avg_stress: Option<f64>,
    pub avg_fatigue: Option<f64>,
    pub readiness_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TrendMetrics {
    pub activity_count_delta: Option<i64>,
    pub time_delta_pct: Option<f64>,
    pub distance_delta_pct: Option<f64>,
    pub elevation_delta_pct: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct AcwrMetrics {
    pub acute_load: f64,
    pub chronic_load: f64,
    pub ratio: f64,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LoadManagementMetrics {
    pub acwr: Option<AcwrMetrics>,
    pub monotony: Option<f64>,
    pub strain: Option<f64>,
    pub fatigue_index: Option<f64>,
    pub stress_tolerance: Option<f64>,
    pub durability_index: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct DecouplingMetrics {
    pub efficiency_factor_first_half: f64,
    pub efficiency_factor_second_half: f64,
    pub decoupling_pct: f64,
    pub state: String,
}

/// Seiler 80/20 polarisation metrics — collapses standard 5-zone model into 3 macro-zones:
///   Z1 (Easy)      = zones 1+2 (below LT1)
///   Z2 (Threshold) = zone 3 (LT1–LT2)
///   Z3 (High)      = zones 4+5 (above LT2)
/// Formula: `ratio = (z1_pct + z3_pct) / (2 * z2_pct)`
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PolarisationMetrics {
    pub z1_pct: f64,
    pub z2_pct: f64,
    pub z3_pct: f64,
    pub ratio: Option<f64>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ConsistencyMetrics {
    pub sessions_planned: usize,
    pub sessions_completed: usize,
    pub ratio: Option<f64>,
    pub state: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct WorkoutMetricsContext {
    pub interval_count: Option<usize>,
    pub avg_hr: Option<f64>,
    pub avg_power: Option<f64>,
    pub efficiency_factor: Option<f64>,
    pub aerobic_decoupling: Option<DecouplingMetrics>,
    pub execution_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct RaceMetrics {
    pub race_duration_secs: Option<i64>,
    pub race_distance_m: Option<f64>,
    pub avg_hr: Option<f64>,
    pub segment_count: Option<usize>,
    pub race_load_note: Option<String>,
    pub post_race_recovery_note: Option<String>,
    pub efficiency_factor: Option<f64>,
    pub aerobic_decoupling: Option<DecouplingMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct CoachMetrics {
    pub volume: Option<VolumeMetrics>,
    pub fitness: Option<FitnessMetrics>,
    pub wellness: Option<WellnessMetrics>,
    pub load_management: Option<LoadManagementMetrics>,
    pub trend: Option<TrendMetrics>,
    pub workout: Option<WorkoutMetricsContext>,
    pub race: Option<RaceMetrics>,
    pub polarisation: Option<PolarisationMetrics>,
    pub consistency: Option<ConsistencyMetrics>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoachContext {
    pub meta: CoachMeta,
    pub audit: DataAudit,
    pub metrics: CoachMetrics,
    pub alerts: Vec<CoachAlert>,
    pub guidance: CoachGuidance,
}

impl CoachContext {
    pub fn new(analysis_kind: AnalysisKind, window: AnalysisWindow) -> Self {
        Self {
            meta: CoachMeta::new(analysis_kind, &window),
            audit: DataAudit::default(),
            metrics: CoachMetrics::default(),
            alerts: Vec::new(),
            guidance: CoachGuidance::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> AnalysisWindow {
        AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
        )
    }

    #[test]
    fn coach_context_records_degraded_mode_reason() {
        let mut ctx = CoachContext::new(AnalysisKind::RecoveryAssessment, window());
        ctx.audit
            .degraded_mode_reasons
            .push("fitness summary unavailable".into());

        assert_eq!(ctx.audit.degraded_mode_reasons.len(), 1);
        assert_eq!(
            ctx.audit.degraded_mode_reasons[0],
            "fitness summary unavailable"
        );
    }

    #[test]
    fn coach_context_starts_with_empty_alerts() {
        let ctx = CoachContext::new(AnalysisKind::TrainingPeriod, window());
        assert!(ctx.alerts.is_empty());
    }

    #[test]
    fn coach_alert_severity_round_trips_through_json() {
        let value = serde_json::to_value(CoachAlertSeverity::Priority).unwrap();
        let severity: CoachAlertSeverity = serde_json::from_value(value).unwrap();

        assert_eq!(severity, CoachAlertSeverity::Priority);
    }

    #[test]
    fn coach_metrics_allow_missing_sections() {
        let metrics = CoachMetrics::default();

        assert!(metrics.volume.is_none());
        assert!(metrics.fitness.is_none());
        assert!(metrics.wellness.is_none());
        assert!(metrics.load_management.is_none());
        assert!(metrics.trend.is_none());
        assert!(metrics.workout.is_none());
        assert!(metrics.race.is_none());
    }

    #[test]
    fn coach_metrics_supports_optional_load_management_section() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                acwr: Some(AcwrMetrics {
                    acute_load: 320.0,
                    chronic_load: 280.0,
                    ratio: 1.14,
                    state: "productive".into(),
                }),
                monotony: Some(1.8),
                strain: Some(540.0),
                fatigue_index: None,
                stress_tolerance: None,
                durability_index: None,
            }),
            ..Default::default()
        };

        assert!(metrics.load_management.is_some());
    }

    #[test]
    fn wellness_metrics_support_recovery_index() {
        let metrics = WellnessMetrics {
            avg_sleep_hours: Some(7.4),
            avg_resting_hr: Some(50.0),
            avg_hrv: Some(68.0),
            hrv_baseline: Some(70.0),
            resting_hr_baseline: Some(49.0),
            hrv_deviation_pct: Some(-2.9),
            hrv_trend_state: Some("within_range".into()),
            recovery_index: Some(1.36),
            wellness_days_count: 4,
            avg_mood: None,
            avg_stress: None,
            avg_fatigue: None,
            readiness_score: None,
        };

        assert_eq!(metrics.recovery_index, Some(1.36));
    }

    #[test]
    fn workout_metrics_support_execution_efficiency_and_decoupling() {
        let metrics = WorkoutMetricsContext {
            interval_count: Some(5),
            avg_hr: Some(148.0),
            avg_power: Some(235.0),
            efficiency_factor: Some(1.59),
            aerobic_decoupling: Some(DecouplingMetrics {
                efficiency_factor_first_half: 1.62,
                efficiency_factor_second_half: 1.54,
                decoupling_pct: 4.94,
                state: "acceptable".into(),
            }),
            execution_notes: vec!["steady start".into()],
        };

        assert_eq!(metrics.efficiency_factor, Some(1.59));
        assert!(metrics.aerobic_decoupling.is_some());
    }

    #[test]
    fn vnext_metric_sections_round_trip_through_json() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                recovery_index: Some(1.22),
                ..Default::default()
            }),
            load_management: Some(LoadManagementMetrics {
                acwr: Some(AcwrMetrics {
                    acute_load: 180.0,
                    chronic_load: 150.0,
                    ratio: 1.2,
                    state: "productive".into(),
                }),
                monotony: Some(1.5),
                strain: Some(270.0),
                fatigue_index: None,
                stress_tolerance: None,
                durability_index: None,
            }),
            workout: Some(WorkoutMetricsContext {
                efficiency_factor: Some(1.48),
                aerobic_decoupling: Some(DecouplingMetrics {
                    efficiency_factor_first_half: 1.5,
                    efficiency_factor_second_half: 1.44,
                    decoupling_pct: 4.0,
                    state: "acceptable".into(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let round_trip: CoachMetrics =
            serde_json::from_value(serde_json::to_value(&metrics).unwrap()).unwrap();

        assert_eq!(round_trip, metrics);
    }

    #[test]
    fn analysis_window_counts_inclusive_days() {
        let window = AnalysisWindow::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            NaiveDate::from_ymd_opt(2026, 3, 7).unwrap(),
        );

        assert_eq!(window.window_days(), 7);
    }

    #[test]
    fn data_audit_all_available_when_all_sources_present() {
        let audit = DataAudit {
            activities_available: true,
            wellness_available: true,
            fitness_available: true,
            intervals_available: true,
            streams_available: true,
            degraded_mode_reasons: vec![],
        };

        assert!(audit.all_available());
    }

    #[test]
    fn data_audit_not_all_available_when_wellness_missing() {
        let audit = DataAudit {
            activities_available: true,
            wellness_available: false,
            fitness_available: true,
            intervals_available: true,
            streams_available: true,
            degraded_mode_reasons: vec!["wellness data unavailable".to_string()],
        };

        assert!(!audit.all_available());
    }

    #[test]
    fn data_audit_availability_summary_returns_all_sources() {
        let audit = DataAudit {
            activities_available: true,
            wellness_available: false,
            fitness_available: true,
            intervals_available: false,
            streams_available: true,
            degraded_mode_reasons: vec![],
        };

        let summary = audit.availability_summary();
        assert_eq!(summary.len(), 5);
        assert_eq!(summary[0], ("Activities", true));
        assert_eq!(summary[1], ("Wellness", false));
        assert_eq!(summary[2], ("Fitness", true));
        assert_eq!(summary[3], ("Intervals", false));
        assert_eq!(summary[4], ("Streams", true));
    }
}
