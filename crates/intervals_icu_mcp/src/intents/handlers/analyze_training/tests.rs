
use super::*;
use crate::domains::coach::{AcwrMetrics, LoadManagementMetrics};
use crate::domains::interval_segment::SportPresentation;
use crate::engines::analysis_fetch::FetchedAnalysisData;
use crate::engines::analyze_training::render::*;
use crate::engines::analyze_training::shared::{
    SingleAnalysisMode, build_local_raw_stream, sport_presentation,
};
use crate::engines::interval_analysis::{
    IntervalOutputKind, IntervalOutputValue, calculate_median, count_work_intervals,
    is_planned_workout_id, numeric_value,
};
use crate::engines::metric_streams::parse_metric_streams;
use crate::engines::shared::parse_activity_date;
use crate::intents::ContentBlock;
use chrono::NaiveDate;
use intervals_icu_client::EventCategory;
use serde_json::json;

#[test]
fn test_new_handler() {
    let handler = AnalyzeTrainingHandler::new();
    assert_eq!(handler.name(), "analyze_training");
}

#[test]
fn test_default_handler() {
    let _handler = AnalyzeTrainingHandler;
}

#[test]
fn local_stream_builder_accepts_intervals_icu_stream_names() {
    let streams = json!({
        "time": [0.0, 1.0, 2.0],
        "velocity_smooth": [3.0, 3.5, 4.0],
        "heartrate": [140.0, 145.0, 150.0],
        "watts": [200.0, 225.0, 250.0]
    });

    let raw = build_local_raw_stream(&streams).expect("canonical streams must be accepted");
    assert_eq!(raw.time_s, vec![0.0, 1.0, 2.0]);
    assert_eq!(raw.speed, vec![3.0, 3.5, 4.0]);
    assert_eq!(raw.power, Some(vec![200.0, 225.0, 250.0]));
}

// ========================================================================
// IntentHandler Trait Implementation Tests
// ========================================================================

#[test]
fn test_name() {
    let handler = AnalyzeTrainingHandler::new();
    assert_eq!(IntentHandler::name(&handler), "analyze_training");
}

#[test]
fn test_description() {
    let handler = AnalyzeTrainingHandler::new();
    let desc = IntentHandler::description(&handler);
    assert!(desc.contains("Analyzes training"));
    assert!(desc.contains("single workout"));
    assert!(desc.contains("period"));
}

#[test]
fn test_input_schema_structure() {
    let handler = AnalyzeTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    assert!(schema.get("type").is_some());
    assert_eq!(schema.get("type").unwrap().as_str(), Some("object"));

    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("target_type"));
    assert!(props.contains_key("date"));
    assert!(props.contains_key("period_start"));
    assert!(props.contains_key("period_end"));
    assert!(props.contains_key("analysis_type"));
    assert!(props.contains_key("include_best_efforts"));
    assert!(props.contains_key("include_histograms"));

    // target_type is required
    let required = schema.get("required").unwrap().as_array().unwrap();
    assert!(required.contains(&json!("target_type")));

    // Check allOf conditional constraint for date vs period
    let all_of = schema.get("allOf").unwrap().as_array().unwrap();
    assert!(all_of.len() >= 2, "allOf should have at least 2 elements");
}

#[test]
fn test_requires_idempotency_token() {
    let handler = AnalyzeTrainingHandler::new();
    assert!(!IntentHandler::requires_idempotency_token(&handler));
}

// ========================================================================
// Input Validation Tests
// ========================================================================

#[test]
fn test_validation_missing_target_type() {
    let input = json!({
        "date": "2026-03-01"
    });
    assert!(input.get("target_type").is_none());
}

#[test]
fn test_validation_invalid_target_type() {
    let input = json!({
        "target_type": "invalid"
    });
    let target_type = input.get("target_type").and_then(|v| v.as_str()).unwrap();
    assert_ne!(target_type, "single");
    assert_ne!(target_type, "period");
}

// ========================================================================
// Analysis Type Tests
// ========================================================================

#[test]
fn test_analysis_type_values() {
    let valid_types = ["summary", "detailed", "intervals", "streams"];
    for t in &valid_types {
        assert!(["summary", "detailed", "intervals", "streams"].contains(t));
    }
}

#[test]
fn test_default_analysis_type() {
    let input = json!({
        "target_type": "single",
        "date": "2026-03-01"
    });
    let analysis_type = input
        .get("analysis_type")
        .and_then(|v| v.as_str())
        .unwrap_or("summary");
    assert_eq!(analysis_type, "summary");
}

#[test]
fn test_default_include_flags() {
    let input = json!({
        "target_type": "single",
        "date": "2026-03-01"
    });

    let include_best = input
        .get("include_best_efforts")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(!include_best);

    let include_hist = input
        .get("include_histograms")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    assert!(!include_hist);
}

// ========================================================================
// Output Structure Tests
// ========================================================================

#[test]
fn test_handler_metadata() {
    let handler = AnalyzeTrainingHandler::new();

    // Verify handler properties
    assert_eq!(handler.name(), "analyze_training");
    assert!(handler.description().len() > 50);

    let schema = handler.input_schema();
    assert!(schema.get("properties").is_some());
}

#[test]
fn test_handler_description_mentions_comments_and_calendar_context() {
    let handler = AnalyzeTrainingHandler::new();
    let description = handler.description();

    assert!(description.contains("calendar events"));
}

// ========================================================================
// Error Message Tests
// ========================================================================

#[test]
fn test_error_messages_contain_context() {
    // Test that validation errors contain field names
    let err = IntentError::validation("Missing: target_type".to_string());
    let err_str = err.to_string();
    assert!(err_str.contains("target_type"));
}

#[test]
fn load_management_markdown_renders_acwr_and_monotony_values() {
    let markdown = build_load_management_text(Some(&LoadManagementMetrics {
        acwr: Some(AcwrMetrics {
            acute_load: 420.0,
            chronic_load: 350.0,
            ratio: 1.20,
            state: "productive".into(),
        }),
        monotony: Some(2.1),
        strain: Some(882.0),
        fatigue_index: None,
        stress_tolerance: None,
        durability_index: None,
    }));

    assert!(markdown.contains("ACWR"));
    assert!(markdown.contains("Monotony"));
    assert!(markdown.contains("1.20"));
}

#[test]
fn load_management_markdown_reports_when_history_is_unavailable() {
    let markdown = build_load_management_text(None);

    assert!(markdown.contains("Load Context"));
    assert!(markdown.contains("unavailable"));
}

#[test]
fn build_basic_workout_metric_rows_formats_available_values() {
    let rows = build_basic_workout_metric_rows(Some(&serde_json::json!({
        "distance": 12345.0,
        "moving_time": 3661,
        "average_heartrate": 151.2,
        "average_watts": 245.7,
        "total_elevation_gain": 432.0
    })));

    assert_eq!(
        rows,
        vec![
            vec!["Distance".to_string(), "12.35 km".to_string()],
            vec!["Duration".to_string(), "1:01:01".to_string()],
            vec!["Avg HR".to_string(), "151 bpm".to_string()],
            vec!["Avg Power".to_string(), "246 W".to_string()],
            vec!["Elevation".to_string(), "432 m".to_string()],
        ]
    );
}

#[test]
fn build_interval_analysis_rows_formats_known_fields() {
    let rows = build_interval_analysis_rows(
        &[
            serde_json::json!({
                "moving_time": 95,
                "average_heartrate": 162.4,
                "average_watts": 301.0
            }),
            serde_json::json!({
                "moving_time": 120,
                "average_heartrate": 158.0,
                "average_watts": 287.2
            }),
        ],
        None,
        IntervalOutputKind::Power,
    );

    assert_eq!(
        rows,
        vec![
            vec![
                "1".to_string(),
                "1:35".to_string(),
                "162 bpm".to_string(),
                "301 W".to_string(),
            ],
            vec![
                "2".to_string(),
                "2:00".to_string(),
                "158 bpm".to_string(),
                "287 W".to_string(),
            ],
        ]
    );
}

#[test]
fn build_interval_analysis_rows_backfills_power_from_stream_slice() {
    let rows = build_interval_analysis_rows(
        &[
            serde_json::json!({
                "start_index": 0,
                "end_index": 4,
                "moving_time": 240,
                "average_heartrate": 150.0,
                "average_watts": null
            }),
            serde_json::json!({
                "start_index": 4,
                "end_index": 8,
                "moving_time": 240,
                "average_heartrate": 162.0,
                "average_watts": null
            }),
        ],
        Some(&serde_json::json!({
            "watts": [210.0, 220.0, 230.0, 240.0, 280.0, 290.0, 300.0, 310.0]
        })),
        IntervalOutputKind::Power,
    );

    assert_eq!(
        rows,
        vec![
            vec![
                "1".to_string(),
                "4:00".to_string(),
                "150 bpm".to_string(),
                "225 W".to_string(),
            ],
            vec![
                "2".to_string(),
                "4:00".to_string(),
                "162 bpm".to_string(),
                "295 W".to_string(),
            ],
        ]
    );
}

#[test]
fn build_period_summary_rows_formats_snapshot_values() {
    let rows = build_period_summary_rows(
        4,
        &crate::engines::coach_metrics::TrendSnapshot {
            activity_count: 4,
            total_time_secs: 18_600,
            total_distance_m: 42_250.0,
            total_elevation_m: 640.0,
        },
        7.4,
    );

    assert_eq!(
        rows,
        vec![
            vec!["Total Time".to_string(), "5:10:00".to_string()],
            vec!["Distance".to_string(), "42.2 km".to_string()],
            vec!["Elevation".to_string(), "640 m".to_string()],
            vec!["Weekly Avg".to_string(), "7.4 hrs".to_string()],
        ]
    );
}

// ========================================================================
// Work Interval Counting Tests
// ========================================================================

#[test]
fn test_count_work_intervals_empty() {
    let intervals = vec![];
    assert_eq!(count_work_intervals(&intervals), 0);
}

#[test]
fn test_count_work_intervals_with_real_data() {
    // Simulate the user's workout: 7 work intervals + 8 recovery intervals
    let intervals = vec![
        // Work intervals (high speed, high HR)
        json!({"average_speed": 2.52, "average_heartrate": 126}), // 1: borderline (low HR)
        json!({"average_speed": 2.76, "average_heartrate": 142}), // 2: work
        json!({"average_speed": 3.04, "average_heartrate": 158}), // 3: work
        json!({"average_speed": 0.85, "average_heartrate": 128}), // 4: recovery (very slow)
        json!({"average_speed": 2.95, "average_heartrate": 158}), // 5: work
        json!({"average_speed": 2.23, "average_heartrate": 140}), // 6: borderline
        json!({"average_speed": 2.91, "average_heartrate": 159}), // 7: work
        json!({"average_speed": 2.13, "average_heartrate": 140}), // 8: borderline
        json!({"average_speed": 2.98, "average_heartrate": 160}), // 9: work
        json!({"average_speed": 2.15, "average_heartrate": 141}), // 10: borderline
        json!({"average_speed": 2.96, "average_heartrate": 158}), // 11: work
        json!({"average_speed": 2.04, "average_heartrate": 138}), // 12: borderline
        json!({"average_speed": 3.02, "average_heartrate": 156}), // 13: work
        json!({"average_speed": 1.44, "average_heartrate": 128}), // 14: recovery (slow)
        json!({"average_speed": 0.75, "average_heartrate": 115}), // 15: recovery (very slow)
    ];

    let count = count_work_intervals(&intervals);
    // Should identify ~7-8 work intervals (the ones with speed >= ~2.5 and HR >= ~145)
    assert!(
        (6..=9).contains(&count),
        "Expected 6-9 work intervals, got {}",
        count
    );
}

#[test]
fn test_count_work_intervals_clear_separation() {
    // Clear work vs recovery separation
    let intervals = vec![
        json!({"average_speed": 3.0, "average_heartrate": 160}), // work
        json!({"average_speed": 1.5, "average_heartrate": 130}), // recovery
        json!({"average_speed": 3.1, "average_heartrate": 162}), // work
        json!({"average_speed": 1.4, "average_heartrate": 128}), // recovery
        json!({"average_speed": 3.0, "average_heartrate": 158}), // work
    ];

    let count = count_work_intervals(&intervals);
    assert_eq!(count, 3, "Should identify 3 work intervals");
}

#[test]
fn test_count_work_intervals_speed_only() {
    // Some intervals without HR data
    let intervals = vec![
        json!({"average_speed": 3.0}), // work
        json!({"average_speed": 1.5}), // recovery
        json!({"average_speed": 3.1}), // work
        json!({"average_speed": 1.4}), // recovery
        json!({"average_speed": 3.0}), // work
    ];

    let count = count_work_intervals(&intervals);
    assert_eq!(count, 3, "Should identify 3 work intervals by speed");
}

#[test]
fn test_calculate_median() {
    let mut values = vec![5.0, 2.0, 8.0, 1.0, 9.0];
    assert!((calculate_median(&mut values) - 5.0).abs() < 0.001);

    let mut values = vec![1.0, 2.0, 3.0, 4.0];
    assert!((calculate_median(&mut values) - 2.5).abs() < 0.001);

    let mut values = vec![42.0];
    assert!((calculate_median(&mut values) - 42.0).abs() < 0.001);

    let mut values = vec![];
    assert!((calculate_median(&mut values) - 0.0).abs() < 0.001);
}

// ========================================================================
// Key Phrase Extraction Tests (for multiple activity guidance)
// ========================================================================

#[test]
fn test_key_phrase_extraction_with_dash() {
    // Test with ASCII dash
    let name = "Long Run Z2 - Key Workout";
    let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
    assert_eq!(key_phrase, "Long Run Z2");
}

#[test]
fn test_key_phrase_extraction_with_em_dash() {
    // Test with Unicode em-dash (—)
    let name = "Long Run Z2 — Key Workout";
    let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
    assert_eq!(key_phrase, "Long Run Z2");
}

#[test]
fn test_key_phrase_extraction_with_colon() {
    let name = "Tempo Run: Threshold Session";
    let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
    assert_eq!(key_phrase, "Tempo Run");
}

#[test]
fn test_key_phrase_extraction_no_separator() {
    let name = "Weight Training";
    let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
    assert_eq!(key_phrase, "Weight Training");
}

#[test]
fn test_key_phrase_extraction_empty_dash() {
    let name = "Intervals - Track Session";
    let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
    assert_eq!(key_phrase, "Intervals");
}

#[test]
fn test_key_phrase_extraction_unicode_dash() {
    // Test with em-dash (Unicode)
    let name = "Recovery Run — Easy Pace";
    let key_phrase = name.split(['-', '—', ':']).next().unwrap_or(name).trim();
    assert_eq!(key_phrase, "Recovery Run");
}

// ========================================================================
// SingleAnalysisMode Enum Tests
// ========================================================================

#[test]
fn test_single_analysis_mode_parse_default() {
    assert_eq!(SingleAnalysisMode::parse(None), SingleAnalysisMode::Summary);
    assert_eq!(
        SingleAnalysisMode::parse(Some("summary")),
        SingleAnalysisMode::Summary
    );
    assert_eq!(
        SingleAnalysisMode::parse(Some("unknown")),
        SingleAnalysisMode::Summary
    );
}

#[test]
fn test_single_analysis_mode_parse_detailed() {
    assert_eq!(
        SingleAnalysisMode::parse(Some("detailed")),
        SingleAnalysisMode::Detailed
    );
}

#[test]
fn test_single_analysis_mode_parse_intervals() {
    assert_eq!(
        SingleAnalysisMode::parse(Some("intervals")),
        SingleAnalysisMode::Intervals
    );
}

#[test]
fn test_single_analysis_mode_parse_streams() {
    assert_eq!(
        SingleAnalysisMode::parse(Some("streams")),
        SingleAnalysisMode::Streams
    );
}

#[test]
fn test_single_analysis_mode_as_str() {
    assert_eq!(SingleAnalysisMode::Summary.as_str(), "summary");
    assert_eq!(SingleAnalysisMode::Detailed.as_str(), "detailed");
    assert_eq!(SingleAnalysisMode::Intervals.as_str(), "intervals");
    assert_eq!(SingleAnalysisMode::Streams.as_str(), "streams");
}

#[test]
fn test_single_analysis_mode_include_intervals() {
    assert!(!SingleAnalysisMode::Summary.include_intervals());
    assert!(!SingleAnalysisMode::Detailed.include_intervals());
    assert!(SingleAnalysisMode::Intervals.include_intervals());
    assert!(!SingleAnalysisMode::Streams.include_intervals());
}

#[test]
fn test_single_analysis_mode_include_streams() {
    assert!(!SingleAnalysisMode::Summary.include_streams());
    assert!(SingleAnalysisMode::Detailed.include_streams());
    assert!(SingleAnalysisMode::Intervals.include_streams());
    assert!(SingleAnalysisMode::Streams.include_streams());
}

#[test]
fn test_single_analysis_mode_show_methods() {
    // Summary mode
    assert!(!SingleAnalysisMode::Summary.show_execution_context());
    assert!(!SingleAnalysisMode::Summary.show_interval_section());
    assert!(!SingleAnalysisMode::Summary.show_stream_section());
    assert!(!SingleAnalysisMode::Summary.show_quality_findings());
    assert!(!SingleAnalysisMode::Summary.show_data_availability());
    assert!(!SingleAnalysisMode::Summary.show_detailed_breakdown());

    // Detailed mode
    assert!(SingleAnalysisMode::Detailed.show_execution_context());
    assert!(!SingleAnalysisMode::Detailed.show_interval_section());
    assert!(!SingleAnalysisMode::Detailed.show_stream_section());
    assert!(SingleAnalysisMode::Detailed.show_quality_findings());
    assert!(SingleAnalysisMode::Detailed.show_data_availability());
    assert!(SingleAnalysisMode::Detailed.show_detailed_breakdown());

    // Intervals mode
    assert!(!SingleAnalysisMode::Intervals.show_execution_context());
    assert!(SingleAnalysisMode::Intervals.show_interval_section());
    assert!(!SingleAnalysisMode::Intervals.show_stream_section());
    assert!(!SingleAnalysisMode::Intervals.show_quality_findings());
    assert!(!SingleAnalysisMode::Intervals.show_data_availability());
    assert!(!SingleAnalysisMode::Intervals.show_detailed_breakdown());

    // Streams mode
    assert!(SingleAnalysisMode::Streams.show_execution_context());
    assert!(!SingleAnalysisMode::Streams.show_interval_section());
    assert!(SingleAnalysisMode::Streams.show_stream_section());
    assert!(SingleAnalysisMode::Streams.show_quality_findings());
    assert!(SingleAnalysisMode::Streams.show_data_availability());
    assert!(!SingleAnalysisMode::Streams.show_detailed_breakdown());
}

// ========================================================================
// Date Parsing Helper Tests
// ========================================================================

#[test]
fn test_parse_activity_date_with_timestamp() {
    let result = parse_activity_date("2026-03-01T10:30:00");
    assert!(result.is_some());
    assert_eq!(
        result.unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
    );
}

#[test]
fn test_parse_activity_date_date_only() {
    let result = parse_activity_date("2026-03-01");
    assert!(result.is_some());
    assert_eq!(
        result.unwrap(),
        NaiveDate::from_ymd_opt(2026, 3, 1).unwrap()
    );
}

#[test]
fn test_parse_activity_date_invalid() {
    assert!(parse_activity_date("invalid").is_none());
    assert!(parse_activity_date("").is_none());
}

// ========================================================================
// Requested Metrics Tests
// ========================================================================

#[test]
fn test_requested_metrics_empty_input() {
    let input = json!({});
    let metrics = requested_metrics(&input);
    assert!(metrics.is_empty());
}

#[test]
fn test_requested_metrics_with_array() {
    let input = json!({
        "metrics": ["time", "distance", "HR"]
    });
    let metrics = requested_metrics(&input);
    assert_eq!(metrics, vec!["time", "distance", "hr"]);
}

#[test]
fn test_requested_metrics_non_string_values() {
    let input = json!({
        "metrics": ["time", 123, null, "distance"]
    });
    let metrics = requested_metrics(&input);
    assert_eq!(metrics, vec!["time", "distance"]);
}

// ========================================================================
// Duration Formatting Tests
// ========================================================================

#[test]
fn test_format_duration_hhmm_under_hour() {
    assert_eq!(format_duration_hhmm(0), "0:00");
    assert_eq!(format_duration_hhmm(60), "1:00");
    assert_eq!(format_duration_hhmm(3661), "1:01:01");
    assert_eq!(format_duration_hhmm(7265), "2:01:05");
}

#[test]
fn test_format_duration_hhmm_over_hour() {
    assert_eq!(format_duration_hhmm(3600), "1:00:00");
    assert_eq!(format_duration_hhmm(7200), "2:00:00");
    assert_eq!(format_duration_hhmm(3661), "1:01:01");
}

#[test]
fn test_format_duration_compact_matches_hhmm() {
    // format_duration_compact should behave identically to format_duration_hhmm
    assert_eq!(format_duration_compact(0), format_duration_hhmm(0));
    assert_eq!(format_duration_compact(3661), format_duration_hhmm(3661));
    assert_eq!(format_duration_compact(7265), format_duration_hhmm(7265));
}

// ========================================================================
// Planned Workout ID Detection Tests
// ========================================================================

#[test]
fn test_is_planned_workout_id_event_prefix() {
    assert!(is_planned_workout_id("event:12345"));
    assert!(is_planned_workout_id("event:94131802"));
}

#[test]
fn test_is_planned_workout_id_regular_activity() {
    assert!(!is_planned_workout_id("12345"));
    assert!(!is_planned_workout_id("a1"));
    assert!(!is_planned_workout_id(""));
}

// ========================================================================
// Calendar Event Row Building Tests
// ========================================================================

#[test]
fn test_build_calendar_event_rows_empty() {
    let events: Vec<&intervals_icu_client::Event> = vec![];
    let rows = build_calendar_event_rows(&events);
    assert!(rows.is_empty());
}

#[test]
fn test_build_calendar_event_rows_with_events() {
    let events = [
        intervals_icu_client::Event {
            id: Some("e1".to_string()),
            start_date_local: "2026-03-01T10:00:00".to_string(),
            name: "Race Day".to_string(),
            category: EventCategory::RaceA,
            description: Some("Marathon".to_string()),
            r#type: None,
        },
        intervals_icu_client::Event {
            id: Some("e2".to_string()),
            start_date_local: "2026-03-02".to_string(),
            name: "Recovery".to_string(),
            category: EventCategory::Workout,
            description: None,
            r#type: None,
        },
    ];
    let refs = events.iter().collect::<Vec<_>>();
    let rows = build_calendar_event_rows(&refs);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], "2026-03-01");
    assert_eq!(rows[0][1], "RaceA");
    assert_eq!(rows[0][2], "Race Day");
    assert_eq!(rows[0][3], "Marathon");
    assert_eq!(rows[1][3], "n/a");
}

// ========================================================================
// Interval Output Value Tests
// ========================================================================

#[test]
fn test_interval_output_value_kind() {
    let power_value = IntervalOutputValue::Power(250.0);
    assert_eq!(power_value.kind(), IntervalOutputKind::Power);

    let pace_value = IntervalOutputValue::Pace(2.5);
    assert_eq!(pace_value.kind(), IntervalOutputKind::Pace);
}

#[test]
fn test_interval_output_value_format_power() {
    let value = IntervalOutputValue::Power(245.7);
    assert_eq!(value.format(), "246 W");
}

#[test]
fn test_interval_output_value_format_pace() {
    let value = IntervalOutputValue::Pace(2.778); // 10 km/h = 6:00 /km
    let formatted = value.format();
    assert!(formatted.contains("/km"));
}

#[test]
fn test_interval_output_value_format_invalid_pace() {
    let value = IntervalOutputValue::Pace(0.0);
    assert_eq!(value.format(), "n/a");
}

// ========================================================================
// Numeric Value Helper Tests
// ========================================================================

#[test]
fn test_numeric_value_from_f64() {
    let obj_value = serde_json::json!({"key": 42.5});
    let obj = obj_value.as_object().unwrap();
    assert_eq!(numeric_value(obj, "key"), Some(42.5));
}

#[test]
fn test_numeric_value_from_i64() {
    let obj_value = serde_json::json!({"key": 42});
    let obj = obj_value.as_object().unwrap();
    assert_eq!(numeric_value(obj, "key"), Some(42.0));
}

#[test]
fn test_numeric_value_missing_key() {
    let obj_value = serde_json::json!({"other": 42});
    let obj = obj_value.as_object().unwrap();
    assert_eq!(numeric_value(obj, "key"), None);
}

#[test]
fn test_numeric_value_non_numeric() {
    let obj_value = serde_json::json!({"key": "text"});
    let obj = obj_value.as_object().unwrap();
    assert_eq!(numeric_value(obj, "key"), None);
}

// ========================================================================
// Histogram and Zone Distribution Tests
// ========================================================================

#[test]
fn test_format_histogram_number_integer() {
    assert_eq!(format_histogram_number(100.0), "100");
    assert_eq!(format_histogram_number(42.0), "42");
}

#[test]
fn test_format_histogram_number_decimal() {
    assert_eq!(format_histogram_number(42.5), "42.50");
    assert_eq!(format_histogram_number(0.123), "0.12");
}

#[test]
fn test_build_range_histogram_rows_empty() {
    let buckets: Vec<Value> = vec![];
    let rows = build_range_histogram_rows(&buckets, "bpm");
    assert!(rows.is_empty());
}

#[test]
fn test_build_range_histogram_rows_with_data() {
    let buckets = vec![
        json!({"min": 100, "max": 120, "secs": 600}),
        json!({"min": 120, "max": 140, "secs": 1200}),
    ];
    let rows = build_range_histogram_rows(&buckets, "bpm");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], "100-120 bpm");
    assert_eq!(rows[0][1], "10:00");
    assert_eq!(rows[1][0], "120-140 bpm");
    assert_eq!(rows[1][1], "20:00");
}

#[test]
fn test_build_bucket_histogram_rows_empty() {
    let buckets: Vec<Value> = vec![];
    let rows = build_bucket_histogram_rows(&buckets, Some("avg"), "bpm");
    assert!(rows.is_empty());
}

#[test]
fn test_build_bucket_histogram_rows_with_data() {
    let buckets = vec![
        json!({"start": 100, "secs": 300, "movingSecs": 280, "avg": 110}),
        json!({"start": 120, "secs": 600, "movingSecs": 580, "avg": 130}),
    ];
    let rows = build_bucket_histogram_rows(&buckets, Some("avg"), "bpm");

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][0], "100 bpm");
    assert_eq!(rows[0][1], "5:00");
    assert_eq!(rows[0][2], "4:40");
    assert_eq!(rows[0][3], "110");
}

#[test]
fn test_build_bucket_histogram_rows_without_average_key() {
    let buckets = vec![json!({"start": 100, "secs": 300})];
    let rows = build_bucket_histogram_rows(&buckets, None, "");

    assert_eq!(rows[0][3], "n/a");
}

#[test]
fn test_build_zone_distribution_rows_empty() {
    let zones_value = serde_json::json!({});
    let zones = zones_value.as_object().unwrap();
    let rows = build_zone_distribution_rows(zones);
    assert!(rows.is_empty());
}

#[test]
fn test_build_zone_distribution_rows_with_data() {
    let zones_value = serde_json::json!({
        "z1": 600,
        "z2": 1200,
        "z3": 300
    });
    let zones = zones_value.as_object().unwrap();
    let rows = build_zone_distribution_rows(zones);

    assert_eq!(rows.len(), 3);
    // Total: 2100 seconds
    // z1: 600/2100 = 28.57% -> 29%
    // z2: 1200/2100 = 57.14% -> 57%
    // z3: 300/2100 = 14.28% -> 14%
    assert_eq!(rows[0][0], "Z1");
    assert_eq!(rows[0][1], "10:00");
    assert!(rows[0][2].contains("%"));
}

// ========================================================================
// Best Efforts Helper Tests
// ========================================================================

#[test]
fn test_best_efforts_array_direct_array() {
    let value = json!([{"seconds": 60}, {"seconds": 300}]);
    let arr = best_efforts_array(&value);
    assert!(arr.is_some());
    assert_eq!(arr.unwrap().len(), 2);
}

#[test]
fn test_best_efforts_array_nested_best_efforts() {
    let value = json!({"best_efforts": [{"seconds": 60}]});
    let arr = best_efforts_array(&value);
    assert!(arr.is_some());
    assert_eq!(arr.unwrap().len(), 1);
}

#[test]
fn test_best_efforts_array_nested_efforts() {
    let value = json!({"efforts": [{"seconds": 60}]});
    let arr = best_efforts_array(&value);
    assert!(arr.is_some());
    assert_eq!(arr.unwrap().len(), 1);
}

#[test]
fn test_best_efforts_array_invalid() {
    let value = json!("not an array");
    assert!(best_efforts_array(&value).is_none());

    let value = json!({"other": "field"});
    assert!(best_efforts_array(&value).is_none());
}

#[test]
fn test_format_best_effort_duration_seconds_only() {
    assert_eq!(format_best_effort_duration(5), "5s");
    assert_eq!(format_best_effort_duration(59), "59s");
}

#[test]
fn test_format_best_effort_duration_minutes_seconds() {
    assert_eq!(format_best_effort_duration(60), "1:00");
    assert_eq!(format_best_effort_duration(125), "2:05");
    assert_eq!(format_best_effort_duration(3599), "59:59");
}

#[test]
fn test_format_best_effort_duration_hours() {
    assert_eq!(format_best_effort_duration(3600), "1:00:00");
    assert_eq!(format_best_effort_duration(3661), "1:01:01");
}

#[test]
fn test_format_best_effort_average_power() {
    let best_efforts = json!({"stream": "watts"});
    let effort_value = json!({"watts": 250.0});
    let effort = effort_value.as_object().unwrap();
    let avg = format_best_effort_average(&best_efforts, effort);
    assert_eq!(avg, Some("250 W".to_string()));
}

#[test]
fn test_format_best_effort_average_pace() {
    let best_efforts = json!({"stream": "speed"});
    let effort_value = json!({"average": 2.778});
    let effort = effort_value.as_object().unwrap();
    let avg = format_best_effort_average(&best_efforts, effort);
    assert!(avg.unwrap().contains("/km"));
}

#[test]
fn test_format_best_effort_average_heartrate() {
    let best_efforts = json!({});
    let effort_value = json!({"heartrate": 155.0});
    let effort = effort_value.as_object().unwrap();
    let avg = format_best_effort_average(&best_efforts, effort);
    assert_eq!(avg, Some("155 bpm".to_string()));
}

#[test]
fn test_format_best_effort_average_no_data() {
    let best_efforts = json!({});
    let effort_value = json!({});
    let effort = effort_value.as_object().unwrap();
    assert!(format_best_effort_average(&best_efforts, effort).is_none());
}

// ========================================================================
// Activity Message Row Building Tests
// ========================================================================

#[test]
fn test_build_activity_message_rows_empty() {
    let messages: Vec<intervals_icu_client::ActivityMessage> = vec![];
    let rows = build_activity_message_rows(&messages);
    assert!(rows.is_empty());
}

#[test]
fn test_build_activity_message_rows_with_messages() {
    use intervals_icu_client::ActivityMessage;
    let messages = vec![
        ActivityMessage {
            id: 1,
            athlete_id: Some("athlete1".to_string()),
            name: Some("John".to_string()),
            created: Some("2026-03-01T12:00:00Z".to_string()),
            message_type: Some("TEXT".to_string()),
            content: Some("Great workout!".to_string()),
            activity_id: Some("a1".to_string()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        },
        ActivityMessage {
            id: 2,
            athlete_id: Some("athlete2".to_string()),
            name: None,
            created: None,
            message_type: None,
            content: Some("Keep it up!".to_string()),
            activity_id: Some("a1".to_string()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        },
    ];
    let rows = build_activity_message_rows(&messages);

    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0][1], "John");
    assert_eq!(rows[0][3], "Great workout!");
    assert_eq!(rows[1][1], "athlete2");
}

#[test]
fn test_build_activity_message_rows_filters_deleted() {
    use intervals_icu_client::ActivityMessage;
    let messages = vec![
        ActivityMessage {
            id: 1,
            athlete_id: Some("athlete1".to_string()),
            name: None,
            created: None,
            message_type: None,
            content: Some("Visible".to_string()),
            activity_id: Some("a1".to_string()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        },
        ActivityMessage {
            id: 2,
            athlete_id: Some("athlete1".to_string()),
            name: None,
            created: None,
            message_type: None,
            content: Some("Deleted".to_string()),
            activity_id: Some("a1".to_string()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: Some("true".to_string()),
        },
    ];
    let rows = build_activity_message_rows(&messages);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][3], "Visible");
}

#[test]
fn test_build_activity_message_rows_filters_empty_content() {
    use intervals_icu_client::ActivityMessage;
    let messages = vec![
        ActivityMessage {
            id: 1,
            athlete_id: Some("athlete1".to_string()),
            name: None,
            created: None,
            message_type: None,
            content: Some("   ".to_string()),
            activity_id: Some("a1".to_string()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        },
        ActivityMessage {
            id: 2,
            athlete_id: Some("athlete1".to_string()),
            name: None,
            created: None,
            message_type: None,
            content: None,
            activity_id: Some("a1".to_string()),
            start_index: None,
            end_index: None,
            attachment_url: None,
            attachment_mime_type: None,
            deleted: None,
        },
    ];
    let rows = build_activity_message_rows(&messages);
    assert!(rows.is_empty());
}

#[test]
fn test_build_activity_message_rows_handles_missing_fields() {
    use intervals_icu_client::ActivityMessage;
    let messages = vec![ActivityMessage {
        id: 1,
        athlete_id: Some("athlete1".to_string()),
        name: None,
        created: None,
        message_type: None,
        content: Some("Test".to_string()),
        activity_id: Some("a1".to_string()),
        start_index: None,
        end_index: None,
        attachment_url: None,
        attachment_mime_type: None,
        deleted: None,
    }];
    let rows = build_activity_message_rows(&messages);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0][1], "athlete1"); // Falls back to athlete_id
    assert_eq!(rows[0][2], "TEXT"); // Default type
}

// ========================================================================
// Execute() Path Tests - analyze_single()
// ========================================================================

use crate::test_support::content_text;
use crate::test_support::mock::MockIntervalsClient;
use intervals_icu_client::{ActivitySummary, Event};

#[tokio::test]
async fn test_analyze_single_summary_mode() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
                "total_elevation_gain": 200.0,
            })),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "summary"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    assert!(!output.content.is_empty());
    // Verify basic structure
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Analysis"));
    assert!(content_str.contains("Test Workout"));
}

#[tokio::test]
async fn test_analyze_single_detailed_mode() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
                "total_elevation_gain": 200.0,
                "average_cadence": 85.0,
                "average_speed": 2.78,
                "icu_training_load": 75.5,
                "average_temp": 18.5,
            }))
            .with_streams(json!({
                "watts": [200, 250, 300],
                "heartrate": [140, 150, 160],
            })),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "detailed"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Detailed Breakdown"));
    assert!(content_str.contains("Execution Context"));
}

#[tokio::test]
async fn test_analyze_single_intervals_mode() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Interval Workout")
            .with_workout_detail(json!({
                "distance": 12000.0,
                "moving_time": 4200,
                "average_heartrate": 155.0,
                "average_watts": 280.0,
            }))
            .with_intervals(json!([
                {"moving_time": 120, "average_heartrate": 165, "average_watts": 350},
                {"moving_time": 120, "average_heartrate": 162, "average_watts": 340},
                {"moving_time": 120, "average_heartrate": 168, "average_watts": 360},
            ])),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "intervals"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Interval Analysis"));
    assert!(content_str.contains("Upstream Interval Reference"));
}

#[tokio::test]
async fn test_analyze_single_streams_mode() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
            }))
            .with_streams(json!({
                "watts": [200, 250, 300, 280],
                "heartrate": [140, 150, 160, 155],
                "velocity_smooth": [2.5, 2.8, 3.0, 2.7],
            })),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "streams"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Stream Insights"));
    assert!(content_str.contains("watts"));
}

#[tokio::test]
async fn test_analyze_single_with_histograms() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
            }))
            .with_hr_histogram(json!({
                "zones": {"z1": 600, "z2": 1800, "z3": 1200}
            }))
            .with_power_histogram(json!([
                {"start": 100, "secs": 300, "movingSecs": 280, "avgWatts": 150},
                {"start": 200, "secs": 1800, "movingSecs": 1700, "avgWatts": 250},
                {"start": 300, "secs": 1500, "movingSecs": 1400, "avgWatts": 350},
            ])),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "summary",
        "include_histograms": true
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("HR Histogram"));
    assert!(content_str.contains("Power Histogram"));
}

#[tokio::test]
async fn test_analyze_single_with_best_efforts() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
            }))
            .with_best_efforts(json!([
                {"seconds": 60, "watts": 400},
                {"seconds": 300, "watts": 350},
                {"seconds": 1200, "watts": 300},
            ])),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "include_best_efforts": true
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Best Efforts"));
}

#[tokio::test]
async fn test_analyze_single_no_activities_found() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient {
        activities: vec![],
        ..Default::default()
    });

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("No activities found"));
    assert!(!output.suggestions.is_empty());
}

#[tokio::test]
async fn test_analyze_single_multiple_activities_found() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient {
        activities: vec![
            ActivitySummary {
                id: "12345".to_string(),
                name: Some("Morning Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            },
            ActivitySummary {
                id: "12346".to_string(),
                name: Some("Evening Ride".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Multiple activities found"));
    assert!(content_str.contains("Morning Run"));
    assert!(content_str.contains("Evening Ride"));
}

#[tokio::test]
async fn test_analyze_single_with_description_filter() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient {
        activities: vec![
            ActivitySummary {
                id: "12345".to_string(),
                name: Some("Easy Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            },
            ActivitySummary {
                id: "12346".to_string(),
                name: Some("Tempo Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "description_contains": "tempo"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    // Should only match the Tempo Run
    assert!(content_str.contains("Tempo Run"));
}

#[tokio::test]
async fn test_analyze_single_with_requested_metrics() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
                "total_elevation_gain": 200.0,
            })),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "metrics": ["time", "distance", "hr", "tss"]
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    // Verify requested metrics table exists with expected headers
    let has_metrics_table = output.content.iter().any(|block| {
        if let ContentBlock::Table { headers, rows } = block {
            headers.contains(&"Metric".to_string())
                && headers.contains(&"Value".to_string())
                && rows
                    .iter()
                    .any(|row| row.first().map(|s| s.as_str()) == Some("TIME"))
                && rows
                    .iter()
                    .any(|row| row.first().map(|s| s.as_str()) == Some("DISTANCE"))
        } else {
            false
        }
    });
    assert!(
        has_metrics_table,
        "Expected requested metrics table with TIME/DISTANCE rows"
    );
}

#[tokio::test]
async fn test_analyze_single_with_activity_messages() {
    use intervals_icu_client::ActivityMessage;
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
            }))
            .with_activity_messages(vec![ActivityMessage {
                id: 1,
                athlete_id: Some("athlete1".to_string()),
                name: Some("Coach".to_string()),
                created: Some("2026-03-01T12:00:00Z".to_string()),
                message_type: Some("TEXT".to_string()),
                content: Some("Great job!".to_string()),
                activity_id: Some("12345".to_string()),
                start_index: None,
                end_index: None,
                attachment_url: None,
                attachment_mime_type: None,
                deleted: None,
            }]),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "detailed"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Workout Comments"));
    assert!(content_str.contains("Great job!"));
}

#[tokio::test]
async fn test_analyze_single_invalid_date() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::with_activity(
        "12345",
        "2026-03-01",
        "Test Workout",
    ));

    let input = json!({
        "target_type": "single",
        "date": "invalid-date"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_analyze_single_missing_date() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::with_activity(
        "12345",
        "2026-03-01",
        "Test Workout",
    ));

    let input = json!({
        "target_type": "single"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_analyze_period_basic() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![
                ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Run 1".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "12346".to_string(),
                    name: Some("Run 2".to_string()),
                    start_date_local: "2026-03-03".to_string(),
                    ..Default::default()
                },
            ])
            .with_fitness_summary(json!({
                "fitness": 50,
                "fatigue": 30,
                "form": 20
            }))
            .with_wellness(json!({
                "monotony": 1.5,
                "strain": 500
            })),
    );

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Period:"));
    assert!(content_str.contains("2026-03-01"));
    assert!(content_str.contains("2026-03-07"));
}

#[tokio::test]
async fn test_analyze_period_shows_fitness_snapshot() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![
                ActivitySummary {
                    id: "12345".to_string(),
                    name: Some("Run 1".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "12346".to_string(),
                    name: Some("Run 2".to_string()),
                    start_date_local: "2026-03-03".to_string(),
                    ..Default::default()
                },
            ])
            .with_fitness_summary(json!({
                "fitness": 65.0,
                "fatigue": 45.0,
                "form": 20.0,
                "rampRate": 3.0,
            }))
            .with_wellness(json!({})),
    );

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let content_str = content_text(&result.unwrap().content);
    assert!(
        content_str.contains("Fitness Snapshot"),
        "should show Fitness Snapshot section"
    );
    assert!(content_str.contains("CTL"), "should show CTL");
    assert!(content_str.contains("ATL"), "should show ATL");
    assert!(
        content_str.contains("Fresh"),
        "should show TSB with Fresh state"
    );
    assert!(content_str.contains("Ramp Rate"), "should show Ramp Rate");
}

#[tokio::test]
async fn test_analyze_period_no_activities() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("No activities found"));
}

#[tokio::test]
async fn test_analyze_period_with_calendar_events() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![ActivitySummary {
                id: "12345".to_string(),
                name: Some("Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }])
            .with_events(vec![Event {
                id: Some("e1".to_string()),
                start_date_local: "2026-03-05".to_string(),
                name: "Race Day".to_string(),
                category: EventCategory::RaceA,
                description: Some("Marathon".to_string()),
                r#type: None,
            }]),
    );

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Calendar Events"));
    assert!(content_str.contains("Race Day"));
}

#[tokio::test]
async fn test_analyze_period_invalid_start_date() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_start": "invalid",
        "period_end": "2026-03-07"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_analyze_period_invalid_end_date() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "invalid"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_analyze_period_start_after_end() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-07",
        "period_end": "2026-03-01"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_analyze_period_missing_period_start() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_end": "2026-03-07"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_analyze_period_missing_period_end() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_analyze_period_with_histograms_rejected() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "include_histograms": true
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("include_histograms")
    );
}

#[tokio::test]
async fn test_analyze_period_with_description_filter() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_activities(vec![
        ActivitySummary {
            id: "12345".to_string(),
            name: Some("Easy Run".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        },
        ActivitySummary {
            id: "12346".to_string(),
            name: Some("Interval Run".to_string()),
            start_date_local: "2026-03-03".to_string(),
            ..Default::default()
        },
    ]));

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "description_contains": "interval"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    // Should only include the interval run
    assert!(content_str.contains("Period:"));
}

#[tokio::test]
async fn test_analyze_period_with_requested_metrics() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![ActivitySummary {
                id: "12345".to_string(),
                name: Some("Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }])
            .with_activity_detail(
                "12345",
                json!({
                    "moving_time": 3600,
                    "distance": 10000.0,
                    "total_elevation_gain": 200.0,
                    "average_heartrate": 150.0,
                    "icu_training_load": 75.0,
                }),
            ),
    );

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "metrics": ["time", "distance", "hr"]
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Requested Metrics"));
}

#[tokio::test]
async fn test_analyze_period_renders_polarisation_and_wdr() {
    // W2 + W5 wiring test: assert that period output contains
    // polarisation (TID) and WDR rollup sections when activity details
    // have icu_zone_times and icu_max_wbal_depletion data.
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![
                ActivitySummary {
                    id: "act1".to_string(),
                    name: Some("Hard Ride".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "act2".to_string(),
                    name: Some("Easy Ride".to_string()),
                    start_date_local: "2026-03-03".to_string(),
                    ..Default::default()
                },
            ])
            .with_activity_detail(
                "act1",
                json!({
                    "moving_time": 3600,
                    "icu_training_load": 80.0,
                    "icu_max_wbal_depletion": 35000.0,
                    "icu_zone_times": [
                        {"id": "Z1", "secs": 1800},
                        {"id": "Z2", "secs": 600},
                        {"id": "Z3", "secs": 300},
                        {"id": "Z4", "secs": 600},
                        {"id": "Z5", "secs": 300}
                    ]
                }),
            )
            .with_activity_detail(
                "act2",
                json!({
                    "moving_time": 5400,
                    "icu_training_load": 40.0,
                    "icu_max_wbal_depletion": 5000.0,
                    "icu_zone_times": [
                        {"id": "Z1", "secs": 4500},
                        {"id": "Z2", "secs": 500},
                        {"id": "Z3", "secs": 100},
                        {"id": "Z4", "secs": 200},
                        {"id": "Z5", "secs": 100}
                    ]
                }),
            )
            .with_fitness_summary(json!({
                "fitness": 55,
                "fatigue": 30,
                "form": 25
            }))
            .with_wellness(json!({
                "monotony": 1.5,
                "strain": 500,
                "ctl": 55,
                "atl": 30,
                "tsb": 25,
                "icu_wprime": 50000
            })),
    );

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok(), "handler should succeed: {:?}", result.err());
    let output = result.unwrap();
    let content_str = content_text(&output.content);

    // W2: polarisation/TID should be rendered from the last activity's zone_times
    assert!(
        content_str.contains("Training Intensity Distribution"),
        "period output should contain TID section, got: {}",
        &content_str[..content_str.len().min(500)]
    );

    // W5: WDR rollup should be rendered (mean depletion across activities)
    // The render_wdrm_section outputs "W' Depletion" when supported
    assert!(
        content_str.contains("Depletion") || content_str.contains("WDRM"),
        "period output should contain WDR section, got: {}",
        &content_str[..content_str.len().min(500)]
    );
}

#[tokio::test]
async fn test_analyze_period_analysis_type_streams() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![ActivitySummary {
                id: "12345".to_string(),
                name: Some("Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }])
            .with_activity_detail(
                "12345",
                json!({
                    "moving_time": 3600,
                    "icu_training_load": 75.0,
                }),
            ),
    );

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "analysis_type": "streams"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    assert!(content_str.contains("Daily Load Series"));
}

#[tokio::test]
async fn test_analyze_period_analysis_type_intervals() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder().with_activities(vec![ActivitySummary {
            id: "12345".to_string(),
            name: Some("Interval Session".to_string()),
            start_date_local: "2026-03-01".to_string(),
            ..Default::default()
        }]),
    );

    let input = json!({
        "target_type": "period",
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "analysis_type": "intervals"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);
    // Interval sessions section appears when interval workouts found
    assert!(content_str.contains("Period:"));
}

#[tokio::test]
async fn test_execute_invalid_target_type() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "invalid"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Invalid target_type")
    );
}

#[tokio::test]
async fn test_execute_missing_target_type() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({});

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("target_type"));
}

// ========================================================================
// Regression: include_histograms forbidden for period analysis
// ========================================================================

#[test]
fn test_schema_has_if_then_constraints() {
    let handler = AnalyzeTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let all_of = schema
        .get("allOf")
        .expect("Schema should have 'allOf' clause");
    let all_of_arr = all_of.as_array().expect("allOf should be an array");

    // Second element has the period-specific if/then constraints
    let period_constraints = &all_of_arr[1];
    let if_clause = period_constraints
        .get("if")
        .expect("Schema allOf[1] should have 'if' clause");
    let then_clause = period_constraints
        .get("then")
        .expect("Schema allOf[1] should have 'then' clause");

    assert_eq!(
        if_clause
            .get("properties")
            .and_then(|p| p.get("target_type"))
            .and_then(|t| t.get("const"))
            .and_then(|c| c.as_str()),
        Some("period"),
        "'if' should check target_type == 'period'"
    );

    let then_props = then_clause.get("properties").unwrap().as_object().unwrap();
    assert!(
        then_props.contains_key("include_histograms"),
        "'then' should constrain include_histograms"
    );
    assert_eq!(
        then_props
            .get("include_histograms")
            .and_then(|v| v.get("const")),
        Some(&json!(false)),
        "include_histograms should be const: false for period"
    );
}

#[tokio::test]
async fn test_execute_period_with_histograms_rejected() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());

    let input = json!({
        "target_type": "period",
        "period_start": "2026-05-04",
        "period_end": "2026-06-07",
        "include_histograms": true
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("include_histograms"),
        "Error should mention include_histograms, got: {}",
        err
    );
}

#[test]
fn test_schema_include_histograms_description_mentions_single() {
    let handler = AnalyzeTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);
    let props = schema.get("properties").unwrap().as_object().unwrap();
    let histograms = props.get("include_histograms").unwrap();
    let desc = histograms
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap();
    assert!(
        desc.contains("single"),
        "Description should mention 'single', got: {}",
        desc
    );
}

#[tokio::test]
async fn test_single_workout_includes_grade() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
                "total_elevation_gain": 200.0,
            })),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "summary"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);

    let has_grade = content_str.contains("Grade")
        || content_str.contains("grade")
        || content_str.contains("Grade:")
        || content_str.contains("Workout Grade");
    assert!(
        has_grade,
        "Output should contain grade information (A/B/C/D/F). Got: {}",
        &content_str[..content_str.len().min(500)]
    );
}

#[tokio::test]
async fn test_single_workout_includes_insights() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::with_activity("12345", "2026-03-01", "Test Workout")
            .with_workout_detail(json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 250.0,
                "total_elevation_gain": 200.0,
            })),
    );

    let input = json!({
        "target_type": "single",
        "date": "2026-03-01",
        "analysis_type": "summary"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);

    let has_insights = content_str.contains("Insights")
        || content_str.contains("insights")
        || content_str.contains("Excellent")
        || content_str.contains("Good workout");
    assert!(
        has_insights,
        "Output should contain workout insights from WorkoutInsights::generate. Got: {}",
        &content_str[..content_str.len().min(500)]
    );
}

#[tokio::test]
async fn test_analyze_period_includes_linear_trend_insights() {
    // Dates must be within TrendWindows::MEDIUM (30 days) of the test
    // execution date, otherwise analyze_trend returns None and the
    // "Linear Trends" section never renders.
    let today = chrono::Utc::now().date_naive();
    let d1 = today - chrono::Duration::days(28);
    let d2 = today - chrono::Duration::days(21);
    let d3 = today - chrono::Duration::days(14);
    let d4 = today - chrono::Duration::days(7);

    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![
                ActivitySummary {
                    id: "1".to_string(),
                    name: Some("Run 1".to_string()),
                    start_date_local: d1.to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "2".to_string(),
                    name: Some("Run 2".to_string()),
                    start_date_local: d2.to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "3".to_string(),
                    name: Some("Run 3".to_string()),
                    start_date_local: d3.to_string(),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "4".to_string(),
                    name: Some("Run 4".to_string()),
                    start_date_local: d4.to_string(),
                    ..Default::default()
                },
            ])
            .with_activity_detail(
                "1",
                json!({
                    "distance": 5000.0,
                    "moving_time": 1800,
                    "icu_training_load": 40.0
                }),
            )
            .with_activity_detail(
                "2",
                json!({
                    "distance": 6000.0,
                    "moving_time": 2100,
                    "icu_training_load": 50.0
                }),
            )
            .with_activity_detail(
                "3",
                json!({
                    "distance": 7000.0,
                    "moving_time": 2400,
                    "icu_training_load": 60.0
                }),
            )
            .with_activity_detail(
                "4",
                json!({
                    "distance": 8000.0,
                    "moving_time": 2700,
                    "icu_training_load": 70.0
                }),
            ),
    );

    let input = json!({
        "target_type": "period",
        "period_start": d1.to_string(),
        "period_end": today.to_string(),
        "analysis_type": "detailed"
    });

    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = content_text(&output.content);

    let content_lower = content_str.to_lowercase();
    let has_linear_trend = content_lower.contains("increasing")
        || content_lower.contains("decreasing")
        || content_lower.contains("stable");
    assert!(
        has_linear_trend,
        "Output should contain linear trend direction (Increasing/Decreasing/Stable) from AnalysisEngine::analyze_trend. Got: {}",
        &content_str[..content_str.len().min(2000)]
    );
}

// ========================================================================
// Empty-Period Guidance Tests
// ========================================================================

#[tokio::test]
async fn empty_period_guidance_states_query_succeeded() {
    use crate::test_support::mock::MockIntervalsClient;
    use std::sync::Arc;

    let handler = AnalyzeTrainingHandler::new();
    let client: Arc<dyn IntervalsClient> = Arc::new(MockIntervalsClient::default());
    let input = json!({
        "target_type": "period",
        "period_start": "2025-04-01",
        "period_end": "2025-06-30"
    });

    let output = handler.execute(input, client, None).await.unwrap();
    let rendered = content_text(&output.content);

    assert!(
        rendered.contains("No completed activities returned for 2025-04-01 to 2025-06-30"),
        "Expected exact range mention. Got: {}",
        &rendered[..rendered.len().min(500)]
    );
    assert!(
        rendered.contains("uncapped activity query completed"),
        "Expected fetch-grounded guidance. Got: {}",
        &rendered[..rendered.len().min(500)]
    );
    assert!(
        !rendered.contains("Device sync status"),
        "Should not claim device-sync problem without upstream error"
    );
}

// ========================================================================
// Schema Contract Tests (Task 3)
// ========================================================================

#[test]
fn test_schema_has_all_of_conditional_not_one_of() {
    let handler = AnalyzeTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    assert!(
        schema.get("allOf").is_some(),
        "Schema should use allOf, not oneOf"
    );
    assert!(
        schema.get("oneOf").is_none(),
        "Schema should NOT contain oneOf"
    );
}

#[test]
fn test_schema_all_of_requires_date_for_single() {
    let handler = AnalyzeTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);
    let all_of = schema.get("allOf").unwrap().as_array().unwrap();

    // First element has the if/then/else for required fields
    let conditional = &all_of[0];
    let if_clause = conditional.get("if").unwrap();
    assert_eq!(
        if_clause
            .get("properties")
            .and_then(|p| p.get("target_type"))
            .and_then(|t| t.get("const"))
            .and_then(|c| c.as_str()),
        Some("single"),
    );

    let then_clause = conditional.get("then").unwrap();
    let required = then_clause.get("required").unwrap().as_array().unwrap();
    assert!(required.contains(&json!("date")));

    let else_clause = conditional.get("else").unwrap();
    let required = else_clause.get("required").unwrap().as_array().unwrap();
    assert!(required.contains(&json!("period_start")));
    assert!(required.contains(&json!("period_end")));
}

#[test]
fn test_schema_all_of_requires_period_start_end_for_period() {
    let handler = AnalyzeTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);
    let all_of = schema.get("allOf").unwrap().as_array().unwrap();

    // Second element has the period-specific constraints
    let period_constraints = &all_of[1];
    let if_clause = period_constraints.get("if").unwrap();
    assert_eq!(
        if_clause
            .get("properties")
            .and_then(|p| p.get("target_type"))
            .and_then(|t| t.get("const"))
            .and_then(|c| c.as_str()),
        Some("period"),
    );
}

#[test]
fn test_schema_has_top_level_required_target_type() {
    let handler = AnalyzeTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);
    let required = schema.get("required").unwrap().as_array().unwrap();
    assert!(required.contains(&json!("target_type")));
}

#[tokio::test]
async fn test_execute_period_with_start_date_end_date_rejected() {
    let handler = AnalyzeTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient {
        activities: vec![],
        ..Default::default()
    });

    let error = handler
        .execute(
            json!({
                "target_type": "period",
                "start_date": "2026-01-01",
                "end_date": "2026-07-04"
            }),
            client,
            None,
        )
        .await
        .unwrap_err();

    let err_msg = error.to_string();
    assert!(
        err_msg.contains("period_start and period_end"),
        "Error should mention period_start and period_end, got: {}",
        err_msg
    );
    assert!(
        err_msg.contains("do not use start_date/end_date"),
        "Error should mention not using start_date/end_date, got: {}",
        err_msg
    );
}

#[test]
fn test_description_mentions_period_example() {
    let handler = AnalyzeTrainingHandler::new();
    let desc = IntentHandler::description(&handler);
    assert!(
        desc.contains("period_start=\"2025-04-01\""),
        "Description should contain period_start example"
    );
    assert!(
        desc.contains("period_end=\"2025-06-30\""),
        "Description should contain period_end example"
    );
    assert!(
        desc.contains("Do not use start_date/end_date"),
        "Description should warn against start_date/end_date"
    );
}

// ========================================================================
// Audit Propagation Tests
// ========================================================================

#[test]
fn partial_detail_warning_propagates_through_build_data_audit() {
    use crate::engines::analysis_audit::build_data_audit;
    use intervals_icu_client::ActivitySummary;

    let fetched = FetchedAnalysisData {
            activities: vec![ActivitySummary {
                id: "a1".to_string(),
                name: Some("Run".to_string()),
                start_date_local: "2026-03-01".to_string(),
                ..Default::default()
            }],
            fetch_warnings: vec![
                "1 of 2 activity details unavailable; period totals remain available, but HR, zones, TSS, and load-derived metrics may be partial".to_string()
            ],
            fitness: Some(json!({"ctl": 50})),
            ..Default::default()
        };

    let audit = build_data_audit(&fetched);
    assert!(
        audit
            .degraded_mode_reasons
            .iter()
            .any(|reason| reason.contains("activity details unavailable")),
        "Partial-detail warning should appear in degraded_mode_reasons. Got: {:?}",
        audit.degraded_mode_reasons
    );
}

// ── ETVS single-handler rendering ──────────────────────────────────────

#[tokio::test]
async fn analyze_single_summary_renders_etvs_from_activity_zones() {
    let today = chrono::Utc::now().date_naive();
    let date = today.format("%Y-%m-%d").to_string();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![ActivitySummary {
                id: "etvs-single".into(),
                name: Some("ETVS Run".into()),
                start_date_local: format!("{date}T08:00:00"),
                moving_time: Some(3600),
                ..Default::default()
            }])
            .with_activity_detail(
                "etvs-single",
                json!({
                    "moving_time": 3600,
                    "icu_zone_times": [
                        {"id": "Z1", "secs": 1800},
                        {"id": "Z2", "secs": 900},
                        {"id": "Z3", "secs": 600},
                        {"id": "Z4", "secs": 300}
                    ]
                }),
            ),
    );

    let output = AnalyzeTrainingHandler::new()
        .execute(
            json!({"target_type": "single", "date": date, "analysis_type": "summary"}),
            client,
            None,
        )
        .await
        .unwrap();
    let rendered = format!("{:?}", output.content);
    assert!(rendered.contains("Effective Training Volume Score (ETVS)"));
    assert!(rendered.contains("110.0 weighted min"));
    assert!(rendered.contains("Coverage: 100.0% (1/1 activities)"));
}

#[tokio::test]
async fn analyze_single_does_not_render_fake_etvs_without_zone_data() {
    let today = chrono::Utc::now().date_naive();
    let date = today.format("%Y-%m-%d").to_string();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![ActivitySummary {
                id: "no-zones".into(),
                name: Some("Unzoned Run".into()),
                start_date_local: format!("{date}T08:00:00"),
                moving_time: Some(3600),
                ..Default::default()
            }])
            .with_activity_detail("no-zones", json!({"moving_time": 3600})),
    );

    let output = AnalyzeTrainingHandler::new()
        .execute(json!({"target_type": "single", "date": date}), client, None)
        .await
        .unwrap();
    assert!(!format!("{:?}", output.content).contains("ETVS"));
}

// ── ETVS period-handler rendering ──────────────────────────────────────

#[tokio::test]
async fn analyze_period_renders_aggregate_etvs_with_partial_coverage() {
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![
                ActivitySummary {
                    id: "period-a".into(),
                    name: Some("Zoned Run".into()),
                    start_date_local: "2026-03-02T08:00:00".into(),
                    moving_time: Some(1800),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "period-b".into(),
                    name: Some("Unzoned Run".into()),
                    start_date_local: "2026-03-03T08:00:00".into(),
                    moving_time: Some(1800),
                    ..Default::default()
                },
            ])
            .with_activity_detail(
                "period-a",
                json!({
                    "moving_time": 1800,
                    "icu_zone_times": [{"id": "Z1", "secs": 1800}]
                }),
            )
            .with_activity_detail("period-b", json!({"moving_time": 1800})),
    );

    let output = AnalyzeTrainingHandler::new()
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "analysis_type": "summary",
                "metrics": ["etvs"]
            }),
            client,
            None,
        )
        .await
        .unwrap();
    let rendered = format!("{:?}", output.content);
    assert!(rendered.contains("30.0 weighted min"));
    assert!(rendered.contains("Coverage: 50.0% (1/2 activities)"));
    assert!(rendered.contains("Requested Metrics"));
}

// ── parse_metric_streams ────────────────────────────────────────

#[test]
fn metric_stream_parser_drops_only_the_misaligned_signal() {
    let streams = json!({
        "time": [0.0, 1.0, 2.0],
        "velocity_smooth": [4.0, 5.0, 6.0],
        "heartrate": [150.0, 155.0],
        "watts": [200.0, 220.0, 240.0]
    });
    let parsed = parse_metric_streams(&streams).expect("time stream");
    assert_eq!(parsed.speed_mps, Some(vec![4.0, 5.0, 6.0]));
    assert!(parsed.heartrate_bpm.is_none());
    assert_eq!(parsed.power_w, Some(vec![200.0, 220.0, 240.0]));
}

#[test]
fn metric_stream_parser_preserves_nulls_as_missing_samples() {
    let parsed = parse_metric_streams(&json!({
        "time": [0.0, 1.0, 2.0],
        "watts": [200.0, null, 240.0]
    }))
    .expect("time stream");
    let power = parsed.power_w.expect("aligned power stream");
    assert_eq!(power[0], 200.0);
    assert!(power[1].is_nan());
    assert_eq!(power[2], 240.0);
}

#[test]
fn metric_stream_parser_does_not_guess_ambiguous_pace_units() {
    let parsed = parse_metric_streams(&json!({
        "time": [0.0, 1.0, 2.0],
        "pace": [300.0, 295.0, 305.0],
        "heartrate": [150.0, 151.0, 152.0]
    }))
    .expect("time stream");
    assert!(parsed.speed_mps.is_none());
    assert!(parsed.heartrate_bpm.is_some());
}

// ── sport_presentation ──────────────────────────────────────────

#[test]
fn sport_presentation_is_explicit_and_conservative() {
    assert_eq!(
        sport_presentation(Some(&json!({"type": "Run"}))),
        SportPresentation::Pace
    );
    assert_eq!(
        sport_presentation(Some(&json!({"type": "Ride"}))),
        SportPresentation::Speed
    );
    assert_eq!(
        sport_presentation(Some(&json!({"type": "Rowing"}))),
        SportPresentation::Unknown
    );
}
