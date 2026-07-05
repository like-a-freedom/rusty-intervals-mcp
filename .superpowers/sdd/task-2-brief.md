# Task 2: Re-add Parameterized Load and Taper Efficiency (Gap 2)

## Context
`parameterized_load()` and `compute_taper_efficiency()` were deleted from `engines/forecast.rs` during audit (0 production callers). The P0 Performance Intelligence plan (P3.1) specified these should be wired into the `plan_training` handler for focus-aware TSB forecasting and taper efficiency rendering.

**THIS TASK:** Re-add `parameterized_load()`, `compute_taper_efficiency()` + constants + tests to `engines/forecast.rs`, then wire into the `plan_training` handler and render taper efficiency output.

## Files to modify

| File | Change |
|---|---|
| `engines/forecast.rs` | Re-add `parameterized_load()` + `compute_taper_efficiency()` + 7 constants + 7 tests |
| `intents/handlers/plan_training.rs` | Replace `estimate_daily_loads()` call with focus-aware `parameterized_load()`, add taper efficiency block |
| `intents/handlers/plan_training.rs` (test) | Update tests that depend on forecast output |

## Exact Changes

### 1. engines/forecast.rs — Re-add constants

After line 19 `const TSB_FRESH_UPPER: f64 = 25.0;`, add:

```rust
/// Default TSS values by training intensity.
/// These are rough defaults; personalized values should scale from athlete's CTL.
const DEFAULT_TSS_EASY: f64 = 50.0;
const DEFAULT_TSS_TEMPO: f64 = 100.0;
const DEFAULT_TSS_HARD: f64 = 150.0;
const DEFAULT_TSS_RACE: f64 = 250.0;

/// Taper efficiency clamp bounds.
const TAPER_EFFICIENCY_MIN: f64 = 0.0;
const TAPER_EFFICIENCY_MAX: f64 = 2.0;

/// Fallback intensity multiplier for unrecognized labels.
const FALLBACK_INTENSITY_MULTIPLIER: f64 = 1.5;
```

### 2. engines/forecast.rs — Re-add functions

After the closing `}` of `project_tsb()` (line 65), add:

```rust
/// Parameterized load values by intensity.
/// Note: these are rough defaults. For personalized forecasting,
/// scale from athlete's CTL (e.g., easy = 0.5×CTL, hard = 1.5×CTL).
pub fn parameterized_load(intensity: &str) -> f64 {
    match intensity {
        "easy" => DEFAULT_TSS_EASY,
        "tempo" => DEFAULT_TSS_TEMPO,
        "hard" => DEFAULT_TSS_HARD,
        "race" => DEFAULT_TSS_RACE,
        _ => DEFAULT_TSS_EASY * FALLBACK_INTENSITY_MULTIPLIER,
    }
}

/// Compute taper efficiency.
/// `actual_volume_reduction_pct`: actual % volume reduction achieved.
/// `target_volume_reduction_pct`: target % volume reduction planned.
/// `tsb_gain`: TSB gain during taper period.
pub fn compute_taper_efficiency(
    actual_volume_reduction_pct: f64,
    target_volume_reduction_pct: f64,
    tsb_gain: f64,
) -> (f64, f64) {
    let efficiency = if target_volume_reduction_pct > 0.0 {
        (actual_volume_reduction_pct / target_volume_reduction_pct)
            .clamp(TAPER_EFFICIENCY_MIN, TAPER_EFFICIENCY_MAX)
    } else {
        1.0
    };
    let tsb_response = if actual_volume_reduction_pct > 0.0 {
        tsb_gain / actual_volume_reduction_pct
    } else {
        0.0
    };
    (efficiency, tsb_response)
}
```

### 3. engines/forecast.rs — Re-add tests

In the `mod tests` block at the bottom, AFTER the existing `tsb_production_classification_transition_band` test (last one at ~line 157), add these 7 tests BEFORE the closing `}` of the mod tests:

```rust
    #[test]
    fn parameterized_load_values() {
        assert!((parameterized_load("easy") - 50.0).abs() < 0.01);
        assert!((parameterized_load("tempo") - 100.0).abs() < 0.01);
        assert!((parameterized_load("hard") - 150.0).abs() < 0.01);
        assert!((parameterized_load("race") - 250.0).abs() < 0.01);
    }

    #[test]
    fn parameterized_load_unknown_intensity_uses_fallback_multiplier() {
        // Unknown intensity defaults to 1.5× easy
        assert!((parameterized_load("xyz") - 75.0).abs() < 0.01);
        assert!((parameterized_load("") - 75.0).abs() < 0.01);
    }

    #[test]
    fn taper_efficiency_perfect() {
        let (efficiency, response) = compute_taper_efficiency(40.0, 40.0, 15.0);
        assert!((efficiency - 1.0).abs() < 0.01);
        assert!((response - 0.375).abs() < 0.01);
    }

    #[test]
    fn taper_efficiency_partial() {
        let (efficiency, _) = compute_taper_efficiency(20.0, 40.0, 8.0);
        assert!((efficiency - 0.5).abs() < 0.01);
    }

    #[test]
    fn taper_efficiency_overreduction_clamps_to_max() {
        let (efficiency, _) = compute_taper_efficiency(80.0, 40.0, 30.0);
        assert!((efficiency - 2.0).abs() < 0.01);
    }

    #[test]
    fn taper_efficiency_zero_target_returns_one() {
        let (efficiency, _) = compute_taper_efficiency(40.0, 0.0, 15.0);
        assert!((efficiency - 1.0).abs() < 0.01);
    }

    #[test]
    fn taper_efficiency_zero_actual_returns_zero_response() {
        let (efficiency, response) = compute_taper_efficiency(0.0, 40.0, 15.0);
        assert_eq!(efficiency, 0.0);
        assert_eq!(response, 0.0);
    }

    #[test]
    fn taper_efficiency_negative_actual_clamped_to_zero() {
        let (efficiency, _) = compute_taper_efficiency(-10.0, 40.0, 5.0);
        assert_eq!(efficiency, 0.0);
    }
```

### 4. intents/handlers/plan_training.rs — Wire parameterized_load

First, add the import at line ~13:
```rust
use crate::engines::forecast::{parameterized_load, project_tsb};
```
(Change existing `use crate::engines::forecast::project_tsb;` to include `parameterized_load`)

Then, replace lines 503-504 (estimate_daily_loads call + project_tsb):
OLD:
```rust
            let daily_loads = estimate_daily_loads(max_hours, weeks);
            let projection = project_tsb(current_ctl, current_atl, &daily_loads);
```

NEW:
```rust
            let daily_tss = parameterized_load(focus.as_str());
            let daily_loads = std::iter::repeat_n(daily_tss, (weeks as usize) * 7).collect::<Vec<_>>();
            let projection = project_tsb(current_ctl, current_atl, &daily_loads);
```

### 5. intents/handlers/plan_training.rs — Add taper efficiency block

AFTER the TSB Forecast rendering (after line 531 `content.push(ContentBlock::table(...))`), add:

```rust
            // Taper efficiency (only relevant when focus is taper)
            if focus == TrainingFocus::Taper {
                let first_tsb = projection.first().map(|p| p.tsb).unwrap_or(0.0);
                let last_tsb = projection.last().map(|p| p.tsb).unwrap_or(0.0);
                let tsb_gain = last_tsb - first_tsb;
                let reduction_pct = 50.0; // ~50% reduction from pre-taper volume
                let target_pct = 40.0;    // standard taper target
                let (efficiency, tsb_response) = crate::engines::forecast::compute_taper_efficiency(
                    reduction_pct, target_pct, tsb_gain,
                );
                let efficiency_label = if efficiency >= 1.0 {
                    "effective"
                } else if efficiency >= 0.7 {
                    "moderate"
                } else {
                    "ineffective"
                };
                content.push(ContentBlock::markdown(format!(
                    "Taper Efficiency\n\
                     ️ Efficiency Ratio: {:.2} ({})\n\
                     ️ TSB Response: {:.1} pts per % volume reduced",
                    efficiency, efficiency_label, tsb_response
                )));
            }
```

### 6. Test updates

The test `test_tsb_forecast_renders_table` (or similar) in plan_training.rs may need adjustment since the forecast values will now be different (using parameterized_load instead of estimate_daily_loads). Find and update the test assertion values.

Search for tests that construct a PlanTrainingHandler and check TSB forecast output. They likely use specific load values that will now change.

## Quality gates
```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```
