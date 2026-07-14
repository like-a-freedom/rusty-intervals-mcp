use crate::domains::coach::{DecouplingMetrics, EspePowerAnchors, FitnessMetrics};
use crate::engines::coach_metrics::*;
use intervals_icu_client::ActivitySummary;
use serde_json::{Value, json};
use std::collections::HashMap;

fn wellness_entry(sleep_secs: f64, resting_hr: f64, hrv: f64) -> Value {
    json!({
        "sleepSecs": sleep_secs,
        "restingHR": resting_hr,
        "hrv": hrv
    })
}

#[test]
fn weekly_avg_hours_is_computed_from_total_time_and_window() {
    let metrics = derive_volume_metrics(7, 28_800, 42_000.0, 1_200.0, 4);

    assert_eq!(metrics.weekly_avg_hours, 8.0);
}

#[test]
fn tsb_below_minus_20_is_classified_as_fatigued() {
    let fitness = interpret_fitness_metrics(Some(50.0), Some(70.0), Some(-25.0), None);

    assert_eq!(fitness.load_state.as_deref(), Some("fatigued"));
}

#[test]
fn tsb_between_minus_10_and_10_is_classified_as_balanced() {
    let fitness = interpret_fitness_metrics(Some(50.0), Some(47.0), Some(3.0), None);

    assert_eq!(fitness.load_state.as_deref(), Some("balanced"));
}

#[test]
fn empty_fitness_values_stay_optional() {
    let fitness = FitnessMetrics::default();

    assert!(fitness.ctl.is_none());
    assert!(fitness.atl.is_none());
    assert!(fitness.tsb.is_none());
}

#[test]
fn trend_metrics_compute_percentage_deltas() {
    let trend = derive_trend_metrics(
        TrendSnapshot {
            activity_count: 4,
            total_time_secs: 28_800,
            total_distance_m: 42_000.0,
            total_elevation_m: 1500.0,
        },
        TrendSnapshot {
            activity_count: 3,
            total_time_secs: 21_600,
            total_distance_m: 35_000.0,
            total_elevation_m: 1200.0,
        },
    );

    assert_eq!(trend.activity_count_delta, Some(1));
    assert!(trend.time_delta_pct.unwrap() > 30.0);
    assert!(trend.distance_delta_pct.unwrap() > 15.0);
}

#[test]
fn build_trend_snapshot_aggregates_activity_details() {
    let activities = [
        ActivitySummary {
            id: "a1".into(),
            name: Some("Run 1".into()),
            start_date_local: "2026-03-01".into(),
            ..Default::default()
        },
        ActivitySummary {
            id: "a2".into(),
            name: Some("Run 2".into()),
            start_date_local: "2026-03-02".into(),
            ..Default::default()
        },
    ];
    let refs = activities.iter().collect::<Vec<_>>();
    let details = HashMap::from([
        (
            "a1".to_string(),
            json!({"moving_time": 3600, "distance": 10000.0, "total_elevation_gain": 100.0}),
        ),
        (
            "a2".to_string(),
            json!({"moving_time": 5400, "distance": 15000.0, "total_elevation_gain": 250.0}),
        ),
    ]);

    let snapshot = build_trend_snapshot(&refs, &details);

    assert_eq!(snapshot.activity_count, 2);
    assert_eq!(snapshot.total_time_secs, 9000);
    assert_eq!(snapshot.total_distance_m, 25_000.0);
    assert_eq!(snapshot.total_elevation_m, 350.0);
}

#[test]
fn parse_fitness_metrics_supports_summary_payload() {
    let payload = json!([{"fitness": 50.0, "fatigue": 70.0, "form": -20.0}]);

    let metrics = parse_fitness_metrics(Some(&payload)).unwrap();
    assert_eq!(metrics.ctl, Some(50.0));
    assert_eq!(metrics.atl, Some(70.0));
    assert_eq!(metrics.tsb, Some(-20.0));
}

#[test]
fn parse_fitness_metrics_extracts_ramp_rate() {
    let payload = json!([{"fitness": 50.0, "fatigue": 70.0, "form": -20.0, "rampRate": 2.5}]);

    let metrics = parse_fitness_metrics(Some(&payload)).unwrap();
    assert_eq!(metrics.ramp_rate, Some(2.5));
}

#[test]
fn parse_wellness_metrics_supports_seconds_and_snake_case() {
    let payload = json!([
        {"sleepSecs": 25200.0, "restingHR": 50.0, "hrv": 60.0},
        {"sleep_hours": 8.0, "resting_hr": 52.0, "hrv": 66.0}
    ]);

    let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
    assert!(metrics.avg_sleep_hours.unwrap() > 7.4);
    assert_eq!(metrics.wellness_days_count, 2);
    assert!(metrics.recovery_index.unwrap() > 1.0);
}

#[test]
fn parse_wellness_metrics_derives_adaptive_hrv_baseline_and_recent_deviation() {
    let mut entries = Vec::new();
    entries.extend((0..28).map(|_| wellness_entry(28_800.0, 50.0, 80.0)));
    entries.extend((0..7).map(|_| wellness_entry(25_200.0, 55.0, 64.0)));

    let payload = Value::Array(entries);

    let metrics = parse_wellness_metrics(Some(&payload)).unwrap();

    assert_eq!(metrics.avg_sleep_hours, Some(7.0));
    assert_eq!(metrics.avg_resting_hr, Some(55.0));
    assert_eq!(metrics.avg_hrv, Some(64.0));
    assert_eq!(metrics.wellness_days_count, 7);
    assert_eq!(metrics.hrv_baseline, Some(80.0));
    assert_eq!(metrics.resting_hr_baseline, Some(50.0));
    assert_eq!(metrics.hrv_deviation_pct, Some(-20.0));
    assert_eq!(metrics.hrv_trend_state.as_deref(), Some("suppressed"));
}

#[test]
fn parse_api_load_snapshot_supports_wellness_load_fields() {
    let payload = json!({"atlLoad": 432.0, "ctlLoad": 360.0});

    let metrics = parse_api_load_snapshot(Some(&payload)).unwrap();

    assert_eq!(metrics.acute_load, 432.0);
    assert_eq!(metrics.chronic_load, 360.0);
    assert!((metrics.ratio - 1.2).abs() < 0.001);
}

#[test]
fn parse_api_load_snapshot_supports_activity_load_fields() {
    let payload = json!({"icu_atl": 510.0, "icu_ctl": 400.0});

    let metrics = parse_api_load_snapshot(Some(&payload)).unwrap();

    assert_eq!(metrics.acute_load, 510.0);
    assert_eq!(metrics.chronic_load, 400.0);
    assert_eq!(metrics.state, "productive");
}

#[test]
fn parse_api_load_snapshot_supports_integer_load_fields() {
    let payload = json!({"atlLoad": 432, "ctlLoad": 360});

    let metrics = parse_api_load_snapshot(Some(&payload)).unwrap();

    assert_eq!(metrics.acute_load, 432.0);
    assert_eq!(metrics.chronic_load, 360.0);
}

#[test]
fn derive_execution_metrics_prefers_api_values_over_stream_fallbacks() {
    let detail = json!({
        "icu_efficiency_factor": 1.23,
        "decoupling": 4.0
    });
    let streams = json!({
        "heartrate": [100.0, 100.0, 100.0, 100.0],
        "watts": [100.0, 200.0, 300.0, 400.0]
    });

    let (efficiency_factor, decoupling) = derive_execution_metrics(Some(&detail), Some(&streams));

    assert_eq!(efficiency_factor, Some(1.23));
    assert_eq!(
        decoupling.as_ref().map(|metric| metric.decoupling_pct),
        Some(4.0)
    );
    assert_eq!(
        decoupling.as_ref().map(|metric| metric.state.as_str()),
        Some("acceptable")
    );
}

#[test]
fn acwr_ewma_marks_ratio_above_1_5_as_overreaching() {
    let loads = vec![
        10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0,
        10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0, 150.0,
    ];

    let acwr = compute_acwr(&loads).unwrap();
    assert_eq!(acwr.state, "overreaching");
    assert!(acwr.ratio > 1.5);
}

#[test]
fn acwr_maps_productive_and_underloaded_states() {
    let productive = compute_acwr(&vec![40.0; 28]).unwrap();
    assert_eq!(productive.state, "productive");

    let mut underloaded_loads = vec![50.0; 21];
    underloaded_loads.extend(vec![0.0; 7]);
    let underloaded = compute_acwr(&underloaded_loads).unwrap();
    assert_eq!(underloaded.state, "underloaded");
}

#[test]
fn monotony_is_mean_divided_by_standard_deviation() {
    let loads = [10.0, 10.0, 20.0, 20.0, 30.0, 30.0, 40.0];

    let monotony = compute_monotony(&loads).unwrap();

    assert!((monotony - 2.22).abs() < 0.05);
}

#[test]
fn monotony_capped_at_10_for_identical_loads() {
    let loads = [25.0; 7];
    let monotony = compute_monotony(&loads).unwrap();
    assert_eq!(monotony, 10.0);
}

#[test]
fn strain_is_weekly_total_multiplied_by_monotony() {
    let loads = [10.0, 10.0, 20.0, 20.0, 30.0, 30.0, 40.0];
    let monotony = compute_monotony(&loads).unwrap();

    let strain = compute_strain(&loads, monotony);

    assert!((strain - (160.0 * monotony)).abs() < 0.01);
}

#[test]
fn recovery_index_is_hrv_divided_by_resting_hr() {
    let recovery_index = compute_recovery_index(72.0, 48.0, None, None).unwrap();

    assert!((recovery_index - 1.5).abs() < 0.001);
}

#[test]
fn recovery_index_compares_recent_hrv_and_rhr_to_personal_baseline() {
    let recovery_index = compute_recovery_index(64.0, 55.0, Some(80.0), Some(50.0)).unwrap();

    assert!((recovery_index - 0.727).abs() < 0.01);
}

#[test]
fn load_management_metrics_require_sufficient_lookback_for_acwr() {
    let loads = vec![25.0; 14];

    let metrics = compute_load_management_metrics(&loads, None).unwrap();

    assert_eq!(metrics.acwr, None);
    assert_eq!(metrics.monotony, Some(10.0));
    assert_eq!(metrics.strain, Some(175.0 * 10.0));
    assert_eq!(metrics.fatigue_index, None);
    assert!((metrics.stress_tolerance.unwrap() - 1.75).abs() < 0.01);
}

#[test]
fn acwr_returns_none_for_short_lookback() {
    let loads = vec![40.0; 14];

    assert!(compute_acwr(&loads).is_none());
}

#[test]
fn efficiency_factor_is_mean_output_divided_by_mean_heart_rate() {
    let hr = [140.0, 142.0, 144.0, 146.0];
    let output = [220.0, 224.0, 228.0, 232.0];

    let efficiency_factor = compute_efficiency_factor(&hr, &output).unwrap();

    assert!((efficiency_factor - 1.58).abs() < 0.01);
}

#[test]
fn aerobic_decoupling_above_five_percent_creates_watch_signal() {
    let hr = [140.0, 141.0, 142.0, 150.0, 151.0, 152.0];
    let output = [220.0, 220.0, 220.0, 220.0, 220.0, 220.0];

    let metrics = compute_aerobic_decoupling(&hr, &output).unwrap();

    assert!(metrics.decoupling_pct > 5.0);
    assert_eq!(metrics.state, "watch");
}

#[test]
fn aerobic_decoupling_returns_none_for_mismatched_stream_lengths() {
    let hr = [140.0, 141.0, 142.0, 150.0];
    let output = [220.0, 220.0, 220.0];

    assert_eq!(
        compute_aerobic_decoupling(&hr, &output),
        None::<DecouplingMetrics>
    );
}

#[test]
fn polarisation_ratio_classifies_threshold_biased() {
    let m = compute_polarisation(0.50, 0.45, 0.05).unwrap();
    assert!(m.ratio.unwrap() < 0.75);
    assert_eq!(m.state.as_deref(), Some("threshold_biased"));
}

#[test]
fn polarisation_ratio_classifies_polarised() {
    let m = compute_polarisation(0.50, 0.35, 0.15).unwrap();
    assert!(m.ratio.unwrap() > 0.75);
    assert!(m.ratio.unwrap() <= 1.0);
    assert_eq!(m.state.as_deref(), Some("polarised"));
}

#[test]
fn polarisation_ratio_classifies_high_intensity_dominant() {
    let m = compute_polarisation(0.50, 0.10, 0.40).unwrap();
    assert!(m.ratio.unwrap() > 1.0);
    assert_eq!(m.state.as_deref(), Some("high_intensity_dominant"));
}

#[test]
fn polarisation_returns_none_ratio_when_z2_is_zero() {
    let m = compute_polarisation(0.50, 0.0, 0.50).unwrap();
    assert_eq!(m.ratio, None);
    assert_eq!(m.state, None);
}

#[test]
fn polarisation_preserves_input_percentages() {
    let m = compute_polarisation(0.70, 0.20, 0.10).unwrap();
    assert!((m.z1_pct.unwrap() - 0.70).abs() < f64::EPSILON);
    assert!((m.z2_pct.unwrap() - 0.20).abs() < f64::EPSILON);
    assert!((m.z3_pct.unwrap() - 0.10).abs() < f64::EPSILON);
}

#[test]
fn parse_polarisation_from_api_uses_polarization_index() {
    let detail = json!({"polarization_index": 0.85});
    let m = parse_polarisation_from_api(Some(&detail), None).unwrap();
    assert_eq!(m.ratio, Some(0.85));
    assert_eq!(m.state.as_deref(), Some("polarised"));
}

#[test]
fn parse_polarisation_from_api_aggregates_5_zone_times() {
    let zone_times = json!([
        {"id": "Z1", "secs": 3600},
        {"id": "Z2", "secs": 3000},
        {"id": "Z3", "secs": 600},
        {"id": "Z4", "secs": 500},
        {"id": "Z5", "secs": 300}
    ]);
    let m = parse_polarisation_from_api(None, Some(&zone_times)).unwrap();
    assert!(m.ratio.unwrap() > 1.0);
    assert_eq!(m.state.as_deref(), Some("high_intensity_dominant"));
}

#[test]
fn parse_polarisation_from_api_aggregates_5_zone_polarised() {
    let zone_times = json!([
        {"id": "Z1", "secs": 3000},
        {"id": "Z2", "secs": 2000},
        {"id": "Z3", "secs": 3500},
        {"id": "Z4", "secs": 1000},
        {"id": "Z5", "secs": 500}
    ]);
    let m = parse_polarisation_from_api(None, Some(&zone_times)).unwrap();
    assert!(m.ratio.unwrap() > 0.75);
    assert!(m.ratio.unwrap() <= 1.0);
    assert_eq!(m.state.as_deref(), Some("polarised"));
}

#[test]
fn parse_polarisation_from_api_returns_none_when_no_data() {
    assert!(parse_polarisation_from_api(None, None).is_none());
}

#[test]
fn consistency_index_full_adherence() {
    let m = compute_consistency_index(10, 10);
    assert_eq!(m.ratio, Some(1.0));
    assert_eq!(m.state.as_deref(), Some("excellent"));
}

#[test]
fn consistency_index_good_adherence() {
    let m = compute_consistency_index(8, 10);
    assert_eq!(m.ratio, Some(0.8));
    assert_eq!(m.state.as_deref(), Some("good"));
}

#[test]
fn consistency_index_moderate_adherence() {
    let m = compute_consistency_index(5, 10);
    assert_eq!(m.ratio, Some(0.5));
    assert_eq!(m.state.as_deref(), Some("moderate"));
}

#[test]
fn consistency_index_low_adherence() {
    let m = compute_consistency_index(3, 10);
    assert_eq!(m.ratio, Some(0.3));
    assert_eq!(m.state.as_deref(), Some("low"));
}

#[test]
fn consistency_index_no_plans_returns_none_ratio() {
    let m = compute_consistency_index(0, 0);
    assert_eq!(m.ratio, None);
    assert_eq!(m.state, None);
}

#[test]
fn fatigue_index_is_load_7d_divided_by_recovery_index() {
    let fi = compute_fatigue_index(175.0, 1.4).unwrap();
    assert!((fi - 125.0).abs() < 0.01);
}

#[test]
fn fatigue_index_returns_none_when_recovery_index_is_zero() {
    assert!(compute_fatigue_index(175.0, 0.0).is_none());
}

#[test]
fn stress_tolerance_is_strain_over_monotony_divided_by_100() {
    let st = compute_stress_tolerance(450.0, 2.0).unwrap();
    assert!((st - 2.25).abs() < 0.01);
}

#[test]
fn stress_tolerance_returns_none_when_monotony_is_zero() {
    assert!(compute_stress_tolerance(450.0, 0.0).is_none());
}

#[test]
fn readiness_score_computes_weighted_average() {
    let rs = compute_readiness_score(Some(8.0), Some(7.5), Some(5.0), Some(4.0)).unwrap();
    assert!((rs - 6.45).abs() < 0.01);
}

#[test]
fn readiness_score_returns_none_when_all_inputs_are_none() {
    assert!(compute_readiness_score(None, None, None, None).is_none());
}

#[test]
fn readiness_score_returns_none_for_partial_inputs() {
    assert!(compute_readiness_score(Some(8.0), None, Some(5.0), None).is_none());
    assert!(compute_readiness_score(Some(8.0), Some(7.5), None, None).is_none());
    assert!(compute_readiness_score(None, Some(7.5), Some(5.0), Some(4.0)).is_none());
}

#[test]
fn readiness_score_normalizes_sleep_hours_over_10() {
    let rs = compute_readiness_score(Some(8.0), Some(8.0), Some(5.0), Some(4.0)).unwrap();
    assert!((rs - 6.6).abs() < 0.01);
}

#[test]
fn readiness_score_clamps_sleep_above_10_hours() {
    let rs = compute_readiness_score(Some(8.0), Some(12.0), Some(5.0), Some(4.0)).unwrap();
    assert!((rs - 7.2).abs() < 0.01);
}

#[test]
fn load_management_metrics_computes_fatigue_index_and_stress_tolerance() {
    let loads = vec![25.0; 28];
    let metrics = compute_load_management_metrics(&loads, Some(1.5)).unwrap();
    assert!(metrics.fatigue_index.is_some());
    assert!(metrics.stress_tolerance.is_some());
}

#[test]
fn load_management_metrics_without_recovery_index_has_no_fatigue_index() {
    let loads = vec![25.0; 28];
    let metrics = compute_load_management_metrics(&loads, None).unwrap();
    assert!(metrics.fatigue_index.is_none());
    assert!(metrics.stress_tolerance.is_some());
}

#[test]
fn parse_wellness_metrics_extracts_mood_stress_fatigue() {
    let payload = json!([
        {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0, "mood": 8.0, "stress": 5.0, "fatigue": 4.0},
        {"sleep_hours": 7.5, "resting_hr": 51.0, "hrv": 63.0, "mood": 7.0, "stress": 6.0, "fatigue": 5.0}
    ]);
    let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
    assert_eq!(metrics.avg_mood, Some(7.5));
    assert_eq!(metrics.avg_stress, Some(5.5));
    assert_eq!(metrics.avg_fatigue, Some(4.5));
    assert!(metrics.readiness_score.is_some());
}

#[test]
fn parse_wellness_metrics_uses_api_readiness_as_primary() {
    let payload = json!([
        {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0, "readiness": 7.5}
    ]);
    let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
    assert_eq!(metrics.readiness_score, Some(7.5));
}

#[test]
fn parse_wellness_metrics_readiness_falls_back_to_formula_when_api_missing() {
    let payload = json!([
        {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0, "mood": 8.0, "stress": 3.0, "fatigue": 2.0}
    ]);
    let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
    assert!(metrics.readiness_score.is_some());
    let rs = metrics.readiness_score.unwrap();
    let expected = 8.0 * 0.3 + 8.0 * 0.3 + 3.0 * 0.2 + 2.0 * 0.2;
    assert!((rs - expected).abs() < 0.01);
}

#[test]
fn parse_wellness_metrics_readiness_requires_api_or_all_components() {
    let payload = json!([
        {"sleep_hours": 8.0, "resting_hr": 50.0, "hrv": 65.0}
    ]);
    let metrics = parse_wellness_metrics(Some(&payload)).unwrap();
    assert!(metrics.avg_mood.is_none());
    assert!(metrics.avg_stress.is_none());
    assert!(metrics.avg_fatigue.is_none());
    assert!(metrics.readiness_score.is_none());
}

#[test]
fn durability_index_is_current_divided_by_baseline() {
    let di = compute_durability_index(295.0, 310.0).unwrap();
    assert!((di - 0.952).abs() < 0.001);
}

#[test]
fn durability_index_returns_none_when_baseline_is_zero() {
    assert!(compute_durability_index(295.0, 0.0).is_none());
}

#[test]
fn durability_index_returns_none_when_baseline_is_negative() {
    assert!(compute_durability_index(295.0, -310.0).is_none());
}

#[test]
fn durability_index_handles_zero_current() {
    let di = compute_durability_index(0.0, 310.0).unwrap();
    assert!((di - 0.0).abs() < f64::EPSILON);
}

#[test]
fn extract_sportinfo_anchors_from_wellness_array() {
    let payload = json!([
        {"type": "Ride", "eftp": 250.0, "wPrime": 20000.0, "pMax": 800.0},
        {"type": "Run", "eftp": null, "wPrime": null, "pMax": null}
    ]);
    let anchors = extract_sportinfo_anchors(Some(&payload));
    assert!(anchors.supported);
    assert_eq!(anchors.eftp, Some(250.0));
    assert_eq!(anchors.w_prime, Some(20000.0));
    assert_eq!(anchors.p_max, Some(800.0));
    assert_eq!(anchors.source, "sportinfo");
}

#[test]
fn extract_sportinfo_anchors_unsupported_when_empty() {
    let payload = json!([]);
    let anchors = extract_sportinfo_anchors(Some(&payload));
    assert!(!anchors.supported);
    assert_eq!(anchors.source, "none");
}

#[test]
fn extract_sportinfo_anchors_unsupported_when_null() {
    let anchors = extract_sportinfo_anchors(None);
    assert!(!anchors.supported);
}

#[test]
fn extract_sportinfo_anchors_multi_sport_first_non_null_wins() {
    let payload = json!([
        {"type": "Ride", "eftp": null, "wPrime": 18000.0, "pMax": null},
        {"type": "Run", "eftp": 280.0, "wPrime": null, "pMax": 900.0}
    ]);
    let anchors = extract_sportinfo_anchors(Some(&payload));
    assert!(anchors.supported);
    assert_eq!(anchors.eftp, Some(280.0));
    assert_eq!(anchors.w_prime, Some(18000.0));
    assert_eq!(anchors.p_max, Some(900.0));
}

#[test]
fn enrich_anchors_from_activity_pm_fallback() {
    let mut anchors = EspePowerAnchors::unsupported();
    let detail = json!({
        "icu_pm_ftp": 265.0,
        "icu_pm_w_prime": 22000.0,
        "icu_pm_p_max": 850.0
    });
    enrich_anchors_from_activity(&mut anchors, Some(&detail));
    assert!(anchors.supported);
    assert_eq!(anchors.eftp, Some(265.0));
    assert_eq!(anchors.w_prime, Some(22000.0));
    assert_eq!(anchors.p_max, Some(850.0));
    assert_eq!(anchors.source, "activity");
}

#[test]
fn enrich_anchors_does_not_overwrite_existing_values() {
    let mut anchors = EspePowerAnchors {
        eftp: Some(250.0),
        w_prime: None,
        p_max: None,
        source: "sportinfo".into(),
        supported: true,
    };
    let detail = json!({"icu_pm_ftp": 999.0, "icu_pm_w_prime": 99999.0});
    enrich_anchors_from_activity(&mut anchors, Some(&detail));
    assert_eq!(anchors.eftp, Some(250.0));
    assert_eq!(anchors.w_prime, Some(99999.0));
}

#[test]
fn derive_espe_metrics_computes_glycolytic_bias() {
    let anchors = EspePowerAnchors {
        eftp: Some(250.0),
        p_max: Some(800.0),
        ..Default::default()
    };
    let derived = derive_espe_metrics(&anchors, Some(600.0), None, Some(300.0), None);
    assert!(derived.supported);
    assert!((derived.glycolytic_bias.unwrap() - 2.0).abs() < 0.01);
}

#[test]
fn derive_espe_metrics_unsupported_when_no_anchors() {
    let anchors = EspePowerAnchors::unsupported();
    let derived = derive_espe_metrics(&anchors, None, None, None, None);
    assert!(!derived.supported);
    assert!(derived.glycolytic_bias.is_none());
}

#[test]
fn compute_wdr_metrics_from_wbal_intervals() {
    let intervals = json!([
        {"wbal_start": 20000.0, "wbal_end": 15000.0},
        {"wbal_start": 15000.0, "wbal_end": 8000.0}
    ]);
    let wdrm = compute_wdr_metrics(Some(&intervals), None, Some(20000.0));
    assert!(wdrm.supported);
    assert_eq!(wdrm.max_wbal_depletion, Some(7000.0));
    assert!((wdrm.depletion_pct.unwrap() - 0.35).abs() < 0.01);
}

#[test]
fn compute_wdr_metrics_unsupported_for_null_wbal() {
    let intervals = json!([
        {"moving_time": 300, "average_heartrate": 140.0}
    ]);
    let wdrm = compute_wdr_metrics(Some(&intervals), None, None);
    assert!(!wdrm.supported);
}

#[test]
fn compute_wdr_metrics_fallback_to_icu_max_wbal_depletion() {
    let detail = json!({
        "icu_max_wbal_depletion": 12000.0,
        "icu_joules_above_ftp": 45000.0
    });
    let wdrm = compute_wdr_metrics(None, Some(&detail), Some(20000.0));
    assert!(wdrm.supported);
    assert_eq!(wdrm.max_wbal_depletion, Some(12000.0));
    assert!((wdrm.depletion_pct.unwrap() - 0.60).abs() < 0.01);
    assert_eq!(wdrm.joules_above_ftp, Some(45000.0));
}

#[test]
fn compute_wdr_metrics_clips_depletion_at_150_pct() {
    let detail = json!({"icu_max_wbal_depletion": 30000.0});
    let wdrm = compute_wdr_metrics(None, Some(&detail), Some(10000.0));
    assert!(wdrm.supported);
    assert!((wdrm.depletion_pct.unwrap() - 1.5).abs() < f64::EPSILON);
}

#[test]
fn classify_durability_state_stable() {
    assert_eq!(classify_durability_state(1.0, 1.0), "stable");
    assert_eq!(classify_durability_state(-1.0, 1.0), "stable");
    assert_eq!(classify_durability_state(3.0, 3.0), "stable");
}

#[test]
fn classify_durability_state_improving() {
    assert_eq!(classify_durability_state(-5.0, 5.0), "improving");
    assert_eq!(classify_durability_state(-3.5, 3.5), "improving");
}

#[test]
fn classify_durability_state_drifting() {
    assert_eq!(classify_durability_state(6.0, 6.0), "drifting");
    assert_eq!(classify_durability_state(4.0, 9.0), "drifting");
    assert_eq!(classify_durability_state(2.0, 10.0), "drifting");
}

#[test]
fn classify_durability_state_watch() {
    assert_eq!(classify_durability_state(4.0, 4.0), "watch");
    assert_eq!(classify_durability_state(3.5, 3.5), "watch");
}

#[test]
fn signed_decoupling_positive_is_drifting() {
    let hr = [140.0, 141.0, 142.0, 150.0, 151.0, 152.0];
    let output = [220.0, 220.0, 220.0, 220.0, 220.0, 220.0];
    let metrics = compute_aerobic_decoupling(&hr, &output).unwrap();
    assert!(metrics.signed_decoupling_pct > 0.0);
    assert_eq!(metrics.durability_state, "drifting");
}

#[test]
fn signed_decoupling_negative_is_improving() {
    let hr = [150.0, 151.0, 152.0, 140.0, 141.0, 142.0];
    let output = [220.0, 220.0, 220.0, 220.0, 220.0, 220.0];
    let metrics = compute_aerobic_decoupling(&hr, &output).unwrap();
    assert!(metrics.signed_decoupling_pct < 0.0);
    assert_eq!(metrics.durability_state, "improving");
}

#[test]
fn compute_z2_hr_variance_returns_some() {
    let hr = vec![
        120.0, 122.0, 124.0, 126.0, 128.0, 130.0, 132.0, 134.0, 136.0, 138.0,
    ];
    let variance = compute_z2_hr_variance(&hr, 120.0, 140.0);
    assert!(variance.is_some());
    assert!(variance.unwrap() > 0.0);
}

#[test]
fn compute_z2_hr_variance_returns_none_for_too_few_points() {
    let hr = vec![120.0, 122.0, 124.0];
    let variance = compute_z2_hr_variance(&hr, 120.0, 140.0);
    assert!(variance.is_none());
}

#[test]
fn compute_z2_hr_variance_returns_none_when_no_points_in_z2() {
    let hr = vec![150.0; 20];
    let variance = compute_z2_hr_variance(&hr, 120.0, 140.0);
    assert!(variance.is_none());
}

#[test]
fn compute_consistency_index_perfect() {
    let m = compute_consistency_index(5, 5);
    assert_eq!(m.ratio, Some(1.0));
    assert_eq!(m.state.as_deref(), Some("excellent"));
}

#[test]
fn compute_consistency_index_zero_planned() {
    let m = compute_consistency_index(0, 0);
    assert_eq!(m.ratio, None);
    assert_eq!(m.state, None);
}

#[test]
fn compute_consistency_index_above_100() {
    let m = compute_consistency_index(6, 5);
    assert_eq!(m.ratio, Some(1.2));
    assert_eq!(m.state.as_deref(), Some("excellent"));
}

#[test]
fn compute_load_management_empty() {
    let metrics = compute_load_management_metrics(&[], None);
    assert!(metrics.is_none());
}

#[test]
fn compute_ndli_7d_empty_returns_not_supported() {
    let metrics = compute_ndli_7d(&HashMap::new(), &[]);
    assert!(!metrics.supported);
}

#[test]
fn compute_heat_metrics_7d_empty_returns_default() {
    let metrics = compute_heat_metrics_7d(&HashMap::new(), &[]);
    assert!(!metrics.supported);
}

#[test]
fn compute_acwr_empty_returns_none() {
    assert!(compute_acwr(&[]).is_none());
}

#[test]
fn compute_monotony_empty_returns_none() {
    assert!(compute_monotony(&[]).is_none());
}

#[test]
fn compute_monotony_constant_load() {
    assert_eq!(compute_monotony(&[100.0; 7]), Some(10.0));
}

#[test]
fn extract_ctl_series_sorts_entries_by_date() {
    let payload = json!([
        {"date": "2026-01-03", "fitness": 62.0},
        {"date": "2026-01-01", "fitness": 60.0},
        {"date": "2026-01-02", "ctl": 61.0}
    ]);

    let (dates, values) = extract_ctl_series(Some(&payload)).unwrap();
    assert_eq!(dates, vec!["2026-01-01", "2026-01-02", "2026-01-03"]);
    assert_eq!(values, vec![60.0, 61.0, 62.0]);
}

#[test]
fn extract_hrv_series_sorts_entries_by_date() {
    let payload = json!([
        {"date": "2026-01-02", "hrv": 64.0},
        {"date": "2026-01-01", "hrv": 62.0}
    ]);

    let values = extract_hrv_series(Some(&payload)).unwrap();
    assert_eq!(values, vec![62.0, 64.0]);
}

#[test]
fn compute_lnrmssd_rollup_requires_seven_days() {
    let rollup = compute_lnrmssd_rollup(&[60.0, 61.0, 62.0]);
    assert!(!rollup.supported);
    assert_eq!(rollup.sample_count, 3);
}

#[test]
fn compute_tid_entropy_is_high_for_even_distribution() {
    let entropy = compute_tid_entropy(0.33, 0.34, 0.33).unwrap();
    assert!(entropy > 1.5);
}

#[test]
fn compute_wdr_7d_rollup_aggregates_across_activities() {
    let mut details = HashMap::new();
    details.insert("act1".into(), json!({"icu_max_wbal_depletion": 35000.0}));
    details.insert("act2".into(), json!({"icu_max_wbal_depletion": 12000.0}));
    details.insert("act3".into(), json!({"icu_max_wbal_depletion": 48000.0}));
    let ids: Vec<String> = vec!["act1".into(), "act2".into(), "act3".into()];

    let wdr = compute_wdr_7d_rollup(&details, &ids, Some(50000.0));

    assert!(wdr.supported);
    assert_eq!(wdr.sessions_with_data_7d, 3);
    let expected_mean = (0.70 + 0.24 + 0.96) / 3.0;
    let actual_mean = wdr.mean_depletion_pct_7d.unwrap();
    assert!((actual_mean - expected_mean).abs() < 0.01);
    assert_eq!(wdr.high_depletion_sessions_7d, 2);
}

#[test]
fn compute_wdr_7d_rollup_empty_ids_returns_unsupported() {
    let details = HashMap::new();
    let wdr = compute_wdr_7d_rollup(&details, &[], Some(50000.0));
    assert!(!wdr.supported);
    assert!(wdr.mean_depletion_pct_7d.is_none());
    assert_eq!(wdr.sessions_with_data_7d, 0);
}

#[test]
fn compute_wdr_7d_rollup_no_depletion_data_returns_unsupported() {
    let mut details = HashMap::new();
    details.insert("act1".into(), json!({"some_field": 42}));
    let wdr = compute_wdr_7d_rollup(&details, &["act1".into()], Some(50000.0));
    assert!(!wdr.supported);
    assert_eq!(wdr.sessions_with_data_7d, 0);
}

#[test]
fn compute_wdr_7d_rollup_without_w_prime_counts_sessions() {
    let mut details = HashMap::new();
    details.insert("act1".into(), json!({"icu_max_wbal_depletion": 35000.0}));
    let wdr = compute_wdr_7d_rollup(&details, &["act1".into()], None);
    assert!(wdr.supported);
    assert_eq!(wdr.sessions_with_data_7d, 1);
    assert!(wdr.mean_depletion_pct_7d.is_none());
    assert_eq!(wdr.high_depletion_sessions_7d, 0);
}

#[test]
fn etvs_weights_zone_minutes_linearly() {
    let zones = json!([
        {"id": "Z1", "secs": 1800},
        {"id": "Z2", "secs": 900},
        {"id": "Z3", "secs": 600},
        {"id": "Z4", "secs": 300},
        {"id": "Z5", "secs": 0}
    ]);

    let metrics = compute_etvs(Some(&zones), Some(3600.0)).unwrap();
    assert!((metrics.score_weighted_minutes - 110.0).abs() < 1e-12);
    assert_eq!(metrics.zone_minutes, [30.0, 15.0, 10.0, 5.0, 0.0]);
    assert_eq!(metrics.coverage_ratio, Some(1.0));
    assert_eq!(metrics.activities_with_zone_data, 1);
    assert_eq!(metrics.activities_total, 1);
}

#[test]
fn etvs_returns_none_for_missing_or_empty_zone_data() {
    assert!(compute_etvs(None, Some(3600.0)).is_none());
    assert!(compute_etvs(Some(&json!([])), Some(3600.0)).is_none());
}

#[test]
fn etvs_keeps_coverage_none_without_positive_moving_time() {
    let zones = json!([{"id": "Z1", "secs": 600}]);
    let metrics = compute_etvs(Some(&zones), None).unwrap();
    assert_eq!(metrics.coverage_ratio, None);
    assert!((metrics.score_weighted_minutes - 10.0).abs() < 1e-12);
}

#[test]
fn period_etvs_is_additive_and_reports_partial_coverage() {
    let activities = [
        ActivitySummary {
            id: "a".into(),
            moving_time: Some(1800),
            ..Default::default()
        },
        ActivitySummary {
            id: "b".into(),
            moving_time: Some(1800),
            ..Default::default()
        },
    ];
    let refs = activities.iter().collect::<Vec<_>>();
    let details = HashMap::from([
        (
            "a".into(),
            json!({"icu_zone_times": [{"id": "Z1", "secs": 1800}]}),
        ),
        ("b".into(), json!({"moving_time": 1800})),
    ]);

    let metrics = aggregate_period_etvs(&refs, &details).unwrap();
    assert!((metrics.score_weighted_minutes - 30.0).abs() < 1e-12);
    assert_eq!(metrics.coverage_ratio, Some(0.5));
    assert_eq!(metrics.activities_with_zone_data, 1);
    assert_eq!(metrics.activities_total, 2);
}

#[test]
fn period_etvs_returns_none_when_no_activity_has_zone_data() {
    let activities = [ActivitySummary {
        id: "a".into(),
        moving_time: Some(1800),
        ..Default::default()
    }];
    let refs = activities.iter().collect::<Vec<_>>();
    assert!(aggregate_period_etvs(&refs, &HashMap::new()).is_none());
}
