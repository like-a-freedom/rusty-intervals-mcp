//! Domain types for deterministic progress tracking.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TrendState {
    Rising,
    #[default]
    Flat,
    Declining,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TidDriftState {
    #[default]
    Stable,
    Converging,
    Polarizing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum HypothesisDomain {
    #[default]
    Volume,
    IntensityDistribution,
    Recovery,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ChangepointResult {
    pub supported: bool,
    pub plateau_detected: bool,
    pub plateau_start_date: Option<String>,
    pub plateau_duration_days: Option<usize>,
    pub trailing_slope_per_week: Option<f64>,
    pub trend: TrendState,
}

impl ChangepointResult {
    #[must_use]
    pub fn unsupported() -> Self {
        Self {
            supported: false,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct LnRmssdRollup {
    pub supported: bool,
    pub recent_mean_7d: Option<f64>,
    pub recent_cv_7d: Option<f64>,
    pub trend_slope: Option<f64>,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TidDriftMetrics {
    pub supported: bool,
    pub weekly_samples: usize,
    pub entropy_recent_4w: Option<f64>,
    pub entropy_prior_4w: Option<f64>,
    pub drift_state: TidDriftState,
    pub dominant_zone: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProgressHypothesis {
    pub domain: HypothesisDomain,
    pub confidence: f64,
    pub evidence: Vec<String>,
    pub suggested_intervention: String,
    pub tracking_metric: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProgressReport {
    pub plateau: ChangepointResult,
    pub acwr_ratio: Option<f64>,
    pub acwr_state: Option<String>,
    pub monotony: Option<f64>,
    pub strain: Option<f64>,
    pub tid_drift: TidDriftMetrics,
    pub lnrmssd: LnRmssdRollup,
    pub hrv_ratio: Option<f64>,
    pub hrv_trend_state: Option<String>,
    pub hrv_suppressed: bool,
    pub hypotheses: Vec<ProgressHypothesis>,
    pub recommendations: Vec<String>,
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changepoint_result_unsupported_is_not_supported() {
        let result = ChangepointResult::unsupported();
        assert!(!result.supported);
        assert!(!result.plateau_detected);
        assert_eq!(result.trend, TrendState::Flat);
    }

    #[test]
    fn progress_report_defaults_to_empty_warnings_and_hypotheses() {
        let report = ProgressReport::default();
        assert!(report.hypotheses.is_empty());
        assert!(report.recommendations.is_empty());
        assert!(report.warnings.is_empty());
    }
}
