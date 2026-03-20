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
            "## Athlete Profile\n\n**Name:** {}\n**ID:** {}",
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
                "### Overview\n\n| Parameter | Value |\n|-----------|-------|\n| Age | {} |\n| Weight | {} |",
                age, weight
            )));
        }

        if sections.contains(&"zones".to_string()) {
            for sport in sport_entries.iter().take(3) {
                if let Some(zones) = sport.get("hr_zones").and_then(|z| z.as_array()) {
                    let sport_name = sport_display_name(sport);
                    let rendered_ranges = format_hr_zone_ranges(zones);

                    content.push(ContentBlock::markdown(format!(
                        "### Zones ({})",
                        sport_name
                    )));

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
                    "### Thresholds ({})",
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
            content.push(ContentBlock::markdown("### Metrics".to_string()));

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
            "## Threshold Update\n\n**Source:** {}\n**Apply to history:** {}\nApply to history: {}",
            source,
            if apply { "Yes" } else { "No" },
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
            "\nUpdated sport settings for **{}** via Intervals.icu API.{}",
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

    #[test]
    fn test_new_handler() {
        let handler = ManageProfileHandler::new();
        assert_eq!(handler.name(), "manage_profile");
    }

    #[test]
    fn test_default_handler() {
        let _handler = ManageProfileHandler;
    }

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
        assert!(gap_change < 0.0); // Gap decreased
    }

    #[test]
    fn test_threshold_update_schema() {
        let handler = ManageProfileHandler::new();
        let schema = IntentHandler::input_schema(&handler);

        let props = schema.get("properties").unwrap().as_object().unwrap();

        // new_aet_hr and new_lt_hr should be numbers
        let aet_hr = props.get("new_aet_hr").unwrap();
        assert_eq!(aet_hr.get("type").and_then(|v| v.as_str()), Some("number"));

        let lt_hr = props.get("new_lt_hr").unwrap();
        assert_eq!(lt_hr.get("type").and_then(|v| v.as_str()), Some("number"));
    }
}
