use intervals_icu_client::ActivitySummary;
use intervals_icu_mcp::engines::coach_metrics::aggregate_period_etvs;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn realistic_activity_details_produce_auditable_period_etvs() {
    let activities = [
        ActivitySummary {
            id: "run-1".into(),
            moving_time: Some(3600),
            ..Default::default()
        },
        ActivitySummary {
            id: "ride-1".into(),
            moving_time: Some(1800),
            ..Default::default()
        },
    ];
    let activity_refs = activities.iter().collect::<Vec<_>>();
    let details = HashMap::from([
        (
            "run-1".into(),
            json!({
                "icu_zone_times": [
                    {"id": "Z1", "secs": 1800},
                    {"id": "Z2", "secs": 900},
                    {"id": "Z3", "secs": 600},
                    {"id": "Z4", "secs": 300}
                ]
            }),
        ),
        (
            "ride-1".into(),
            json!({
                "icu_zone_times": [
                    {"id": "Z5", "secs": 600},
                    {"id": "Z6", "secs": 600},
                    {"id": "Z7", "secs": 600}
                ]
            }),
        ),
    ]);

    let metrics = aggregate_period_etvs(&activity_refs, &details).unwrap();
    assert!((metrics.score_weighted_minutes - 260.0).abs() < 1e-12);
    assert_eq!(metrics.zone_minutes, [30.0, 15.0, 10.0, 5.0, 30.0]);
    assert_eq!(metrics.coverage_ratio, Some(1.0));
    assert_eq!(metrics.activities_with_zone_data, 2);
    assert_eq!(metrics.activities_total, 2);
}
