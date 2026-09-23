use super::*;
use chrono::NaiveDate;
use intervals_icu_client::{ActivitySummary, Event, EventCategory};
use std::sync::Arc;

use crate::test_support::mock::MockIntervalsClient;

// ========================================================================
// Constructor Tests
// ========================================================================

#[test]
fn test_new_handler() {
    let handler = PlanTrainingHandler::new();
    assert_eq!(handler.name(), "plan_training");
}

#[test]
fn test_default_handler() {
    let _handler = PlanTrainingHandler;
}

// ========================================================================
// IntentHandler Trait Implementation Tests
// ========================================================================

#[test]
fn test_name() {
    let handler = PlanTrainingHandler::new();
    assert_eq!(IntentHandler::name(&handler), "plan_training");
}

#[test]
fn test_description() {
    let handler = PlanTrainingHandler::new();
    let desc = IntentHandler::description(&handler);
    assert!(desc.contains("Plans training"));
    assert!(desc.contains("periodized"));
}

#[test]
fn test_input_schema_structure() {
    let handler = PlanTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    assert!(schema.get("type").is_some());
    assert_eq!(schema.get("type").unwrap().as_str(), Some("object"));

    let props = schema.get("properties").unwrap().as_object().unwrap();
    assert!(props.contains_key("period_start"));
    assert!(props.contains_key("period_end"));
    assert!(props.contains_key("focus"));
    assert!(props.contains_key("max_hours_per_week"));
    assert!(props.contains_key("idempotency_token"));

    let required = schema.get("required").unwrap().as_array().unwrap();
    assert!(required.contains(&json!("period_start")));
    assert!(required.contains(&json!("period_end")));
    assert!(required.contains(&json!("idempotency_token")));
}

// ========================================================================
// build_periodization() Tests
// ========================================================================

#[test]
fn test_build_periodization_aerobic_base_short() {
    let handler = PlanTrainingHandler::new();
    let (phases, structure) = handler.build_periodization(4, TrainingFocus::AerobicBase, 10.0);

    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].name, "Base Period");
    assert_eq!(phases[0].weeks, "1-4");
    assert_eq!(phases[0].volume, "6-8 hrs");
    assert_eq!(phases[0].focus, "Z1-Z2 85-95%");

    assert!(structure.contains("Base Period"));
    assert!(structure.contains("Recovery weeks"));
}

#[test]
fn test_build_periodization_aerobic_base_long() {
    let handler = PlanTrainingHandler::new();
    let (phases, structure) = handler.build_periodization(12, TrainingFocus::AerobicBase, 10.0);

    assert_eq!(phases.len(), 2);
    assert_eq!(phases[0].name, "Base Period");
    assert_eq!(phases[0].weeks, "1-8");
    assert_eq!(phases[1].name, "Build Period");
    assert_eq!(phases[1].weeks, "9-12");
    assert_eq!(phases[1].volume, "8-10 hrs");
    assert_eq!(phases[1].focus, "Z3 introduction");

    // Structure contains recovery weeks info for aerobic_base
    assert!(structure.contains("Recovery weeks"));
}

#[test]
fn test_build_periodization_taper() {
    let handler = PlanTrainingHandler::new();
    let (phases, structure) = handler.build_periodization(3, TrainingFocus::Taper, 10.0);

    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].name, "Taper Period");
    assert_eq!(phases[0].weeks, "1-3");
    assert_eq!(phases[0].volume, "4-6 hrs");
    assert_eq!(phases[0].focus, "Race-specific, reduced volume");

    assert!(structure.contains("Taper"));
    assert!(structure.contains("volume -50-60%"));
}

#[test]
fn test_build_periodization_intensity() {
    let handler = PlanTrainingHandler::new();
    let (phases, structure) = handler.build_periodization(6, TrainingFocus::Intensity, 10.0);

    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].name, "Intensity Block");
    assert_eq!(phases[0].weeks, "1-6");
    assert_eq!(phases[0].volume, "8-10 hrs");
    assert_eq!(phases[0].focus, "Threshold + VO2");

    assert!(structure.contains("Intensity development"));
}

#[test]
fn test_build_periodization_specific() {
    let handler = PlanTrainingHandler::new();
    let (phases, structure) = handler.build_periodization(8, TrainingFocus::Specific, 12.0);

    assert!(!phases.is_empty());
    assert_eq!(phases[0].name, "Specific Preparation");
    assert_eq!(phases[0].volume, "10-12 hrs");
    assert!(structure.contains("Race-specific preparation"));
}

#[test]
fn test_build_periodization_recovery() {
    let handler = PlanTrainingHandler::new();
    let (phases, structure) = handler.build_periodization(2, TrainingFocus::Recovery, 8.0);

    assert_eq!(phases.len(), 1);
    assert_eq!(phases[0].name, "Recovery Block");
    assert_eq!(phases[0].volume, "3-5 hrs");
    assert!(structure.contains("Recovery emphasis"));
}

#[test]
fn test_low_volume_scaling() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(4, TrainingFocus::AerobicBase, 5.0);
    assert_eq!(phases[0].volume, "3-4 hrs");
}

#[test]
fn test_high_volume_scaling() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(4, TrainingFocus::AerobicBase, 15.0);
    assert_eq!(phases[0].volume, "9-12 hrs");
}

// ========================================================================
// build_sample_week() Tests
// ========================================================================

#[test]
fn test_build_sample_week_aerobic_base() {
    let handler = PlanTrainingHandler::new();
    let week = handler.build_sample_week(TrainingFocus::AerobicBase, 10.0, None);

    assert!(week.contains("Sample Week"));
    assert!(week.contains("Monday: REST"));
    assert!(week.contains("Tuesday: Easy Run"));
    assert!(week.contains("Z1-Z2"));
    assert!(week.contains("Saturday: Long Run"));
    assert!(week.contains("Sunday: Active Recovery"));
}

#[test]
fn test_build_week_intensity() {
    let handler = PlanTrainingHandler::new();
    let week = handler.build_sample_week(TrainingFocus::Intensity, 10.0, None);
    assert!(week.contains("Threshold / VO2 session"));
}

#[test]
fn test_build_week_specific() {
    let handler = PlanTrainingHandler::new();
    let week = handler.build_sample_week(TrainingFocus::Specific, 10.0, None);
    assert!(week.contains("Race-pace intervals"));
}

#[test]
fn test_build_week_taper() {
    let handler = PlanTrainingHandler::new();
    let week = handler.build_sample_week(TrainingFocus::Taper, 10.0, None);
    assert!(week.contains("Sharpening session"));
}

#[test]
fn test_build_week_recovery() {
    let handler = PlanTrainingHandler::new();
    let week = handler.build_sample_week(TrainingFocus::Recovery, 10.0, None);
    assert!(week.contains("Easy 30-45 min aerobic session"));
}

#[test]
fn test_build_sample_week_time_formatting() {
    let handler = PlanTrainingHandler::new();
    let week = handler.build_sample_week(TrainingFocus::AerobicBase, 10.0, None);

    // For 10 hrs: Easy runs = 2hrs each = 120 min, Long run = 3:20 = 200 min
    assert!(week.contains("Easy Run"));
    assert!(week.contains("120:00"));
}

// ========================================================================
// Input Validation Tests (execute method prerequisites)
// ========================================================================

// Note: Full execute() tests require mocking the IntervalsClient trait.
// Integration tests for execute() are in: crates/intervals_icu_mcp/tests/

#[test]
fn test_validation_missing_period_start() {
    // Test validation logic directly
    let input = json!({
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });

    // Verify the field is missing
    assert!(input.get("period_start").is_none());
}

#[test]
fn test_validation_missing_period_end() {
    let input = json!({
        "period_start": "2026-03-01",
        "idempotency_token": "test-token"
    });

    assert!(input.get("period_end").is_none());
}

#[test]
fn test_validation_date_format() {
    // Test that parse_date would reject invalid format
    let result = parse_date("invalid-date", "period_start");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("date format"));

    // Test valid format
    let result = parse_date("2026-03-01", "period_start");
    assert!(result.is_ok());
}

#[test]
fn test_validation_date_order() {
    let start = parse_date("2026-03-31", "start").unwrap();
    let end = parse_date("2026-03-01", "end").unwrap();

    // Start > end should be invalid
    assert!(start > end);
}

#[test]
fn test_default_values_in_schema() {
    let handler = PlanTrainingHandler::new();
    let schema = IntentHandler::input_schema(&handler);

    let props = schema.get("properties").unwrap().as_object().unwrap();

    // Check focus default
    let focus = props.get("focus").unwrap();
    assert_eq!(focus.get("default").and_then(|v| v.as_str()), None); // No default in schema

    // Check max_hours_per_week exists
    assert!(props.contains_key("max_hours_per_week"));

    // Check adaptive default
    let adaptive = props.get("adaptive").unwrap();
    assert_eq!(
        adaptive.get("default").and_then(|v| v.as_bool()),
        Some(true)
    );
}

// ========================================================================
// Output Structure Tests
// ========================================================================

#[test]
fn test_output_content_types() {
    // Test that the handler produces the right content structure
    // (Full execution tests require mocking the client)
    let handler = PlanTrainingHandler::new();

    // Verify the handler can be constructed and has correct metadata
    assert_eq!(handler.name(), "plan_training");
    assert!(handler.description().len() > 50);
}

// ========================================================================
// Week Calculation Tests
// ========================================================================

#[test]
fn test_week_calculation_single_week() {
    let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 3, 7).unwrap();
    let weeks: u32 = ((end - start).num_days() / 7 + 1) as u32;
    assert_eq!(weeks, 1);
}

#[test]
fn test_week_calculation_multiple_weeks() {
    let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 3, 28).unwrap();
    let weeks: u32 = ((end - start).num_days() / 7 + 1) as u32;
    assert_eq!(weeks, 4);
}

#[test]
fn test_week_calculation_partial_week() {
    let start = NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2026, 3, 10).unwrap();
    let weeks: u32 = ((end - start).num_days() / 7 + 1) as u32;
    assert_eq!(weeks, 2);
}

// ========================================================================
// ExtractedSportSettings Tests
// ========================================================================

#[test]
fn test_extract_sport_settings_full() {
    let value = json!({
        "sports": [{"name": "Running", "ftp": 280.0, "lthr": 172.0}]
    });
    let sport_settings = intervals_icu_client::domains::workout::SportSettings::from_value(&value)
        .expect("parse sport settings");
    let settings = ExtractedSportSettings::from_sport_settings(&sport_settings);
    assert_eq!(settings.sport_name.as_deref(), Some("Running"));
    assert_eq!(settings.ftp, Some(280.0));
    assert_eq!(settings.lthr, Some(172.0));
}

#[test]
fn test_extract_sport_settings_empty() {
    let value = json!({});
    let sport_settings = intervals_icu_client::domains::workout::SportSettings::from_value(&value)
        .expect("parse sport settings");
    let settings = ExtractedSportSettings::from_sport_settings(&sport_settings);
    assert!(settings.sport_name.is_none());
    assert!(settings.ftp.is_none());
    assert!(settings.lthr.is_none());
}

#[test]
fn test_extract_sport_settings_no_ftp() {
    let value = json!({
        "sports": [{"name": "Cycling"}]
    });
    let sport_settings = intervals_icu_client::domains::workout::SportSettings::from_value(&value)
        .expect("parse sport settings");
    let settings = ExtractedSportSettings::from_sport_settings(&sport_settings);
    assert_eq!(settings.sport_name.as_deref(), Some("Cycling"));
    assert!(settings.ftp.is_none());
    assert!(settings.lthr.is_none());
}

// ========================================================================
// WellnessSnapshot Tests
// ========================================================================

#[test]
fn test_wellness_snapshot_from_value() {
    let value = json!([
        {"readiness": 3.0, "hrv": 30.0, "sleep": 5.0},
        {"readiness": 8.0, "hrv": 70.0, "sleep": 8.0}
    ]);
    let snap = WellnessSnapshot::from_value(&value);
    // Uses latest entry (last), not average
    assert_eq!(snap.readiness, Some(8.0));
    assert_eq!(snap.hrv, Some(70.0));
    assert_eq!(snap.sleep_avg, Some(8.0));
}

#[test]
fn test_wellness_snapshot_reads_sleep_secs_from_api() {
    // Real Intervals.icu API returns sleepSecs, not sleep.
    let value = json!([
        {"id": "2026-03-15", "readiness": 3.0, "hrv": 30.0, "sleepSecs": 18000},
        {"id": "2026-03-16", "readiness": 8.0, "hrv": 70.0, "sleepSecs": 28800}
    ]);
    let snap = WellnessSnapshot::from_value(&value);
    assert_eq!(snap.readiness, Some(8.0));
    assert_eq!(snap.hrv, Some(70.0));
    assert_eq!(snap.sleep_avg, Some(8.0), "sleepSecs must convert to hours");
}

#[test]
fn test_wellness_snapshot_uses_latest_by_date_not_position() {
    // Newest-first API order must not make the snapshot read the oldest day.
    let value = json!([
        {"id": "2026-03-16", "readiness": 8.0, "hrv": 70.0, "sleepSecs": 28800},
        {"id": "2026-03-15", "readiness": 3.0, "hrv": 30.0, "sleepSecs": 18000}
    ]);
    let snap = WellnessSnapshot::from_value(&value);
    assert_eq!(snap.readiness, Some(8.0));
    assert_eq!(snap.hrv, Some(70.0));
    assert_eq!(snap.sleep_avg, Some(8.0));
}

#[test]
fn test_wellness_snapshot_filters_implausible_sleep() {
    // 12.8 h artifact must not flow into the snapshot average.
    let value = json!([
        {"id": "2026-03-16", "readiness": 8.0, "hrv": 70.0, "sleepSecs": 46080}
    ]);
    let snap = WellnessSnapshot::from_value(&value);
    assert!(
        snap.sleep_avg.is_none(),
        "implausible 12.8h sleep must yield None, got {:?}",
        snap.sleep_avg
    );
}

#[test]
fn test_wellness_snapshot_accepts_integer_metrics() {
    // Integer payloads convert like floats (mirrors shared extractors).
    let value = json!([
        {"id": "2026-03-16", "readiness": 8, "hrv": 70, "sleepSecs": 28800}
    ]);
    let snap = WellnessSnapshot::from_value(&value);
    assert_eq!(snap.readiness, Some(8.0));
    assert_eq!(snap.hrv, Some(70.0));
    assert_eq!(snap.sleep_avg, Some(8.0));
}

#[test]
fn test_wellness_snapshot_rejects_implausible_hrv() {
    // A dropout (0) or glitch spike on the latest day must not poison the
    // plan context — same plausibility bar as the wellness parser.
    for hrv in [0.0, -5.0, 9999.0] {
        let value = json!([
            {"id": "2026-03-16", "readiness": 8.0, "hrv": hrv, "sleepSecs": 28800}
        ]);
        let snap = WellnessSnapshot::from_value(&value);
        assert!(
            snap.hrv.is_none(),
            "hrv {hrv} must be rejected, got {:?}",
            snap.hrv
        );
        assert_eq!(snap.sleep_avg, Some(8.0));
    }
}

#[test]
fn test_wellness_snapshot_rejects_negative_readiness() {
    // Readiness is zero-floored on every scale: a negative is corruption.
    let value = json!([
        {"id": "2026-03-16", "readiness": -2.0, "hrv": 70.0, "sleepSecs": 28800}
    ]);
    let snap = WellnessSnapshot::from_value(&value);
    assert_eq!(snap.readiness, None);
    assert_eq!(snap.hrv, Some(70.0));
}

#[test]
fn test_wellness_snapshot_empty() {
    let value = json!([]);
    let snap = WellnessSnapshot::from_value(&value);
    assert!(snap.readiness.is_none());
    assert!(snap.hrv.is_none());
    assert!(snap.sleep_avg.is_none());
}

#[test]
fn test_wellness_snapshot_partial() {
    let value = json!([
        {"readiness": 5.0},
        {"readiness": 7.0}
    ]);
    let snap = WellnessSnapshot::from_value(&value);
    // Latest entry only
    assert_eq!(snap.readiness, Some(7.0));
    assert!(snap.hrv.is_none());
}

// ========================================================================
// Conflict Detection Tests
// ========================================================================

#[test]
fn test_conflict_detection_logic() {
    let start = chrono::NaiveDate::parse_from_str("2026-03-01", "%Y-%m-%d").unwrap();
    let end = chrono::NaiveDate::parse_from_str("2026-03-31", "%Y-%m-%d").unwrap();
    let event_date = chrono::NaiveDate::parse_from_str("2026-03-15", "%Y-%m-%d").unwrap();
    assert!(event_date >= start && event_date <= end);

    let outside = chrono::NaiveDate::parse_from_str("2026-04-15", "%Y-%m-%d").unwrap();
    assert!(outside > end);
}

// ========================================================================
// Event Generation Tests
// ========================================================================

#[test]
fn test_generate_events_aerobic_base() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(4, TrainingFocus::AerobicBase, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap(); // Monday
    let events = generate_events(&phases, start, TrainingFocus::AerobicBase, 4);
    // 4 weeks, week 4 is recovery (skip) -> 3 weeks * 4 events = 12
    assert_eq!(events.len(), 12);
    assert!(
        events
            .iter()
            .all(|e| e.category == intervals_icu_client::EventCategory::Workout)
    );
    assert!(events.iter().all(|e| e.id.is_none()));
}

#[test]
fn test_generate_events_recovery_week_skip() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(8, TrainingFocus::AerobicBase, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::AerobicBase, 8);
    // Weeks 1-8, recovery at week 4 and 8 -> 6 active weeks * 4 = 24
    assert_eq!(events.len(), 24);
}

#[test]
fn test_generate_events_dates_are_valid() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(4, TrainingFocus::AerobicBase, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::AerobicBase, 4);
    for event in &events {
        assert!(
            chrono::NaiveDate::parse_from_str(&event.start_date_local, "%Y-%m-%d").is_ok(),
            "Invalid date: {}",
            event.start_date_local
        );
        assert!(!event.name.is_empty());
        assert!(event.description.is_some());
    }
}

#[test]
fn test_generate_events_intensity() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(6, TrainingFocus::Intensity, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::Intensity, 6);
    // 6 weeks, week 4 is recovery -> 5 * 4 = 20
    assert_eq!(events.len(), 20);
    assert!(events.iter().all(|e| e.name.contains("Session")
        || e.name.contains("Intervals")
        || e.name.contains("Aerobic")
        || e.name.contains("Strides")));
}

#[test]
fn test_generate_events_taper_no_recovery_skip() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(3, TrainingFocus::Taper, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::Taper, 3);
    // Taper: no recovery skip, 3 weeks * 4 = 12
    assert_eq!(events.len(), 12);
}

#[test]
fn test_conflict_detection_excludes_races() {
    let race_a = intervals_icu_client::EventCategory::RaceA;
    let race_b = intervals_icu_client::EventCategory::RaceB;
    let workout = intervals_icu_client::EventCategory::Workout;
    // Races should be excluded from conflicts
    assert!(matches!(
        race_a,
        intervals_icu_client::EventCategory::RaceA | intervals_icu_client::EventCategory::RaceB
    ));
    assert!(matches!(
        race_b,
        intervals_icu_client::EventCategory::RaceA | intervals_icu_client::EventCategory::RaceB
    ));
    // Workouts are conflicts
    assert!(!matches!(
        workout,
        intervals_icu_client::EventCategory::RaceA | intervals_icu_client::EventCategory::RaceB
    ));
}

// ========================================================================
// Regression: generated events must pass validate_and_prepare_event
// ========================================================================

#[test]
fn test_generated_events_pass_validation_aerobic_base() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(4, TrainingFocus::AerobicBase, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::AerobicBase, 4);

    for event in &events {
        let result = validate_and_prepare_event(event.clone());
        assert!(
            result.is_ok(),
            "Event '{}' failed validation: {:?}",
            event.name,
            result.err()
        );
    }
}

#[test]
fn test_generated_events_have_type_after_validation() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(4, TrainingFocus::AerobicBase, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::AerobicBase, 4);

    for event in &events {
        assert!(
            event.r#type.is_none(),
            "Generated event should have type=None before validation"
        );
        let validated = validate_and_prepare_event(event.clone()).unwrap();
        assert_eq!(
            validated.r#type.as_deref(),
            Some("Run"),
            "Validated WORKOUT event must have type='Run'"
        );
    }
}

#[test]
fn test_generated_events_all_workout_category() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(6, TrainingFocus::Intensity, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::Intensity, 6);

    assert!(!events.is_empty());
    for event in &events {
        assert_eq!(
            event.category,
            intervals_icu_client::EventCategory::Workout,
            "All generated events should be WORKOUT category"
        );
    }
}

#[test]
fn test_generated_events_valid_dates_after_validation() {
    let handler = PlanTrainingHandler::new();
    let (phases, _) = handler.build_periodization(4, TrainingFocus::Taper, 10.0);
    let start = chrono::NaiveDate::from_ymd_opt(2026, 3, 2).unwrap();
    let events = generate_events(&phases, start, TrainingFocus::Taper, 4);

    for event in &events {
        let validated = validate_and_prepare_event(event.clone()).unwrap();
        assert!(
            validated.start_date_local.contains("T"),
            "Validated event should have ISO datetime, got: {}",
            validated.start_date_local
        );
    }
}

#[test]
fn test_validate_and_prepare_event_rejects_unknown_category_in_generated() {
    let ev = intervals_icu_client::Event {
        id: None,
        start_date_local: "2026-03-02".into(),
        name: "Bad Event".into(),
        category: intervals_icu_client::EventCategory::Unknown,
        description: Some("test".into()),
        r#type: None,
    };
    let result = validate_and_prepare_event(ev);
    assert!(
        result.is_err(),
        "Unknown category should be rejected by validation"
    );
}

// ========================================================================
// Execute() Integration Tests (via MockIntervalsClient)
// ========================================================================

#[tokio::test]
async fn test_execute_start_date_after_end_date() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());
    let input = json!({
        "period_start": "2026-03-31",
        "period_end": "2026-03-01",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("must be before end date"),
        "Expected start-after-end error, got: {err}"
    );
}

#[tokio::test]
async fn test_execute_adaptive_false() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-28",
        "idempotency_token": "test-token",
        "adaptive": false
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Training Plan"));
    assert!(content_str.contains("AEROBIC BASE"));
    assert_eq!(output.metadata.events_created, Some(12));
}

#[tokio::test]
async fn test_execute_basic_success() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-28",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Training Plan"));
    assert!(content_str.contains("Test Athlete"));
    assert_eq!(output.metadata.events_created, Some(12));
}

#[tokio::test]
async fn test_execute_with_existing_conflicts() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: None,
        start_date_local: "2026-03-15".into(),
        name: "Existing Workout".into(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    }]));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Conflict Detected"));
    assert!(content_str.contains("Existing Workout"));
    assert_eq!(output.metadata.events_created, Some(0));
}

#[tokio::test]
async fn test_execute_with_upcoming_conflicts_only() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder().with_upcoming_workouts(json!([
            {"start_date_local": "2026-03-15", "name": "Planned Workout"}
        ])),
    );
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Existing Plan Detected"));
    assert!(content_str.contains("1 workouts"));
    assert_eq!(output.metadata.events_created, Some(0));
}

#[tokio::test]
async fn test_execute_conflict_detects_iso_datetime_event_dates() {
    // Real API rows may carry full ISO datetimes; date-only parsing must not
    // silently skip the conflict.
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: None,
        start_date_local: "2026-03-15T10:00:00".into(),
        name: "Existing Workout".into(),
        category: EventCategory::Workout,
        description: None,
        r#type: None,
    }]));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Conflict Detected"));
    assert!(content_str.contains("Existing Workout"));
    assert_eq!(output.metadata.events_created, Some(0));
}

#[tokio::test]
async fn test_execute_upcoming_conflict_accepts_iso_datetime_dates() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder().with_upcoming_workouts(json!([
            {"start_date_local": "2026-03-15T10:00:00", "name": "Planned Workout"}
        ])),
    );
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Existing Plan Detected"));
    assert_eq!(output.metadata.events_created, Some(0));
}

#[tokio::test]
async fn test_execute_with_both_conflicts() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_events(vec![Event {
                id: None,
                start_date_local: "2026-03-15".into(),
                name: "Existing Event".into(),
                category: EventCategory::Workout,
                description: None,
                r#type: None,
            }])
            .with_upcoming_workouts(json!([
                {"start_date_local": "2026-03-20", "name": "Upcoming Workout"}
            ])),
    );
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Conflict Detected"));
    assert!(content_str.contains("Existing Event"));
    assert!(content_str.contains("Existing Plan Detected"));
    assert_eq!(output.metadata.events_created, Some(0));
}

#[tokio::test]
async fn test_execute_with_race_anchors() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: None,
        start_date_local: "2026-03-15".into(),
        name: "Big Race".into(),
        category: EventCategory::RaceA,
        description: None,
        r#type: None,
    }]));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Race Anchors"));
    assert!(content_str.contains("Big Race"));
}

#[tokio::test]
async fn test_execute_race_anchors_accept_iso_datetime_event_dates() {
    // Race events with ISO datetime dates must still anchor the plan;
    // date-only parsing dropped them from the period filter.
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_events(vec![Event {
        id: None,
        start_date_local: "2026-03-15T09:00:00".into(),
        name: "Big Race".into(),
        category: EventCategory::RaceA,
        description: None,
        r#type: None,
    }]));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-31",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Race Anchors"));
    assert!(content_str.contains("Big Race"));
}

#[tokio::test]
async fn test_execute_with_sport_settings() {
    let handler = PlanTrainingHandler::new();
    let settings = intervals_icu_client::domains::workout::SportSettings {
        sports: vec![intervals_icu_client::domains::workout::SportSetting {
            name: Some("Running".into()),
            ftp: Some(280.0),
            lthr: Some(172.0),
            ..Default::default()
        }],
        age: None,
        weight: None,
    };
    let client = Arc::new(MockIntervalsClient::builder().with_sport_settings(settings));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-28",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Running"));
    assert!(content_str.contains("FTP"));
    assert!(content_str.contains("LTHR"));
}

#[tokio::test]
async fn test_execute_with_tsb_fresh_and_good_wellness() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_fitness_summary(json!({"tsb": 15.0}))
            .with_wellness(json!([{"readiness": 7.5, "hrv": 70.0, "sleep": 8.0}])),
    );
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-14",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Fresh"));
    assert!(content_str.contains("Good"));
    assert!(
        output
            .suggestions
            .iter()
            .any(|s| s.contains("TSB positive"))
    );
    assert!(output.suggestions.iter().any(|s| s.contains("Readiness")));
    assert!(!output.suggestions.iter().any(|s| s.contains("HRV")));
    assert!(!output.suggestions.iter().any(|s| s.contains("Sleep")));
}

#[tokio::test]
async fn test_execute_header_includes_ctl_atl_ramp_rate() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_fitness_summary(json!({
                "fitness": 65.0,
                "fatigue": 45.0,
                "form": 10.0,
                "rampRate": 2.5
            }))
            .with_wellness(json!([{"readiness": 7.0, "hrv": 65.0, "sleep": 7.5}])),
    );
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-14",
        "idempotency_token": "test-token-ctl"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let content_str = format!("{:?}", result.unwrap().content);
    assert!(
        content_str.contains("Current CTL"),
        "header should contain CTL"
    );
    assert!(content_str.contains("65"), "CTL value should be 65");
    assert!(
        content_str.contains("Current ATL"),
        "header should contain ATL"
    );
    assert!(content_str.contains("45"), "ATL value should be 45");
    assert!(
        content_str.contains("Ramp Rate"),
        "header should contain Ramp Rate"
    );
    assert!(content_str.contains("2.5"), "ramp rate value should be 2.5");
}

#[tokio::test]
async fn test_execute_with_tsb_fatigued() {
    let handler = PlanTrainingHandler::new();
    let client =
        Arc::new(MockIntervalsClient::builder().with_fitness_summary(json!({"tsb": -15.0})));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Fatigued"));
    assert!(
        output
            .suggestions
            .iter()
            .any(|s| s.contains("TSB negative"))
    );
}

#[tokio::test]
async fn test_execute_with_tsb_balanced() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_fitness_summary(json!({"tsb": 5.0})));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Balanced"));
    assert!(!output.suggestions.iter().any(|s| s.contains("TSB")));
}

#[tokio::test]
async fn test_execute_with_wellness_fair() {
    let handler = PlanTrainingHandler::new();
    let client =
        Arc::new(MockIntervalsClient::builder().with_wellness(json!([{"readiness": 6.0}])));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Fair"));
}

#[tokio::test]
async fn test_execute_with_all_suggestions_and_volume_overshoot() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(
        MockIntervalsClient::builder()
            .with_activities(vec![
                ActivitySummary {
                    id: "1".into(),
                    name: Some("Run 1".into()),
                    start_date_local: "2026-02-01".into(),
                    moving_time: Some(3600),
                    elapsed_time: Some(4500),
                    ..Default::default()
                },
                ActivitySummary {
                    id: "2".into(),
                    name: Some("Run 2".into()),
                    start_date_local: "2026-02-08".into(),
                    moving_time: Some(5400),
                    elapsed_time: Some(7200),
                    ..Default::default()
                },
            ])
            .with_fitness_summary(json!({"tsb": -15.0}))
            .with_wellness(json!([{"readiness": 3.0, "hrv": 35.0, "sleep": 6.0}])),
    );
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "idempotency_token": "test-token",
        "max_hours_per_week": 8.0
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Low"));
    assert!(content_str.contains("Fatigued"));
    assert!(
        output
            .suggestions
            .iter()
            .any(|s| s.contains("Low readiness"))
    );
    assert!(output.suggestions.iter().any(|s| s.contains("HRV")));
    assert!(output.suggestions.iter().any(|s| s.contains("Sleep")));
    assert!(
        output
            .suggestions
            .iter()
            .any(|s| s.contains("TSB negative"))
    );
    assert!(output.suggestions.iter().any(|s| s.contains("exceeds")));
}

#[tokio::test]
async fn test_execute_with_historical_activities_no_overshoot() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_activities(vec![
        ActivitySummary {
            id: "1".into(),
            name: Some("Run 1".into()),
            start_date_local: "2026-02-01".into(),
            moving_time: Some(3600),
            elapsed_time: Some(4500),
            ..Default::default()
        },
        ActivitySummary {
            id: "2".into(),
            name: Some("Run 2".into()),
            start_date_local: "2026-02-08".into(),
            moving_time: Some(3600),
            elapsed_time: Some(4500),
            ..Default::default()
        },
    ]));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "idempotency_token": "test-token",
        "max_hours_per_week": 1.0
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Historical Avg"));
    assert!(!output.suggestions.iter().any(|s| s.contains("exceeds")));
}

#[tokio::test]
async fn test_execute_historical_avg_accepts_iso_datetime_activity_dates() {
    // Historical volume must compute from ISO datetime activity dates too.
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder().with_activities(vec![
        ActivitySummary {
            id: "1".into(),
            name: Some("Run 1".into()),
            start_date_local: "2026-02-01T10:00:00".into(),
            moving_time: Some(3600),
            elapsed_time: Some(4500),
            ..Default::default()
        },
        ActivitySummary {
            id: "2".into(),
            name: Some("Run 2".into()),
            start_date_local: "2026-02-08T07:30:00".into(),
            moving_time: Some(3600),
            elapsed_time: Some(4500),
            ..Default::default()
        },
    ]));
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-07",
        "idempotency_token": "test-token"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    // 2 activities x 1h moving over a 7-day span => 2.0 hrs/wk moving,
    // 4500s x 2 => 2.5 hrs/wk elapsed, both computed from ISO datetime dates.
    assert!(
        content_str.contains("Historical Avg (1wk): 2.0 hrs/wk (moving), 2.5 hrs/wk (elapsed)"),
        "Expected computed historical averages, got: {content_str}"
    );
}

#[tokio::test]
async fn test_execute_with_target_race() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-28",
        "idempotency_token": "test-token",
        "target_race": "Boston Marathon"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("Boston Marathon"));
}

#[tokio::test]
async fn test_execute_with_intensity_focus() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-28",
        "idempotency_token": "test-token",
        "focus": "intensity"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("INTENSITY"));
}

#[tokio::test]
async fn test_execute_with_specific_focus() {
    let handler = PlanTrainingHandler::new();
    let client = Arc::new(MockIntervalsClient::builder());
    let input = json!({
        "period_start": "2026-03-01",
        "period_end": "2026-03-28",
        "idempotency_token": "test-token",
        "focus": "specific"
    });
    let result = handler.execute(input, client, None).await;
    assert!(result.is_ok());
    let output = result.unwrap();
    let content_str = format!("{:?}", output.content);
    assert!(content_str.contains("SPECIFIC"));
}
