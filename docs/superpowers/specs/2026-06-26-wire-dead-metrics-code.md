# Wire Dead Metrics Code into Production (v4 — Strengthened)

## Problem Statement

Audit of the metrics calculation logic found 11 dead code items. After verification, 5 items cannot be wired (missing API data or MCP contract break), 1 is already wired, and 5 can be wired. The highest-value item is `AnalysisEngine::compare_periods` which provides richer comparison than the current ad-hoc rendering in `compare_periods.rs`.

## Full MCP Response Path

```
compare_periods.rs handler
  → builds IntentOutput { content: Vec<ContentBlock>, suggestions, next_actions }
  → lib.rs call_tool() receives IntentOutput
  → intent_output_to_call_tool_result() serializes to CallToolResult
  → CallToolResult.structured_content = serde_json::to_value(output)
  → MCP client receives JSON with content, suggestions, next_actions
```

## Current Handler Output Structure

The `compare_periods.rs` handler currently produces this ContentBlock sequence:

1. `Markdown("# Comparison: {a} vs {b}")` — title
2. `Table(Metric | A | B | Δ)` — ad-hoc comparison: Activities, Total Time, Distance, Elevation  
3. `Markdown(Fitness Snapshot)` — CTL, ATL, TSB, Ramp Rate (if available)
4. `Markdown(Requested Metrics)` + `Table(...)` — user-requested metrics (if any)
5. `Markdown(Trend Context)` — activity/time/distance/elevation deltas, weekly avg, consistency
6. Suggestions (volume change, elevation change)
7. Next actions (analyze_training, assess_recovery, optional: recovery week for volume spikes)

## Revised Plan (Strengthened)

### Phase 1: Wire `AnalysisEngine::compare_periods` + render into MCP response

**Task 1.1: Add robust `build_period_summary` helper in `compare_periods.rs`**

Add a private function that maps `PeriodStats` → `PeriodSummary` with proper error handling:

```rust
fn build_period_summary(stats: &PeriodStats) -> PeriodSummary {
    let total_tss: f32 = stats.activities.iter()
        .filter_map(|activity| {
            stats.activity_details.get(&activity.id)
                .and_then(|detail| detail.get("icu_training_load"))
                .and_then(|value| {
                    value.as_f64()
                        .or_else(|| value.as_i64().map(|n| n as f64))
                        .or_else(|| value.as_str().and_then(|s| s.parse::<f64>().ok()))
                })
        })
        .sum::<f64>() as f32;
    
    let total_time_hours = stats.snapshot.total_time_secs as f32 / 3600.0;
    let weeks = (stats.window_days.max(1) as f32 / 7.0).max(1.0); // Protect against div by zero
    
    PeriodSummary {
        workout_count: stats.snapshot.activity_count as u32,
        total_time_hours,
        total_distance_km: stats.snapshot.total_distance_m as f32 / 1000.0,
        total_elevation_m: stats.snapshot.total_elevation_m as f32,
        avg_weekly_hours: total_time_hours / weeks,
        total_tss,
        avg_tss_per_week: total_tss / weeks,
    }
}
```

Add unit tests:
- `build_period_summary` handles missing/null/invalid `icu_training_load` gracefully
- `build_period_summary` with zero activities produces zero values
- `build_period_summary` with negative `window_days` doesn't crash (protected by `max(1)`)
- `build_period_summary` correctly converts string `icu_training_load` values

**Task 1.2: Call `AnalysisEngine::compare_periods` in handler**

In `compare_periods.rs`, after building `a_stats` and `b_stats` (line 100), add:

```rust
let a_summary = build_period_summary(&a_stats);
let b_summary = build_period_summary(&b_stats);
let comparison = AnalysisEngine::compare_periods(&a_summary, &b_summary, a_label, b_label);
```

Keep existing `derive_trend_metrics` call — it's still needed for TrendContext section.

**Task 1.3: Replace ad-hoc comparison table with engine output**

Replace the ad-hoc table construction (lines 124-181) with rendering of `comparison.metrics` with appropriate formatting:

```rust
// Build comparison table from engine output with proper formatting
let mut rows = vec![vec![
    "Metric".into(),
    a_label.into(),
    b_label.into(),
    "Δ".into(),
]];
for m in &comparison.metrics {
    let formatted_a = if m.name == "Workouts" {
        format!("{:.0}", m.period_a_value)
    } else {
        format!("{:.1}", m.period_a_value)
    };
    let formatted_b = if m.name == "Workouts" {
        format!("{:.0}", m.period_b_value)
    } else {
        format!("{:.1}", m.period_b_value)
    };
    rows.push(vec![
        m.name.clone(),
        formatted_a,
        formatted_b,
        format!("{:+.1} ({:+.0}%)", m.delta_absolute, m.delta_percent),
    ]);
}
content.push(ContentBlock::table(rows[0].clone(), rows[1..].to_vec()));
```

Add integration tests:
- Handler output table contains "Workouts", "Volume (hours)", "Distance (km)", "TSS" rows
- Workouts are formatted as integers, other metrics as 1 decimal place
- Delta percentages are whole numbers (no decimals)

**Task 1.4: Integrate engine summary with existing suggestions**

The engine's `comparison.summary` replaces the ad-hoc volume analysis, but we must preserve the elevation change suggestion and volume spike next action:

```rust
// Engine summary replaces ad-hoc volume analysis
suggestions.push(comparison.summary.clone());

// Preserve elevation change suggestion (engine doesn't cover this)
if let Some(elev_delta) = trend.elevation_delta_pct
    && elev_delta.abs() > 30.0
{
    suggestions.push(format!(
        "Elevation change: {:+.0}% - consider extra recovery and hill-specific work",
        elev_delta
    ));
}

// Preserve volume spike next action (engine doesn't cover this)  
let volume_change = if let Some(time_delta_pct) = trend.time_delta_pct {
    time_delta_pct as f32
} else if let Some(distance_delta_pct) = trend.distance_delta_pct {
    distance_delta_pct as f32
} else {
    0.0
};

if volume_change > 15.0 {
    next_actions.insert(0, "Consider recovery week if volume spike continues".into());
}
```

Remove the ad-hoc `volume_change` calculation and `if/else` block that generates the same text.

Add integration test:
- Handler suggestions include both engine summary and elevation change when applicable
- Handler next_actions include volume spike warning when applicable

**Task 1.5: Keep all other sections unchanged**

These sections are NOT replaced by `AnalysisEngine::compare_periods`:
- **TrendContext** (lines 227-245): Uses `derive_trend_metrics` output — keep as-is
- **Requested Metrics** (lines 207-226): Uses `requested_metric_value()` — keep as-is  
- **Fitness Snapshot** (lines 183-205): Uses `parse_fitness_metrics` — keep as-is

**Task 1.6: Run quality gates and commit**

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`  
- `cargo test --all-targets --all-features`
- Commit: `refactor: wire AnalysisEngine::compare_periods into compare_periods handler`

### Phase 2: Delete truly dead code

**Task 2.1: Delete unused types from `analysis.rs`**

Remove types that are never instantiated:
- `AnalysisType` enum (line 13-18) — no consumer
- `WorkoutAnalysis` struct (line 22-29) — never instantiated (would need `ContentBlock::Json` to wire)
- `PeriodAnalysis` struct (line 59-65) — never instantiated

Keep: `PeriodSummary` (now wired), `ZoneDistribution` (different from planning.rs), `TrendInsight`, `TrendDirection`.

**Task 2.2: Delete functions needing unavailable API data from `coach_metrics.rs`**

- `normalize_running_power` (line 1316-1322) — needs `source` param not available from API
- `gap_to_running_power` (line 1327-1335) — needs GAP histogram, extra API call
- `aggregate_wdr_metrics_7d` (line 1178-1218) — needs `intervals_map` not fetched by any handler
- Related constants: `STRYD_POWER_CORRECTION`, `GARMIN_RP_POWER_CORRECTION`, `GAP_SPEED_EXPONENT`, `GAP_POWER_COEFFICIENT`, `GAP_UPHILL_GRADIENT_FACTOR`, `GAP_DOWNHILL_GRADIENT_FACTOR`

**Task 2.3: Run quality gates and commit**

- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-targets --all-features`
- Commit: `refactor: remove dead code that cannot be wired (missing API data)`

### Phase 3: Final verification

**Task 3.1: Run full quality gates**

**Task 3.2: Verify MCP compatibility**

The MCP client receives the same JSON structure but with enhanced content:
```json
{
  "content": [
    {"type": "markdown", "markdown": "# Comparison: ..."},
    {"type": "table", "headers": ["Metric", "A", "B", "Δ"], "rows": [
      ["Workouts", "12", "10", "+2 (+20%)"], 
      ["Volume (hours)", "8.5", "7.2", "+1.3 (+18%)"],
      ["Distance (km)", "150.3", "135.7", "+14.6 (+11%)"],
      ["TSS", "850.0", "780.0", "+70.0 (+9%)"]
    ]},
    {"type": "markdown", "markdown": "Fitness Snapshot\n  CTL: ..."},
    {"type": "markdown", "markdown": "Trend Context\n  ..."}
  ],
  "suggestions": [
    "Volume increased by 18% - monitor for overtraining",
    "Elevation change: +45% - consider extra recovery and hill-specific work"
  ],
  "next_actions": [
    "Consider recovery week if volume spike continues",
    "To analyze a specific period: analyze_training ...",
    "To assess recovery: assess_recovery"
  ]
}
```

**Backward compatibility:** 
- Same JSON structure (no breaking change)
- Table headers unchanged (`Metric | A | B | Δ`)
- All existing sections preserved
- New richer comparison data in same format
- Existing suggestions/next_actions logic preserved

## Decision Document

- **Modules modified:** `engines/analysis.rs` (delete dead types), `engines/coach_metrics.rs` (delete dead functions), `intents/handlers/compare_periods.rs` (wire engine, render output)
- **Interfaces:** `AnalysisEngine::compare_periods` called with `PeriodSummary` built from `PeriodStats`; `LikeForLikeComparison` rendered as markdown table with proper formatting
- **MCP contract:** Same `CallToolResult` structure. Comparison table enhanced with Workouts/Volume/Distance/TSS rows. No breaking changes.
- **What stays unchanged:** TrendContext section, Requested Metrics section, Fitness Snapshot section, Elevation change suggestion, Volume spike next action
- **What was NOT wired and why:** `normalize_running_power` (no source metadata), `gap_to_running_power` (needs extra API call), `aggregate_wdr_metrics_7d` (needs intervals data), `WorkoutAnalysis`/`PeriodAnalysis` (MCP contract break), `AnalysisType` (no consumer), `ZoneDistribution` consolidation (not duplicates)

## Testing Decisions

- Unit test: `build_period_summary` handles edge cases (missing/null/invalid data, zero activities, negative window_days)
- Integration test: handler comparison table has 4 rows (Workouts, Volume, Distance, TSS) with proper formatting
- Integration test: handler suggestions include both engine summary and elevation change when applicable  
- Integration test: handler next_actions include volume spike warning when applicable
- Integration test: TrendContext, Requested Metrics, Fitness Snapshot sections unchanged
- Prior art: existing tests in `compare_periods.rs` (integration tests at lines 1015-1194)

## Out of Scope

- Changing MCP tool signatures or argument schemas
- Adding `ContentBlock::Json` variant (breaking MCP contract)
- Wiring functions that need API data not currently fetched
- Consolidating `analysis.rs::ZoneDistribution` with `planning.rs::ZoneDistribution` (different types, different purposes)
- Modifying TrendContext, Requested Metrics, or Fitness Snapshot rendering
- Performance optimization of calculation functions
