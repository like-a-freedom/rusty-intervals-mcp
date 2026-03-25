//! Coach guidance engine - maps metrics and alerts to findings, suggestions, and next actions.
//!
//! This module implements deterministic guidance rules based on metric thresholds and alert states.
//! All suggestions are derived from metric/alert states, not ad-hoc prose.

use crate::domains::coach::{CoachAlert, CoachAlertSeverity, CoachGuidance, CoachMetrics};

// =============================================================================
// Wellness Thresholds
// =============================================================================

/// Sleep: good threshold (≥ this value)
pub const SLEEP_GOOD_HOURS: f64 = 7.0;
/// Sleep: fair minimum threshold (6.0–7.0)
pub const SLEEP_FAIR_MIN_HOURS: f64 = 6.0;
/// Sleep: alert threshold (< this value)
const SLEEP_ALERT_HOURS: f64 = 6.5;

/// RHR: normal threshold (≤ this value)
pub const RHR_NORMAL_BPM: f64 = 55.0;
/// RHR: elevated threshold (56–60)
pub const RHR_ELEVATED_MAX_BPM: f64 = 60.0;
/// RHR: alert threshold (> this value)
const RHR_ALERT_BPM: f64 = 60.0;

/// HRV: stable threshold (≥ this value)
pub const HRV_STABLE_MS: f64 = 60.0;
/// HRV: low threshold (40–60)
pub const HRV_LOW_MIN_MS: f64 = 40.0;
/// Recovery index: alert threshold (< this value)
const RECOVERY_INDEX_ALERT: f64 = 0.6;

// =============================================================================
// Fitness/Load Thresholds
// =============================================================================

/// TSB: fresh threshold (> this value)
pub const TSB_FRESH: f64 = 10.0;
/// TSB: fatigued threshold (< this value)
pub const TSB_FATIGUED: f64 = -10.0;
/// TSB: deep fatigue alert threshold (< this value)
const TSB_DEEP_FATIGUE: f64 = -20.0;

// =============================================================================
// Volume Thresholds
// =============================================================================

/// Weekly average hours: low volume threshold (< this value)
pub const WEEKLY_AVG_LOW_HOURS: f64 = 5.0;
/// Weekly average hours: high volume threshold (> this value)
pub const WEEKLY_AVG_HIGH_HOURS: f64 = 15.0;

/// ACWR: watch threshold (> this value)
const ACWR_WATCH_RATIO: f64 = 1.3;
/// ACWR: overload threshold (> this value)
const ACWR_OVERREACH_RATIO: f64 = 1.5;
/// Monotony: repetitive-stress threshold (Foster 1998 recommends ≤ 2.0; Seiler uses 2.5).
/// Values above this indicate insufficient training variety → elevated injury/overtraining risk.
const MONOTONY_ALERT: f64 = 2.5;
/// Fatigue Index: high fatigue alert threshold (> this value)
const FATIGUE_INDEX_ALERT: f64 = 2.5;
/// Durability Index: low durability alert threshold (< this value)
const DURABILITY_INDEX_ALERT: f64 = 0.85;

// =============================================================================
// Alert Generation
// =============================================================================

pub fn build_alerts(metrics: &CoachMetrics) -> Vec<CoachAlert> {
    let mut alerts = Vec::new();

    // Deep fatigue alert (TSB < -20)
    if let Some(fitness) = &metrics.fitness
        && let Some(tsb) = fitness.tsb
        && tsb < TSB_DEEP_FATIGUE
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Priority,
            code: "deep_fatigue".to_string(),
            title: "Deep fatigue signal".to_string(),
            evidence: vec![format!("TSB below -20 ({:.1})", tsb)],
            section: "fitness".to_string(),
        });
    }

    // Fatigue alert (TSB < -10 but >= -20)
    if let Some(fitness) = &metrics.fitness
        && let Some(tsb) = fitness.tsb
        && (TSB_DEEP_FATIGUE..TSB_FATIGUED).contains(&tsb)
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "fatigue".to_string(),
            title: "Fatigue accumulation".to_string(),
            evidence: vec![format!("TSB below -10 ({:.1})", tsb)],
            section: "fitness".to_string(),
        });
    }

    // Low sleep alert
    if let Some(wellness) = &metrics.wellness
        && let Some(sleep) = wellness.avg_sleep_hours
        && sleep < SLEEP_ALERT_HOURS
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "low_sleep".to_string(),
            title: "Low sleep support".to_string(),
            evidence: vec![format!(
                "Average sleep below {:.1}h ({:.1}h)",
                SLEEP_ALERT_HOURS, sleep
            )],
            section: "wellness".to_string(),
        });
    }

    // Elevated RHR alert
    if let Some(wellness) = &metrics.wellness
        && let Some(rhr) = wellness.avg_resting_hr
        && rhr > RHR_ALERT_BPM
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "elevated_rhr".to_string(),
            title: "Elevated RHR signal".to_string(),
            evidence: vec![format!(
                "RHR above {:.0} bpm ({:.0} bpm)",
                RHR_ALERT_BPM, rhr
            )],
            section: "wellness".to_string(),
        });
    }

    // Personal-baseline HRV alert
    if let Some(wellness) = &metrics.wellness
        && let Some(hrv_state) = wellness.hrv_trend_state.as_deref()
        && matches!(hrv_state, "suppressed" | "below_range")
    {
        let evidence = match (
            wellness.hrv_deviation_pct,
            wellness.hrv_baseline,
            wellness.avg_hrv,
        ) {
            (Some(deviation_pct), Some(baseline), Some(current)) => vec![format!(
                "HRV {:.1}% below personal baseline ({:.0} ms vs {:.0} ms)",
                deviation_pct.abs(),
                current,
                baseline
            )],
            _ => vec!["HRV is below the athlete's recent personal range".to_string()],
        };

        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "low_hrv".to_string(),
            title: "HRV below personal baseline".to_string(),
            evidence,
            section: "wellness".to_string(),
        });
    }

    // Low recovery index alert
    if let Some(wellness) = &metrics.wellness
        && let Some(recovery_index) = wellness.recovery_index
        && recovery_index < RECOVERY_INDEX_ALERT
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Priority,
            code: "low_recovery_index".to_string(),
            title: "Recovery-first signal".to_string(),
            evidence: vec![format!(
                "Recovery index below {:.2} ({:.2})",
                RECOVERY_INDEX_ALERT, recovery_index
            )],
            section: "wellness".to_string(),
        });
    }

    // ACWR alerts
    if let Some(load_management) = &metrics.load_management
        && let Some(acwr) = &load_management.acwr
    {
        if acwr.ratio > ACWR_OVERREACH_RATIO {
            alerts.push(CoachAlert {
                severity: CoachAlertSeverity::Priority,
                code: "acwr_overreaching".to_string(),
                title: "Acute load spike".to_string(),
                evidence: vec![format!(
                    "ACWR {:.2} exceeds {:.1}",
                    acwr.ratio, ACWR_OVERREACH_RATIO
                )],
                section: "load_management".to_string(),
            });
        } else if acwr.ratio > ACWR_WATCH_RATIO {
            alerts.push(CoachAlert {
                severity: CoachAlertSeverity::Caution,
                code: "acwr_watch".to_string(),
                title: "Load ramp watch".to_string(),
                evidence: vec![format!(
                    "ACWR {:.2} exceeds {:.1}",
                    acwr.ratio, ACWR_WATCH_RATIO
                )],
                section: "load_management".to_string(),
            });
        }
    }

    // Monotony alert
    if let Some(load_management) = &metrics.load_management
        && let Some(monotony) = load_management.monotony
        && monotony > MONOTONY_ALERT
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "high_monotony".to_string(),
            title: "Repetitive load pattern".to_string(),
            evidence: vec![format!(
                "Monotony {:.2} exceeds {:.1}",
                monotony, MONOTONY_ALERT
            )],
            section: "load_management".to_string(),
        });
    }

    // Fatigue Index alert
    if let Some(load_management) = &metrics.load_management
        && let Some(fatigue_index) = load_management.fatigue_index
        && fatigue_index > FATIGUE_INDEX_ALERT
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "high_fatigue_index".to_string(),
            title: "High fatigue index".to_string(),
            evidence: vec![format!(
                "Fatigue Index {:.2} exceeds {:.1}",
                fatigue_index, FATIGUE_INDEX_ALERT
            )],
            section: "load_management".to_string(),
        });
    }

    // Durability Index alert
    if let Some(load_management) = &metrics.load_management
        && let Some(durability_index) = load_management.durability_index
        && durability_index < DURABILITY_INDEX_ALERT
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "low_durability_index".to_string(),
            title: "Low durability index".to_string(),
            evidence: vec![format!(
                "Durability Index {:.3} is below {:.2} — power curve degraded after accumulated work",
                durability_index, DURABILITY_INDEX_ALERT
            )],
            section: "load_management".to_string(),
        });
    }

    // High training load alert
    if let Some(volume) = &metrics.volume
        && volume.weekly_avg_hours > WEEKLY_AVG_HIGH_HOURS
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "high_training_load".to_string(),
            title: "High training load".to_string(),
            evidence: vec![format!(
                "Weekly average {:.1}h exceeds {:.0}h threshold",
                volume.weekly_avg_hours, WEEKLY_AVG_HIGH_HOURS
            )],
            section: "volume".to_string(),
        });
    }

    // Threshold-biased polarisation alert
    if let Some(polarisation) = &metrics.polarisation
        && let Some(state) = polarisation.state.as_deref()
        && state == "threshold_biased"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "threshold_biased_polarisation".to_string(),
            title: "Threshold-biased training distribution".to_string(),
            evidence: vec![format!(
                "Polarisation ratio {:.2} indicates too much threshold-zone work ({:.0}% Z2)",
                polarisation.ratio.unwrap_or(0.0),
                polarisation.z2_pct.unwrap_or(0.0) * 100.0
            )],
            section: "distribution".to_string(),
        });
    }

    // Low consistency alert
    if let Some(consistency) = &metrics.consistency
        && let Some(state) = consistency.state.as_deref()
        && state == "low"
    {
        alerts.push(CoachAlert {
            severity: CoachAlertSeverity::Caution,
            code: "low_consistency".to_string(),
            title: "Low training plan adherence".to_string(),
            evidence: vec![format!(
                "Only {} of {} planned sessions completed ({:.0}%)",
                consistency.sessions_completed,
                consistency.sessions_planned,
                consistency.ratio.unwrap_or(0.0) * 100.0
            )],
            section: "adherence".to_string(),
        });
    }

    alerts
}

fn has_alert_code(alerts: &[CoachAlert], code: &str) -> bool {
    alerts.iter().any(|alert| alert.code == code)
}

fn has_any_alert_code(alerts: &[CoachAlert], codes: &[&str]) -> bool {
    alerts
        .iter()
        .any(|alert| codes.iter().any(|code| alert.code == *code))
}

pub fn build_guidance(metrics: &CoachMetrics, alerts: &[CoachAlert]) -> CoachGuidance {
    let mut guidance = CoachGuidance::default();

    let tier_one_recovery_alert = has_any_alert_code(
        alerts,
        &[
            "deep_fatigue",
            "fatigue",
            "low_sleep",
            "elevated_rhr",
            "low_hrv",
            "low_recovery_index",
        ],
    );
    let wellness_ready_support = metrics.wellness.is_some();

    // Fitness-based guidance
    if let Some(fitness) = &metrics.fitness
        && let Some(tsb) = fitness.tsb
    {
        if tier_one_recovery_alert {
            guidance
                .findings
                .push("Recovery signals currently outweigh freshness markers.".to_string());
            guidance
                .suggestions
                .push("Prioritize recovery before intensity or key work.".to_string());
        } else if tsb > TSB_FRESH && wellness_ready_support && alerts.is_empty() {
            guidance
                .findings
                .push("Readiness markers look supportive for key work.".to_string());
            guidance
                .suggestions
                .push("Athlete looks ready for key work.".to_string());
        } else if tsb > TSB_FRESH && !wellness_ready_support {
            guidance.findings.push(
                "Freshness markers are positive, but wellness support is incomplete.".to_string(),
            );
        } else if tsb < TSB_FATIGUED {
            guidance
                .findings
                .push("Load balance indicates accumulated fatigue.".to_string());
            guidance
                .suggestions
                .push("Prioritize recovery before intensity.".to_string());
        }
    }

    if tier_one_recovery_alert && guidance.suggestions.is_empty() {
        guidance
            .suggestions
            .push("Prioritize recovery before intensity or key work.".to_string());
    }

    if has_alert_code(alerts, "low_recovery_index") {
        guidance.findings.push(
            "Recovery index is materially suppressed relative to resting strain.".to_string(),
        );
    }

    // Volume-based guidance
    if let Some(volume) = &metrics.volume {
        if volume.weekly_avg_hours < WEEKLY_AVG_LOW_HOURS {
            guidance
                .suggestions
                .push("Current period is low-volume.".to_string());
        } else if volume.weekly_avg_hours > WEEKLY_AVG_HIGH_HOURS {
            guidance
                .suggestions
                .push("Monitor recovery load carefully during this high-volume block.".to_string());
        }
    }

    // Tier 2 load-management guidance only after Tier 1 gating is applied.
    if has_alert_code(alerts, "acwr_overreaching") {
        guidance
            .findings
            .push("Acute load is rising faster than the recent chronic baseline.".to_string());
        guidance
            .suggestions
            .push("Reduce overload risk and absorb the block before adding more load.".to_string());
    } else if has_alert_code(alerts, "acwr_watch") {
        guidance.suggestions.push(
            "Load ramp is elevated; monitor recovery closely over the next few sessions."
                .to_string(),
        );
    }

    if has_alert_code(alerts, "high_monotony") {
        guidance.findings.push(
            "Recent training pattern looks repetitive, which can raise stress without added signal."
                .to_string(),
        );
        guidance.suggestions.push(
            "Add more day-to-day variety to reduce repetitive stress and monotony.".to_string(),
        );
    }

    if has_alert_code(alerts, "high_fatigue_index") {
        guidance.findings.push(
            "Fatigue index is elevated — accumulated load is outpacing recovery.".to_string(),
        );
        guidance.suggestions.push(
            "Prioritize recovery and consider reducing load until fatigue index improves."
                .to_string(),
        );
    }

    if has_alert_code(alerts, "low_durability_index") {
        guidance
            .findings
            .push("Power curve degraded after accumulated work — reduced durability.".to_string());
        guidance.suggestions.push(
            "Consider reducing training volume or adding recovery to restore power curve."
                .to_string(),
        );
    }

    // Distribution guidance (polarisation)
    if has_alert_code(alerts, "threshold_biased_polarisation") {
        guidance
            .findings
            .push("Training distribution is skewed toward threshold-zone work.".to_string());
        guidance.suggestions.push(
            "Shift more volume to easy (Z1) or high-intensity (Z3) to reach 80/20 polarisation."
                .to_string(),
        );
    }

    // Adherence guidance (consistency)
    if has_alert_code(alerts, "low_consistency") {
        guidance
            .findings
            .push("Plan adherence is significantly below target.".to_string());
        guidance
            .suggestions
            .push("Review schedule constraints or adjust the training plan.".to_string());
    }

    // Workout-specific guidance
    if let Some(workout) = &metrics.workout
        && let Some(count) = workout.interval_count
        && count > 0
    {
        guidance.suggestions.push(format!(
            "Completed {} work intervals - check consistency and recovery between efforts.",
            count
        ));
    }

    // Alert-based next actions
    if has_alert_code(alerts, "deep_fatigue") {
        guidance
            .next_actions
            .push("Consider a recovery-focused review before the next hard session.".to_string());
    }

    // Default next action if none specified
    if guidance.next_actions.is_empty() {
        guidance
            .next_actions
            .push("Use assess_recovery or analyze_training for deeper follow-up.".to_string());
    }

    guidance
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::coach::{
        AcwrMetrics, CoachMetrics, ConsistencyMetrics, FitnessMetrics, LoadManagementMetrics,
        PolarisationMetrics, VolumeMetrics, WellnessMetrics,
    };

    #[test]
    fn tsb_below_minus_20_creates_deep_fatigue_alert() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                ctl: Some(50.0),
                atl: Some(75.0),
                tsb: Some(-25.0),
                load_state: Some("fatigued".into()),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "deep_fatigue"));
    }

    #[test]
    fn low_sleep_creates_low_sleep_alert() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(6.2),
                avg_resting_hr: Some(52.0),
                avg_hrv: Some(65.0),
                wellness_days_count: 5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "low_sleep"));
    }

    #[test]
    fn elevated_rhr_creates_alert() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.0),
                avg_resting_hr: Some(65.0),
                avg_hrv: Some(55.0),
                wellness_days_count: 5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "elevated_rhr"));
    }

    #[test]
    fn low_hrv_creates_alert() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.0),
                avg_resting_hr: Some(52.0),
                avg_hrv: Some(35.0),
                hrv_baseline: Some(50.0),
                hrv_deviation_pct: Some(-30.0),
                hrv_trend_state: Some("suppressed".into()),
                wellness_days_count: 5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "low_hrv"));
    }

    #[test]
    fn low_hrv_alert_uses_personal_baseline_drop_even_when_absolute_value_is_high() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(55.0),
                avg_hrv: Some(64.0),
                hrv_baseline: Some(80.0),
                resting_hr_baseline: Some(50.0),
                hrv_deviation_pct: Some(-20.0),
                hrv_trend_state: Some("suppressed".into()),
                recovery_index: Some(0.73),
                wellness_days_count: 7,
                avg_mood: None,
                avg_stress: None,
                avg_fatigue: None,
                readiness_score: None,
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "low_hrv"));
    }

    #[test]
    fn low_hrv_alert_does_not_fire_for_personal_norm_even_when_absolute_value_is_low() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(50.0),
                avg_hrv: Some(45.0),
                hrv_baseline: Some(44.0),
                resting_hr_baseline: Some(50.0),
                hrv_deviation_pct: Some(2.3),
                hrv_trend_state: Some("within_range".into()),
                recovery_index: Some(1.02),
                wellness_days_count: 7,
                avg_mood: None,
                avg_stress: None,
                avg_fatigue: None,
                readiness_score: None,
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(!alerts.iter().any(|a| a.code == "low_hrv"));
    }

    #[test]
    fn high_volume_generates_cautionary_suggestion() {
        let metrics = CoachMetrics {
            volume: Some(VolumeMetrics {
                weekly_avg_hours: 16.0,
                ..Default::default()
            }),
            ..Default::default()
        };

        let guidance = build_guidance(&metrics, &[]);
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("recovery load") || s.contains("load"))
        );
    }

    #[test]
    fn positive_tsb_with_supportive_wellness_creates_ready_suggestion() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                ctl: Some(55.0),
                atl: Some(40.0),
                tsb: Some(15.0),
                load_state: Some("fresh".into()),
            }),
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.6),
                avg_resting_hr: Some(51.0),
                avg_hrv: Some(66.0),
                recovery_index: Some(1.29),
                wellness_days_count: 5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let guidance = build_guidance(&metrics, &[]);
        assert!(guidance.suggestions.iter().any(|s| s.contains("ready")));
    }

    #[test]
    fn fatigue_tsb_between_minus_10_and_minus_20_creates_alert() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                ctl: Some(50.0),
                atl: Some(65.0),
                tsb: Some(-15.0),
                load_state: Some("fatigued".into()),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "fatigue"));
        assert!(!alerts.iter().any(|a| a.code == "deep_fatigue"));
    }

    #[test]
    fn high_training_load_creates_alert() {
        let metrics = CoachMetrics {
            volume: Some(VolumeMetrics {
                weekly_avg_hours: 16.5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "high_training_load"));
    }

    #[test]
    fn low_volume_does_not_create_alert() {
        let metrics = CoachMetrics {
            volume: Some(VolumeMetrics {
                weekly_avg_hours: 4.0,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(!alerts.iter().any(|a| a.code == "high_training_load"));
    }

    #[test]
    fn optimal_volume_does_not_create_alert() {
        let metrics = CoachMetrics {
            volume: Some(VolumeMetrics {
                weekly_avg_hours: 10.0,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(!alerts.iter().any(|a| a.code == "high_training_load"));
    }

    #[test]
    fn fatigue_guidance_without_deep_fatigue() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                tsb: Some(-15.0),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(guidance.suggestions.iter().any(|s| s.contains("recovery")));
    }

    #[test]
    fn tier_one_recovery_alert_suppresses_ready_for_key_work_guidance() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                tsb: Some(14.0),
                ..Default::default()
            }),
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(5.8),
                avg_resting_hr: Some(57.0),
                avg_hrv: Some(58.0),
                wellness_days_count: 4,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(
            !guidance
                .suggestions
                .iter()
                .any(|s| s.contains("ready for key work"))
        );
        assert!(guidance.suggestions.iter().any(|s| s.contains("recovery")));
    }

    #[test]
    fn acwr_overreach_creates_overload_alert_and_guidance() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                acwr: Some(AcwrMetrics {
                    acute_load: 420.0,
                    chronic_load: 260.0,
                    ratio: 1.62,
                    state: "overreaching".into(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(alerts.iter().any(|a| a.code == "acwr_overreaching"));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("load") || s.contains("overload"))
        );
    }

    #[test]
    fn high_monotony_creates_repetitive_stress_guidance() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                monotony: Some(2.8),
                strain: Some(810.0),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(alerts.iter().any(|a| a.code == "high_monotony"));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("repetitive") || s.contains("variety"))
        );
    }

    #[test]
    fn low_recovery_index_creates_recovery_first_guidance() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.1),
                avg_resting_hr: Some(60.0),
                avg_hrv: Some(32.0),
                recovery_index: Some(0.53),
                wellness_days_count: 5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(alerts.iter().any(|a| a.code == "low_recovery_index"));
        assert!(guidance.suggestions.iter().any(|s| s.contains("recovery")));
    }

    #[test]
    fn missing_wellness_prevents_overconfident_ready_language() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                tsb: Some(12.0),
                ..Default::default()
            }),
            wellness: None,
            ..Default::default()
        };

        let guidance = build_guidance(&metrics, &[]);

        assert!(
            !guidance
                .suggestions
                .iter()
                .any(|s| s.contains("ready for key work"))
        );
    }

    #[test]
    fn alert_code_helpers_match_expected_codes() {
        let alerts = vec![
            CoachAlert {
                severity: CoachAlertSeverity::Caution,
                code: "low_sleep".into(),
                title: "Low sleep".into(),
                evidence: vec!["sleep 5.8h".into()],
                section: "wellness".into(),
            },
            CoachAlert {
                severity: CoachAlertSeverity::Priority,
                code: "deep_fatigue".into(),
                title: "Deep fatigue".into(),
                evidence: vec!["TSB -22".into()],
                section: "fitness".into(),
            },
        ];

        assert!(has_alert_code(&alerts, "low_sleep"));
        assert!(has_any_alert_code(&alerts, &["acwr_watch", "deep_fatigue"]));
        assert!(!has_alert_code(&alerts, "high_monotony"));
    }

    #[test]
    fn threshold_biased_polarisation_creates_alert_and_guidance() {
        let metrics = CoachMetrics {
            polarisation: Some(PolarisationMetrics {
                z1_pct: Some(0.50),
                z2_pct: Some(0.45),
                z3_pct: Some(0.05),
                ratio: Some(0.61),
                state: Some("threshold_biased".into()),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(
            alerts
                .iter()
                .any(|a| a.code == "threshold_biased_polarisation")
        );
        assert!(guidance.suggestions.iter().any(|s| s.contains("80/20")));
    }

    #[test]
    fn polarised_training_does_not_create_alert() {
        let metrics = CoachMetrics {
            polarisation: Some(PolarisationMetrics {
                z1_pct: Some(0.50),
                z2_pct: Some(0.35),
                z3_pct: Some(0.15),
                ratio: Some(0.93),
                state: Some("polarised".into()),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(
            !alerts
                .iter()
                .any(|a| a.code == "threshold_biased_polarisation")
        );
    }

    #[test]
    fn low_consistency_creates_alert_and_guidance() {
        let metrics = CoachMetrics {
            consistency: Some(ConsistencyMetrics {
                sessions_planned: 10,
                sessions_completed: 3,
                ratio: Some(0.3),
                state: Some("low".into()),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(alerts.iter().any(|a| a.code == "low_consistency"));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("schedule") || s.contains("plan"))
        );
    }

    #[test]
    fn good_consistency_does_not_create_alert() {
        let metrics = CoachMetrics {
            consistency: Some(ConsistencyMetrics {
                sessions_planned: 10,
                sessions_completed: 9,
                ratio: Some(0.9),
                state: Some("excellent".into()),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(!alerts.iter().any(|a| a.code == "low_consistency"));
    }

    #[test]
    fn high_fatigue_index_creates_alert_and_guidance() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                fatigue_index: Some(3.2),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(alerts.iter().any(|a| a.code == "high_fatigue_index"));
        assert!(
            guidance
                .findings
                .iter()
                .any(|f| f.contains("Fatigue index"))
        );
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("recovery") || s.contains("fatigue"))
        );
    }

    #[test]
    fn fatigue_index_below_threshold_does_not_alert() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                fatigue_index: Some(1.5),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(!alerts.iter().any(|a| a.code == "high_fatigue_index"));
    }

    #[test]
    fn low_durability_index_creates_alert_and_guidance() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                durability_index: Some(0.82),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(alerts.iter().any(|a| a.code == "low_durability_index"));
        assert!(guidance.findings.iter().any(|f| f.contains("durability")));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("recovery") || s.contains("volume"))
        );
    }

    #[test]
    fn durability_index_above_threshold_does_not_alert() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                durability_index: Some(0.92),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(!alerts.iter().any(|a| a.code == "low_durability_index"));
    }
}
