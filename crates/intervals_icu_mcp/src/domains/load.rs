use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum LoadSource {
    IcuTrainingLoad,
    TrainingLoadAlias,
    TssAlias,
    ActivitySummaryTrainingLoad,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoadObservation {
    pub value: f64,
    pub source: LoadSource,
}

impl LoadObservation {
    pub fn new(value: f64, source: LoadSource) -> Option<Self> {
        if !value.is_finite() || value < 0.0 {
            return None;
        }
        Some(Self { value, source })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparableLoadSeries {
    pub daily: Vec<(NaiveDate, f64)>,
    pub activities_total: usize,
    pub activities_with_load: usize,
    pub source_counts: BTreeMap<LoadSource, usize>,
}

impl ComparableLoadSeries {
    pub fn coverage_ratio(&self) -> Option<f64> {
        if self.activities_total == 0 {
            return None;
        }
        Some(self.activities_with_load as f64 / self.activities_total as f64)
    }

    pub fn values(&self) -> impl Iterator<Item = f64> + '_ {
        self.daily.iter().map(|(_, value)| *value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use serde_json;

    #[test]
    fn load_observation_rejects_negative_values() {
        assert_eq!(
            LoadObservation::new(-1.0, LoadSource::IcuTrainingLoad),
            None
        );
    }

    #[test]
    fn load_observation_rejects_nan() {
        assert_eq!(
            LoadObservation::new(f64::NAN, LoadSource::IcuTrainingLoad),
            None
        );
    }

    #[test]
    fn load_observation_rejects_infinite() {
        assert_eq!(
            LoadObservation::new(f64::INFINITY, LoadSource::IcuTrainingLoad),
            None
        );
    }

    #[test]
    fn load_observation_accepts_zero() {
        assert_eq!(
            LoadObservation::new(0.0, LoadSource::IcuTrainingLoad),
            Some(LoadObservation {
                value: 0.0,
                source: LoadSource::IcuTrainingLoad
            })
        );
    }

    #[test]
    fn load_observation_accepts_positive() {
        assert_eq!(
            LoadObservation::new(73.5, LoadSource::TssAlias),
            Some(LoadObservation {
                value: 73.5,
                source: LoadSource::TssAlias
            })
        );
    }

    #[test]
    fn comparable_load_series_coverage_ratio_zero_activities_returns_none() {
        let series = ComparableLoadSeries {
            daily: vec![],
            activities_total: 0,
            activities_with_load: 0,
            source_counts: BTreeMap::new(),
        };
        assert_eq!(series.coverage_ratio(), None);
    }

    #[test]
    fn comparable_load_series_coverage_ratio_partial_coverage() {
        let series = ComparableLoadSeries {
            daily: vec![
                (NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), 50.0),
                (NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(), 0.0),
            ],
            activities_total: 4,
            activities_with_load: 2,
            source_counts: BTreeMap::from([(LoadSource::IcuTrainingLoad, 2)]),
        };
        assert_eq!(series.coverage_ratio(), Some(0.5));
    }

    #[test]
    fn comparable_load_series_coverage_ratio_full_coverage() {
        let series = ComparableLoadSeries {
            daily: vec![(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), 50.0)],
            activities_total: 3,
            activities_with_load: 3,
            source_counts: BTreeMap::from([(LoadSource::IcuTrainingLoad, 3)]),
        };
        assert_eq!(series.coverage_ratio(), Some(1.0));
    }

    #[test]
    fn comparable_load_series_values_iterator() {
        let series = ComparableLoadSeries {
            daily: vec![
                (NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), 50.0),
                (NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(), 0.0),
                (NaiveDate::from_ymd_opt(2026, 1, 3).unwrap(), 75.0),
            ],
            activities_total: 3,
            activities_with_load: 2,
            source_counts: BTreeMap::from([(LoadSource::IcuTrainingLoad, 2)]),
        };
        let values: Vec<f64> = series.values().collect();
        assert_eq!(values, vec![50.0, 0.0, 75.0]);
    }

    #[test]
    fn source_counts_total_equals_activities_with_load() {
        let mut source_counts = BTreeMap::new();
        source_counts.insert(LoadSource::IcuTrainingLoad, 5);
        source_counts.insert(LoadSource::TssAlias, 3);
        source_counts.insert(LoadSource::TrainingLoadAlias, 2);

        let series = ComparableLoadSeries {
            daily: vec![],
            activities_total: 15,
            activities_with_load: 10,
            source_counts,
        };

        let total_from_sources: usize = series.source_counts.values().sum();
        assert_eq!(total_from_sources, series.activities_with_load);
    }

    #[test]
    fn load_source_serialization_round_trip() {
        let sources = vec![
            LoadSource::IcuTrainingLoad,
            LoadSource::TrainingLoadAlias,
            LoadSource::TssAlias,
            LoadSource::ActivitySummaryTrainingLoad,
        ];

        for source in &sources {
            let json = serde_json::to_string(source).unwrap();
            let deserialized: LoadSource = serde_json::from_str(&json).unwrap();
            assert_eq!(*source, deserialized);
        }
    }

    #[test]
    fn load_observation_serialization_round_trip() {
        let obs = LoadObservation {
            value: 73.5,
            source: LoadSource::TssAlias,
        };
        let json = serde_json::to_string(&obs).unwrap();
        let deserialized: LoadObservation = serde_json::from_str(&json).unwrap();
        assert_eq!(obs, deserialized);
    }

    #[test]
    fn comparable_load_series_serialization_round_trip() {
        let series = ComparableLoadSeries {
            daily: vec![
                (NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(), 50.0),
                (NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(), 0.0),
            ],
            activities_total: 4,
            activities_with_load: 2,
            source_counts: BTreeMap::from([
                (LoadSource::IcuTrainingLoad, 1),
                (LoadSource::TssAlias, 1),
            ]),
        };
        let json = serde_json::to_string(&series).unwrap();
        let deserialized: ComparableLoadSeries = serde_json::from_str(&json).unwrap();
        assert_eq!(series, deserialized);
    }
}
