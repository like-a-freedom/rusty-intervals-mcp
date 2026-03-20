# Token-Efficient Intervals.icu MCP Server

A high-performance, **token-efficient** Model Context Protocol (MCP) server for Intervals.icu written in Rust. Optimized for LLMs to minimize context window pressure while providing deep access to training data, wellness metrics, and performance analysis.

[![.github/workflows/ci.yml](https://github.com/like-a-freedom/rusty-intervals/actions/workflows/ci.yml/badge.svg)](https://github.com/like-a-freedom/rusty-intervals/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/like-a-freedom/rusty-intervals?label=release)](https://github.com/like-a-freedom/rusty-intervals/releases)
[![codecov](https://codecov.io/gh/like-a-freedom/rusty-intervals-mcp/graph/badge.svg?token=I47UV16VY5)](https://codecov.io/gh/like-a-freedom/rusty-intervals-mcp)
![Rust](https://img.shields.io/badge/rust-1.92+-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)

## Token Efficiency: The LLM-First Advantage

Standard MCP servers often overwhelm LLMs with raw, verbose data, consuming thousands of tokens and degrading reasoning performance. This server is built with a **Token-First** philosophy:

- **Server-Side Data Compression**: Intelligently downsamples high-resolution stream data (heart rate, power, etc.) using configurable sampling, reducing payload size by up to 90% while preserving critical trends.
- **Pre-computed Summaries**: Calculates statistical insights (min/max/average/total) on the server, saving the LLM from expensive and error-prone numerical computations within the context.
- **Smart Field Filtering**: Tools default to compact summaries, with opt-in expansion for specific fields, ensuring the LLM only receives what is necessary for the task.
- **Concise Definitions**: Tool and prompt descriptions are optimized for brevity to minimize the static overhead in every LLM turn.

## Overview

This MCP server builds its MCP tools **dynamically from the Intervals.icu OpenAPI spec**, mirroring the elegant design of https://github.com/derrix060/intervals-mcp but rewritten in Rust for performance and maintainability.

**Why choose this project?**

1. **Token‑efficiency by default.** Every tool response is automatically compacted to minimise LLM context usage without sacrificing detail—saving tokens is the default, and you can still disable it per‑call if you really need the whole payload.
2. **Automatically up‑to‑date tools.** The server parses the live OpenAPI document on startup (with cache/refresh logic) so new endpoints and schema changes from intervals.icu appear instantly. No hand‑coding, no drift – just restart the binary and you have the latest API surface.

The generated toolset is organized into 9 practical categories:

- **Activities** (11 tools) - Query, search, update, delete, and download activities
- **Activity Analysis** (8 tools) - Deep dive into streams, intervals, best efforts, and histograms
- **Athlete** (2 tools) - Access profile, fitness metrics, and training load
- **Wellness** (3 tools) - Track and update recovery, HRV, sleep, and health metrics
- **Events/Calendar** (9 tools) - Manage planned workouts, races, notes with bulk operations
- **Performance/Curves** (3 tools) - Analyze power, heart rate, and pace curves
- **Workout Library** (5 tools) - Browse, create, update, and delete workout folders and training plans
- **Gear Management** (6 tools) - Track equipment and maintenance reminders
- **Sport Settings** (5 tools) - Configure FTP, FTHR, pace thresholds, and zones

Additionally, the server provides:
- **1 MCP Resource** - Athlete profile with fitness metrics for ongoing context

The companion crate `crates/intervals_icu_client` now includes spec-aligned coverage for weather configuration, athlete routes, wellness bulk updates, and the currently used MCP production paths (including calendar/event flows and sport-settings apply semantics), plus an ignored live OpenAPI smoke test that checks key client contracts against `https://intervals.icu/api/v1/docs`.

## Deterministic Coach Analytics Layer (v2.x)

Starting with version 2.x, the server includes a **deterministic coaching analytics layer** built into existing read-only intents. This layer provides data-driven insights, alerts, and guidance without relying on LLM computations.

### Architecture

The coach layer follows a strict pipeline:

```text
Fetch → Audit → Compute → Interpret → Render
```

1. **Fetch**: Gather data via `IntervalsClient` (activities, wellness, fitness, intervals, streams)
2. **Audit**: Check data quality and availability, detect missing data scenarios
3. **Compute**: Calculate derived metrics (volume, fitness, wellness, trends)
4. **Interpret**: Map metrics to states and alerts using deterministic rules
5. **Render**: Generate intent-specific sections with findings, suggestions, and next actions

### Enhanced Intents

Three primary intents consume the shared analytics engine:

#### `analyze_training`
- **Single workout mode** (`target_type: single`):
  - Workout summary with execution context
  - Read-only workout comments/messages rendered in a dedicated section when the source activity has them
  - Stream-aware execution metrics (`Efficiency Factor`, `Aerobic Decoupling`) when supported
  - Interval analysis and quality findings
  - Data availability reporting
- **Period mode** (`target_type: period`):
  - Period totals and weekly averages
  - `analysis_type: "summary"` keeps the response compact (totals + requested metrics + planned workouts)
  - omitted `analysis_type` or `"detailed"` includes trend context plus load-management context where `ACWR` prefers API-native acute/chronic load snapshots (`atlLoad`/`ctlLoad` or `icu_atl`/`icu_ctl`) and falls back to local EWMA only when those fields are unavailable
  - `analysis_type: "streams"` additionally renders a daily load series for the requested window
  - `analysis_type: "intervals"` additionally highlights interval-labeled sessions in the window
  - `Monotony` and weekly `Strain` remain deterministic derivatives from a 7-day daily load series when history is available
  - Load classification (low/balanced/high) and coaching guidance

#### `assess_recovery`
- Readiness summary with wellness snapshot
- Fitness state (fresh/neutral/fatigued)
- `Recovery Index` when HRV and resting HR are both available
- Red flags detection:
  - Deep fatigue (TSB < -20)
  - Low sleep (< 6.5h)
  - Elevated RHR (> 60 bpm)
  - HRV suppressed vs personal baseline
  - Low recovery index (< 0.60)
- Recovery-oriented guidance

#### `analyze_race`
- Race summary and execution pattern
- Stream-aware execution metrics when race streams are available
- Post-race load context
- Recovery guidance
- Degraded mode handling for missing data

### Internal Contract: `CoachContext`

All analytics flow through a shared `CoachContext` structure:

```rust
pub struct CoachContext {
    pub meta: CoachMeta,           // Analysis kind, window, timestamps
    pub audit: DataAudit,          // Data availability checks
    pub metrics: CoachMetrics,     // Volume, fitness, wellness, trends
    pub alerts: Vec<CoachAlert>,   // Detected issues with severity
    pub guidance: CoachGuidance,   // Findings, suggestions, next actions
}
```

### Key Metrics and Thresholds

**Fitness/Load:**
- TSB > 10 → Fresh
- -10 ≤ TSB ≤ 10 → Neutral
- TSB < -10 → Fatigued
- TSB < -20 → Deep fatigue alert

**Wellness:**
- Sleep ≥ 7.0h → Good, < 6.0h → Poor
- RHR ≤ 55 → Normal, > 60 → Elevated
- HRV is evaluated from the recent 7-day average relative to a personal 28-day baseline when enough history is available
- HRV status is rendered as `Within personal range`, `Below personal baseline`, or `Suppressed vs personal baseline`
- Recovery Index compares current HRV and resting HR to personal baseline when enough history exists; otherwise it falls back to the current HRV/resting-HR ratio
- Recovery Index < 0.60 → recovery-first alert

**Load management:**
- ACWR 0.8–1.3 → Productive
- ACWR > 1.5 → Overreaching alert
- Acute/chronic load values prefer Intervals.icu API snapshots (`atlLoad` / `ctlLoad` or `icu_atl` / `icu_ctl`) when available
- Monotony > 2.0 → repetitive-stress alert
- Weekly Strain remains a deterministic derived metric (`sum(daily_load_7d) × monotony`); it is not replaced by per-activity `strain_score`

**Execution metrics:**
- Efficiency Factor is rendered when heart-rate + output streams are present
- Aerobic Decoupling > 5% → watch signal

**Volume:**
- Weekly avg < 5h → Low volume
- Weekly avg > 15h → High volume (monitor recovery)

### Degraded Mode Handling

When data is unavailable, the system:
- Does NOT fabricate numbers
- Marks sections as unavailable
- Adds explicit reasons to `Data Availability` section
- Adjusts guidance based on missing data
- Suppresses optimistic readiness wording when wellness support is incomplete

### Current vNext Boundaries

- Implemented:
  - API-sourced or API-preferred: `CTL`, `ATL`, `TSB`, `ACWR` acute/chronic load snapshot
  - Deterministic derived: `Recovery Index`, `Monotony`, weekly `Strain`, `Efficiency Factor`, `Aerobic Decoupling`
- Deferred on purpose: polarisation metrics, because the current read-only pipeline does not yet have a documented deterministic mapping from Intervals.icu histogram/zone payloads into the required 3-zone model

### Why Deterministic Analytics?

- **Token efficiency**: LLM receives computed insights, not raw data
- **Consistency**: Same inputs always produce same outputs
- **Testability**: All rules are unit-tested
- **Explainability**: Every alert includes evidence
- **No LLM hallucinations**: Metrics computed server-side in Rust

### Why Rust?

This implementation offers several advantages over the Python reference implementation:
- **Performance** - Significantly faster execution and lower memory footprint
- **Reliability** - Memory safety and thread safety guaranteed by Rust
- **Single Binary** - No runtime dependencies or virtual environments
- **Docker Friendly** - Small container images (< 50MB) with distroless base
- **Production Ready** - Built-in observability (metrics, tracing, health checks)

## Prerequisites

- **Rust 1.93+** (with Cargo), OR
- **Docker**

## Intervals.icu API Key Setup

Before installation, you need to obtain your Intervals.icu API key:

1. Go to https://intervals.icu/settings
2. Scroll to the **Developer** section
3. Click **Create API Key**
4. Copy the API key (you'll use it during configuration)
5. Note your **Athlete ID** from your profile URL (format: `i123456`)

## Installation & Setup

### How Authentication Works

1. **API Key** - Simple API key authentication (no OAuth required)
2. **Environment Variables** - API key and athlete ID configured via `.env` file or environment
3. **Basic Auth** - HTTP Basic Auth with username "API_KEY" and your key as password
4. **Persistence** - Configuration reused across runs

### Option 1: Using Cargo (Rust)

```sh
# Clone the repository
git clone https://github.com/yourusername/rusty-intervals-mcp.git
cd rusty-intervals-mcp

# Build the project
cargo build --release

# Copy and edit the environment file
cp .env.example .env
# Edit .env and add your credentials:
# INTERVALS_ICU_API_KEY=your_api_key_here
# INTERVALS_ICU_ATHLETE_ID=i123456
```

### Option 1b: Install via `cargo install`

If you prefer to install the binary into your Cargo bin (typically `~/.cargo/bin`), use `cargo install --locked --path`:

```sh
# Install from the repository root
cargo install --locked --path crates/intervals_icu_mcp

# The binary will be installed to $CARGO_HOME/bin (default: ~/.cargo/bin).
# Make sure $CARGO_HOME/bin is in your PATH.

# After installation, run the server:
export INTERVALS_ICU_API_KEY=your_api_key_here
export INTERVALS_ICU_ATHLETE_ID=i123456
intervals_icu_mcp
```

> Note: if the package provides multiple binaries, select one with `--bin intervals_icu_mcp`.

### Option 2: Using Docker

```sh
# Build the image
docker build -t rusty-intervals-mcp:latest .

# Create environment file
cat > intervals-icu.env << EOF
INTERVALS_ICU_API_KEY=your_api_key_here
INTERVALS_ICU_ATHLETE_ID=i123456
EOF
```

## VSCode Integration

### Quick Setup for VSCode with Copilot

VSCode with GitHub Copilot supports MCP servers starting from version 1.x. Here's how to integrate rusty-intervals-mcp:

#### Step 1: Configure MCP Server

Create or edit your VSCode MCP configuration file:

**Location:**
- **macOS/Linux**: `~/.vscode/mcp-servers.json`
- **Windows**: `%USERPROFILE%\.vscode\mcp-servers.json`

**Using Cargo (Recommended for Development):**

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "cargo",
      "args": [
        "run",
        "--manifest-path",
        "/absolute/path/to/rusty-intervals-mcp/Cargo.toml",
        "-p",
        "intervals_icu_mcp",
        "--bin",
        "intervals_icu_mcp"
      ],
      "env": {
        "INTERVALS_ICU_API_KEY": "your_api_key_here",
        "INTERVALS_ICU_ATHLETE_ID": "i123456"
      }
    }
  }
}
```

**Using Compiled Binary (Recommended for Production):**

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "/absolute/path/to/rusty-intervals-mcp/target/release/intervals_icu_mcp",
      "args": [],
      "env": {
        "INTERVALS_ICU_API_KEY": "your_api_key_here",
        "INTERVALS_ICU_ATHLETE_ID": "i123456"
      }
    }
  }
}
```

**Using Docker:**

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "--env-file",
        "/absolute/path/to/intervals-icu.env",
        "rusty-intervals:latest"
      ]
    }
  }
}
```

#### Step 2: Restart VSCode

After configuring the MCP server, restart VSCode to load the new configuration. The intervals-icu MCP server will be available in Copilot chat.

#### Step 3: Verify Connection

Open VSCode Copilot Chat and try:

```
@intervals-icu Show me my activities from the last 7 days
```

If configured correctly, Copilot will use the MCP tools to fetch your data.

### Advanced VSCode Configuration

#### Using Environment File

Instead of hardcoding credentials in the config, reference an `.env` file:

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "bash",
      "args": [
        "-c",
        "source /absolute/path/to/rusty-intervals-mcp/.env && /absolute/path/to/rusty-intervals-mcp/target/release/intervals_icu_mcp"
      ]
    }
  }
}
```

#### Multiple Profiles

Configure multiple profiles for different athletes:

```json
{
  "mcpServers": {
    "intervals-icu-athlete1": {
      "command": "/path/to/intervals_icu_mcp",
      "env": {
        "INTERVALS_ICU_API_KEY": "key1",
        "INTERVALS_ICU_ATHLETE_ID": "i123456"
      }
    },
    "intervals-icu-athlete2": {
      "command": "/path/to/intervals_icu_mcp",
      "env": {
        "INTERVALS_ICU_API_KEY": "key2",
        "INTERVALS_ICU_ATHLETE_ID": "i789012"
      }
    }
  }
}
```

## Claude Desktop Configuration

Add to your Claude Desktop configuration file:

**Locations:**
- **macOS**: `~/Library/Application Support/Claude/claude_desktop_config.json`
- **Windows**: `%APPDATA%\Claude\claude_desktop_config.json`

### Using Cargo

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "cargo",
      "args": [
        "run",
        "--manifest-path",
        "/ABSOLUTE/PATH/TO/rusty-intervals-mcp/Cargo.toml",
        "-p",
        "intervals_icu_mcp",
        "--bin",
        "intervals_icu_mcp"
      ],
      "env": {
        "INTERVALS_ICU_API_KEY": "your_api_key_here",
        "INTERVALS_ICU_ATHLETE_ID": "i123456"
      }
    }
  }
}
```

### Using Compiled Binary

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "/ABSOLUTE/PATH/TO/rusty-intervals-mcp/target/release/intervals_icu_mcp",
      "env": {
        "INTERVALS_ICU_API_KEY": "your_api_key_here",
        "INTERVALS_ICU_ATHLETE_ID": "i123456"
      }
    }
  }
}
```

### Using Docker

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "--env-file",
        "/ABSOLUTE/PATH/TO/intervals-icu.env",
        "rusty-intervals:latest"
      ]
    }
  }
}
```

## Usage

Ask Claude or VSCode Copilot to interact with your Intervals.icu data using natural language. The server provides **8 high-level intents**, a resource, and guidance-driven responses.

### Quick Start with Intents (v2.0+)

Use built-in intent templates for common queries:

- `intervals_plan_training` - Create training plans across any horizon (1 week to annual)
- `intervals_analyze_training` - Analyze single workouts or periods with detailed metrics
- `intervals_modify_training` - Modify, move, create, or delete workouts
- `intervals_compare_periods` - Compare performance between periods (like-for-like)
- `intervals_assess_recovery` - Assess recovery status with red flag detection
- `intervals_manage_profile` - View/update profile, zones, and thresholds
- `intervals_manage_gear` - Track equipment mileage and wear
- `intervals_analyze_race` - Post-race analysis with strategy evaluation

**Example natural language queries:**
- "Create a 12-week 50K preparation plan" → `intervals_plan_training`
- "Analyze yesterday's workout" → `intervals_analyze_training`
- "Move Saturday's workout to Sunday" → `intervals_modify_training`
- "Compare this month vs last month" → `intervals_compare_periods`
- "How's my recovery?" → `intervals_assess_recovery`

### Activities

```
"Show me my activities from the last 30 days"
"Get details for my last long run"
"Find all my threshold workouts"
"Update the name of my last activity"
"Delete that duplicate activity"
"Download the FIT file for my race"
```

### Activity Analysis

```
"Show me the power data from yesterday's ride"
"What were my best efforts in my last race?"
"Find similar interval workouts to my last session"
"Show me the intervals from my workout on Tuesday"
"Get the power histogram for my last ride"
"Show me the heart rate distribution for that workout"
```

### Athlete Profile & Fitness

```
"Show my current fitness metrics and training load"
"Am I overtraining? Check my CTL, ATL, and TSB"
```

*Note: The athlete profile resource (`intervals-icu://athlete/profile`) automatically provides ongoing context.*

### Wellness & Recovery

```
"How's my recovery this week? Show HRV and sleep trends"
"What was my wellness data for yesterday?"
"Update my wellness data for today - I slept 8 hours and feel great"
```

### Calendar & Planning

```
"What workouts do I have planned this week?"
"Create a threshold workout for tomorrow"
"Update my workout on Friday"
"Delete the workout on Saturday"
"Duplicate this week's plan for next week"
"Create 5 workouts for my build phase"
```

### Performance Analysis

```
"What's my 20-minute power and FTP?"
"Show me my heart rate zones"
"Analyze my running pace curve"
```

### Workout Library

```
"Show me my workout library"
"What workouts are in my threshold folder?"
```

### Gear Management

```
"Show me my gear list"
"Add my new running shoes to gear tracking"
"Create a reminder to replace my bike chain at 3000km"
"Update the mileage on my road bike"
```

### Sport Settings

```
"Update my FTP to 275 watts"
"Show my current zone settings for cycling"
"Set my running threshold pace to 4:30 per kilometer"
"Apply my new threshold settings to historical activities"
```

## Intent-Driven Architecture (v2.0+)

This MCP server implements an **intent-driven architecture** that exposes 8 high-level business intents instead of 146 low-level API endpoints. This design reduces token consumption by 95% and simplifies LLM interactions.

### 8 High-Level Intents

| Intent | Purpose | Mutating |
|--------|---------|----------|
| [`plan_training`](#intent-plan_training) | Planning across horizons (microcycle → annual) | ✅ |
| [`analyze_training`](#intent-analyze_training) | Analysis (single workout or period) | ❌ |
| [`modify_training`](#intent-modify_training) | CRUD operations (modify, create, delete) | ✅ |
| [`compare_periods`](#intent-compare_periods) | Like-for-like performance comparison | ❌ |
| [`assess_recovery`](#intent-assess_recovery) | Recovery assessment + red flags | ❌ |
| [`manage_profile`](#intent-manage_profile) | Profile, zones, thresholds | ✅ |
| [`manage_gear`](#intent-manage_gear) | Equipment tracking | ✅ |
| [`analyze_race`](#intent-analyze_race) | Post-race analysis | ❌ |

**Key Benefits:**
- **95% token reduction**: 8 intents vs 146 tools (from ~14,000 to ~800 tokens of metadata)
- **Single-call workflows**: LLM calls one intent instead of orchestrating 6+ API calls
- **Business identifiers**: Use `date`, `description` instead of system IDs (`event_id`)
- **Guidance-driven responses**: Every response includes `suggestions` and `next_actions`
- **Schema-only wire output**: intent tools return canonical `structuredContent` without duplicating the same payload into text `content`
- **Idempotency**: All mutating operations support `idempotency_token` (TTL: 24h)

### Intent Output Contract

Intent tools publish an MCP `outputSchema` and return the canonical result in `structuredContent`.
For token efficiency, they do **not** mirror the same payload into free-form text blocks; successful intent tool calls therefore use an empty `content` array and a populated `structuredContent` object.

This keeps the wire contract deterministic, validator-friendly, and cheaper for LLM hosts that would otherwise receive the same data twice.

### Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│              LLM Host (Claude/Cursor/VSCode)            │
│  "Create a 12-week 50K preparation plan"               │
│  "Analyze yesterday's workout"                         │
│  "Move Saturday's workout to Sunday"                   │
└─────────────────────────────────────────────────────────┘
                        │
                        │ call_tool("intervals_plan_training", {...})
                        ▼
┌─────────────────────────────────────────────────────────┐
│              MCP Server: Intent Layer                   │
│  ┌─────────────────────────────────────────────────┐   │
│  │  8 High-Level Intents (exposed to LLM)          │   │
│  │  - plan_training, analyze_training, ...         │   │
│  └─────────────────────────────────────────────────┘   │
│                        │                                │
│                        ▼ Intent Router (Rust, NO LLM)   │
│  ┌─────────────────────────────────────────────────┐   │
│  │  IntentHandler:                                 │   │
│  │  - Validate & flatten input                     │   │
│  │  - Check idempotency cache (TTL: 24h)           │   │
│  │  - Orchestrate internal API calls               │   │
│  │  - Format response with guidance                │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
                        │
                        ▼ Internal API calls (hidden from LLM)
┌─────────────────────────────────────────────────────────┐
│         Dynamic OpenAPI Tools (146 endpoints)           │
│    ❌ NOT EXPOSED to LLM host (internal use only)       │
└─────────────────────────────────────────────────────────┘
```

### Intent Specifications

#### Intent: `plan_training`

**Purpose:** Training planning across arbitrary horizons (1 week to annual plan).

**Intent Patterns:**
- "12-week plan for 50K"
- "100K preparation in 4 months"
- "Plan from March 1 to April 15"
- "Base period plan, 6 weeks"

**Input Parameters (Flattened):**
```json
{
  "period_start": "2026-03-01",
  "period_end": "2026-06-15",
  "focus": "aerobic_base",
  "target_race": "50K Ultramarathon",
  "max_hours_per_week": 10,
  "adaptive": true,
  "idempotency_token": "plan-50k-2026-06-15-12weeks"
}
```

**Output:**
```markdown
## Plan: 50K Preparation (12 weeks)

**Period:** March 2 — May 25, 2026
**Focus:** Aerobic Base → Build → Specific + Taper
**Events created:** 84 workouts

### Structure
- Weeks 1-4: Base Period (aerobic base, 6-8 hrs/week)
- Weeks 5-8: Build Period (vertical + intensity, 8-10 hrs/week)
- Weeks 9-12: Specific Period + Taper (race-specific, 6-8 hrs/week)
```

**Guidance:**
```json
{
  "suggestions": [
    "Weeks 1-4: Base → focus on aerobic base, 95% Z1-Z2",
    "CTL growing at +8 TSS/week — good progression"
  ],
  "next_actions": [
    "After week 4: call assess_recovery to adapt Build phase",
    "To view week details: call analyze_training with period_start/period_end"
  ]
}
```

**Business Rules:**
- Volume progression: max +7-10% per week
- Recovery weeks: every 3-4 weeks (-40-60% volume)
- Taper: 7-10 days (50K), 10-14 days (100K), 14-21 days (100+ miles)
- Base Period: Z1-Z2 85-95%

---

#### Intent: `analyze_training`

**Purpose:** Workout analysis — single session or period summary.

**Current analytical behavior:** The intent uses a shared deterministic coaching layer. `single` mode can render execution context / quality findings / degraded-data notes, optional HR/power/pace histogram sections, and a read-only `Workout Comments` table populated from Intervals.icu activity messages when present. `period` mode adds trend context, shared load guidance, and now loads future and historical calendar events alongside activities so race, sick, injured, note, and planned workout items are available to the intent without folding them into the load metrics. When planned workouts are present, the response includes a `Planned Workouts in Window` table with date/name/duration/load, and non-workout calendar items appear in a separate `Calendar Events in Window` table. When the optional `metrics` array is supplied, the response now includes an explicit `Requested Metrics` table instead of silently ignoring unrenderable requests.

**Selector behavior:** For `target_type: period`, `analysis_type: "summary"` intentionally omits trend/load/data-availability sections, while omitted `analysis_type` (or `"detailed"`) keeps the full analytical context. `analysis_type: "streams"` adds a `Daily Load Series` section and `analysis_type: "intervals"` adds an `Interval Sessions` section.

**Intent Patterns:**
- "Analyze yesterday's workout"
- "Review intervals from tempo session"
- "February summary"
- "How did I execute Z3 this past month?"
- "What upcoming workouts do I have this week?"

**Input Parameters (Flattened):**

**Option A: Single workout**
```json
{
  "target_type": "single",
  "date": "2026-03-02",
  "description_contains": "long run",
  "analysis_type": "detailed",
  "metrics": ["time", "distance", "pace", "hr"],
  "include_histograms": true
}
```

**Option B: Period**
```json
{
  "target_type": "period",
  "period_start": "2026-02-01",
  "period_end": "2026-02-28",
  "analysis_type": "summary",
  "metrics": ["time", "distance", "vertical", "tss"]
}
```

**Metric / histogram semantics:**
- `metrics` is advisory-but-explicit: each requested metric is surfaced in a `Requested Metrics` table with `available`, `unavailable`, or `unsupported` status.
- Exact `tss` is only returned when the underlying activity payload exposes an exact TSS field; the server does **not** substitute generic training-load proxies.
- `include_histograms` is supported for `target_type: "single"` only. Period requests using it should be treated as invalid.
- Single-workout output may include a read-only `Workout Comments` section derived from `/api/v1/activity/{id}/messages`; deleted/empty messages are filtered out and the intent does not expose write operations for comments.

**Output (single):**
```markdown
## Analysis: Tuesday Tempo

| Metric | Value |
|--------|-------|
| Distance | 14.00 km |
| Duration | 1:00 |
| Avg HR | 145 bpm |
| Avg Power | 220 W |
| Elevation | 80 m |

### Requested Metrics

| Metric | Value | Status |
|--------|-------|--------|
| TIME | 1:00 | available |
| DISTANCE | 14.00 km | available |
| PACE | 4:17 /km | available |
| HR | 145 bpm | available |

### Workout Comments

| When | Author | Type | Comment |
|------|--------|------|---------|
| 2026-03-08 09:15:00 | Alice | TEXT | Felt controlled until the final climb. |
| 2026-03-08 10:00:00 | Coach Bob | TEXT | Nice restraint early on. |

### Pace Zone Distribution

| Zone | Time | % |
|------|------|---|
| Z1 | 10:00 | 29% |
| Z2 | 20:00 | 57% |
| Z3 | 5:00 | 14% |
```

**Output (period):**
```markdown
## Period Analysis: 2026-02-01 - 2026-02-28

| Metric | Value |
|--------|-------|
| Total time | 28:45 |
| Distance | 245 km |
| Elevation | 4,520 m |
| Weekly Avg | 7.2 hrs |

### Requested Metrics

| Metric | Value | Status |
|--------|-------|--------|
| TIME | 28:45 | available |
| DISTANCE | 245.0 km | available |
| VERTICAL | 4520 m | available |
| TSS | n/a | unavailable |

### Planned Workouts in Window

| Date | Workout | Duration | Planned Load |
|------|---------|----------|--------------|
| 2026-03-09 | Recovery Run Z1 | 0:45 | 30.0 |
| 2026-03-10 | Endurance Run Z2 — Pre-Trip | 1:45 | 82.0 |
```

**Multiple Activities Handling:**

When multiple activities are found for a date, the response includes **explicit retry examples** with ready-to-use parameter values:
```markdown
## Analysis: 2026-03-07

**Status:** Multiple activities found
**Found activities:**
1. Lunch Weight Training (ID: i130141297)
2. Long Run Z2 — Key Workout (ID: i130141300)

**To analyze a specific activity, retry with:**
1. **Lunch Weight Training** → `description_contains: "Lunch Weight Training"` or ID: `i130141297`
2. **Long Run Z2 — Key Workout** → `description_contains: "Long Run Z2"` or ID: `i130141300`

## Suggestions
- Choose one activity from the list and retry with its `description_contains` value
- For interval analysis, look for keywords like 'tempo', 'threshold', 'intervals', 'repeats', 'VO2'
- Note: Only workouts created with structured intervals will show interval data

## Next Actions
- Retry with `description_contains` from the list above (e.g., `description_contains: "Long Run Z2"`)
- Use `analyze_training` with `target_type: period` to see all activities
- Or specify activity ID directly if your MCP client supports it (e.g., `i130141300`)
```

This guidance-driven response helps LLM clients:
- **See exact parameter values** to retry with (no guessing)
- **Copy-paste ready examples** reduce hallucination
- **Explicit IDs** enable direct access if client supports it
- **No complex heuristics** — just clearer guidance

---

#### Intent: `modify_training`

**Purpose:** Adjust existing workouts (modify, move, create, delete).

**Current behavior:** `modify_training` resolves events by calendar date across both historical events and future calendar entries, matches `target_description_contains` against both event `name` and `description`, and in `dry_run` mode previews the exact field changes that would be sent to the API. This now includes race events and non-training calendar items such as sick days, injuries, notes, and plan markers. `create` accepts `target_date` as a practical alias for `new_date` when an LLM supplies the date in the target slot. For create flows, `new_category` controls the calendar event category (usually `Workout`) while `new_type` controls the workout or sport type for Workout events (for example `Run`, `Ride`, `Swim`, `WeightTraining`). If `new_category` is `Workout` and `new_type` is omitted, the server defaults it to `Run`. `dry_run` previews do not occupy idempotency cache entries, while non-`dry_run` mutations bind an `idempotency_token` to one exact request payload for safe retries.

**Batch behavior:** `modify` and `delete` accept either a single `target_date` or a date window via `target_date_from` + `target_date_to`. Batch delete now previews in `dry_run` mode and applies on a follow-up non-`dry_run` call instead of being preview-only forever.

**Intent Patterns:**
- "Move Saturday's workout to Sunday"
- "Change Tuesday's workout duration to 1 hour"
- "Delete all workouts this week"
- "Create a recovery workout for Wednesday"

**Input Parameters (Flattened):**
```json
{
  "action": "modify",
  "target_date": "2026-03-07",
  "target_description_contains": "long run",
  "new_date": "2026-03-08",
  "new_duration": null,
  "dry_run": false,
  "idempotency_token": "modify-2026-03-07-to-2026-03-08"
}
```

**Scenarios:**

| Scenario | Parameters |
|----------|-----------|
| Move workout | `action: "modify"`, `target_date: "2026-03-07"`, `new_date: "2026-03-08"` |
| Change duration | `action: "modify"`, `target_date: "2026-03-04"`, `new_duration: "1:00"` |
| Delete range | `action: "delete"`, `target_date_from: "2026-03-01"`, `target_date_to: "2026-03-07"` |
| Create | `action: "create"`, `new_date: "2026-03-10"` (or `target_date` alias), `new_name: "Tempo"`, `new_duration: "1:30"`, `new_category: "Workout"`, `new_type: "Run"` |
| Dry run | any action + `dry_run: true` — preview changes without applying |

**Create example (single-day workout insertion):**
```json
{
  "action": "create",
  "target_date": "2026-03-10",
  "new_name": "Tempo Run (Z3)",
  "new_duration": "1:00",
  "new_category": "Workout",
  "new_type": "Run",
  "new_description": "10m easy warm-up, 30m Z3, 10m cool-down",
  "dry_run": true,
  "idempotency_token": "create-2026-03-10-tempo-run"
}
```

**Category vs. type:**
- `new_category` is the calendar event category (`Workout`, `RaceA`, `Note`, `Plan`, ...)
- `new_type` is the workout or sport type used by Intervals.icu for `Workout` events (`Run`, `Ride`, `Swim`, `WeightTraining`, ...)
- If `new_category` is `Workout` and `new_type` is omitted, the server defaults it to `Run`

**Safety:** Destructive operations support an optional `dry_run: true` preview first; applying the same request without `dry_run` performs the delete.

**Idempotency note:** reuse the same `idempotency_token` only for an exact retry of the same non-`dry_run` mutation. Reusing a token for a different mutation payload now returns an explicit conflict instead of replaying a stale cached response.

**Recommended mutation flow for agents:**
1. Use `analyze_training` to inspect the target day if needed
2. Call `modify_training` with `dry_run: true`
3. Apply the exact same payload without `dry_run`
4. Reuse the same `idempotency_token` only for an exact retry of the same non-`dry_run` mutation

---

#### Intent: `compare_periods`

**Purpose:** Like-for-like performance comparison between periods.

**Current analytical behavior:** This intent now reuses the shared deterministic snapshot/trend helpers introduced for the coach layer, so period comparisons and `analyze_training(period)` rely on the same aggregation formulas instead of duplicating them. `workout_type` filters activities by workout-name keywords (for example `tempo`, `intervals`, `long_run`), and the optional `metrics` array renders a `Requested Metrics` table with concrete values for `volume`, `pace`, `hr`, and `tss`, plus explicit status notes for unsupported metrics.

**Intent Patterns:**
- "Compare my tempo workouts this month vs last month"
- "Threshold progression over the quarter"
- "This month vs last month"

**Input Parameters (Flattened):**
```json
{
  "period_a_start": "2026-02-01",
  "period_a_end": "2026-02-28",
  "period_a_label": "February",
  "period_b_start": "2026-01-01",
  "period_b_end": "2026-01-31",
  "period_b_label": "January",
  "workout_type": "tempo",
  "metrics": ["volume", "zones", "tss", "pace"]
}
```

**Output:**
```markdown
## Comparison: February vs January

| Metric | January | February | Δ |
|--------|---------|----------|---|
| Activities | 12 | 14 | +2 |
| Total Time | 24:30 | 28:45 | +4:15 |
| Distance (km) | 180.0 | 205.0 | +25.0 |
| Elevation (m) | 3200 | 3680 | +480 |

### Requested Metrics

| Metric | February | January | Status |
|--------|----------|---------|--------|
| TSS | 452.0 | 380.0 | sum of training load |
| Pace | 4:58 /km | 5:07 /km | derived |
```

---

#### Intent: `assess_recovery`

**Purpose:** Recovery assessment, readiness check, red flag detection.

**Current analytical behavior:** Recovery output is built from a shared readiness pipeline (`fetch → audit → metrics → alerts → guidance`) and explicitly reports degraded mode when wellness or fitness inputs are incomplete.

**Intent Patterns:**
- "How's my recovery?"
- "Am I ready for a key workout?"
- "Any red flags?"
- "Signs of overtraining?"

**Input Parameters (Flattened):**
```json
{
  "period_days": 7,
  "for_activity": "intensity",
  "include_wellness": true,
  "include_red_flags": true
}
```

**Red Flags (from athlete-monitoring skill):**
- Sleep <6.5h for 3+ nights
- Resting HR +3-5 bpm for 2+ weeks
- HRV materially below personal baseline (recent 7-day average vs 28-day baseline)
- HR drift >10%

**Output:**
```markdown
## Recovery Assessment (Feb 24 — Mar 2, 2026)

| Metric | Value | Status |
|--------|-------|--------|
| Sleep (avg) | 7.5 hrs/night | ✅ Normal |
| Resting HR | 52 bpm | ✅ Baseline |
| HRV (avg) | 65 ms | ✅ Within personal range |
| TSB | +15 | ✅ Fresh |

### Readiness for intensity: ✅ Yes

### Red Flags
| Flag | Status | Details |
|------|--------|---------|
| Sleep <6.5h (3+ nights) | ✅ Normal | Avg: 7.5 hrs |
| Resting HR +3-5 bpm (2+ weeks) | ✅ Normal | Baseline: 52, current: 53 |
| HRV vs personal baseline | ✅ Within range | Baseline: 64 ms, recent: 65 ms (+2%) |
| HR drift >10% | ✅ Normal | Max drift: 6% |

**Red flags:** None detected
**Recommendation:** Ready for key workout
```

---

#### Intent: `manage_profile`

**Purpose:** Athlete profile, zones, thresholds, and fitness snapshot management. Includes threshold updates from lab tests.

**Intent Patterns:**
- "Show my profile"
- "Update thresholds from VO2 max test"
- "Sync zones with lab test"
- "What are my zones?"

**Input Parameters (Flattened):**

**Option A: View**
```json
{
  "action": "get",
  "sections": ["overview", "zones", "thresholds", "metrics"]
}
```

**Option B: Update thresholds**
```json
{
  "action": "update_thresholds",
  "new_aet_hr": 155,
  "new_lt_hr": 175,
  "thresholds_source": "lab_test",
  "apply_to_activities": true,
  "idempotency_token": "profile-thresholds-2026-02-15"
}
```

**Output (get):**
```markdown
## Athlete Profile

### Zones (Run)
| Zone | HR Range |
|------|----------|
| Z1 | ≤ 144 bpm |
| Z2 | 145-160 bpm |
| Z3 | 161-167 bpm |
| Z4 | 168-173 bpm |
| Z5 | 174-180 bpm |

### Thresholds (Run)
| Parameter | Value |
|-----------|-------|
| LTHR | 171 bpm |
| Max HR | 180 bpm |
| FTP | 315 W |
| Threshold Pace | 3:42 /km |
| Load Order | HR_PACE_POWER |

### Metrics
| Metric | Value |
|--------|-------|
| CTL (Fitness) | 61.0 |
| ATL (Fatigue) | 47.0 |
| TSB (Form) | +14.0 |
| Load State | fresh |
```

**Notes:**
- `zones` and `thresholds` are derived from the live `/sport-settings` response, which is currently array-shaped in the real Intervals.icu API.
- `metrics` renders the latest fitness snapshot (`CTL`/`ATL`/`TSB`) when available.
- `update_thresholds` now calls the live sport-settings update API and, when `apply_to_activities: true`, also triggers the upstream historical recalculation endpoint.

**AeT-LT Gap → Focus:**

| AeT-LT Gap | Recommendation |
|------------|----------------|
| >20% | 100% Z1-Z2, minimum 12 weeks |
| 10-20% | 95% Z1-Z2 + strides/hill sprints |
| <10% | Can add Z3 after 6-8 weeks base |

---

#### Intent: `manage_gear`

**Purpose:** Equipment management (view, add, retire).

**Current behavior:** `list` reads the live gear inventory, `add` calls the live gear-creation endpoint, and `retire` updates the selected gear with a `retired` date through the live update endpoint.

**Intent Patterns:**
- "Show my shoes"
- "How much mileage on Nike Pegasus?"
- "Add new shoes"
- "Retire old Pegasus"

**Input Parameters (Flattened):**
```json
{
  "action": "list",
  "gear_type": "shoes",
  "gear_name": null
}
```

**Output:**
```markdown
## Gear: Running Shoes

| Name | Mileage | Remaining | Status |
|------|---------|-----------|--------|
| Nike Pegasus 40 | 850 km | 150 km | 🔶 85% worn |
| Hoka Clifton 9 | 420 km | 580 km | ✅ Normal |
```

Mutating examples:

```json
{
  "action": "add",
  "new_gear_name": "Nike Pegasus 41",
  "new_gear_type": "shoes",
  "idempotency_token": "gear-add-pegasus-41"
}
```

```json
{
  "action": "retire",
  "gear_name": "Nike Pegasus 40",
  "idempotency_token": "gear-retire-pegasus-40"
}
```

**Guidance:**
```json
{
  "suggestions": [
    "Nike Pegasus 40: ~150 km remaining — plan replacement"
  ],
  "next_actions": [
    "Retire Pegasus? (manage_gear action: retire, gear_name: 'Nike Pegasus 40')",
    "Add new pair? (manage_gear action: add)"
  ]
}
```

---

#### Intent: `analyze_race`

**Purpose:** Post-race analysis: results, strategy, comparison to plan.

**Current analytical behavior:** Race analysis now shares the same deterministic load/recovery layer used by other read-only intents and can add execution-pattern, post-race load, and recovery-forward guidance sections when data is available. `date: "last_race"` selects the latest race-like activity (instead of just the latest activity), `analysis_type: "strategy"` adds a strategy review section, `analysis_type: "recovery"` pivots the post-race section into a recovery outlook, and `compare_to_planned: true` adds a `Comparison to Plan` section when a matching calendar event is found.

**Intent Patterns:**
- "Analyze my last race"
- "50K race strategy analysis"
- "How did I perform compared to the plan?"

**Input Parameters (Flattened):**
```json
{
  "date": "last_race",
  "description_contains": "50K",
  "analysis_type": "strategy",
  "compare_to_planned": true
}
```

**Output:**
```markdown
## Race Analysis

### Mountain Trail 50K Race

| Metric | Value |
|--------|-------|
| Distance | 50.00 km |
| Time | 5:42:30 |
| Avg HR | 158 bpm |

### Comparison to Plan
- Planned event: Mountain Trail 50K
- Planned date: 2026-03-01
- Actual race: Mountain Trail 50K Race

### Strategy Review
- Focus on pacing discipline, fueling timing, and how effort changed across segments.

### Execution Pattern
- Detected 4 race segments/interval blocks.
- Efficiency Factor: 1.12
```

---

### Token Efficiency

This MCP server is optimized for minimal token consumption while maintaining quality:

#### Compact Tool Responses

Several tools support **compact mode** to reduce token usage:

#### `get_activity_streams`
| Parameter | Type | Description |
|-----------|------|-------------|
| `activity_id` | string | Activity ID (required) |
| `max_points` | integer | Downsample arrays to this size (e.g., 100) |
| `summary` | boolean | Return stats (min/max/avg/p10/p50/p90) instead of arrays |
| `streams` | array | Filter to specific streams (e.g., `["power", "heartrate"]`) |

**Example - Summary mode (saves ~95% tokens for long activities):**
```json
{"activity_id": "i123", "summary": true, "streams": ["power"]}
```

#### `get_activity_details`
| Parameter | Type | Description |
|-----------|------|-------------|
| `activity_id` | string | Activity ID (required) |
| `expand` | boolean | Return full payload (default: false = compact summary) |
| `fields` | array | Specific fields to return (e.g., `["id", "distance", "moving_time"]`) |

**Example - Compact mode (default, saves ~70% tokens):**
```json
{"activity_id": "i123"}
```

**Example - Full mode when needed:**
```json
{"activity_id": "i123", "expand": true}
```

#### Best Practices for Token Efficiency

1. **Start with compact** - Use default (compact) responses first, expand only when needed
2. **Filter streams** - Request only the streams you need (e.g., just `power` and `heartrate`)
3. **Use summary for analysis** - For trend analysis, use `summary: true` to get statistics without raw data
4. **Limit recent activities** - Use `limit` parameter to control result count
5. **Use specific fields** - Request only the fields you need with `fields` parameter
6. **Use intents** - Prefer 8 high-level intents over 146 low-level tools (95% token reduction)

## Available Tools

Tool availability is generated dynamically from OpenAPI and can change with spec updates.

Common tag groups in the OpenAPI spec include:

| Tag (OpenAPI section) | Typical domain |
|---|---|
| `Activities` | Activities, streams, intervals, curves/histograms |
| `Wellness` | Recovery and wellness metrics |
| `Events` | Calendar, workouts, planned events |
| `Gear` | Gear inventory and reminders |
| `Sports` | Sport settings and thresholds |
| `Athletes` | Athlete profile and related athlete endpoints |
| `Library` | Workout library, folders, plans |

Note: Intervals.icu event IDs are numeric; MCP event tools accept number/string and normalize automatically.

### API Compliance Notes

- Activity search calls `/activities/search` and `/activities/search-full`.
- Interval search requires `minSecs`, `maxSecs`, `minIntensity`, and `maxIntensity`; optional `type`, `minReps`, and `maxReps` are supported.
- Bulk event deletion uses `PUT /events/bulk-delete`; event duplication uses `/duplicate-events` with `numCopies` and `weeksBetween`.
- Gear reminders use `/gear/{gearId}/reminder` endpoints; updates require `reset` and `snoozeDays` query parameters.
- Sport settings updates require `recalcHrZones`; apply is a `PUT` with no start date parameter.
- Workout library endpoints use `/folders` and `/workouts` (folder filtering is applied client-side).

## MCP Resources

Resources provide ongoing context to the LLM without requiring explicit tool calls:

| Resource | Description |
|----------|-------------|
| `intervals-icu://athlete/profile` | Complete athlete profile with current fitness metrics and sport settings |

| Prompt | Description | Replacement Intent |
|--------|-------------|-------------------|
| `analyze-recent-training` | Comprehensive training analysis over a specified period | `intervals_analyze_training` |
| `performance-analysis` | Detailed power/HR/pace curve analysis with zones | `intervals_compare_periods` |
| `activity-deep-dive` | Deep dive into a specific activity with streams, intervals, best efforts | `intervals_analyze_training` (single) |
| `recovery-check` | Recovery assessment with wellness trends and training load | `intervals_assess_recovery` |
| `training-plan-review` | Weekly training plan evaluation with workout library | `intervals_plan_training` |
| `plan-training-week` | AI-assisted weekly training plan creation based on current fitness | `intervals_plan_training` |
| `analyze-and-adapt-plan` | Compare recent training vs. plan and propose adaptive adjustments | `intervals_analyze_training` + `intervals_plan_training` |

**Migration:** Update your workflows to use the new intent-driven API for better token efficiency (95% reduction) and single-call workflows.

## Development

### Environment

See `.env.example` for example environment variables. Set `RUST_LOG` (info, warn, debug, trace) to control logging.

### OpenAPI runtime configuration

The dynamic registry is built from OpenAPI and cached in memory.

| Variable | Default | Description |
|---|---|---|
| `INTERVALS_ICU_OPENAPI_SPEC` | _unset_ | Optional authoritative OpenAPI source (HTTP(S) URL or local JSON file path). If unset, runtime fetches `${INTERVALS_ICU_BASE_URL}/api/v1/docs` and falls back to `docs/intervals_icu_api.json`. If set explicitly, runtime uses that source as-is and surfaces failures instead of silently switching to another spec. |
| `INTERVALS_ICU_SPEC_REFRESH_SECS` | `300` | Refresh cadence for cache revalidation. On cache hit, refresh is attempted on interval boundaries. |

Behavior summary:
- Initial tool request loads OpenAPI and caches generated tools.
- Cache hits return immediately and attempt periodic refresh.
- If refresh fails (network or parse error), cached registry remains active.
- Athlete-scoped operations auto-inject the configured athlete context for current-spec placeholders such as `athlete_id`, `athleteId`, and legacy literal `/athlete/{id}` routes, while preserving nested resource identifiers like `/sport-settings/{id}` as user-supplied parameters.

### Continuous Integration & Releases ✅

This project uses GitHub Actions to run checks and to produce release artifacts
(binaries and checksums) for Linux (x86_64 & aarch64), macOS (x86_64 & aarch64),
and Windows (x86_64). Release artifacts are attached to GitHub releases.

- PRs run formatting checks, clippy, and the test suite.
- Tagging a release (e.g. `v1.2.3`) will trigger the release build which
  compiles and publishes binaries for supported platforms.

See `.github/workflows/ci.yml` for workflow details and `CONTRIBUTING.md` for how to run checks locally.

### Running Examples

```sh
# Basic profile fetch
cargo run -p intervals_icu_client --example basic_client

# List recent activities
cargo run -p intervals_icu_client --example list_recent_activities

# Download activity file
cargo run -p intervals_icu_client --example download_activity_file -- <activity_id>
```

### Running Tests

```sh
# Run all tests
cargo test

# Run library tests only
cargo test --lib

# Run integration tests
cargo test -p intervals_icu_client --tests

# Run the live OpenAPI client smoke test (ignored by default; requires network)
cargo test -p intervals_icu_client live_openapi_spec_contract_smoke -- --ignored --nocapture

# Run E2E tests
cargo test -p intervals_icu_mcp --test e2e_http
```

### Running HTTP Server

The HTTP server exposes MCP at `/mcp` along with observability endpoints:

```sh
export INTERVALS_ICU_API_KEY=your_key
export INTERVALS_ICU_ATHLETE_ID=i123456
cargo run -p intervals_icu_mcp --bin server
```

**Endpoints:**
- `/mcp` - MCP server endpoint
- `/health` - Health check
- `/metrics` - Prometheus metrics
- `/athlete/profile` - Direct athlete profile endpoint

### Code Quality

```sh
# Check formatting without rewriting files
cargo fmt --all -- --check

# Lint all targets/features and fail on warnings
cargo clippy --all-targets --all-features -- -D warnings

# Run the full workspace test suite
cargo test --all-targets --all-features

# Run benchmarks
cargo bench -p intervals_icu_client
```

## Docker

### Building

```sh
docker build -t rusty-intervals:latest .
```

### Running

```sh
# Using environment file
docker run -i --rm --env-file intervals-icu.env rusty-intervals:latest

# Using environment variables
docker run -i --rm \
  -e INTERVALS_ICU_API_KEY=your_key \
  -e INTERVALS_ICU_ATHLETE_ID=i123456 \
  rusty-intervals:latest
```

### Run the MCP server container (example)

The production image provides the `intervals_icu_mcp` server binary and exposes the HTTP endpoints. Run the container and map port 8080 to access the server endpoints (e.g., `/mcp`, `/health`, `/metrics`):

```sh
# Run locally and expose port 8080
docker run -i --rm \
  -p 8080:8080 \
  -e INTERVALS_ICU_API_KEY=your_key \
  -e INTERVALS_ICU_ATHLETE_ID=i123456 \
  ghcr.io/<your-org-or-user>/rusty-intervals:latest
```

Replace `<your-org-or-user>` with the GitHub organization or username that owns the repository.

### Using docker-compose

You can use the included `docker-compose.yml` to build and run the service locally (recommended for testing and quick iterations). The compose file builds from the repository `Dockerfile` and tags the local image as `intervals_icu_mcp:local`.

- Build and start (foreground):

```sh
docker compose up --build
```

- Build and start (detached):

```sh
docker compose up -d --build
```

- Tail logs:

```sh
docker compose logs -f intervals_icu_mcp
```

- Check health:

```sh
curl http://localhost:8080/health
# or
docker compose ps
```

Environment variables may be provided via a top-level `.env` file (Docker Compose reads it automatically) or passed at runtime. The compose file sets common variables including `INTERVALS_ICU_API_KEY`, `INTERVALS_ICU_ATHLETE_ID`, `INTERVALS_ICU_BASE_URL`, `RUST_LOG` and `ADDRESS` (default `0.0.0.0:8080`).

### Remote MCP server deployment

Deploy the `docker-compose.yml` on your production host and run the service:

1. On the remote host:

```sh
# clone, configure .env with credentials, then start in detached mode
git clone https://github.com/yourusername/rusty-intervals.git
cd rusty-intervals
# create a .env with INTERVALS_ICU_API_KEY and INTERVALS_ICU_ATHLETE_ID set
docker compose up -d --build
```

2. Configure your MCP-capable client using the example `examples/mcp_remote.json` (point `url` to your production domain, e.g. `https://mcp.example.com/mcp`).

Security notes:
- For production, run the service behind a TLS-terminating reverse proxy (Caddy/Traefik/Nginx) and an authentication layer (OIDC/JWT, mTLS, or similar). Do NOT expose port 8080 directly to the public internet without TLS and access controls.

### Example `mcp.json` for remote MCP clients

If your MCP client supports an HTTP MCP endpoint, configure it to point at the remote service (example file at `examples/mcp_remote.json`). Example:

```json
{
  "mcpServers": {
    "intervals-icu-remote": {
      "name": "intervals-icu",
      "url": "https://mcp.example.com/mcp",
      "description": "Remote Intervals.icu MCP server",
      "tls": { "insecureSkipVerify": false }
    }
  }
}
```

Notes:
- Replace `https://mcp.example.com` with your production domain. The path `/mcp` is the HTTP MCP endpoint implemented by the server.
- The `headers` block is optional and useful when using a reverse-proxy-based auth layer (JWT, static bearer token, etc.).
- Ensure the reverse proxy validates credentials (or mTLS) and terminates TLS before forwarding to the service.

If you'd like, I can add a `docker-compose.override.yml` example with Caddy (automatic TLS) and an example minimal JWT/basic-auth proxy to demonstrate a secure production layout.

If you want, I can add a small `docker-compose.override.yml` for remote production (reverse proxy, TLS, and systemd unit) or an example GitHub Actions workflow to deploy to a server via SSH.


### Published images

We publish multi-architecture Docker images to GitHub Container Registry (GHCR) on releases. The image repository is:

- ghcr.io/<your-org-or-user>/rusty-intervals


To pull the image from GHCR:

```sh
docker pull ghcr.io/<your-org-or-user>/rusty-intervals:latest
```

## Troubleshooting

### VSCode MCP Server Not Connecting

1. **Check Logs**: Look at VSCode Output panel → "MCP Servers" channel
2. **Verify Path**: Ensure all absolute paths in config are correct
3. **Test Standalone**: Run the MCP server directly to verify it works:
   ```sh
   cargo run -p intervals_icu_mcp --bin intervals_icu_mcp
   ```
4. **Check Credentials**: Verify `INTERVALS_ICU_API_KEY` and `INTERVALS_ICU_ATHLETE_ID` are set correctly. Note: the HTTP server will perform a startup check and refuse to start (exit status 1) if these variables are missing; ensure they are present in your `.env` or environment when running the service.

### VSCode Logs Show `Discovered 0 tools`

If the MCP process starts but the host reports `Discovered 0 tools`, the most common cause is an outdated binary that does not advertise MCP tool capability during `initialize`.

- Rebuild/reinstall the stdio binary you actually run from the MCP host configuration.
- If you use a local install, refresh it with `cargo install --locked --path crates/intervals_icu_mcp --force`.
- If you use `cargo run`, restart the host after rebuilding so it performs a fresh MCP handshake.

Current releases advertise both tool and resource capabilities during `initialize`, and the stdio end-to-end test suite now verifies that handshake explicitly.

### Authentication Errors

- Verify API key is valid at https://intervals.icu/settings
- Check athlete ID matches your profile (format: `i123456`)
- Ensure no extra whitespace in credentials

### Performance Issues

- Use compiled binary instead of `cargo run` for production
- Consider using Docker with resource limits
- Check network connectivity to intervals.icu API

## Architecture

This project is organized as a Cargo workspace:

- `crates/intervals_icu_client` - Core HTTP client library with retry logic and observability
- `crates/intervals_icu_mcp` - MCP server implementation with RMCP integration

## License

MIT License - see [LICENSE](LICENSE) file for details

## Disclaimer

This project is not affiliated with, endorsed by, or sponsored by Intervals.icu. All product names, logos, and brands are property of their respective owners.

