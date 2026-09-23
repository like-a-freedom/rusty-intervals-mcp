# Progress Tracking Engine Specification

> Status: Draft — grounded against the current codebase as of 2026-05-24; not implemented yet.
> Source of truth: this file.

## Goal

Define a deterministic `track_progress` MCP intent that analyzes recent training history, detects a trailing CTL plateau, summarizes load-management context, surfaces TID drift, and produces evidence-weighted coaching hypotheses, using athlete-aware baselines where the available data supports it and explicit fallback heuristics otherwise.

## Why this exists

The repository already has strong deterministic building blocks for:

- load management (`compute_acwr`, `compute_monotony`, `compute_strain`)
- wellness interpretation (`parse_wellness_metrics`, `compute_hrv_ratio`, `compute_hrv_trend_slope`)
- activity fetching and daily load aggregation (`build_daily_load_series`)

What is missing is a single intent that combines those pieces into a reusable progress-diagnosis workflow.

## Scope

### In scope

- trailing CTL plateau detection from wellness history
- deterministic summary of ACWR, monotony, and strain
- optional TID drift summary from sampled activity details
- optional lnRMSSD rollup from wellness `hrv`
- athlete-aware baselining for HRV / lnRMSSD and for "unusual vs this athlete" load-pattern interpretation when enough history exists
- heuristic hypothesis ranking for volume / intensity distribution / recovery issues
- markdown output optimized for MCP / LLM consumption

### Out of scope

- predictive modeling, model fitting, or athlete-specific causal inference
- validated injury-risk prediction or claims of a fully individualized "optimal" training formula
- UI design beyond markdown/text/table output
- guaranteed sport-type filtering (current `ActivitySummary` shape is insufficient for reliable filtering)

## Tool contract

### Name

`track_progress`

### Input

```json
{
	"type": "object",
	"properties": {
		"period_weeks": {
			"type": "integer",
			"minimum": 4,
			"maximum": 24,
			"default": 12,
			"description": "Lookback window used for progress analysis."
		},
		"hypothesis_mode": {
			"type": "boolean",
			"default": true,
			"description": "When true, include root-cause hypotheses and interventions."
		}
	},
	"required": []
}
```

### Output

The intent returns the repository-standard `IntentOutput` shape:

- `content`: markdown blocks containing the report
- `suggestions`: short actionable recommendations
- `next_actions`: follow-up intent suggestions
- `metadata`: standard output metadata

The rendered report must cover these sections when data is available:

1. plateau detection
2. load context
3. TID drift (optional)
4. HRV / lnRMSSD context (optional)
5. hypotheses + interventions (optional, gated by `hypothesis_mode`)
6. data-quality warnings

## Data sources

### Required

- `get_wellness(Some(period_days))`
	- source for historical CTL-like fitness values **when present in the wellness payload**
	- implementation must tolerate current/observed aliases such as `fitness` and `ctl` rather than assuming one canonical key
	- source for raw `hrv`

### Optional but strongly preferred

- `get_recent_activities(...)`
	- source for training-load aggregation window
	- source for activity ids and dates
- `get_activity_details(activity_id)`
	- source for `icu_training_load` aliases used by load aggregation fallbacks
	- source for `icu_zone_times` and `polarization_index` when present

### Existing helpers that must be reused where possible

- `crate::engines::analysis_fetch::build_daily_load_series`
- `crate::engines::coach_metrics::compute_acwr`
- `crate::engines::coach_metrics::compute_monotony`
- `crate::engines::coach_metrics::compute_strain`
- `crate::engines::coach_metrics::parse_wellness_metrics`
- `crate::engines::coach_metrics::compute_hrv_ratio`
- `crate::engines::coach_metrics::compute_hrv_trend_slope`
- `crate::engines::coach_metrics::classify_tid_model`

## Deterministic processing rules

## 1. Wellness ordering

- Wellness entries must be sorted by date ascending before time-series calculations.
- Entries without a parseable date may be ignored with a warning.

## 2. CTL plateau detection

Plateau detection is a heuristic, not a medical or scientific truth claim.

Required behavior:

- minimum data: 28 CTL points
- compute linear-regression slope over the trailing 28 CTL points
- convert slope to `ctl_per_week`
- when enough athlete history exists, the flat-band threshold should be derived from that athlete's own week-to-week CTL change variability
- classify trend:
	- `flat` when $|slope\_per\_week|$ is inside the athlete-aware flat band, or inside the default fallback band when personalization is unavailable
	- `rising` when slope is positive and outside the flat band
	- `declining` when slope is negative and outside the flat band
- compute trailing plateau duration from contiguous trailing flat windows
- report `plateau_detected = true` only when trailing flat duration is at least 28 days

Important:

- `0.5 CTL/week` is an implementation fallback default only; when enough athlete history exists, the implementation should prefer a band derived from the athlete's own historical CTL variability.
- plateau duration must reflect the trailing flat segment, not the total series length.
- a CTL plateau is a statement about training-load trend, not proof of a physiological adaptation plateau.

## 3. Load-management context

Load context must reuse existing load helpers and remain deterministic:

- ACWR from `compute_acwr`
- monotony from `compute_monotony`
- strain from `compute_strain`

Interpretation rules:

- the raw metrics remain deterministic outputs
- when enough historical load data exists, the report should prefer athlete-relative framing such as "high for this athlete" / "within this athlete's typical range"
- when athlete-specific history is insufficient, the report may fall back to generic heuristic bands and wording
- documentation must not present ACWR, monotony, or strain as validated individual injury-risk predictors

When the daily load history is too short, the report must degrade gracefully and explain that load metrics are unavailable.

## 4. TID drift

TID drift is optional because it depends on activity details.

Rules:

- fetch at most 5 activities per week of lookback, capped at 60 total activities
- sort activities chronologically before grouping
- group by ISO week using activity date
- for each week, compute zone proportions primarily from `icu_zone_times` entries returned by activity detail
- `polarization_index` may be surfaced as supporting context when present, but is not a substitute for the weekly 3-zone proportions required for entropy
- if fewer than 4 weekly groups are available, mark TID drift as unsupported
- weekly entropy is calculated with Shannon entropy on normalized 3-zone proportions
- drift classification is heuristic:
	- `converging` when recent entropy is materially lower than prior entropy
	- `polarizing` when recent entropy is materially higher than prior entropy
	- `stable` otherwise

Important:

- entropy delta thresholds must be documented as heuristics
- athlete-relative entropy comparison may be used descriptively when enough historical weekly data exists, but the feature must not claim a validated personalized "optimal" entropy threshold
- do not assume scalar per-zone fields such as `icu_zone1_secs` / `icu_zone2_secs` / `icu_zone3_secs` are the primary API contract unless they are empirically confirmed in the implementation context
- iterating over `HashMap::values()` without sorting is not acceptable for time-series logic

## 5. HRV / lnRMSSD context

Rules:

- raw wellness `hrv` values are treated as RMSSD
- lnRMSSD is derived with `ln(rmssd)` for positive values only
- when enough history exists, interpretation should be anchored to the athlete's own rolling lnRMSSD baseline and variability rather than population norms
- a rollup may report recent mean, recent CV, and recent slope
- existing `parse_wellness_metrics` remains the primary source for `hrv_ratio`, suppression flags, and trend-state wording

Important:

- this feature must not claim that a fixed `10th percentile` or `1.5× CV` threshold is scientific consensus unless explicitly backed by codebase-approved documentation
- single-day HRV deviations should not drive strong claims without an athlete-specific baseline and, ideally, short-term smoothing / persistence checks
- lnRMSSD metrics are supportive diagnostics, not sole decision criteria

## 6. Hypothesis ranking

Hypothesis ranking is heuristic and deterministic.

Supported domains:

- `Volume`
- `IntensityDistribution`
- `Recovery`

Rules:

- confidence is a weighted evidence score in `[0.0, 1.0]`
- the score must be described as heuristic, not Bayesian
- where athlete-aware baselines are available, evidence features should prefer deviations from the athlete's own history over fixed population-style cutoffs
- every hypothesis must include:
	- evidence bullets
	- suggested intervention
	- tracking metric

## 7. Degraded mode

The intent must still succeed when some inputs are missing.

Expected degraded behavior:

- no CTL-like field in wellness history → no plateau detection
- no recent activity details → no TID drift section
- too little load history → no ACWR / monotony / strain
- no usable HRV → no lnRMSSD section
- insufficient athlete-specific history for personalization → fall back to conservative heuristic defaults and state that personalized interpretation was unavailable

In all cases, the report must emit explicit warnings instead of silently fabricating values.

## Proposed report shape

The internal report model should contain, at minimum:

- plateau result
- ACWR ratio/state
- monotony
- strain
- TID drift summary
- lnRMSSD rollup summary
- HRV suppression / trend summary
- hypotheses
- recommendations
- warnings

Exact Rust type layout is an implementation concern, but the semantic fields above are required.

## Acceptance criteria

- plateau detection is reproducible from the same wellness input
- plateau duration is based on the trailing flat segment, not total lookback length
- TID drift uses chronological weekly grouping
- the documentation and output wording clearly distinguish athlete-aware baselines from fallback heuristics
- the intent compiles without adding new dependencies
- output uses the standard `IntentOutput` contract already used by existing handlers
- all unavailable metrics are reported explicitly, never implied

## Non-goals and anti-requirements

- do not claim the feature is already implemented
- do not cite `docs/PROGRESS_TRACKING_ENGINE.md` as a source; that file does not exist in the repository
- do not describe heuristic thresholds as settled scientific law
- do not require `domains/mod.rs`, `engines/mod.rs`, or `intents/handlers/render/mod.rs`; the current repository uses `domains.rs`, `engines.rs`, `intents/handlers.rs`, and `intents/handlers/render.rs`
