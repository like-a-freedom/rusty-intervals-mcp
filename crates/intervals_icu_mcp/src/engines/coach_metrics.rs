pub(crate) mod builder;
pub(crate) mod espe;
pub(crate) mod etvs;
pub(crate) mod heat;
pub(crate) mod helpers;
pub(crate) mod load;
pub(crate) mod ndli;
pub(crate) mod parse;
pub(crate) mod performance;
pub(crate) mod recovery;
pub(crate) mod tid;
pub(crate) mod tid_model;
pub(crate) mod trend;
pub(crate) mod wdr;

pub use trend::TrendSnapshot;

pub use builder::CoachMetricsBuilder;
pub use espe::{
    compare_power_curves, derive_espe_metrics, enrich_anchors_from_activity,
    extract_sportinfo_anchors,
};
pub use etvs::{aggregate_period_etvs, compute_etvs};
pub use heat::compute_heat_metrics_7d;
pub use load::{
    compute_acwr, compute_load_management_metrics, compute_monotony, compute_strain,
    parse_api_load_snapshot,
};
pub use ndli::compute_ndli_7d;
pub use parse::{
    compute_lnrmssd_rollup, extract_ctl_series, extract_hrv_series, parse_fitness_metrics,
    parse_wellness_metrics,
};
pub use performance::{
    classify_durability_state, compute_aerobic_decoupling, compute_efficiency_factor,
    derive_execution_metrics, derive_execution_metrics_from_streams,
};
pub use recovery::{
    classify_hrv_state, compute_durability_index, compute_fatigue_index, compute_hrv_ratio,
    compute_hrv_trend_slope, compute_readiness_score, compute_recovery_index,
    compute_recovery_quality_index, compute_stress_tolerance,
};
pub use tid::{
    compute_consistency_index, compute_polarisation, compute_tid_entropy,
    parse_polarisation_from_api,
};
pub use tid_model::{classify_tid_model, compute_z2_hr_variance};
pub use trend::{
    build_trend_snapshot, derive_trend_metrics, derive_volume_metrics,
    derive_workout_metrics_context, interpret_fitness_metrics,
};
pub use wdr::{compute_wdr_7d_rollup, compute_wdr_metrics};

#[cfg(test)]
pub(crate) mod tests;
