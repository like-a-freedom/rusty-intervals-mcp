# Interval Detection Benchmark Design

**Status:** Legacy baseline and detector wiring approved; accuracy comparison is gated pending a reviewed corpus.

**Scope:** Establish a reproducible behavioral baseline for the current interval-count heuristic and define the corpus contract required before claiming accuracy improvements for a future local detector.

## Problem

`analyze_training` with `analysis_type: "intervals"` requests already structured intervals from Intervals.icu, normalizes `icu_intervals` or `icu_groups`, and calls `count_work_intervals`. The current helper classifies each upstream segment from median `average_speed` and `average_heartrate`; it does not find boundaries in streams and cannot identify session type.

The three supplied GPX tracks are valuable positive candidates, but they are untracked raw GPS files without work/recovery boundaries, session labels, or negative examples. They cannot support an accuracy claim until human-reviewed labels and a held-out corpus exist.

## Decisions

1. Build a **test-only legacy behavioral baseline** now. It freezes the existing median behavior on a versioned, synthetic upstream-interval corpus. Its report contains exact-match rate, count totals, and MAE. It is explicitly not an accuracy benchmark.
2. Do not parse GPX in the MCP crate and do not add an XML, GPX, or Criterion dependency in this iteration. The benchmark uses checked-in JSON fixtures and existing `serde_json` only.
3. Do not commit, move, or expose raw GPS tracks from `data/`. Future checked-in fixtures are derived, anonymized streams without coordinates; their provenance records a source hash maintained separately from the raw file.
4. Keep the existing public MCP input unchanged: `analyze_training` with `analysis_type: "intervals"` remains the only entry point. The future detector is an internal component.
5. Preserve upstream intervals only as a legacy/reference source. The future detector consumes normalized stream samples and cannot treat `icu_groups` as work repetitions.

## Benchmark Layers

| Layer | Input | Result | Valid claim |
|---|---|---|---|
| Legacy behavioral baseline | Synthetic upstream interval objects | Exact legacy count behavior | Regression protection for the old heuristic |
| Corpus validation | Annotated derived streams and manifest | Dataset-readiness report | Labels and split are usable |
| Accuracy baseline | Frozen gold corpus | Session and segment metrics | Measured current-vs-new detector comparison |

Only the first layer is implementable with the present data. The other two are deliberately gated rather than silently filled with inferred labels.

## Corpus Contract for the Accuracy Baseline

Each session has an immutable derived-stream fixture and a separate annotation JSON document. Required annotation fields are:

```json
{
  "schema_version": 1,
  "annotation_version": 1,
  "source": {
    "id": "opaque-session-id",
    "sha256": "source-export-hash",
    "exporter": "Suunto GPX 1.1"
  },
  "groups": {
    "athlete_group": "athlete-a",
    "device_group": "device-a",
    "route_group": "route-family-a"
  },
  "session": {
    "intent_class": "structured_interval",
    "label_confidence": "confirmed_by_athlete",
    "evidence": "workout-plan-and-manual-review",
    "split": "test"
  },
  "segments": [
    {
      "id": "set-1-rep-1",
      "phase": "work_rep",
      "start": { "point_index": 120, "time": "2026-07-09T17:35:00Z" },
      "end": { "point_index": 300, "time": "2026-07-09T17:38:00Z" },
      "set_id": "set-1",
      "rep_index": 1,
      "boundary_tolerance_s": 5,
      "quality_flags": [],
      "excluded_from_scoring": false
    }
  ],
  "exclusions": [],
  "provenance": {
    "annotator": "athlete",
    "reviewer": "independent-reviewer",
    "adjudication": "agreed"
  }
}
```

Valid `intent_class` values are `structured_interval`, `fartlek`, `steady_or_tempo`, `progression`, `mixed`, and `unusable_or_unknown`. Valid segment phases are `warmup`, `work_rep`, `recovery`, `fartlek_surge`, `fartlek_easy`, `steady`, `cooldown`, `pause`, and `unknown`.

All segment ranges are half-open `[start, end)`. A recording gap, uncertain boundary, or noise flag is independent from the phase and determines whether the range participates in scoring.

## Leakage Controls

- Points, segments, derived variants, and annotations from one workout stay in one split.
- The locked test set is grouped at least by session; external generalization is grouped by athlete, device, route, and workout family.
- Duplicate and near-duplicate exports are rejected using source hash, timestamp, and route-family checks.
- Gold labels are created without viewing detector predictions, unless the annotation explicitly records assisted review.
- Filenames, workout names, and source metadata are never detector features unless the same data exists in the real production input.

## Metrics and Acceptance Reporting

The accuracy report, once its corpus gate passes, contains:

- session-level confusion matrix and macro precision, recall, and F1;
- work-repetition precision, recall, and F1 after one-to-one temporal-IoU matching;
- exact rep-count accuracy and count MAE;
- median and P95 absolute start/end boundary error in seconds;
- false positive segments per hour and per non-interval session class;
- 95% bootstrap intervals resampled by session.

Temporal match threshold and boundary tolerance are frozen in the manifest before algorithm tuning. Results are reported by class, not only as a pooled score.

## Future Detector Boundary

The internal detector receives normalized samples rather than upstream interval objects:

```rust
pub(crate) enum SourceFetchState {
    NotRequested,
    Available,
    Empty,
    Failed { reason: String },
}

pub(crate) struct IntervalDetectionResult {
    pub session_kind: SessionKind,
    pub work_segments: Vec<DetectedSegment>,
    pub recovery_segments: Vec<DetectedSegment>,
    pub confidence: Option<f64>,
    pub reasons: Vec<String>,
}
```

The detector pipeline is: validate and resample streams, mark gaps, find candidate change points, form work/recovery candidates, score repetition regularity, classify the session, and return decision-ready reasons. Heart rate is a lagging corroborating signal; velocity and power, when present, are primary change signals. No hardcoded universal pace or heart-rate threshold defines an interval.

`FetchedAnalysisData` retains its legacy optional payloads for compatibility but gains source states. This makes an empty upstream response distinct from a failed endpoint and allows a local result to remain useful when the upstream interval endpoint is unavailable.

## TDD and Verification Contract

Every new benchmark/scorer behavior starts with a focused failing Rust test. A test may not use a mock that only asserts mock behavior, and no production API is widened solely for tests. The legacy baseline runs independently of any future detector so it remains a stable comparison point.

Required checks after benchmark changes are:

```bash
cargo fmt --all -- --check
env RUSTC_WRAPPER= cargo test -p intervals_icu_mcp --lib interval_baseline -- --nocapture
env RUSTC_WRAPPER= cargo clippy --all-targets --all-features -- -D warnings
env RUSTC_WRAPPER= cargo test --all-targets --all-features
```

If dependency resolution or the compiler cache is unavailable, the result is reported as an environment limitation rather than a passing verification result.

## Implementation Status

**Date:** 2026-07-10
**Status:** The legacy behavioral baseline is valid. The checked-in derived-stream corpus is provisional and intentionally rejected for accuracy metrics until it contains real source hashes, leakage-control groups, and independent-review provenance.

### What is implemented

- **Legacy behavioral baseline** (`legacy_work_interval_baseline`): freezes the median
  `count_work_intervals` heuristic on a versioned synthetic upstream-interval
  corpus. Report carries exact-match rate, count totals, and MAE. Not an
  accuracy benchmark.
- **Corpus validation + accuracy scorer** (`interval_benchmark`): `validate_corpus`
  returns blockers (instead of a fabricated score) when labels, source
  hashes, split assignment, or required session classes are absent. `score_sessions`
  performs one-to-one maximum-IoU matching of `work_rep` segments and
  reports segment metrics, per-class metrics, rep-count MAE, and boundary errors.
- **Provisional corpus fixtures** (`tests/fixtures/interval_detection/`):
  coordinate-free stream and annotation examples which exercise the loader and
  prove that `validate_corpus` rejects incomplete provenance. They are not a
  locked gold corpus and must not be used for an accuracy claim.
- **Local detector** (`interval_detection`): consumes normalized streams,
  treats gaps as exclusions, scores repetition regularity, and classifies
  `StructuredIntervals` / `Fartlek` / `Other` / `InsufficientData`.
- **MCP wiring** (`analyze_training` `analysis_type: "intervals"`): adds
  `SourceFetchState` to `FetchedAnalysisData`; runs the local detector when
  streams are available and renders mutually exclusive guidance for structured
  intervals, fartlek/other, upstream failure with local result, and
  unavailable streams. Upstream failure, upstream empty, and unavailable
  streams are visibly distinct. `summary` / `detailed` / `streams` are unchanged.
- **Comparison report** (`compare_legacy_and_candidate`): accepts independently
  captured legacy predictions and local-detector output for a validated corpus.
  It rejects provisional fixtures before scoring, preventing gold annotations
  from leaking into the legacy baseline.

### Release gate

Accuracy claims remain unpublished until human-reviewed labels and the
locked group-held-out corpus pass validation. Only then may the comparison
report become the single source of truth for any future "detector beats
legacy" statement.
