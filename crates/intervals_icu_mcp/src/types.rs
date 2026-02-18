use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{DownloadStatus, EventId};

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
    /// Return compact summaries for created events (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return per event (default: id,name,start_date_local,category,type)
    pub fields: Option<Vec<String>>,
}

/// Parameters for create_event with optional response filtering
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateEventParams {
    #[serde(flatten)]
    pub event: intervals_icu_client::Event,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,start_date_local,category,type,description)
    pub fields: Option<Vec<String>>,
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
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return per interval (default: type,start,end,duration,intensity)
    pub fields: Option<Vec<String>>,
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

/// Parameters for get_workout_library with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WorkoutLibraryParams {
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,description)
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

/// Parameters for get_sport_settings with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SportSettingsParams {
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific sport types to return (default: all)
    pub sports: Option<Vec<String>>,
    /// Specific fields to return per sport (default: type,ftp,fthr,hrzones,powerzones)
    pub fields: Option<Vec<String>>,
}

/// Parameters for get_fitness_summary with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FitnessSummaryParams {
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: ctl,atl,tsb,ctl_ramp_rate,atl_ramp_rate,date)
    pub fields: Option<Vec<String>>,
}

/// Parameters for get_events with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct EventsParams {
    /// Days back (default: 30)
    pub days_back: Option<i32>,
    /// Maximum events to return (default: 50)
    pub limit: Option<u32>,
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,start_date_local,name,category)
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateActivityParams {
    pub activity_id: String,
    pub fields: serde_json::Value,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,start_date_local,type,distance,moving_time)
    pub response_fields: Option<Vec<String>>,
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
    /// Maximum activities to return (default: 10)
    pub limit: Option<u32>,
    pub route_id: Option<i64>,
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,start_date_local,type,distance,moving_time)
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DateParam {
    pub date: String,
}

/// Parameters for get_wellness_for_date with compact mode
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WellnessDateParams {
    pub date: String,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,sleepSecs,stress,restingHR,hrv,weight,fatigue,motivation)
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct WellnessUpdateParams {
    pub date: String,
    pub data: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DaysAheadParams {
    pub days_ahead: Option<u32>,
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Maximum workouts to return (default: 20)
    pub limit: Option<u32>,
    /// Specific fields to return (default: id,name,start_date_local,category,type)
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateEventParams {
    pub event_id: EventId,
    pub fields: serde_json::Value,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,start_date_local,category,type,description)
    pub response_fields: Option<Vec<String>>,
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
    /// Return compact summaries (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return per event (default: id,name,start_date_local,category,type)
    pub fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct FolderIdParam {
    pub folder_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateFolderParams {
    /// Folder data (name, description, etc.)
    pub folder: serde_json::Value,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,description)
    pub response_fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateFolderParams {
    pub folder_id: String,
    pub fields: serde_json::Value,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,description)
    pub response_fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteFolderParams {
    pub folder_id: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct CreateGearParams {
    pub gear: serde_json::Value,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,type,distance)
    pub response_fields: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct UpdateGearParams {
    pub gear_id: String,
    pub fields: serde_json::Value,
    /// Return compact summary (default: true)
    pub compact: Option<bool>,
    /// Specific fields to return (default: id,name,type,distance)
    pub response_fields: Option<Vec<String>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_event_params_accept_numeric_event_id() {
        let json = serde_json::json!({
            "event_id": 123,
            "fields": {"name": "x"}
        });
        let params: UpdateEventParams = serde_json::from_value(json).expect("should parse");
        assert_eq!(params.event_id.as_cow(), "123");
    }

    #[test]
    fn search_params_accept_q_alias_query() {
        let json = serde_json::json!({"query": "run"});
        let params: SearchParams = serde_json::from_value(json).expect("should parse");
        assert_eq!(params.q, "run");
    }
}
