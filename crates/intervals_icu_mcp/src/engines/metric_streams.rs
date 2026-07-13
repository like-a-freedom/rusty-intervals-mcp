//! Strict JSON-to-`MetricStreams` parser extracted from the analyzer handler.
//!
//! This preserves the historical stream-key and NaN semantics that existing
//! tests already cover: only confirmed m/s stream names (`velocity_smooth`,
//! `speed`) are accepted as speed signals; `pace` is intentionally not
//! promoted for the segmentation engine. Null and non-finite signal
//! samples are normalised to `f64::NAN` so per-signal coverage can be
//! measured independently.
//!
//! The legacy detector keeps using `numeric_series` for its `pace` alias —
//! the helper is re-exported here so the existing handler-local
//! `build_local_raw_stream` does not duplicate numeric-array parsing.

use serde_json::Value;

use crate::domains::interval_segment::MetricStreams;

/// Strict JSON-to-`MetricStreams` parser.
///
/// Returns `None` when timestamps are missing, fewer than 2, non-monotonic,
/// or non-finite. Per-signal samples are aligned with the time axis and
/// filtered through a per-stream validity predicate.
pub fn parse_metric_streams(streams: &Value) -> Option<MetricStreams> {
    let time_s = numeric_series(streams, &["time", "time_s"])?;
    if time_s.len() < 2
        || time_s
            .windows(2)
            .any(|pair| !pair[0].is_finite() || !pair[1].is_finite() || pair[1] <= pair[0])
    {
        return None;
    }

    let aligned = |keys: &[&str], valid: fn(f64) -> bool| {
        metric_signal_series(streams, keys)
            .filter(|values| values.len() == time_s.len())
            .map(|values| {
                values
                    .into_iter()
                    .map(|value| {
                        if value.is_finite() && valid(value) {
                            value
                        } else {
                            f64::NAN
                        }
                    })
                    .collect()
            })
    };

    Some(MetricStreams {
        speed_mps: aligned(&["velocity_smooth", "speed"], |value| value >= 0.0),
        heartrate_bpm: aligned(&["heartrate", "hr"], |value| value > 0.0),
        power_w: aligned(&["watts", "power"], |value| value >= 0.0),
        time_s,
    })
}

/// Numeric-array helper retained for the analyzer's `pace` alias path.
pub(crate) fn numeric_series(streams: &Value, keys: &[&str]) -> Option<Vec<f64>> {
    keys.iter().find_map(|key| {
        streams
            .get(*key)
            .and_then(Value::as_array)
            .and_then(|values| values.iter().map(Value::as_f64).collect::<Option<Vec<_>>>())
    })
}

fn metric_signal_series(streams: &Value, keys: &[&str]) -> Option<Vec<f64>> {
    keys.iter().find_map(|key| {
        streams.get(*key).and_then(Value::as_array).map(|values| {
            values
                .iter()
                .map(|value| value.as_f64().unwrap_or(f64::NAN))
                .collect()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parser_drops_only_a_misaligned_signal() {
        let streams = json!({
            "time": [0.0, 1.0, 2.0],
            "velocity_smooth": [4.0, 5.0, 6.0],
            "heartrate": [150.0, 155.0],
            "watts": [200.0, 220.0, 240.0]
        });
        let parsed = parse_metric_streams(&streams).expect("time stream");
        assert_eq!(parsed.speed_mps, Some(vec![4.0, 5.0, 6.0]));
        assert!(parsed.heartrate_bpm.is_none());
        assert_eq!(parsed.power_w, Some(vec![200.0, 220.0, 240.0]));
    }

    #[test]
    fn parser_preserves_null_signal_samples_and_rejects_pace_as_speed() {
        let parsed = parse_metric_streams(&json!({
            "time": [0.0, 1.0, 2.0],
            "pace": [300.0, 295.0, 305.0],
            "watts": [200.0, null, 240.0]
        }))
        .expect("time stream");
        assert!(parsed.speed_mps.is_none());
        assert!(parsed.power_w.unwrap()[1].is_nan());
    }
}
