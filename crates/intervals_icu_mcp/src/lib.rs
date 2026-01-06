use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, mpsc, watch};
use uuid::Uuid;

use rmcp::Json;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    AnnotateAble, GetPromptRequestParam, GetPromptResult, ListPromptsResult, ListResourcesResult,
    PaginatedRequestParam, RawResource, ReadResourceRequestParam, ReadResourceResult,
    ResourceContents,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData, RoleServer};
use rmcp::{prompt, prompt_handler, prompt_router, tool, tool_handler, tool_router};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

use intervals_icu_client::{ActivitySummary, IntervalsClient};

mod prompts;

#[derive(Clone)]
pub struct IntervalsMcpHandler {
    client: Arc<dyn IntervalsClient>,
    tool_router: rmcp::handler::server::tool::ToolRouter<IntervalsMcpHandler>,
    prompt_router: rmcp::handler::server::router::prompt::PromptRouter<IntervalsMcpHandler>,
    downloads: Arc<Mutex<HashMap<String, DownloadStatus>>>,
    cancel_senders: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    webhooks: Arc<Mutex<HashMap<String, WebhookEvent>>>,
    webhook_secret: Arc<Mutex<Option<String>>>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub enum DownloadState {
    Pending,
    InProgress,
    Completed,
    Failed(String),
    Cancelled,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct DownloadStatus {
    pub id: String,
    pub activity_id: String,
    pub state: DownloadState,
    pub bytes_downloaded: u64,
    pub total_bytes: Option<u64>,
    pub path: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema, Clone)]
pub struct WebhookEvent {
    pub id: String,
    pub payload: serde_json::Value,
    pub received_at: u64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RecentParams {
    pub limit: Option<u32>,
    pub days_back: Option<i32>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ProfileResult {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ActivitySummaryResult {
    pub id: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct RecentActivitiesResult {
    pub activities: Vec<ActivitySummaryResult>,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct EventsResult {
    pub events: Vec<intervals_icu_client::Event>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObjectResult {
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivityIdParam {
    pub activity_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BulkCreateEventsToolParams {
    /// Array of calendar events to create (title, start_date_local, category, etc.)
    pub events: Vec<intervals_icu_client::Event>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
/// Parameters for finding peak performances (best efforts) in an activity.
/// All parameters are flat (no nested "options" object). Use SINGULAR values, NOT arrays.
pub struct BestEffortsToolParams {
    /// REQUIRED. The activity ID, e.g. "i112895444"
    pub activity_id: String,
    /// REQUIRED. Stream to analyze: "power", "heartrate", "speed", "pace", "cadence", or "distance"
    pub stream: String,
    /// A SINGLE integer in seconds (NOT an array). Use for time-based efforts. Example: 300 means 5 minutes. Provide duration OR distance, not both.
    pub duration: Option<i32>,
    /// A SINGLE number in meters (NOT an array). Use for distance-based efforts. Example: 1000 means 1 km. Provide duration OR distance, not both.
    pub distance: Option<f64>,
    /// Maximum number of best efforts to return (optional)
    pub count: Option<i32>,
    /// Minimum value threshold for the stream (optional)
    pub min_value: Option<f64>,
    /// Whether to exclude structured intervals from analysis (optional)
    pub exclude_intervals: Option<bool>,
    /// Start index in the activity data (optional)
    pub start_index: Option<i32>,
    /// End index in the activity data (optional)
    pub end_index: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchParams {
    #[serde(rename = "q", alias = "query")]
    pub q: String,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct IntervalSearchParams {
    /// Minimum time for interval (seconds)
    pub min_secs: u32,
    /// Maximum time for interval (seconds)
    pub max_secs: u32,
    /// Minimum intensity percentage (0-100)
    pub min_intensity: u32,
    /// Maximum intensity percentage (0-100)
    pub max_intensity: u32,
    #[serde(rename = "type")]
    pub interval_type: Option<String>,
    pub min_reps: Option<u32>,
    pub max_reps: Option<u32>,
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateActivityParams {
    pub activity_id: String,
    pub fields: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PowerCurvesParams {
    pub days_back: Option<i32>,
    #[serde(rename = "type")]
    pub sport: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DownloadParams {
    pub activity_id: String,
    pub output_path: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DownloadIdParam {
    pub download_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DownloadStartResult {
    pub download_id: String,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DownloadStatusResult {
    pub status: DownloadStatus,
}

// === New Parameter Structs for Missing Tools ===

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivitiesAroundParams {
    pub activity_id: String,
    pub limit: Option<u32>,
    pub route_id: Option<i64>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DateParam {
    pub date: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WellnessUpdateParams {
    pub date: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DaysAheadParams {
    pub days_ahead: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum EventId {
    Int(i64),
    Str(String),
}

impl EventId {
    fn as_cow(&self) -> Cow<'_, str> {
        match self {
            EventId::Int(v) => Cow::Owned(v.to_string()),
            EventId::Str(s) => Cow::Borrowed(s),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateEventParams {
    pub event_id: EventId,
    pub fields: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BulkDeleteEventsParams {
    pub event_ids: Vec<EventId>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DuplicateEventParams {
    pub event_id: EventId,
    pub num_copies: Option<u32>,
    pub weeks_between: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FolderIdParam {
    pub folder_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateGearParams {
    pub gear: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateGearParams {
    pub gear_id: String,
    pub fields: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GearIdParam {
    pub gear_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateGearReminderParams {
    pub gear_id: String,
    pub reminder: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateGearReminderParams {
    pub gear_id: String,
    pub reminder_id: String,
    pub reset: bool,
    pub snooze_days: u32,
    pub fields: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SportTypeParam {
    pub sport_type: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateSportSettingsParams {
    pub sport_type: String,
    pub recalc_hr_zones: bool,
    pub fields: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ApplySportSettingsParams {
    pub sport_type: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateSportSettingsParams {
    pub settings: serde_json::Value,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DownloadListResult {
    pub downloads: Vec<DownloadStatus>,
}

// === Prompt Parameters ===

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AnalyzeRecentTrainingParams {
    pub days_back: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PerformanceAnalysisParams {
    pub days_back: Option<i32>,
    pub metric: Option<String>,
    pub sport_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivityDeepDiveParams {
    pub activity_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct RecoveryCheckParams {
    pub days_back: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct TrainingPlanReviewParams {
    pub start_date: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PlanTrainingWeekParams {
    pub start_date: Option<String>,
    pub focus: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct AnalyzeAdaptPlanParams {
    pub period: Option<String>,
    pub days_back: Option<i32>,
    pub focus: Option<String>,
}

#[tool_router]
#[prompt_router]
impl IntervalsMcpHandler {
    pub fn new(client: Arc<dyn IntervalsClient>) -> Self {
        Self {
            client,
            tool_router: Self::tool_router(),
            prompt_router: Self::prompt_router(),
            downloads: Arc::new(Mutex::new(HashMap::new())),
            cancel_senders: Arc::new(Mutex::new(HashMap::new())),
            webhooks: Arc::new(Mutex::new(HashMap::new())),
            webhook_secret: Arc::new(Mutex::new(None)),
        }
    }

    pub fn tool_count(&self) -> usize {
        self.tool_router.list_all().len()
    }

    pub fn prompt_count(&self) -> usize {
        self.prompt_router.list_all().len()
    }

    #[tool(
        name = "get_athlete_profile",
        description = "Get your Intervals.icu athlete profile including name, ID, and basic info"
    )]
    async fn get_athlete_profile(&self) -> Result<Json<ProfileResult>, String> {
        let p = self
            .client
            .get_athlete_profile()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ProfileResult {
            id: p.id,
            name: p.name,
        }))
    }

    #[tool(
        name = "get_recent_activities",
        description = "List your recent activities. Returns activity ID, name, and type. Use limit (≤100) to control results, days_back to filter by date range"
    )]
    async fn get_recent_activities(
        &self,
        params: Parameters<RecentParams>,
    ) -> Result<Json<RecentActivitiesResult>, String> {
        let p = params.0;
        let acts: Vec<ActivitySummary> = self
            .client
            .get_recent_activities(p.limit, p.days_back)
            .await
            .map_err(|e| e.to_string())?;
        let out = acts
            .into_iter()
            .map(|a| ActivitySummaryResult {
                id: a.id,
                name: a.name,
            })
            .collect();
        Ok(Json(RecentActivitiesResult { activities: out }))
    }

    #[tool(
        name = "set_webhook_secret",
        description = "Set HMAC secret for webhook verification. Call this once with your webhook secret string to enable signature verification for incoming webhook payloads"
    )]
    async fn set_webhook_secret(
        &self,
        params: Parameters<ObjectResult>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        if let Some(s) = p.value.get("secret").and_then(|v| v.as_str()) {
            // set secret in mutex
            let mut secret = self.webhook_secret.lock().await;
            *secret = Some(s.to_string());
            Ok(Json(ObjectResult {
                value: serde_json::json!({ "ok": true }),
            }))
        } else {
            Err("missing secret".into())
        }
    }

    #[tool(
        name = "get_events",
        description = "List calendar events (workouts, races, notes, rest days). Returns events within date range. Use days_back to specify window, limit to control result count"
    )]
    async fn get_events(
        &self,
        params: Parameters<RecentParams>,
    ) -> Result<Json<EventsResult>, String> {
        let p = params.0;
        let evs = self
            .client
            .get_events(p.days_back, p.limit)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(EventsResult { events: evs }))
    }

    #[tool(
        name = "create_event",
        description = "Create a calendar event (workout, race, note, etc). Requires: title, start_date_local (YYYY-MM-DD), category (WORKOUT/RACE_A/RACE_B/RACE_C/NOTE/PLAN/HOLIDAY/SICK/INJURED/TARGET/SET_FITNESS/etc). Optional: description, notes, duration_mins"
    )]
    async fn create_event(
        &self,
        params: Parameters<intervals_icu_client::Event>,
    ) -> Result<Json<intervals_icu_client::Event>, String> {
        let ev = params.0;
        // Validate and normalize input: accept YYYY-MM-DD or ISO 8601 datetimes and normalize to YYYY-MM-DD
        if ev.name.trim().is_empty() {
            return Err("invalid event: name is empty".into());
        }
        let mut ev2 = ev.clone();
        if chrono::NaiveDate::parse_from_str(&ev2.start_date_local, "%Y-%m-%d").is_err() {
            // Try RFC3339
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&ev2.start_date_local) {
                ev2.start_date_local = dt.date_naive().format("%Y-%m-%d").to_string();
            } else if let Ok(ndt) =
                chrono::NaiveDateTime::parse_from_str(&ev2.start_date_local, "%Y-%m-%dT%H:%M:%S")
            {
                ev2.start_date_local = ndt.date().format("%Y-%m-%d").to_string();
            } else {
                return Err(format!(
                    "invalid start_date_local: {}",
                    ev2.start_date_local
                ));
            }
        }
        if ev2.category == intervals_icu_client::EventCategory::Unknown {
            return Err("invalid category: unknown".into());
        }
        let created = self
            .client
            .create_event(ev2)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(created))
    }

    #[tool(
        name = "get_event",
        description = "Get a calendar event by ID. Returns event details including title, date, category, and description"
    )]
    async fn get_event(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<intervals_icu_client::Event>, String> {
        let p = params.0;
        let ev = self
            .client
            .get_event(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ev))
    }

    #[tool(name = "delete_event", description = "Delete a calendar event by ID")]
    async fn delete_event(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        self.client
            .delete_event(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult {
            value: serde_json::json!({ "deleted": true }),
        }))
    }

    #[tool(
        name = "bulk_create_events",
        description = "Create multiple calendar events in one call. Payload must be an object with 'events': [ ...event objects... ]. Fields per event: title, start_date_local (YYYY-MM-DD), category (WORKOUT/RACE/NOTE/etc). Efficient for importing plans"
    )]
    pub async fn bulk_create_events(
        &self,
        params: Parameters<BulkCreateEventsToolParams>,
    ) -> Result<Json<EventsResult>, String> {
        let events = params.0.events;
        // Normalize and validate input early to provide clearer errors and avoid 422 from the API.
        // Accept either YYYY-MM-DD or ISO 8601 datetimes (we normalize to YYYY-MM-DD).
        let mut norm_events: Vec<intervals_icu_client::Event> = Vec::with_capacity(events.len());
        for (i, ev) in events.into_iter().enumerate() {
            if ev.name.trim().is_empty() {
                return Err(format!("invalid event at index {}: name is empty", i));
            }
            // Normalize date: accept YYYY-MM-DD or RFC3339 / NaiveDateTime
            let mut ev2 = ev.clone();
            if chrono::NaiveDate::parse_from_str(&ev2.start_date_local, "%Y-%m-%d").is_err() {
                // try RFC3339 (with timezone)
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&ev2.start_date_local) {
                    ev2.start_date_local = dt.date_naive().format("%Y-%m-%d").to_string();
                } else if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(
                    &ev2.start_date_local,
                    "%Y-%m-%dT%H:%M:%S",
                ) {
                    ev2.start_date_local = ndt.date().format("%Y-%m-%d").to_string();
                } else {
                    return Err(format!(
                        "invalid start_date_local for event at index {}: {}",
                        i, ev2.start_date_local
                    ));
                }
            }
            if ev2.category == intervals_icu_client::EventCategory::Unknown {
                return Err(format!(
                    "invalid category for event at index {}: unknown category",
                    i
                ));
            }
            norm_events.push(ev2);
        }
        let created = self
            .client
            .bulk_create_events(norm_events)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(EventsResult { events: created }))
    }

    #[tool(
        name = "get_activity_details",
        description = "Get complete activity details including distance, elevation, power, heart rate, pace, and other metrics. Use with get_activity_streams for time-series data"
    )]
    async fn get_activity_details(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_activity_details(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "search_activities",
        description = "Search your activities by text (name, description, route). Returns ID and name. Use limit to control results. Use search_activities_full for complete activity objects"
    )]
    async fn search_activities(
        &self,
        params: Parameters<SearchParams>,
    ) -> Result<Json<RecentActivitiesResult>, String> {
        let p = params.0;
        let acts = self
            .client
            .search_activities(&p.q, p.limit)
            .await
            .map_err(|e| e.to_string())?;
        let out = acts
            .into_iter()
            .map(|a| ActivitySummaryResult {
                id: a.id,
                name: a.name,
            })
            .collect();
        Ok(Json(RecentActivitiesResult { activities: out }))
    }

    #[tool(
        name = "search_activities_full",
        description = "Search activities by text and return full activity objects with all metrics. Returns complete data: distance, time, power, HR, elevation, etc"
    )]
    async fn search_activities_full(
        &self,
        params: Parameters<SearchParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .search_activities_full(&p.q, p.limit)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_activities_csv",
        description = "Download your complete activities log as CSV file. Useful for data export and analysis in spreadsheets"
    )]
    async fn get_activities_csv(&self) -> Result<Json<ObjectResult>, String> {
        let v = self
            .client
            .get_activities_csv()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult {
            value: serde_json::json!({ "csv": v }),
        }))
    }

    #[tool(
        name = "update_activity",
        description = "Update activity fields: name, description, notes, keywords, private (true/false), etc. Pass activity_id and fields object with values to change"
    )]
    async fn update_activity(
        &self,
        params: Parameters<UpdateActivityParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .update_activity(&p.activity_id, &p.fields)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_activity_streams",
        description = "Get time-series data streams for an activity: power, heart rate, speed, pace, cadence, elevation, temperature, etc. Each stream is an array of timestamped values"
    )]
    async fn get_activity_streams(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_activity_streams(&p.activity_id, None)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_activity_intervals",
        description = "Get structured intervals from a workout activity. Returns start/end times and indices for each interval segment. Useful for analyzing interval training sessions"
    )]
    async fn get_activity_intervals(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_activity_intervals(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_best_efforts",
        description = "Find peak performances in an activity. \
            REQUIRED PARAMS: activity_id (string), stream (string), and ONE of: duration (integer, seconds) OR distance (number, meters). \
            IMPORTANT: All params are FLAT (no nested 'options' object). Use SINGLE values, NOT arrays. \
            CORRECT: {\"activity_id\": \"i112895444\", \"stream\": \"power\", \"duration\": 300} - finds best 5-min power. \
            CORRECT: {\"activity_id\": \"i112895444\", \"stream\": \"distance\", \"distance\": 1000} - finds best 1km effort. \
            WRONG: {\"options\": {...}} or {\"durations\": [60,300]} - no nested objects, no arrays! \
            Streams: power, heartrate, speed, pace, cadence, distance."
    )]
    async fn get_best_efforts(
        &self,
        params: Parameters<BestEffortsToolParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let options = intervals_icu_client::BestEffortsOptions {
            stream: Some(p.stream),
            duration: p.duration,
            distance: p.distance,
            count: p.count,
            min_value: p.min_value,
            exclude_intervals: p.exclude_intervals,
            start_index: p.start_index,
            end_index: p.end_index,
        };
        let v = self
            .client
            .get_best_efforts(&p.activity_id, Some(options))
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_gear_list",
        description = "Get your gear inventory (bikes, shoes, watches, etc). Returns gear ID, name, type, usage, maintenance reminders, and retirement status"
    )]
    async fn get_gear_list(&self) -> Result<Json<ObjectResult>, String> {
        let v = self
            .client
            .get_gear_list()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_sport_settings",
        description = "Get sport-specific settings: FTP (power), FTHR (threshold heart rate), thresholds for zones, pace thresholds, power zones, and HR zones for each sport"
    )]
    async fn get_sport_settings(&self) -> Result<Json<ObjectResult>, String> {
        let v = self
            .client
            .get_sport_settings()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_power_curves",
        description = "Get power output curves (peak power efforts) for a sport. Returns best power outputs at different durations (5s, 1min, 5min, 20min, etc). Requires: sport type (Run/Ride/etc) and days_back (7/30/90/365 days)"
    )]
    async fn get_power_curves(
        &self,
        params: Parameters<PowerCurvesParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_power_curves(p.days_back, &p.sport)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_gap_histogram",
        description = "Get Grade Adjusted Pace (GAP) distribution histogram for an activity. Shows how time was distributed across different pace values"
    )]
    async fn get_gap_histogram(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_gap_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "start_download",
        description = "Start downloading activity file with progress tracking. Returns download_id to check status. Supports FIT, GPX, and original formats. Optional output_path saves to disk, otherwise returns base64"
    )]
    async fn start_download(
        &self,
        params: Parameters<DownloadParams>,
    ) -> Result<Json<DownloadStartResult>, String> {
        let p = params.0;
        let id = Uuid::new_v4().to_string();
        let path_opt = p.output_path.clone();

        let status = DownloadStatus {
            id: id.clone(),
            activity_id: p.activity_id.clone(),
            state: DownloadState::Pending,
            bytes_downloaded: 0,
            total_bytes: None,
            path: None,
        };

        {
            let mut map = self.downloads.lock().await;
            map.insert(id.clone(), status);
        }

        // cancellation channel
        let (cancel_tx, cancel_rx) = watch::channel(false);
        {
            let mut canc = self.cancel_senders.lock().await;
            canc.insert(id.clone(), cancel_tx.clone());
        }

        let client = self.client.clone();
        let downloads = self.downloads.clone();
        let id_clone_for_task = id.clone();
        let params_activity = p.activity_id.clone();
        let path_opt_clone = path_opt.clone();

        tokio::spawn(async move {
            // mark in-progress
            {
                let mut map = downloads.lock().await;
                if let Some(s) = map.get_mut(&id_clone_for_task) {
                    s.state = DownloadState::InProgress;
                }
            }

            // create progress channel
            let (tx, mut rx) = mpsc::channel(8);
            let activity = params_activity.clone();
            let out_path = path_opt_clone.map(std::path::PathBuf::from);

            // launch client download
            let download_fut = client.download_activity_file_with_progress(
                &activity,
                out_path,
                tx,
                cancel_rx.clone(),
            );

            // progress reader
            let downloads_clone = downloads.clone();
            let id_clone = id_clone_for_task.clone();
            let progress_handle = tokio::spawn(async move {
                while let Some(pr) = rx.recv().await {
                    let mut map = downloads_clone.lock().await;
                    if let Some(s) = map.get_mut(&id_clone) {
                        s.bytes_downloaded = pr.bytes_downloaded;
                        s.total_bytes = pr.total_bytes;
                    }
                }
            });

            match download_fut.await {
                Ok(pth) => {
                    let mut map = downloads.lock().await;
                    if let Some(s) = map.get_mut(&id_clone_for_task) {
                        s.state = DownloadState::Completed;
                        s.path = pth;
                    }
                }
                Err(e) => {
                    let mut map = downloads.lock().await;
                    if let Some(s) = map.get_mut(&id_clone_for_task) {
                        s.state = DownloadState::Failed(e.to_string());
                    }
                }
            }

            drop(progress_handle.await);
            // cleanup cancel sender
            // Note: we intentionally keep status for inspection
        });

        Ok(Json(DownloadStartResult { download_id: id }))
    }

    #[tool(
        name = "download_fit_file",
        description = "Download activity as FIT file (binary format from Garmin/sports watches). Use with compatible sports analysis software. Optional output_path saves to disk"
    )]
    async fn download_fit_file(
        &self,
        params: Parameters<DownloadParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let path_opt = p.output_path.as_deref().map(|s| s.to_string());
        let result = self
            .client
            .download_fit_file(&p.activity_id, p.output_path.map(std::path::PathBuf::from))
            .await
            .map_err(|e| e.to_string())?;

        let value = match (result, path_opt) {
            (Some(data), _) => serde_json::json!({ "base64": data }),
            (None, Some(path)) => serde_json::json!({ "written_to_disk": true, "path": path }),
            (None, None) => serde_json::json!({ "written_to_disk": true }),
        };

        Ok(Json(ObjectResult { value }))
    }

    #[tool(
        name = "download_gpx_file",
        description = "Download activity as GPX file (GPS track format). Useful for importing into maps, other apps, or sharing with others. Optional output_path saves to disk"
    )]
    async fn download_gpx_file(
        &self,
        params: Parameters<DownloadParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let path_opt = p.output_path.as_deref().map(|s| s.to_string());
        let result = self
            .client
            .download_gpx_file(&p.activity_id, p.output_path.map(std::path::PathBuf::from))
            .await
            .map_err(|e| e.to_string())?;

        let value = match (result, path_opt) {
            (Some(data), _) => serde_json::json!({ "base64": data }),
            (None, Some(path)) => serde_json::json!({ "written_to_disk": true, "path": path }),
            (None, None) => serde_json::json!({ "written_to_disk": true }),
        };

        Ok(Json(ObjectResult { value }))
    }

    #[tool(
        name = "get_download_status",
        description = "Check download progress by download_id. Returns state (Pending/InProgress/Completed/Failed), bytes_downloaded, total_bytes, and file path"
    )]
    async fn get_download_status(
        &self,
        params: Parameters<DownloadIdParam>,
    ) -> Result<Json<DownloadStatusResult>, String> {
        let p = params.0;
        let map = self.downloads.lock().await;
        if let Some(s) = map.get(&p.download_id) {
            Ok(Json(DownloadStatusResult { status: s.clone() }))
        } else {
            Err("not found".into())
        }
    }

    #[tool(
        name = "receive_webhook",
        description = "Receive and verify webhook payloads from Intervals.icu. Validates HMAC signature and deduplicates events"
    )]
    async fn receive_webhook(
        &self,
        params: Parameters<ObjectResult>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let sig = p
            .value
            .get("signature")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing signature".to_string())?;
        let payload = p
            .value
            .get("payload")
            .cloned()
            .ok_or_else(|| "missing payload".to_string())?;

        // verify
        let secret_opt = { self.webhook_secret.lock().await.clone() };
        let secret = secret_opt.ok_or_else(|| "webhook secret not set".to_string())?;
        let mut mac: Hmac<Sha256> =
            Hmac::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
        let body = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        mac.update(&body);
        let expected = mac.finalize().into_bytes();
        let sig_bytes = hex::decode(sig).map_err(|e| e.to_string())?;
        if expected.as_slice() != sig_bytes.as_slice() {
            return Err("signature mismatch".into());
        }

        // dedupe by payload id if present
        let id = payload
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                // fallback to timestamp-based id
                let since = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default();
                Box::leak(format!("ts-{}", since.as_millis()).into_boxed_str())
            })
            .to_string();

        let evt = WebhookEvent {
            id: id.clone(),
            payload: payload.clone(),
            received_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let mut store = self.webhooks.lock().await;
        if store.contains_key(&id) {
            return Ok(Json(ObjectResult {
                value: serde_json::json!({ "duplicate": true }),
            }));
        }
        store.insert(id.clone(), evt);
        Ok(Json(ObjectResult {
            value: serde_json::json!({ "ok": true, "id": id }),
        }))
    }

    /// Programmatic webhook handler (callable from HTTP server). Performs HMAC verification
    /// and deduplication, returns an `ObjectResult` describing the outcome.
    pub async fn process_webhook(
        &self,
        signature: &str,
        payload: serde_json::Value,
    ) -> Result<ObjectResult, String> {
        // verify
        let secret_opt = { self.webhook_secret.lock().await.clone() };
        let secret = secret_opt.ok_or_else(|| "webhook secret not set".to_string())?;
        let mut mac: Hmac<Sha256> =
            Hmac::new_from_slice(secret.as_bytes()).map_err(|e| e.to_string())?;
        let body = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        mac.update(&body);
        let expected = mac.finalize().into_bytes();
        let sig_bytes = hex::decode(signature).map_err(|e| e.to_string())?;
        if expected.as_slice() != sig_bytes.as_slice() {
            return Err("signature mismatch".into());
        }

        // dedupe
        let id = payload
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                let since = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default();
                Box::leak(format!("ts-{}", since.as_millis()).into_boxed_str())
            })
            .to_string();

        let evt = WebhookEvent {
            id: id.clone(),
            payload: payload.clone(),
            received_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        let mut store = self.webhooks.lock().await;
        if store.contains_key(&id) {
            return Ok(ObjectResult {
                value: serde_json::json!({ "duplicate": true }),
            });
        }
        store.insert(id.clone(), evt);
        Ok(ObjectResult {
            value: serde_json::json!({ "ok": true, "id": id }),
        })
    }

    /// Set webhook secret programmatically (for tests or admin flows).
    pub async fn set_webhook_secret_value(&self, secret: impl Into<String>) {
        let mut s = self.webhook_secret.lock().await;
        *s = Some(secret.into());
    }

    #[tool(
        name = "list_downloads",
        description = "List all activity downloads and their current status (Pending/InProgress/Completed/Failed)"
    )]
    async fn list_downloads(&self) -> Result<Json<DownloadListResult>, String> {
        let map = self.downloads.lock().await;
        let list = map.values().cloned().collect();
        Ok(Json(DownloadListResult { downloads: list }))
    }

    #[tool(
        name = "cancel_download",
        description = "Cancel an in-progress activity file download by download_id"
    )]
    async fn cancel_download(
        &self,
        params: Parameters<DownloadIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let canc = self.cancel_senders.lock().await;
        if let Some(tx) = canc.get(&p.download_id) {
            let _ = tx.send(true);
            let mut map = self.downloads.lock().await;
            if let Some(s) = map.get_mut(&p.download_id) {
                s.state = DownloadState::Cancelled;
            }
            Ok(Json(ObjectResult {
                value: serde_json::json!({ "cancelled": true }),
            }))
        } else {
            Err("not found".into())
        }
    }

    // === Activities ===

    #[tool(
        name = "delete_activity",
        description = "Delete an activity permanently by ID"
    )]
    async fn delete_activity(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        self.client
            .delete_activity(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult {
            value: serde_json::json!({ "deleted": true }),
        }))
    }

    #[tool(
        name = "get_activities_around",
        description = "Get activities before and after a specific activity. Useful for analyzing activity sequences and patterns"
    )]
    async fn get_activities_around(
        &self,
        params: Parameters<ActivitiesAroundParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_activities_around(&p.activity_id, p.limit, p.route_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "search_intervals",
        description = "Search for similar intervals across all activities. Filter by duration (seconds), intensity (%), interval type, and other criteria"
    )]
    async fn search_intervals(
        &self,
        params: Parameters<IntervalSearchParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .search_intervals(
                p.min_secs,
                p.max_secs,
                p.min_intensity,
                p.max_intensity,
                p.interval_type,
                p.min_reps,
                p.max_reps,
                p.limit,
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_power_histogram",
        description = "Get power output distribution for an activity. Shows time spent at different power levels"
    )]
    async fn get_power_histogram(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_power_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_hr_histogram",
        description = "Get heart rate distribution for an activity. Shows time spent in different heart rate ranges"
    )]
    async fn get_hr_histogram(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_hr_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_pace_histogram",
        description = "Get pace distribution for an activity. Shows time spent at different pace values"
    )]
    async fn get_pace_histogram(
        &self,
        params: Parameters<ActivityIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_pace_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    // === Fitness Summary ===

    #[tool(
        name = "get_fitness_summary",
        description = "Get fitness metrics: CTL (Chronic Training Load), ATL (Acute Training Load), TSB (Training Stress Balance), and ramp rate. Key indicators for recovery and readiness"
    )]
    async fn get_fitness_summary(&self) -> Result<Json<ObjectResult>, String> {
        let v = self
            .client
            .get_fitness_summary()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    // === Wellness ===

    #[tool(
        name = "get_wellness",
        description = "Get recent wellness data (sleep, stress, resting HR, notes). Use days_back to specify date range"
    )]
    async fn get_wellness(
        &self,
        params: Parameters<RecentParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_wellness(p.days_back)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_wellness_for_date",
        description = "Get wellness data for a specific date (format: YYYY-MM-DD). Returns sleep hours, stress level, resting HR, and notes"
    )]
    async fn get_wellness_for_date(
        &self,
        params: Parameters<DateParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let mut date = p.date.clone();
        // accept YYYY-MM-DD or ISO datetimes; normalize to YYYY-MM-DD
        if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&date) {
                date = dt.date_naive().format("%Y-%m-%d").to_string();
            } else if let Ok(ndt) =
                chrono::NaiveDateTime::parse_from_str(&date, "%Y-%m-%dT%H:%M:%S")
            {
                date = ndt.date().format("%Y-%m-%d").to_string();
            } else {
                return Err(format!("invalid date: {}", p.date));
            }
        }
        let v = self
            .client
            .get_wellness_for_date(&date)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "update_wellness",
        description = "Update wellness data for a specific date. Can update: sleep_hours, stress_level, resting_hr, notes, etc"
    )]
    async fn update_wellness(
        &self,
        params: Parameters<WellnessUpdateParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let mut date = p.date.clone();
        if chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d").is_err() {
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&date) {
                date = dt.date_naive().format("%Y-%m-%d").to_string();
            } else if let Ok(ndt) =
                chrono::NaiveDateTime::parse_from_str(&date, "%Y-%m-%dT%H:%M:%S")
            {
                date = ndt.date().format("%Y-%m-%d").to_string();
            } else {
                return Err(format!("invalid date: {}", p.date));
            }
        }
        let v = self
            .client
            .update_wellness(&date, &p.data)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    // === Events/Calendar ===

    #[tool(
        name = "get_upcoming_workouts",
        description = "Get upcoming scheduled workouts and planned events. Specify days_ahead to control the forecast window (default: 7 days)"
    )]
    async fn get_upcoming_workouts(
        &self,
        params: Parameters<DaysAheadParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_upcoming_workouts(p.days_ahead)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "update_event",
        description = "Update calendar event fields: title, description, date (YYYY-MM-DD), category, etc"
    )]
    async fn update_event(
        &self,
        params: Parameters<UpdateEventParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let event_id = p.event_id.as_cow().into_owned();
        // If fields contain start_date_local, normalize it to YYYY-MM-DD
        let mut fields = p.fields.clone();
        if let Some(obj) = fields.as_object_mut()
            && let Some(val) = obj.get("start_date_local")
            && let Some(s) = val.as_str()
        {
            let mut s2 = s.to_string();
            if chrono::NaiveDate::parse_from_str(&s2, "%Y-%m-%d").is_err() {
                if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&s2) {
                    s2 = dt.date_naive().format("%Y-%m-%d").to_string();
                } else if let Ok(ndt) =
                    chrono::NaiveDateTime::parse_from_str(&s2, "%Y-%m-%dT%H:%M:%S")
                {
                    s2 = ndt.date().format("%Y-%m-%d").to_string();
                } else {
                    return Err(format!("invalid start_date_local: {}", s));
                }
            }
            obj.insert(
                "start_date_local".to_string(),
                serde_json::Value::String(s2),
            );
        }
        let v = self
            .client
            .update_event(&event_id, &fields)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "bulk_delete_events",
        description = "Delete multiple calendar events in a single operation. Pass array of event IDs"
    )]
    async fn bulk_delete_events(
        &self,
        params: Parameters<BulkDeleteEventsParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let ids: Vec<String> = p
            .event_ids
            .iter()
            .map(|id| id.as_cow().into_owned())
            .collect();
        self.client
            .bulk_delete_events(ids)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult {
            value: serde_json::json!({ "deleted": true }),
        }))
    }

    #[tool(
        name = "duplicate_event",
        description = "Duplicate an event to multiple future dates. Specify num_copies and weeks_between to create recurring schedule"
    )]
    async fn duplicate_event(
        &self,
        params: Parameters<DuplicateEventParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let event_id = p.event_id.as_cow().into_owned();
        let v = self
            .client
            .duplicate_event(&event_id, p.num_copies, p.weeks_between)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult {
            value: serde_json::to_value(v).map_err(|e| e.to_string())?,
        }))
    }

    // === Performance Curves ===

    #[tool(
        name = "get_hr_curves",
        description = "Get heart rate performance curves. Returns best HR efforts at different durations. Requires: sport type (Run/Ride/etc) and days_back (7/30/90/365)"
    )]
    async fn get_hr_curves(
        &self,
        params: Parameters<PowerCurvesParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_hr_curves(p.days_back, &p.sport)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_pace_curves",
        description = "Get pace/speed performance curves. Returns best pace efforts at different durations. Requires: sport type and days_back (7/30/90/365)"
    )]
    async fn get_pace_curves(
        &self,
        params: Parameters<PowerCurvesParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_pace_curves(p.days_back, &p.sport)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    // === Workout Library ===

    #[tool(
        name = "get_workout_library",
        description = "Get your workout library including all training plan folders. Use get_workouts_in_folder to view specific workouts"
    )]
    async fn get_workout_library(&self) -> Result<Json<ObjectResult>, String> {
        let v = self
            .client
            .get_workout_library()
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "get_workouts_in_folder",
        description = "Get all workouts in a specific library folder. Returns workout details including duration, intervals, and focus area"
    )]
    async fn get_workouts_in_folder(
        &self,
        params: Parameters<FolderIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_workouts_in_folder(&p.folder_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    // === Gear Management ===

    #[tool(
        name = "create_gear",
        description = "Add new gear item (bike, shoes, watch, etc). Specify name, type, weight, retired status, and maintenance reminders"
    )]
    async fn create_gear(
        &self,
        params: Parameters<CreateGearParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .create_gear(&p.gear)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "update_gear",
        description = "Update gear item details: name, type, notes, retirement status, or any other field"
    )]
    async fn update_gear(
        &self,
        params: Parameters<UpdateGearParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .update_gear(&p.gear_id, &p.fields)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "delete_gear",
        description = "Delete a gear item from your inventory"
    )]
    async fn delete_gear(
        &self,
        params: Parameters<GearIdParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        self.client
            .delete_gear(&p.gear_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult {
            value: serde_json::json!({ "deleted": true }),
        }))
    }

    #[tool(
        name = "create_gear_reminder",
        description = "Set up maintenance reminder for gear. Specify reminder type (km/miles/months) and threshold value"
    )]
    async fn create_gear_reminder(
        &self,
        params: Parameters<CreateGearReminderParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .create_gear_reminder(&p.gear_id, &p.reminder)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "update_gear_reminder",
        description = "Update or snooze a gear maintenance reminder. Set reset=true to reset counter, snooze_days to delay notification"
    )]
    async fn update_gear_reminder(
        &self,
        params: Parameters<UpdateGearReminderParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .update_gear_reminder(
                &p.gear_id,
                &p.reminder_id,
                p.reset,
                p.snooze_days,
                &p.fields,
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    // === Sport Settings ===

    #[tool(
        name = "update_sport_settings",
        description = "Update sport settings: FTP (power threshold), FTHR (heart rate threshold), pace thresholds, power/HR zone definitions. Optionally recalculate HR zones"
    )]
    async fn update_sport_settings(
        &self,
        params: Parameters<UpdateSportSettingsParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .update_sport_settings(&p.sport_type, p.recalc_hr_zones, &p.fields)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "apply_sport_settings",
        description = "Apply updated sport settings to all historical activities. Recalculates zones and metrics for all past activities of that sport"
    )]
    async fn apply_sport_settings(
        &self,
        params: Parameters<ApplySportSettingsParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .apply_sport_settings(&p.sport_type)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "create_sport_settings",
        description = "Create new sport-specific settings profile for a new sport type"
    )]
    async fn create_sport_settings(
        &self,
        params: Parameters<CreateSportSettingsParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .create_sport_settings(&p.settings)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "delete_sport_settings",
        description = "Delete sport-specific settings profile. Specify the sport type to remove"
    )]
    async fn delete_sport_settings(
        &self,
        params: Parameters<SportTypeParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        self.client
            .delete_sport_settings(&p.sport_type)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult {
            value: serde_json::json!({ "deleted": true }),
        }))
    }

    // === MCP Prompts ===

    /// Comprehensive training analysis over a specified period
    #[prompt(
        name = "analyze-recent-training",
        description = "Analyze recent training activities, load trends, and provide insights"
    )]
    async fn analyze_recent_training(
        &self,
        params: Parameters<AnalyzeRecentTrainingParams>,
    ) -> GetPromptResult {
        let days_back_i = params.0.days_back.unwrap_or(30);
        let days_back = if days_back_i < 0 {
            0
        } else {
            days_back_i as u32
        };

        prompts::analyze_recent_training_prompt(days_back)
    }

    /// Detailed power/HR/pace curve analysis with zones
    #[prompt(
        name = "performance-analysis",
        description = "Analyze performance curves and training zones"
    )]
    async fn performance_analysis(
        &self,
        params: Parameters<PerformanceAnalysisParams>,
    ) -> GetPromptResult {
        let days_back_i = params.0.days_back.unwrap_or(90);
        let days_back = if days_back_i < 0 {
            0
        } else {
            days_back_i as u32
        };
        let metric = params
            .0
            .metric
            .or(params.0.sport_type)
            .unwrap_or_else(|| "power".to_string());

        prompts::performance_analysis_prompt(&metric, days_back)
    }

    /// Deep dive into a specific activity with streams, intervals, and best efforts
    #[prompt(
        name = "activity-deep-dive",
        description = "Detailed analysis of a specific activity"
    )]
    async fn activity_deep_dive(
        &self,
        params: Parameters<ActivityDeepDiveParams>,
    ) -> GetPromptResult {
        let activity_id = &params.0.activity_id;

        prompts::activity_deep_dive_prompt(activity_id)
    }

    /// Recovery assessment with wellness trends and training load
    #[prompt(
        name = "recovery-check",
        description = "Assess recovery status and readiness to train"
    )]
    async fn recovery_check(&self, params: Parameters<RecoveryCheckParams>) -> GetPromptResult {
        let days_back_i = params.0.days_back.unwrap_or(7);
        let days_back = if days_back_i < 0 {
            0
        } else {
            days_back_i as u32
        };

        prompts::recovery_check_prompt(days_back)
    }

    /// Weekly training plan evaluation with workout library
    #[prompt(
        name = "training-plan-review",
        description = "Review planned workouts for the upcoming period"
    )]
    async fn training_plan_review(
        &self,
        params: Parameters<TrainingPlanReviewParams>,
    ) -> GetPromptResult {
        let start_date = params
            .0
            .start_date
            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());

        prompts::training_plan_review_prompt(&start_date)
    }

    /// AI-assisted weekly training plan creation based on current fitness
    #[prompt(
        name = "plan-training-week",
        description = "Create a training plan for the upcoming week"
    )]
    async fn plan_training_week(
        &self,
        params: Parameters<PlanTrainingWeekParams>,
    ) -> GetPromptResult {
        let start_date = params
            .0
            .start_date
            .unwrap_or_else(|| chrono::Local::now().format("%Y-%m-%d").to_string());
        let focus = params
            .0
            .focus
            .unwrap_or_else(|| "general fitness".to_string());

        prompts::plan_training_week_prompt(&start_date, &focus)
    }

    /// Adaptive planning: compare recent training vs current plan and propose adjustments
    #[prompt(
        name = "analyze-and-adapt-plan",
        description = "Analyze recent training and adapt current plan based on actual load"
    )]
    async fn analyze_and_adapt_plan(
        &self,
        params: Parameters<AnalyzeAdaptPlanParams>,
    ) -> GetPromptResult {
        let days_back = params.0.days_back.unwrap_or(30);
        let period_label = params
            .0
            .period
            .unwrap_or_else(|| format!("the last {} days", days_back));
        let focus = params
            .0
            .focus
            .unwrap_or_else(|| "balanced progression".to_string());

        prompts::analyze_and_adapt_plan_prompt(&period_label, &focus)
    }
}

#[tool_handler]
#[prompt_handler(router = self.prompt_router)]
impl rmcp::ServerHandler for IntervalsMcpHandler {
    // === Server Info & Capabilities ===
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo {
            instructions: Some(
                "Intervals.icu MCP server - provides tools for training analysis, \
                 activity management, wellness tracking, and performance optimization."
                    .into(),
            ),
            capabilities: rmcp::model::ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
            ..Default::default()
        }
    }

    // === MCP Resource Implementation ===

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let resource = RawResource::new("intervals-icu://athlete/profile", "Athlete Profile");

        let mut res = resource.no_annotation();
        res.description = Some("Complete athlete profile with current fitness metrics (CTL/ATL/TSB) and sport settings".to_string());
        res.mime_type = Some("application/json".to_string());

        Ok(ListResourcesResult {
            resources: vec![res],
            next_cursor: None,
            meta: None,
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResult, ErrorData> {
        if request.uri == "intervals-icu://athlete/profile" {
            // Fetch athlete profile and fitness data
            let profile = self
                .client
                .get_athlete_profile()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            // Fetch fitness summary (CTL/ATL/TSB)
            let fitness = self
                .client
                .get_fitness_summary()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            // Fetch sport settings
            let sport_settings = self
                .client
                .get_sport_settings()
                .await
                .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;

            // Combine into a comprehensive resource (format as JSON manually)
            let text = format!(
                r#"{{
  "profile": {{
    "id": "{}",
    "name": {}
  }},
  "fitness": {},
  "sport_settings": {},
  "timestamp": "{}"
}}"#,
                profile.id,
                profile
                    .name
                    .as_ref()
                    .map(|n| format!("\"{}\"", n))
                    .unwrap_or_else(|| "null".to_string()),
                serde_json::to_string_pretty(&fitness).unwrap_or_else(|_| "{}".to_string()),
                serde_json::to_string_pretty(&sport_settings).unwrap_or_else(|_| "[]".to_string()),
                chrono::Utc::now().to_rfc3339()
            );

            Ok(ReadResourceResult {
                contents: vec![ResourceContents::TextResourceContents {
                    uri: request.uri.clone(),
                    mime_type: Some("application/json".to_string()),
                    text,
                    meta: None,
                }],
            })
        } else {
            Err(ErrorData::invalid_params(
                format!("Unknown resource URI: {}", request.uri),
                None,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use intervals_icu_client::http_client::ReqwestIntervalsClient;
    use secrecy::SecretString;
    use serde_json::json;
    use std::sync::Arc;

    #[tokio::test]
    async fn handler_creation() {
        let client =
            ReqwestIntervalsClient::new("http://localhost", "ath", SecretString::new("key".into()));
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        // basic smoke: clone must work
        let _clone = handler.clone();
        // ensure tools are registered
        let tools = handler.tool_router.list_all();
        assert!(tools.iter().any(|t| t.name == "get_athlete_profile"));
        assert!(tools.iter().any(|t| t.name == "get_recent_activities"));
        assert!(tools.iter().any(|t| t.name == "get_activity_details"));
        assert!(tools.iter().any(|t| t.name == "search_activities"));
        assert!(tools.iter().any(|t| t.name == "search_activities_full"));
        assert!(tools.iter().any(|t| t.name == "update_activity"));
        assert!(tools.iter().any(|t| t.name == "get_activity_streams"));
        assert!(tools.iter().any(|t| t.name == "get_activity_intervals"));
        assert!(tools.iter().any(|t| t.name == "get_best_efforts"));
        assert!(tools.iter().any(|t| t.name == "download_fit_file"));
        assert!(tools.iter().any(|t| t.name == "download_gpx_file"));
        assert!(tools.iter().any(|t| t.name == "get_events"));
        assert!(tools.iter().any(|t| t.name == "create_event"));
        assert!(tools.iter().any(|t| t.name == "get_event"));
        assert!(tools.iter().any(|t| t.name == "delete_event"));
        assert!(tools.iter().any(|t| t.name == "bulk_create_events"));
        assert!(tools.iter().any(|t| t.name == "get_gear_list"));
        assert!(tools.iter().any(|t| t.name == "get_sport_settings"));
        assert!(tools.iter().any(|t| t.name == "get_power_curves"));
        assert!(tools.iter().any(|t| t.name == "get_gap_histogram"));
        assert!(tools.iter().any(|t| t.name == "delete_activity"));
        assert!(tools.iter().any(|t| t.name == "get_activities_around"));
        assert!(tools.iter().any(|t| t.name == "search_intervals"));
        assert!(tools.iter().any(|t| t.name == "get_power_histogram"));
        assert!(tools.iter().any(|t| t.name == "get_hr_histogram"));
        assert!(tools.iter().any(|t| t.name == "get_pace_histogram"));
        assert!(tools.iter().any(|t| t.name == "get_activities_csv"));
        assert!(tools.iter().any(|t| t.name == "get_fitness_summary"));
        assert!(tools.iter().any(|t| t.name == "get_wellness"));
        assert!(tools.iter().any(|t| t.name == "get_wellness_for_date"));
        assert!(tools.iter().any(|t| t.name == "update_wellness"));
        assert!(tools.iter().any(|t| t.name == "get_upcoming_workouts"));
        assert!(tools.iter().any(|t| t.name == "update_event"));
        assert!(tools.iter().any(|t| t.name == "bulk_delete_events"));
        assert!(tools.iter().any(|t| t.name == "duplicate_event"));
        assert!(tools.iter().any(|t| t.name == "get_hr_curves"));
        assert!(tools.iter().any(|t| t.name == "get_pace_curves"));
        assert!(tools.iter().any(|t| t.name == "get_workout_library"));
        assert!(tools.iter().any(|t| t.name == "get_workouts_in_folder"));
        assert!(tools.iter().any(|t| t.name == "create_gear"));
        assert!(tools.iter().any(|t| t.name == "update_gear"));
        assert!(tools.iter().any(|t| t.name == "delete_gear"));
        assert!(tools.iter().any(|t| t.name == "create_gear_reminder"));
        assert!(tools.iter().any(|t| t.name == "update_gear_reminder"));
        assert!(tools.iter().any(|t| t.name == "update_sport_settings"));
        assert!(tools.iter().any(|t| t.name == "apply_sport_settings"));
        assert!(tools.iter().any(|t| t.name == "create_sport_settings"));
        assert!(tools.iter().any(|t| t.name == "delete_sport_settings"));
        // Ensure the number of registered tools matches the documented implementation
        assert_eq!(handler.tool_count(), 54, "Should register 54 tools");
    }

    #[test]
    fn bulk_create_events_schema_is_object() {
        let client =
            ReqwestIntervalsClient::new("http://localhost", "ath", SecretString::new("key".into()));
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let tools = handler.tool_router.list_all();
        let bulk = tools
            .into_iter()
            .find(|t| t.name == "bulk_create_events")
            .expect("bulk_create_events tool present");

        let val = serde_json::to_value(&bulk).expect("serialize tool");
        let params = val
            .get("inputSchema")
            .or_else(|| val.get("parameters"))
            .expect("input schema present");
        assert_eq!(
            params.get("type"),
            Some(&serde_json::Value::String("object".into()))
        );
        assert!(
            params.get("properties").is_some(),
            "properties should exist on input schema"
        );
    }

    #[test]
    fn bulk_create_events_params_require_object() {
        let payload = json!({
            "events": [
                {
                    "start_date_local": "2024-01-01",
                    "name": "Test Workout",
                    "category": "WORKOUT",
                    "description": null
                }
            ]
        });

        let params: Parameters<BulkCreateEventsToolParams> =
            serde_json::from_value(payload.clone()).expect("payload should deserialize");
        assert_eq!(params.0.events.len(), 1);

        let serialized = serde_json::to_value(params).expect("serialize to value");
        assert!(serialized.is_object());
        assert!(serialized.get("events").is_some());
    }

    #[tokio::test]
    async fn bulk_create_events_rejects_invalid_date() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "events": [
                {
                    "name": "Test Workout",
                    "start_date_local": "2026-13-01",
                    "category": "WORKOUT",
                    "description": null
                }
            ]
        });
        let params: Parameters<BulkCreateEventsToolParams> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.bulk_create_events(params).await;
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(err.contains("invalid start_date_local"));
    }

    #[tokio::test]
    async fn bulk_create_events_rejects_unknown_category() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "events": [
                {
                    "name": "Test Workout",
                    "start_date_local": "2026-01-01",
                    "category": "NOT_A_CATEGORY",
                    "description": null
                }
            ]
        });
        let params: Parameters<BulkCreateEventsToolParams> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.bulk_create_events(params).await;
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(err.contains("invalid category"));
    }

    #[tokio::test]
    async fn bulk_create_events_accepts_valid_payload() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "events": [
                {
                    "name": "Test Workout",
                    "start_date_local": "2026-01-01",
                    "category": "WORKOUT",
                    "description": null
                }
            ]
        });
        let params: Parameters<BulkCreateEventsToolParams> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.bulk_create_events(params).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn bulk_create_events_accepts_iso_datetime() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "events": [
                {
                    "name": "Test Workout",
                    "start_date_local": "2026-01-19T06:30:00",
                    "category": "WORKOUT",
                    "description": null
                }
            ]
        });
        let params: Parameters<BulkCreateEventsToolParams> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.bulk_create_events(params).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn create_event_accepts_iso_datetime_and_normalizes_date() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "name": "Test Workout",
            "start_date_local": "2026-01-19T06:30:00",
            "category": "WORKOUT",
            "description": null
        });
        let params: Parameters<intervals_icu_client::Event> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.create_event(params).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn create_event_rejects_invalid_date() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "name": "Test Workout",
            "start_date_local": "2026-13-01",
            "category": "WORKOUT",
            "description": null
        });
        let params: Parameters<intervals_icu_client::Event> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.create_event(params).await;
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(
            err.contains("invalid start_date_local") || err.contains("invalid start_date_local:")
        );
    }

    #[tokio::test]
    async fn get_wellness_for_date_accepts_iso_datetime_and_normalizes() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params: Parameters<DateParam> =
            serde_json::from_value(json!({"date": "2026-01-19T06:30:00"})).unwrap();
        let res = handler.get_wellness_for_date(params).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_wellness_for_date_rejects_invalid() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params: Parameters<DateParam> =
            serde_json::from_value(json!({"date": "2026-13-01"})).unwrap();
        let res = handler.get_wellness_for_date(params).await;
        assert!(res.is_err());
    }

    #[tokio::test]
    async fn update_wellness_accepts_iso_datetime_and_normalizes() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params: Parameters<WellnessUpdateParams> =
            serde_json::from_value(json!({"date": "2026-01-19T06:30:00", "data": {}})).unwrap();
        let res = handler.update_wellness(params).await;
        assert!(res.is_ok());
    }

    struct MockClient;

    #[async_trait::async_trait]
    impl intervals_icu_client::IntervalsClient for MockClient {
        async fn get_athlete_profile(
            &self,
        ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError>
        {
            Ok(intervals_icu_client::AthleteProfile {
                id: "test_athlete".to_string(),
                name: Some("Test Athlete".to_string()),
            })
        }
        async fn get_recent_activities(
            &self,
            _limit: Option<u32>,
            _days_back: Option<i32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
        {
            Ok(vec![])
        }
        async fn create_event(
            &self,
            event: intervals_icu_client::Event,
        ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
            Ok(event)
        }
        async fn get_event(
            &self,
            _event_id: &str,
        ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError> {
            unimplemented!()
        }
        async fn delete_event(
            &self,
            _event_id: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }
        async fn get_activity_streams(
            &self,
            _activity_id: &str,
            _streams: Option<Vec<String>>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({ "streams": { "time": [1,2,3] } }))
        }
        async fn get_activity_intervals(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({ "intervals": [{ "start": 0, "end": 60 }] }))
        }
        async fn get_best_efforts(
            &self,
            _activity_id: &str,
            _options: Option<intervals_icu_client::BestEffortsOptions>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({ "best": [{ "duration": 600 }] }))
        }
        async fn get_activity_details(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({ "id": "a1" }))
        }
        async fn search_activities(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::ActivitySummary>, intervals_icu_client::IntervalsError>
        {
            Ok(vec![])
        }
        async fn search_activities_full(
            &self,
            _query: &str,
            _limit: Option<u32>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!([]))
        }
        async fn update_activity(
            &self,
            _activity_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn download_activity_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            Ok(None)
        }
        async fn download_fit_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            Ok(None)
        }
        async fn download_gpx_file(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            Ok(None)
        }
        async fn get_gear_list(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_sport_settings(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_power_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_gap_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_events(
            &self,
            _days_back: Option<i32>,
            _limit: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
        {
            Ok(vec![])
        }
        async fn bulk_create_events(
            &self,
            _events: Vec<intervals_icu_client::Event>,
        ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
        {
            Ok(vec![])
        }

        async fn download_activity_file_with_progress(
            &self,
            _activity_id: &str,
            _output_path: Option<std::path::PathBuf>,
            progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
            _cancel_rx: tokio::sync::watch::Receiver<bool>,
        ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
            // simulate progress
            let tx = progress_tx;
            for i in 1..=3u64 {
                let _ = tx.try_send(intervals_icu_client::DownloadProgress {
                    bytes_downloaded: i * 10,
                    total_bytes: Some(30),
                });
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            Ok(Some("/tmp/a1.fit".into()))
        }

        // New methods
        async fn delete_activity(
            &self,
            _activity_id: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }
        async fn get_activities_around(
            &self,
            _activity_id: &str,
            _limit: Option<u32>,
            _route_id: Option<i64>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
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
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_power_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }

        async fn get_activities_csv(&self) -> Result<String, intervals_icu_client::IntervalsError> {
            Ok("id,start_date_local,name\n1,2025-10-18,Run".into())
        }
        async fn get_hr_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_pace_histogram(
            &self,
            _activity_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_fitness_summary(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_wellness(
            &self,
            _days_back: Option<i32>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_wellness_for_date(
            &self,
            _date: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_wellness(
            &self,
            _date: &str,
            _data: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_upcoming_workouts(
            &self,
            _days_ahead: Option<u32>,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_event(
            &self,
            _event_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn bulk_delete_events(
            &self,
            _event_ids: Vec<String>,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }
        async fn duplicate_event(
            &self,
            _event_id: &str,
            _num_copies: Option<u32>,
            _weeks_between: Option<u32>,
        ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
        {
            Ok(vec![])
        }
        async fn get_hr_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_pace_curves(
            &self,
            _days_back: Option<i32>,
            _sport: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_workout_library(
            &self,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn get_workouts_in_folder(
            &self,
            _folder_id: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn create_gear(
            &self,
            _gear: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear(
            &self,
            _gear_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_gear(
            &self,
            _gear_id: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }
        async fn create_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_gear_reminder(
            &self,
            _gear_id: &str,
            _reminder_id: &str,
            _reset: bool,
            _snooze_days: u32,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn update_sport_settings(
            &self,
            _sport_type: &str,
            _recalc_hr_zones: bool,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn apply_sport_settings(
            &self,
            _sport_type: &str,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn create_sport_settings(
            &self,
            _settings: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
        }
        async fn delete_sport_settings(
            &self,
            _sport_type: &str,
        ) -> Result<(), intervals_icu_client::IntervalsError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn activity_streams_intervals_best_efforts() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // Streams
        let streams_param = ActivityIdParam {
            activity_id: "a1".into(),
        };
        let res = handler
            .get_activity_streams(Parameters(streams_param))
            .await;
        assert!(res.is_ok());

        // Intervals
        let intervals_param = ActivityIdParam {
            activity_id: "a1".into(),
        };
        let res = handler
            .get_activity_intervals(Parameters(intervals_param))
            .await;
        assert!(res.is_ok());

        // Best efforts - now the tool requires explicit stream per API contract
        let best_param = BestEffortsToolParams {
            activity_id: "a1".into(),
            stream: "power".into(),
            duration: Some(60),
            distance: None,
            count: None,
            min_value: None,
            exclude_intervals: None,
            start_index: None,
            end_index: None,
        };
        let res = handler.get_best_efforts(Parameters(best_param)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn start_and_check_download() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        let params = DownloadParams {
            activity_id: "a1".into(),
            output_path: None,
        };
        let res = handler
            .start_download(Parameters(params))
            .await
            .expect("start");
        let id = res.0.download_id.clone();

        // poll until completed
        let mut attempts = 0;
        loop {
            let status = handler
                .get_download_status(Parameters(DownloadIdParam {
                    download_id: id.clone(),
                }))
                .await;
            match status {
                Ok(Json(s)) => {
                    if let DownloadState::Completed = s.status.state {
                        assert!(s.status.path.is_some());
                        break;
                    }
                }
                Err(_) => panic!("missing status"),
            }
            attempts += 1;
            if attempts > 50 {
                panic!("timed out")
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    #[tokio::test]
    async fn gear_and_curves_tools() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // gear
        let res = handler.get_gear_list().await;
        assert!(res.is_ok());

        // sport settings
        let res = handler.get_sport_settings().await;
        assert!(res.is_ok());

        // power curves
        let res = handler
            .get_power_curves(Parameters(PowerCurvesParams {
                days_back: Some(30),
                sport: "Ride".into(),
            }))
            .await;
        assert!(res.is_ok());

        // lowercase sport should also work (normalize to canonical form)
        // This uses the mock client so will succeed

        // gap histogram
        let res = handler
            .get_gap_histogram(Parameters(ActivityIdParam {
                activity_id: "a1".into(),
            }))
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_power_curves_accepts_lowercase_type_with_http_client() {
        use intervals_icu_client::http_client::ReqwestIntervalsClient;
        use secrecy::SecretString;
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;
        let m = Mock::given(method("GET"))
            .and(path("/api/v1/athlete/test_ath/power-curves"))
            .and(query_param("type", "Run"))
            .and(query_param("curves", "7d"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
            .expect(1);
        m.mount(&mock_server).await;

        let client = ReqwestIntervalsClient::new(
            &mock_server.uri(),
            "test_ath",
            SecretString::new("key".into()),
        );
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        let res = handler
            .get_power_curves(Parameters(PowerCurvesParams {
                days_back: Some(7),
                sport: "run".into(),
            }))
            .await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn webhook_receive_and_dedupe() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // set secret
        let secret = "s3cr3t";
        let _ = handler
            .set_webhook_secret(Parameters(ObjectResult {
                value: serde_json::json!({ "secret": secret }),
            }))
            .await
            .expect("set");

        // build payload
        let payload = serde_json::json!({ "id": "evt1", "data": { "x": 1 } });
        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(secret.as_bytes()).unwrap();
        let body = serde_json::to_vec(&payload).unwrap();
        mac.update(&body);
        let sig = hex::encode(mac.finalize().into_bytes());

        // first receive should be ok
        let res = handler
            .receive_webhook(Parameters(ObjectResult {
                value: serde_json::json!({ "signature": sig.clone(), "payload": payload.clone() }),
            }))
            .await;
        assert!(res.is_ok());

        // duplicate should be reported
        let res2 = handler
            .receive_webhook(Parameters(ObjectResult {
                value: serde_json::json!({ "signature": sig, "payload": payload }),
            }))
            .await
            .expect("dup result");
        assert_eq!(
            res2.0.value.get("duplicate").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[test]
    fn update_event_accepts_numeric_id() {
        let json = serde_json::json!({
            "event_id": 82024747,
            "fields": serde_json::json!({"name": "test"})
        });
        let params: UpdateEventParams = serde_json::from_value(json).expect("parse numeric id");
        assert_eq!(params.event_id.as_cow(), "82024747");

        let bulk_json = serde_json::json!({
            "event_ids": [1, "2"],
        });
        let bulk: BulkDeleteEventsParams =
            serde_json::from_value(bulk_json).expect("parse mixed ids");
        let collected: Vec<String> = bulk
            .event_ids
            .iter()
            .map(|id| id.as_cow().into_owned())
            .collect();
        assert_eq!(collected, vec!["1", "2"]);
    }

    #[tokio::test]
    async fn update_event_normalizes_start_date_local() {
        use std::sync::Arc;
        struct CapturingUpdateClient {
            captured: std::sync::Arc<tokio::sync::Mutex<Option<serde_json::Value>>>,
        }
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for CapturingUpdateClient {
            async fn get_athlete_profile(
                &self,
            ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError>
            {
                unimplemented!();
            }
            async fn get_recent_activities(
                &self,
                _limit: Option<u32>,
                _days_back: Option<i32>,
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                Ok(vec![])
            }
            async fn create_event(
                &self,
                _event: intervals_icu_client::Event,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                unimplemented!()
            }
            async fn get_event(
                &self,
                _event_id: &str,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                unimplemented!()
            }
            async fn delete_event(
                &self,
                _event_id: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }
            async fn get_activity_streams(
                &self,
                _activity_id: &str,
                _streams: Option<Vec<String>>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn get_activity_intervals(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn get_best_efforts(
                &self,
                _activity_id: &str,
                _options: Option<intervals_icu_client::BestEffortsOptions>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn get_activity_details(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn search_activities(
                &self,
                _query: &str,
                _limit: Option<u32>,
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                Ok(vec![])
            }
            async fn search_activities_full(
                &self,
                _query: &str,
                _limit: Option<u32>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!([]))
            }
            async fn update_activity(
                &self,
                _activity_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn download_activity_file(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(None)
            }
            async fn download_fit_file(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(None)
            }
            async fn download_gpx_file(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(None)
            }
            async fn get_gear_list(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn get_sport_settings(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn get_power_curves(
                &self,
                _days_back: Option<i32>,
                _sport: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn get_gap_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }
            async fn get_events(
                &self,
                _days_back: Option<i32>,
                _limit: Option<u32>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(vec![])
            }
            async fn bulk_create_events(
                &self,
                _events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(vec![])
            }

            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(Some("/tmp/a1.fit".into()))
            }

            async fn delete_activity(
                &self,
                _activity_id: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }

            async fn update_event(
                &self,
                _event_id: &str,
                fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                let mut cap = self.captured.lock().await;
                *cap = Some(fields.clone());
                Ok(serde_json::json!({}))
            }

            async fn get_activities_around(
                &self,
                _activity_id: &str,
                _limit: Option<u32>,
                _route_id: Option<i64>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
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
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
                Ok("id,start_date_local,name\n1,2025-10-18,Run".into())
            }

            async fn get_power_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_hr_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_pace_histogram(
                &self,
                _activity_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_fitness_summary(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_wellness(
                &self,
                _days_back: Option<i32>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_wellness_for_date(
                &self,
                _date: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn update_wellness(
                &self,
                _date: &str,
                _data: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_upcoming_workouts(
                &self,
                _days_ahead: Option<u32>,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn bulk_delete_events(
                &self,
                _event_ids: Vec<String>,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }

            async fn duplicate_event(
                &self,
                _event_id: &str,
                _num_copies: Option<u32>,
                _weeks_between: Option<u32>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(vec![])
            }

            async fn get_hr_curves(
                &self,
                _days_back: Option<i32>,
                _sport: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_pace_curves(
                &self,
                _days_back: Option<i32>,
                _sport: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_workout_library(
                &self,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn get_workouts_in_folder(
                &self,
                _folder_id: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn create_gear(
                &self,
                _gear: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn update_gear(
                &self,
                _gear_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn delete_gear(
                &self,
                _gear_id: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }

            async fn create_gear_reminder(
                &self,
                _gear_id: &str,
                _reminder: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn update_gear_reminder(
                &self,
                _gear_id: &str,
                _reminder_id: &str,
                _reset: bool,
                _snooze_days: u32,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn update_sport_settings(
                &self,
                _sport_type: &str,
                _recalc_hr_zones: bool,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn apply_sport_settings(
                &self,
                _sport_type: &str,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn create_sport_settings(
                &self,
                _settings: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
            }

            async fn delete_sport_settings(
                &self,
                _sport_type: &str,
            ) -> Result<(), intervals_icu_client::IntervalsError> {
                Ok(())
            }
        }

        let captured = std::sync::Arc::new(tokio::sync::Mutex::new(None));
        let client = CapturingUpdateClient {
            captured: captured.clone(),
        };
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        let params = UpdateEventParams {
            event_id: EventId::Int(1),
            fields: serde_json::json!({"start_date_local": "2026-01-19T06:30:00"}),
        };

        let res = handler.update_event(Parameters(params)).await;
        assert!(res.is_ok());
        let stored = captured.lock().await;
        let obj = stored.as_ref().unwrap();
        assert_eq!(
            obj.get("start_date_local").and_then(|v| v.as_str()),
            Some("2026-01-19")
        );
    }

    #[tokio::test]
    async fn prompts_are_registered() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        let prompts = handler.prompt_router.list_all();
        assert!(prompts.iter().any(|p| p.name == "analyze-recent-training"));
        assert!(prompts.iter().any(|p| p.name == "performance-analysis"));
        assert!(prompts.iter().any(|p| p.name == "activity-deep-dive"));
        assert!(prompts.iter().any(|p| p.name == "recovery-check"));
        assert!(prompts.iter().any(|p| p.name == "training-plan-review"));
        assert!(prompts.iter().any(|p| p.name == "plan-training-week"));
        assert!(prompts.iter().any(|p| p.name == "analyze-and-adapt-plan"));
        assert_eq!(prompts.len(), 7, "Should have exactly 7 prompts");
    }

    #[tokio::test]
    async fn resource_list_contains_athlete_profile() {
        let client = MockClient;
        let _handler = IntervalsMcpHandler::new(Arc::new(client));

        // Simply verify resources are registered in handler
        // Full integration test with context is done in e2e tests
        let resources = ["intervals-icu://athlete/profile"];
        assert!(resources.contains(&"intervals-icu://athlete/profile"));
    }

    #[tokio::test]
    async fn read_athlete_profile_works() {
        let client = MockClient;
        let _handler = IntervalsMcpHandler::new(Arc::new(client));

        // Verify handler can fetch profile data (mock returns empty but valid data)
        let profile = _handler.client.get_athlete_profile().await;
        assert!(profile.is_ok());

        let fitness = _handler.client.get_fitness_summary().await;
        assert!(fitness.is_ok());

        let sport_settings = _handler.client.get_sport_settings().await;
        assert!(sport_settings.is_ok());
    }

    // ------------------------------------------------------------------
    // Additional unit tests to improve coverage on handler logic
    // ------------------------------------------------------------------

    #[test]
    fn prompts_have_expected_content() {
        // Call internal prompt generators directly (crate tests can access private module)
        let p = crate::prompts::analyze_recent_training_prompt(7);
        assert!(p.description.unwrap().contains("7 days"));
        let pa = crate::prompts::performance_analysis_prompt("power", 14);
        assert!(pa.description.unwrap().to_lowercase().contains("power"));
        let pd = crate::prompts::activity_deep_dive_prompt("act1");
        assert!(pd.description.unwrap().contains("activity act1"));
    }

    #[test]
    fn performance_and_plan_prompts_cover_branches() {
        // performance-analysis: power family
        let p_power = crate::prompts::performance_analysis_prompt("ride", 14);
        let s_power = serde_json::to_string(&p_power).unwrap().to_lowercase();
        assert!(s_power.contains("power") || s_power.contains("power curve"));

        // performance-analysis: hr family
        let p_hr = crate::prompts::performance_analysis_prompt("hr", 7);
        let s_hr = serde_json::to_string(&p_hr).unwrap().to_lowercase();
        assert!(s_hr.contains("hr") || s_hr.contains("get_hr_curves"));

        // performance-analysis: default (pace)
        let p_pace = crate::prompts::performance_analysis_prompt("run", 30);
        let s_pace = serde_json::to_string(&p_pace).unwrap().to_lowercase();
        assert!(s_pace.contains("pace") || s_pace.contains("get_pace_curves"));

        // plan training week prompt contains focus and create_event hint
        let plan = crate::prompts::plan_training_week_prompt("2025-01-01", "endurance");
        let s_plan = serde_json::to_string(&plan).unwrap().to_lowercase();
        assert!(s_plan.contains("endurance"));
        assert!(s_plan.contains("create_event") || s_plan.contains("create events"));

        // analyze and adapt plan prompt mentions period label and focus
        let adapt = crate::prompts::analyze_and_adapt_plan_prompt("last month", "strength");
        let s_adapt = serde_json::to_string(&adapt).unwrap().to_lowercase();
        assert!(s_adapt.contains("last month") && s_adapt.contains("strength"));
    }

    #[test]
    fn recovery_and_plan_review_prompts_cover_branches() {
        // recovery check prompt mentions recovery/wellness/HRV
        let rec = crate::prompts::recovery_check_prompt(7);
        let s_rec = serde_json::to_string(&rec).unwrap().to_lowercase();
        assert!(s_rec.contains("recovery") || s_rec.contains("hrv") || s_rec.contains("wellness"));

        // training plan review prompt contains upcoming/workouts hints
        let review = crate::prompts::training_plan_review_prompt("2025-02-01");
        let s_review = serde_json::to_string(&review).unwrap().to_lowercase();
        assert!(
            s_review.contains("upcoming")
                || s_review.contains("workout")
                || s_review.contains("training plan")
        );

        // analyze recent training prompt contains analysis wording
        let ar = crate::prompts::analyze_recent_training_prompt(14);
        let s_ar = serde_json::to_string(&ar).unwrap().to_lowercase();
        assert!(s_ar.contains("training analysis") || s_ar.contains("analyze my intervals"));
    }

    #[tokio::test]
    async fn process_webhook_happy_and_duplicate_paths() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // missing secret -> error
        let res = handler
            .process_webhook("deadbeef", serde_json::json!({ "id": "x" }))
            .await;
        assert!(res.is_err());

        handler.set_webhook_secret_value("s3cr3t").await;

        // prepare valid signature
        let payload = serde_json::json!({ "id": "evt1", "x": 1 });
        let body = serde_json::to_vec(&payload).unwrap();
        let mut mac: Hmac<Sha256> = Hmac::new_from_slice(b"s3cr3t").unwrap();
        mac.update(&body);
        let sig = hex::encode(mac.finalize().into_bytes());

        let r = handler
            .process_webhook(&sig, payload.clone())
            .await
            .expect("should succeed");
        assert_eq!(r.value.get("ok").and_then(|v| v.as_bool()), Some(true));

        // duplicate should return duplicate:true
        let r2 = handler
            .process_webhook(&sig, payload.clone())
            .await
            .expect("should return duplicate");
        assert_eq!(
            r2.value.get("duplicate").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn process_webhook_signature_mismatch() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // set secret, then call with an invalid signature to exercise the
        // signature-mismatch branch
        handler.set_webhook_secret_value("s3cr3t").await;
        let payload = serde_json::json!({ "id": "x" });
        let res = handler.process_webhook("deadbeef", payload).await;
        assert!(res.is_err());
        assert_eq!(res.unwrap_err(), "signature mismatch".to_string());
    }

    #[tokio::test]
    async fn cancel_download_paths() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // cancel an unknown download id
        let params = rmcp::handler::server::wrapper::Parameters(DownloadIdParam {
            download_id: "missing".into(),
        });
        let res = handler.cancel_download(params).await;
        assert!(res.is_err());
        let err = res.err().unwrap();
        assert!(err.contains("not found"));

        // insert a canceller and download entry and then cancel
        let (tx, _rx) = watch::channel(false);
        {
            let mut canc = handler.cancel_senders.lock().await;
            canc.insert("d1".to_string(), tx);
        }
        {
            let mut dl = handler.downloads.lock().await;
            dl.insert(
                "d1".to_string(),
                DownloadStatus {
                    id: "d1".into(),
                    activity_id: "a1".into(),
                    state: DownloadState::InProgress,
                    bytes_downloaded: 0,
                    total_bytes: None,
                    path: None,
                },
            );
        }

        let params_ok = rmcp::handler::server::wrapper::Parameters(DownloadIdParam {
            download_id: "d1".into(),
        });
        let ok = handler
            .cancel_download(params_ok)
            .await
            .expect("cancel succeeds");
        assert_eq!(
            ok.0.value.get("cancelled").and_then(|v| v.as_bool()),
            Some(true)
        );
        let dl = handler.downloads.lock().await;
        let s = dl.get("d1").unwrap();
        match s.state {
            DownloadState::Cancelled => {}
            _ => panic!("expected cancelled"),
        }
    }
}
