use std::collections::HashMap;

use intervals_icu_client::ActivitySummary;
use intervals_icu_mcp::domains::coach::AnalysisWindow;
use intervals_icu_mcp::domains::progress::{HypothesisDomain, TrendState};
use intervals_icu_mcp::engines::progress_tracking::build_progress_report;
use serde_json::json;

#[test]
fn full_report_detects_flat_tail_and_volume_hypothesis() {
    let mut wellness_entries = Vec::new();
    for day in 0..28 {
        wellness_entries.push(json!({
            "date": format!("2026-01-{:02}", day + 1),
            "ctl": 55.0 + (day as f64 * 0.4),
            "hrv": 65.0,
        }));
    }
    for day in 28..56 {
        wellness_entries.push(json!({
            "date": format!("2026-02-{:02}", day - 27),
            "ctl": 66.0,
            "hrv": 64.0,
        }));
    }
    let wellness = json!(wellness_entries);

    let activities = vec![
        ActivitySummary {
            id: "act-1".into(),
            start_date_local: "2026-02-20".into(),
            training_load: Some(70),
            ..Default::default()
        },
        ActivitySummary {
            id: "act-2".into(),
            start_date_local: "2026-02-21".into(),
            training_load: Some(72),
            ..Default::default()
        },
        ActivitySummary {
            id: "act-3".into(),
            start_date_local: "2026-02-22".into(),
            training_load: Some(68),
            ..Default::default()
        },
    ];

    let details = HashMap::from([
        (
            "act-1".to_string(),
            json!({"icu_zone_times": [{"id": "Z1", "secs": 1800}, {"id": "Z2", "secs": 600}, {"id": "Z3", "secs": 300}], "icu_training_load": 70, "polarization_index": 0.88}),
        ),
        (
            "act-2".to_string(),
            json!({"icu_zone_times": [{"id": "Z1", "secs": 1700}, {"id": "Z2", "secs": 700}, {"id": "Z3", "secs": 300}], "icu_training_load": 72, "polarization_index": 0.85}),
        ),
        (
            "act-3".to_string(),
            json!({"icu_zone_times": [{"id": "Z1", "secs": 1900}, {"id": "Z2", "secs": 500}, {"id": "Z3", "secs": 200}], "icu_training_load": 68, "polarization_index": 0.91}),
        ),
    ]);

    let window = AnalysisWindow::new(
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 2, 25).unwrap(),
    );

    let report = build_progress_report(&wellness, &activities, &details, &window);
    assert!(report.plateau.plateau_detected);
    assert_eq!(report.plateau.trend, TrendState::Flat);
    assert!(!report.hypotheses.is_empty());
    assert_eq!(report.hypotheses[0].domain, HypothesisDomain::Volume);
}

#[test]
fn report_degrades_gracefully_without_activity_details() {
    let wellness = json!([
        {"date": "2026-01-01", "ctl": 60.0, "hrv": 65.0},
        {"date": "2026-01-02", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-03", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-04", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-05", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-06", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-07", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-08", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-09", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-10", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-11", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-12", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-13", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-14", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-15", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-16", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-17", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-18", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-19", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-20", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-21", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-22", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-23", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-24", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-25", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-26", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-27", "ctl": 60.0, "hrv": 64.0},
        {"date": "2026-01-28", "ctl": 60.0, "hrv": 64.0}
    ]);

    let activities = vec![ActivitySummary {
        id: "act-1".into(),
        start_date_local: "2026-01-28".into(),
        training_load: Some(60),
        ..Default::default()
    }];

    let window = AnalysisWindow::new(
        chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        chrono::NaiveDate::from_ymd_opt(2026, 1, 28).unwrap(),
    );

    let report = build_progress_report(&wellness, &activities, &HashMap::new(), &window);
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("TID drift unavailable"))
    );
}
