# SPEC: Intervals.icu MCP (Concise SRS + Tool Reference)

Crate: `intervals_icu_client` (workspace member `crates/intervals_icu_client`)

> **📊 API Coverage:** See [API_DIFF.md](./API_DIFF.md) for detailed comparison of MCP coverage vs available Intervals.icu API endpoints.

> **⚙️ Architecture:** Tools are **dynamically generated** from the OpenAPI specification. The available toolset varies based on the API version and tag-scoping configuration. See [ARCHITECTURE.md](./ARCHITECTURE.md) for details.

> **🎯 Intent-Driven API (v2.0+):** Starting with version 2.0, the primary API surface consists of **8 high-level intents** that reduce token consumption by 95% and simplify LLM interactions. See [INTENT_DRIVEN_SKILLS.md](./INTENT_DRIVEN_SKILLS.md) for complete specifications.

> **🧠 Analytical Coach Layer:** The deterministic coach/reporting layer is specified in [ANALYTICAL_COACH_REPORTING_SRS.md](./ANALYTICAL_COACH_REPORTING_SRS.md). In the current implementation it enriches `analyze_training`, `assess_recovery`, and `analyze_race` with shared readiness, load, trend, alert, and guidance logic, and `compare_periods` reuses the same shared snapshot/trend helpers for like-for-like comparisons.

> **📡 Observability:** Prometheus metrics for HTTP mode are specified in [OBSERVABILITY_SRS.md](./OBSERVABILITY_SRS.md). Covers upstream API health, MCP protocol layer, HTTP transport, auth/security, and active athlete tracking.

### Deterministic Coach Analytics Layer

**Version:** 2.x+  
**Status:** Implemented  
**Scope:** Internal analytics engine for read-only intents

The server includes a deterministic coaching analytics layer that provides data-driven insights without relying on LLM computations. This layer is embedded within existing intents rather than exposed as a separate tool.

#### Architecture

The coach layer follows a strict five-stage pipeline:

```text
IntervalsClient → Fetch → Audit → Compute → Interpret → Render → IntentOutput
```

| Stage | Responsibility | Key Modules |
|-------|----------------|-------------|
| **Fetch** | Gather raw data via `IntervalsClient` | `analysis_fetch.rs` |
| **Audit** | Check data quality and availability | `analysis_audit.rs` |
| **Compute** | Calculate derived metrics | `coach_metrics.rs` |
| **Interpret** | Map metrics to states and alerts | `coach_guidance.rs` |
| **Render** | Generate intent-specific sections | Intent handlers |

#### Internal Contract: `CoachContext`

All analytics flow through a shared internal contract:

```rust
pub struct CoachContext {
    pub meta: CoachMeta,           // Analysis kind, window, timestamps
    pub audit: DataAudit,          // Data availability checks
    pub metrics: CoachMetrics,     // Volume, fitness, wellness, trends
    pub alerts: Vec<CoachAlert>,   // Detected issues with severity
    pub guidance: CoachGuidance,   // Findings, suggestions, next actions
}
```

**Key Properties:**
- **Deterministic**: Same inputs always produce same outputs
- **Explicit missing data**: Does not fabricate numbers; marks sections as unavailable
- **Testable**: All rules are unit-tested
- **Explainable**: Every alert includes evidence

#### Enhanced Intents

Three primary intents consume the shared analytics engine:

| Intent | Analytics Mode | Key Sections |
|--------|----------------|--------------|
| `analyze_training` (single) | Workout context | Summary, read-only workout comments, execution context, `Efficiency Factor`, `Aerobic Decoupling`, quality findings, data availability |
| `analyze_training` (period) | Volume + trend | Period totals, trend context, future planned workouts in-window, `ACWR`, `Monotony`, `Strain`, `Fatigue Index`, `Stress Tolerance`, `Durability Index`, `Polarisation Ratio`, load classification, guidance |
| `assess_recovery` | Readiness + red flags | Wellness snapshot, `Readiness Score`, `Recovery Index`, `Consistency Index`, fitness state, red flags, recovery guidance |
| `analyze_race` | Race-specific | Race summary, execution pattern, post-race load, execution metrics, recovery guidance |
| `compare_periods` | Trend comparison | Period stats, trend deltas, volume analysis, `Polarisation Ratio` delta, `Consistency Index` comparison |

#### Metrics and Thresholds

**Fitness/Load (from `get_fitness_summary()`):**

| Metric | State | Threshold |
|--------|-------|-----------|
| TSB | Fresh | > 10 |
| TSB | Neutral | -10 ≤ TSB ≤ 10 |
| TSB | Fatigued | TSB < -10 |
| TSB | **Alert: Deep Fatigue** | TSB < -20 |

**Wellness (from `get_wellness()`):**

| Metric | State | Threshold |
|--------|-------|-----------|
| Sleep | Good | ≥ 7.0h |
| Sleep | Fair | 6.0–7.0h |
| Sleep | Poor | < 6.0h |
| Sleep | **Alert: Low Sleep** | < 6.5h |
| RHR | Normal | ≤ 55 bpm |
| RHR | Elevated | 56–60 bpm |
| RHR | **Alert: High RHR** | > 60 bpm |
| HRV | Stable | ≥ 60 ms |
| HRV | Low | 40–60 ms |
| HRV | **Alert: Very Low** | < 40 ms |
| Recovery Index | **Alert: Recovery First** | < 0.60 |
| Readiness Score | **Alert: Low Readiness** | < 5.0 |
| Readiness Score | Good | ≥ 7.0 |

**Volume (aggregated from activities):**

| Metric | Interpretation | Threshold |
|--------|----------------|-----------|
| Weekly avg | Low volume | < 5 hours |
| Weekly avg | Optimal | 5–15 hours |
| Weekly avg | High volume | > 15 hours |

**Load management (conditional on sufficient history):**

| Metric | Interpretation | Threshold |
|--------|----------------|-----------|
| ACWR | Productive | 0.8–1.3 |
| ACWR | Watch | > 1.3 and ≤ 1.5 |
| ACWR | Overreaching | > 1.5 |
| Monotony | Repetitive stress | > 2.5 |
| Fatigue Index | **Alert: High Fatigue** | > 2.5 |
| Stress Tolerance | Sustainable range | 3–6 |
| Durability Index | **Alert: Low Durability** | < 0.85 |
| Durability Index | Robust | ≥ 0.90 |

**Execution metrics (conditional on streams):**

| Metric | Interpretation | Threshold |
|--------|----------------|-----------|
| Aerobic Decoupling | Acceptable | ≤ 5% |
| Aerobic Decoupling | Watch | > 5% and ≤ 10% |
| Aerobic Decoupling | High | > 10% |

**Distribution metrics (conditional on zone-time data):**

| Metric | Interpretation | Threshold |
|--------|----------------|-----------|
| Polarisation Ratio | Threshold-biased | < 0.75 |
| Polarisation Ratio | Polarised (Seiler 80/20) | 0.75–1.0 |
| Polarisation Ratio | High-intensity dominant | > 1.0 |

**Adherence metrics (conditional on planned events):**

| Metric | Interpretation | Threshold |
|--------|----------------|-----------|
| Consistency Index | Excellent | ≥ 0.9 |
| Consistency Index | Good | ≥ 0.7 |
| Consistency Index | Moderate | ≥ 0.5 |
| Consistency Index | Low | < 0.5 |

#### API-First Principle

All metrics follow a strict data-source priority:

1. **API-native value** — use as-is when Intervals.icu provides it (e.g., `Activity.polarization_index`, `Wellness.readiness`, `Wellness.ctl`/`atl`)
2. **Aggregated from API fields** — compute from structured API data (e.g., `icu_zone_times` → zone percentages, `ctl - atl` → TSB)
3. **Derived from raw data** — compute from daily series (e.g., EWMA ACWR from 28-day load series, monotony from 7-day loads)
4. **Estimated from histograms** — last resort, requires zone boundary mapping (e.g., histogram buckets → zone percentages)

Metrics are marked as `api_value`, `derived`, or `estimated` in `meta.data_sources`.

#### Degraded Mode Handling

When data is unavailable, the system:

1. **Does NOT fabricate numbers** — missing data is a first-class state
2. **Marks sections as unavailable** — explicit `Data Availability` section
3. **Records reasons** — `audit.degraded_mode_reasons[]` contains specific explanations
4. **Adjusts guidance** — suggestions account for missing data

Additionally, readiness-positive wording is suppressed when supportive wellness inputs are missing, so the server does not claim an athlete is ready for key work on freshness alone.

#### Current Boundary

Polarisation metrics use a caller-provided zone-percentage input (`compute_polarisation(z1_pct, z2_pct, z3_pct)`). The histogram-to-zones mapping from the Intervals.icu API is a follow-up task — the current iteration provides the deterministic computation and classification layer only.

Consistency index (`sessions_completed / sessions_planned`) is defined and uses existing `get_events()` + `get_recent_activities()` data sources.

Example degraded mode reasons:
- "activities unavailable for requested window"
- "wellness data unavailable or empty"
- "fitness summary unavailable"
- "interval data unavailable"
- "stream data unavailable"

#### Implementation Modules

| Module | Responsibility |
|--------|----------------|
| `domains/coach.rs` | Domain types: `CoachContext`, `CoachMetrics`, `CoachAlert`, etc. |
| `engines/analysis_fetch.rs` | Request builders, fetch helpers, previous window calculation |
| `engines/analysis_audit.rs` | Data quality checks, availability flags |
| `engines/coach_metrics.rs` | Metric derivation, parsing, trend snapshots |
| `engines/coach_guidance.rs` | Alert generation, guidance mapping |

#### Why Deterministic Analytics?

| Benefit | Description |
|---------|-------------|
| **Token efficiency** | LLM receives computed insights, not raw data (95% reduction) |
| **Consistency** | Same inputs → same outputs (no LLM variance) |
| **Testability** | All rules covered by unit tests |
| **Explainability** | Every alert includes evidence array |
| **No hallucinations** | Metrics computed server-side in Rust |

---

## Краткая цель

- Реализовать MCP-сервер (совместимый по поведению с eddmann/intervals-icu-mcp) для взаимодействия с Intervals.icu API.
- Аудитория: атлеты, пользователи Intervals.icu, разработчики интеграций.
- Язык реализации: Rust.
- Ограничение: аутентификация — токен (см. Intervals.icu docs). Поддерживаем одиночный ключ (env). REST пути требуют обязательного path-параметра athlete id `/api/v1/athlete/{id}/…`, клиент должен подставлять `INTERVALS_ICU_ATHLETE_ID`.

## Intent-Driven API (Primary Interface)

**Starting with v2.0**, the MCP server exposes **8 high-level intents** as the primary API surface for LLM interactions. These intents encapsulate complex orchestration logic internally while presenting a simple, business-oriented interface.

### 8 High-Level Intents

| Intent | Purpose | Mutating | Token Reduction |
|--------|---------|----------|-----------------|
| [`intervals_plan_training`](#intent-plan_training) | Planning across horizons (microcycle → annual) | ✅ | 95% |
| [`intervals_analyze_training`](#intent-analyze_training) | Analysis (single workout or period) | ❌ | 95% |
| [`intervals_modify_training`](#intent-modify_training) | CRUD operations (modify, create, delete) | ✅ | 95% |
| [`intervals_compare_periods`](#intent-compare_periods) | Like-for-like performance comparison | ❌ | 95% |
| [`intervals_assess_recovery`](#intent-assess_recovery) | Recovery assessment + red flags | ❌ | 95% |
| [`intervals_manage_profile`](#intent-manage_profile) | Profile, zones, thresholds | ✅ | 95% |
| [`intervals_manage_gear`](#intent-manage_gear) | Equipment tracking | ✅ | 95% |
| [`intervals_analyze_race`](#intent-analyze_race) | Post-race analysis | ❌ | 95% |

**Key Benefits:**
- **95% token reduction**: 8 intents vs 146 tools (~14,000 → ~800 tokens of metadata)
- **Single-call workflows**: LLM calls one intent instead of orchestrating 6+ API calls
- **Business identifiers**: Use `date`, `description` instead of system IDs (`event_id`)
- **Guidance-driven responses**: Every response includes `suggestions` and `next_actions`
- **Idempotency**: All mutating operations support `idempotency_token` (TTL: 24h)

### Intent Specifications Summary

For complete specifications, see [INTENT_DRIVEN_SKILLS.md](./INTENT_DRIVEN_SKILLS.md).

#### Intent: `plan_training`

**Purpose:** Training planning across arbitrary horizons (1 week to annual plan) with actual event creation in Intervals.icu.

**Input:**
```json
{
  "period_start": "2026-03-01",
  "period_end": "2026-06-15",
  "focus": "aerobic_base",
  "target_race": "50K Ultramarathon",
  "race_date": "2026-06-15",
  "max_hours_per_week": 10,
  "adaptive": true,
  "idempotency_token": "plan-50k-2026-06-15-12weeks"
}
```

**Output:** Markdown report with athlete context (FTP, HR zones, TSB, readiness), periodization structure, race anchors, sample week with zone targets, and created event count. Events are **actually created** in Intervals.icu via `bulk_create_events()`.

**Data-flow:**

| Phase | API Call | Purpose |
|-------|----------|---------|
| REQUIRED | `get_athlete_profile()` | Athlete name |
| REQUIRED | `get_sport_settings()` | FTP, LTHR, zones for intensity targets |
| REQUIRED | `get_fitness_summary()` | CTL/ATL/TSB for starting load + CTL for max_hours validation |
| REQUIRED | `get_wellness(14)` | Readiness, HRV, sleep for adaptive start |
| REQUIRED | `get_events(horizon)` | Conflict detection + race anchors |
| ADAPTIVE | `get_recent_activities(56)` | Historical 8-week volume via moving_time → validate max_hours |
| ADAPTIVE | `get_upcoming_workouts()` | Existing planned workouts → avoid duplication |
| WRITE | `bulk_create_events()` | Actual event creation |

**Business Rules:**
- Volume progression: max +7-10% per week
- Recovery weeks: every 3-4 weeks (-40-60%), only for aerobic_base/intensity/specific focus
- Taper: 7-10 days (50K), 10-14 days (100K), 14-21 days (100+ miles)
- Conflict detection: abort if Workout/Note/Plan events overlap planned period; RaceA/B excluded
- Upcoming workouts from library also count as conflicts
- Volume validation: warn if requested max_hours > 130% of historical average (actual weeks, not hardcoded)
- Wellness adaptation: uses latest entry (not 14-day average)

---

#### Intent: `analyze_training`

**Purpose:** Workout analysis — single session or period summary.

**Input (single):**
```json
{
  "target_type": "single",
  "date": "2026-03-02",
  "description_contains": "long run",
  "analysis_type": "detailed"
}
```

**Input (period):**
```json
{
  "target_type": "period",
  "period_start": "2026-02-01",
  "period_end": "2026-02-28",
  "analysis_type": "summary",
  "metrics": ["time", "distance", "vertical", "tss"]
}
```

**Output:** Structured `IntentOutput` with baseline analysis sections, an explicit `Requested Metrics` table when `metrics` is provided, and a read-only `Workout Comments` section when the source activity exposes activity messages.

**Contract details:**
- `metrics` requests are never silently ignored; each requested item is surfaced as `available`, `unavailable`, or `unsupported`.
- Exact `tss` is only returned when an exact TSS field exists in the source activity payload; generic load proxies are not renamed to `tss`.
- `include_histograms` is supported for `target_type: "single"` only. Period requests with `include_histograms: true` must fail validation.
- Single-workout analysis may surface user/coach comments from Intervals.icu `GET /api/v1/activity/{id}/messages`; these comments are read-only and rendered as ordinary `IntentOutput.content` blocks rather than a special top-level field.
- `target_type: "period"` merges completed activities with future planned `WORKOUT` calendar events that fall inside the requested window; calendar duplicates linked via `paired_activity_id` are skipped so completed sessions are not double-counted.

---

#### Intent: `modify_training`

**Purpose:** Adjust existing workouts (modify, move, create, delete).

**Input:**
```json
{
  "action": "modify",
  "target_date": "2026-03-07",
  "new_date": "2026-03-08",
  "dry_run": false,
  "idempotency_token": "modify-2026-03-07-to-2026-03-08"
}
```

**Contract details:**
- Event resolution is calendar-aware: historical dates are looked up from `get_events(...)`, while future dates are resolved from planned `WORKOUT` calendar entries via `get_upcoming_workouts(...)`.
- `target_description_contains` matches against both event `name` and event `description`.
- `dry_run` previews the concrete field changes that would be sent via `update_event`/`delete_event`.
- `dry_run` previews are not cached by idempotency middleware, so a follow-up apply call can reuse the same token without receiving the preview response back from cache.
- Non-`dry_run` mutations bind `idempotency_token` to one exact request fingerprint; reusing the same token for a different mutation payload must fail with an idempotency conflict.
- `action: "create"` accepts `target_date` as an alias for `new_date` when the caller provides the new workout date in the target slot.

**Safety:** Destructive operations (`action: "delete"`) require `dry_run: true` first.

---

#### Intent: `compare_periods`

**Purpose:** Like-for-like performance comparison between periods.

**Input:**
```json
{
  "period_a_start": "2026-02-01",
  "period_a_end": "2026-02-28",
  "period_b_start": "2026-01-01",
  "period_b_end": "2026-01-31",
  "workout_type": "tempo",
  "metrics": ["volume", "zones", "tss", "pace"]
}
```

**Output:** Markdown table with metrics, deltas, and aerobic efficiency trends.

**Implemented metrics:** `volume`, `pace`, `hr`, `tss`, `zones`, `intensity`.
- `zones`: aggregated from `icu_zone_times` (power zones) or `icu_hr_zone_times` per activity
- `intensity`: weekly TSS average from `icu_training_load` sum

**Business Rules:**
- Volume change: primary metric is `time_delta_pct` (time-based), secondary is `distance_delta_pct`. For trail/mountain, time is more reliable than distance.
- Elevation delta > 30% → suggest additional recovery and hill-specific training.

---

#### Intent: `assess_recovery`

**Purpose:** Recovery assessment, readiness check, red flag detection.

**Input:**
```json
{
  "period_days": 7,
  "for_activity": "intensity",
  "include_wellness": true,
  "include_red_flags": true
}
```

**Red Flags:**
- Sleep <6.5h for 3+ nights
- Resting HR +3-5 bpm for 2+ weeks
- HRV -20% from baseline
- HR drift >10%

**Output:** Readiness status, red flag table, recommendations.

**Look-ahead:** Fetches `get_upcoming_workouts(7, 5)` to check if key workouts/races are scheduled tomorrow — verdict tightens if so.

**Threshold constants:** Readiness thresholds (`sleep`, `tsb`, `recovery_index`) must reference named constants from `coach_guidance`, not inline magic numbers.

---

#### Intent: `manage_profile`

**Purpose:** Athlete profile, zones, thresholds, and fitness snapshot management.

**Input (view):**
```json
{
  "action": "get",
  "sections": ["overview", "zones", "thresholds", "metrics"]
}
```

**Input (update):**
```json
{
  "action": "update_thresholds",
  "new_aet_hr": 155,
  "new_lt_hr": 175,
  "apply_to_activities": true
}
```

**Output:** Profile header plus section-specific `IntentOutput` blocks for overview, zones, thresholds, and metrics.

**Contract details:**
- `zones`/`thresholds` are derived from the live `/sport-settings` payload, including the current array-shaped response used by the Intervals.icu API.
- `metrics` renders the latest fitness snapshot (`CTL`/`ATL`/`TSB` + load state) when available.

---

#### Intent: `manage_gear`

**Purpose:** Equipment management (view, add, retire).

**Input:**
```json
{
  "action": "list",
  "gear_type": "shoes"
}
```

**Output:** Gear list with mileage, remaining life, wear status.

---

#### Intent: `analyze_race`

**Purpose:** Post-race analysis: results, strategy, comparison to plan.

**Input:**
```json
{
  "date": "2026-03-01",
  "description_contains": "50K",
  "analysis_type": "performance",
  "compare_to_planned": true
}
```

**Output:** Race results, segment analysis, strategy evaluation, recommendations.

---

## Dynamic OpenAPI Tools (Internal Use)

**Note:** The 146 dynamic OpenAPI tools are used **internally** by intent handlers and are not exposed directly to LLM hosts. This section documents the underlying implementation.

The MCP tools are generated **dynamically** from the OpenAPI spec (`operationId` + schema).

Внутренняя модульная организация (implementation note)
-------------------------------------------------------
- Для снижения связности и размера `crates/intervals_icu_mcp/src/lib.rs` параметры/DTO инструментов вынесены в `crates/intervals_icu_mcp/src/types.rs`.
- Повторно используемая логика фильтрации/компактизации сосредоточена в `crates/intervals_icu_mcp/src/compact.rs`.
- Оркестрация long-running downloads и webhook HMAC/dedupe логики вынесена в `crates/intervals_icu_mcp/src/services.rs`.
- Доменная логика событий/дат (compact/normalize helpers) вынесена в `crates/intervals_icu_mcp/src/domains/events.rs`.
- Доменная логика wellness (summary/filter helpers) вынесена в `crates/intervals_icu_mcp/src/domains/wellness.rs`.
- Доменная логика activity analysis (streams/intervals/curves/histogram/best-efforts) вынесена в `crates/intervals_icu_mcp/src/domains/activity_analysis.rs`.
- Публичное поведение инструментов и JSON-схемы сохраняются эквивалентными предыдущей реализации (поведенческая совместимость).

Статус реализации инструментов (сравнение с upstream eddmann/intervals-icu-mcp)
------------------------------------------------------------------------------
**Важно:** Точное количество инструментов зависит от версии OpenAPI спецификации Intervals.icu. Ниже приведены ориентировочные значения.

| Категория | Примерное количество | Статус |
|-----------|---------------------|--------|
| Activities | ~11 | ✅ |
| Activity Analysis | ~8 | ✅ |
| Athlete | ~2 | ✅ |
| Wellness | ~3 | ✅ |
| Events/Calendar | ~9 | ✅ |
| Performance/Curves | ~3 | ✅ |
| Workout Library | ~2-5 | ✅ |
| Gear Management | ~6 | ✅ |
| Sport Settings | ~5 | ✅ |
| **MCP Resources** | **1** | ✅ |
| **MCP Prompts** | **7** | ✅ |

> **Примечание:** Для получения актуального списка инструментов используйте MCP метод `list_tools()` или команду в MCP клиенте: "What tools are available?"
**Activities (11 tools):**
- ✅ `get_recent_activities` — реализовано
- ✅ `get_activity_details` — реализовано
- ✅ `search_activities` — реализовано
- ✅ `search_activities_full` — реализовано
- ✅ `get_activities_csv` — реализовано
- ✅ `get_activities_around` — реализовано
- ✅ `update_activity` — реализовано
- ✅ `delete_activity` — реализовано
- ✅ `download_activity_file` — реализовано (как `start_download`)
- ✅ `download_fit_file` — реализовано (отдельный endpoint для FIT)
- ✅ `download_gpx_file` — реализовано (отдельный endpoint для GPX)`,

**Activity Analysis (8 tools):**
- ✅ `get_activity_streams` — реализовано
- ✅ `get_activity_intervals` — реализовано
- ✅ `get_best_efforts` — реализовано
- ✅ `search_intervals` — реализовано
- ✅ `get_power_histogram` — реализовано
- ✅ `get_hr_histogram` — реализовано
- ✅ `get_pace_histogram` — реализовано
- ✅ `get_gap_histogram` — реализовано

**Athlete (2 tools):**
- ✅ `get_athlete_profile` — реализовано
- ✅ `get_fitness_summary` — реализовано (CTL/ATL/TSB анализ)

**Wellness (3 tools):**
- ✅ `get_wellness` — реализовано
- ✅ `get_wellness_for_date` — реализовано
- ✅ `update_wellness` — реализовано

**Events/Calendar (9 tools):**
- ✅ `get_events` — реализовано
- ✅ `get_upcoming_workouts` — реализовано
- ✅ `get_event` — реализовано
- ✅ `create_event` — реализовано
- ✅ `update_event` — реализовано
- ✅ `delete_event` — реализовано
- ✅ `bulk_create_events` — реализовано
- ✅ `bulk_delete_events` — реализовано
- ✅ `duplicate_event` — реализовано

**Performance/Curves (3 tools):**
- ✅ `get_power_curves` — реализовано
- ✅ `get_hr_curves` — реализовано
- ✅ `get_pace_curves` — реализовано

**Workout Library (2 tools):**
- ✅ `get_workout_library` — реализовано
- ✅ `get_workouts_in_folder` — реализовано

**Gear Management (6 tools):**
- ✅ `get_gear_list` — реализовано
- ✅ `create_gear` — реализовано
- ✅ `update_gear` — реализовано
- ✅ `delete_gear` — реализовано
- ✅ `create_gear_reminder` — реализовано
- ✅ `update_gear_reminder` — реализовано

**Sport Settings (5 tools):**
- ✅ `get_sport_settings` — реализовано
- ✅ `update_sport_settings` — реализовано
- ✅ `apply_sport_settings` — реализовано
- ✅ `create_sport_settings` — реализовано
- ✅ `delete_sport_settings` — реализовано

### Оставшиеся инструменты (0 из 55):
- Все инструменты из upstream перечня реализованы.


Краткое SRS (бизнес-сценарио и логика)
-------------------------------------
- Просмотр и поиск активностей: вернуть сводки и детальные объекты; поддерживать параметр `limit`, `days_back` и текстовый поиск.

Token Efficiency
----------------
- Added compact-response options to reduce tokens when interacting with LLMs:
  - `get_activity_streams` supports `max_points`, `summary`, and `streams` to downsample or replace arrays with statistics.
  - `get_activity_details` defaults to a compact summary unless `expand=true` is provided; `fields` allows returning just requested fields.
- Intent handler output uses **compact markdown** in `ContentBlock::Markdown`:
  - `# Title` instead of `## Title` for section headers
  - Plain text instead of `### Title` for subsections
  - `Key: value` instead of `**Key:** value` for key-value pairs
  - 2-space indent instead of `- item` for list items
  - Single `\n` instead of `\n\n` between sections
  - No emoji (✅/⚠️) in text blocks; status lives in Table columns
- Recommendation: adopt "summary-first" workflows — request compact summaries, then `expand=true` or `max_points` when raw data is needed.
- Prompts and tool descriptions were shortened to reduce metadata/token overhead while retaining tool names and usage hints.
- Работа с событиями/календарём: создание/обновление/удаление/дублирование/массовые операции; валидация дат в формате `YYYY-MM-DD` и полей категорий. Поле даты в API — `start_date_local`.
- Аналитика активности: потоки (streams), интервалы, гистограммы, best-efforts, кривые мощности/пейса/ЧСС с опцией `time window`.
- Управление профилем и настройками: чтение/обновление sport-settings, gear и wellness; преобразования единиц (m/km, s/HH:MM, pace format).
- Файлы: загрузка/скачивание (GPX/FIT/оригинал) с потоковой записью на диск; возврат base64 для небольших файлов.
- Надёжность: применять retry/backoff для transient-ошибок, уважать `Retry-After` при 429 и иметь лёгкий rate-limiter для outgoing запросов.
- Webhooks: предусмотреть безопасный потребитель (HMAC verification), дедупликацию по event UID и идемпотентную обработку.

Аутентификация
---------------
- Поддерживается token-based авторизация согласно документации Intervals.icu (API key). Клиент должен уметь использовать ключ из конфигурации/ENV.
- Рекомендуется: хранить токены в окружении (`INTERVALS_ICU_API_KEY` и `INTERVALS_ICU_ATHLETE_ID`) и не логировать значения.
- **Fail-fast on missing creds:** the HTTP MCP server must validate that `INTERVALS_ICU_API_KEY` and `INTERVALS_ICU_ATHLETE_ID` are set before binding to a network socket; if either is missing the server MUST exit with a non-zero status and emit a clear **info-level** log message explaining which env vars are required. This prevents accidental deployments without credentials and surfaces the problem early in logs and orchestrator events.

Примечание из upstream README: API key используется в HTTP Basic Auth с username `API_KEY` и самим ключом как паролем; реализация setup обычно сохраняет ключ и athlete id в `.env` (интерактивный setup или вручную).

OpenAPI runtime / cache / tag-scope
-----------------------------------
- `INTERVALS_ICU_OPENAPI_SPEC` — опциональный source спеки (URL или локальный JSON путь).
- Если source не задан, runtime загружает `${INTERVALS_ICU_BASE_URL}/api/v1/docs` с локальным fallback на `docs/intervals_icu_api.json`.
- `INTERVALS_ICU_SPEC_REFRESH_SECS` (default `300`) задаёт периодические попытки refresh кэша реестра tools.
- При ошибке refresh (network/parse) runtime MUST продолжить работу на последнем валидном cached registry.

Поведение клиента и нефункциональные требования
------------------------------------------------
- Retry policy: экспоненциальный backoff с jitter для сетевых/5xx ошибок; уважать `Retry-After` при 429.
- Rate limits: конфигурируемый лимитер (max concurrent requests, per-second cap).
- Idempotency: по возможности поддерживать `Idempotency-Key` и локальную дедупликацию для повторных write-запросов.
- File handling: потоковая загрузка/сохранение для больших файлов (не держать весь файл в памяти).
- Observability: метрики (requests, failures, retries, webhook events), structured logs и health/readiness endpoints.

Tool Reference (сводка)
-----------------------
Формат: `tool-name` — короткая цель; входы (валидация) — Intervals API endpoint(ы) — поведение/заметки — ошибки.

- `get_recent_activities` — список недавних активностей; входы: `limit` (int ≤100), `days_back` (int); GET /api/v1/athlete/{id}/activities; возвращает список summary объектов; ошибки: validation_error, api_error.

- `get_activity_details` — детальные данные активности; входы: `activity_id`, optional `expand` (boolean), optional `fields` (array). GET /api/v1/activity/{id}. По умолчанию MCP возвращает компактную сводку (subset полей) чтобы снизить токен-стоимость в LLM workflows; укажите `expand=true` чтобы получить полный объект (streams остаются отдельным вызовом), или укажите `fields` чтобы вернуть конкретные поля (например `["id","distance","moving_time"]`). **Важно:** это изменение делает поведение по умолчанию компактным — если вы полагаетесь на полный payload по умолчанию, обновите запросы с `expand=true` (breaking change).

- `search_activities` / `search_activities_full` — текстовый поиск; входы: `q` (non-empty, query string), `limit`; GET /athlete/{id}/activities/search и /activities/search-full; **note:** the upstream API expects the query parameter name `q` (not `query`); the client validates that `q` is non-empty and forwards it as `q` to avoid 422 responses; errors: validation_error.

- `get_activity_streams` — time-series streams; входы: `activity_id`, optional `streams` list, optional `max_points` (integer) and `summary` (boolean). GET /activity/{id}/streams; возвращает map streams → arrays по умолчанию. Если `max_points` задан — сервер делает downsampling до указанного количества точек (равномерная выборка); если `summary=true` — возвращаются только статистики (count/min/max/avg/p10/p50/p90) вместо полных массивов. Также доступна фильтрация по `streams` (например, `power`, `heartrate`). Это позволяет существенно сократить токены при взаимодействии с LLM (см. раздел "Token Efficiency").

- `get_activity_intervals` / `get_best_efforts` — интервалы и best-efforts; входы: `activity_id`; GET /activity/{id}/intervals, /best-efforts; возвращает структурированные интервалы.

- `get_activities_csv` — Download activities as CSV; входы: none; GET /api/v1/athlete/{id}/activities.csv; возвращает CSV body as text.
- `get_activities_around` — получить активности до и после указанной активности (контекст); входы: `activity_id`, `count` (default 5); GET /api/v1/athlete/{id}/activities-around; возвращает списки before/after и относительные позиции.

- `update_activity` — частичное обновление метаданных; входы: `activity_id` + fields; PUT /activity/{id}; требуется минимум 1 изменяемое поле; ошибки: validation_error, api_error.

- `delete_activity` — удаление по `activity_id`; DELETE /activity/{id}; возвращает confirmation.

- `download_activity_file` / `download_fit_file` / `download_gpx_file` — скачивание файлов; входы: `activity_id`, optional `output_path`; GET /activity/{id}/file|/fit-file|/gpx-file; поведение: потоковая запись на диск при `output_path`, иначе base64.

- Events / Calendar: `get_calendar_events`, `get_event`, `create_event`, `update_event`, `delete_event`, `bulk_create_events`, `bulk_delete_events`, `duplicate_event` — валидация даты `YYYY-MM-DD` (поле `start_date_local`), категории совместимы с Intervals.icu (`WORKOUT`, `RACE_A/B/C`, `NOTE`, `PLAN`, `HOLIDAY`, `SICK`, `INJURED`, `TARGET`, `SET_FITNESS`, …); POST/PUT/DELETE к /athlete/{id}/events endpoints; bulk удаление — `PUT /events/bulk-delete` c массивом объектов `{id|external_id}`; дублирование — `POST /duplicate-events` с полями `eventIds`, `numCopies`, `weeksBetween`.
	- NOTE: `get_event` expects an *event id* (calendar event). Passing an `activity_id` (e.g. an id belonging to an activity) will return the activity payload from the API which does not match the `Event` schema; the client now returns a descriptive decoding error that includes a short snippet of the received body to aid debugging.
	- NOTE: Event IDs in the API are numeric; the MCP tools accept either numbers or strings and normalize them before calling the client to avoid deserialization errors.

- `get_upcoming_workouts` — возвращает ближайшие запланированные тренировки; входы: `limit` (default 7); GET /athlete/{id}/events?filter=workouts or dedicated endpoint; возвращает отфильтрованные события.

- `search_intervals` — поиск интервалов по длительности/интенсивности; входы: `minSecs`, `maxSecs`, `minIntensity`, `maxIntensity` (обязательные), опционально `type` (AUTO|POWER|HR|PACE), `minReps`, `maxReps`, `limit`; GET /api/v1/athlete/{id}/activities/interval-search.

- Wellness: `get_wellness_data`, `get_wellness_for_date`, `update_wellness` — даты, числовые поля; PUT/GET к /athlete/{id}/wellness endpoints; учитывать `locked` поведение при слияниях от интеграций.

- `get_athlete_profile` — профиль атлета и спортивные настройки; входы: none; GET /athlete/{id}; возвращает `profile`, `fitness` и `sports`.

- `get_fitness_summary` — агрегированные CTL/ATL/TSB и интерпретация; входы: optional `time_period`/`days_back`; чтение данных проводится через `GET /api/v1/athlete/{id}` (поля `fitness`, `fatigue`, `form` включены в объект атлета); возвращает summary и рекомендации.

- Gear: `get_gear_list`, `create_gear`, `update_gear`, `delete_gear`, reminder endpoints — POST/PUT/DELETE к /athlete/{id}/gear; создание напоминаний — `POST /gear/{gearId}/reminder`; обновление — `PUT /gear/{gearId}/reminder/{reminderId}` c query `reset` и `snoozeDays`; конвертации единиц (km→m, hours→seconds).

- Sport Settings: `get_sport_settings`, `update_sport_settings`, `apply_sport_settings`, `create_sport_settings`, `delete_sport_settings` — работы с порогами FTP/FTHR/pace; `GET /sport-settings`, `PUT /sport-settings/{id}` (update, требуется query `recalcHrZones`), `PUT /sport-settings/{id}/apply` (apply settings, без start_date), `POST /sport-settings` (create); форматирование pace (MM:SS per km).

- Performance / Curves: `get_power_curves`, `get_hr_curves`, `get_pace_curves` — входы: `days_back`/`time_period`, **`type` (sport, required)**, опция `use_gap`; GET /athlete/{id}/power-curves etc.; возвращает peak_efforts и анализ FTP/HR.  
  *Note: the upstream API requires the `type` query parameter (e.g., `Ride`, `Run`) — the client and MCP tools now require and forward this parameter to avoid 422 responses.*

- Гистограммы: `get_power_histogram`, `get_hr_histogram`, `get_pace_histogram`, `get_gap_histogram` — входы: `activity_id`; GET /activity/{id}/power-histogram, /hr-histogram, /pace-histogram, /gap-histogram; возвращают бин-распределения и время/количество в бинах.

- Workout Library: `get_workout_library`, `get_workouts_in_folder` — GET /athlete/{id}/folders и /athlete/{id}/workouts; API не фильтрует по папке, поэтому клиент фильтрует по `folder_id` локально.
  **Добавлено (CRUD для folders/training plans):**
  - `create_folder` — создание новой папки/плана; POST /athlete/{id}/folders; входы: `folder` (JSON с name/description), `compact` (default true), `response_fields`; token-efficient: compact по умолчанию возвращает только id/name/description.
  - `update_folder` — обновление папки; PUT /athlete/{id}/folders/{id}; входы: `folder_id`, `fields`, `compact` (default true), `response_fields`; token-efficient: компактный ответ с фильтрацией полей.
  - `delete_folder` — удаление папки; DELETE /athlete/{id}/folders/{id}; входы: `folder_id`; возвращает `{"deleted": true}`.

- Webhook consumer (recommended, optional): endpoint для приёма событий; HMAC verification; dedupe by event uid; return 2xx quickly; idempotent processing to handle retries.

MCP Resource & Prompts
----------------------
- MCP Resource: `intervals-icu://athlete/profile` — предоставляет постоянный контекст для LLM (профиль атлета, текущие метрики, sport settings). **Статус: РЕАЛИЗОВАН**.
- MCP Prompts (шаблоны): `analyze-recent-training`, `performance-analysis`, `activity-deep-dive`, `recovery-check`, `training-plan-review`, `plan-training-week`, `analyze-and-adapt-plan` — используются как готовые LLM-запросы для типичных сценариев. **Статус: РЕАЛИЗОВАНЫ**.

**Реализация:**
1. ✅ MCP Resource через `ServerHandler::list_resources()` и `ServerHandler::read_resource()` для URI `intervals-icu://athlete/profile`
2. ✅ 7 MCP Prompts через декларативную регистрацию в `IntervalsMcpHandler::available_prompts()` и роутинг в `IntervalsMcpHandler::prompt_from_request()`:
	- `analyze-recent-training` (days_back)
	- `performance-analysis` (days_back, sport_type/metric)
	- `activity-deep-dive` (activity_id)
	- `recovery-check` (days_back)
	- `training-plan-review` (start_date)
	- `plan-training-week` (start_date, focus)
	- `analyze-and-adapt-plan` (period/days_back, focus)

**Источник шаблонов (upstream cb91d4a0):** взяты тексты промптов из `eddmann/intervals-icu-mcp/src/intervals_icu_mcp/server.py` и адаптированы без дубликатов.

Тестирование и валидация
------------------------
- Unit tests: валидация входов, преобразования единиц, форматирование pace/time.
- Integration tests: мок HTTP-сервер с зафиксированными ответами (VCR-like) для ключевых endpoints.
- Webhook tests: дедуп, HMAC verification, idempotency.

Операционная заметка
---------------------
- Логи: компактный, человекочитаемый формат по умолчанию (timestamp, level, message). По умолчанию мы подавляем излишне подробные внутренние структуры (например, полные Debug-репрезентации RMCP `peer_info`) — для этого установлено `rmcp=warn` в фильтре логов. Для более подробного вывода можно включить `RUST_LOG=debug` или `RUST_LOG=rmcp=debug`.

  Пример удобочитаемой записи:

  2025-12-16T12:31:11.791 [INFO] intervals_icu_mcp: registered tools=53 prompts=7
  2025-12-16T12:31:11.792 [INFO] intervals_icu_mcp: starting stdio MCP server
  2025-12-16T12:31:12.000 [DEBUG] intervals_icu_mcp: rmcp client connected name="Visual Studio Code" version="1.107.0"

  (Детальные поля RMCP доступны при включении `debug`.)
- Health: readiness / liveness endpoints; конфигурация rate-limiter и retry policy внешне конфигурируемы через переменные окружения.

Rust MCP SDK — архитектурные заметки
------------------------------------
- Используемая база: официальная Rust MCP SDK (`modelcontextprotocol/rust-sdk`). Она поддерживает несколько транспортов: stdio, SSE, streamable HTTP (Axum) и предоставляет `ServerHandler` trait для реализации MCP сервера.
- **Архитектура инструментов:** MCP tools **не используют** `#[tool]` макросы — вместо этого применяется **динамическая генерация** из OpenAPI спецификации через `DynamicRuntime` и `DynamicRegistry`.
- **Динамический dispatch:** `list_tools()` возвращает инструменты из `DynamicRegistry`, `call_tool()` диспатчит через `DynamicRuntime::dispatch_openapi()`.
- Транспорты и развёртывание:
	- `stdio`: лёгкий способ запустить MCP сервер, полезен для локальных и container-stdin подходов (должно быть совместимо с LLM-хостом через stdio bridge).
	- `sse` и `streamable http` (Axum): для HTTP-хостинга и поддержки прогресса/streaming ответов.
	- Выбор транспорта влияет на деплой: stdio → container/CLI, HTTP/SSE → web service (Axum/tokio).
- Инструменты и валидация:
	- **Динамическая генерация:** инструменты создаются из OpenAPI spec при старте сервера
	- **JSON Schema:** генерируется автоматически из OpenAPI schemas
	- **Валидация:** выполняется RMCP SDK на основе JSON Schema
- Долгие операции и прогресс:
	- SDK примеры включают `progress_demo` с прогресс-уведомлениями — используйте streaming/SSE или MCP progress notifications для долгих задач (file download, large conversions).
- Тестирование и примеры:
	- Репозиторий SDK содержит примеры: `servers_counter_stdio`, `servers_prompt_stdio`, `servers_progress_demo` — используйте их как шаблоны для unit/integration tests.
	- Запуск примеров: `cargo run --example servers_counter_stdio` или `cargo run --example servers_progress_demo -- http`.
- Рекомендуемые добавочные crates и практики:
	- HTTP client: `reqwest` или `hyper` + `tower` middleware (retry, timeout).
	- Async runtime: `tokio`.
	- JSON/Schema: `serde`, `schemars` (для JSON Schema generation), используйте SDK-валидацию где возможно.
	- Rate-limiter: `governor` или `leaky-bucket`.
	- Retries: `tower-retry` or custom backoff with `tokio::time::sleep` and jitter.
	- Logging/metrics: `tracing`, `opentelemetry`, `prometheus` exporter or `metrics` crate.
	- Secrets: `secrecy` crate, OS keyring, or encrypted storage for tokens (if storing beyond env vars).
- Архитектурная рекомендация для интеграции Intervals.icu client:
	- HTTP-клиент Intervals.icu вынесен в trait (`IntervalsClient`) для DI и тестирования
	- Реализация: `ReqwestIntervalsClient` с logging middleware
	- DynamicRuntime не зависит от конкретного client implementation

---

## История изменений

### Версия 0.9.0 (Dynamic OpenAPI Architecture)

**Полный переход на динамическую генерацию инструментов из OpenAPI спецификации:**

**Архитектурные изменения:**
- ✅ Удалены все `#[tool]` макросы и хардкоженные инструменты
- ✅ Внедрён `DynamicRuntime` для генерации tools из OpenAPI spec
- ✅ Универсальный `dispatch_openapi()` для всех инструментов
- ✅ Кэширование registry с periodic refresh (`INTERVALS_ICU_SPEC_REFRESH_SECS`)
- ✅ Token efficiency: `compact`, `fields`, `body_only` параметры

**Новые ENV переменные:**
```bash
INTERVALS_ICU_OPENAPI_SPEC          # URL или путь к spec (опционально)
INTERVALS_ICU_SPEC_REFRESH_SECS     # Частота refresh кэша (default: 300)
```

**Статус миграции:** Завершено (SRS_DYNAMIC_OPENAPI_CUTOVER.md)

---
