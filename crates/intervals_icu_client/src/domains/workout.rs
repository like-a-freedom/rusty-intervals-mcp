//! Domain types for workout library and sport settings responses.
//!
//! These types model the shapes returned by the Intervals.icu API for
//! workout folder/library endpoints and sport settings, replacing
//! `serde_json::Value` with compile-time-safe structures.

use serde::{Deserialize, Deserializer, Serialize};

fn deserialize_opt_i64<'de, D>(deserializer: D) -> std::result::Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Option<serde_json::Value> = Option::deserialize(deserializer)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => Ok(n.as_i64()),
        Some(serde_json::Value::String(s)) => Ok(s.parse::<i64>().ok()),
        _ => Ok(None),
    }
}

/// A workout item (folder entry, plan, or individual workout) from the library.
///
/// The `/athlete/{id}/folders` endpoint returns a flat array of these items.
/// Items can represent folders, plans, or individual workouts depending on `type`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkoutItem {
    pub id: i64,
    pub name: String,
    /// Item type: `"folder"`, `"plan"`, `"workout"`, etc.
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, deserialize_with = "deserialize_opt_i64")]
    pub folder_id: Option<i64>,
    #[serde(default)]
    pub sport_type: Option<String>,
    #[serde(default)]
    pub start_date_local: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub distance_meters: Option<f64>,
}

/// A training plan or folder from the workout library.
///
/// The API returns folders and plans as items in the library response.
/// Plans may contain nested workout references.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Folder {
    pub id: i64,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub parent_id: Option<i64>,
    #[serde(default)]
    pub children: Vec<Folder>,
}

/// Top-level sport settings response.
///
/// The API returns either:
/// - An array of sport setting objects, or
/// - An object with a `"sports"` key containing the array
///
/// Use [`SportSettings::from_value`] to handle both shapes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SportSettings {
    /// Per-sport configuration entries.
    pub sports: Vec<SportSetting>,
    /// Athlete age (top-level field when response is an object).
    #[serde(default)]
    pub age: Option<u32>,
    /// Athlete weight in kg (top-level field when response is an object).
    #[serde(default)]
    pub weight: Option<f64>,
}

impl SportSettings {
    /// Parse sport settings from a `serde_json::Value`, handling both
    /// the array shape and the object-with-`"sports"` shape.
    pub fn from_value(value: &serde_json::Value) -> Option<Self> {
        // Object with "sports" key
        if let Some(obj) = value.as_object() {
            let age = obj.get("age").and_then(|v| v.as_u64()).map(|v| v as u32);
            let weight = obj.get("weight").and_then(|v| v.as_f64());
            let sports = obj
                .get("sports")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default();
            return Some(Self {
                sports,
                age,
                weight,
            });
        }
        // Top-level array
        if let Some(arr) = value.as_array() {
            let sports: Vec<SportSetting> = arr
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect();
            return Some(Self {
                sports,
                age: None,
                weight: None,
            });
        }
        None
    }
}

/// Configuration for a single sport type.
///
/// Fields are optional because not every sport has all thresholds,
/// zones, or units configured.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SportSetting {
    /// Sport ID (numeric or string).
    #[serde(default)]
    pub id: Option<i64>,
    /// Sport display name (e.g., `"Cycling"`, `"Running"`).
    #[serde(default)]
    pub name: Option<String>,
    /// Sport type identifiers (e.g., `["Ride"]`, `["Run", "TrailRun"]`).
    #[serde(default)]
    pub types: Option<Vec<String>>,
    /// Primary sport type string.
    #[serde(default, rename = "type")]
    pub sport_type: Option<String>,
    /// Functional Threshold Power in watts.
    #[serde(default)]
    pub ftp: Option<f64>,
    /// Lactate Heart Rate Threshold in bpm.
    #[serde(default)]
    pub lthr: Option<f64>,
    /// Aerobic Threshold HR in bpm.
    #[serde(default)]
    pub threshold_aet_hr: Option<f64>,
    /// Lactate Threshold HR in bpm.
    #[serde(default)]
    pub threshold_lt_hr: Option<f64>,
    /// Maximum heart rate in bpm.
    #[serde(default)]
    pub max_hr: Option<f64>,
    /// Threshold pace (minutes per unit).
    #[serde(default)]
    pub threshold_pace: Option<f64>,
    /// Pace units: `"MINS_KM"`, `"SECS_100M"`, etc.
    #[serde(default)]
    pub pace_units: Option<String>,
    /// Load order for periodization.
    #[serde(default)]
    pub load_order: Option<String>,
    /// Heart rate zones as upper-bound values.
    #[serde(default)]
    pub hr_zones: Vec<serde_json::Value>,
    /// Power zones.
    #[serde(default)]
    pub power_zones: Vec<serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deserialize_workout_item_minimal() {
        let item: WorkoutItem = serde_json::from_value(json!({"id": 1, "name": "Test"}))
            .expect("deserialize minimal workout item");
        assert_eq!(item.id, 1);
        assert_eq!(item.name, "Test");
        assert!(item.r#type.is_none());
        assert!(item.folder_id.is_none());
    }

    #[test]
    fn deserialize_workout_item_full() {
        let item: WorkoutItem = serde_json::from_value(json!({
            "id": 42,
            "name": "Morning Run",
            "type": "workout",
            "description": "Easy 5k",
            "folder_id": 10,
            "sport_type": "Run",
            "start_date_local": "2025-06-15",
            "duration_seconds": 1800.0,
            "distance_meters": 5000.0
        }))
        .expect("deserialize full workout item");
        assert_eq!(item.id, 42);
        assert_eq!(item.r#type.as_deref(), Some("workout"));
        assert_eq!(item.folder_id, Some(10));
        assert_eq!(item.duration_seconds, Some(1800.0));
    }

    #[test]
    fn deserialize_folder_minimal() {
        let folder: Folder = serde_json::from_value(json!({"id": 1, "name": "Plans"}))
            .expect("deserialize minimal folder");
        assert_eq!(folder.id, 1);
        assert_eq!(folder.name, "Plans");
        assert!(folder.children.is_empty());
    }

    #[test]
    fn deserialize_folder_nested() {
        let folder: Folder = serde_json::from_value(json!({
            "id": 1,
            "name": "Plans",
            "description": "My plans",
            "parent_id": null,
            "children": [
                {"id": 2, "name": "Sub", "children": []}
            ]
        }))
        .expect("deserialize nested folder");
        assert_eq!(folder.children.len(), 1);
        assert_eq!(folder.children[0].id, 2);
    }

    #[test]
    fn deserialize_sport_setting_minimal() {
        let setting: SportSetting =
            serde_json::from_value(json!({"name": "Cycling"})).expect("deserialize sport setting");
        assert_eq!(setting.name.as_deref(), Some("Cycling"));
        assert!(setting.ftp.is_none());
        assert!(setting.hr_zones.is_empty());
    }

    #[test]
    fn deserialize_sport_setting_full() {
        let setting: SportSetting = serde_json::from_value(json!({
            "name": "Cycling",
            "types": ["Ride"],
            "type": "Ride",
            "ftp": 250.0,
            "lthr": 165.0,
            "threshold_aet_hr": 150.0,
            "threshold_lt_hr": 165.0,
            "max_hr": 190.0,
            "threshold_pace": 4.5,
            "pace_units": "MINS_KM",
            "load_order": "primary",
            "hr_zones": [120, 140, 155, 170, 185],
            "power_zones": [{"min": 0, "max": 100}]
        }))
        .expect("deserialize full sport setting");
        assert_eq!(setting.ftp, Some(250.0));
        assert_eq!(setting.hr_zones.len(), 5);
    }

    #[test]
    fn sport_settings_from_array() {
        let value = json!([
            {"name": "Cycling", "ftp": 250},
            {"name": "Running", "lthr": 165}
        ]);
        let settings = SportSettings::from_value(&value).expect("parse from array");
        assert_eq!(settings.sports.len(), 2);
        assert!(settings.age.is_none());
    }

    #[test]
    fn sport_settings_from_object_with_sports() {
        let value = json!({
            "age": 30,
            "weight": 75.5,
            "sports": [
                {"name": "Cycling", "ftp": 250}
            ]
        });
        let settings = SportSettings::from_value(&value).expect("parse from object");
        assert_eq!(settings.age, Some(30));
        assert!((settings.weight.unwrap() - 75.5).abs() < 0.01);
        assert_eq!(settings.sports.len(), 1);
    }

    #[test]
    fn sport_settings_from_empty_array() {
        let value = json!([]);
        let settings = SportSettings::from_value(&value).expect("parse empty array");
        assert!(settings.sports.is_empty());
    }

    #[test]
    fn sport_settings_from_null_returns_none() {
        let value = json!(null);
        assert!(SportSettings::from_value(&value).is_none());
    }

    #[test]
    fn sport_settings_roundtrip() {
        let original = SportSettings {
            sports: vec![SportSetting {
                id: Some(42),
                name: Some("Run".into()),
                types: Some(vec!["Run".into()]),
                sport_type: None,
                ftp: None,
                lthr: Some(170.0),
                threshold_aet_hr: None,
                threshold_lt_hr: Some(170.0),
                max_hr: Some(195.0),
                threshold_pace: None,
                pace_units: None,
                load_order: None,
                hr_zones: vec![json!(120), json!(145), json!(160), json!(175), json!(190)],
                power_zones: vec![],
            }],
            age: Some(28),
            weight: Some(70.0),
        };
        let serialized = serde_json::to_value(&original).expect("serialize");
        let deserialized: SportSettings =
            serde_json::from_value(serialized).expect("deserialize roundtrip");
        assert_eq!(original, deserialized);
    }
}
