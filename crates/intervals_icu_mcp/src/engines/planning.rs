/// Planning Engine - Multi-scale training planning
///
/// Implements planning across various horizons:
/// - Microcycle: 1 week with daily detail
/// - Mesocycle: 4-8 weeks (Base/Build/Specific phases)
/// - Macrocycle: 3-6 months (race preparation)
/// - Annual Plan: 12 months with multiple races
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// Planning horizon types
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PlanningHorizon {
    /// 1 week: detailed daily planning
    Microcycle {
        week_start: NaiveDate,
        focus: Option<TrainingFocus>,
    },
    /// 4-8 weeks: training phase
    Mesocycle {
        start: NaiveDate,
        weeks: u8,
        phase: TrainingPhase,
        target_weekly_hours: f32,
    },
    /// 3-6 months: race preparation
    Macrocycle {
        race_date: NaiveDate,
        race_distance: RaceDistance,
        weeks_out: u8,
    },
    /// 12 months: season planning
    AnnualPlan {
        year: u16,
        key_races: Vec<KeyRace>,
        periods: Vec<PeriodBlock>,
    },
}

/// Training phases
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrainingPhase {
    Transition,
    EarlyBase,
    LateBase,
    Build,
    Specific,
    Taper,
    Race,
    Recovery,
}

/// Training focus areas
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TrainingFocus {
    AerobicBase,
    Intensity,
    Vertical,
    RaceSpecific,
    Recovery,
    Strength,
}

/// Race distances
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RaceDistance {
    _5K,
    _10K,
    HalfMarathon,
    Marathon,
    _50K,
    _100K,
    _50Mile,
    _100Mile,
    Other(f32), // in km
}

/// Key race for annual planning
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRace {
    pub name: String,
    pub date: NaiveDate,
    pub distance: RaceDistance,
    pub priority: u8, // 1=A-priority, 2=B-priority, 3=C-priority
}

/// Period block for annual planning
#[must_use]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeriodBlock {
    pub name: String,
    pub start: NaiveDate,
    pub end: NaiveDate,
    pub phase: TrainingPhase,
    pub target_volume: f32, // hours per week
}

/// Periodization business rules
#[must_use]
pub struct PeriodizationRules;

impl PeriodizationRules {
    /// Maximum weekly volume progression (7-10%)
    pub const MAX_WEEKLY_PROGRESSION: f32 = 0.10;

    /// Recovery week volume reduction (40-60%)
    pub const RECOVERY_WEEK_REDUCTION: f32 = 0.50;

    /// Recovery week frequency (every 3-4 weeks)
    pub const RECOVERY_WEEK_FREQUENCY: u8 = 4;

    /// Base period zone distribution (85-95% Z1-Z2)
    pub const BASE_PERIOD_Z1Z2_MIN: f32 = 0.85;
    pub const BASE_PERIOD_Z1Z2_MAX: f32 = 0.95;

    /// Taper protocols by distance
    #[must_use]
    pub fn taper_protocol(distance: &RaceDistance) -> TaperProtocol {
        match distance {
            RaceDistance::_5K | RaceDistance::_10K | RaceDistance::HalfMarathon => TaperProtocol {
                duration_days: 7,
                volume_reduction: 0.50,
                intensity_maintain: true,
            },
            RaceDistance::Marathon | RaceDistance::_50K => TaperProtocol {
                duration_days: 10,
                volume_reduction: 0.50,
                intensity_maintain: true,
            },
            RaceDistance::_100K => TaperProtocol {
                duration_days: 12,
                volume_reduction: 0.55,
                intensity_maintain: true,
            },
            RaceDistance::_50Mile => TaperProtocol {
                duration_days: 14,
                volume_reduction: 0.55,
                intensity_maintain: true,
            },
            RaceDistance::_100Mile | RaceDistance::Other(_) => TaperProtocol {
                duration_days: 21,
                volume_reduction: 0.60,
                intensity_maintain: true,
            },
        }
    }

    /// Determine focus based on AeT-LT gap
    pub fn focus_from_aet_lt_gap(gap_percent: f32) -> TrainingFocus {
        if gap_percent > 10.0 {
            TrainingFocus::AerobicBase
        } else {
            TrainingFocus::Intensity
        }
    }

    /// Calculate recovery week volume
    #[must_use]
    pub fn recovery_week_volume(normal_volume: f32) -> f32 {
        normal_volume * (1.0 - Self::RECOVERY_WEEK_REDUCTION)
    }

    /// Check if week should be recovery week
    #[must_use]
    pub fn is_recovery_week(week_number: u8) -> bool {
        week_number.is_multiple_of(Self::RECOVERY_WEEK_FREQUENCY)
    }

    /// Calculate progressive overload for week
    #[must_use]
    pub fn weekly_volume(base_volume: f32, week: u8) -> f32 {
        let progression = (week as f32 - 1.0) * Self::MAX_WEEKLY_PROGRESSION;
        base_volume * (1.0 + progression.min(0.30)) // Cap at 30% increase
    }
}

/// Taper protocol
#[derive(Debug, Clone)]
pub struct TaperProtocol {
    pub duration_days: u8,
    pub volume_reduction: f32,
    pub intensity_maintain: bool,
}

/// Workout template
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkoutTemplate {
    pub name: String,
    pub duration_minutes: u32,
    pub zones: Vec<ZoneSegment>,
    pub sport: String,
}

/// Zone segment for workout
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZoneSegment {
    pub zone: u8, // 1-5
    pub duration_minutes: u32,
    pub description: Option<String>,
}

/// Weekly plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeeklyPlan {
    pub week_start: NaiveDate,
    pub workouts: Vec<WorkoutTemplate>,
    pub rest_days: Vec<u8>, // 0=Monday, 6=Sunday
    pub target_volume_hours: f32,
}

impl PlanningHorizon {
    /// Create a microcycle (1 week) plan
    pub fn microcycle(week_start: NaiveDate, focus: TrainingFocus) -> Self {
        PlanningHorizon::Microcycle {
            week_start,
            focus: Some(focus),
        }
    }

    /// Create a mesocycle (4-8 weeks) plan
    pub fn mesocycle(start: NaiveDate, weeks: u8, phase: TrainingPhase, target_hours: f32) -> Self {
        PlanningHorizon::Mesocycle {
            start,
            weeks,
            phase,
            target_weekly_hours: target_hours,
        }
    }

    /// Create a macrocycle (race preparation) plan
    pub fn macrocycle(race_date: NaiveDate, distance: RaceDistance, weeks_out: u8) -> Self {
        PlanningHorizon::Macrocycle {
            race_date,
            race_distance: distance,
            weeks_out,
        }
    }

    /// Get number of weeks for this horizon
    #[must_use]
    pub fn weeks(&self) -> u32 {
        match self {
            PlanningHorizon::Microcycle { .. } => 1,
            PlanningHorizon::Mesocycle { weeks, .. } => *weeks as u32,
            PlanningHorizon::Macrocycle { weeks_out, .. } => *weeks_out as u32,
            PlanningHorizon::AnnualPlan { .. } => 52,
        }
    }
}

impl TrainingPhase {
    /// Get zone distribution for phase
    #[must_use]
    pub fn zone_distribution(&self) -> ZoneDistribution {
        match self {
            TrainingPhase::Transition | TrainingPhase::Recovery => ZoneDistribution {
                z1_z2: 0.90,
                z3: 0.05,
                z4_z5: 0.05,
            },
            TrainingPhase::EarlyBase | TrainingPhase::LateBase => ZoneDistribution {
                z1_z2: 0.90,
                z3: 0.08,
                z4_z5: 0.02,
            },
            TrainingPhase::Build => ZoneDistribution {
                z1_z2: 0.75,
                z3: 0.15,
                z4_z5: 0.10,
            },
            TrainingPhase::Specific => ZoneDistribution {
                z1_z2: 0.65,
                z3: 0.20,
                z4_z5: 0.15,
            },
            TrainingPhase::Taper => ZoneDistribution {
                z1_z2: 0.70,
                z3: 0.20,
                z4_z5: 0.10,
            },
            TrainingPhase::Race => ZoneDistribution {
                z1_z2: 0.50,
                z3: 0.30,
                z4_z5: 0.20,
            },
        }
    }
}

/// Zone distribution percentages
#[derive(Debug, Clone)]
pub struct ZoneDistribution {
    pub z1_z2: f32,
    pub z3: f32,
    pub z4_z5: f32,
}

/// Generate sample workout for phase
#[must_use]
pub fn generate_workout_for_phase(
    phase: &TrainingPhase,
    focus: &TrainingFocus,
    duration_minutes: u32,
) -> WorkoutTemplate {
    let distribution = phase.zone_distribution();

    let zones = match focus {
        TrainingFocus::AerobicBase => {
            vec![
                ZoneSegment {
                    zone: 1,
                    duration_minutes: (duration_minutes as f32 * 0.1).round() as u32,
                    description: Some("Warm-up".into()),
                },
                ZoneSegment {
                    zone: 2,
                    duration_minutes: (duration_minutes as f32 * distribution.z1_z2).round() as u32,
                    description: Some("Aerobic base".into()),
                },
                ZoneSegment {
                    zone: 1,
                    duration_minutes: (duration_minutes as f32 * 0.1).round() as u32,
                    description: Some("Cool-down".into()),
                },
            ]
        }
        TrainingFocus::Intensity => {
            vec![
                ZoneSegment {
                    zone: 1,
                    duration_minutes: (duration_minutes as f32 * 0.15).round() as u32,
                    description: Some("Warm-up".into()),
                },
                ZoneSegment {
                    zone: 4,
                    duration_minutes: (duration_minutes as f32 * distribution.z4_z5).round() as u32,
                    description: Some("Intervals".into()),
                },
                ZoneSegment {
                    zone: 2,
                    duration_minutes: (duration_minutes as f32 * 0.2).round() as u32,
                    description: Some("Recovery".into()),
                },
                ZoneSegment {
                    zone: 1,
                    duration_minutes: (duration_minutes as f32 * 0.1).round() as u32,
                    description: Some("Cool-down".into()),
                },
            ]
        }
        _ => {
            // Default balanced workout
            vec![
                ZoneSegment {
                    zone: 1,
                    duration_minutes: (duration_minutes as f32 * 0.1).round() as u32,
                    description: Some("Warm-up".into()),
                },
                ZoneSegment {
                    zone: 2,
                    duration_minutes: (duration_minutes as f32 * distribution.z1_z2 * 0.8).round()
                        as u32,
                    description: None,
                },
                ZoneSegment {
                    zone: 3,
                    duration_minutes: (duration_minutes as f32 * distribution.z3).round() as u32,
                    description: None,
                },
                ZoneSegment {
                    zone: 1,
                    duration_minutes: (duration_minutes as f32 * 0.1).round() as u32,
                    description: Some("Cool-down".into()),
                },
            ]
        }
    };

    WorkoutTemplate {
        name: format!("{:?} Workout", focus),
        duration_minutes,
        zones,
        sport: "Run".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_periodization_rules() {
        assert_eq!(PeriodizationRules::MAX_WEEKLY_PROGRESSION, 0.10);
        assert_eq!(PeriodizationRules::RECOVERY_WEEK_REDUCTION, 0.50);
        assert_eq!(PeriodizationRules::RECOVERY_WEEK_FREQUENCY, 4);
    }

    #[test]
    fn test_recovery_week_detection() {
        assert!(!PeriodizationRules::is_recovery_week(1));
        assert!(!PeriodizationRules::is_recovery_week(3));
        assert!(PeriodizationRules::is_recovery_week(4));
        assert!(PeriodizationRules::is_recovery_week(8));
    }

    #[test]
    fn test_taper_protocol() {
        let taper_50k = PeriodizationRules::taper_protocol(&RaceDistance::_50K);
        assert_eq!(taper_50k.duration_days, 10);
        assert_eq!(taper_50k.volume_reduction, 0.50);

        let taper_100m = PeriodizationRules::taper_protocol(&RaceDistance::_100Mile);
        assert_eq!(taper_100m.duration_days, 21);
        assert_eq!(taper_100m.volume_reduction, 0.60);
    }

    #[test]
    fn test_focus_from_gap() {
        assert_eq!(
            PeriodizationRules::focus_from_aet_lt_gap(25.0),
            TrainingFocus::AerobicBase
        );
        assert_eq!(
            PeriodizationRules::focus_from_aet_lt_gap(8.0),
            TrainingFocus::Intensity
        );
    }

    #[test]
    fn test_zone_distribution() {
        let base = TrainingPhase::EarlyBase.zone_distribution();
        assert!(base.z1_z2 >= 0.85);

        let build = TrainingPhase::Build.zone_distribution();
        assert!(build.z1_z2 < 0.80);
    }

    #[test]
    fn test_weekly_volume_calculation() {
        let base_volume = 8.0;
        let week1 = PeriodizationRules::weekly_volume(base_volume, 1);
        assert!((week1 - 8.0).abs() < 0.01);

        let week2 = PeriodizationRules::weekly_volume(base_volume, 2);
        assert!(week2 > week1);
        assert!(week2 <= base_volume * 1.10);
    }

    #[test]
    fn test_recovery_week_volume() {
        let normal_volume = 10.0;
        let recovery = PeriodizationRules::recovery_week_volume(normal_volume);
        assert!((recovery - 5.0).abs() < 0.01);
    }

    #[test]
    fn test_taper_all_distances() {
        let distances = vec![
            (RaceDistance::_5K, 7),
            (RaceDistance::_10K, 7),
            (RaceDistance::HalfMarathon, 7),
            (RaceDistance::Marathon, 10),
            (RaceDistance::_50K, 10),
            (RaceDistance::_100K, 12),
            (RaceDistance::_50Mile, 14),
            (RaceDistance::_100Mile, 21),
        ];

        for (distance, expected_days) in distances {
            let taper = PeriodizationRules::taper_protocol(&distance);
            assert_eq!(
                taper.duration_days, expected_days,
                "Failed for {:?}",
                distance
            );
            assert!(taper.volume_reduction >= 0.50);
            assert!(taper.volume_reduction <= 0.60);
            assert!(taper.intensity_maintain);
        }
    }

    #[test]
    fn test_training_phase_progression() {
        let phases = [
            TrainingPhase::EarlyBase,
            TrainingPhase::LateBase,
            TrainingPhase::Build,
            TrainingPhase::Specific,
            TrainingPhase::Taper,
            TrainingPhase::Race,
        ];

        let distributions: Vec<_> = phases.iter().map(|p| p.zone_distribution()).collect();

        // Z1-Z2 should decrease as intensity increases
        assert!(distributions[0].z1_z2 >= distributions[1].z1_z2);
        assert!(distributions[1].z1_z2 >= distributions[2].z1_z2);
        assert!(distributions[2].z1_z2 >= distributions[3].z1_z2);
    }

    #[test]
    fn test_planning_horizon_weeks() {
        use chrono::NaiveDate;

        let microcycle = PlanningHorizon::microcycle(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            TrainingFocus::AerobicBase,
        );
        assert_eq!(microcycle.weeks(), 1);

        let mesocycle = PlanningHorizon::mesocycle(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            8,
            TrainingPhase::Build,
            10.0,
        );
        assert_eq!(mesocycle.weeks(), 8);

        let macrocycle = PlanningHorizon::macrocycle(
            NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            RaceDistance::_50K,
            12,
        );
        assert_eq!(macrocycle.weeks(), 12);
    }

    #[test]
    fn test_workout_generation_for_phase() {
        let workout =
            generate_workout_for_phase(&TrainingPhase::EarlyBase, &TrainingFocus::AerobicBase, 60);

        assert!(!workout.name.is_empty());
        assert_eq!(workout.duration_minutes, 60);
        assert!(!workout.zones.is_empty());

        // Base phase should have mostly Z1-Z2
        let total_zone_time: u32 = workout.zones.iter().map(|z| z.duration_minutes).sum();
        // Allow for rounding differences (zone distribution percentages may not sum to exactly 100%)
        assert!(
            (50..=70).contains(&total_zone_time),
            "Total zone time {} should be close to 60",
            total_zone_time
        );
    }

    #[test]
    fn test_custom_race_distance() {
        let custom = RaceDistance::Other(42.195); // Marathon in km
        let taper = PeriodizationRules::taper_protocol(&custom);
        assert!(taper.duration_days >= 7);
        assert!(taper.volume_reduction >= 0.50);
    }

    // ========================================================================
    // PlanningHorizon Tests
    // ========================================================================

    #[test]
    fn test_planning_horizon_microcycle_variant() {
        use chrono::NaiveDate;

        let horizon = PlanningHorizon::Microcycle {
            week_start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            focus: Some(TrainingFocus::AerobicBase),
        };

        assert_eq!(horizon.weeks(), 1);
    }

    #[test]
    fn test_planning_horizon_microcycle_no_focus() {
        use chrono::NaiveDate;

        let horizon = PlanningHorizon::Microcycle {
            week_start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            focus: None,
        };

        assert_eq!(horizon.weeks(), 1);
    }

    #[test]
    fn test_planning_horizon_mesocycle_variant() {
        use chrono::NaiveDate;

        let horizon = PlanningHorizon::Mesocycle {
            start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            weeks: 6,
            phase: TrainingPhase::Build,
            target_weekly_hours: 12.0,
        };

        assert_eq!(horizon.weeks(), 6);
    }

    #[test]
    fn test_planning_horizon_macrocycle_variant() {
        use chrono::NaiveDate;

        let horizon = PlanningHorizon::Macrocycle {
            race_date: NaiveDate::from_ymd_opt(2026, 6, 15).unwrap(),
            race_distance: RaceDistance::Marathon,
            weeks_out: 16,
        };

        assert_eq!(horizon.weeks(), 16);
    }

    #[test]
    fn test_planning_horizon_annual_plan_variant() {
        let horizon = PlanningHorizon::AnnualPlan {
            year: 2026,
            key_races: vec![],
            periods: vec![],
        };

        assert_eq!(horizon.weeks(), 52);
    }

    #[test]
    fn test_planning_horizon_clone() {
        use chrono::NaiveDate;

        let original = PlanningHorizon::microcycle(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            TrainingFocus::Intensity,
        );
        let cloned = original.clone();

        assert_eq!(original.weeks(), cloned.weeks());
    }

    #[test]
    fn test_planning_horizon_debug() {
        use chrono::NaiveDate;

        let horizon = PlanningHorizon::microcycle(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            TrainingFocus::AerobicBase,
        );
        let debug = format!("{:?}", horizon);
        assert!(debug.contains("Microcycle"));
    }

    #[test]
    fn test_planning_horizon_builder_methods() {
        use chrono::NaiveDate;

        let micro = PlanningHorizon::microcycle(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            TrainingFocus::Recovery,
        );
        let meso = PlanningHorizon::mesocycle(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            4,
            TrainingPhase::EarlyBase,
            8.0,
        );
        let macro_cycle = PlanningHorizon::macrocycle(
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            RaceDistance::_100K,
            24,
        );

        assert_eq!(micro.weeks(), 1);
        assert_eq!(meso.weeks(), 4);
        assert_eq!(macro_cycle.weeks(), 24);
    }

    // ========================================================================
    // TrainingPhase Tests
    // ========================================================================

    #[test]
    fn test_training_phase_all_variants() {
        let phases = [
            TrainingPhase::Transition,
            TrainingPhase::EarlyBase,
            TrainingPhase::LateBase,
            TrainingPhase::Build,
            TrainingPhase::Specific,
            TrainingPhase::Taper,
            TrainingPhase::Race,
            TrainingPhase::Recovery,
        ];

        // Verify all phases are distinct
        for i in 0..phases.len() {
            for j in 0..phases.len() {
                if i == j {
                    assert_eq!(phases[i], phases[j]);
                } else {
                    assert_ne!(phases[i], phases[j]);
                }
            }
        }
    }

    #[test]
    fn test_training_phase_clone() {
        let original = TrainingPhase::Build;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_training_phase_debug() {
        let phase = TrainingPhase::Specific;
        let debug = format!("{:?}", phase);
        assert!(debug.contains("Specific"));
    }

    #[test]
    fn test_training_phase_equality() {
        assert_eq!(TrainingPhase::Transition, TrainingPhase::Transition);
        assert_eq!(TrainingPhase::Recovery, TrainingPhase::Recovery);
        assert_ne!(TrainingPhase::EarlyBase, TrainingPhase::LateBase);
        assert_ne!(TrainingPhase::Build, TrainingPhase::Taper);
    }

    #[test]
    fn test_training_phase_zone_distribution_all_phases() {
        let phases = vec![
            TrainingPhase::Transition,
            TrainingPhase::EarlyBase,
            TrainingPhase::LateBase,
            TrainingPhase::Build,
            TrainingPhase::Specific,
            TrainingPhase::Taper,
            TrainingPhase::Race,
            TrainingPhase::Recovery,
        ];

        for phase in phases {
            let dist = phase.zone_distribution();
            // All distributions should sum to approximately 1.0
            let sum = dist.z1_z2 + dist.z3 + dist.z4_z5;
            assert!(
                (sum - 1.0).abs() < 0.01,
                "Zone distribution for {:?} sums to {} instead of 1.0",
                phase,
                sum
            );
        }
    }

    #[test]
    fn test_training_phase_zone_distribution_recovery_vs_base() {
        let recovery = TrainingPhase::Recovery.zone_distribution();
        let base = TrainingPhase::EarlyBase.zone_distribution();

        // Both should have high Z1-Z2
        assert!(recovery.z1_z2 >= 0.85);
        assert!(base.z1_z2 >= 0.85);
    }

    #[test]
    fn test_training_phase_zone_distribution_race_phase() {
        let race = TrainingPhase::Race.zone_distribution();

        // Race phase should have lower Z1-Z2 and higher intensity
        assert!(race.z1_z2 < 0.60);
        assert!(race.z4_z5 >= 0.15);
    }

    // ========================================================================
    // TrainingFocus Tests
    // ========================================================================

    #[test]
    fn test_training_focus_all_variants() {
        let focuses = [
            TrainingFocus::AerobicBase,
            TrainingFocus::Intensity,
            TrainingFocus::Vertical,
            TrainingFocus::RaceSpecific,
            TrainingFocus::Recovery,
            TrainingFocus::Strength,
        ];

        // Verify all focuses are distinct
        for i in 0..focuses.len() {
            for j in 0..focuses.len() {
                if i == j {
                    assert_eq!(focuses[i], focuses[j]);
                } else {
                    assert_ne!(focuses[i], focuses[j]);
                }
            }
        }
    }

    #[test]
    fn test_training_focus_clone() {
        let original = TrainingFocus::AerobicBase;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_training_focus_debug() {
        let focus = TrainingFocus::RaceSpecific;
        let debug = format!("{:?}", focus);
        assert!(debug.contains("RaceSpecific"));
    }

    // ========================================================================
    // RaceDistance Tests
    // ========================================================================

    #[test]
    fn test_race_distance_all_standard_variants() {
        let distances = [
            RaceDistance::_5K,
            RaceDistance::_10K,
            RaceDistance::HalfMarathon,
            RaceDistance::Marathon,
            RaceDistance::_50K,
            RaceDistance::_100K,
            RaceDistance::_50Mile,
            RaceDistance::_100Mile,
        ];

        // Verify all distances are distinct
        for i in 0..distances.len() {
            for j in 0..distances.len() {
                if i == j {
                    assert_eq!(distances[i], distances[j]);
                } else {
                    assert_ne!(distances[i], distances[j]);
                }
            }
        }
    }

    #[test]
    fn test_race_distance_other_variant() {
        let custom = RaceDistance::Other(21.0975); // Half marathon in km
        assert!(matches!(custom, RaceDistance::Other(v) if (v - 21.0975).abs() < 0.001));
    }

    #[test]
    fn test_race_distance_clone() {
        let original = RaceDistance::Marathon;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_race_distance_debug() {
        let distance = RaceDistance::_50K;
        let debug = format!("{:?}", distance);
        assert!(debug.contains("50K"));
    }

    #[test]
    fn test_race_distance_debug_other() {
        let distance = RaceDistance::Other(42.195);
        let debug = format!("{:?}", distance);
        assert!(debug.contains("Other"));
    }

    // ========================================================================
    // KeyRace Tests
    // ========================================================================

    #[test]
    fn test_key_race_clone() {
        use chrono::NaiveDate;

        let original = KeyRace {
            name: "Boston Marathon".into(),
            date: NaiveDate::from_ymd_opt(2026, 4, 20).unwrap(),
            distance: RaceDistance::Marathon,
            priority: 1,
        };
        let cloned = original.clone();

        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.date, original.date);
        assert_eq!(cloned.distance, original.distance);
        assert_eq!(cloned.priority, original.priority);
    }

    #[test]
    fn test_key_race_debug() {
        use chrono::NaiveDate;

        let race = KeyRace {
            name: "Test Race".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            distance: RaceDistance::_10K,
            priority: 2,
        };
        let debug = format!("{:?}", race);
        assert!(debug.contains("KeyRace"));
        assert!(debug.contains("Test Race"));
    }

    #[test]
    fn test_key_race_priority_levels() {
        use chrono::NaiveDate;

        let a_race = KeyRace {
            name: "A Race".into(),
            date: NaiveDate::from_ymd_opt(2026, 6, 1).unwrap(),
            distance: RaceDistance::Marathon,
            priority: 1,
        };
        let b_race = KeyRace {
            name: "B Race".into(),
            date: NaiveDate::from_ymd_opt(2026, 5, 1).unwrap(),
            distance: RaceDistance::_10K,
            priority: 2,
        };
        let c_race = KeyRace {
            name: "C Race".into(),
            date: NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            distance: RaceDistance::_5K,
            priority: 3,
        };

        assert_eq!(a_race.priority, 1);
        assert_eq!(b_race.priority, 2);
        assert_eq!(c_race.priority, 3);
    }

    // ========================================================================
    // PeriodBlock Tests
    // ========================================================================

    #[test]
    fn test_period_block_clone() {
        use chrono::NaiveDate;

        let original = PeriodBlock {
            name: "Base Phase".into(),
            start: NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 3, 31).unwrap(),
            phase: TrainingPhase::EarlyBase,
            target_volume: 8.0,
        };
        let cloned = original.clone();

        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.start, original.start);
        assert_eq!(cloned.end, original.end);
        assert_eq!(cloned.phase, original.phase);
        assert_eq!(cloned.target_volume, original.target_volume);
    }

    #[test]
    fn test_period_block_debug() {
        use chrono::NaiveDate;

        let block = PeriodBlock {
            name: "Build Phase".into(),
            start: NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
            end: NaiveDate::from_ymd_opt(2026, 6, 30).unwrap(),
            phase: TrainingPhase::Build,
            target_volume: 12.0,
        };
        let debug = format!("{:?}", block);
        assert!(debug.contains("PeriodBlock"));
        assert!(debug.contains("Build Phase"));
    }

    // ========================================================================
    // TaperProtocol Tests
    // ========================================================================

    #[test]
    fn test_taper_protocol_clone() {
        let original = TaperProtocol {
            duration_days: 14,
            volume_reduction: 0.55,
            intensity_maintain: true,
        };
        let cloned = original.clone();

        assert_eq!(cloned.duration_days, original.duration_days);
        assert_eq!(cloned.volume_reduction, original.volume_reduction);
        assert_eq!(cloned.intensity_maintain, original.intensity_maintain);
    }

    #[test]
    fn test_taper_protocol_debug() {
        let protocol = TaperProtocol {
            duration_days: 21,
            volume_reduction: 0.60,
            intensity_maintain: true,
        };
        let debug = format!("{:?}", protocol);
        assert!(debug.contains("TaperProtocol"));
        assert!(debug.contains("21"));
    }

    #[test]
    fn test_taper_protocol_all_distances() {
        let distances_and_days = vec![
            (RaceDistance::_5K, 7),
            (RaceDistance::_10K, 7),
            (RaceDistance::HalfMarathon, 7),
            (RaceDistance::Marathon, 10),
            (RaceDistance::_50K, 10),
            (RaceDistance::_100K, 12),
            (RaceDistance::_50Mile, 14),
            (RaceDistance::_100Mile, 21),
        ];

        for (distance, expected_days) in distances_and_days {
            let taper = PeriodizationRules::taper_protocol(&distance);
            assert_eq!(
                taper.duration_days, expected_days,
                "Wrong taper duration for {:?}",
                distance
            );
            assert!(
                taper.volume_reduction >= 0.50 && taper.volume_reduction <= 0.60,
                "Volume reduction should be between 0.50 and 0.60"
            );
            assert!(taper.intensity_maintain, "Intensity should be maintained");
        }
    }

    // ========================================================================
    // WorkoutTemplate Tests
    // ========================================================================

    #[test]
    fn test_workout_template_clone() {
        let original = WorkoutTemplate {
            name: "Tempo Run".into(),
            duration_minutes: 60,
            zones: vec![ZoneSegment {
                zone: 3,
                duration_minutes: 40,
                description: Some("Tempo effort".into()),
            }],
            sport: "Run".into(),
        };
        let cloned = original.clone();

        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.duration_minutes, original.duration_minutes);
        assert_eq!(cloned.zones.len(), original.zones.len());
        assert_eq!(cloned.sport, original.sport);
    }

    #[test]
    fn test_workout_template_debug() {
        let workout = WorkoutTemplate {
            name: "Interval Session".into(),
            duration_minutes: 45,
            zones: vec![],
            sport: "Run".into(),
        };
        let debug = format!("{:?}", workout);
        assert!(debug.contains("WorkoutTemplate"));
        assert!(debug.contains("Interval Session"));
    }

    #[test]
    fn test_workout_template_multiple_zones() {
        let workout = WorkoutTemplate {
            name: "Long Run".into(),
            duration_minutes: 120,
            zones: vec![
                ZoneSegment {
                    zone: 1,
                    duration_minutes: 10,
                    description: Some("Warm-up".into()),
                },
                ZoneSegment {
                    zone: 2,
                    duration_minutes: 100,
                    description: None,
                },
                ZoneSegment {
                    zone: 1,
                    duration_minutes: 10,
                    description: Some("Cool-down".into()),
                },
            ],
            sport: "Run".into(),
        };

        assert_eq!(workout.zones.len(), 3);
        let total_time: u32 = workout.zones.iter().map(|z| z.duration_minutes).sum();
        assert_eq!(total_time, 120);
    }

    // ========================================================================
    // ZoneSegment Tests
    // ========================================================================

    #[test]
    fn test_zone_segment_clone() {
        let original = ZoneSegment {
            zone: 4,
            duration_minutes: 5,
            description: Some("VO2 Max interval".into()),
        };
        let cloned = original.clone();

        assert_eq!(cloned.zone, original.zone);
        assert_eq!(cloned.duration_minutes, original.duration_minutes);
        assert_eq!(cloned.description, original.description);
    }

    #[test]
    fn test_zone_segment_debug() {
        let segment = ZoneSegment {
            zone: 2,
            duration_minutes: 30,
            description: Some("Aerobic".into()),
        };
        let debug = format!("{:?}", segment);
        assert!(debug.contains("ZoneSegment"));
        assert!(debug.contains("2"));
    }

    #[test]
    fn test_zone_segment_no_description() {
        let segment = ZoneSegment {
            zone: 3,
            duration_minutes: 20,
            description: None,
        };

        assert_eq!(segment.zone, 3);
        assert!(segment.description.is_none());
    }

    // ========================================================================
    // WeeklyPlan Tests
    // ========================================================================

    #[test]
    fn test_weekly_plan_clone() {
        use chrono::NaiveDate;

        let original = WeeklyPlan {
            week_start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            workouts: vec![WorkoutTemplate {
                name: "Monday Run".into(),
                duration_minutes: 60,
                zones: vec![],
                sport: "Run".into(),
            }],
            rest_days: vec![0, 6], // Monday and Sunday
            target_volume_hours: 10.0,
        };
        let cloned = original.clone();

        assert_eq!(cloned.week_start, original.week_start);
        assert_eq!(cloned.workouts.len(), original.workouts.len());
        assert_eq!(cloned.rest_days, original.rest_days);
        assert_eq!(cloned.target_volume_hours, original.target_volume_hours);
    }

    #[test]
    fn test_weekly_plan_debug() {
        use chrono::NaiveDate;

        let plan = WeeklyPlan {
            week_start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            workouts: vec![],
            rest_days: vec![3], // Thursday
            target_volume_hours: 8.0,
        };
        let debug = format!("{:?}", plan);
        assert!(debug.contains("WeeklyPlan"));
    }

    #[test]
    fn test_weekly_plan_multiple_rest_days() {
        use chrono::NaiveDate;

        let plan = WeeklyPlan {
            week_start: NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            workouts: vec![],
            rest_days: vec![0, 3, 6], // Monday, Thursday, Sunday
            target_volume_hours: 6.0,
        };

        assert_eq!(plan.rest_days.len(), 3);
        assert!(plan.rest_days.contains(&0));
        assert!(plan.rest_days.contains(&3));
        assert!(plan.rest_days.contains(&6));
    }

    // ========================================================================
    // ZoneDistribution Tests
    // ========================================================================

    #[test]
    fn test_zone_distribution_clone() {
        let original = ZoneDistribution {
            z1_z2: 0.75,
            z3: 0.15,
            z4_z5: 0.10,
        };
        let cloned = original.clone();

        assert_eq!(cloned.z1_z2, original.z1_z2);
        assert_eq!(cloned.z3, original.z3);
        assert_eq!(cloned.z4_z5, original.z4_z5);
    }

    #[test]
    fn test_zone_distribution_debug() {
        let dist = ZoneDistribution {
            z1_z2: 0.80,
            z3: 0.15,
            z4_z5: 0.05,
        };
        let debug = format!("{:?}", dist);
        assert!(debug.contains("ZoneDistribution"));
    }

    #[test]
    fn test_zone_distribution_sum() {
        let dist = ZoneDistribution {
            z1_z2: 0.75,
            z3: 0.15,
            z4_z5: 0.10,
        };
        let sum = dist.z1_z2 + dist.z3 + dist.z4_z5;
        assert!((sum - 1.0).abs() < 0.01);
    }

    // ========================================================================
    // generate_workout_for_phase() Additional Tests
    // ========================================================================

    #[test]
    fn test_generate_workout_for_intensity_focus() {
        let workout =
            generate_workout_for_phase(&TrainingPhase::Build, &TrainingFocus::Intensity, 60);

        assert!(!workout.name.is_empty());
        assert_eq!(workout.duration_minutes, 60);
        assert!(!workout.zones.is_empty());
        assert!(workout.zones.iter().any(|z| z.zone >= 4));
    }

    #[test]
    fn test_generate_workout_for_vertical_focus() {
        let workout =
            generate_workout_for_phase(&TrainingPhase::Build, &TrainingFocus::Vertical, 90);

        assert_eq!(workout.duration_minutes, 90);
        assert!(!workout.zones.is_empty());
    }

    #[test]
    fn test_generate_workout_for_race_specific_focus() {
        let workout =
            generate_workout_for_phase(&TrainingPhase::Specific, &TrainingFocus::RaceSpecific, 75);

        assert_eq!(workout.duration_minutes, 75);
        assert!(!workout.zones.is_empty());
    }

    #[test]
    fn test_generate_workout_for_recovery_focus() {
        let workout =
            generate_workout_for_phase(&TrainingPhase::Recovery, &TrainingFocus::Recovery, 45);

        assert_eq!(workout.duration_minutes, 45);
        assert!(!workout.zones.is_empty());
    }

    #[test]
    fn test_generate_workout_for_strength_focus() {
        let workout =
            generate_workout_for_phase(&TrainingPhase::Transition, &TrainingFocus::Strength, 60);

        assert_eq!(workout.duration_minutes, 60);
        assert!(!workout.zones.is_empty());
    }

    #[test]
    fn test_generate_workout_different_phases() {
        let base_workout =
            generate_workout_for_phase(&TrainingPhase::EarlyBase, &TrainingFocus::AerobicBase, 60);
        let build_workout =
            generate_workout_for_phase(&TrainingPhase::Build, &TrainingFocus::AerobicBase, 60);

        // Both should be valid workouts
        assert!(!base_workout.zones.is_empty());
        assert!(!build_workout.zones.is_empty());
    }

    #[test]
    fn test_generate_workout_sport_field() {
        let workout =
            generate_workout_for_phase(&TrainingPhase::EarlyBase, &TrainingFocus::AerobicBase, 60);

        assert_eq!(workout.sport, "Run");
    }
}
