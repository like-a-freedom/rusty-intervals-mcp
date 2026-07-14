# Endurance Performance Metrics — Methodology

This document defines the cycling-only, protocol-bound endurance metrics
emitted by `analyze_training` period detail. It is the scientific scope
reference for the implementation in
`crates/intervals_icu_mcp/src/engines/endurance_evidence.rs` and
`crates/intervals_icu_mcp/src/domains/endurance_evidence.rs`.

## Goal

Report two protocol-bound, power-based observations per period:

1. **Submaximal HR–power response.** Compare matched, steady 10-minute
   HR–power windows from the recent cohort (0–14 days before the
   period end) and a personal reference cohort (15–90 days before the
   period end).
2. **Matched early-to-late HR–power shift after prolonged work.**
   Within the most recent eligible ride, pair two matched 600-second
   windows — one ending between 20 and 60 minutes into the ride,
   one ending at or after 120 minutes — and report the HR and
   efficiency deltas at matched power.

The surface deliberately stops at observations and explicit availability
reasons. The renderer never labels a sign as good, bad, ready, fatigued,
durable, or fit.

## Raw protocol constants

All constants are product-level. They define what this MCP feature
recognises as a "control window", "matched pair", or "eligible
candidate". They are not physiological good/bad thresholds.

| Constant | Value | Source |
|---|---|---|
| `CONTROL_WINDOW_S` | 600 s | One matching window. |
| `CONTROL_STRIDE_S` | 60 s | Window snapshot cadence. |
| `CONTROL_MIN_COVERAGE` | 90 % | Per-signal coverage floor. |
| `CONTROL_MAX_POWER_CV_PCT` | 5 % | Steady-power floor. |
| `CONTROL_MIN_EFTP_FRACTION` | 55 % × eFTP | Submaximal intensity floor. |
| `CONTROL_MAX_EFTP_FRACTION` | 80 % × eFTP | Submaximal intensity ceiling. |
| `POWER_MATCH_TOLERANCE` | 5 % | Pair-wise power match. |
| `RECENT_DAYS` | 14 d | Recent cohort ceiling. |
| `REFERENCE_MIN_AGE_DAYS` | 15 d | Reference cohort floor. |
| `REFERENCE_MAX_AGE_DAYS` | 90 d | Reference cohort ceiling. |
| `MIN_COHORT_WINDOWS` | 2 | Per cohort minimum count. |
| `EARLY_END_MIN_S` | 1 200 s (20 min) | Early window earliest end. |
| `EARLY_END_MAX_S` | 3 600 s (60 min) | Early window latest end. |
| `LATE_END_MIN_S` | 7 200 s (120 min) | Late window earliest end. |

## Eligibility predicate

For each control window:

1. **Power band**: `min_eftp_fraction * eFTP ≤ avg_power_w ≤ max_eftp_fraction * eFTP`
2. **Steady power**: `power_cv_pct ≤ 5 %`
3. **Signal coverage**: `power_coverage.ratio ≥ 90 %` AND `hr_coverage.ratio ≥ 90 %`
4. **Finite signal**: both `avg_hr_bpm` and `avg_power_w` are finite and positive
5. **Time span**: a full 600-second window at the chosen stride boundary

Implementations may use any `MetricStreams` parser that preserves the
existing `time`, `watts`, `heartrate` keys and converts null/invalid signal
samples to `f64::NAN` (see `engines/metric_streams.rs`).

## Algorithms

### Sliding-window collection

Two-pointer iteration over the time array:

- `left` / `right` index pointers, both inclusive of the active window
- Running accumulators: sum, sum-of-squares, and non-NaN count for both
  power and HR
- At each stride boundary (`time[right] ≡ n * CONTROL_STRIDE_S` for
  integer `n`), the running statistics are snapshotted into a
  `ControlWindow` candidates

Determinism: ties on `activity_id` handle same-date sessions, ties on
`range.start` handle same-power windows.

### Longitudinal comparison

For each session, retain the single `best_window` (lowest CV, ties
broken by earliest start) eligible under the predicate.

Partition into recent (`0..=14` days) and reference (`15..=90` days)
cohorts. Compute the median power across the recent cohort's accepted
windows as `anchor`. Accept only cohort windows where
`abs(window.avg_power_w - anchor.avg_power_w) / anchor.avg_power_w ≤
POWER_MATCH_TOLERANCE`. Each cohort must contain `MIN_COHORT_WINDOWS`
distinct activity ids.

Report:

- Status when each cohort meets the minimum and matched-power criteria; else
  `InsufficientCandidateSessions` or `NoComparableControlWindows`.
- `hr_delta_bpm = recent_median_hr_bpm - reference_median_hr_bpm`
- `efficiency_delta_pct = 100 * (recent_eff - reference_eff) / reference_eff`
  where `efficiency = avg_power_w / avg_hr_bpm`

### Matched early-to-late pairing

For the most recent session deterministically ordered by date
(alphabetical id tiebreaker), collect all eligible early windows
(end in `[1 200 s, 3 600 s]`) and all eligible late windows
(end `≥ 7 200 s`).

For every `(early, late)` pair with `|late_power - early_power| /
early_power ≤ POWER_MATCH_TOLERANCE`, compute `hr_delta = late_hr -
early_hr`. Select the pair with the smallest relative power difference;
on tie, prefer the largest `|hr_delta|` (most informative observation
of cardiovascular drift).

`matched_power_w = mean(early_power, late_power)`. Report both
source values and the same raw deltas.

## Output contract

`CoachMetrics::endurance_evidence: Option<EnduranceEvidenceMetrics>`
holds the report; the renderer maps that onto
markdown surfaced between `ETVS` and the per-mode requested-metrics
section. The renderer must:

- Show `Protocol: 10-minute windows, ≥90% HR/power coverage, power CV
  ≤5%, 55–80% eFTP; comparisons require power match within 5%.`
- For each numeric block, append one of these token-efficient context
  phrases:

  Submaximal: "Lower HR at matched power may indicate improved aerobic
  efficiency. Higher HR may indicate acute strain, thermal stress, or
  reduced plasma volume. Trend is individual — not a standalone
  diagnosis."

  Prolonged: "Rising HR at steady power (cardiovascular drift) is
  normal during prolonged work. Larger drift than personal baseline may
  reflect fuelling, heat, or residual strain. Individual observation — not
  a durability score."

- Never emit the words `ready`, `durable`, `fatigue(d)`, or `fit` as
  judgements.

## Availability reasons

| Status | Renderer phrase |
|---|---|
| `Available` | Numeric block + context phrase. |
| `UnsupportedSport` | Cycling power data is required; no speed-based estimate was made. |
| `MissingEftp` | eFTP is unavailable, so submaximal intensity could not be standardised. |
| `InsufficientCandidateSessions` | Fewer than two accepted sessions were available in either comparison cohort. |
| `NoComparableControlWindows` | No matched 10-minute HR–power windows were found across the two cohorts. |
| `IncompleteSignalCoverage` | HR or power coverage was below the protocol requirement. |
| `NoEligibleProlongedRide` | No eligible prolonged ride was available for an early-to-late comparison. |
| `NoMatchedEarlyLateWindows` | No early and late control windows matched in power within 5%. |

## Not a diagnostic or composite score

| Direction | Decision | Reason |
|---|---|---|
| HRV/RHR/readiness composite | Excluded | HRV requires standardised longitudinal collection and individual interpretation; multiplying it by RHR/readiness is not a validated physiological construct. Existing HRV rollups remain separate evidence channels. |
| FatOx, glycogen, LEA, hydration, or metabolic scores | Excluded | The available Intervals.icu power/HR streams do not measure substrate utilisation, energy availability, or hydration. |
| Generic durability score | Excluded | The durability literature requires protocol, nutrition, environment, and fatigue-exposure controls; a single linear power-drop score is not adequate. |
| ACWR/monotony/strain variants | Excluded | These already exist and are load descriptors, not new athlete-performance measurements. |
| Critical Power/Speed model | Deferred | Intervals.icu already exposes eFTP/CP models; adding a second model now would duplicate ESPE/eFTP. |
| Running, trail, swimming, speed-only support | Deferred | Speed is strongly confounded by grade, wind, surface, and technique. A valid extension needs an explicit standardised test and sport-specific route/grade controls. |

## Data acquisition contract

The fetch layer (`analysis_fetch::collect_endurance_evidence`) is
bounded, best-effort, and **never fails** the period analysis:

- Up to 12 recent + 12 reference candidate activities selected from a
  single `get_recent_activities(None, Some(90))` call
- Per-candidate `get_activity_details` filtered by `type == "Ride"` and
  `moving_time ≥ 1800 s`
- Per-survivor stream fetch with `buffer_unordered` semantics (3 concurrent
- A single aggregate warning emitted on partial failure:
  `endurance evidence partial: <n> candidate details and <m> ride streams unavailable`
- `analyze_training` `summary` mode skips **both** the fetch and the renderer call

## References

- Buchheit, M. (2014). *Monitoring training status with HR measures: Do
  all roads lead to Rome?* Frontiers in Physiology. [pubmed/24578692](https://pubmed.ncbi.nlm.nih.gov/24578692/)
- Lundstrom, C. J. et al. (2023). *Markers of endurance — HRV or
  running economy?* [pubmed/35853460](https://pubmed.ncbi.nlm.nih.gov/35853460/)
- Maunder, E. et al. (2021). *Durability — Functional vs physiological
  perspectives.* [pubmed/33886100](https://pubmed.ncbi.nlm.nih.gov/33886100/)
- Maunder, E. et al. (2025). *Durability and its quantification in trained cyclists.* [pubmed/40150840](https://pubmed.ncbi.nlm.nih.gov/40150840/)
- Jones, N. et al. (2024). *Session-by-session monitoring of
  durability.* [pubmed/37606604](https://pubmed.ncbi.nlm.nih.gov/37606604/)
