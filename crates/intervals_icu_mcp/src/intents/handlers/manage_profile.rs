use crate::engines::coach_metrics::parse_fitness_metrics;
use crate::intents::{ContentBlock, IdempotencyCache, IntentError, IntentHandler, IntentOutput};
use async_trait::async_trait;
use chrono::Utc;
use intervals_icu_client::IntervalsClient;
use serde_json::{Value, json};
/// Manage Profile Intent Handler
///
/// Manages athlete profile, zones, and thresholds.
use std::sync::Arc;

pub struct ManageProfileHandler;
impl ManageProfileHandler {
    pub fn new() -> Self {
        Self
    }
}

fn sport_settings_entries(sport_settings: &Value) -> Vec<&serde_json::Map<String, Value>> {
    if let Some(entries) = sport_settings.as_array() {
        return entries.iter().filter_map(Value::as_object).collect();
    }

    let Some(object) = sport_settings.as_object() else {
        return Vec::new();
    };

    if let Some(entries) = object.get("sports").and_then(Value::as_array) {
        entries.iter().filter_map(Value::as_object).collect()
    } else {
        vec![object]
    }
}

fn sport_display_name(setting: &serde_json::Map<String, Value>) -> String {
    setting
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            setting
                .get("types")
                .and_then(Value::as_array)
                .and_then(|types| types.iter().find_map(Value::as_str))
        })
        .unwrap_or("Unknown")
        .to_string()
}

fn primary_sport_setting<'a>(
    settings: &'a [&serde_json::Map<String, Value>],
) -> Option<&'a serde_json::Map<String, Value>> {
    settings
        .iter()
        .copied()
        .find(|setting| {
            setting
                .get("types")
                .and_then(Value::as_array)
                .map(|types| {
                    types.iter().any(|value| {
                        matches!(
                            value.as_str(),
                            Some("Run") | Some("TrailRun") | Some("VirtualRun")
                        )
                    })
                })
                .unwrap_or(false)
        })
        .or_else(|| settings.first().copied())
}

fn get_number(setting: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    keys.iter().find_map(|key| {
        setting
            .get(*key)
            .and_then(|value| value.as_f64().or_else(|| value.as_i64().map(|n| n as f64)))
    })
}

fn format_hr_zone_ranges(zones: &[Value]) -> Vec<String> {
    let bounds = zones
        .iter()
        .filter_map(|value| {
            value
                .as_i64()
                .or_else(|| value.as_f64().map(|n| n.round() as i64))
        })
        .collect::<Vec<_>>();

    bounds
        .iter()
        .enumerate()
        .map(|(index, upper)| {
            if index == 0 {
                format!("≤ {} bpm", upper)
            } else {
                let lower = bounds[index - 1] + 1;
                format!("{}-{} bpm", lower, upper)
            }
        })
        .collect()
}

fn format_threshold_pace(value: f64, units: Option<&str>) -> String {
    match units {
        Some("MINS_KM") => {
            let total_seconds = (value * 60.0).round() as i64;
            format!("{}:{:02} /km", total_seconds / 60, total_seconds % 60)
        }
        Some("SECS_100M") => format!("{:.1} sec/100m", value * 60.0),
        Some(unit) => format!("{:.2} {}", value, unit),
        None => format!("{:.2}", value),
    }
}

#[async_trait]
impl IntentHandler for ManageProfileHandler {
    fn name(&self) -> &'static str {
        "manage_profile"
    }

    fn description(&self) -> &'static str {
        "Manages athlete profile, zones, and thresholds. \
         Use for viewing profile, updating thresholds from test results, \
         and synchronizing zones with lab data."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["get", "update_thresholds"], "description": "Action: view or update"},
                "sections": {"type": "array", "items": {"type": "string"}, "description": "Sections: overview, zones, thresholds, metrics (fitness/load snapshot)"},
                "new_aet_hr": {"type": "number", "description": "New AeT HR (bpm) for update_thresholds"},
                "new_lt_hr": {"type": "number", "description": "New LT HR (bpm) for update_thresholds"},
                "thresholds_source": {"type": "string", "enum": ["manual", "lab_test"], "description": "Threshold source"},
                "apply_to_activities": {"type": "boolean", "default": true, "description": "Apply to historical activities"},
                "idempotency_token": {"type": "string", "description": "Idempotency token (required for update)"}
            },
            "required": ["action"]
        })
    }

    async fn execute(
        &self,
        input: Value,
        client: Arc<dyn IntervalsClient>,
        _cache: Option<&IdempotencyCache>,
    ) -> Result<IntentOutput, IntentError> {
        let action = input
            .get("action")
            .and_then(Value::as_str)
            .ok_or_else(|| IntentError::validation("Missing required field: action"))?;

        match action {
            "get" => self.get_profile(&input, client.as_ref()).await,
            "update_thresholds" => {
                let _token = input
                    .get("idempotency_token")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        IntentError::validation("Missing required field: idempotency_token")
                    })?;
                self.update_thresholds(&input, client.as_ref()).await
            }
            _ => Err(IntentError::validation(format!(
                "Invalid action: {}. Must be 'get' or 'update_thresholds'",
                action
            ))),
        }
    }

    fn requires_idempotency_token(&self) -> bool {
        false
    }
}

impl ManageProfileHandler {
    async fn get_profile(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let sections = input
            .get("sections")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_else(|| vec!["overview".into(), "zones".into(), "thresholds".into()]);

        let profile = client
            .get_athlete_profile()
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch profile: {}", e)))?;

        let sport_settings = client
            .get_sport_settings()
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch sport settings: {}", e)))?;
        let fitness_summary = if sections.contains(&"metrics".to_string()) {
            client.get_fitness_summary().await.ok()
        } else {
            None
        };
        let wellness_for_today = if sections.contains(&"overview".to_string())
            || sections.contains(&"metrics".to_string())
        {
            client
                .get_wellness_for_date(&Utc::now().date_naive().to_string())
                .await
                .ok()
        } else {
            None
        };
        let sport_entries = sport_settings_entries(&sport_settings);

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(format!(
            "# Athlete Profile\nName: {}\nID: {}",
            profile.name.as_deref().unwrap_or("Unknown"),
            profile.id
        )));

        if sections.contains(&"overview".to_string()) {
            let settings_obj = sport_settings.as_object();
            let wellness_obj = wellness_for_today.as_ref().and_then(Value::as_object);
            let age = settings_obj
                .and_then(|o| o.get("age"))
                .and_then(|v| v.as_i64())
                .map(|v| v.to_string())
                .unwrap_or_else(|| "Not set".to_string());
            let weight = settings_obj
                .and_then(|o| o.get("weight"))
                .and_then(|v| v.as_f64())
                .or_else(|| {
                    wellness_obj
                        .and_then(|o| o.get("weight"))
                        .and_then(Value::as_f64)
                })
                .map(|v| format!("{:.1} kg", v))
                .unwrap_or_else(|| "Not set".to_string());

            content.push(ContentBlock::markdown(format!(
                "Overview\n| Parameter | Value |\n|-----------|-------|\n| Age | {} |\n| Weight | {} |",
                age, weight
            )));
        }

        if sections.contains(&"zones".to_string()) {
            for sport in sport_entries.iter().take(3) {
                if let Some(zones) = sport.get("hr_zones").and_then(|z| z.as_array()) {
                    let sport_name = sport_display_name(sport);
                    let rendered_ranges = format_hr_zone_ranges(zones);

                    content.push(ContentBlock::markdown(format!("Zones ({})", sport_name)));

                    let mut hr_rows = vec![vec!["Zone".to_string(), "HR Range".to_string()]];
                    for (index, range) in rendered_ranges.iter().enumerate() {
                        hr_rows.push(vec![format!("Z{}", index + 1), range.clone()]);
                    }
                    content.push(ContentBlock::table(
                        hr_rows[0].clone(),
                        hr_rows[1..].to_vec(),
                    ));
                }
            }
        }

        if sections.contains(&"thresholds".to_string()) {
            let mut threshold_rows = vec![vec!["Parameter".to_string(), "Value".to_string()]];

            if let Some(primary) = primary_sport_setting(&sport_entries) {
                let sport_name = sport_display_name(primary);

                content.push(ContentBlock::markdown(format!(
                    "Thresholds ({})",
                    sport_name
                )));

                if let Some(lthr) = get_number(primary, &["lthr", "threshold_lt_hr"]) {
                    threshold_rows.push(vec!["LTHR".to_string(), format!("{:.0} bpm", lthr)]);
                }
                if let Some(max_hr) = get_number(primary, &["max_hr"]) {
                    threshold_rows.push(vec!["Max HR".to_string(), format!("{:.0} bpm", max_hr)]);
                }
                if let Some(ftp) = get_number(primary, &["ftp"]) {
                    threshold_rows.push(vec!["FTP".to_string(), format!("{:.0} W", ftp)]);
                }
                if let Some(threshold_pace) = get_number(primary, &["threshold_pace"]) {
                    threshold_rows.push(vec![
                        "Threshold Pace".to_string(),
                        format_threshold_pace(
                            threshold_pace,
                            primary.get("pace_units").and_then(Value::as_str),
                        ),
                    ]);
                }
                if let Some(load_order) = primary.get("load_order").and_then(Value::as_str) {
                    threshold_rows.push(vec!["Load Order".to_string(), load_order.to_string()]);
                }
            }

            if threshold_rows.len() > 1 {
                content.push(ContentBlock::table(
                    threshold_rows[0].clone(),
                    threshold_rows[1..].to_vec(),
                ));
            } else {
                content.push(ContentBlock::markdown(
                    "No threshold data available. Update thresholds in Intervals.icu.",
                ));
            }
        }

        if sections.contains(&"metrics".to_string()) {
            content.push(ContentBlock::markdown("Metrics".to_string()));

            if let Some(fitness) = parse_fitness_metrics(fitness_summary.as_ref())
                .or_else(|| parse_fitness_metrics(wellness_for_today.as_ref()))
            {
                let mut metric_rows = vec![vec!["Metric".to_string(), "Value".to_string()]];
                if let Some(ctl) = fitness.ctl {
                    metric_rows.push(vec!["CTL (Fitness)".to_string(), format!("{:.1}", ctl)]);
                }
                if let Some(atl) = fitness.atl {
                    metric_rows.push(vec!["ATL (Fatigue)".to_string(), format!("{:.1}", atl)]);
                }
                if let Some(tsb) = fitness.tsb {
                    metric_rows.push(vec!["TSB (Form)".to_string(), format!("{:+.1}", tsb)]);
                }
                if let Some(load_state) = fitness.load_state {
                    metric_rows.push(vec!["Load State".to_string(), load_state]);
                }

                if metric_rows.len() > 1 {
                    content.push(ContentBlock::table(
                        metric_rows[0].clone(),
                        metric_rows[1..].to_vec(),
                    ));
                } else {
                    content.push(ContentBlock::markdown(
                        "No fitness metrics available for this athlete yet.".to_string(),
                    ));
                }
            } else {
                content.push(ContentBlock::markdown(
                    "No fitness metrics available for this athlete yet.".to_string(),
                ));
            }
        }

        let suggestions =
            vec!["View sport settings in Intervals.icu for detailed zone configuration.".into()];

        let next_actions = vec![
            "To update thresholds: manage_profile action: update_thresholds".into(),
            "To plan based on current zones: plan_training".into(),
        ];

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }

    async fn update_thresholds(
        &self,
        input: &Value,
        client: &dyn IntervalsClient,
    ) -> Result<IntentOutput, IntentError> {
        let new_aet_hr = input
            .get("new_aet_hr")
            .and_then(Value::as_i64)
            .ok_or_else(|| IntentError::validation("Missing required field: new_aet_hr"))?;
        let new_lt_hr = input
            .get("new_lt_hr")
            .and_then(Value::as_i64)
            .ok_or_else(|| IntentError::validation("Missing required field: new_lt_hr"))?;
        let source = input
            .get("thresholds_source")
            .and_then(Value::as_str)
            .unwrap_or("manual");
        let apply = input
            .get("apply_to_activities")
            .and_then(Value::as_bool)
            .unwrap_or(true);

        let sport_settings = client
            .get_sport_settings()
            .await
            .map_err(|e| IntentError::api(format!("Failed to fetch sport settings: {}", e)))?;
        let sport_entries = sport_settings_entries(&sport_settings);
        let primary = primary_sport_setting(&sport_entries)
            .ok_or_else(|| IntentError::api("No sport settings available".to_string()))?;
        let sport_type = primary
            .get("types")
            .and_then(Value::as_array)
            .and_then(|types| types.iter().find_map(Value::as_str))
            .or_else(|| primary.get("type").and_then(Value::as_str))
            .unwrap_or("Run");

        let old_aet = get_number(primary, &["threshold_aet_hr", "aet_hr"])
            .map(|value| value.round() as i64)
            .unwrap_or(150);
        let old_lt = get_number(primary, &["threshold_lt_hr", "lthr"])
            .map(|value| value.round() as i64)
            .unwrap_or(170);

        let update_fields = json!({
            "threshold_aet_hr": new_aet_hr,
            "threshold_lt_hr": new_lt_hr,
            "lthr": new_lt_hr,
            "thresholds_source": source,
        });

        client
            .update_sport_settings(sport_type, true, &update_fields)
            .await
            .map_err(|e| IntentError::api(format!("Failed to update sport settings: {}", e)))?;

        if apply {
            client
                .apply_sport_settings(sport_type)
                .await
                .map_err(|e| IntentError::api(format!("Failed to apply sport settings: {}", e)))?;
        }

        let gap_change = ((new_lt_hr - new_aet_hr) as f64 * 100.0 / new_aet_hr as f64)
            - ((old_lt - old_aet) as f64 * 100.0 / old_aet as f64);

        let mut content = Vec::new();
        content.push(ContentBlock::markdown(format!(
            "# Threshold Update\nSource: {}\nApply to history: {}",
            source,
            if apply { "Yes" } else { "No" }
        )));

        let mut rows = vec![vec![
            "Parameter".into(),
            "Old".into(),
            "New".into(),
            "Δ".into(),
        ]];
        rows.push(vec![
            "AeT HR".into(),
            format!("{} bpm", old_aet),
            format!("{} bpm", new_aet_hr),
            format!("+{} bpm", new_aet_hr - old_aet),
        ]);
        rows.push(vec![
            "LT HR".into(),
            format!("{} bpm", old_lt),
            format!("{} bpm", new_lt_hr),
            format!("+{} bpm", new_lt_hr - old_lt),
        ]);
        rows.push(vec![
            "AeT-LT Gap".into(),
            format!("{:.1}%", (old_lt - old_aet) as f64 * 100.0 / old_aet as f64),
            format!(
                "{:.1}%",
                (new_lt_hr - new_aet_hr) as f64 * 100.0 / new_aet_hr as f64
            ),
            format!("{:+.1}%", gap_change),
        ]);
        content.push(ContentBlock::table(rows[0].clone(), rows[1..].to_vec()));
        content.push(ContentBlock::markdown(format!(
            "Updated sport settings for {} via Intervals.icu API.{}",
            sport_type,
            if apply {
                " Historical activities were queued for recalculation."
            } else {
                " Historical activity recalculation was skipped."
            }
        )));

        let suggestions = vec![
            format!(
                "AeT-LT Gap changed to {:.1}% ({:+.1}%)",
                (new_lt_hr - new_aet_hr) as f64 * 100.0 / new_aet_hr as f64,
                gap_change
            ),
            "Recommended to verify zones after threshold update.".into(),
        ];

        let next_actions = vec![
            "To verify recalculation: assess_recovery in 5-10 minutes".into(),
            "To plan based on new zones: plan_training".into(),
        ];

        Ok(IntentOutput::new(content)
            .with_suggestions(suggestions)
            .with_next_actions(next_actions))
    }
}

impl Default for ManageProfileHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use intervals_icu_client::{AthleteProfile, IntervalsError};
    use std::sync::Arc;

    // ========================================================================
    // Constructor Tests
    // ========================================================================

    #[test]
    fn test_new_handler() {
        let handler = ManageProfileHandler::new();
        assert_eq!(handler.name(), "manage_profile");
    }

    #[test]
    fn test_default_handler() {
        let _handler = ManageProfileHandler;
    }

    // ========================================================================
    // IntentHandler Trait Implementation Tests
    // ========================================================================

    #[test]
    fn test_name() {
        let handler = ManageProfileHandler::new();
        assert_eq!(IntentHandler::name(&handler), "manage_profile");
    }

    #[test]
    fn test_description() {
        let handler = ManageProfileHandler::new();
        let desc = IntentHandler::description(&handler);
        assert!(desc.contains("Manages athlete profile"));
        assert!(desc.contains("zones"));
        assert!(desc.contains("thresholds"));
    }

    #[test]
    fn test_input_schema_structure() {
        let handler = ManageProfileHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        assert!(props.contains_key("action"));
        assert!(props.contains_key("sections"));
        assert!(props.contains_key("new_aet_hr"));
        assert!(props.contains_key("new_lt_hr"));
        assert!(props.contains_key("thresholds_source"));
        assert!(props.contains_key("apply_to_activities"));

        // Check action enum values
        let action = props.get("action").unwrap();
        let action_enum = action.get("enum").unwrap().as_array().unwrap();
        assert!(action_enum.contains(&json!("get")));
        assert!(action_enum.contains(&json!("update_thresholds")));
    }

    #[test]
    fn test_requires_idempotency_token() {
        let handler = ManageProfileHandler::new();
        assert!(!IntentHandler::requires_idempotency_token(&handler));
    }

    // ========================================================================
    // sport_settings_entries() Tests
    // ========================================================================

    #[test]
    fn test_sport_settings_entries_from_array() {
        let settings = json!([
            {"name": "Run", "types": ["Run"]},
            {"name": "Bike", "types": ["Ride"]}
        ]);

        let entries = sport_settings_entries(&settings);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_sport_settings_entries_from_object_with_sports() {
        let settings = json!({
            "sports": [
                {"name": "Run", "types": ["Run"]},
                {"name": "Bike", "types": ["Ride"]}
            ]
        });

        let entries = sport_settings_entries(&settings);
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_sport_settings_entries_from_single_object() {
        let settings = json!({"name": "Run", "types": ["Run"]});

        let entries = sport_settings_entries(&settings);
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_sport_settings_entries_empty_array() {
        let settings = json!([]);

        let entries = sport_settings_entries(&settings);
        assert_eq!(entries.len(), 0);
    }

    #[test]
    fn test_sport_settings_entries_null_value() {
        let settings = json!(null);

        let entries = sport_settings_entries(&settings);
        assert_eq!(entries.len(), 0);
    }

    // ========================================================================
    // sport_display_name() Tests
    // ========================================================================

    #[test]
    fn test_sport_display_name_from_name_field() {
        let setting = serde_json::Map::new();
        let mut setting_with_name = setting.clone();
        setting_with_name.insert("name".to_string(), json!("Running"));

        assert_eq!(sport_display_name(&setting_with_name), "Running");
    }

    #[test]
    fn test_sport_display_name_from_types_array() {
        let mut setting = serde_json::Map::new();
        setting.insert("types".to_string(), json!(["Run", "TrailRun"]));

        assert_eq!(sport_display_name(&setting), "Run");
    }

    #[test]
    fn test_sport_display_name_falls_back_to_unknown() {
        let setting = serde_json::Map::new();

        assert_eq!(sport_display_name(&setting), "Unknown");
    }

    #[test]
    fn test_sport_display_name_priority_name_over_types() {
        let mut setting = serde_json::Map::new();
        setting.insert("name".to_string(), json!("Custom Run"));
        setting.insert("types".to_string(), json!(["Run"]));

        assert_eq!(sport_display_name(&setting), "Custom Run");
    }

    // ========================================================================
    // primary_sport_setting() Tests
    // ========================================================================

    #[test]
    fn test_primary_sport_setting_prefers_run() {
        let run_setting = serde_json::Map::new();
        let mut run_with_types = run_setting.clone();
        run_with_types.insert("types".to_string(), json!(["Run"]));

        let bike_setting = serde_json::Map::new();
        let mut bike_with_types = bike_setting.clone();
        bike_with_types.insert("types".to_string(), json!(["Ride"]));

        let settings = vec![&bike_with_types, &run_with_types];
        let primary = primary_sport_setting(&settings);

        assert!(primary.is_some());
        assert_eq!(
            primary
                .unwrap()
                .get("types")
                .and_then(|v| v.as_array())
                .unwrap()[0]
                .as_str(),
            Some("Run")
        );
    }

    #[test]
    fn test_primary_sport_setting_prefers_trail_run() {
        let trail_setting = serde_json::Map::new();
        let mut trail_with_types = trail_setting.clone();
        trail_with_types.insert("types".to_string(), json!(["TrailRun"]));

        let settings = vec![&trail_with_types];
        let primary = primary_sport_setting(&settings);

        assert!(primary.is_some());
    }

    #[test]
    fn test_primary_sport_setting_prefers_virtual_run() {
        let virtual_setting = serde_json::Map::new();
        let mut virtual_with_types = virtual_setting.clone();
        virtual_with_types.insert("types".to_string(), json!(["VirtualRun"]));

        let settings = vec![&virtual_with_types];
        let primary = primary_sport_setting(&settings);

        assert!(primary.is_some());
    }

    #[test]
    fn test_primary_sport_setting_falls_back_to_first() {
        let swim_setting = serde_json::Map::new();
        let mut swim_with_types = swim_setting.clone();
        swim_with_types.insert("types".to_string(), json!(["Swim"]));

        let settings = vec![&swim_with_types];
        let primary = primary_sport_setting(&settings);

        assert!(primary.is_some());
    }

    #[test]
    fn test_primary_sport_setting_empty_input() {
        let settings: Vec<&serde_json::Map<String, Value>> = vec![];
        let primary = primary_sport_setting(&settings);

        assert!(primary.is_none());
    }

    // ========================================================================
    // get_number() Tests
    // ========================================================================

    #[test]
    fn test_get_number_from_f64() {
        let mut setting = serde_json::Map::new();
        setting.insert("ftp".to_string(), json!(250.5));

        assert_eq!(get_number(&setting, &["ftp"]), Some(250.5));
    }

    #[test]
    fn test_get_number_from_i64() {
        let mut setting = serde_json::Map::new();
        setting.insert("ftp".to_string(), json!(250));

        assert_eq!(get_number(&setting, &["ftp"]), Some(250.0));
    }

    #[test]
    fn test_get_number_tries_multiple_keys() {
        let mut setting = serde_json::Map::new();
        setting.insert("threshold_lt_hr".to_string(), json!(170));

        assert_eq!(
            get_number(&setting, &["lthr", "threshold_lt_hr"]),
            Some(170.0)
        );
    }

    #[test]
    fn test_get_number_returns_none_when_keys_not_found() {
        let setting = serde_json::Map::new();

        assert_eq!(get_number(&setting, &["ftp", "threshold"]), None);
    }

    #[test]
    fn test_get_number_returns_none_for_non_numeric() {
        let mut setting = serde_json::Map::new();
        setting.insert("name".to_string(), json!("Run"));

        assert_eq!(get_number(&setting, &["name"]), None);
    }

    // ========================================================================
    // format_hr_zone_ranges() Tests
    // ========================================================================

    #[test]
    fn test_format_hr_zone_ranges_basic() {
        let zones: Vec<Value> = vec![json!(140), json!(160), json!(180)];

        let ranges = format_hr_zone_ranges(&zones);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], "≤ 140 bpm");
        assert_eq!(ranges[1], "141-160 bpm");
        assert_eq!(ranges[2], "161-180 bpm");
    }

    #[test]
    fn test_format_hr_zone_ranges_empty() {
        let zones: Vec<Value> = vec![];

        let ranges = format_hr_zone_ranges(&zones);
        assert!(ranges.is_empty());
    }

    #[test]
    fn test_format_hr_zone_ranges_with_floats() {
        let zones: Vec<Value> = vec![json!(140.5), json!(160.7), json!(180.2)];

        let ranges = format_hr_zone_ranges(&zones);
        assert_eq!(ranges.len(), 3);
        assert_eq!(ranges[0], "≤ 141 bpm");
        assert_eq!(ranges[1], "142-161 bpm");
        assert_eq!(ranges[2], "162-180 bpm");
    }

    #[test]
    fn test_format_hr_zone_ranges_single_zone() {
        let zones: Vec<Value> = vec![json!(150)];

        let ranges = format_hr_zone_ranges(&zones);
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0], "≤ 150 bpm");
    }

    // ========================================================================
    // format_threshold_pace() Tests
    // ========================================================================

    #[test]
    fn test_format_threshold_pace_mins_km() {
        let pace = 5.0; // 5:00 /km
        assert_eq!(format_threshold_pace(pace, Some("MINS_KM")), "5:00 /km");
    }

    #[test]
    fn test_format_threshold_pace_mins_km_with_seconds() {
        let pace = 5.5; // 5:30 /km
        assert_eq!(format_threshold_pace(pace, Some("MINS_KM")), "5:30 /km");
    }

    #[test]
    fn test_format_threshold_pace_secs_100m() {
        let pace = 3.0; // 3 min/100m = 180 sec/100m
        assert_eq!(
            format_threshold_pace(pace, Some("SECS_100M")),
            "180.0 sec/100m"
        );
    }

    #[test]
    fn test_format_threshold_pace_unknown_unit() {
        let pace = 10.0;
        assert_eq!(
            format_threshold_pace(pace, Some("UNKNOWN")),
            "10.00 UNKNOWN"
        );
    }

    #[test]
    fn test_format_threshold_pace_no_unit() {
        let pace = 5.5;
        assert_eq!(format_threshold_pace(pace, None), "5.50");
    }

    // ========================================================================
    // Input Validation and Default Value Tests
    // ========================================================================

    #[test]
    fn test_action_values() {
        let valid_actions = ["get", "update_thresholds"];
        for action in &valid_actions {
            assert!(["get", "update_thresholds"].contains(action));
        }
    }

    #[test]
    fn test_default_sections() {
        let input = json!({
            "action": "get"
        });

        let sections = input
            .get("sections")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_else(|| vec!["overview", "zones", "thresholds"]);

        assert_eq!(sections.len(), 3);
        assert!(sections.contains(&"overview"));
        assert!(sections.contains(&"zones"));
        assert!(sections.contains(&"thresholds"));
    }

    #[test]
    fn test_threshold_source_values() {
        let valid_sources = ["manual", "lab_test"];
        for source in &valid_sources {
            assert!(["manual", "lab_test"].contains(source));
        }
    }

    #[test]
    fn test_apply_to_activities_default() {
        let input = json!({
            "action": "update_thresholds",
            "new_aet_hr": 155,
            "new_lt_hr": 171
        });

        let apply = input
            .get("apply_to_activities")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        assert!(apply);
    }

    #[test]
    fn test_gap_change_calculation() {
        let old_aet: i64 = 150;
        let old_lt: i64 = 170;
        let new_aet: i64 = 155;
        let new_lt: i64 = 171;

        let old_gap = (old_lt - old_aet) as f64 * 100.0 / old_aet as f64;
        let new_gap = (new_lt - new_aet) as f64 * 100.0 / new_aet as f64;
        let gap_change = new_gap - old_gap;

        assert!((old_gap - 13.33).abs() < 0.1);
        assert!((new_gap - 10.32).abs() < 0.1);
        assert!(gap_change < 0.0);
    }

    #[test]
    fn test_threshold_update_schema() {
        let handler = ManageProfileHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();

        let aet_hr = props.get("new_aet_hr").unwrap();
        assert_eq!(aet_hr.get("type").and_then(|v| v.as_str()), Some("number"));

        let lt_hr = props.get("new_lt_hr").unwrap();
        assert_eq!(lt_hr.get("type").and_then(|v| v.as_str()), Some("number"));
    }

    #[test]
    fn test_sections_field_type() {
        let handler = ManageProfileHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        let sections = props.get("sections").unwrap();

        assert_eq!(sections.get("type").and_then(|v| v.as_str()), Some("array"));
    }

    #[test]
    fn test_apply_to_activities_field_default() {
        let handler = ManageProfileHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();
        let apply = props.get("apply_to_activities").unwrap();

        assert_eq!(apply.get("default").and_then(|v| v.as_bool()), Some(true));
    }

    // ========================================================================
    // Mock Client for Integration Tests
    // ========================================================================

    struct MockClient {
        profile: AthleteProfile,
        sport_settings: Value,
        fitness_summary: Option<Value>,
        wellness: Option<Value>,
    }

    impl Default for MockClient {
        fn default() -> Self {
            Self {
                profile: AthleteProfile {
                    id: "ath1".to_string(),
                    name: Some("Test Athlete".to_string()),
                },
                sport_settings: json!([{
                    "name": "Run",
                    "types": ["Run"],
                    "hr_zones": [140, 160, 180],
                    "lthr": 170,
                    "max_hr": 190,
                    "ftp": 250,
                    "threshold_pace": 5.0,
                    "pace_units": "MINS_KM",
                    "load_order": "heart_rate"
                }]),
                fitness_summary: Some(json!({
                    "ctl": 45.0,
                    "atl": 30.0,
                    "tsb": 15.0
                })),
                wellness: Some(json!({
                    "weight": 75.5
                })),
            }
        }
    }

    #[async_trait]
    impl IntervalsClient for MockClient {
        async fn get_athlete_profile(&self) -> Result<AthleteProfile, IntervalsError> {
            Ok(self.profile.clone())
        }

        async fn get_sport_settings(&self) -> Result<Value, IntervalsError> {
            Ok(self.sport_settings.clone())
        }

        async fn get_fitness_summary(&self) -> Result<Value, IntervalsError> {
            self.fitness_summary
                .clone()
                .ok_or_else(|| IntervalsError::NotFound("No fitness summary".to_string()))
        }

        async fn get_wellness_for_date(&self, _date: &str) -> Result<Value, IntervalsError> {
            self.wellness
                .clone()
                .ok_or_else(|| IntervalsError::NotFound("No wellness data".to_string()))
        }

        async fn update_sport_settings(
            &self,
            _sport_type: &str,
            _recalc_hr_zones: bool,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({"updated": true}))
        }

        async fn apply_sport_settings(&self, _sport_type: &str) -> Result<Value, IntervalsError> {
            Ok(json!({"applied": true}))
        }

        // Stub implementations for other required methods
        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
            Ok(vec![])
        }

        async fn create_event(
            &self,
            event: intervals_icu_client::Event,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            Ok(event)
        }

        async fn get_event(
            &self,
            event_id: &str,
        ) -> Result<intervals_icu_client::Event, IntervalsError> {
            Ok(intervals_icu_client::Event {
                id: Some(event_id.to_string()),
                start_date_local: "2026-03-04".to_string(),
                name: "Mock event".to_string(),
                category: intervals_icu_client::EventCategory::Workout,
                description: None,
                r#type: None,
            })
        }

        async fn delete_event(&self, _event_id: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_events(
            &self,
            _days_back: Option<i32>,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn bulk_create_events(
            &self,
            _events: Vec<intervals_icu_client::Event>,
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
            Ok(vec![])
        }

        async fn get_activity_streams(
            &self,
            _activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_best_efforts(
            &self,
            _activity_id: &str,
            _options: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_activity_details(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn search_activities(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, IntervalsError> {
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
            Ok("id,name\n1,Run".to_string())
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
            _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
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

        async fn get_power_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_hr_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_pace_histogram(&self, _activity_id: &str) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_wellness(&self, _days_back: Option<i32>) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn update_wellness(
            &self,
            _date: &str,
            _data: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_upcoming_workouts(
            &self,
            _days_ahead: Option<u32>,
            _limit: Option<u32>,
            _category: Option<String>,
        ) -> Result<Value, IntervalsError> {
            Ok(json!([]))
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
        ) -> Result<Vec<intervals_icu_client::Event>, IntervalsError> {
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

        async fn update_gear(
            &self,
            _gear_id: &str,
            _fields: &Value,
        ) -> Result<Value, IntervalsError> {
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

        async fn create_sport_settings(&self, _settings: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn delete_sport_settings(&self, _sport_type: &str) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn update_wellness_bulk(&self, _entries: &[Value]) -> Result<(), IntervalsError> {
            Ok(())
        }

        async fn get_weather_config(&self) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_weather_config(&self, _config: &Value) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn list_routes(&self) -> Result<Value, IntervalsError> {
            Ok(json!([]))
        }

        async fn get_route(
            &self,
            _route_id: i64,
            _include_path: bool,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn update_route(
            &self,
            _route_id: i64,
            _route: &Value,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }

        async fn get_route_similarity(
            &self,
            _route_id: i64,
            _other_id: i64,
        ) -> Result<Value, IntervalsError> {
            Ok(json!({}))
        }
    }

    // ========================================================================
    // Handler Execution Tests
    // ========================================================================

    #[tokio::test]
    async fn test_execute_get_profile_action() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "get"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn test_execute_get_profile_with_all_sections() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "get",
            "sections": ["overview", "zones", "thresholds", "metrics"]
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn test_execute_get_profile_with_specific_sections() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "get",
            "sections": ["thresholds"]
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.content.is_empty());
    }

    #[tokio::test]
    async fn test_execute_update_thresholds_action() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "update_thresholds",
            "new_aet_hr": 155,
            "new_lt_hr": 171,
            "thresholds_source": "lab_test",
            "apply_to_activities": true,
            "idempotency_token": "test-token-123"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.content.is_empty());

        // Check that suggestions are present
        assert!(!output.suggestions.is_empty());
    }

    #[tokio::test]
    async fn test_execute_update_thresholds_without_apply() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "update_thresholds",
            "new_aet_hr": 150,
            "new_lt_hr": 170,
            "apply_to_activities": false,
            "idempotency_token": "test-token-456"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        let content_text = format!("{:?}", output.content);
        assert!(content_text.contains("Historical activity recalculation was skipped"));
    }

    #[tokio::test]
    async fn test_execute_invalid_action() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "invalid_action"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_missing_action() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({});

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_update_thresholds_missing_token() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "update_thresholds",
            "new_aet_hr": 155,
            "new_lt_hr": 171
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_update_thresholds_missing_new_aet_hr() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "update_thresholds",
            "new_lt_hr": 171,
            "idempotency_token": "test-token"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_update_thresholds_missing_new_lt_hr() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "update_thresholds",
            "new_aet_hr": 155,
            "idempotency_token": "test-token"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            IntentError::ValidationError(_)
        ));
    }

    #[tokio::test]
    async fn test_execute_update_thresholds_default_source() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "update_thresholds",
            "new_aet_hr": 155,
            "new_lt_hr": 171,
            "idempotency_token": "test-token"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        let content_text = format!("{:?}", output.content);
        assert!(content_text.contains("manual")); // Default source
    }

    #[tokio::test]
    async fn test_execute_get_profile_suggestions() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "get"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.suggestions.is_empty());
        assert!(!output.next_actions.is_empty());
    }

    #[tokio::test]
    async fn test_execute_update_thresholds_next_actions() {
        let handler = ManageProfileHandler::new();
        let client = Arc::new(MockClient::default());
        let input = json!({
            "action": "update_thresholds",
            "new_aet_hr": 155,
            "new_lt_hr": 171,
            "idempotency_token": "test-token"
        });

        let result = handler.execute(input, client, None).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert!(!output.next_actions.is_empty());
        assert!(
            output
                .next_actions
                .iter()
                .any(|a| a.contains("assess_recovery"))
        );
    }
}
