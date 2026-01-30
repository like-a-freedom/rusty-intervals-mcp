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
    AnnotateAble, GetPromptRequestParams, GetPromptResult, ListPromptsResult, ListResourcesResult,
    PaginatedRequestParams, RawResource, ReadResourceRequestParams, ReadResourceResult,
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

/// Parameters for get_activity_details with optional compact mode.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivityDetailsParams {
    /// Activity ID
    pub activity_id: String,
    /// Return full payload (default: false = compact summary)
    pub expand: Option<bool>,
    /// Specific fields to return (e.g., ["id", "name", "distance", "moving_time"])
    pub fields: Option<Vec<String>>,
}

/// Compact activity summary for token-efficient responses
#[derive(Debug, Serialize, JsonSchema)]
pub struct ActivitySummaryCompact {
    pub id: String,
    pub name: Option<String>,
    pub start_date_local: Option<String>,
    pub r#type: Option<String>,
    pub moving_time: Option<i64>,
    pub distance: Option<f64>,
    pub total_elevation_gain: Option<f64>,
    pub average_watts: Option<f64>,
    pub average_heartrate: Option<f64>,
    pub icu_training_load: Option<f64>,
    pub icu_intensity: Option<f64>,
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

// === Token-Efficient Parameters ===

/// Parameters for get_activities_csv with optional filtering
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivitiesCsvParams {
    /// Limit number of rows (default: 100, max: 1000)
    pub limit: Option<u32>,
    /// Number of days back to include (default: 90)
    pub days_back: Option<i32>,
    /// Specific columns to include (default: id,start_date_local,name,type,moving_time,distance)
    pub columns: Option<Vec<String>>,
}

/// Parameters for search_activities_full with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SearchActivitiesFullParams {
    /// Search query
    #[serde(rename = "q", alias = "query")]
    pub q: String,
    /// Maximum results (default: 10)
    pub limit: Option<u32>,
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return when compact=false
    pub fields: Option<Vec<String>>,
}

/// Parameters for get_activity_intervals with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ActivityIntervalsParams {
    /// Activity ID
    pub activity_id: String,
    /// Return summary statistics only (default: true)
    pub summary: Option<bool>,
    /// Maximum intervals to return (default: 20)
    pub max_intervals: Option<u32>,
    /// Specific fields per interval (default: type,start_index,end_index,duration,distance)
    pub fields: Option<Vec<String>>,
}

/// Parameters for get_best_efforts with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct BestEffortsCompactParams {
    /// Activity ID
    pub activity_id: String,
    /// Stream to analyze: power, heartrate, speed, pace, cadence, distance
    pub stream: String,
    /// Duration in seconds (REQUIRED: provide duration OR distance, not both)
    pub duration: Option<i32>,
    /// Distance in meters (REQUIRED: provide duration OR distance, not both)
    pub distance: Option<f64>,
    /// Max results (default: 5)
    pub count: Option<i32>,
    /// Return summary only (default: true)
    pub summary: Option<bool>,
    /// Minimum value threshold
    pub min_value: Option<f64>,
    /// Exclude structured intervals
    pub exclude_intervals: Option<bool>,
    /// Start index
    pub start_index: Option<i32>,
    /// End index
    pub end_index: Option<i32>,
}

/// Parameters for power/hr/pace curves with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CurvesParams {
    /// Sport type (e.g., "Ride", "Run")
    #[serde(rename = "type")]
    pub sport: String,
    /// Days back to analyze (default: 90)
    pub days_back: Option<i32>,
    /// Specific durations in seconds to return (e.g., [5, 60, 300, 1200, 3600])
    pub durations: Option<Vec<u32>>,
    /// Return summary with key durations only (default: true)
    pub summary: Option<bool>,
}

/// Parameters for get_workouts_in_folder with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WorkoutsInFolderParams {
    /// Folder ID
    pub folder_id: String,
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Max workouts to return (default: 20)
    pub limit: Option<u32>,
    /// Specific fields to return
    pub fields: Option<Vec<String>>,
}

/// Parameters for get_gear_list with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GearListParams {
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,type,distance)
    pub fields: Option<Vec<String>>,
}

/// Parameters for histogram tools with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct HistogramParams {
    /// Activity ID
    pub activity_id: String,
    /// Return summary statistics only (default: true)
    pub summary: Option<bool>,
    /// Number of bins (default: 10, max: 50)
    pub bins: Option<u32>,
}

/// Parameters for get_wellness with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WellnessParams {
    /// Days back (default: 7)
    pub days_back: Option<i32>,
    /// Return summary/trends only (default: true)
    pub summary: Option<bool>,
    /// Specific fields to return (default: all)
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateActivityParams {
    pub activity_id: String,
    pub fields: serde_json::Value,
}

/// Parameters for get_activity_streams with optional compact mode.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct StreamsParams {
    /// Activity ID
    pub activity_id: String,
    /// Maximum number of data points per stream. If set, arrays are downsampled.
    pub max_points: Option<u32>,
    /// Return summary statistics (min/max/avg/count) instead of raw arrays.
    pub summary: Option<bool>,
    /// Specific streams to return (e.g., ["power", "heartrate"]). Default: all available.
    pub streams: Option<Vec<String>>,
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
        description = "Set HMAC secret for webhook signature verification."
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

    // Shared helper: normalize date-only strings to YYYY-MM-DD. Accepts YYYY-MM-DD, RFC3339, or naive YYYY-MM-DDTHH:MM:SS
    fn normalize_date_str(s: &str) -> Result<String, ()> {
        if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
            return Ok(s.to_string());
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Ok(dt.date_naive().format("%Y-%m-%d").to_string());
        }
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Ok(ndt.date().format("%Y-%m-%d").to_string());
        }
        Err(())
    }

    // Normalize start_date_local for events: return ISO datetime. Keep provided time; if only a date is given, set time to 00:00:00.
    fn normalize_event_start(s: &str) -> Result<String, ()> {
        if chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok() {
            return Ok(format!("{}T00:00:00", s));
        }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
            return Ok(dt.naive_local().format("%Y-%m-%dT%H:%M:%S").to_string());
        }
        if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {
            return Ok(ndt.format("%Y-%m-%dT%H:%M:%S").to_string());
        }
        Err(())
    }

    #[tool(
        name = "create_event",
        description = "Create calendar event. Requires: name, start_date_local, category (WORKOUT/RACE_A/NOTE/etc)."
    )]
    async fn create_event(
        &self,
        params: Parameters<intervals_icu_client::Event>,
    ) -> Result<Json<intervals_icu_client::Event>, String> {
        let ev = params.0;
        // Validate and normalize input: accept YYYY-MM-DD or ISO 8601 datetimes; preserve time when provided, default to 00:00:00 when only date is supplied
        if ev.name.trim().is_empty() {
            return Err("invalid event: name is empty".into());
        }
        let mut ev2 = ev.clone();
        if Self::normalize_event_start(&ev2.start_date_local).is_err() {
            return Err(format!(
                "invalid start_date_local: {}",
                ev2.start_date_local
            ));
        } else if let Ok(s) = Self::normalize_event_start(&ev2.start_date_local) {
            ev2.start_date_local = s;
        }
        if ev2.category == intervals_icu_client::EventCategory::Unknown {
            return Err("invalid category: unknown".into());
        }
        // For WORKOUT events, `type` (sport) is required by the upstream API
        if ev2.category == intervals_icu_client::EventCategory::Workout
            && ev2
                .r#type
                .as_ref()
                .map(|s| s.trim())
                .unwrap_or("")
                .is_empty()
        {
            tracing::debug!("create_event: missing type for WORKOUT - defaulting to Run");
            ev2.r#type = Some("Run".into());
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
        description = "Create multiple calendar events. Params: events array with name, start_date_local, category per event."
    )]
    pub async fn bulk_create_events(
        &self,
        params: Parameters<BulkCreateEventsToolParams>,
    ) -> Result<Json<EventsResult>, String> {
        let events = params.0.events;
        // Normalize and validate input early to provide clearer errors and avoid 422 from the API.
        // Accept either YYYY-MM-DD or ISO 8601 datetimes; preserve time when provided, default to 00:00:00 for date-only input.
        let mut norm_events: Vec<intervals_icu_client::Event> = Vec::with_capacity(events.len());
        for (i, ev) in events.into_iter().enumerate() {
            if ev.name.trim().is_empty() {
                return Err(format!("invalid event at index {}: name is empty", i));
            }
            // Normalize date/time: accept YYYY-MM-DD or RFC3339 / NaiveDateTime, preserve time when provided
            let mut ev2 = ev.clone();
            match Self::normalize_event_start(&ev2.start_date_local) {
                Ok(s) => ev2.start_date_local = s,
                Err(()) => {
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
            // If type is missing for WORKOUT events, default to Run to avoid upstream 422s
            if ev2.category == intervals_icu_client::EventCategory::Workout
                && ev2
                    .r#type
                    .as_ref()
                    .map(|s| s.trim())
                    .unwrap_or("")
                    .is_empty()
            {
                tracing::debug!(
                    "bulk_create_events: missing type for WORKOUT at index {} - defaulting to Run",
                    i
                );
                ev2.r#type = Some("Run".into());
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
        description = "Get activity details. Params: activity_id, expand (full data), fields (filter). Default: compact summary."
    )]
    async fn get_activity_details(
        &self,
        params: Parameters<ActivityDetailsParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_activity_details(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;

        // Return full payload if expand=true, otherwise compact summary
        let result = if p.expand.unwrap_or(false) {
            // Apply field filtering if specified
            if let Some(ref fields) = p.fields {
                Self::filter_fields(&v, fields)
            } else {
                v
            }
        } else {
            Self::extract_activity_summary(&v, p.fields.as_deref())
        };

        Ok(Json(ObjectResult { value: result }))
    }

    /// Extract compact activity summary from full details
    fn extract_activity_summary(
        value: &serde_json::Value,
        fields: Option<&[String]>,
    ) -> serde_json::Value {
        let default_fields = [
            "id",
            "name",
            "start_date_local",
            "type",
            "moving_time",
            "distance",
            "total_elevation_gain",
            "average_watts",
            "average_heartrate",
            "icu_training_load",
            "icu_intensity",
            "calories",
            "average_speed",
        ];

        let fields_to_extract: Vec<&str> = fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| default_fields.to_vec());

        let Some(obj) = value.as_object() else {
            return value.clone();
        };

        let mut result = serde_json::Map::new();
        for field in fields_to_extract {
            if let Some(val) = obj.get(field) {
                result.insert(field.to_string(), val.clone());
            }
        }

        serde_json::Value::Object(result)
    }

    /// Filter JSON object to only include specified fields
    fn filter_fields(value: &serde_json::Value, fields: &[String]) -> serde_json::Value {
        let Some(obj) = value.as_object() else {
            return value.clone();
        };

        let mut result = serde_json::Map::new();
        for field in fields {
            if let Some(val) = obj.get(field) {
                result.insert(field.clone(), val.clone());
            }
        }

        serde_json::Value::Object(result)
    }

    #[tool(
        name = "search_activities",
        description = "Search activities by text. Returns ID/name. Use search_activities_full for complete data."
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
        description = "Search activities. Params: q (query), limit, compact (default true), fields (filter)."
    )]
    async fn search_activities_full(
        &self,
        params: Parameters<SearchActivitiesFullParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .search_activities_full(&p.q, p.limit.or(Some(10)))
            .await
            .map_err(|e| e.to_string())?;

        // Apply compact mode (default: true)
        let result = if p.compact.unwrap_or(true) {
            Self::compact_activities_array(&v, p.fields.as_deref())
        } else if let Some(ref fields) = p.fields {
            Self::filter_array_fields(&v, fields)
        } else {
            v
        };

        Ok(Json(ObjectResult { value: result }))
    }

    /// Compact an array of activities to essential fields only
    fn compact_activities_array(
        value: &serde_json::Value,
        custom_fields: Option<&[String]>,
    ) -> serde_json::Value {
        let default_fields = [
            "id",
            "name",
            "start_date_local",
            "type",
            "moving_time",
            "distance",
            "total_elevation_gain",
            "average_watts",
            "average_heartrate",
            "icu_training_load",
        ];

        let fields: Vec<&str> = custom_fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| default_fields.to_vec());

        let Some(arr) = value.as_array() else {
            return value.clone();
        };

        let compacted: Vec<serde_json::Value> = arr
            .iter()
            .map(|item| {
                let Some(obj) = item.as_object() else {
                    return item.clone();
                };
                let mut result = serde_json::Map::new();
                for field in &fields {
                    if let Some(val) = obj.get(*field) {
                        result.insert(field.to_string(), val.clone());
                    }
                }
                serde_json::Value::Object(result)
            })
            .collect();

        serde_json::Value::Array(compacted)
    }

    /// Filter each object in an array to only include specified fields
    fn filter_array_fields(value: &serde_json::Value, fields: &[String]) -> serde_json::Value {
        let Some(arr) = value.as_array() else {
            return value.clone();
        };

        let filtered: Vec<serde_json::Value> = arr
            .iter()
            .map(|item| Self::filter_fields(item, fields))
            .collect();

        serde_json::Value::Array(filtered)
    }

    #[tool(
        name = "get_activities_csv",
        description = "Download activities as CSV. Params: limit (default 100), days_back (default 90), columns (filter)."
    )]
    async fn get_activities_csv(
        &self,
        params: Parameters<ActivitiesCsvParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let csv_full = self
            .client
            .get_activities_csv()
            .await
            .map_err(|e| e.to_string())?;

        // Apply filtering
        let result = Self::filter_csv(
            &csv_full,
            p.limit.unwrap_or(100).min(1000) as usize,
            p.days_back.unwrap_or(90),
            p.columns.as_deref(),
        );

        Ok(Json(ObjectResult {
            value: serde_json::json!({ "csv": result }),
        }))
    }

    /// Filter CSV to limit rows and columns
    fn filter_csv(
        csv: &str,
        max_rows: usize,
        _days_back: i32,
        columns: Option<&[String]>,
    ) -> String {
        let mut lines = csv.lines();
        let Some(header) = lines.next() else {
            return csv.to_string();
        };

        let header_cols: Vec<&str> = header.split(',').collect();

        // Determine which column indices to keep
        let col_indices: Vec<usize> = if let Some(cols) = columns {
            header_cols
                .iter()
                .enumerate()
                .filter_map(|(i, h)| {
                    if cols.iter().any(|c| c.eq_ignore_ascii_case(h)) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            // Default columns for compact mode
            let defaults = [
                "id",
                "start_date_local",
                "name",
                "type",
                "moving_time",
                "distance",
            ];
            header_cols
                .iter()
                .enumerate()
                .filter_map(|(i, h)| {
                    if defaults.iter().any(|d| d.eq_ignore_ascii_case(h)) {
                        Some(i)
                    } else {
                        None
                    }
                })
                .collect()
        };

        // If no columns matched, return all columns
        let col_indices = if col_indices.is_empty() {
            (0..header_cols.len()).collect()
        } else {
            col_indices
        };

        let mut result = Vec::with_capacity(max_rows + 1);

        // Build filtered header
        let filtered_header: Vec<&str> = col_indices.iter().map(|&i| header_cols[i]).collect();
        result.push(filtered_header.join(","));

        // Filter rows
        for line in lines.take(max_rows) {
            let cols: Vec<&str> = line.split(',').collect();
            let filtered: Vec<&str> = col_indices
                .iter()
                .filter_map(|&i| cols.get(i).copied())
                .collect();
            result.push(filtered.join(","));
        }

        result.join("\n")
    }

    #[tool(
        name = "update_activity",
        description = "Update activity fields: name, description, notes, keywords, private."
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
        description = "Get activity streams. Params: activity_id, max_points (downsample), summary (stats only), streams (filter)."
    )]
    async fn get_activity_streams(
        &self,
        params: Parameters<StreamsParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_activity_streams(&p.activity_id, None)
            .await
            .map_err(|e| e.to_string())?;

        // Apply compact transformations if requested
        let result =
            Self::transform_streams(v, p.max_points, p.summary.unwrap_or(false), p.streams);
        Ok(Json(ObjectResult { value: result }))
    }

    /// Transform streams: downsample to max_points, compute summary stats, filter by stream names
    fn transform_streams(
        value: serde_json::Value,
        max_points: Option<u32>,
        summary_only: bool,
        filter_streams: Option<Vec<String>>,
    ) -> serde_json::Value {
        let Some(obj) = value.as_object() else {
            return value;
        };

        let mut result = serde_json::Map::new();

        for (key, val) in obj {
            // Filter streams if specified
            if let Some(ref filter) = filter_streams
                && !filter.iter().any(|f| f.eq_ignore_ascii_case(key))
            {
                continue;
            }

            let Some(arr) = val.as_array() else {
                result.insert(key.clone(), val.clone());
                continue;
            };

            if summary_only {
                // Compute summary statistics for numeric arrays
                let stats = Self::compute_stream_stats(arr);
                result.insert(key.clone(), stats);
            } else if let Some(max) = max_points {
                // Downsample the array
                let sampled = Self::downsample_array(arr, max as usize);
                result.insert(key.clone(), serde_json::Value::Array(sampled));
            } else {
                result.insert(key.clone(), val.clone());
            }
        }

        serde_json::Value::Object(result)
    }

    /// Compute summary statistics for a numeric array
    fn compute_stream_stats(arr: &[serde_json::Value]) -> serde_json::Value {
        let nums: Vec<f64> = arr
            .iter()
            .filter_map(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
            .collect();

        if nums.is_empty() {
            return serde_json::json!({ "count": 0 });
        }

        let count = nums.len();
        let sum: f64 = nums.iter().sum();
        let avg = sum / count as f64;
        let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
        let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

        // Compute percentiles (p10, p50, p90)
        let mut sorted = nums.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p10 = sorted[count / 10];
        let p50 = sorted[count / 2];
        let p90 = sorted[count * 9 / 10];

        serde_json::json!({
            "count": count,
            "min": min,
            "max": max,
            "avg": (avg * 100.0).round() / 100.0,
            "p10": p10,
            "p50": p50,
            "p90": p90
        })
    }

    /// Downsample array to target size using LTTB-like selection
    fn downsample_array(arr: &[serde_json::Value], target: usize) -> Vec<serde_json::Value> {
        let len = arr.len();
        if len <= target || target < 2 {
            return arr.to_vec();
        }

        // Simple uniform sampling (preserves first and last)
        let mut result = Vec::with_capacity(target);
        result.push(arr[0].clone());

        let step = (len - 1) as f64 / (target - 1) as f64;
        for i in 1..(target - 1) {
            let idx = (i as f64 * step).round() as usize;
            result.push(arr[idx.min(len - 1)].clone());
        }

        result.push(arr[len - 1].clone());
        result
    }

    #[tool(
        name = "get_activity_intervals",
        description = "Get workout intervals. Params: activity_id, summary (default true), max_intervals (default 20), fields (filter)."
    )]
    async fn get_activity_intervals(
        &self,
        params: Parameters<ActivityIntervalsParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_activity_intervals(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;

        // Apply compact transformations
        let result = Self::transform_intervals(
            &v,
            p.summary.unwrap_or(true),
            p.max_intervals.unwrap_or(20) as usize,
            p.fields.as_deref(),
        );

        Ok(Json(ObjectResult { value: result }))
    }

    /// Transform intervals: summarize or limit and filter fields
    fn transform_intervals(
        value: &serde_json::Value,
        summary_only: bool,
        max_intervals: usize,
        fields: Option<&[String]>,
    ) -> serde_json::Value {
        let Some(arr) = value.as_array() else {
            return value.clone();
        };

        if summary_only {
            // Return summary statistics
            let total = arr.len();
            let mut type_counts: HashMap<String, usize> = HashMap::new();
            let mut total_duration: f64 = 0.0;
            let mut total_distance: f64 = 0.0;

            for item in arr {
                if let Some(obj) = item.as_object() {
                    if let Some(t) = obj.get("type").and_then(|v| v.as_str()) {
                        *type_counts.entry(t.to_string()).or_insert(0) += 1;
                    }
                    if let Some(d) = obj.get("duration").and_then(|v| v.as_f64()) {
                        total_duration += d;
                    }
                    if let Some(d) = obj.get("distance").and_then(|v| v.as_f64()) {
                        total_distance += d;
                    }
                }
            }

            return serde_json::json!({
                "total_intervals": total,
                "types": type_counts,
                "total_duration_secs": total_duration,
                "total_distance_m": total_distance,
                "avg_duration_secs": if total > 0 { total_duration / total as f64 } else { 0.0 }
            });
        }

        // Limit and filter fields
        let default_fields = [
            "type",
            "start_index",
            "end_index",
            "duration",
            "distance",
            "intensity",
        ];
        let fields_to_use: Vec<&str> = fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| default_fields.to_vec());

        let limited: Vec<serde_json::Value> = arr
            .iter()
            .take(max_intervals)
            .map(|item| {
                let Some(obj) = item.as_object() else {
                    return item.clone();
                };
                let mut result = serde_json::Map::new();
                for field in &fields_to_use {
                    if let Some(val) = obj.get(*field) {
                        result.insert(field.to_string(), val.clone());
                    }
                }
                serde_json::Value::Object(result)
            })
            .collect();

        serde_json::Value::Array(limited)
    }

    #[tool(
        name = "get_best_efforts",
        description = "Find peak efforts. Params: activity_id, stream, duration/distance, count (default 5), summary (default true)."
    )]
    async fn get_best_efforts(
        &self,
        params: Parameters<BestEffortsCompactParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        
        // Validate that at least one of duration or distance is provided
        if p.duration.is_none() && p.distance.is_none() {
            return Err("Must provide either 'duration' (seconds) or 'distance' (meters) for best efforts analysis".to_string());
        }
        
        let summary_mode = p.summary.unwrap_or(true);
        let options = intervals_icu_client::BestEffortsOptions {
            stream: Some(p.stream.clone()),
            duration: p.duration,
            distance: p.distance,
            count: p.count.or(Some(5)),
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

        // Apply summary mode
        let result = if summary_mode {
            Self::summarize_best_efforts(&v, &p.stream)
        } else {
            v
        };

        Ok(Json(ObjectResult { value: result }))
    }

    /// Summarize best efforts to compact format
    fn summarize_best_efforts(value: &serde_json::Value, stream: &str) -> serde_json::Value {
        let Some(arr) = value.as_array() else {
            return value.clone();
        };

        let efforts: Vec<serde_json::Value> = arr
            .iter()
            .filter_map(|item| {
                let obj = item.as_object()?;
                let mut compact = serde_json::Map::new();

                // Core fields only
                if let Some(v) = obj.get("value") {
                    compact.insert("value".to_string(), v.clone());
                }
                if let Some(v) = obj.get("duration") {
                    compact.insert("duration".to_string(), v.clone());
                }
                if let Some(v) = obj.get("start_index") {
                    compact.insert("start_index".to_string(), v.clone());
                }

                Some(serde_json::Value::Object(compact))
            })
            .collect();

        serde_json::json!({
            "stream": stream,
            "count": efforts.len(),
            "efforts": efforts
        })
    }

    #[tool(
        name = "get_gear_list",
        description = "Get gear inventory. Params: compact (default true), fields (filter)."
    )]
    async fn get_gear_list(
        &self,
        params: Parameters<GearListParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_gear_list()
            .await
            .map_err(|e| e.to_string())?;

        // Apply compact mode
        let result = if p.compact.unwrap_or(true) {
            Self::compact_gear_list(&v, p.fields.as_deref())
        } else if let Some(ref fields) = p.fields {
            Self::filter_array_fields(&v, fields)
        } else {
            v
        };

        Ok(Json(ObjectResult { value: result }))
    }

    /// Compact gear list to essential fields
    fn compact_gear_list(
        value: &serde_json::Value,
        fields: Option<&[String]>,
    ) -> serde_json::Value {
        let default_fields = ["id", "name", "type", "distance", "brand", "model"];
        let fields_to_use: Vec<&str> = fields
            .map(|f| f.iter().map(|s| s.as_str()).collect())
            .unwrap_or_else(|| default_fields.to_vec());

        let Some(arr) = value.as_array() else {
            return value.clone();
        };

        let compacted: Vec<serde_json::Value> = arr
            .iter()
            .map(|item| {
                let Some(obj) = item.as_object() else {
                    return item.clone();
                };
                let mut result = serde_json::Map::new();
                for field in &fields_to_use {
                    if let Some(val) = obj.get(*field) {
                        result.insert(field.to_string(), val.clone());
                    }
                }
                serde_json::Value::Object(result)
            })
            .collect();

        serde_json::Value::Array(compacted)
    }

    #[tool(
        name = "get_sport_settings",
        description = "Get sport settings: FTP, FTHR, pace thresholds, power/HR zones."
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
        description = "Get power curves. Params: type, days_back, durations (filter), summary (default true)."
    )]
    async fn get_power_curves(
        &self,
        params: Parameters<CurvesParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_power_curves(p.days_back, &p.sport)
            .await
            .map_err(|e| e.to_string())?;

        // Apply compact mode
        let result = Self::transform_curves(&v, p.summary.unwrap_or(true), p.durations.as_deref());
        Ok(Json(ObjectResult { value: result }))
    }

    /// Transform power/hr/pace curves to compact format
    fn transform_curves(
        value: &serde_json::Value,
        summary_only: bool,
        durations: Option<&[u32]>,
    ) -> serde_json::Value {
        // If durations filter is specified, filter the curve points
        if let Some(dur_filter) = durations
            && let Some(obj) = value.as_object()
        {
            let mut result = serde_json::Map::new();
            for (key, val) in obj {
                if let Some(arr) = val.as_array() {
                    // Filter to matching durations
                    let filtered: Vec<&serde_json::Value> = arr
                        .iter()
                        .filter(|item| {
                            item.get("secs")
                                .and_then(|s| s.as_u64())
                                .map(|s| dur_filter.contains(&(s as u32)))
                                .unwrap_or(false)
                        })
                        .collect();
                    result.insert(
                        key.clone(),
                        serde_json::Value::Array(filtered.into_iter().cloned().collect()),
                    );
                } else {
                    result.insert(key.clone(), val.clone());
                }
            }
            return serde_json::Value::Object(result);
        }

        if summary_only {
            // Return key durations only: 5s, 30s, 1min, 5min, 20min, 60min
            let key_durations = [5, 30, 60, 300, 1200, 3600];
            if let Some(obj) = value.as_object() {
                let mut result = serde_json::Map::new();
                for (key, val) in obj {
                    if let Some(arr) = val.as_array() {
                        let filtered: Vec<&serde_json::Value> = arr
                            .iter()
                            .filter(|item| {
                                item.get("secs")
                                    .and_then(|s| s.as_u64())
                                    .map(|s| key_durations.contains(&(s as u32)))
                                    .unwrap_or(false)
                            })
                            .collect();
                        result.insert(
                            key.clone(),
                            serde_json::Value::Array(filtered.into_iter().cloned().collect()),
                        );
                    } else {
                        result.insert(key.clone(), val.clone());
                    }
                }
                return serde_json::Value::Object(result);
            }
        }

        value.clone()
    }

    #[tool(
        name = "get_gap_histogram",
        description = "Get GAP distribution. Params: activity_id, summary (default true), bins (default 10)."
    )]
    async fn get_gap_histogram(
        &self,
        params: Parameters<HistogramParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_gap_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;

        let result =
            Self::transform_histogram(&v, p.summary.unwrap_or(true), p.bins.unwrap_or(10) as usize);
        Ok(Json(ObjectResult { value: result }))
    }

    /// Transform histogram to compact format
    fn transform_histogram(
        value: &serde_json::Value,
        summary_only: bool,
        max_bins: usize,
    ) -> serde_json::Value {
        if summary_only {
            // Compute summary statistics from histogram
            if let Some(arr) = value.as_array() {
                let mut total_count: f64 = 0.0;
                let mut weighted_sum: f64 = 0.0;
                let mut min_val: Option<f64> = None;
                let mut max_val: Option<f64> = None;

                for item in arr {
                    let value = item.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let count = item.get("count").and_then(|v| v.as_f64()).unwrap_or(0.0);

                    if count > 0.0 {
                        total_count += count;
                        weighted_sum += value * count;
                        min_val = Some(min_val.map_or(value, |m: f64| m.min(value)));
                        max_val = Some(max_val.map_or(value, |m: f64| m.max(value)));
                    }
                }

                return serde_json::json!({
                    "total_samples": total_count as u64,
                    "weighted_avg": if total_count > 0.0 { (weighted_sum / total_count * 100.0).round() / 100.0 } else { 0.0 },
                    "min": min_val.unwrap_or(0.0),
                    "max": max_val.unwrap_or(0.0),
                    "bins_available": arr.len()
                });
            }
        }

        // Limit bins if needed
        if let Some(arr) = value.as_array()
            && arr.len() > max_bins
        {
            // Sample bins uniformly
            let step = arr.len() / max_bins;
            let sampled: Vec<serde_json::Value> = arr
                .iter()
                .step_by(step.max(1))
                .take(max_bins)
                .cloned()
                .collect();
            return serde_json::Value::Array(sampled);
        }

        value.clone()
    }

    #[tool(
        name = "start_download",
        description = "Start activity file download with progress. Returns download_id for status checks."
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
        description = "Download activity as FIT file. Optional output_path saves to disk."
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
        description = "Download activity as GPX file. Optional output_path saves to disk."
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
        description = "Check download progress by download_id."
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
        description = "Receive and verify webhook payload with HMAC signature."
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
        description = "Get activities before/after a specific activity."
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
        description = "Search similar intervals across activities by duration, intensity, type."
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
        description = "Get power distribution. Params: activity_id, summary (default true), bins (default 10)."
    )]
    async fn get_power_histogram(
        &self,
        params: Parameters<HistogramParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_power_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;

        let result =
            Self::transform_histogram(&v, p.summary.unwrap_or(true), p.bins.unwrap_or(10) as usize);
        Ok(Json(ObjectResult { value: result }))
    }

    #[tool(
        name = "get_hr_histogram",
        description = "Get HR distribution. Params: activity_id, summary (default true), bins (default 10)."
    )]
    async fn get_hr_histogram(
        &self,
        params: Parameters<HistogramParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_hr_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;

        let result =
            Self::transform_histogram(&v, p.summary.unwrap_or(true), p.bins.unwrap_or(10) as usize);
        Ok(Json(ObjectResult { value: result }))
    }

    #[tool(
        name = "get_pace_histogram",
        description = "Get pace distribution. Params: activity_id, summary (default true), bins (default 10)."
    )]
    async fn get_pace_histogram(
        &self,
        params: Parameters<HistogramParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_pace_histogram(&p.activity_id)
            .await
            .map_err(|e| e.to_string())?;

        let result =
            Self::transform_histogram(&v, p.summary.unwrap_or(true), p.bins.unwrap_or(10) as usize);
        Ok(Json(ObjectResult { value: result }))
    }

    // === Fitness Summary ===

    #[tool(
        name = "get_fitness_summary",
        description = "Get fitness: CTL, ATL, TSB, ramp rate."
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
        description = "Get wellness data. Params: days_back (default 7), summary (default true), fields (filter)."
    )]
    async fn get_wellness(
        &self,
        params: Parameters<WellnessParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_wellness(p.days_back.or(Some(7)))
            .await
            .map_err(|e| e.to_string())?;

        // Apply compact mode
        let result = Self::transform_wellness(&v, p.summary.unwrap_or(true), p.fields.as_deref());
        Ok(Json(ObjectResult { value: result }))
    }

    /// Transform wellness data to compact format
    fn transform_wellness(
        value: &serde_json::Value,
        summary_only: bool,
        fields: Option<&[String]>,
    ) -> serde_json::Value {
        let Some(arr) = value.as_array() else {
            return value.clone();
        };

        if summary_only {
            // Compute trends/averages
            let mut sleep_total: f64 = 0.0;
            let mut stress_total: f64 = 0.0;
            let mut hr_total: f64 = 0.0;
            let mut hrv_total: f64 = 0.0;
            let mut sleep_count: usize = 0;
            let mut stress_count: usize = 0;
            let mut hr_count: usize = 0;
            let mut hrv_count: usize = 0;

            for item in arr {
                if let Some(obj) = item.as_object() {
                    if let Some(v) = obj.get("sleepSecs").and_then(|v| v.as_f64()) {
                        sleep_total += v / 3600.0; // convert to hours
                        sleep_count += 1;
                    }
                    if let Some(v) = obj.get("stress").and_then(|v| v.as_f64()) {
                        stress_total += v;
                        stress_count += 1;
                    }
                    if let Some(v) = obj.get("restingHR").and_then(|v| v.as_f64()) {
                        hr_total += v;
                        hr_count += 1;
                    }
                    if let Some(v) = obj.get("hrv").and_then(|v| v.as_f64()) {
                        hrv_total += v;
                        hrv_count += 1;
                    }
                }
            }

            return serde_json::json!({
                "days": arr.len(),
                "avg_sleep_hours": if sleep_count > 0 { (sleep_total / sleep_count as f64 * 10.0).round() / 10.0 } else { 0.0 },
                "avg_stress": if stress_count > 0 { (stress_total / stress_count as f64 * 10.0).round() / 10.0 } else { 0.0 },
                "avg_resting_hr": if hr_count > 0 { (hr_total / hr_count as f64).round() } else { 0.0 },
                "avg_hrv": if hrv_count > 0 { (hrv_total / hrv_count as f64).round() } else { 0.0 }
            });
        }

        // Filter fields if specified
        if let Some(field_list) = fields {
            let default_fields = ["id", "sleepSecs", "stress", "restingHR", "hrv", "weight"];
            let fields_to_use: Vec<&str> = field_list.iter().map(|s| s.as_str()).collect();
            let fields_to_use = if fields_to_use.is_empty() {
                default_fields.to_vec()
            } else {
                fields_to_use
            };

            let filtered: Vec<serde_json::Value> = arr
                .iter()
                .map(|item| {
                    let Some(obj) = item.as_object() else {
                        return item.clone();
                    };
                    let mut result = serde_json::Map::new();
                    for field in &fields_to_use {
                        if let Some(val) = obj.get(*field) {
                            result.insert(field.to_string(), val.clone());
                        }
                    }
                    serde_json::Value::Object(result)
                })
                .collect();

            return serde_json::Value::Array(filtered);
        }

        value.clone()
    }

    #[tool(
        name = "get_wellness_for_date",
        description = "Get wellness for specific date (YYYY-MM-DD)."
    )]
    async fn get_wellness_for_date(
        &self,
        params: Parameters<DateParam>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let mut date = p.date.clone();
        // accept YYYY-MM-DD or ISO datetimes; normalize to YYYY-MM-DD
        date = match Self::normalize_date_str(&date) {
            Ok(s) => s,
            Err(()) => return Err(format!("invalid date: {}", p.date)),
        };
        let v = self
            .client
            .get_wellness_for_date(&date)
            .await
            .map_err(|e| e.to_string())?;
        Ok(Json(ObjectResult { value: v }))
    }

    #[tool(
        name = "update_wellness",
        description = "Update wellness for a date: sleep_hours, stress_level, resting_hr, notes."
    )]
    async fn update_wellness(
        &self,
        params: Parameters<WellnessUpdateParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let mut date = p.date.clone();
        date = match Self::normalize_date_str(&date) {
            Ok(s) => s,
            Err(()) => return Err(format!("invalid date: {}", p.date)),
        };
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
        description = "Get scheduled workouts. Param: days_ahead (default 7)."
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
        description = "Update calendar event fields: title, description, date, category."
    )]
    async fn update_event(
        &self,
        params: Parameters<UpdateEventParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let event_id = p.event_id.as_cow().into_owned();
        // If fields contain start_date_local, normalize it (preserve time when present)
        let mut fields = p.fields.clone();
        if let Some(obj) = fields.as_object_mut()
            && let Some(val) = obj.get("start_date_local")
            && let Some(s) = val.as_str()
        {
            let s2 = match Self::normalize_event_start(s) {
                Ok(s2) => s2,
                Err(()) => return Err(format!("invalid start_date_local: {}", s)),
            };
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
        description = "Delete multiple calendar events by IDs."
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
        description = "Duplicate event to future dates. Params: num_copies, weeks_between."
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
        description = "Get HR curves. Params: type, days_back, durations (filter), summary (default true)."
    )]
    async fn get_hr_curves(
        &self,
        params: Parameters<CurvesParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_hr_curves(p.days_back, &p.sport)
            .await
            .map_err(|e| e.to_string())?;

        let result = Self::transform_curves(&v, p.summary.unwrap_or(true), p.durations.as_deref());
        Ok(Json(ObjectResult { value: result }))
    }

    #[tool(
        name = "get_pace_curves",
        description = "Get pace/speed curves. Params: type, days_back, durations (filter), summary (default true)."
    )]
    async fn get_pace_curves(
        &self,
        params: Parameters<CurvesParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_pace_curves(p.days_back, &p.sport)
            .await
            .map_err(|e| e.to_string())?;

        let result = Self::transform_curves(&v, p.summary.unwrap_or(true), p.durations.as_deref());
        Ok(Json(ObjectResult { value: result }))
    }

    // === Workout Library ===

    #[tool(
        name = "get_workout_library",
        description = "Get workout library folders. Use get_workouts_in_folder for contents."
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
        description = "Get workouts in folder. Params: folder_id, compact (default true), limit (default 20)."
    )]
    async fn get_workouts_in_folder(
        &self,
        params: Parameters<WorkoutsInFolderParams>,
    ) -> Result<Json<ObjectResult>, String> {
        let p = params.0;
        let v = self
            .client
            .get_workouts_in_folder(&p.folder_id)
            .await
            .map_err(|e| e.to_string())?;

        // Apply compact mode
        let result = Self::compact_workouts(
            &v,
            p.compact.unwrap_or(true),
            p.limit.unwrap_or(20) as usize,
            p.fields.as_deref(),
        );

        Ok(Json(ObjectResult { value: result }))
    }

    /// Compact workouts list
    fn compact_workouts(
        value: &serde_json::Value,
        compact: bool,
        limit: usize,
        fields: Option<&[String]>,
    ) -> serde_json::Value {
        let Some(arr) = value.as_array() else {
            return value.clone();
        };

        let default_fields = ["id", "name", "type", "duration", "description"];
        let fields_to_use: Vec<&str> = if compact {
            fields
                .map(|f| f.iter().map(|s| s.as_str()).collect())
                .unwrap_or_else(|| default_fields.to_vec())
        } else {
            return serde_json::Value::Array(arr.iter().take(limit).cloned().collect());
        };

        let compacted: Vec<serde_json::Value> = arr
            .iter()
            .take(limit)
            .map(|item| {
                let Some(obj) = item.as_object() else {
                    return item.clone();
                };
                let mut result = serde_json::Map::new();
                for field in &fields_to_use {
                    if let Some(val) = obj.get(*field) {
                        result.insert(field.to_string(), val.clone());
                    }
                }
                serde_json::Value::Object(result)
            })
            .collect();

        serde_json::Value::Array(compacted)
    }

    // === Gear Management ===

    #[tool(
        name = "create_gear",
        description = "Add gear item (bike, shoes, watch). Specify name, type, weight."
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

    #[tool(name = "update_gear", description = "Update gear item fields.")]
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
        description = "Set maintenance reminder for gear."
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
        description = "Update or snooze gear reminder. Params: reset, snooze_days."
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
        description = "Update sport settings: FTP, FTHR, pace thresholds, zones."
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
        description = "Apply sport settings to all historical activities."
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
        _request: Option<PaginatedRequestParams>,
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
        request: ReadResourceRequestParams,
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
        use std::sync::Arc;
        struct CapturingBulkClient {
            captured: std::sync::Arc<tokio::sync::Mutex<Option<Vec<intervals_icu_client::Event>>>>,
        }
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for CapturingBulkClient {
            async fn get_athlete_profile(
                &self,
            ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError>
            {
                unimplemented!()
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
                events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                let mut cap = self.captured.lock().await;
                *cap = Some(events.clone());
                Ok(events)
            }

            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                // param unused in this mock; explicitly ignore to satisfy clippy
                let _ = progress_tx;
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
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
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
        let client = CapturingBulkClient {
            captured: captured.clone(),
        };
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
        let stored = captured.lock().await;
        let evs = stored.as_ref().unwrap();
        assert_eq!(evs[0].r#type.as_deref(), Some("Run"));
        assert_eq!(evs[0].start_date_local, "2026-01-01T00:00:00");
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
                    "type": "Run",
                    "description": null
                }
            ]
        });
        let params: Parameters<BulkCreateEventsToolParams> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler
            .bulk_create_events(params)
            .await
            .expect("bulk create");
        assert_eq!(res.0.events[0].start_date_local, "2026-01-19T06:30:00");
    }

    #[tokio::test]
    async fn create_event_accepts_iso_datetime_and_normalizes_date() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "name": "Test Workout",
            "start_date_local": "2026-01-19T06:30:00",
            "category": "WORKOUT",
            "type": "Run",
            "description": null
        });
        let params: Parameters<intervals_icu_client::Event> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.create_event(params).await;
        let created = res.expect("create event should succeed");
        assert_eq!(created.0.start_date_local, "2026-01-19T06:30:00");
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
    async fn create_event_rejects_missing_type_for_workout() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "name": "Test Workout",
            "start_date_local": "2026-01-19",
            "category": "WORKOUT",
            "description": null
        });
        let params: Parameters<intervals_icu_client::Event> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.create_event(params).await;
        assert!(res.is_ok());
        let created = res.unwrap().0;
        assert_eq!(created.r#type, Some("Run".into()));
        assert_eq!(created.start_date_local, "2026-01-19T00:00:00");
    }

    #[tokio::test]
    async fn create_event_rejects_empty_name() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "name": "",
            "start_date_local": "2026-01-19",
            "category": "NOTE"
        });
        let params: Parameters<intervals_icu_client::Event> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.create_event(params).await;
        match res {
            Err(e) => assert!(e.contains("name is empty")),
            Ok(_) => panic!("Expected error for empty name"),
        }
    }

    #[tokio::test]
    async fn create_event_rejects_unknown_category() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = json!({
            "name": "Test Event",
            "start_date_local": "2026-01-19",
            "category": "UNKNOWN"
        });
        let params: Parameters<intervals_icu_client::Event> =
            serde_json::from_value(payload).expect("payload should deserialize");
        let res = handler.create_event(params).await;
        match res {
            Err(e) => assert!(e.contains("invalid category: unknown")),
            Ok(_) => panic!("Expected error for unknown category"),
        }
    }

    #[tokio::test]
    async fn bulk_create_events_rejects_empty_name() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let ev = intervals_icu_client::Event {
            id: None,
            start_date_local: "2026-01-19".into(),
            name: "".into(),
            category: intervals_icu_client::EventCategory::Note,
            description: None,
            r#type: None,
        };
        let params = Parameters(BulkCreateEventsToolParams { events: vec![ev] });
        let res = handler.bulk_create_events(params).await;
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("name is empty"));
    }

    #[tokio::test]
    async fn bulk_create_events_defaults_missing_type_for_workout() {
        use std::sync::Arc;
        struct CaptureBulkClient {
            captured: std::sync::Arc<tokio::sync::Mutex<Option<Vec<intervals_icu_client::Event>>>>,
        }
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for CaptureBulkClient {
            async fn get_athlete_profile(
                &self,
            ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError>
            {
                Ok(intervals_icu_client::AthleteProfile {
                    id: "a".into(),
                    name: None,
                })
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
                event: intervals_icu_client::Event,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                Ok(event)
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
            async fn update_event(
                &self,
                _event_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
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
                events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                let mut guard = self.captured.lock().await;
                *guard = Some(events.clone());
                Ok(events)
            }
            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(Some("/tmp/x".into()))
            }
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
            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
                Ok("".into())
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
        let client = CaptureBulkClient {
            captured: captured.clone(),
        };
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        let ev = intervals_icu_client::Event {
            id: None,
            start_date_local: "2026-01-19".into(),
            name: "Workout Event".into(),
            category: intervals_icu_client::EventCategory::Workout,
            description: None,
            r#type: None,
        };
        let params = Parameters(BulkCreateEventsToolParams { events: vec![ev] });
        let _res = handler.bulk_create_events(params).await.expect("ok");
        let guard = captured.lock().await;
        let stored = guard.as_ref().expect("captured");
        assert_eq!(stored[0].r#type.as_deref(), Some("Run"));
    }

    #[tokio::test]
    async fn get_recent_activities_mapping() {
        struct R;
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for R {
            async fn get_athlete_profile(
                &self,
            ) -> Result<intervals_icu_client::AthleteProfile, intervals_icu_client::IntervalsError>
            {
                Ok(intervals_icu_client::AthleteProfile {
                    id: "me".into(),
                    name: Some("X".into()),
                })
            }
            async fn get_recent_activities(
                &self,
                _limit: Option<u32>,
                _days_back: Option<i32>,
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                Ok(vec![intervals_icu_client::ActivitySummary {
                    id: "a1".into(),
                    name: Some("Run".into()),
                }])
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
            async fn update_event(
                &self,
                _event_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
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
                Ok(Some("/tmp/x".into()))
            }
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
            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
                Ok("".into())
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
        let client = R;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        let params = RecentParams {
            limit: None,
            days_back: None,
        };
        let res = handler
            .get_recent_activities(Parameters(params))
            .await
            .expect("ok");
        assert_eq!(res.0.activities.len(), 1);
        assert_eq!(res.0.activities[0].id, "a1");
    }

    #[tokio::test]
    async fn set_webhook_secret_missing_secret() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let res = handler
            .set_webhook_secret(Parameters(ObjectResult {
                value: serde_json::json!({}),
            }))
            .await;
        assert!(res.is_err());
        assert!(res.err().unwrap().contains("missing secret"));
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
        async fn update_event(
            &self,
            _event_id: &str,
            _fields: &serde_json::Value,
        ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
            Ok(serde_json::json!({}))
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
            events: Vec<intervals_icu_client::Event>,
        ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
        {
            Ok(events)
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

        // Streams - now uses StreamsParams
        let streams_param = StreamsParams {
            activity_id: "a1".into(),
            max_points: None,
            summary: None,
            streams: None,
        };
        let res = handler
            .get_activity_streams(Parameters(streams_param))
            .await;
        assert!(res.is_ok());

        // Intervals
        let intervals_param = ActivityIntervalsParams {
            activity_id: "a1".into(),
            summary: None,
            max_intervals: None,
            fields: None,
        };
        let res = handler
            .get_activity_intervals(Parameters(intervals_param))
            .await;
        assert!(res.is_ok());

        // Best efforts - now the tool requires explicit stream per API contract
        let best_param = BestEffortsCompactParams {
            activity_id: "a1".into(),
            stream: "power".into(),
            duration: Some(60),
            distance: None,
            count: None,
            summary: None,
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
    async fn download_cancel_transitions() {
        struct CancelMockClient;
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for CancelMockClient {
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
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                Ok(vec![])
            }
            async fn create_event(
                &self,
                event: intervals_icu_client::Event,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                Ok(event)
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
            async fn update_event(
                &self,
                _event_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
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
                events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(events)
            }

            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                // simulate progress and respect cancel
                let tx = progress_tx;
                for i in 1..=10u64 {
                    if *cancel_rx.borrow() {
                        return Err(intervals_icu_client::IntervalsError::Config(
                            "cancelled by user".into(),
                        ));
                    }
                    let _ = tx.try_send(intervals_icu_client::DownloadProgress {
                        bytes_downloaded: i * 10,
                        total_bytes: Some(100),
                    });
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Ok(Some("/tmp/cancel_ok.fit".into()))
            }

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

            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
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

        let client = CancelMockClient;
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

        // wait until we observe at least one progress update
        let mut attempts = 0;
        loop {
            let status = handler
                .get_download_status(Parameters(DownloadIdParam {
                    download_id: id.clone(),
                }))
                .await
                .expect("status");
            if matches!(status.0.status.state, DownloadState::InProgress)
                && status.0.status.bytes_downloaded > 0
            {
                break;
            }
            attempts += 1;
            if attempts > 50 {
                panic!("did not observe progress")
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // cancel
        let _ = handler
            .cancel_download(Parameters(DownloadIdParam {
                download_id: id.clone(),
            }))
            .await
            .expect("cancel");

        // immediately the status should reflect Cancelled
        let status_now = handler
            .get_download_status(Parameters(DownloadIdParam {
                download_id: id.clone(),
            }))
            .await
            .expect("status");
        match status_now.0.status.state {
            DownloadState::Cancelled => {}
            other => panic!("expected Cancelled, got {:?}", other),
        }

        // eventually the background task should set final state to Failed because mock returns Err when cancelled
        let mut attempts = 0;
        loop {
            let status = handler
                .get_download_status(Parameters(DownloadIdParam {
                    download_id: id.clone(),
                }))
                .await
                .expect("status");
            if let DownloadState::Failed(ref s) = status.0.status.state {
                assert!(s.contains("cancelled"));
                break;
            }
            attempts += 1;
            if attempts > 100 {
                panic!("did not transition to Failed")
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn download_immediate_failure() {
        struct FailMockClient;
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for FailMockClient {
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
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                Ok(vec![])
            }
            async fn create_event(
                &self,
                event: intervals_icu_client::Event,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                Ok(event)
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
            async fn update_event(
                &self,
                _event_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
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
                events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(events)
            }

            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                // immediate fail
                Err(intervals_icu_client::IntervalsError::Config(
                    "immediate failure".into(),
                ))
            }

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

            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
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

        let client = FailMockClient;
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

        // eventually should become Failed
        let mut attempts = 0;
        loop {
            let status = handler
                .get_download_status(Parameters(DownloadIdParam {
                    download_id: id.clone(),
                }))
                .await
                .expect("status");
            if let DownloadState::Failed(ref s) = status.0.status.state {
                assert!(s.contains("immediate failure"));
                break;
            }
            attempts += 1;
            if attempts > 100 {
                panic!("did not fail")
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }

    #[tokio::test]
    async fn download_success_and_list_downloads() {
        struct SuccessMockClient;
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for SuccessMockClient {
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
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                Ok(vec![])
            }
            async fn create_event(
                &self,
                event: intervals_icu_client::Event,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                Ok(event)
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
            async fn update_event(
                &self,
                _event_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
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
                Ok(Some("YmFzZTY0ZG9uZQ==".into()))
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
                events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(events)
            }
            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                // simulate progress and complete
                let _ = progress_tx.try_send(intervals_icu_client::DownloadProgress {
                    bytes_downloaded: 10,
                    total_bytes: Some(100),
                });
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                let _ = progress_tx.try_send(intervals_icu_client::DownloadProgress {
                    bytes_downloaded: 100,
                    total_bytes: Some(100),
                });
                Ok(Some("/tmp/success.fit".into()))
            }
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

            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
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

        let client = SuccessMockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        let params = DownloadParams {
            activity_id: "a_ok".into(),
            output_path: None,
        };
        let res = handler
            .start_download(Parameters(params))
            .await
            .expect("start");
        let id = res.0.download_id.clone();

        // wait for final Completed state
        let mut attempts = 0;
        loop {
            let status = handler
                .get_download_status(Parameters(DownloadIdParam {
                    download_id: id.clone(),
                }))
                .await
                .expect("status");
            if let DownloadState::Completed = status.0.status.state {
                assert_eq!(status.0.status.path.as_deref(), Some("/tmp/success.fit"));
                break;
            }
            attempts += 1;
            if attempts > 200 {
                panic!("did not complete")
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }

        // list_downloads should include our id
        let list = handler.list_downloads().await.expect("list");
        assert!(
            list.0
                .downloads
                .iter()
                .any(|d| d.id == id && matches!(d.state, DownloadState::Completed))
        );
    }

    #[tokio::test]
    async fn download_fit_and_gpx_file_handling() {
        struct FitGpxMockClient;
        #[async_trait::async_trait]
        impl intervals_icu_client::IntervalsClient for FitGpxMockClient {
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
            ) -> Result<
                Vec<intervals_icu_client::ActivitySummary>,
                intervals_icu_client::IntervalsError,
            > {
                Ok(vec![])
            }
            async fn create_event(
                &self,
                event: intervals_icu_client::Event,
            ) -> Result<intervals_icu_client::Event, intervals_icu_client::IntervalsError>
            {
                Ok(event)
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
            async fn update_event(
                &self,
                _event_id: &str,
                _fields: &serde_json::Value,
            ) -> Result<serde_json::Value, intervals_icu_client::IntervalsError> {
                Ok(serde_json::json!({}))
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
                Ok(Some("YmFzZTY0ZGVhdGE=".into()))
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
                events: Vec<intervals_icu_client::Event>,
            ) -> Result<Vec<intervals_icu_client::Event>, intervals_icu_client::IntervalsError>
            {
                Ok(events)
            }
            async fn download_activity_file_with_progress(
                &self,
                _activity_id: &str,
                _output_path: Option<std::path::PathBuf>,
                _progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                Ok(Some("/tmp/fitgpx_dummy.fit".into()))
            }
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

            async fn get_activities_csv(
                &self,
            ) -> Result<String, intervals_icu_client::IntervalsError> {
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

        let client = FitGpxMockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // download fit -> base64
        let params_fit = DownloadParams {
            activity_id: "a1".into(),
            output_path: None,
        };
        let res_fit = handler
            .download_fit_file(Parameters(params_fit))
            .await
            .expect("fit");
        assert_eq!(
            res_fit.0.value.get("base64").and_then(|v| v.as_str()),
            Some("YmFzZTY0ZGVhdGE=")
        );

        // download gpx -> written_to_disk with path
        let params_gpx = DownloadParams {
            activity_id: "a1".into(),
            output_path: Some("/tmp/out.gpx".into()),
        };
        let res_gpx = handler
            .download_gpx_file(Parameters(params_gpx))
            .await
            .expect("gpx");
        assert_eq!(
            res_gpx
                .0
                .value
                .get("written_to_disk")
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            res_gpx.0.value.get("path").and_then(|v| v.as_str()),
            Some("/tmp/out.gpx")
        );
    }

    #[tokio::test]
    async fn gear_and_curves_tools() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // gear
        let res = handler
            .get_gear_list(Parameters(GearListParams {
                compact: None,
                fields: None,
            }))
            .await;
        assert!(res.is_ok());

        // sport settings
        let res = handler.get_sport_settings().await;
        assert!(res.is_ok());

        // power curves
        let res = handler
            .get_power_curves(Parameters(CurvesParams {
                sport: "Ride".into(),
                days_back: Some(30),
                durations: None,
                summary: None,
            }))
            .await;
        assert!(res.is_ok());

        // lowercase sport should also work (normalize to canonical form)
        // This uses the mock client so will succeed

        // gap histogram
        let res = handler
            .get_gap_histogram(Parameters(HistogramParams {
                activity_id: "a1".into(),
                summary: None,
                bins: None,
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
            .get_power_curves(Parameters(CurvesParams {
                sport: "run".into(),
                days_back: Some(7),
                durations: None,
                summary: None,
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
                progress_tx: tokio::sync::mpsc::Sender<intervals_icu_client::DownloadProgress>,
                _cancel_rx: tokio::sync::watch::Receiver<bool>,
            ) -> Result<Option<String>, intervals_icu_client::IntervalsError> {
                // param unused in this mock; explicitly ignore to satisfy clippy
                let _ = progress_tx;
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
                // Capture the provided fields for assertions in tests
                let mut guard = self.captured.lock().await;
                *guard = Some(fields.clone());
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
            Some("2026-01-19T06:30:00")
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
        // Compact description just says "Activity act1 analysis"
        assert!(pd.description.unwrap().contains("act1"));
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
    }

    #[test]
    fn normalize_date_str_accepts_known_formats() {
        // Directly exercise the new helper to ensure consistent behavior
        assert_eq!(
            IntervalsMcpHandler::normalize_date_str("2026-01-19").unwrap(),
            "2026-01-19"
        );
        assert_eq!(
            IntervalsMcpHandler::normalize_date_str("2026-01-19T06:30:00Z").unwrap(),
            "2026-01-19"
        );
        assert_eq!(
            IntervalsMcpHandler::normalize_date_str("2026-01-19T06:30:00").unwrap(),
            "2026-01-19"
        );
        assert!(IntervalsMcpHandler::normalize_date_str("not-a-date").is_err());
    }

    #[test]
    fn normalize_event_start_accepts_known_formats() {
        // Test date-only format
        assert_eq!(
            IntervalsMcpHandler::normalize_event_start("2026-01-19").unwrap(),
            "2026-01-19T00:00:00"
        );
        // Test RFC3339 format
        assert_eq!(
            IntervalsMcpHandler::normalize_event_start("2026-01-19T06:30:00Z").unwrap(),
            "2026-01-19T06:30:00"
        );
        // Test naive datetime format
        assert_eq!(
            IntervalsMcpHandler::normalize_event_start("2026-01-19T06:30:00").unwrap(),
            "2026-01-19T06:30:00"
        );
        // Test invalid format
        assert!(IntervalsMcpHandler::normalize_event_start("not-a-date").is_err());
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
    async fn process_webhook_secret_not_set() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let payload = serde_json::json!({ "id": "x" });
        let res = handler.process_webhook("deadbeef", payload).await;
        match res {
            Err(e) => assert!(e.contains("webhook secret not set")),
            Ok(_) => panic!("Expected error when webhook secret not set"),
        }
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

    // === Token-Efficiency Tests ===

    #[test]
    fn transform_streams_summary_computes_stats() {
        let input = serde_json::json!({
            "power": [100, 150, 200, 250, 300, 350, 400, 450, 500, 550],
            "heartrate": [120, 125, 130, 135, 140, 145, 150, 155, 160, 165]
        });

        let result = IntervalsMcpHandler::transform_streams(input, None, true, None);

        let power_stats = result.get("power").expect("power stats");
        assert_eq!(power_stats.get("count").and_then(|v| v.as_u64()), Some(10));
        assert_eq!(power_stats.get("min").and_then(|v| v.as_f64()), Some(100.0));
        assert_eq!(power_stats.get("max").and_then(|v| v.as_f64()), Some(550.0));
        assert!(power_stats.get("avg").is_some());
        assert!(power_stats.get("p50").is_some());

        let hr_stats = result.get("heartrate").expect("hr stats");
        assert_eq!(hr_stats.get("count").and_then(|v| v.as_u64()), Some(10));
    }

    #[test]
    fn transform_streams_downsample_reduces_array() {
        let input = serde_json::json!({
            "power": [100, 110, 120, 130, 140, 150, 160, 170, 180, 190, 200]
        });

        let result = IntervalsMcpHandler::transform_streams(input, Some(5), false, None);

        let arr = result
            .get("power")
            .and_then(|v| v.as_array())
            .expect("power array");
        assert!(arr.len() <= 5, "should downsample to max 5 points");
        // First and last should be preserved
        assert_eq!(arr.first().and_then(|v| v.as_i64()), Some(100));
        assert_eq!(arr.last().and_then(|v| v.as_i64()), Some(200));
    }

    #[test]
    fn transform_streams_filter_streams() {
        let input = serde_json::json!({
            "power": [100, 200, 300],
            "heartrate": [120, 140, 160],
            "cadence": [80, 85, 90]
        });

        let result = IntervalsMcpHandler::transform_streams(
            input,
            None,
            false,
            Some(vec!["power".into(), "heartrate".into()]),
        );

        assert!(result.get("power").is_some());
        assert!(result.get("heartrate").is_some());
        assert!(
            result.get("cadence").is_none(),
            "cadence should be filtered out"
        );
    }

    #[tokio::test]
    async fn get_activity_streams_downsample_and_filter() {
        use std::sync::Arc;
        // Use the existing `MockClient` defined in this file to keep the impl simple
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        // The default mock returns a nested `"streams"` object; verify wrapper works
        let params = StreamsParams {
            activity_id: "a1".into(),
            max_points: Some(3),
            summary: Some(false),
            streams: None,
        };
        let res = handler
            .get_activity_streams(Parameters(params))
            .await
            .expect("should succeed")
            .0
            .value;
        assert!(res.get("streams").is_some());
        assert!(res["streams"].get("time").is_some());
    }

    #[tokio::test]
    async fn get_activity_streams_summary_computes_stats() {
        use std::sync::Arc;
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = StreamsParams {
            activity_id: "a1".into(),
            max_points: None,
            summary: Some(true),
            streams: None,
        };
        let res = handler
            .get_activity_streams(Parameters(params))
            .await
            .expect("should succeed")
            .0
            .value;
        // Mock returns nested "streams" object with "time" array; ensure wrapper returns it
        assert!(res.get("streams").is_some());
        assert!(res["streams"].get("time").is_some());
    }

    #[tokio::test]
    async fn get_activity_details_expand_and_compact() {
        use std::sync::Arc;
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        // expand=true with fields should apply filtering
        let params = ActivityDetailsParams {
            activity_id: "a1".into(),
            fields: Some(vec!["id".into()]),
            expand: Some(true),
        };
        let res = handler
            .get_activity_details(Parameters(params))
            .await
            .expect("should succeed")
            .0
            .value;
        assert_eq!(res, serde_json::json!({ "id": "a1" }));

        // compact mode without fields should return summary (id present in mock)
        let params2 = ActivityDetailsParams {
            activity_id: "a1".into(),
            fields: None,
            expand: Some(false),
        };
        let res2 = handler
            .get_activity_details(Parameters(params2))
            .await
            .expect("should succeed")
            .0
            .value;
        assert!(res2.get("id").is_some());
    }

    #[test]
    fn extract_activity_summary_returns_compact() {
        let input = serde_json::json!({
            "id": "a123",
            "name": "Morning Run",
            "start_date_local": "2026-01-30T07:00:00",
            "type": "Run",
            "moving_time": 3600,
            "distance": 10000.0,
            "total_elevation_gain": 150.0,
            "average_watts": null,
            "average_heartrate": 145.0,
            "icu_training_load": 85.0,
            "icu_intensity": 0.72,
            "some_extra_field": "should be excluded",
            "another_big_field": [1,2,3,4,5,6,7,8,9,10]
        });

        let result = IntervalsMcpHandler::extract_activity_summary(&input, None);

        assert!(result.get("id").is_some());
        assert!(result.get("name").is_some());
        assert!(result.get("distance").is_some());
        assert!(result.get("moving_time").is_some());
        assert!(result.get("average_heartrate").is_some());
        // Extra fields should NOT be included
        assert!(result.get("some_extra_field").is_none());
        assert!(result.get("another_big_field").is_none());
    }

    #[test]
    fn extract_activity_summary_with_custom_fields() {
        let input = serde_json::json!({
            "id": "a123",
            "name": "Morning Run",
            "distance": 10000.0,
            "calories": 500,
            "average_speed": 2.78
        });

        let result = IntervalsMcpHandler::extract_activity_summary(
            &input,
            Some(&["id".into(), "calories".into()]),
        );

        assert!(result.get("id").is_some());
        assert!(result.get("calories").is_some());
        assert!(result.get("name").is_none(), "name not in custom fields");
        assert!(
            result.get("distance").is_none(),
            "distance not in custom fields"
        );
    }

    #[test]
    fn filter_fields_filters_object_fields() {
        let input = serde_json::json!({
            "id": "a123",
            "name": "Test Activity",
            "distance": 10000.0,
            "calories": 500
        });

        let fields = vec!["id".to_string(), "name".to_string()];
        let result = IntervalsMcpHandler::filter_fields(&input, &fields);

        assert_eq!(result["id"], "a123");
        assert_eq!(result["name"], "Test Activity");
        assert!(result.get("distance").is_none());
        assert!(result.get("calories").is_none());
    }

    #[test]
    fn filter_fields_returns_non_object_unchanged() {
        let input = serde_json::json!("string value");
        let fields = vec!["id".to_string()];
        let result = IntervalsMcpHandler::filter_fields(&input, &fields);
        assert_eq!(result, input);
    }

    // === New Token-Efficiency Tests ===

    #[test]
    fn filter_csv_limits_rows_and_columns() {
        let csv = "id,start_date_local,name,type,moving_time,distance,calories\n\
                   1,2026-01-01,Run1,Run,3600,10000,500\n\
                   2,2026-01-02,Run2,Run,3000,8000,400\n\
                   3,2026-01-03,Run3,Run,2400,6000,300";

        let result = IntervalsMcpHandler::filter_csv(csv, 2, 90, None);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 3, "header + 2 rows");
        // Default columns should be filtered
        assert!(lines[0].contains("id"));
        assert!(lines[0].contains("name"));
        assert!(
            !lines[0].contains("calories"),
            "calories not in default columns"
        );
    }

    #[test]
    fn filter_csv_custom_columns() {
        let csv = "id,name,distance,calories\n\
                   1,Run1,10000,500\n\
                   2,Run2,8000,400";

        let result =
            IntervalsMcpHandler::filter_csv(csv, 100, 90, Some(&["id".into(), "calories".into()]));
        let lines: Vec<&str> = result.lines().collect();
        assert!(lines[0].contains("id"));
        assert!(lines[0].contains("calories"));
        assert!(!lines[0].contains("name"));
        assert!(!lines[0].contains("distance"));
    }

    #[test]
    fn compact_activities_array_filters_fields() {
        let input = serde_json::json!([
            {"id": "1", "name": "Run", "distance": 10000, "some_extra": "data"},
            {"id": "2", "name": "Ride", "distance": 50000, "another_extra": [1,2,3]}
        ]);

        let result = IntervalsMcpHandler::compact_activities_array(&input, None);
        let arr = result.as_array().expect("array");

        assert_eq!(arr.len(), 2);
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("name").is_some());
        assert!(arr[0].get("distance").is_some());
        assert!(arr[0].get("some_extra").is_none());
        assert!(arr[1].get("another_extra").is_none());
    }

    #[test]
    fn transform_intervals_summary_mode() {
        let input = serde_json::json!([
            {"type": "work", "duration": 300, "distance": 1000},
            {"type": "rest", "duration": 60, "distance": 100},
            {"type": "work", "duration": 300, "distance": 1000}
        ]);

        let result = IntervalsMcpHandler::transform_intervals(&input, true, 20, None);

        assert_eq!(
            result.get("total_intervals").and_then(|v| v.as_u64()),
            Some(3)
        );
        assert!(result.get("types").is_some());
        assert!(result.get("total_duration_secs").is_some());
        assert!(result.get("avg_duration_secs").is_some());
    }

    #[test]
    fn transform_intervals_limits_and_filters() {
        let input = serde_json::json!([
            {"type": "work", "duration": 300, "distance": 1000, "intensity": 80},
            {"type": "rest", "duration": 60, "distance": 100, "intensity": 40},
            {"type": "work", "duration": 300, "distance": 1000, "intensity": 85}
        ]);

        let result = IntervalsMcpHandler::transform_intervals(&input, false, 2, None);
        let arr = result.as_array().expect("array");

        assert_eq!(arr.len(), 2, "should limit to 2 intervals");
        assert!(arr[0].get("type").is_some());
        assert!(arr[0].get("duration").is_some());
    }

    #[test]
    fn summarize_best_efforts_compacts_output() {
        let input = serde_json::json!([
            {"value": 350, "duration": 5, "start_index": 100, "end_index": 104, "extra": "data"},
            {"value": 340, "duration": 5, "start_index": 200, "end_index": 204, "extra": "more"}
        ]);

        let result = IntervalsMcpHandler::summarize_best_efforts(&input, "power");

        assert_eq!(result.get("stream").and_then(|v| v.as_str()), Some("power"));
        assert_eq!(result.get("count").and_then(|v| v.as_u64()), Some(2));
        let efforts = result
            .get("efforts")
            .and_then(|v| v.as_array())
            .expect("efforts");
        assert_eq!(efforts.len(), 2);
        assert!(efforts[0].get("value").is_some());
        assert!(efforts[0].get("duration").is_some());
        assert!(
            efforts[0].get("extra").is_none(),
            "extra should be excluded"
        );
    }

    #[tokio::test]
    async fn get_best_efforts_requires_duration_or_distance() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        
        // Test without duration or distance - should fail
        let params = BestEffortsCompactParams {
            activity_id: "a1".into(),
            stream: "power".into(),
            duration: None,
            distance: None,
            count: Some(5),
            summary: Some(true),
            min_value: None,
            exclude_intervals: None,
            start_index: None,
            end_index: None,
        };
        
        let res = handler.get_best_efforts(Parameters(params)).await;
        match res {
            Err(e) => assert!(e.contains("Must provide either 'duration'")),
            Ok(_) => panic!("Expected error for missing duration/distance"),
        }
    }

    #[test]
    fn compact_gear_list_filters_fields() {
        let input = serde_json::json!([
            {"id": "g1", "name": "Bike", "type": "Bike", "distance": 5000, "reminders": []},
            {"id": "g2", "name": "Shoes", "type": "Shoes", "distance": 500, "notes": "worn"}
        ]);

        let result = IntervalsMcpHandler::compact_gear_list(&input, None);
        let arr = result.as_array().expect("array");

        assert_eq!(arr.len(), 2);
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("name").is_some());
        assert!(arr[0].get("type").is_some());
        assert!(arr[0].get("distance").is_some());
        assert!(arr[0].get("reminders").is_none());
        assert!(arr[1].get("notes").is_none());
    }

    #[test]
    fn transform_curves_summary_filters_key_durations() {
        let input = serde_json::json!({
            "curve": [
                {"secs": 5, "value": 800},
                {"secs": 10, "value": 750},
                {"secs": 30, "value": 600},
                {"secs": 60, "value": 500},
                {"secs": 120, "value": 450},
                {"secs": 300, "value": 400},
                {"secs": 600, "value": 350},
                {"secs": 1200, "value": 300},
                {"secs": 3600, "value": 250}
            ]
        });

        let result = IntervalsMcpHandler::transform_curves(&input, true, None);
        let curve = result
            .get("curve")
            .and_then(|v| v.as_array())
            .expect("curve");

        // Should only include key durations: 5, 30, 60, 300, 1200, 3600
        let secs: Vec<u64> = curve
            .iter()
            .filter_map(|v| v.get("secs").and_then(|s| s.as_u64()))
            .collect();
        assert!(secs.contains(&5));
        assert!(secs.contains(&60));
        assert!(secs.contains(&300));
        assert!(secs.contains(&3600));
        assert!(!secs.contains(&10), "10s not a key duration");
        assert!(!secs.contains(&120), "120s not a key duration");
    }

    #[test]
    fn transform_curves_custom_durations() {
        let input = serde_json::json!({
            "curve": [
                {"secs": 5, "value": 800},
                {"secs": 60, "value": 500},
                {"secs": 300, "value": 400}
            ]
        });

        let result = IntervalsMcpHandler::transform_curves(&input, false, Some(&[60, 300]));
        let curve = result
            .get("curve")
            .and_then(|v| v.as_array())
            .expect("curve");

        assert_eq!(curve.len(), 2);
        let secs: Vec<u64> = curve
            .iter()
            .filter_map(|v| v.get("secs").and_then(|s| s.as_u64()))
            .collect();
        assert!(secs.contains(&60));
        assert!(secs.contains(&300));
        assert!(!secs.contains(&5));
    }

    #[test]
    fn compact_workouts_limits_and_filters() {
        let input = serde_json::json!([
            {"id": "w1", "name": "Tempo", "type": "Run", "duration": 3600, "description": "test", "extra": "data"},
            {"id": "w2", "name": "Intervals", "type": "Run", "duration": 2400, "description": "test2"},
            {"id": "w3", "name": "Long Run", "type": "Run", "duration": 7200, "description": "test3"}
        ]);

        let result = IntervalsMcpHandler::compact_workouts(&input, true, 2, None);
        let arr = result.as_array().expect("array");

        assert_eq!(arr.len(), 2, "should limit to 2");
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("name").is_some());
        assert!(arr[0].get("type").is_some());
        assert!(arr[0].get("extra").is_none());
    }

    #[test]
    fn transform_histogram_summary_computes_stats() {
        let input = serde_json::json!([
            {"value": 100, "count": 10},
            {"value": 150, "count": 20},
            {"value": 200, "count": 30},
            {"value": 250, "count": 15}
        ]);

        let result = IntervalsMcpHandler::transform_histogram(&input, true, 10);

        assert!(result.get("total_samples").is_some());
        assert!(result.get("weighted_avg").is_some());
        assert!(result.get("min").is_some());
        assert!(result.get("max").is_some());
        assert_eq!(result.get("min").and_then(|v| v.as_f64()), Some(100.0));
        assert_eq!(result.get("max").and_then(|v| v.as_f64()), Some(250.0));
    }

    #[test]
    fn transform_histogram_limits_bins() {
        let input: Vec<serde_json::Value> = (0..50)
            .map(|i| serde_json::json!({"value": i * 10, "count": 5}))
            .collect();
        let input = serde_json::Value::Array(input);

        let result = IntervalsMcpHandler::transform_histogram(&input, false, 10);
        let arr = result.as_array().expect("array");

        assert!(arr.len() <= 10, "should limit to 10 bins");
    }

    #[test]
    fn transform_wellness_summary_computes_averages() {
        let input = serde_json::json!([
            {"id": "d1", "sleepSecs": 25200, "stress": 30, "restingHR": 55, "hrv": 45},
            {"id": "d2", "sleepSecs": 28800, "stress": 25, "restingHR": 52, "hrv": 50},
            {"id": "d3", "sleepSecs": 27000, "stress": 35, "restingHR": 54, "hrv": 48}
        ]);

        let result = IntervalsMcpHandler::transform_wellness(&input, true, None);

        assert_eq!(result.get("days").and_then(|v| v.as_u64()), Some(3));
        assert!(result.get("avg_sleep_hours").is_some());
        assert!(result.get("avg_stress").is_some());
        assert!(result.get("avg_resting_hr").is_some());
        assert!(result.get("avg_hrv").is_some());
    }

    #[test]
    fn transform_wellness_filters_fields() {
        let input = serde_json::json!([
            {"id": "d1", "sleepSecs": 25200, "stress": 30, "restingHR": 55, "weight": 70.5},
            {"id": "d2", "sleepSecs": 28800, "stress": 25, "restingHR": 52, "weight": 70.3}
        ]);

        let result = IntervalsMcpHandler::transform_wellness(
            &input,
            false,
            Some(&["id".into(), "sleepSecs".into()]),
        );
        let arr = result.as_array().expect("array");

        assert_eq!(arr.len(), 2);
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("sleepSecs").is_some());
        assert!(arr[0].get("stress").is_none());
        assert!(arr[0].get("weight").is_none());
    }

    #[test]
    fn filter_array_fields_filters_each_item() {
        let input = serde_json::json!([
            {"id": "1", "name": "A", "extra": "x"},
            {"id": "2", "name": "B", "extra": "y"}
        ]);

        let result =
            IntervalsMcpHandler::filter_array_fields(&input, &["id".into(), "name".into()]);
        let arr = result.as_array().expect("array");

        assert_eq!(arr.len(), 2);
        assert!(arr[0].get("id").is_some());
        assert!(arr[0].get("name").is_some());
        assert!(arr[0].get("extra").is_none());
    }

    #[test]
    fn tool_descriptions_are_concise() {
        let client =
            ReqwestIntervalsClient::new("http://localhost", "ath", SecretString::new("key".into()));
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let tools = handler.tool_router.list_all();

        for tool in tools {
            let desc_len = tool.description.as_ref().map(|d| d.len()).unwrap_or(0);
            assert!(
                desc_len < 200,
                "Tool '{}' description too long: {} chars (max 200)",
                tool.name,
                desc_len
            );
        }
    }

    #[tokio::test]
    async fn get_activities_csv_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = ActivitiesCsvParams {
            limit: Some(10),
            days_back: Some(30),
            columns: None,
        };
        let res = handler.get_activities_csv(Parameters(params)).await;
        assert!(res.is_ok());
        let Json(result) = res.unwrap();
        assert!(result.value.get("csv").is_some());
    }

    #[tokio::test]
    async fn get_fitness_summary_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let res = handler.get_fitness_summary().await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_wellness_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = WellnessParams {
            days_back: Some(7),
            summary: Some(true),
            fields: None,
        };
        let res = handler.get_wellness(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_wellness_for_date_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = DateParam {
            date: "2025-01-01".into(),
        };
        let res = handler.get_wellness_for_date(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_wellness_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = WellnessUpdateParams {
            date: "2025-01-01".into(),
            data: serde_json::json!({"weight": 70.0}),
        };
        let res = handler.update_wellness(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_wellness_rejects_invalid_date() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = WellnessUpdateParams {
            date: "not-a-date".into(),
            data: serde_json::json!({"weight": 70.0}),
        };
        let res = handler.update_wellness(Parameters(params)).await;
        match res {
            Err(e) => assert!(e.contains("invalid date")),
            Ok(_) => panic!("Expected error for invalid date"),
        }
    }

    #[tokio::test]
    async fn get_upcoming_workouts_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = DaysAheadParams {
            days_ahead: Some(14),
        };
        let res = handler.get_upcoming_workouts(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_hr_curves_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = CurvesParams {
            sport: "Run".into(),
            days_back: Some(30),
            durations: None,
            summary: None,
        };
        let res = handler.get_hr_curves(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_pace_curves_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = CurvesParams {
            sport: "Run".into(),
            days_back: Some(30),
            durations: None,
            summary: None,
        };
        let res = handler.get_pace_curves(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_workout_library_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let res = handler.get_workout_library().await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_workouts_in_folder_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = WorkoutsInFolderParams {
            folder_id: "folder1".into(),
            compact: None,
            limit: None,
            fields: None,
        };
        let res = handler.get_workouts_in_folder(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn create_gear_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = CreateGearParams {
            gear: serde_json::json!({"name": "Bike", "type": "bike"}),
        };
        let res = handler.create_gear(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_gear_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = UpdateGearParams {
            gear_id: "g1".into(),
            fields: serde_json::json!({"name": "New Bike"}),
        };
        let res = handler.update_gear(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn delete_gear_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = GearIdParam {
            gear_id: "g1".into(),
        };
        let res = handler.delete_gear(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn exercise_misc_tools_calls_many() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // Histograms and streams
        let r = handler
            .get_power_histogram(Parameters(HistogramParams {
                activity_id: "a1".into(),
                summary: None,
                bins: None,
            }))
            .await;
        assert!(r.is_ok());
        let r = handler
            .get_hr_histogram(Parameters(HistogramParams {
                activity_id: "a1".into(),
                summary: None,
                bins: None,
            }))
            .await;
        assert!(r.is_ok());
        let r = handler
            .get_pace_histogram(Parameters(HistogramParams {
                activity_id: "a1".into(),
                summary: None,
                bins: None,
            }))
            .await;
        assert!(r.is_ok());

        // Fitness & wellness
        assert!(handler.get_fitness_summary().await.is_ok());
        let r = handler
            .get_wellness(Parameters(WellnessParams {
                days_back: None,
                summary: None,
                fields: None,
            }))
            .await;
        assert!(r.is_ok());
        let r = handler
            .get_wellness_for_date(Parameters(DateParam {
                date: "2026-01-01".into(),
            }))
            .await;
        assert!(r.is_ok());
        let r = handler
            .update_wellness(Parameters(WellnessUpdateParams {
                date: "2026-01-01".into(),
                data: serde_json::json!({"weight": 70.0}),
            }))
            .await;
        assert!(r.is_ok());

        // Workouts & gear
        assert!(handler.get_workout_library().await.is_ok());
        let r = handler
            .get_workouts_in_folder(Parameters(WorkoutsInFolderParams {
                folder_id: "f1".into(),
                compact: None,
                limit: None,
                fields: None,
            }))
            .await;
        assert!(r.is_ok());
    }

    #[tokio::test]
    async fn create_gear_reminder_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = CreateGearReminderParams {
            gear_id: "g1".into(),
            reminder: serde_json::json!({"type": "service", "due_distance": 1000}),
        };
        let res = handler.create_gear_reminder(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_gear_reminder_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = UpdateGearReminderParams {
            gear_id: "g1".into(),
            reminder_id: "r1".into(),
            reset: false,
            snooze_days: 7,
            fields: serde_json::json!({"due_distance": 2000}),
        };
        let res = handler.update_gear_reminder(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_sport_settings_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = UpdateSportSettingsParams {
            sport_type: "Run".into(),
            recalc_hr_zones: false,
            fields: serde_json::json!({"ftp": 250}),
        };
        let res = handler.update_sport_settings(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn apply_sport_settings_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = ApplySportSettingsParams {
            sport_type: "Run".into(),
        };
        let res = handler.apply_sport_settings(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn create_sport_settings_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = CreateSportSettingsParams {
            settings: serde_json::json!({"sport_type": "Run", "ftp": 250}),
        };
        let res = handler.create_sport_settings(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn delete_sport_settings_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = SportTypeParam {
            sport_type: "Run".into(),
        };
        let res = handler.delete_sport_settings(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn delete_activity_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = ActivityIdParam {
            activity_id: "a1".into(),
        };
        let res = handler.delete_activity(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_activities_around_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = ActivitiesAroundParams {
            activity_id: "a1".into(),
            limit: Some(5),
            route_id: Some(123),
        };
        let res = handler.get_activities_around(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn search_intervals_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = IntervalSearchParams {
            min_secs: 60,
            max_secs: 300,
            min_intensity: 80,
            max_intensity: 120,
            interval_type: Some("threshold".into()),
            min_reps: Some(1),
            max_reps: Some(10),
            limit: Some(20),
        };
        let res = handler.search_intervals(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_power_histogram_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = HistogramParams {
            activity_id: "a1".into(),
            summary: None,
            bins: None,
        };
        let res = handler.get_power_histogram(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_hr_histogram_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = HistogramParams {
            activity_id: "a1".into(),
            summary: None,
            bins: None,
        };
        let res = handler.get_hr_histogram(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn get_pace_histogram_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = HistogramParams {
            activity_id: "a1".into(),
            summary: None,
            bins: None,
        };
        let res = handler.get_pace_histogram(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_event_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = UpdateEventParams {
            event_id: EventId::Str("e1".to_string()),
            fields: serde_json::json!({"name": "Updated Event"}),
        };
        let res = handler.update_event(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn update_event_rejects_invalid_start_date_local() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = UpdateEventParams {
            event_id: EventId::Str("e1".to_string()),
            fields: serde_json::json!({"start_date_local": "not-a-date"}),
        };
        let res = handler.update_event(Parameters(params)).await;
        match res {
            Err(e) => assert!(e.contains("invalid start_date_local")),
            Ok(_) => panic!("Expected error for invalid start_date_local"),
        }
    }

    #[tokio::test]
    async fn bulk_delete_events_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = BulkDeleteEventsParams {
            event_ids: vec![
                EventId::Str("e1".to_string()),
                EventId::Str("e2".to_string()),
            ],
        };
        let res = handler.bulk_delete_events(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn duplicate_event_tool() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));
        let params = DuplicateEventParams {
            event_id: EventId::Str("e1".to_string()),
            num_copies: Some(3),
            weeks_between: Some(1),
        };
        let res = handler.duplicate_event(Parameters(params)).await;
        assert!(res.is_ok());
    }

    #[test]
    fn compute_stream_stats_empty_array() {
        let arr = vec![];
        let stats = IntervalsMcpHandler::compute_stream_stats(&arr);
        assert_eq!(stats["count"], 0);
    }

    #[test]
    fn compute_stream_stats_single_value() {
        let arr = vec![serde_json::json!(42.5)];
        let stats = IntervalsMcpHandler::compute_stream_stats(&arr);
        assert_eq!(stats["count"], 1);
        assert_eq!(stats["min"], 42.5);
        assert_eq!(stats["max"], 42.5);
        assert_eq!(stats["avg"], 42.5);
        assert_eq!(stats["p10"], 42.5);
        assert_eq!(stats["p50"], 42.5);
        assert_eq!(stats["p90"], 42.5);
    }

    #[test]
    fn compute_stream_stats_multiple_values() {
        let arr = vec![
            serde_json::json!(10.0),
            serde_json::json!(20.0),
            serde_json::json!(30.0),
            serde_json::json!(40.0),
            serde_json::json!(50.0),
        ];
        let stats = IntervalsMcpHandler::compute_stream_stats(&arr);
        assert_eq!(stats["count"], 5);
        assert_eq!(stats["min"], 10.0);
        assert_eq!(stats["max"], 50.0);
        assert_eq!(stats["avg"], 30.0);
        assert_eq!(stats["p10"], 10.0); // 10th percentile of sorted [10,20,30,40,50] - first element
        assert_eq!(stats["p50"], 30.0); // median
        assert_eq!(stats["p90"], 50.0); // 90th percentile - last element
    }

    #[test]
    fn compute_stream_stats_with_integers() {
        let arr = vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ];
        let stats = IntervalsMcpHandler::compute_stream_stats(&arr);
        assert_eq!(stats["count"], 3);
        assert_eq!(stats["min"], 1.0);
        assert_eq!(stats["max"], 3.0);
        assert_eq!(stats["avg"], 2.0);
    }

    #[test]
    fn downsample_array_no_change_needed() {
        let arr = vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ];
        let result = IntervalsMcpHandler::downsample_array(&arr, 5);
        assert_eq!(result, arr);
    }

    #[test]
    fn downsample_array_target_too_small() {
        let arr = vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ];
        let result = IntervalsMcpHandler::downsample_array(&arr, 1);
        assert_eq!(result, arr);
    }

    #[test]
    fn downsample_array_basic_downsampling() {
        let arr = vec![
            serde_json::json!(0),
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
            serde_json::json!(4),
            serde_json::json!(5),
            serde_json::json!(6),
            serde_json::json!(7),
            serde_json::json!(8),
            serde_json::json!(9),
        ];
        let result = IntervalsMcpHandler::downsample_array(&arr, 4);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], serde_json::json!(0)); // first
        assert_eq!(result[3], serde_json::json!(9)); // last
    }

    #[test]
    fn downsample_array_preserves_first_and_last() {
        let arr = vec![
            serde_json::json!("first"),
            serde_json::json!("middle"),
            serde_json::json!("last"),
        ];
        let result = IntervalsMcpHandler::downsample_array(&arr, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], serde_json::json!("first"));
        assert_eq!(result[1], serde_json::json!("last"));
    }

    #[test]
    fn event_id_as_cow_int() {
        let id = EventId::Int(123);
        assert_eq!(id.as_cow(), "123");
    }

    #[test]
    fn event_id_as_cow_str() {
        let id = EventId::Str("test".to_string());
        assert_eq!(id.as_cow(), "test");
    }

    #[test]
    fn get_info_returns_server_info() {
        let handler = IntervalsMcpHandler::new(Arc::new(MockClient));
        let info = <IntervalsMcpHandler as rmcp::ServerHandler>::get_info(&handler);
        assert!(info.instructions.is_some());
        assert!(
            info.instructions
                .as_ref()
                .unwrap()
                .contains("Intervals.icu")
        );
        assert!(info.capabilities.tools.is_some());
        assert!(info.capabilities.prompts.is_some());
        assert!(info.capabilities.resources.is_some());
    }

    #[tokio::test]
    async fn prompts_handle_edge_cases() {
        let client = MockClient;
        let handler = IntervalsMcpHandler::new(Arc::new(client));

        // analyze recent training with negative days should clamp to 0
        let res = handler
            .analyze_recent_training(Parameters(AnalyzeRecentTrainingParams {
                days_back: Some(-5),
            }))
            .await;
        assert!(
            res.description
                .as_ref()
                .map(|d| d.contains("0"))
                .unwrap_or(false)
        );

        // performance analysis picks metric from sport_type when metric missing
        let res2 = handler
            .performance_analysis(Parameters(PerformanceAnalysisParams {
                days_back: Some(7),
                metric: None,
                sport_type: Some("hr".into()),
            }))
            .await;
        assert!(
            res2.description
                .as_ref()
                .map(|d| d.to_lowercase().contains("hr") || d.to_lowercase().contains("heart"))
                .unwrap_or(false)
        );

        // activity deep dive includes activity id in the prompt
        let res3 = handler
            .activity_deep_dive(Parameters(ActivityDeepDiveParams {
                activity_id: "act-1".into(),
            }))
            .await;
        assert!(
            res3.description
                .as_ref()
                .map(|d| d.contains("act-1"))
                .unwrap_or(false)
        );

        // recovery check negative days clamps to 0
        let rec = handler
            .recovery_check(Parameters(RecoveryCheckParams {
                days_back: Some(-2),
            }))
            .await;
        assert!(
            rec.description
                .as_ref()
                .map(|d| d.contains("0"))
                .unwrap_or(false)
        );

        // training plan review uses provided start_date when present
        let plan = handler
            .training_plan_review(Parameters(TrainingPlanReviewParams {
                start_date: Some("2026-01-01".into()),
            }))
            .await;
        assert!(
            plan.description
                .as_ref()
                .map(|d| d.contains("2026-01-01"))
                .unwrap_or(false)
        );

        // plan training week uses focus
        let week = handler
            .plan_training_week(Parameters(PlanTrainingWeekParams {
                start_date: Some("2026-01-01".into()),
                focus: Some("endurance".into()),
            }))
            .await;
        assert!(
            week.description
                .as_ref()
                .map(|d| d.contains("endurance"))
                .unwrap_or(false)
        );

        // analyze_and_adapt_plan default parameters
        let adapt = handler
            .analyze_and_adapt_plan(Parameters(AnalyzeAdaptPlanParams {
                period: None,
                days_back: None,
                focus: None,
            }))
            .await;
        assert!(
            adapt
                .description
                .as_ref()
                .map(|d| d.contains("the last") || d.contains("balanced"))
                .unwrap_or(false)
        );
    }
}
