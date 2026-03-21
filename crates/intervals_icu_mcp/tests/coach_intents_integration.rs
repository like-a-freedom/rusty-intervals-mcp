use async_trait::async_trait;
use chrono::{Duration, Utc};
use intervals_icu_client::{
    ActivityMessage, ActivitySummary, AthleteProfile, BestEffortsOptions, DownloadProgress, Event,
    IntervalsClient, IntervalsError,
};
use intervals_icu_mcp::intents::handlers::{
    AnalyzeRaceHandler, AnalyzeTrainingHandler, AssessRecoveryHandler, ComparePeriodsHandler,
    ManageProfileHandler, PlanTrainingHandler,
};
use intervals_icu_mcp::intents::{ContentBlock, IntentHandler};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, OnceLock};

fn wellness_days_requests() -> &'static Mutex<Vec<Option<i32>>> {
    static WELLNESS_DAYS_REQUESTS: OnceLock<Mutex<Vec<Option<i32>>>> = OnceLock::new();
    WELLNESS_DAYS_REQUESTS.get_or_init(|| Mutex::new(Vec::new()))
}

struct MockCoachClient {
    activities: Vec<ActivitySummary>,
    events: Vec<Event>,
    fitness: Value,
    wellness: Value,
    wellness_for_date: Value,
    upcoming_workouts: Value,
    activity_details: Value,
    intervals: Value,
    streams: Value,
    best_efforts: Value,
    sport_settings: Value,
    hr_histogram: Value,
    power_histogram: Value,
    pace_histogram: Value,
}

impl MockCoachClient {
    fn adaptive_wellness_series(
        baseline_sleep_secs: f64,
        baseline_resting_hr: f64,
        baseline_hrv: f64,
        recent_sleep_secs: f64,
        recent_resting_hr: f64,
        recent_hrv: f64,
    ) -> Value {
        let mut entries = Vec::new();
        entries.extend((0..28).map(|_| {
            json!({
                "sleepSecs": baseline_sleep_secs,
                "restingHR": baseline_resting_hr,
                "hrv": baseline_hrv
            })
        }));
        entries.extend((0..7).map(|_| {
            json!({
                "sleepSecs": recent_sleep_secs,
                "restingHR": recent_resting_hr,
                "hrv": recent_hrv
            })
        }));
        Value::Array(entries)
    }

    fn relative_date(days_from_today: i64) -> String {
        (Utc::now().date_naive() + Duration::days(days_from_today))
            .format("%Y-%m-%d")
            .to_string()
    }

    fn mock_event(event_id: Option<&str>) -> Event {
        Event {
            id: event_id.map(str::to_owned),
            start_date_local: "2026-03-04".to_string(),
            name: "Mock event".to_string(),
            category: intervals_icu_client::EventCategory::Workout,
            description: None,
            r#type: None,
        }
    }

    fn with_tsb(tsb: f64) -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "activity-1".to_string(),
                name: Some("Hard Session".to_string()),
                start_date_local: "2026-03-04".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 50.0, "fatigue": 75.0, "form": tsb }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 10000.0,
                "moving_time": 3600,
                "average_heartrate": 150.0,
                "average_watts": 220.0,
                "total_elevation_gain": 200.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_race_activity() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "race-1".to_string(),
                name: Some("Mountain 50K".to_string()),
                start_date_local: "2026-03-01".to_string(),
            }],
            events: vec![Event {
                id: Some("event-race-1".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "Mountain 50K Plan".to_string(),
                category: intervals_icu_client::EventCategory::RaceA,
                description: Some("Planned race target".to_string()),
                r#type: Some("Race".to_string()),
            }],
            fitness: json!([{ "fitness": 42.0, "fatigue": 68.0, "form": -18.0 }]),
            wellness: json!([
                {"sleepSecs": 21600.0, "restingHR": 58.0, "hrv": 45.0},
                {"sleepSecs": 21000.0, "restingHR": 60.0, "hrv": 42.0}
            ]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 50000.0,
                "moving_time": 18000,
                "average_heartrate": 148.0,
                "total_elevation_gain": 1800.0
            }),
            intervals: json!([
                {"moving_time": 1800, "average_heartrate": 145.0, "average_watts": 210.0},
                {"moving_time": 1800, "average_heartrate": 152.0, "average_watts": 205.0}
            ]),
            streams: json!({
                "velocity_smooth": [3.0, 3.0, 3.0, 3.0, 3.0, 3.0],
                "heartrate": [140.0, 141.0, 142.0, 150.0, 151.0, 152.0],
                "watts": [220.0, 220.0, 220.0, 220.0, 220.0, 220.0]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_period_blocks() -> Self {
        Self {
            activities: vec![
                ActivitySummary {
                    id: "a1".to_string(),
                    name: Some("Run 1".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                },
                ActivitySummary {
                    id: "a2".to_string(),
                    name: Some("Run 2".to_string()),
                    start_date_local: "2026-03-03".to_string(),
                },
                ActivitySummary {
                    id: "a3".to_string(),
                    name: Some("Run 3".to_string()),
                    start_date_local: "2026-02-25".to_string(),
                },
            ],
            events: vec![],
            fitness: json!([{ "fitness": 55.0, "fatigue": 45.0, "form": 10.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 15000.0,
                "moving_time": 5400,
                "average_heartrate": 145.0,
                "average_watts": 210.0,
                "total_elevation_gain": 300.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_single_workout_degraded_streams() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "single-1".to_string(),
                name: Some("Track Intervals".to_string()),
                start_date_local: "2026-03-04".to_string(),
            }],
            events: vec![],
            fitness: json!({}),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12000.0,
                "moving_time": 4200,
                "average_heartrate": 158.0,
                "average_watts": 245.0,
                "total_elevation_gain": 90.0
            }),
            intervals: json!([
                {"moving_time": 300, "average_heartrate": 162.0, "average_watts": 265.0},
                {"moving_time": 300, "average_heartrate": 164.0, "average_watts": 268.0}
            ]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_race_degraded_context() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "race-degraded-1".to_string(),
                name: Some("Spring Marathon".to_string()),
                start_date_local: "2026-03-02".to_string(),
            }],
            events: vec![],
            fitness: json!({}),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 42195.0,
                "moving_time": 12600,
                "average_heartrate": 151.0,
                "total_elevation_gain": 180.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_positive_tsb_and_low_sleep() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "activity-1".to_string(),
                name: Some("Sharpening Session".to_string()),
                start_date_local: "2026-03-04".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 62.0, "fatigue": 48.0, "form": 14.0 }]),
            wellness: json!([
                {"sleepSecs": 19800.0, "restingHR": 58.0, "hrv": 30.0},
                {"sleepSecs": 20700.0, "restingHR": 60.0, "hrv": 34.0}
            ]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12000.0,
                "moving_time": 4300,
                "average_heartrate": 150.0,
                "average_watts": 230.0,
                "total_elevation_gain": 120.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_supportive_recovery_metrics() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "supportive-1".to_string(),
                name: Some("Pre-race tune-up".to_string()),
                start_date_local: "2026-03-04".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 64.0, "fatigue": 46.0, "form": 15.0 }]),
            wellness: json!([
                {"sleepSecs": 28800.0, "restingHR": 48.0, "hrv": 74.0},
                {"sleepSecs": 28200.0, "restingHR": 49.0, "hrv": 71.0}
            ]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 10000.0,
                "moving_time": 3300,
                "average_heartrate": 142.0,
                "average_watts": 225.0,
                "total_elevation_gain": 80.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_personal_hrv_drop_profile() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "adaptive-drop-1".to_string(),
                name: Some("Quality Session".to_string()),
                start_date_local: "2026-03-04".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 62.0, "fatigue": 48.0, "form": 14.0 }]),
            wellness: Self::adaptive_wellness_series(28_800.0, 50.0, 60.0, 28_800.0, 50.0, 45.0),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12000.0,
                "moving_time": 4300,
                "average_heartrate": 150.0,
                "average_watts": 230.0,
                "total_elevation_gain": 120.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_personal_hrv_norm_profile() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "adaptive-norm-1".to_string(),
                name: Some("Quality Session".to_string()),
                start_date_local: "2026-03-04".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 62.0, "fatigue": 48.0, "form": 14.0 }]),
            wellness: Self::adaptive_wellness_series(28_800.0, 50.0, 44.0, 28_800.0, 50.0, 45.0),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12000.0,
                "moving_time": 4300,
                "average_heartrate": 150.0,
                "average_watts": 230.0,
                "total_elevation_gain": 120.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_load_ramp_block() -> Self {
        let activities = (1..=28)
            .map(|day| ActivitySummary {
                id: format!("load-{day}"),
                name: Some(format!("Run {day}")),
                start_date_local: format!("2026-03-{day:02}"),
            })
            .collect();

        Self {
            activities,
            events: vec![],
            fitness: json!([{ "fitness": 58.0, "fatigue": 50.0, "form": 8.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12000.0,
                "moving_time": 3600,
                "average_heartrate": 145.0,
                "average_watts": 215.0,
                "total_elevation_gain": 120.0,
                "icu_training_load": 55.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_stream_supported_workout() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "stream-1".to_string(),
                name: Some("Tempo Session".to_string()),
                start_date_local: "2026-03-04".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 55.0, "fatigue": 47.0, "form": 8.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 14000.0,
                "moving_time": 3600,
                "average_heartrate": 145.0,
                "average_watts": 220.0,
                "total_elevation_gain": 80.0
            }),
            intervals: json!([]),
            streams: json!({
                "heartrate": [140.0, 141.0, 142.0, 144.0, 145.0, 146.0],
                "watts": [220.0, 221.0, 222.0, 224.0, 225.0, 226.0]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({
                "zones": {
                    "z1": 600,
                    "z2": 1200,
                    "z3": 300
                }
            }),
        }
    }

    fn with_api_load_snapshot() -> Self {
        let mut client = Self::with_load_ramp_block();
        client.wellness_for_date = json!({"atlLoad": 444.0, "ctlLoad": 333.0});
        client
    }

    fn with_profile_metrics() -> Self {
        Self {
            activities: vec![],
            events: vec![],
            fitness: json!([{
                "fitness": 61.0,
                "fatigue": 47.0,
                "form": 14.0
            }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({}),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([
                {
                    "id": 1783043,
                    "types": ["Run", "VirtualRun", "TrailRun"],
                    "lthr": 171,
                    "max_hr": 180,
                    "hr_zones": [144, 160, 167, 173, 180],
                    "hr_zone_names": ["Recovery", "Endurance", "Tempo", "Threshold", "VO2"],
                    "threshold_pace": 3.7037036,
                    "pace_units": "MINS_KM",
                    "pace_zones": [77.5, 87.7, 94.3, 100.0, 103.4, 111.5, 999.0],
                    "pace_zone_names": ["Zone 1", "Zone 2", "Zone 3", "Zone 4", "Zone 5a", "Zone 5b", "Zone 5c"],
                    "load_order": "HR_PACE_POWER"
                }
            ]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_future_workouts_only() -> Self {
        let first_date = Self::relative_date(1);
        let second_date = Self::relative_date(2);

        Self {
            activities: vec![],
            events: vec![],
            fitness: json!([{ "fitness": 57.0, "fatigue": 43.0, "form": 14.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([
                {
                    "id": 94131802,
                    "category": "WORKOUT",
                    "start_date_local": format!("{first_date}T00:00:00"),
                    "description": "Recovery Run Z1",
                    "moving_time": 2700,
                    "icu_training_load": 30.0,
                    "paired_activity_id": null
                },
                {
                    "id": 94131803,
                    "category": "WORKOUT",
                    "start_date_local": format!("{second_date}T00:00:00"),
                    "description": "Endurance Run Z2 — Pre-Trip",
                    "moving_time": 6300,
                    "icu_training_load": 82.0,
                    "paired_activity_id": null
                }
            ]),
            activity_details: json!({}),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_future_calendar_events_only() -> Self {
        let race_date = Self::relative_date(1);
        let sick_date = Self::relative_date(2);

        Self {
            activities: vec![],
            events: vec![],
            fitness: json!([]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([
                {
                    "id": 99131991,
                    "category": "RACE_A",
                    "start_date_local": format!("{race_date}T00:00:00"),
                    "description": "Race day",
                    "name": "City Marathon",
                    "type": "Race",
                    "moving_time": 14400,
                    "paired_activity_id": null
                },
                {
                    "id": 99131992,
                    "category": "SICK",
                    "start_date_local": format!("{sick_date}T00:00:00"),
                    "description": "Out sick, rest only",
                    "name": "Sick day",
                    "type": null,
                    "moving_time": 0,
                    "paired_activity_id": null
                }
            ]),
            activity_details: json!({}),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_paired_activity_and_calendar_duplicate() -> Self {
        let planned_date = Self::relative_date(0);

        Self {
            activities: vec![ActivitySummary {
                id: "i130349092".to_string(),
                name: Some("Completed Endurance Run".to_string()),
                start_date_local: format!("{planned_date}T07:00:00"),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 57.0, "fatigue": 43.0, "form": 14.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([
                {
                    "id": 94131804,
                    "category": "WORKOUT",
                    "start_date_local": format!("{planned_date}T00:00:00"),
                    "description": "Endurance Run Z2 — Key Workout",
                    "moving_time": 6300,
                    "icu_training_load": 82.0,
                    "paired_activity_id": "i130349092"
                }
            ]),
            activity_details: json!({
                "distance": 18000.0,
                "moving_time": 6300,
                "icu_training_load": 82.0,
                "total_elevation_gain": 220.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_profile_metrics_and_wellness_weight() -> Self {
        let mut client = Self::with_profile_metrics();
        client.wellness_for_date = json!({
            "weight": 86.0,
            "restingHR": 54,
            "ctl": 40.13324,
            "atl": 40.22276
        });
        client
    }

    fn with_recent_non_race_then_race_activity() -> Self {
        Self {
            activities: vec![
                ActivitySummary {
                    id: "activity-regular-1".to_string(),
                    name: Some("Easy Run".to_string()),
                    start_date_local: "2026-03-06".to_string(),
                },
                ActivitySummary {
                    id: "race-2".to_string(),
                    name: Some("City Marathon Race".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                },
            ],
            events: vec![Event {
                id: Some("planned-race-2".to_string()),
                start_date_local: "2026-03-01".to_string(),
                name: "City Marathon Race Plan".to_string(),
                category: intervals_icu_client::EventCategory::RaceA,
                description: Some("Goal marathon plan".to_string()),
                r#type: Some("Race".to_string()),
            }],
            fitness: json!([{ "fitness": 45.0, "fatigue": 60.0, "form": -8.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 42195.0,
                "moving_time": 13200,
                "average_heartrate": 149.0,
                "total_elevation_gain": 120.0
            }),
            intervals: json!([
                {"moving_time": 1800, "average_heartrate": 145.0, "average_watts": 210.0},
                {"moving_time": 1800, "average_heartrate": 152.0, "average_watts": 205.0}
            ]),
            streams: json!({
                "velocity_smooth": [3.1, 3.0, 2.9, 2.8],
                "heartrate": [145.0, 148.0, 151.0, 154.0],
                "watts": [220.0, 218.0, 210.0, 205.0]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_mixed_period_workouts() -> Self {
        Self {
            activities: vec![
                ActivitySummary {
                    id: "tempo-1".to_string(),
                    name: Some("Tempo Builder".to_string()),
                    start_date_local: "2026-03-01".to_string(),
                },
                ActivitySummary {
                    id: "long-1".to_string(),
                    name: Some("Long Run".to_string()),
                    start_date_local: "2026-03-03".to_string(),
                },
                ActivitySummary {
                    id: "tempo-2".to_string(),
                    name: Some("Tempo Cruise Intervals".to_string()),
                    start_date_local: "2026-02-25".to_string(),
                },
            ],
            events: vec![],
            fitness: json!([{ "fitness": 55.0, "fatigue": 45.0, "form": 10.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 15000.0,
                "moving_time": 5400,
                "average_heartrate": 145.0,
                "average_watts": 210.0,
                "total_elevation_gain": 300.0,
                "tss": 77.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_mode_sensitive_single_workout() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "mode-1".to_string(),
                name: Some("Progression Run".to_string()),
                start_date_local: "2026-03-08".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 58.0, "fatigue": 46.0, "form": 12.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 7040.0,
                "moving_time": 2880,
                "average_heartrate": 127.0,
                "average_watts": 188.0,
                "total_elevation_gain": 66.0,
                "decoupling": 2.8,
                "icu_efficiency_factor": 1.74
            }),
            intervals: json!([
                {"moving_time": 600, "average_heartrate": 122.0, "average_watts": 175.0},
                {"moving_time": 600, "average_heartrate": 129.0, "average_watts": 192.0}
            ]),
            streams: json!({
                "heartrate": [120.0, 122.0, 124.0, 126.0, 128.0, 130.0],
                "watts": [170.0, 176.0, 182.0, 188.0, 194.0, 200.0],
                "velocity_smooth": [2.9, 3.0, 3.1, 3.1, 3.0, 2.9]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_mode_collapse_single_workout() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "mode-collapse-1".to_string(),
                name: Some("Uphill intervals".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12240.0,
                "moving_time": 4740,
                "average_heartrate": 145.0,
                "total_elevation_gain": 0.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_object_shaped_interval_payload() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "i126027814".to_string(),
                name: Some("Uphill intervals".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12240.0,
                "moving_time": 4740,
                "average_heartrate": 145.0,
                "total_elevation_gain": 0.0
            }),
            intervals: json!({
                "id": "i126027814",
                "icu_intervals": [
                    {
                        "moving_time": 601,
                        "average_heartrate": 126,
                        "average_watts": null,
                        "type": "WORK"
                    },
                    {
                        "moving_time": 300,
                        "average_heartrate": 142,
                        "average_watts": null,
                        "type": "WORK"
                    },
                    {
                        "moving_time": 360,
                        "average_heartrate": 158,
                        "average_watts": null,
                        "type": "WORK"
                    }
                ],
                "icu_groups": [
                    {
                        "moving_time": 300,
                        "average_heartrate": 138,
                        "count": 6
                    }
                ]
            }),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_interval_power_only_in_streams() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "power-fallback-1".to_string(),
                name: Some("Hill reps".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 6400.0,
                "moving_time": 1800,
                "average_heartrate": 152.0,
                "total_elevation_gain": 120.0,
                "average_cadence": 86.0,
                "icu_training_load": 74.0
            }),
            intervals: json!({
                "id": "power-fallback-1",
                "icu_intervals": [
                    {
                        "start_index": 0,
                        "end_index": 4,
                        "moving_time": 240,
                        "average_heartrate": 150.0,
                        "average_watts": null,
                        "type": "WORK"
                    },
                    {
                        "start_index": 4,
                        "end_index": 8,
                        "moving_time": 240,
                        "average_heartrate": 162.0,
                        "average_watts": null,
                        "type": "WORK"
                    }
                ]
            }),
            streams: json!({
                "watts": [210.0, 220.0, 230.0, 240.0, 280.0, 290.0, 300.0, 310.0],
                "heartrate": [148.0, 149.0, 150.0, 151.0, 158.0, 160.0, 162.0, 164.0]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_noncanonical_stream_payload() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "streams-weird-1".to_string(),
                name: Some("Tempo with sensors".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 10000.0,
                "moving_time": 2700,
                "average_heartrate": 148.0,
                "average_watts": 225.0,
                "average_cadence": 85.0,
                "total_elevation_gain": 40.0,
                "icu_training_load": 63.0
            }),
            intervals: json!([
                {"moving_time": 300, "average_heartrate": 150.0, "average_watts": 240.0}
            ]),
            streams: json!({
                "streams": [
                    {"type": "heartrate", "data": [138.0, 142.0, 147.0, 151.0]},
                    {"type": "watts", "data": [205.0, 218.0, 231.0, 244.0]},
                    {"type": "cadence", "data": [82.0, 84.0, 86.0, 88.0]}
                ]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_rich_detailed_workout() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "detail-rich-1".to_string(),
                name: Some("Steady aerobic run".to_string()),
                start_date_local: "2026-03-08".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 58.0, "fatigue": 46.0, "form": 12.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12000.0,
                "moving_time": 3600,
                "average_heartrate": 141.0,
                "average_watts": 212.0,
                "average_cadence": 84.5,
                "average_speed": 3.3333333,
                "average_temp": 19.4,
                "total_elevation_gain": 95.0,
                "tss": 78.5,
                "icu_training_load": 81.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_interval_power_stream_alias() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "power-alias-1".to_string(),
                name: Some("Threshold reps".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 9000.0,
                "moving_time": 2700,
                "average_heartrate": 151.0,
                "average_speed": 3.2,
                "total_elevation_gain": 70.0
            }),
            intervals: json!({
                "id": "power-alias-1",
                "icu_intervals": [
                    {
                        "start_index": 0,
                        "end_index": 3,
                        "moving_time": 180,
                        "average_heartrate": 148.0,
                        "average_watts": null,
                        "type": "WORK"
                    },
                    {
                        "start_index": 3,
                        "end_index": 6,
                        "moving_time": 180,
                        "average_heartrate": 156.0,
                        "average_watts": null,
                        "type": "WORK"
                    }
                ]
            }),
            streams: json!({
                "power": [250.0, 255.0, 260.0, 300.0, 305.0, 310.0],
                "heartrate": [145.0, 148.0, 151.0, 153.0, 156.0, 159.0],
                "velocity_smooth": [2.9, 3.0, 3.1, 3.2, 3.3, 3.4]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_interval_output_only_in_speed_streams() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "speed-fallback-1".to_string(),
                name: Some("Run intervals from pace stream".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 11000.0,
                "moving_time": 3600,
                "average_heartrate": 148.0,
                "total_elevation_gain": 0.0
            }),
            intervals: json!({
                "id": "speed-fallback-1",
                "icu_intervals": [
                    {
                        "start_index": 0,
                        "end_index": 4,
                        "moving_time": 240,
                        "average_heartrate": 146.0,
                        "average_watts": null,
                        "average_speed": 3.0,
                        "type": "WORK"
                    },
                    {
                        "start_index": 4,
                        "end_index": 8,
                        "moving_time": 240,
                        "average_heartrate": 156.0,
                        "average_watts": null,
                        "average_speed": 3.2,
                        "type": "WORK"
                    }
                ]
            }),
            streams: json!({
                "velocity_smooth": [3.0, 3.0, 3.0, 3.0, 3.2, 3.2, 3.2, 3.2],
                "heartrate": [144.0, 145.0, 146.0, 147.0, 153.0, 155.0, 156.0, 158.0]
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_many_intervals() -> Self {
        let intervals = (0..15)
            .map(|idx| {
                let start = idx * 4;
                json!({
                    "start_index": start,
                    "end_index": start + 4,
                    "moving_time": if idx % 2 == 0 { 300 } else { 360 },
                    "average_heartrate": 140.0 + idx as f64,
                    "average_watts": 220.0 + idx as f64,
                    "type": "WORK"
                })
            })
            .collect::<Vec<_>>();

        Self {
            activities: vec![ActivitySummary {
                id: "many-intervals-1".to_string(),
                name: Some("Big interval session".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 16000.0,
                "moving_time": 5400,
                "average_heartrate": 149.0,
                "total_elevation_gain": 120.0
            }),
            intervals: json!({
                "id": "many-intervals-1",
                "icu_intervals": intervals
            }),
            streams: json!({
                "watts": (0..60).map(|idx| 220.0 + idx as f64).collect::<Vec<_>>(),
                "heartrate": (0..60).map(|idx| 135.0 + idx as f64 * 0.5).collect::<Vec<_>>()
            }),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_priority_streams_without_power() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "priority-streams-1".to_string(),
                name: Some("Uphill intervals".to_string()),
                start_date_local: "2026-02-18".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 54.0, "fatigue": 47.0, "form": 7.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 12240.0,
                "moving_time": 4740,
                "average_heartrate": 145.0,
                "average_speed": 2.5822785,
                "average_cadence": 82.0,
                "total_elevation_gain": 0.0
            }),
            intervals: json!([]),
            streams: json!([
                {"type": "time", "data": [0, 1, 2, 3]},
                {"type": "cadence", "data": [80.0, 81.0, 82.0, 83.0]},
                {"type": "heartrate", "data": [138.0, 142.0, 147.0, 151.0]},
                {"type": "distance", "data": [0.0, 100.0, 200.0, 300.0]},
                {"type": "altitude", "data": [152.2, 152.2, 152.2, 152.2]},
                {"type": "velocity_smooth", "data": [2.50, 2.55, 2.60, 2.68]},
                {"type": "temp", "data": [26.0, 26.2, 26.4, 26.5]},
                {"type": "GroundContactTime", "data": [250.0, 255.0, 260.0, 265.0]},
                {"type": "VerticalOscillation", "data": [70.0, 72.0, 74.0, 76.0]}
            ]),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }

    fn with_best_efforts_and_bucket_histograms() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "payload-1".to_string(),
                name: Some("Structured Long Run".to_string()),
                start_date_local: "2026-03-08".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 60.0, "fatigue": 44.0, "form": 16.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 18000.0,
                "moving_time": 5400,
                "average_heartrate": 138.0,
                "average_watts": 215.0,
                "total_elevation_gain": 110.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!({
                "best_efforts": [
                    {"seconds": 60, "watts": 310.0, "heartrate": 171.0},
                    {"seconds": 300, "watts": 282.0, "heartrate": 165.0}
                ]
            }),
            sport_settings: json!([]),
            hr_histogram: json!([
                {"min": 120, "max": 124, "secs": 469},
                {"min": 125, "max": 129, "secs": 1150}
            ]),
            power_histogram: json!([
                {"min": 200, "max": 224, "secs": 1525},
                {"min": 225, "max": 249, "secs": 1021}
            ]),
            pace_histogram: json!([
                {"min": 2.2593105, "max": 2.354023, "secs": 295},
                {"min": 2.354023, "max": 2.4487357, "secs": 353}
            ]),
        }
    }

    fn with_full_histogram_ranges() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "hist-full-1".to_string(),
                name: Some("Recovery Run Z1".to_string()),
                start_date_local: "2026-03-08".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 58.0, "fatigue": 46.0, "form": 12.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 7040.0,
                "moving_time": 2880,
                "average_heartrate": 127.0,
                "average_watts": 219.0,
                "total_elevation_gain": 66.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!([]),
            sport_settings: json!([]),
            hr_histogram: json!([
                {"min": 80, "max": 84, "secs": 1},
                {"min": 85, "max": 89, "secs": 5},
                {"min": 90, "max": 94, "secs": 8},
                {"min": 95, "max": 99, "secs": 17},
                {"min": 100, "max": 104, "secs": 29},
                {"min": 105, "max": 109, "secs": 40},
                {"min": 110, "max": 114, "secs": 54},
                {"min": 115, "max": 119, "secs": 190},
                {"min": 120, "max": 124, "secs": 469},
                {"min": 125, "max": 129, "secs": 1150},
                {"min": 130, "max": 134, "secs": 720},
                {"min": 135, "max": 139, "secs": 151},
                {"min": 140, "max": 144, "secs": 22},
                {"min": 145, "max": 149, "secs": 9},
                {"min": 150, "max": 154, "secs": 3}
            ]),
            power_histogram: json!([
                {"min": 0, "max": 24, "secs": 71},
                {"min": 25, "max": 49, "secs": 9},
                {"min": 50, "max": 74, "secs": 8},
                {"min": 75, "max": 99, "secs": 7},
                {"min": 100, "max": 124, "secs": 6},
                {"min": 125, "max": 149, "secs": 5},
                {"min": 150, "max": 174, "secs": 4},
                {"min": 175, "max": 199, "secs": 84},
                {"min": 200, "max": 224, "secs": 1525},
                {"min": 225, "max": 249, "secs": 1021},
                {"min": 250, "max": 274, "secs": 91},
                {"min": 275, "max": 299, "secs": 24},
                {"min": 300, "max": 324, "secs": 3},
                {"min": 325, "max": 349, "secs": 1}
            ]),
            pace_histogram: json!([
                {"min": 0.93333334, "max": 1.028046, "secs": 3},
                {"min": 1.028046, "max": 1.1227586, "secs": 12},
                {"min": 1.1227586, "max": 1.2174712, "secs": 18},
                {"min": 1.2174712, "max": 1.3121839, "secs": 22},
                {"min": 1.3121839, "max": 1.4068965, "secs": 27},
                {"min": 1.4068965, "max": 1.5016091, "secs": 31},
                {"min": 1.5016091, "max": 1.5963217, "secs": 36},
                {"min": 1.5963217, "max": 1.6910343, "secs": 41},
                {"min": 1.6910343, "max": 1.785747, "secs": 48},
                {"min": 1.785747, "max": 1.8804595, "secs": 55},
                {"min": 1.8804595, "max": 1.9751722, "secs": 58},
                {"min": 1.9751722, "max": 2.0698848, "secs": 59},
                {"min": 2.0698848, "max": 2.1645975, "secs": 61},
                {"min": 2.1645975, "max": 2.2593105, "secs": 74},
                {"min": 2.2593105, "max": 2.354023, "secs": 295},
                {"min": 2.354023, "max": 2.4487357, "secs": 353},
                {"min": 2.4487357, "max": 2.5434482, "secs": 166},
                {"min": 2.5434482, "max": 2.6381607, "secs": 117},
                {"min": 2.6381607, "max": 2.7328734, "secs": 89},
                {"min": 2.7328734, "max": 2.8275862, "secs": 61},
                {"min": 2.8275862, "max": 2.922299, "secs": 44},
                {"min": 2.922299, "max": 3.0170114, "secs": 29},
                {"min": 3.0170114, "max": 3.1117241, "secs": 17},
                {"min": 3.1117241, "max": 3.2064366, "secs": 8},
                {"min": 3.2064366, "max": 3.3011494, "secs": 4},
                {"min": 3.3011494, "max": 3.395862, "secs": 2},
                {"min": 3.395862, "max": 3.4905746, "secs": 1},
                {"min": 3.4905746, "max": 3.5852873, "secs": 1},
                {"min": 3.5852873, "max": 3.68, "secs": 1},
                {"min": 3.68, "max": 3.77, "secs": 1}
            ]),
        }
    }

    fn with_live_best_efforts_shape() -> Self {
        Self {
            activities: vec![ActivitySummary {
                id: "live-efforts-1".to_string(),
                name: Some("Recovery Run Z1".to_string()),
                start_date_local: "2026-03-08".to_string(),
            }],
            events: vec![],
            fitness: json!([{ "fitness": 58.0, "fatigue": 46.0, "form": 12.0 }]),
            wellness: json!([]),
            wellness_for_date: json!({}),
            upcoming_workouts: json!([]),
            activity_details: json!({
                "distance": 7040.0,
                "moving_time": 2880,
                "average_heartrate": 127.0,
                "average_watts": 219.0,
                "total_elevation_gain": 66.0
            }),
            intervals: json!([]),
            streams: json!({}),
            best_efforts: json!({
                "stream": "watts",
                "efforts": [
                    {"start_index": 2723, "end_index": 2783, "average": 303.51666, "duration": 60, "distance": null},
                    {"start_index": 1320, "end_index": 1380, "average": 241.98334, "duration": 60, "distance": null}
                ]
            }),
            sport_settings: json!([]),
            hr_histogram: json!({}),
            power_histogram: json!({}),
            pace_histogram: json!({}),
        }
    }
}

#[async_trait]
impl IntervalsClient for MockCoachClient {
    async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
        Ok(AthleteProfile {
            id: "athlete-1".into(),
            name: Some("Coach Test".into()),
        })
    }

    async fn get_recent_activities(
        &self,
        _limit: Option<u32>,
        _days_back: Option<i32>,
    ) -> Result<Vec<ActivitySummary>, IntervalsError> {
        Ok(self.activities.clone())
    }

    async fn get_activity_details(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(self.activity_details.clone())
    }

    async fn get_activity_messages(
        &self,
        _activity_id: &str,
    ) -> Result<Vec<ActivityMessage>, IntervalsError> {
        self.activity_details
            .get("__activity_messages")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map(|messages| messages.unwrap_or_default())
            .map_err(|error| {
                IntervalsError::Config(intervals_icu_client::ConfigError::Other(error.to_string()))
            })
    }

    async fn get_activity_intervals(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(self.intervals.clone())
    }

    async fn get_activity_streams(
        &self,
        _activity_id: &str,
        _streams: Option<Vec<String>>,
    ) -> Result<Value, IntervalsError> {
        Ok(self.streams.clone())
    }

    async fn get_best_efforts(
        &self,
        _activity_id: &str,
        _options: Option<BestEffortsOptions>,
    ) -> Result<Value, IntervalsError> {
        Ok(self.best_efforts.clone())
    }

    async fn get_hr_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(self.hr_histogram.clone())
    }

    async fn get_power_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(self.power_histogram.clone())
    }

    async fn get_fitness_summary(&self) -> Result<Value, IntervalsError> {
        Ok(self.fitness.clone())
    }

    async fn get_wellness(&self, days_back: Option<i32>) -> Result<Value, IntervalsError> {
        wellness_days_requests().lock().unwrap().push(days_back);
        Ok(self.wellness.clone())
    }

    async fn create_event(&self, event: Event) -> Result<Event, IntervalsError> {
        Ok(event)
    }
    async fn get_event(&self, event_id: &str) -> Result<Event, IntervalsError> {
        Ok(Self::mock_event(Some(event_id)))
    }
    async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
    async fn get_events(
        &self,
        _days_back: Option<i32>,
        _limit: Option<u32>,
    ) -> Result<Vec<Event>, IntervalsError> {
        Ok(self.events.clone())
    }
    async fn bulk_create_events(&self, _events: Vec<Event>) -> Result<Vec<Event>, IntervalsError> {
        Ok(vec![])
    }
    async fn search_activities(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<Vec<ActivitySummary>, IntervalsError> {
        Ok(vec![])
    }
    async fn search_activities_full(
        &self,
        _query: &str,
        _limit: Option<u32>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn get_activities_csv(&self) -> Result<String, IntervalsError> {
        Ok(String::new())
    }
    async fn update_activity(
        &self,
        _activity_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn download_activity_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }
    async fn download_activity_file_with_progress(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
        _progress_tx: tokio::sync::mpsc::Sender<DownloadProgress>,
        _cancel_rx: tokio::sync::watch::Receiver<bool>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }
    async fn download_fit_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }
    async fn download_gpx_file(
        &self,
        _activity_id: &str,
        _output_path: Option<std::path::PathBuf>,
    ) -> Result<Option<String>, IntervalsError> {
        Ok(None)
    }
    async fn get_gear_list(&self) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn get_sport_settings(&self) -> Result<Value, IntervalsError> {
        Ok(self.sport_settings.clone())
    }
    async fn get_power_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn get_gap_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn delete_activity(&self, _activity_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
    async fn get_activities_around(
        &self,
        _activity_id: &str,
        _limit: Option<u32>,
        _route_id: Option<i64>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn search_intervals(
        &self,
        _min_secs: u32,
        _max_secs: u32,
        _min_intensity: u32,
        _max_intensity: u32,
        _interval_type: Option<String>,
        _min_reps: Option<u32>,
        _max_reps: Option<u32>,
        _limit: Option<u32>,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn get_pace_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
        Ok(self.pace_histogram.clone())
    }
    async fn get_wellness_for_date(&self, _date: &str) -> Result<Value, IntervalsError> {
        Ok(self.wellness_for_date.clone())
    }
    async fn update_wellness(&self, _date: &str, _data: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn get_upcoming_workouts(
        &self,
        _days_ahead: Option<u32>,
        _limit: Option<u32>,
        _category: Option<String>,
    ) -> Result<Value, IntervalsError> {
        Ok(self.upcoming_workouts.clone())
    }
    async fn update_event(
        &self,
        _event_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn bulk_delete_events(&self, _event_ids: Vec<String>) -> Result<(), IntervalsError> {
        Ok(())
    }
    async fn duplicate_event(
        &self,
        _event_id: &str,
        _num_copies: Option<u32>,
        _weeks_between: Option<u32>,
    ) -> Result<Vec<Event>, IntervalsError> {
        Ok(vec![])
    }
    async fn get_hr_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn get_pace_curves(
        &self,
        _days_back: Option<i32>,
        _sport: &str,
    ) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn get_workout_library(&self) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn get_workouts_in_folder(&self, _folder_id: &str) -> Result<Value, IntervalsError> {
        Ok(json!([]))
    }
    async fn create_folder(&self, _folder: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn update_folder(
        &self,
        _folder_id: &str,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn delete_folder(&self, _folder_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
    async fn create_gear(&self, _gear: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn update_gear(&self, _gear_id: &str, _fields: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn delete_gear(&self, _gear_id: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
    async fn create_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn update_gear_reminder(
        &self,
        _gear_id: &str,
        _reminder_id: &str,
        _reset: bool,
        _snooze_days: u32,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn update_sport_settings(
        &self,
        _sport_type: &str,
        _recalc_hr_zones: bool,
        _fields: &Value,
    ) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn apply_sport_settings(&self, _sport_type: &str) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn create_sport_settings(&self, _settings: &Value) -> Result<Value, IntervalsError> {
        Ok(json!({}))
    }
    async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
        Ok(())
    }
}

fn markdown_text(output: &intervals_icu_mcp::intents::IntentOutput) -> String {
    output
        .content
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.clone(),
            ContentBlock::Markdown { markdown } => markdown.clone(),
            ContentBlock::Table { headers, rows } => format!("{:?}{:?}", headers, rows),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test]
async fn assess_recovery_uses_shared_guidance_for_deep_fatigue() {
    let client = Arc::new(MockCoachClient::with_tsb(-25.0));
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    assert!(output.suggestions.iter().any(|s| s.contains("recovery")));
}

#[tokio::test]
async fn analyze_training_period_includes_trend_context() {
    let client = Arc::new(MockCoachClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    assert!(markdown_text(&output).to_lowercase().contains("trend"));
}

#[tokio::test]
async fn analyze_training_single_accepts_today_date_alias() {
    let today = Utc::now().date_naive();
    let client = Arc::new(MockCoachClient {
        activities: vec![ActivitySummary {
            id: "today-training-1".to_string(),
            name: Some("Today's Endurance Run".to_string()),
            start_date_local: format!("{}T07:30:00", today.format("%Y-%m-%d")),
        }],
        events: vec![],
        fitness: json!([{ "fitness": 55.0, "fatigue": 47.0, "form": 8.0 }]),
        wellness: json!([]),
        wellness_for_date: json!({}),
        upcoming_workouts: json!([]),
        activity_details: json!({
            "distance": 18050.0,
            "moving_time": 7742,
            "average_heartrate": 141.0,
            "total_elevation_gain": 233.0
        }),
        intervals: json!([]),
        streams: json!({}),
        best_efforts: json!([]),
        sport_settings: json!([]),
        hr_histogram: json!({}),
        power_histogram: json!({}),
        pace_histogram: json!({}),
    });
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "today"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Today's Endurance Run"));
    assert!(markdown.contains("**Date:** today"));
}

#[tokio::test]
async fn analyze_training_period_surfaces_future_planned_workouts() {
    let client = Arc::new(MockCoachClient::with_future_workouts_only());
    let handler = AnalyzeTrainingHandler::new();
    let period_start = MockCoachClient::relative_date(1);
    let period_end = MockCoachClient::relative_date(2);

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": period_start,
                "period_end": period_end
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Planned Workouts in Window"));
    assert!(markdown.contains("Recovery Run Z1"));
    assert!(markdown.contains("Endurance Run Z2 — Pre-Trip"));
    assert_eq!(output.metadata.total_count, Some(2));
}

#[tokio::test]
async fn analyze_training_period_surfaces_future_calendar_events() {
    let client = Arc::new(MockCoachClient::with_future_calendar_events_only());
    let handler = AnalyzeTrainingHandler::new();
    let period_start = MockCoachClient::relative_date(1);
    let period_end = MockCoachClient::relative_date(3);

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": period_start,
                "period_end": period_end
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Calendar Events in Window"));
    assert!(markdown.contains("City Marathon"));
    assert!(markdown.contains("Sick day"));
    assert!(markdown.contains("RaceA"));
    assert!(markdown.contains("Sick"));
}

#[tokio::test]
async fn analyze_training_period_skips_calendar_duplicates_with_paired_activity_id() {
    let client = Arc::new(MockCoachClient::with_paired_activity_and_calendar_duplicate());
    let handler = AnalyzeTrainingHandler::new();
    let target_date = MockCoachClient::relative_date(0);

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": target_date,
                "period_end": target_date
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(!markdown.contains("Planned Workouts in Window"));
    assert_eq!(output.metadata.total_count, Some(1));
}

#[tokio::test]
async fn analyze_race_adds_post_race_recovery_guidance() {
    let client = Arc::new(MockCoachClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(json!({"description_contains": "50K"}), client, None)
        .await
        .unwrap();

    assert!(
        output
            .next_actions
            .iter()
            .any(|a| a.contains("assess_recovery"))
    );
}

#[tokio::test]
async fn analyze_race_accepts_target_date_alias() {
    let today = Utc::now().date_naive();
    let client = Arc::new(MockCoachClient {
        activities: vec![
            ActivitySummary {
                id: "older-race-1".to_string(),
                name: Some("Mountain 50K".to_string()),
                start_date_local: "2026-02-21T08:23:41".to_string(),
            },
            ActivitySummary {
                id: "today-run-1".to_string(),
                name: Some("Today's Long Run".to_string()),
                start_date_local: format!("{}T08:00:00", today.format("%Y-%m-%d")),
            },
        ],
        events: vec![],
        fitness: json!([{ "fitness": 42.0, "fatigue": 68.0, "form": -18.0 }]),
        wellness: json!([]),
        wellness_for_date: json!({}),
        upcoming_workouts: json!([]),
        activity_details: json!({
            "distance": 18040.0,
            "moving_time": 7742,
            "average_heartrate": 140.0
        }),
        intervals: json!([]),
        streams: json!({}),
        best_efforts: json!([]),
        sport_settings: json!([]),
        hr_histogram: json!({}),
        power_histogram: json!({}),
        pace_histogram: json!({}),
    });
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(json!({"target_date": "today"}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Today's Long Run"));
    assert!(markdown.contains(&format!("{}T08:00:00", today.format("%Y-%m-%d"))));
    assert!(!markdown.contains("Mountain 50K"));
}

#[tokio::test]
async fn compare_periods_includes_shared_trend_context() {
    let client = Arc::new(MockCoachClient::with_period_blocks());
    let handler = ComparePeriodsHandler::new();

    let output = handler
        .execute(
            json!({
                "period_a_start": "2026-03-01",
                "period_a_end": "2026-03-07",
                "period_b_start": "2026-02-24",
                "period_b_end": "2026-02-28"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    assert!(markdown_text(&output).to_lowercase().contains("trend"));
}

#[tokio::test]
async fn analyze_training_single_surfaces_execution_quality_and_degraded_availability() {
    let client = Arc::new(MockCoachClient::with_single_workout_degraded_streams());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("execution context"));
    assert!(markdown.contains("quality findings"));
    assert!(markdown.contains("data availability"));
    assert!(markdown.contains("stream data unavailable"));
    assert!(markdown.contains("fitness summary unavailable"));
}

#[tokio::test]
async fn analyze_race_degraded_mode_reports_missing_supporting_data() {
    let client = Arc::new(MockCoachClient::with_race_degraded_context());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(
            json!({"description_contains": "Spring Marathon"}),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("race analysis"));
    assert!(markdown.contains("data availability"));
    assert!(markdown.contains("wellness data unavailable or empty"));
    assert!(markdown.contains("interval data unavailable"));
    assert!(markdown.contains("stream data unavailable"));
}

#[tokio::test]
async fn assess_recovery_with_fatigue_alert_tsb_minus_15() {
    let client = Arc::new(MockCoachClient {
        activities: vec![ActivitySummary {
            id: "activity-1".to_string(),
            name: Some("Easy Run".to_string()),
            start_date_local: "2026-03-04".to_string(),
        }],
        events: vec![],
        fitness: json!([{ "fitness": 50.0, "fatigue": 65.0, "form": -15.0 }]),
        wellness: json!([]),
        wellness_for_date: json!({}),
        upcoming_workouts: json!([]),
        activity_details: json!({}),
        intervals: json!([]),
        streams: json!({}),
        best_efforts: json!([]),
        sport_settings: json!([]),
        hr_histogram: json!({}),
        power_histogram: json!({}),
        pace_histogram: json!({}),
    });
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("fatigue"));
    assert!(output.suggestions.iter().any(|s| s.contains("recovery")));
}

#[tokio::test]
async fn assess_recovery_with_high_training_load() {
    let client = Arc::new(MockCoachClient {
        activities: vec![ActivitySummary {
            id: "activity-1".to_string(),
            name: Some("Hard Session".to_string()),
            start_date_local: "2026-03-04".to_string(),
        }],
        events: vec![],
        fitness: json!([{ "fitness": 80.0, "fatigue": 60.0, "form": 20.0 }]),
        wellness: json!([
            {"sleepSecs": 28800.0, "restingHR": 48.0, "hrv": 75.0},
            {"sleepSecs": 27000.0, "restingHR": 50.0, "hrv": 70.0}
        ]),
        wellness_for_date: json!({}),
        upcoming_workouts: json!([]),
        activity_details: json!({
            "distance": 15000.0,
            "moving_time": 7200,
            "average_heartrate": 140.0,
            "total_elevation_gain": 300.0
        }),
        intervals: json!([]),
        streams: json!({}),
        best_efforts: json!([]),
        sport_settings: json!([]),
        hr_histogram: json!({}),
        power_histogram: json!({}),
        pace_histogram: json!({}),
    });
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    // High volume should trigger high training load alert
    let markdown = markdown_text(&output).to_lowercase();
    assert!(markdown.contains("data availability"));
}

#[tokio::test]
async fn assess_recovery_shows_recovery_index_and_blocks_ready_language_when_sleep_is_poor() {
    let client = Arc::new(MockCoachClient::with_positive_tsb_and_low_sleep());
    let handler = AssessRecoveryHandler::new();

    let output = handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Recovery Index"));
    assert!(
        !output
            .suggestions
            .iter()
            .any(|s| s.contains("ready for key work"))
    );
    assert!(output.suggestions.iter().any(|s| s.contains("recovery")));
}

#[tokio::test]
async fn assess_recovery_requests_long_enough_wellness_history_for_adaptive_hrv() {
    wellness_days_requests().lock().unwrap().clear();

    let client = Arc::new(MockCoachClient::with_personal_hrv_drop_profile());
    let handler = AssessRecoveryHandler::new();

    handler
        .execute(json!({"period_days": 7}), client, None)
        .await
        .unwrap();

    let requests = wellness_days_requests().lock().unwrap().clone();
    assert!(requests.contains(&Some(35)));
}

#[tokio::test]
async fn assess_recovery_treats_same_absolute_hrv_relative_to_each_athlete_baseline() {
    let high_baseline_client = Arc::new(MockCoachClient::with_personal_hrv_drop_profile());
    let low_baseline_client = Arc::new(MockCoachClient::with_personal_hrv_norm_profile());
    let handler = AssessRecoveryHandler::new();

    let high_baseline_output = handler
        .execute(
            json!({"period_days": 7, "for_activity": "intensity"}),
            high_baseline_client,
            None,
        )
        .await
        .unwrap();
    let low_baseline_output = handler
        .execute(
            json!({"period_days": 7, "for_activity": "intensity"}),
            low_baseline_client,
            None,
        )
        .await
        .unwrap();

    let high_markdown = markdown_text(&high_baseline_output);
    let low_markdown = markdown_text(&low_baseline_output);

    assert!(high_markdown.contains("personal baseline"));
    assert!(high_markdown.contains("Hold intensity"));
    assert!(low_markdown.contains("Within personal range"));
    assert!(low_markdown.contains("Ready for quality"));
}

#[tokio::test]
async fn analyze_training_period_renders_acwr_and_monotony_context() {
    let client = Arc::new(MockCoachClient::with_load_ramp_block());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-28"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("ACWR"));
    assert!(markdown.contains("Monotony"));
}

#[tokio::test]
async fn analyze_training_period_prefers_api_load_snapshot_for_acwr_loads() {
    let client = Arc::new(MockCoachClient::with_api_load_snapshot());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-28"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("acute 444.0, chronic 333.0"));
}

#[tokio::test]
async fn analyze_training_single_renders_execution_metrics_when_streams_exist() {
    let client = Arc::new(MockCoachClient::with_stream_supported_workout());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Efficiency Factor"));
    assert!(markdown.contains("Aerobic Decoupling"));
}

#[tokio::test]
async fn analyze_training_prefers_api_decoupling_over_stream_recalculation() {
    let mut client = MockCoachClient::with_stream_supported_workout();
    client.activity_details = json!({
        "distance": 14000.0,
        "moving_time": 3600,
        "average_heartrate": 145.0,
        "average_watts": 220.0,
        "total_elevation_gain": 80.0,
        "decoupling": 2.7809474
    });
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "streams"
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Aerobic Decoupling: 2.8%"));
    assert!(markdown.to_lowercase().contains("acceptable"));
}

#[tokio::test]
async fn manage_profile_renders_requested_metrics_section_from_fitness_summary() {
    let client = Arc::new(MockCoachClient::with_profile_metrics());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["metrics"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("### Metrics"));
    assert!(markdown.contains("CTL") || markdown.contains("Fitness"));
    assert!(markdown.contains("TSB") || markdown.contains("Form"));
}

#[tokio::test]
async fn manage_profile_supports_real_sport_settings_array_for_zones_and_thresholds() {
    let client = Arc::new(MockCoachClient::with_profile_metrics());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["zones", "thresholds"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Zones (Run)") || markdown.contains("Zones (Running)"));
    assert!(
        markdown.contains("LTHR")
            || markdown.contains("Threshold Pace")
            || markdown.contains("FTP")
    );
}

#[tokio::test]
async fn manage_profile_surfaces_lthr_directly_from_sport_settings() {
    let client = Arc::new(MockCoachClient::with_profile_metrics());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["thresholds"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("LTHR"));
    assert!(markdown.contains("171 bpm"));
}

#[tokio::test]
async fn manage_profile_overview_uses_wellness_weight_when_profile_has_none() {
    let client = Arc::new(MockCoachClient::with_profile_metrics_and_wellness_weight());
    let handler = ManageProfileHandler::new();

    let output = handler
        .execute(
            json!({
                "action": "get",
                "sections": ["overview"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("86.0 kg"));
}

#[tokio::test]
async fn analyze_training_single_renders_pace_histogram_when_requested() {
    let client = Arc::new(MockCoachClient::with_stream_supported_workout());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-04",
                "analysis_type": "detailed",
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Pace Histogram") || markdown.contains("Pace Zone Distribution"));
}

#[tokio::test]
async fn analyze_training_single_surfaces_workout_comments() {
    let mut client = MockCoachClient::with_rich_detailed_workout();
    client.activity_details = json!({
        "distance": 12000.0,
        "moving_time": 3600,
        "average_heartrate": 141.0,
        "average_watts": 212.0,
        "average_cadence": 84.5,
        "average_speed": 3.3333333,
        "average_temp": 19.4,
        "total_elevation_gain": 95.0,
        "tss": 78.5,
        "icu_training_load": 81.0,
        "__activity_messages": [
            {
                "id": 301,
                "athlete_id": "athlete-1",
                "name": "Coach Test",
                "created": "2026-03-08T09:15:00Z",
                "type": "TEXT",
                "content": "Felt smooth until the final 10 minutes.",
                "activity_id": "detail-rich-1"
            },
            {
                "id": 302,
                "athlete_id": "coach-1",
                "name": "Coach Bob",
                "created": "2026-03-08T10:00:00Z",
                "type": "TEXT",
                "content": "Good restraint early, nice finish.",
                "activity_id": "detail-rich-1"
            }
        ]
    });

    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed"
            }),
            Arc::new(client),
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Workout Comments"));
    assert!(markdown.contains("Felt smooth until the final 10 minutes."));
    assert!(markdown.contains("Good restraint early, nice finish."));
    assert!(markdown.contains("Coach Bob"));
}

#[tokio::test]
async fn analyze_training_single_modes_render_distinct_sections() {
    let client = Arc::new(MockCoachClient::with_mode_sensitive_single_workout());
    let handler = AnalyzeTrainingHandler::new();

    let summary = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let detailed = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let intervals = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "intervals"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let streams = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let summary_md = markdown_text(&summary);
    let detailed_md = markdown_text(&detailed);
    let intervals_md = markdown_text(&intervals);
    let streams_md = markdown_text(&streams);

    assert!(!summary_md.contains("Execution Context"));
    assert!(detailed_md.contains("Execution Context"));
    assert!(!detailed_md.contains("Interval Analysis"));
    assert!(intervals_md.contains("Interval Analysis"));
    assert!(streams_md.contains("Stream Insights"));
}

#[tokio::test]
async fn analyze_training_single_modes_do_not_collapse_when_interval_data_is_missing() {
    let client = Arc::new(MockCoachClient::with_mode_collapse_single_workout());
    let handler = AnalyzeTrainingHandler::new();

    let summary = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "summary"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let detailed = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "detailed"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let intervals = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let streams = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let summary_md = markdown_text(&summary);
    let detailed_md = markdown_text(&detailed);
    let intervals_md = markdown_text(&intervals);
    let streams_md = markdown_text(&streams);

    assert!(!summary_md.contains("Quality Findings"));
    assert!(!summary_md.contains("Interval Analysis"));
    assert!(detailed_md.contains("Quality Findings"));
    assert!(!detailed_md.contains("Interval Analysis"));
    assert!(intervals_md.contains("Interval Analysis"));
    assert!(!intervals_md.contains("Quality Findings"));
    assert!(!intervals_md.contains("Execution Context"));
    assert!(streams_md.contains("Stream Insights"));
    assert!(streams_md.contains("Quality Findings"));
    assert!(!streams_md.contains("Interval Analysis"));
}

#[tokio::test]
async fn analyze_training_single_intervals_reads_object_shaped_intervals_payload() {
    let client = Arc::new(MockCoachClient::with_object_shaped_interval_payload());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Interval Analysis"));
    assert!(!markdown.contains("No structured interval data available"));
    assert!(markdown.contains("10:01") || markdown.contains("5:00") || markdown.contains("6:00"));
    assert!(markdown.contains("126 bpm") || markdown.contains("158 bpm"));
}

#[tokio::test]
async fn analyze_training_detailed_adds_expanded_workout_breakdown() {
    let client = Arc::new(MockCoachClient::with_rich_detailed_workout());
    let handler = AnalyzeTrainingHandler::new();

    let summary = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let detailed = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let summary_md = markdown_text(&summary);
    let detailed_md = markdown_text(&detailed);

    assert!(!summary_md.contains("Detailed Breakdown"));
    assert!(detailed_md.contains("Detailed Breakdown"));
    assert!(detailed_md.contains("Cadence") || detailed_md.contains("Training Load"));
}

#[tokio::test]
async fn analyze_training_intervals_backfills_power_from_streams_when_interval_payload_has_null_power()
 {
    let client = Arc::new(MockCoachClient::with_interval_power_only_in_streams());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Interval Analysis"));
    assert!(markdown.contains("225 W"));
    assert!(markdown.contains("295 W"));
    assert!(!markdown.contains("0 W"));
}

#[tokio::test]
async fn analyze_training_streams_mode_renders_streams_from_noncanonical_payload_without_interval_table()
 {
    let client = Arc::new(MockCoachClient::with_noncanonical_stream_payload());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Stream Insights"));
    assert!(markdown.contains("heartrate"));
    assert!(markdown.contains("watts"));
    assert!(!markdown.contains("Interval Analysis"));
    assert!(
        !markdown
            .to_lowercase()
            .contains("stream data requested but unavailable")
    );
}

#[tokio::test]
async fn analyze_training_streams_prioritizes_key_streams_ahead_of_secondary_metrics() {
    let client = Arc::new(MockCoachClient::with_priority_streams_without_power());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Stream Insights"));
    assert!(markdown.contains("heartrate"));
    assert!(markdown.contains("velocity_smooth"));
    assert!(markdown.contains("cadence"));
}

#[tokio::test]
async fn analyze_training_streams_quality_findings_fall_back_to_pace_when_power_is_missing() {
    let client = Arc::new(MockCoachClient::with_priority_streams_without_power());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "streams"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Quality Findings"));
    assert!(markdown.contains("Average pace held at 6:27 /km."));
    assert!(!markdown.contains("Average power tracked"));
}

#[tokio::test]
async fn analyze_training_intervals_backfills_power_from_power_stream_alias() {
    let client = Arc::new(MockCoachClient::with_interval_power_stream_alias());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("255 W"));
    assert!(markdown.contains("305 W"));
    assert!(!markdown.contains("n/a"));
}

#[tokio::test]
async fn analyze_training_intervals_falls_back_to_pace_when_run_streams_have_no_power() {
    let client = Arc::new(MockCoachClient::with_interval_output_only_in_speed_streams());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Avg Pace"));
    assert!(markdown.contains("5:33 /km"));
    assert!(markdown.contains("5:13 /km"));
    assert!(!markdown.contains("n/a"));
}

#[tokio::test]
async fn analyze_training_intervals_renders_all_intervals_without_collapsing_tail() {
    let client = Arc::new(MockCoachClient::with_many_intervals());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-02-18",
                "analysis_type": "intervals"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("\"15\""));
    assert!(!markdown.contains("... and"));
}

#[tokio::test]
async fn analyze_training_single_renders_best_efforts_object_payload_and_bucket_histograms() {
    let client = Arc::new(MockCoachClient::with_best_efforts_and_bucket_histograms());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "detailed",
                "include_best_efforts": true,
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Best Efforts"));
    assert!(markdown.contains("310 W"));
    assert!(markdown.contains("HR Histogram"));
    assert!(markdown.contains("Power Histogram"));
    assert!(markdown.contains("Pace Histogram"));
    assert!(markdown.contains("120-124 bpm"));
    assert!(markdown.contains("200-224 W"));
    assert!(!markdown.contains("0 bpm"));
    assert!(!markdown.contains("\"n/a\""));
}

#[tokio::test]
async fn analyze_training_single_summary_renders_best_efforts_when_requested() {
    let client = Arc::new(MockCoachClient::with_best_efforts_and_bucket_histograms());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary",
                "include_best_efforts": true,
                "include_histograms": false
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Best Efforts"));
    assert!(markdown.contains("310 W"));
    assert!(!markdown.contains("HR Histogram"));
}

#[tokio::test]
async fn analyze_training_single_summary_renders_live_efforts_shape_when_requested() {
    let client = Arc::new(MockCoachClient::with_live_best_efforts_shape());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary",
                "include_best_efforts": true,
                "include_histograms": false
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Best Efforts"));
    assert!(markdown.contains("303.5 W") || markdown.contains("304 W"));
    assert!(markdown.contains("1:00"));
}

#[tokio::test]
async fn analyze_training_single_histograms_include_all_buckets_and_seconds() {
    let client = Arc::new(MockCoachClient::with_full_histogram_ranges());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "single",
                "date": "2026-03-08",
                "analysis_type": "summary",
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(!markdown.contains("more histogram buckets"));
    assert!(markdown.contains("150-154 bpm"));
    assert!(markdown.contains("325-349 W"));
    assert!(markdown.contains("3.68-3.77 m/s"));
    assert!(markdown.contains("0:03"));
    assert!(markdown.contains("0:01"));
}

#[tokio::test]
async fn analyze_training_period_rejects_histograms_flag() {
    let client = Arc::new(MockCoachClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let err = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "include_histograms": true
            }),
            client,
            None,
        )
        .await
        .expect_err("period histograms should be rejected explicitly");

    assert!(err.to_string().contains("include_histograms"));
}

#[tokio::test]
async fn analyze_training_period_reports_requested_tss_when_it_cannot_be_computed() {
    let client = Arc::new(MockCoachClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "metrics": ["tss"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("TSS"));
    assert!(
        markdown.to_lowercase().contains("unavailable")
            || markdown.to_lowercase().contains("unsupported")
    );
}

#[tokio::test]
async fn analyze_race_surfaces_decoupling_warning_when_drift_is_high() {
    let client = Arc::new(MockCoachClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(json!({"description_contains": "50K"}), client, None)
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Aerobic Decoupling"));
    assert!(markdown.to_lowercase().contains("watch"));
}

#[tokio::test]
async fn analyze_training_period_summary_omits_trend_and_load_sections() {
    let client = Arc::new(MockCoachClient::with_period_blocks());
    let handler = AnalyzeTrainingHandler::new();

    let output = handler
        .execute(
            json!({
                "target_type": "period",
                "period_start": "2026-03-01",
                "period_end": "2026-03-07",
                "analysis_type": "summary"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(!markdown.contains("Trend Context"));
    assert!(!markdown.contains("Load Context"));
}

#[tokio::test]
async fn compare_periods_filters_by_workout_type_and_renders_requested_metrics() {
    let client = Arc::new(MockCoachClient::with_mixed_period_workouts());
    let handler = ComparePeriodsHandler::new();

    let output = handler
        .execute(
            json!({
                "period_a_start": "2026-03-01",
                "period_a_end": "2026-03-07",
                "period_b_start": "2026-02-24",
                "period_b_end": "2026-02-28",
                "workout_type": "tempo",
                "metrics": ["pace", "hr", "tss"]
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Requested Metrics"));
    assert!(markdown.contains("TSS"));
    assert!(
        markdown.contains("Activities\", \"1\"")
            || markdown.contains("+0")
            || markdown.contains("Tempo")
    );
}

#[tokio::test]
async fn analyze_race_last_race_selects_race_instead_of_latest_regular_activity() {
    let client = Arc::new(MockCoachClient::with_recent_non_race_then_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(
            json!({
                "date": "last_race",
                "analysis_type": "strategy"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("City Marathon Race"));
    assert!(!markdown.contains("Easy Run"));
    assert!(markdown.contains("Strategy"));
}

#[tokio::test]
async fn analyze_race_compare_to_planned_adds_plan_section_when_enabled() {
    let client = Arc::new(MockCoachClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let output = handler
        .execute(
            json!({
                "description_contains": "50K",
                "compare_to_planned": true,
                "analysis_type": "performance"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let markdown = markdown_text(&output);
    assert!(markdown.contains("Comparison to Plan"));
}

#[tokio::test]
async fn analyze_race_modes_render_distinct_sections() {
    let client = Arc::new(MockCoachClient::with_race_activity());
    let handler = AnalyzeRaceHandler::new();

    let performance = handler
        .execute(
            json!({
                "description_contains": "50K",
                "analysis_type": "performance"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let strategy = handler
        .execute(
            json!({
                "description_contains": "50K",
                "analysis_type": "strategy"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let recovery = handler
        .execute(
            json!({
                "description_contains": "50K",
                "analysis_type": "recovery"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let performance_md = markdown_text(&performance);
    let strategy_md = markdown_text(&strategy);
    let recovery_md = markdown_text(&recovery);

    assert!(performance_md.contains("Performance Review"));
    assert!(!performance_md.contains("Strategy Review"));
    assert!(strategy_md.contains("Strategy Review"));
    assert!(!strategy_md.contains("Performance Review"));
    assert!(recovery_md.contains("Recovery Outlook"));
    assert!(!recovery_md.contains("Strategy Review"));
}

#[tokio::test]
async fn assess_recovery_for_activity_changes_readiness_guidance() {
    let client = Arc::new(MockCoachClient::with_supportive_recovery_metrics());
    let handler = AssessRecoveryHandler::new();

    let easy = handler
        .execute(
            json!({"period_days": 7, "for_activity": "easy"}),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let intensity = handler
        .execute(
            json!({"period_days": 7, "for_activity": "intensity"}),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let race = handler
        .execute(
            json!({"period_days": 7, "for_activity": "race"}),
            client,
            None,
        )
        .await
        .unwrap();

    let easy_md = markdown_text(&easy);
    let intensity_md = markdown_text(&intensity);
    let race_md = markdown_text(&race);
    let easy_md_lower = easy_md.to_lowercase();
    let intensity_md_lower = intensity_md.to_lowercase();
    let race_md_lower = race_md.to_lowercase();

    assert!(easy_md.contains("Activity-Specific Readiness"));
    assert!(easy_md_lower.contains("easy training"));
    assert!(
        intensity_md_lower.contains("quality session") || intensity_md_lower.contains("intensity")
    );
    assert!(race_md_lower.contains("race effort") || race_md_lower.contains("race-ready"));
    assert_ne!(easy_md, intensity_md);
    assert_ne!(intensity_md, race_md);
}

#[tokio::test]
async fn plan_training_focus_modes_do_not_collapse() {
    let client = Arc::new(MockCoachClient::with_profile_metrics());
    let handler = PlanTrainingHandler::new();

    let intensity = handler
        .execute(
            json!({
                "period_start": "2026-03-01",
                "period_end": "2026-03-28",
                "focus": "intensity",
                "idempotency_token": "plan-intensity"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let specific = handler
        .execute(
            json!({
                "period_start": "2026-03-01",
                "period_end": "2026-03-28",
                "focus": "specific",
                "idempotency_token": "plan-specific"
            }),
            client.clone(),
            None,
        )
        .await
        .unwrap();
    let recovery = handler
        .execute(
            json!({
                "period_start": "2026-03-01",
                "period_end": "2026-03-28",
                "focus": "recovery",
                "idempotency_token": "plan-recovery"
            }),
            client,
            None,
        )
        .await
        .unwrap();

    let intensity_md = markdown_text(&intensity);
    let specific_md = markdown_text(&specific);
    let recovery_md = markdown_text(&recovery);

    assert!(intensity_md.contains("Intensity") || intensity_md.contains("threshold"));
    assert!(specific_md.contains("race-specific") || specific_md.contains("Specific"));
    assert!(recovery_md.contains("Recovery") || recovery_md.contains("down week"));
    assert_ne!(intensity_md, specific_md);
    assert_ne!(specific_md, recovery_md);
}
