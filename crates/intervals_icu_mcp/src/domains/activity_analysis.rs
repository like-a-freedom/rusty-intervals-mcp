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
    let min = nums.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = nums.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

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
}
