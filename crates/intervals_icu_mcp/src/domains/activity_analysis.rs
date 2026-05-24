//! Domain module for activity analysis transformations.
//!
//! This module handles transformation of activity data including:
//! - Stream data downsampling and statistics
//! - Interval summarization
//! - Best efforts compaction
//! - Power/HR/pace curve analysis
//! - Histogram transformations
//!
//! # GRASP Principles
//! - **Information Expert**: Activity analysis logic is encapsulated here
//! - **Low Coupling**: Minimal dependencies on other modules
//! - **High Cohesion**: Focused on activity data transformations only

use std::collections::HashMap;

use serde_json::{Map, Value};

pub fn transform_streams(
    value: Value,
    max_points: Option<u32>,
    summary_only: bool,
    filter_streams: Option<Vec<String>>,
) -> Value {
    let Some(obj) = value.as_object() else {
        return value;
    };

    let mut result = Map::new();

    for (key, val) in obj {
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
            result.insert(key.clone(), compute_stream_stats(arr));
        } else if let Some(max) = max_points {
            result.insert(
                key.clone(),
                Value::Array(downsample_array(arr, max as usize)),
            );
        } else {
            result.insert(key.clone(), val.clone());
        }
    }

    Value::Object(result)
}

pub fn compute_stream_stats(arr: &[Value]) -> Value {
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
    let min = nums.iter().copied().fold(f64::INFINITY, f64::min);
    let max = nums.iter().copied().fold(f64::NEG_INFINITY, f64::max);

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

pub fn downsample_array(arr: &[Value], target: usize) -> Vec<Value> {
    let len = arr.len();
    if len <= target || target < 2 {
        return arr.to_vec();
    }

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

pub fn transform_intervals(
    value: &Value,
    summary_only: bool,
    max_intervals: usize,
    fields: Option<&[String]>,
) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    if summary_only {
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

    let limited: Vec<Value> = arr
        .iter()
        .take(max_intervals)
        .map(|item| {
            let Some(obj) = item.as_object() else {
                return item.clone();
            };
            let mut result = Map::new();
            for field in &fields_to_use {
                if let Some(val) = obj.get(*field) {
                    result.insert(field.to_string(), val.clone());
                }
            }
            Value::Object(result)
        })
        .collect();

    Value::Array(limited)
}

pub fn compact_intervals(value: &Value, fields: Option<&[String]>) -> Value {
    let default_fields = [
        "type",
        "start",
        "end",
        "duration",
        "intensity",
        "activity_id",
    ];
    let fields_to_use: Vec<&str> = fields
        .map(|f| f.iter().map(|s| s.as_str()).collect())
        .unwrap_or_else(|| default_fields.to_vec());

    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let compacted: Vec<Value> = arr
        .iter()
        .map(|item| {
            let Some(obj) = item.as_object() else {
                return item.clone();
            };
            let mut result = Map::new();
            for field in &fields_to_use {
                if let Some(val) = obj.get(*field) {
                    result.insert(field.to_string(), val.clone());
                }
            }
            Value::Object(result)
        })
        .collect();

    Value::Array(compacted)
}

pub fn summarize_best_efforts(value: &Value, stream: &str) -> Value {
    let Some(arr) = value.as_array() else {
        return value.clone();
    };

    let efforts: Vec<Value> = arr
        .iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let mut compact = Map::new();

            if let Some(v) = obj.get("value") {
                compact.insert("value".to_string(), v.clone());
            }
            if let Some(v) = obj.get("duration") {
                compact.insert("duration".to_string(), v.clone());
            }
            if let Some(v) = obj.get("start_index") {
                compact.insert("start_index".to_string(), v.clone());
            }

            Some(Value::Object(compact))
        })
        .collect();

    serde_json::json!({
        "stream": stream,
        "count": efforts.len(),
        "efforts": efforts
    })
}

pub fn transform_curves(value: &Value, summary_only: bool, durations: Option<&[u32]>) -> Value {
    const KEY_DURATIONS: [u32; 6] = [5, 30, 60, 300, 1200, 3600];
    let dur_filter: &[u32] = match durations {
        Some(d) => d,
        None if summary_only => &KEY_DURATIONS,
        None => return value.clone(),
    };

    let Some(obj) = value.as_object() else {
        return value.clone();
    };

    let mut result = Map::new();
    for (key, val) in obj {
        if let Some(arr) = val.as_array() {
            // Detect parallel-arrays format by checking whether the first non-empty item's
            // "secs" field is itself an array.  Empty arrays are treated as scalar-secs
            // format (no-op: the filtered result will also be empty).
            let is_parallel = arr
                .first()
                .and_then(|item| item.get("secs"))
                .map(|s| s.is_array())
                .unwrap_or(false);

            if is_parallel {
                let filtered: Vec<Value> = arr
                    .iter()
                    .flat_map(|item| extract_parallel_curve_points(item, dur_filter))
                    .collect();
                result.insert(key.clone(), Value::Array(filtered));
            } else {
                // Original scalar-secs format
                let filtered: Vec<Value> = arr
                    .iter()
                    .filter(|item| {
                        item.get("secs")
                            .and_then(|s| s.as_u64())
                            .map(|s| dur_filter.contains(&(s as u32)))
                            .unwrap_or(false)
                    })
                    .cloned()
                    .collect();
                result.insert(key.clone(), Value::Array(filtered));
            }
        } else {
            result.insert(key.clone(), val.clone());
        }
    }
    Value::Object(result)
}

/// Extract curve data points at specific durations from an item using the parallel-arrays format.
///
/// An item looks like `{"secs": [1,2,5,30,...], "watts": [350,320,280,...], ...}`.
/// Returns one `{"secs": N, "watts": V, ...}` object for each requested duration that
/// has an exact match in the `secs` array.
///
/// A `HashMap` is built from the `secs` array once so that lookups are O(1) instead of
/// performing a linear scan for every requested duration.
fn extract_parallel_curve_points(item: &Value, dur_filter: &[u32]) -> Vec<Value> {
    let Some(obj) = item.as_object() else {
        return vec![];
    };
    let Some(secs_arr) = obj.get("secs").and_then(|v| v.as_array()) else {
        return vec![];
    };

    // Build a duration→index map once for O(1) lookups
    let secs_index: HashMap<u32, usize> = secs_arr
        .iter()
        .enumerate()
        .filter_map(|(idx, v)| v.as_u64().map(|n| (n as u32, idx)))
        .collect();

    dur_filter
        .iter()
        .filter_map(|&dur| {
            let idx = *secs_index.get(&dur)?;

            let mut point = Map::new();
            point.insert("secs".to_string(), Value::from(dur));
            for (k, v) in obj {
                if k == "secs" {
                    continue;
                }
                if let Some(stream_arr) = v.as_array()
                    && let Some(val) = stream_arr.get(idx)
                {
                    point.insert(k.clone(), val.clone());
                }
            }
            Some(Value::Object(point))
        })
        .collect()
}

pub fn transform_histogram(value: &Value, summary_only: bool, max_bins: usize) -> Value {
    if summary_only && let Some(arr) = value.as_array() {
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

    if let Some(arr) = value.as_array()
        && arr.len() > max_bins
    {
        let step = arr.len() / max_bins;
        let sampled: Vec<Value> = arr
            .iter()
            .step_by(step.max(1))
            .take(max_bins)
            .cloned()
            .collect();
        return Value::Array(sampled);
    }

    value.clone()
}

// =============================================================================
// P3.3 — Ultra-Specific Context Tokens
// =============================================================================

/// Compute back-to-back load from consecutive-day training loads.
/// Returns the sum of the two highest consecutive-day load pairs.
pub fn back_to_back_load(daily_loads: &[f64]) -> f64 {
    daily_loads
        .windows(2)
        .map(|w| w[0] + w[1])
        .fold(f64::NEG_INFINITY, f64::max)
        .max(0.0)
}

/// Compute weekly vertical gain from activity details.
pub fn vert_per_week(activity_details: &[&serde_json::Map<String, Value>]) -> f64 {
    activity_details
        .iter()
        .filter_map(|detail| detail.get("total_elevation_gain").and_then(Value::as_f64))
        .sum()
}

/// Compute longest run ratio = longest_run / weekly_volume.
/// ratio_km for distance-based, ratio_hrs for time-based.
pub fn longest_run_ratio(longest_run_km: f64, weekly_volume_km: f64) -> Option<(f64, f64)> {
    if weekly_volume_km <= 0.0 {
        return None;
    }
    let ratio_km = longest_run_km / weekly_volume_km;
    // Approx: for 10 km/h average pace, ratio_hrs ≈ ratio_km
    // Koop 2026: 4-7% in hours, 25-35% in km
    Some((ratio_km, ratio_km * 0.16)) // rough hours conversion
}

/// Compute elevation specificity score — how well training elevation profile
/// matches target race profile. Returns 0.0 (no match) to 1.0 (perfect match).
pub fn elevation_specificity_score(training_vert_per_km: f64, race_vert_per_km: f64) -> f64 {
    if race_vert_per_km <= 0.0 {
        return 1.0;
    }
    let ratio = training_vert_per_km / race_vert_per_km;
    if (0.8..=1.2).contains(&ratio) {
        1.0
    } else if (0.5..=1.5).contains(&ratio) {
        0.6
    } else {
        0.2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_stats_empty_returns_zero_count() {
        let out = compute_stream_stats(&[]);
        assert_eq!(out["count"], 0);
    }

    #[test]
    fn downsample_preserves_edges() {
        let arr = vec![1, 2, 3, 4, 5]
            .into_iter()
            .map(Value::from)
            .collect::<Vec<_>>();
        let out = downsample_array(&arr, 3);
        assert_eq!(out.first(), Some(&Value::from(1)));
        assert_eq!(out.last(), Some(&Value::from(5)));
    }

    #[test]
    fn summarize_best_efforts_keeps_compact_shape() {
        let input = serde_json::json!([
            {"value": 300, "duration": 60, "start_index": 100, "ignored": true}
        ]);
        let out = summarize_best_efforts(&input, "power");
        assert_eq!(out["stream"], "power");
        assert_eq!(out["count"], 1);
        assert!(out["efforts"][0].get("ignored").is_none());
    }

    #[test]
    fn compute_stream_stats_single_value() {
        let arr = vec![serde_json::json!(42.5)];
        let stats = compute_stream_stats(&arr);
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
        let stats = compute_stream_stats(&arr);
        assert_eq!(stats["count"], 5);
        assert_eq!(stats["min"], 10.0);
        assert_eq!(stats["max"], 50.0);
        assert_eq!(stats["avg"], 30.0);
        assert_eq!(stats["p10"], 10.0);
        assert_eq!(stats["p50"], 30.0);
        assert_eq!(stats["p90"], 50.0);
    }

    #[test]
    fn compute_stream_stats_with_integers() {
        let arr = vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ];
        let stats = compute_stream_stats(&arr);
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
        let result = downsample_array(&arr, 5);
        assert_eq!(result, arr);
    }

    #[test]
    fn downsample_array_target_too_small() {
        let arr = vec![
            serde_json::json!(1),
            serde_json::json!(2),
            serde_json::json!(3),
        ];
        let result = downsample_array(&arr, 1);
        assert_eq!(result, arr);
    }

    #[test]
    fn downsample_array_basic_downsampling() {
        let arr = (0..10).map(|i| serde_json::json!(i)).collect::<Vec<_>>();
        let result = downsample_array(&arr, 4);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], serde_json::json!(0));
        assert_eq!(result[3], serde_json::json!(9));
    }

    #[test]
    fn downsample_array_preserves_first_and_last() {
        let arr = vec![
            serde_json::json!("first"),
            serde_json::json!("middle"),
            serde_json::json!("last"),
        ];
        let result = downsample_array(&arr, 2);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], serde_json::json!("first"));
        assert_eq!(result[1], serde_json::json!("last"));
    }

    #[test]
    fn transform_curves_parallel_arrays_summary() {
        // Simulate the actual Intervals.icu API response format with parallel arrays
        let input = serde_json::json!({
            "list": [{"secs": [1, 5, 30, 60, 300], "watts": [400, 350, 300, 270, 220]}],
            "activities": {"123": {"name": "Morning Ride"}}
        });
        let out = transform_curves(&input, true, None);
        // Summary mode should extract values at key durations (5, 30, 60, 300 are all present)
        let list = out["list"].as_array().expect("list should be array");
        assert!(!list.is_empty(), "list should not be empty after transform");
        // Check that extracted points have the correct secs values
        let secs_values: Vec<u64> = list.iter().filter_map(|p| p["secs"].as_u64()).collect();
        assert!(secs_values.contains(&5));
        assert!(secs_values.contains(&30));
        assert!(secs_values.contains(&60));
        assert!(secs_values.contains(&300));
        // Verify watts values are extracted correctly
        let watts_at_5 = list
            .iter()
            .find(|p| p["secs"] == 5)
            .and_then(|p| p["watts"].as_u64());
        assert_eq!(watts_at_5, Some(350));
    }

    #[test]
    fn transform_curves_parallel_arrays_custom_durations() {
        let input = serde_json::json!({
            "list": [{"secs": [1, 5, 30, 60], "watts": [400, 350, 300, 270]}],
            "activities": {}
        });
        let out = transform_curves(&input, false, Some(&[5, 60]));
        let list = out["list"].as_array().expect("list should be array");
        assert_eq!(list.len(), 2);
        let secs_values: Vec<u64> = list.iter().filter_map(|p| p["secs"].as_u64()).collect();
        assert!(secs_values.contains(&5));
        assert!(secs_values.contains(&60));
    }

    #[test]
    fn transform_curves_parallel_arrays_unmatched_duration_skipped() {
        let input = serde_json::json!({
            "list": [{"secs": [1, 5, 30], "watts": [400, 350, 300]}],
            "activities": {}
        });
        // Request duration 3600 which is not in secs array
        let out = transform_curves(&input, false, Some(&[5, 3600]));
        let list = out["list"].as_array().expect("list should be array");
        // Only secs=5 matches, secs=3600 is absent → 1 point
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["secs"], 5);
    }

    #[test]
    fn transform_curves_no_summary_returns_clone() {
        let input = serde_json::json!({"list": [], "activities": {}});
        let out = transform_curves(&input, false, None);
        assert_eq!(out, input);
    }

    // ========================================================================
    // transform_streams() Tests
    // ========================================================================

    #[test]
    fn transform_streams_passes_non_object() {
        let v = Value::String("hello".into());
        let out = transform_streams(v.clone(), None, false, None);
        assert_eq!(out, v);
    }

    #[test]
    fn transform_streams_filter_streams_keeps_matching() {
        let v = serde_json::json!({"power": [100, 200], "hr": [60, 70]});
        let out = transform_streams(v, None, false, Some(vec!["power".into()]));
        assert!(out.as_object().unwrap().contains_key("power"));
        assert!(!out.as_object().unwrap().contains_key("hr"));
    }

    #[test]
    fn transform_streams_filter_streams_case_insensitive() {
        let v = serde_json::json!({"Power": [100]});
        let out = transform_streams(v, None, false, Some(vec!["power".into()]));
        assert!(out.as_object().unwrap().contains_key("Power"));
    }

    #[test]
    fn transform_streams_non_array_value_passthrough() {
        let v = serde_json::json!({"name": "test", "power": [100]});
        let out = transform_streams(v, None, false, None);
        assert_eq!(out["name"], "test");
        assert!(out["power"].is_array());
    }

    #[test]
    fn transform_streams_summary_only() {
        let v = serde_json::json!({"power": [100, 200, 300]});
        let out = transform_streams(v, None, true, None);
        let stats = &out["power"];
        assert_eq!(stats["count"], 3);
        assert_eq!(stats["min"], 100.0);
        assert_eq!(stats["max"], 300.0);
    }

    #[test]
    fn transform_streams_with_max_points() {
        let v = serde_json::json!({"power": [0, 1, 2, 3, 4, 5, 6, 7, 8, 9]});
        let out = transform_streams(v, Some(4), false, None);
        let arr = out["power"].as_array().unwrap();
        assert_eq!(arr.len(), 4);
        assert_eq!(arr[0], 0);
        assert_eq!(arr[3], 9);
    }

    #[test]
    fn transform_streams_no_max_points_or_summary() {
        let v = serde_json::json!({"power": [100, 200]});
        let out = transform_streams(v.clone(), None, false, None);
        assert_eq!(out, v);
    }

    // ========================================================================
    // transform_intervals() Tests
    // ========================================================================

    #[test]
    fn transform_intervals_passes_non_array() {
        let v = serde_json::json!({"key": "value"});
        let out = transform_intervals(&v, false, 10, None);
        assert_eq!(out, v);
    }

    #[test]
    fn transform_intervals_summary_with_typed_items() {
        let v = serde_json::json!([
            {"type": "warmup", "duration": 300, "distance": 1000},
            {"type": "warmup", "duration": 120, "distance": 400},
            {"type": "interval", "duration": 60, "distance": 300},
        ]);
        let out = transform_intervals(&v, true, 10, None);
        assert_eq!(out["total_intervals"], 3);
        assert_eq!(out["types"]["warmup"], 2);
        assert_eq!(out["types"]["interval"], 1);
        assert!((out["total_duration_secs"].as_f64().unwrap() - 480.0).abs() < 0.01);
        assert!((out["total_distance_m"].as_f64().unwrap() - 1700.0).abs() < 0.01);
    }

    #[test]
    fn transform_intervals_summary_empty() {
        let v = serde_json::json!([]);
        let out = transform_intervals(&v, true, 10, None);
        assert_eq!(out["total_intervals"], 0);
    }

    #[test]
    fn transform_intervals_summary_missing_fields() {
        let v = serde_json::json!([{"type": "cooldown"}, {"type": "interval", "duration": 60}]);
        let out = transform_intervals(&v, true, 10, None);
        assert_eq!(out["total_intervals"], 2);
        // Second item has no distance, should be fine
        assert!(out["total_distance_m"].as_f64().unwrap().abs() < 0.01);
    }

    #[test]
    fn transform_intervals_with_custom_fields() {
        let v = serde_json::json!([
            {"type": "interval", "start_index": 10, "end_index": 100, "duration": 60, "distance": 400, "intensity": 0.9, "extra": "ignored"}
        ]);
        let custom = vec!["type".to_string(), "intensity".to_string()];
        let out = transform_intervals(&v, false, 10, Some(&custom));
        let item = out.as_array().unwrap()[0].as_object().unwrap();
        assert!(item.contains_key("type"));
        assert!(item.contains_key("intensity"));
        assert!(!item.contains_key("duration"));
        assert!(!item.contains_key("extra"));
    }

    #[test]
    fn transform_intervals_respects_max_intervals() {
        let v = serde_json::json!([
            {"type": "a"}, {"type": "b"}, {"type": "c"}, {"type": "d"}
        ]);
        let out = transform_intervals(&v, false, 2, None);
        assert_eq!(out.as_array().unwrap().len(), 2);
    }

    #[test]
    fn transform_intervals_skips_non_object_items() {
        let v = serde_json::json!(["string", {"type": "a"}, 42]);
        let out = transform_intervals(&v, false, 10, None);
        let items = out.as_array().unwrap();
        // Non-object items pass through
        assert_eq!(items.len(), 3);
        assert_eq!(items[1]["type"], "a");
    }

    // ========================================================================
    // compact_intervals() Tests
    // ========================================================================

    #[test]
    fn compact_intervals_passes_non_array() {
        let v = serde_json::json!({"x": 1});
        let out = compact_intervals(&v, None);
        assert_eq!(out, v);
    }

    #[test]
    fn compact_intervals_uses_default_fields() {
        let v = serde_json::json!([
            {"type": "interval", "start": 0, "end": 60, "duration": 60, "intensity": 0.9, "activity_id": "abc", "extra": "x"}
        ]);
        let out = compact_intervals(&v, None);
        let item = out.as_array().unwrap()[0].as_object().unwrap();
        assert!(item.contains_key("type"));
        assert!(item.contains_key("duration"));
        assert!(!item.contains_key("extra"));
    }

    #[test]
    fn compact_intervals_with_custom_fields() {
        let v = serde_json::json!([
            {"type": "a", "start": 0, "end": 60, "duration": 60, "intensity": 0.8, "activity_id": "x", "foo": "bar"}
        ]);
        let custom = vec!["foo".to_string()];
        let out = compact_intervals(&v, Some(&custom));
        let item = out.as_array().unwrap()[0].as_object().unwrap();
        assert_eq!(item["foo"], "bar");
        assert!(!item.contains_key("type"));
    }

    #[test]
    fn compact_intervals_skips_non_object_items() {
        let v = serde_json::json!([true, {"type": "a"}]);
        let out = compact_intervals(&v, None);
        let items = out.as_array().unwrap();
        assert_eq!(items[0], true);
    }

    // ========================================================================
    // transform_histogram() Tests
    // ========================================================================

    #[test]
    fn transform_histogram_summary_with_data() {
        let v = serde_json::json!([
            {"value": 100, "count": 10},
            {"value": 200, "count": 5},
            {"value": 300, "count": 0},
        ]);
        let out = transform_histogram(&v, true, 100);
        assert_eq!(out["total_samples"], 15);
        assert!((out["weighted_avg"].as_f64().unwrap() - 133.33).abs() < 0.1);
        assert_eq!(out["min"], 100.0);
        assert_eq!(out["max"], 200.0);
        assert_eq!(out["bins_available"], 3);
    }

    #[test]
    fn transform_histogram_summary_empty_array() {
        let v = serde_json::json!([]);
        let out = transform_histogram(&v, true, 100);
        assert_eq!(out["total_samples"], 0);
        assert_eq!(out["weighted_avg"], 0.0);
    }

    #[test]
    fn transform_histogram_summary_non_array() {
        let v = serde_json::json!({"not": "array"});
        let out = transform_histogram(&v, true, 100);
        assert_eq!(out, v);
    }

    #[test]
    fn transform_histogram_downsample_when_too_large() {
        let v = serde_json::json!(
            (0..20)
                .map(|i| serde_json::json!({"value": i, "count": 1}))
                .collect::<Vec<_>>()
        );
        let out = transform_histogram(&v, false, 5);
        let arr = out.as_array().unwrap();
        assert!(arr.len() <= 5);
        assert_eq!(arr[0]["value"], 0);
        assert!(arr.last().unwrap()["value"].as_i64().unwrap() > 0);
    }

    #[test]
    fn transform_histogram_no_downsample_when_small() {
        let v = serde_json::json!([{"value": 1, "count": 1}]);
        let out = transform_histogram(&v, false, 100);
        assert_eq!(out, v);
    }

    // ========================================================================
    // back_to_back_load() Tests
    // ========================================================================

    #[test]
    fn back_to_back_load_returns_max_consecutive_sum() {
        let loads = vec![100.0, 200.0, 300.0, 50.0];
        let result = back_to_back_load(&loads);
        assert!((result - 500.0).abs() < 0.01);
    }

    #[test]
    fn back_to_back_load_empty_returns_zero() {
        let loads = vec![];
        let result = back_to_back_load(&loads);
        assert!(result.abs() < 0.01);
    }

    #[test]
    fn back_to_back_load_single_element() {
        let loads = vec![100.0];
        // windows(2) yields nothing → fold NEG_INFINITY → max(0) = 0
        let result = back_to_back_load(&loads);
        assert!(result.abs() < 0.01);
    }

    // ========================================================================
    // vert_per_week() Tests
    // ========================================================================

    #[test]
    fn vert_per_week_sums_elevation_gain() {
        let a = serde_json::json!({"total_elevation_gain": 100.0});
        let b = serde_json::json!({"total_elevation_gain": 200.5});
        let c = serde_json::json!({"other": "data"});
        let details = vec![
            a.as_object().unwrap(),
            b.as_object().unwrap(),
            c.as_object().unwrap(),
        ];
        let result = vert_per_week(&details);
        assert!((result - 300.5).abs() < 0.01);
    }

    #[test]
    fn vert_per_week_empty() {
        let result = vert_per_week(&[]);
        assert!(result.abs() < 0.01);
    }

    // ========================================================================
    // longest_run_ratio() Tests
    // ========================================================================

    #[test]
    fn longest_run_ratio_returns_none_when_volume_zero() {
        assert!(longest_run_ratio(10.0, 0.0).is_none());
    }

    #[test]
    fn longest_run_ratio_computes_correctly() {
        let (ratio_km, ratio_hrs) = longest_run_ratio(10.0, 50.0).unwrap();
        assert!((ratio_km - 0.2).abs() < 0.01);
        assert!((ratio_hrs - 0.032).abs() < 0.01);
    }

    // ========================================================================
    // elevation_specificity_score() Tests
    // ========================================================================

    #[test]
    fn elevation_specificity_perfect_match_returns_one() {
        let score = elevation_specificity_score(100.0, 100.0);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn elevation_specificity_zero_race_returns_one() {
        let score = elevation_specificity_score(50.0, 0.0);
        assert!((score - 1.0).abs() < 0.01);
    }

    #[test]
    fn elevation_specificity_partial_match_returns_six_tenths() {
        let score = elevation_specificity_score(60.0, 100.0);
        assert!((score - 0.6).abs() < 0.01);
    }

    #[test]
    fn elevation_specificity_no_match_returns_two_tenths() {
        let score = elevation_specificity_score(200.0, 100.0);
        assert!((score - 0.2).abs() < 0.01);
    }

    // ========================================================================
    // transform_curves() scalar-secs mode Tests
    // ========================================================================

    #[test]
    fn transform_curves_non_object_returns_clone() {
        let v = Value::Array(vec![]);
        let out = transform_curves(&v, false, None);
        assert_eq!(out, v);
    }

    #[test]
    fn transform_curves_non_array_value_passthrough() {
        let v = serde_json::json!({"name": "test"});
        let out = transform_curves(&v, false, None);
        assert_eq!(out, v);
        // Same for summary mode with no durations
        let out2 = transform_curves(&v, true, None);
        assert_eq!(out2, v);
    }

    #[test]
    fn transform_curves_scalar_secs_format() {
        let v = serde_json::json!({
            "list": [
                {"secs": 5, "watts": 350},
                {"secs": 30, "watts": 300},
                {"secs": 60, "watts": 270},
                {"secs": 999, "watts": 100},
            ]
        });
        let out = transform_curves(&v, false, Some(&[5, 30, 60]));
        let list = out["list"].as_array().unwrap();
        assert_eq!(list.len(), 3);
        assert_eq!(list[0]["secs"], 5);
        assert_eq!(list[2]["secs"], 60);
    }

    #[test]
    fn transform_curves_scalar_secs_no_match_excluded() {
        let v = serde_json::json!({
            "list": [
                {"secs": 10, "watts": 400},
            ]
        });
        let out = transform_curves(&v, false, Some(&[5]));
        let list = out["list"].as_array().unwrap();
        assert!(list.is_empty());
    }
}
