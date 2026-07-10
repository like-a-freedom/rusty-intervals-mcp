use serde::Deserialize;
use serde_json::Value;

const LEGACY_WORK_INTERVAL_BASELINE_V1: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/legacy_work_interval_baseline/v1.json"
));

#[derive(Debug, Deserialize)]
struct LegacyWorkIntervalBaselineV1 {
    schema_version: u32,
    benchmark_id: String,
    algorithm_id: String,
    purpose: String,
    cases: Vec<LegacyWorkIntervalBaselineCaseV1>,
}

#[derive(Debug, Deserialize)]
struct LegacyWorkIntervalBaselineCaseV1 {
    id: String,
    intervals: Vec<Value>,
    expected_legacy_work_count: usize,
}

#[derive(Debug, PartialEq)]
struct LegacyWorkIntervalCaseResult {
    id: String,
    expected_legacy_work_count: usize,
    actual_legacy_work_count: usize,
    absolute_error: usize,
}

#[derive(Debug, PartialEq)]
struct LegacyWorkIntervalBaselineReport {
    case_count: usize,
    exact_match_count: usize,
    exact_match_rate: f64,
    expected_legacy_work_count_total: usize,
    actual_legacy_work_count_total: usize,
    sum_absolute_error: usize,
    mean_absolute_error: f64,
    cases: Vec<LegacyWorkIntervalCaseResult>,
}

impl LegacyWorkIntervalBaselineReport {
    fn from_cases(cases: Vec<LegacyWorkIntervalCaseResult>) -> Self {
        let case_count = cases.len();
        let exact_match_count = cases.iter().filter(|case| case.absolute_error == 0).count();
        let expected_legacy_work_count_total = cases
            .iter()
            .map(|case| case.expected_legacy_work_count)
            .sum();
        let actual_legacy_work_count_total =
            cases.iter().map(|case| case.actual_legacy_work_count).sum();
        let sum_absolute_error = cases.iter().map(|case| case.absolute_error).sum();
        let exact_match_rate = if case_count == 0 {
            0.0
        } else {
            exact_match_count as f64 / case_count as f64
        };
        let mean_absolute_error = if case_count == 0 {
            0.0
        } else {
            sum_absolute_error as f64 / case_count as f64
        };

        Self {
            case_count,
            exact_match_count,
            exact_match_rate,
            expected_legacy_work_count_total,
            actual_legacy_work_count_total,
            sum_absolute_error,
            mean_absolute_error,
            cases,
        }
    }
}

fn load_legacy_work_interval_baseline_v1() -> LegacyWorkIntervalBaselineV1 {
    let corpus: LegacyWorkIntervalBaselineV1 =
        serde_json::from_str(LEGACY_WORK_INTERVAL_BASELINE_V1)
            .expect("legacy work-interval baseline fixture must be valid JSON");

    assert_eq!(corpus.schema_version, 1);
    assert_eq!(corpus.benchmark_id, "legacy-work-interval-count-v1");
    assert_eq!(corpus.algorithm_id, "count_work_intervals-median-v1");
    assert_eq!(
        corpus.purpose,
        "synthetic-legacy-output-contract-not-accuracy"
    );

    corpus
}

fn run_legacy_work_interval_baseline_v1(
    corpus: &LegacyWorkIntervalBaselineV1,
) -> LegacyWorkIntervalBaselineReport {
    let cases = corpus
        .cases
        .iter()
        .map(|case| {
            let actual_legacy_work_count = legacy_count_work_intervals_v1(&case.intervals);

            LegacyWorkIntervalCaseResult {
                id: case.id.clone(),
                expected_legacy_work_count: case.expected_legacy_work_count,
                actual_legacy_work_count,
                absolute_error: actual_legacy_work_count.abs_diff(case.expected_legacy_work_count),
            }
        })
        .collect();

    LegacyWorkIntervalBaselineReport::from_cases(cases)
}

fn legacy_count_work_intervals_v1(intervals: &[Value]) -> usize {
    if intervals.is_empty() {
        return 0;
    }

    let mut speed_data: Vec<f64> = Vec::new();
    let mut hr_data: Vec<f64> = Vec::new();

    for interval in intervals.iter().filter_map(Value::as_object) {
        if let Some(speed) = interval
            .get("average_speed")
            .and_then(Value::as_f64)
            .filter(|speed| *speed > 0.0)
        {
            speed_data.push(speed);
        }
        if let Some(heartrate) = interval
            .get("average_heartrate")
            .and_then(Value::as_f64)
            .filter(|heartrate| *heartrate > 0.0)
        {
            hr_data.push(heartrate);
        }
    }

    if speed_data.len() < 3 && hr_data.len() < 3 {
        return intervals.len();
    }

    let median_speed = legacy_calculate_median_v1(&mut speed_data);
    let median_hr = legacy_calculate_median_v1(&mut hr_data);

    intervals
        .iter()
        .filter_map(Value::as_object)
        .filter(|interval| {
            let speed = interval.get("average_speed").and_then(Value::as_f64);
            let heartrate = interval.get("average_heartrate").and_then(Value::as_f64);

            match (speed, heartrate) {
                (Some(speed), Some(heartrate)) => speed >= median_speed && heartrate >= median_hr,
                (Some(speed), None) => speed >= median_speed,
                (None, Some(heartrate)) => heartrate >= median_hr,
                (None, None) => true,
            }
        })
        .count()
        .min(intervals.len())
}

fn legacy_calculate_median_v1(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }

    values.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let middle = values.len() / 2;

    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

#[test]
fn legacy_work_interval_count_baseline_v1() {
    let corpus = load_legacy_work_interval_baseline_v1();
    let report = run_legacy_work_interval_baseline_v1(&corpus);

    println!("{report:#?}");

    assert_eq!(report.case_count, 6);
    assert_eq!(report.cases.len(), 6);
    assert_eq!(report.exact_match_count, 6);
    assert_eq!(report.exact_match_rate, 1.0);
    assert_eq!(report.expected_legacy_work_count_total, 12);
    assert_eq!(report.actual_legacy_work_count_total, 12);
    assert_eq!(report.sum_absolute_error, 0);
    assert_eq!(report.mean_absolute_error, 0.0);
}
