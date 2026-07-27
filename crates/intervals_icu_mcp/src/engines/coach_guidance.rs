//! Coach guidance engine - maps metrics and alerts to findings, suggestions, and next actions.
//!
//! This module implements deterministic guidance rules based on metric thresholds and alert states.
//! All suggestions are derived from metric/alert states, not ad-hoc prose.

use crate::domains::coach::{CoachAlert, CoachGuidance, CoachMetrics};
use crate::engines::adaptation::AdaptationState;

// =============================================================================
// Module structure
// =============================================================================

mod adaptation;
mod constants;
mod distribution;
mod load;
mod neural;
mod race_readiness;
mod sleep_rhr;
mod tsb;

// Re-export thresholds so external call sites (e.g. `assess_recovery`) can
// continue to refer to `crate::engines::coach_guidance::SLEEP_GOOD_HOURS` and
// friends. The constants live in `coach_guidance::constants`; this re-export
// restores the original public surface.
pub use constants::*;

// =============================================================================
// Alert Generation
// =============================================================================

pub fn build_alerts(metrics: &CoachMetrics) -> Vec<CoachAlert> {
    let mut alerts = Vec::new();

    // Deep fatigue / fatigue alerts (TSB-driven) — see `tsb` submodule.
    tsb::push_tsb_alerts(metrics, &mut alerts);

    // Wellness alerts (sleep, RHR, HRV, recovery index) — see `sleep_rhr` submodule.
    sleep_rhr::push_wellness_alerts(metrics, &mut alerts);

    // ACWR / monotony / fatigue / durability / volume alerts — see `load` submodule.
    load::push_load_alerts(metrics, &mut alerts);

    // Distribution-side alerts (polarisation + consistency) — see `distribution` submodule.
    distribution::push_distribution_alerts(metrics, &mut alerts);

    // Neural / heat / WDRM / decoupling alerts — see `neural` submodule.
    neural::push_neural_alerts(metrics, &mut alerts);

    // Race-readiness alert — see `race_readiness` submodule.
    race_readiness::push_race_readiness_alerts(metrics, &mut alerts);

    // Adaptation state alerts — see `adaptation` submodule.
    adaptation::push_adaptation_alerts(metrics, &mut alerts);

    alerts
}

/// Parse an adaptation state string into an `AdaptationState` enum variant.
///
/// Returns `None` for unknown or empty strings.
pub(crate) fn parse_adaptation_state(s: &str) -> Option<AdaptationState> {
    match s {
        "Baseline" => Some(AdaptationState::Baseline),
        "FatigueState" => Some(AdaptationState::FatigueState),
        "Vo2Expansion" => Some(AdaptationState::Vo2Expansion),
        "AerobicConsolidation" => Some(AdaptationState::AerobicConsolidation),
        "AnaerobicBuild" => Some(AdaptationState::AnaerobicBuild),
        "MixedAdaptation" => Some(AdaptationState::MixedAdaptation),
        "Plateau" => Some(AdaptationState::Plateau),
        _ => None,
    }
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
        } else if tsb >= TSB_FATIGUED {
            guidance.findings.push(
                "Load balance sits in a balanced range (roughly the grey/maintenance band) — useful for recovery, taper, or maintaining consistency, but usually not the strongest overload window for building fitness.".to_string(),
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

    // Race readiness guidance
    if has_alert_code(alerts, "low_race_readiness") {
        guidance.findings.push(
            "Race readiness score is suboptimal — review TSB, durability, and neural load."
                .to_string(),
        );
        guidance.suggestions.push(
            "Focus on taper quality: ensure TSB is positive, durability is stable, and neural load is balanced."
                .to_string(),
        );
    }

    // NDLI guidance
    if has_alert_code(alerts, "ndli_overload") {
        guidance
            .findings
            .push("High neural density detected — ≥4 high-intensity sessions in 7 days increases CNS fatigue risk.".to_string());
        guidance
            .suggestions
            .push("Schedule a low-intensity or rest day to allow neural recovery.".to_string());
    }

    if has_alert_code(alerts, "ndli_elevated") {
        guidance.findings.push(
            "3 high-intensity days in the last 7 — neural load is elevated but not yet critical."
                .to_string(),
        );
        guidance.suggestions.push(
            "Monitor next session intensity; avoid a 4th high-intensity day this week.".to_string(),
        );
    }

    // ISDM guidance
    if has_alert_code(alerts, "durability_drifting") {
        guidance.findings.push(
            "Positive decoupling indicates the athlete is drifting — power output drops faster than \
             heart rate rises across the session."
                .to_string(),
        );
        guidance.suggestions.push(
            "Consider reducing session intensity or adding aerobic volume to stabilize durability."
                .to_string(),
        );
    }

    if has_alert_code(alerts, "durability_improving") {
        guidance.findings.push(
            "Negative decoupling trend — athlete is improving aerobic durability.".to_string(),
        );
        guidance.suggestions.push(
            "Continue current training approach; durability development is on track.".to_string(),
        );
    }

    // WDRM guidance
    if has_alert_code(alerts, "high_wbal_depletion") {
        guidance.findings.push(
            "W′ reserves were significantly depleted during the session — anaerobic contribution \
             exceeded sustainable capacity."
                .to_string(),
        );
        guidance.suggestions.push(
            "Allow sufficient recovery before the next high-intensity session to restore W′ capacity."
                .to_string(),
        );
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
        AcwrMetrics, CoachAlertSeverity, CoachMetrics, ConsistencyMetrics, DecouplingMetrics,
        FitnessMetrics, HeatMetrics, LoadManagementMetrics, NdliMetrics, PolarisationMetrics,
        RaceReadinessMetrics, VolumeMetrics, WdrMetrics, WellnessMetrics, WorkoutMetricsContext,
    };
    use crate::engines::adaptation::AdaptationState;

    // -------------------------------------------------------------------------
    // parse_adaptation_state
    // -------------------------------------------------------------------------

    #[test]
    fn parse_adaptation_state_baseline() {
        assert_eq!(
            parse_adaptation_state("Baseline"),
            Some(AdaptationState::Baseline)
        );
    }

    #[test]
    fn parse_adaptation_state_fatigue_state() {
        assert_eq!(
            parse_adaptation_state("FatigueState"),
            Some(AdaptationState::FatigueState)
        );
    }

    #[test]
    fn parse_adaptation_state_vo2_expansion() {
        assert_eq!(
            parse_adaptation_state("Vo2Expansion"),
            Some(AdaptationState::Vo2Expansion)
        );
    }

    #[test]
    fn parse_adaptation_state_aerobic_consolidation() {
        assert_eq!(
            parse_adaptation_state("AerobicConsolidation"),
            Some(AdaptationState::AerobicConsolidation)
        );
    }

    #[test]
    fn parse_adaptation_state_anaerobic_build() {
        assert_eq!(
            parse_adaptation_state("AnaerobicBuild"),
            Some(AdaptationState::AnaerobicBuild)
        );
    }

    #[test]
    fn parse_adaptation_state_mixed_adaptation() {
        assert_eq!(
            parse_adaptation_state("MixedAdaptation"),
            Some(AdaptationState::MixedAdaptation)
        );
    }

    #[test]
    fn parse_adaptation_state_plateau() {
        assert_eq!(
            parse_adaptation_state("Plateau"),
            Some(AdaptationState::Plateau)
        );
    }

    #[test]
    fn parse_adaptation_state_unknown_returns_none() {
        assert_eq!(parse_adaptation_state("Unknown"), None);
    }

    #[test]
    fn parse_adaptation_state_empty_string_returns_none() {
        assert_eq!(parse_adaptation_state(""), None);
    }

    // -------------------------------------------------------------------------

    #[test]
    fn tsb_below_minus_20_creates_deep_fatigue_alert() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                ctl: Some(50.0),
                atl: Some(75.0),
                tsb: Some(-25.0),
                load_state: Some("fatigued".into()),
                ramp_rate: None,
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
                ..Default::default()
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
                ..Default::default()
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
                ramp_rate: None,
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
                ramp_rate: None,
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "fatigue"));
        assert!(!alerts.iter().any(|a| a.code == "deep_fatigue"));
    }

    #[test]
    fn fatigue_alert_fires_at_tsb_boundary_minus_10() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                ctl: Some(50.0),
                atl: Some(60.0),
                tsb: Some(TSB_FATIGUED),
                load_state: Some("fatigued".into()),
                ramp_rate: None,
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
                ..Default::default()
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
                ..Default::default()
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
    fn low_race_readiness_creates_alert_and_guidance() {
        let metrics = CoachMetrics {
            race_readiness: Some(RaceReadinessMetrics {
                supported: true,
                readiness_score: Some(35),
                tsb_tier: "neutral".into(),
                durability_tier: "stable".into(),
                neural_tier: "balanced".into(),
                system_alignment: "aligned".into(),
                taper_quality: "detected_drop".into(),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);

        assert!(alerts.iter().any(|a| a.code == "low_race_readiness"));
        // Score 35 < 40 → Priority severity
        assert!(
            alerts
                .iter()
                .any(|a| a.code == "low_race_readiness"
                    && a.severity == CoachAlertSeverity::Priority)
        );
        assert!(guidance.suggestions.iter().any(|s| s.contains("taper")));
    }

    #[test]
    fn high_race_readiness_does_not_create_alert() {
        let metrics = CoachMetrics {
            race_readiness: Some(RaceReadinessMetrics {
                supported: true,
                readiness_score: Some(85),
                tsb_tier: "fresh".into(),
                durability_tier: "stable".into(),
                neural_tier: "balanced".into(),
                system_alignment: "aligned".into(),
                taper_quality: "optimal".into(),
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(!alerts.iter().any(|a| a.code == "low_race_readiness"));
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

    #[test]
    fn hrv_below_range_triggers_alert() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.2),
                avg_resting_hr: Some(54.0),
                avg_hrv: Some(40.0),
                hrv_baseline: Some(52.0),
                hrv_deviation_pct: Some(-23.1),
                hrv_trend_state: Some("below_range".into()),
                wellness_days_count: 5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "low_hrv"));
    }

    #[test]
    fn hrv_alert_falls_back_when_deviation_missing() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.2),
                avg_resting_hr: Some(54.0),
                avg_hrv: Some(40.0),
                hrv_trend_state: Some("suppressed".into()),
                hrv_baseline: None,
                hrv_deviation_pct: None,
                wellness_days_count: 5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "low_hrv"));
        let alert = alerts.iter().find(|a| a.code == "low_hrv").unwrap();
        assert_eq!(
            alert.evidence[0],
            "HRV is below the athlete's recent personal range"
        );
    }

    #[test]
    fn acwr_watch_creates_watch_alert() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                acwr: Some(AcwrMetrics {
                    acute_load: 370.0,
                    chronic_load: 270.0,
                    ratio: 1.37,
                    state: "watch".into(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "acwr_watch"));
        assert!(!alerts.iter().any(|a| a.code == "acwr_overreaching"));
    }

    #[test]
    fn ndli_overload_creates_priority_alert() {
        let metrics = CoachMetrics {
            ndli: Some(NdliMetrics {
                supported: true,
                high_intensity_days_7d: 5,
                ndli_overload_flag: true,
                ndli_state: "red".into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "ndli_overload"));
        assert!(!alerts.iter().any(|a| a.code == "ndli_elevated"));
    }

    #[test]
    fn ndli_elevated_creates_caution_alert() {
        let metrics = CoachMetrics {
            ndli: Some(NdliMetrics {
                supported: true,
                high_intensity_days_7d: 3,
                ndli_overload_flag: false,
                ndli_state: "amber".into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "ndli_elevated"));
        assert!(!alerts.iter().any(|a| a.code == "ndli_overload"));
    }

    #[test]
    fn heat_high_creates_alert() {
        let metrics = CoachMetrics {
            heat: Some(HeatMetrics {
                supported: true,
                heat_index_7d: Some(1.8),
                heat_state: "high".into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "heat_stress_elevated"));
        assert!(!alerts.iter().any(|a| a.code == "heat_stress_moderate"));
    }

    #[test]
    fn heat_moderate_creates_info_alert() {
        let metrics = CoachMetrics {
            heat: Some(HeatMetrics {
                supported: true,
                heat_index_7d: Some(1.2),
                heat_state: "moderate".into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "heat_stress_moderate"));
        assert!(!alerts.iter().any(|a| a.code == "heat_stress_elevated"));
    }

    #[test]
    fn wdrm_high_depletion_creates_alert() {
        let metrics = CoachMetrics {
            wdrm: Some(WdrMetrics {
                supported: true,
                depletion_pct: Some(0.72),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "high_wbal_depletion"));
    }

    #[test]
    fn durability_drifting_creates_alert() {
        let metrics = CoachMetrics {
            workout: Some(WorkoutMetricsContext {
                aerobic_decoupling: Some(DecouplingMetrics {
                    signed_decoupling_pct: 5.2,
                    durability_state: "drifting".into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "durability_drifting"));
        assert!(!alerts.iter().any(|a| a.code == "durability_improving"));
    }

    #[test]
    fn durability_improving_creates_info_alert() {
        let metrics = CoachMetrics {
            workout: Some(WorkoutMetricsContext {
                aerobic_decoupling: Some(DecouplingMetrics {
                    signed_decoupling_pct: -3.1,
                    durability_state: "improving".into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "durability_improving"));
        assert!(!alerts.iter().any(|a| a.code == "durability_drifting"));
    }

    #[test]
    fn polarised_state_creates_confirmation_alert() {
        let metrics = CoachMetrics {
            polarisation: Some(PolarisationMetrics {
                z1_pct: Some(0.55),
                z2_pct: Some(0.30),
                z3_pct: Some(0.15),
                ratio: Some(1.17),
                state: Some("polarised".into()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.iter().any(|a| a.code == "polarized_confirmation"));
        assert!(
            !alerts
                .iter()
                .any(|a| a.code == "threshold_biased_polarisation")
        );
    }

    #[test]
    fn no_alerts_when_all_metrics_normal() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                tsb: Some(5.0),
                ..Default::default()
            }),
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(7.5),
                avg_resting_hr: Some(52.0),
                avg_hrv: Some(62.0),
                hrv_trend_state: Some("within_range".into()),
                wellness_days_count: 5,
                ..Default::default()
            }),
            volume: Some(VolumeMetrics {
                weekly_avg_hours: 10.0,
                ..Default::default()
            }),
            load_management: Some(LoadManagementMetrics {
                acwr: Some(AcwrMetrics {
                    acute_load: 280.0,
                    chronic_load: 260.0,
                    ratio: 1.08,
                    state: "productive".into(),
                }),
                monotony: Some(1.8),
                fatigue_index: Some(1.5),
                durability_index: Some(0.95),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        assert!(alerts.is_empty());
    }

    #[test]
    fn fresh_tsb_without_wellness_provides_incomplete_finding() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                tsb: Some(14.0),
                ..Default::default()
            }),
            wellness: None,
            ..Default::default()
        };

        let guidance = build_guidance(&metrics, &[]);
        assert!(
            guidance
                .findings
                .iter()
                .any(|f| f.contains("wellness support is incomplete"))
        );
    }

    #[test]
    fn balanced_tsb_adds_maintenance_finding_without_recovery_prompt() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                tsb: Some(3.0),
                ..Default::default()
            }),
            ..Default::default()
        };

        let guidance = build_guidance(&metrics, &[]);
        assert!(
            guidance
                .findings
                .iter()
                .any(|f| f.contains("balanced range"))
        );
        assert!(!guidance.suggestions.iter().any(|s| s.contains("recovery")));
        assert!(!guidance.suggestions.iter().any(|s| s.contains("ready")));
    }

    #[test]
    fn low_volume_adds_suggestion() {
        let metrics = CoachMetrics {
            volume: Some(VolumeMetrics {
                weekly_avg_hours: 3.5,
                ..Default::default()
            }),
            ..Default::default()
        };

        let guidance = build_guidance(&metrics, &[]);
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("low-volume"))
        );
    }

    #[test]
    fn acwr_watch_guidance_suggests_monitoring() {
        let metrics = CoachMetrics {
            load_management: Some(LoadManagementMetrics {
                acwr: Some(AcwrMetrics {
                    acute_load: 370.0,
                    chronic_load: 270.0,
                    ratio: 1.37,
                    state: "watch".into(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(alerts.iter().any(|a| a.code == "acwr_watch"));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("monitor recovery"))
        );
    }

    #[test]
    fn ndli_overload_guidance() {
        let metrics = CoachMetrics {
            ndli: Some(NdliMetrics {
                supported: true,
                high_intensity_days_7d: 5,
                ndli_overload_flag: true,
                ndli_state: "red".into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(guidance.findings.iter().any(|f| f.contains("neural")));
        assert!(guidance.suggestions.iter().any(|s| s.contains("neural")));
    }

    #[test]
    fn ndli_elevated_guidance() {
        let metrics = CoachMetrics {
            ndli: Some(NdliMetrics {
                supported: true,
                high_intensity_days_7d: 3,
                ndli_overload_flag: false,
                ndli_state: "amber".into(),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(
            guidance
                .findings
                .iter()
                .any(|f| f.contains("high-intensity"))
        );
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("high-intensity"))
        );
    }

    #[test]
    fn durability_drifting_guidance() {
        let metrics = CoachMetrics {
            workout: Some(WorkoutMetricsContext {
                aerobic_decoupling: Some(DecouplingMetrics {
                    signed_decoupling_pct: 5.2,
                    durability_state: "drifting".into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(guidance.findings.iter().any(|f| f.contains("decoupling")));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("durability"))
        );
    }

    #[test]
    fn durability_improving_guidance() {
        let metrics = CoachMetrics {
            workout: Some(WorkoutMetricsContext {
                aerobic_decoupling: Some(DecouplingMetrics {
                    signed_decoupling_pct: -3.1,
                    durability_state: "improving".into(),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(guidance.findings.iter().any(|f| f.contains("improving")));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("training approach"))
        );
    }

    #[test]
    fn wdrm_high_depletion_guidance() {
        let metrics = CoachMetrics {
            wdrm: Some(WdrMetrics {
                supported: true,
                depletion_pct: Some(0.72),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(guidance.findings.iter().any(|f| f.contains("W′ reserves")));
        assert!(
            guidance
                .suggestions
                .iter()
                .any(|s| s.contains("W′ capacity"))
        );
    }

    #[test]
    fn workout_with_intervals_adds_suggestion() {
        let metrics = CoachMetrics {
            workout: Some(WorkoutMetricsContext {
                interval_count: Some(6),
                ..Default::default()
            }),
            ..Default::default()
        };

        let guidance = build_guidance(&metrics, &[]);
        assert!(guidance.suggestions.iter().any(|s| s.contains("intervals")));
    }

    #[test]
    fn deep_fatigue_adds_next_action() {
        let metrics = CoachMetrics {
            fitness: Some(FitnessMetrics {
                tsb: Some(-25.0),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(
            guidance
                .next_actions
                .iter()
                .any(|a| a.contains("recovery-focused"))
        );
    }

    #[test]
    fn default_next_action_when_none_specified() {
        let metrics = CoachMetrics::default();
        let guidance = build_guidance(&metrics, &[]);
        assert!(
            guidance
                .next_actions
                .iter()
                .any(|a| a.contains("follow-up"))
        );
    }

    #[test]
    fn tier_one_recovery_adds_fallback_suggestion_when_no_other_suggestions() {
        let metrics = CoachMetrics {
            wellness: Some(WellnessMetrics {
                avg_sleep_hours: Some(5.8),
                ..Default::default()
            }),
            ..Default::default()
        };

        let alerts = build_alerts(&metrics);
        let guidance = build_guidance(&metrics, &alerts);
        assert!(alerts.iter().any(|a| a.code == "low_sleep"));
        assert!(guidance.suggestions.iter().any(|s| s.contains("recovery")));
    }
}
