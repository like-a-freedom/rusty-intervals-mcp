# Task 1: Re-add Adaptation State and Wire Into System (Gap 1)

## Context
The comprehensive audit refactor deleted `AdaptationState` + `classify_adaptation()` from `engines/adaptation.rs` and `parameterized_load()` + `compute_taper_efficiency()` from `engines/forecast.rs` because they had zero production callers. However, the P0 Performance Intelligence plan (P1.2 → P1.5) specified these should be wired into TID recommendations.

**THIS TASK:** Re-add `AdaptationState` enum, `classify_adaptation()` function, and modify `compare_power_curves()` in `coach_metrics.rs` to return adaptation state as a 4th tuple element. Then wire it into alerts and rendering.

## Files to modify

| File | Change |
|---|---|
| `engines/adaptation.rs` | Re-add `AdaptationState` enum + `classify_adaptation()` + 14 constants + 7 tests |
| `engines/coach_metrics.rs` | Modify `compare_power_curves()` return type to add 4th element `Option<String>` (adaptation_state) |
| `intents/handlers/analyze_training.rs` | Update destructuring to include `_adaptation_state` |
| `engines/coach_guidance.rs` | Add 2 adaptation-state alerts (adaptation_stalled, adaptation_fatigue) |
| `intents/handlers/render/analysis.rs` | Render adaptation state text in `render_espe_section()` |
| `intents/handlers/render/analysis.rs` (test) | Update `render_espe_section_supported` test to not break |

## Exact Changes

### 1. engines/adaptation.rs

Add AFTER the comment block (after line 8 `// remain in active use by analyze_training.`) and BEFORE the Curve Profile constants section:

```rust
/// Adaptation: Plateau threshold — all deltas below this % indicate no meaningful change.
const ADAPTATION_PLATEAU_THRESHOLD_PCT: f64 = 1.0;
/// Adaptation: Fatigue state — threshold % decline for threshold power.
const ADAPTATION_FATIGUE_THR_THRESHOLD: f64 = -3.0;
/// Adaptation: Fatigue state — threshold % decline for VO2max power.
const ADAPTATION_FATIGUE_VO2_THRESHOLD: f64 = -3.0;
/// Adaptation: VO2 expansion — threshold % gain.
const ADAPTATION_VO2_EXPANSION_THRESHOLD: f64 = 3.0;
/// Adaptation: Aerobic consolidation — threshold % gain for threshold power.
const ADAPTATION_AEROBIC_THR_THRESHOLD: f64 = 1.0;
/// Adaptation: Aerobic consolidation — threshold % gain for endurance power.
const ADAPTATION_AEROBIC_DUR_THRESHOLD: f64 = 2.0;
/// Adaptation: Anaerobic build — threshold % gain for neural/sprint power.
const ADAPTATION_ANAEROBIC_NEURAL_THRESHOLD: f64 = 5.0;
/// Adaptation: Anaerobic build — threshold % gain for 1-minute power.
const ADAPTATION_ANAEROBIC_1M_THRESHOLD: f64 = 2.0;
/// Adaptation: Mixed adaptation — neural gain threshold.
const ADAPTATION_MIXED_NEURAL_THRESHOLD: f64 = 5.0;
/// Adaptation: Mixed adaptation — endurance decline threshold.
const ADAPTATION_MIXED_DUR_DECLINE: f64 = -2.0;
/// Adaptation: Mixed adaptation — VO2 gain threshold.
const ADAPTATION_MIXED_VO2_THRESHOLD: f64 = 3.0;
/// Adaptation: Mixed adaptation — threshold decline.
const ADAPTATION_MIXED_THR_DECLINE: f64 = -1.0;
```

Add `AdaptationState` enum BEFORE `CurveProfile`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AdaptationState {
    Baseline,
    FatigueState,
    Vo2Expansion,
    AerobicConsolidation,
    AnaerobicBuild,
    MixedAdaptation,
    Plateau,
}
```

Add `classify_adaptation()` function BEFORE `classify_curve_profile()`:

```rust
/// Classify adaptation state from 2-window power-curve deltas.
/// `thr_delta` = % change in threshold power (≈20-60min power)
/// `vo2_delta` = % change in VO2max power (≈5min power)
/// `dur_delta` = % change in endurance power (≈60min+ power)
/// `neural_delta` = % change in short sprint power (≈5s-1min)
/// `ana_1m_delta` = % change in 1-minute anaerobic power
pub fn classify_adaptation(
    thr_delta: Option<f64>,
    vo2_delta: Option<f64>,
    dur_delta: Option<f64>,
    neural_delta: Option<f64>,
    ana_1m_delta: Option<f64>,
) -> AdaptationState {
    let all_some =
        thr_delta.is_some() && vo2_delta.is_some() && dur_delta.is_some() && neural_delta.is_some();

    if !all_some {
        return AdaptationState::Baseline;
    }

    let thr = thr_delta.unwrap();
    let vo2 = vo2_delta.unwrap();
    let dur = dur_delta.unwrap();
    let neural = neural_delta.unwrap();
    let ana_1m = ana_1m_delta.unwrap_or(0.0);

    // Plateau: all deltas < 1%
    if thr.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
        && vo2.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
        && dur.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
        && neural.abs() < ADAPTATION_PLATEAU_THRESHOLD_PCT
    {
        return AdaptationState::Plateau;
    }

    // FatigueState: thr < -3% && vo2 < -3%
    if thr < ADAPTATION_FATIGUE_THR_THRESHOLD && vo2 < ADAPTATION_FATIGUE_VO2_THRESHOLD {
        return AdaptationState::FatigueState;
    }

    // Vo2Expansion: vo2_delta > 3%
    if vo2 > ADAPTATION_VO2_EXPANSION_THRESHOLD {
        return AdaptationState::Vo2Expansion;
    }

    // AerobicConsolidation: thr_delta > 1% && dur_delta > 2%
    if thr > ADAPTATION_AEROBIC_THR_THRESHOLD && dur > ADAPTATION_AEROBIC_DUR_THRESHOLD {
        return AdaptationState::AerobicConsolidation;
    }

    // AnaerobicBuild: neural_delta > 5% && ana_1m_delta > 2%
    if neural > ADAPTATION_ANAEROBIC_NEURAL_THRESHOLD && ana_1m > ADAPTATION_ANAEROBIC_1M_THRESHOLD
    {
        return AdaptationState::AnaerobicBuild;
    }

    // MixedAdaptation: conflicting signals
    if (neural > ADAPTATION_MIXED_NEURAL_THRESHOLD && dur < ADAPTATION_MIXED_DUR_DECLINE)
        || (vo2 > ADAPTATION_MIXED_VO2_THRESHOLD && thr < ADAPTATION_MIXED_THR_DECLINE)
    {
        return AdaptationState::MixedAdaptation;
    }

    AdaptationState::Baseline
}
```

Add tests in the `mod tests` block at the bottom (before the curve_profile tests or after, order doesn't matter):

```rust
    #[test]
    fn adaptation_baseline_when_no_data() {
        assert_eq!(
            classify_adaptation(None, None, None, None, None),
            AdaptationState::Baseline
        );
    }

    #[test]
    fn adaptation_fatigue_state() {
        assert_eq!(
            classify_adaptation(Some(-5.0), Some(-4.0), Some(-2.0), Some(-1.0), None),
            AdaptationState::FatigueState
        );
    }

    #[test]
    fn adaptation_vo2_expansion() {
        assert_eq!(
            classify_adaptation(Some(1.0), Some(5.0), Some(0.5), Some(2.0), None),
            AdaptationState::Vo2Expansion
        );
    }

    #[test]
    fn adaptation_aerobic_consolidation() {
        assert_eq!(
            classify_adaptation(Some(2.0), Some(1.0), Some(3.0), Some(1.0), None),
            AdaptationState::AerobicConsolidation
        );
    }

    #[test]
    fn adaptation_anaerobic_build() {
        assert_eq!(
            classify_adaptation(Some(0.5), Some(1.0), Some(0.5), Some(7.0), Some(3.0)),
            AdaptationState::AnaerobicBuild
        );
    }

    #[test]
    fn adaptation_plateau() {
        assert_eq!(
            classify_adaptation(Some(0.5), Some(0.3), Some(0.4), Some(0.2), None),
            AdaptationState::Plateau
        );
    }

    #[test]
    fn adaptation_mixed_when_conflicting() {
        assert_eq!(
            classify_adaptation(Some(0.5), Some(1.0), Some(-3.0), Some(6.0), None),
            AdaptationState::MixedAdaptation
        );
    }

    #[test]
    fn adaptation_baseline_all_present_no_match() {
        assert_eq!(
            classify_adaptation(Some(0.5), Some(1.0), Some(0.5), Some(1.0), None),
            AdaptationState::Baseline
        );
    }
```

### 2. engines/coach_metrics.rs: compare_power_curves

Current signature (line 1012-1019):
```rust
pub fn compare_power_curves(
    current: &EspeDerivedMetrics,
    previous: &EspeDerivedMetrics,
) -> (
    std::collections::HashMap<String, f64>,
    f64,
    std::collections::HashMap<String, String>,
) {
```

New signature: add `Option<String>` as 4th return value for adaptation_state.

At the END of the function, just before the closing `}` (after line 1061 `(deltas, rotation_index, statuses)`), compute and return adaptation_state:

```rust
use crate::engines::adaptation::classify_adaptation;

let adaptation_state = classify_adaptation(
    deltas.get("20m").copied(),
    deltas.get("5m").copied(),
    deltas.get("60m").copied(),
    deltas.get("1m").copied(),
    deltas.get("1m").copied(),
);
let adaptation_state_str = if adaptation_state != crate::engines::adaptation::AdaptationState::Baseline {
    Some(format!("{:?}", adaptation_state))
} else {
    None
};

(deltas, rotation_index, statuses, adaptation_state_str)
```

**IMPORTANT**: The import `use crate::engines::adaptation::classify_adaptation` and `use crate::engines::adaptation::AdaptationState` must be added at the top of the function or at the module level.

### 3. intents/handlers/analyze_training.rs

Change line 1486-1487 from:
```rust
let (deltas, rotation, statuses) =
    crate::engines::coach_metrics::compare_power_curves(&espe, &espe);
```
to:
```rust
let (deltas, rotation, statuses, _adaptation_state) =
    crate::engines::coach_metrics::compare_power_curves(&espe, &espe);
```

### 4. engines/coach_guidance.rs

In `build_alerts()` function, after the `race_readiness` block (around line 470), add:

```rust
    // Adaptation state alerts
    if let Some(espe) = &metrics.espe_derived
        && let Some(ref state) = espe.adaptation_state
    {
        if state == "Plateau" {
            alerts.push(CoachAlert {
                severity: CoachAlertSeverity::Caution,
                code: "adaptation_stalled".to_string(),
                title: "Adaptation plateau detected".to_string(),
                evidence: vec!["Power-curve deltas below threshold — no meaningful adaptation across any system.".to_string()],
                section: "adaptation".to_string(),
            });
        }
        if state == "FatigueState" {
            alerts.push(CoachAlert {
                severity: CoachAlertSeverity::Priority,
                code: "adaptation_fatigue".to_string(),
                title: "Fatigue-dominant adaptation pattern".to_string(),
                evidence: vec!["Threshold and VO2max power declining — consider reducing load or adding recovery.".to_string()],
                section: "adaptation".to_string(),
            });
        }
    }
```

### 5. domains/coach.rs

Add `adaptation_state: Option<String>` to `EspeDerivedMetrics` struct:

```rust
pub struct EspeDerivedMetrics {
    pub glycolytic_bias: Option<f64>,
    pub aerobic_durability: Option<f64>,
    pub durability_gradient: Option<f64>,
    pub balance_score: Option<f64>,
    pub vo2_reserve_ratio: Option<f64>,
    pub p1m: Option<f64>,
    pub p5m: Option<f64>,
    pub p20m: Option<f64>,
    pub p60m: Option<f64>,
    pub supported: bool,
    pub adaptation_state: Option<String>,  // NEW
}
```

No need to change `Default` — `Option<String>` defaults to `None`.

### 6. intents/handlers/render/analysis.rs

In `render_espe_section()`, AFTER line 1099 (the `if let Some(val) = derived.vo2_reserve_ratio` block) and before line 1101 `Some(lines.join("\n"))`, add:

```rust
        if let Some(ref state) = derived.adaptation_state {
            lines.push(format!("  Adaptation State: {}", state));
        }
```

Also update test `render_espe_section_supported` at line 2723 to include `adaptation_state: None` in the `EspeDerivedMetrics` construction. Currently it's:
```rust
let derived = EspeDerivedMetrics {
    glycolytic_bias: Some(3.2),
    supported: true,
    ..Default::default()
};
```
This is fine as-is since `adaptation_state` defaults to `None` via `Default`.

## Quality gates
```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```
