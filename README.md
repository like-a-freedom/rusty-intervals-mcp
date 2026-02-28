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

This MCP server provides **57 tools** to interact with your Intervals.icu account, organized into 9 categories:

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
- **7 MCP Prompts** - Templates for common queries (training analysis, performance analysis, activity deep dive, recovery check, training plan review, weekly planning, adaptive plan)

### Why Rust?

This implementation offers several advantages over the Python reference implementation:
- **Performance** - Significantly faster execution and lower memory footprint
- **Reliability** - Memory safety and thread safety guaranteed by Rust
- **Single Binary** - No runtime dependencies or virtual environments
- **Docker Friendly** - Small container images (< 50MB) with distroless base
- **Production Ready** - Built-in observability (metrics, tracing, health checks)

## Prerequisites

- **Rust 1.92+** (with Cargo), OR
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

### Option 1b: Install via `cargo install`

If you prefer to install the binary into your Cargo bin (typically `~/.cargo/bin`), use `cargo install --path`:

```sh
# Install from the repository root
cargo install --path crates/intervals_icu_mcp

# The binary will be installed to $CARGO_HOME/bin (default: ~/.cargo/bin).
# Make sure $CARGO_HOME/bin is in your PATH.

# After installation, run the server:
export INTERVALS_ICU_API_KEY=your_api_key_here
export INTERVALS_ICU_ATHLETE_ID=i123456
intervals_icu_mcp
```

> Note: if the package provides multiple binaries, select one with `--bin intervals_icu_mcp`.
```

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

Ask Claude or VSCode Copilot to interact with your Intervals.icu data using natural language. The server provides tools, a resource, and prompt templates to help you get started.

### Quick Start with MCP Prompts

Use built-in prompt templates for common queries:

- `analyze-recent-training` - Comprehensive training analysis over a specified period
- `performance-analysis` - Analyze power/HR/pace curves and zones
- `activity-deep-dive` - Deep dive into a specific activity with streams, intervals, and best efforts
- `recovery-check` - Recovery assessment with wellness trends and training load
- `training-plan-review` - Weekly training plan evaluation with workout library
- `plan-training-week` - AI-assisted weekly training plan creation based on current fitness
- `analyze-and-adapt-plan` - Compare recent training vs. plan and propose adaptive adjustments

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

## Token Efficiency

This MCP server is optimized for minimal token consumption while maintaining quality:

### Compact Tool Responses

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

### Best Practices for Token Efficiency

1. **Start with compact** - Use default (compact) responses first, expand only when needed
2. **Filter streams** - Request only the streams you need (e.g., just `power` and `heartrate`)
3. **Use summary for analysis** - For trend analysis, use `summary: true` to get statistics without raw data
4. **Limit recent activities** - Use `limit` parameter to control result count
5. **Use specific fields** - Request only the fields you need with `fields` parameter

## Available Tools

### Activities (10 tools)

| Tool | Description |
|------|-------------|
| `get_recent_activities` | List recent activities with summary metrics |
| `get_activity_details` | Get activity details (compact by default, use `expand=true` for full) |
| `search_activities` | Search activities by name or tag |
| `search_activities_full` | Search activities with full details |
| `get_activities_csv` | Download activities as CSV |
| `get_activities_around` | Get activities before and after a specific one |
| `update_activity` | Update activity name, description, or metadata |
| `delete_activity` | Delete an activity |
| `download_activity_file` | Download original activity file |
| `download_fit_file` | Download activity as FIT file |
| `download_gpx_file` | Download activity as GPX file |

### Activity Analysis (8 tools)

| Tool | Description |
|------|-------------|
| `get_activity_streams` | Get time-series data with compact options (`max_points`, `summary`, `streams`) |
| `get_activity_intervals` | Get structured workout intervals with targets and performance |
| `get_best_efforts` | Find peak performances across all durations in an activity |
| `search_intervals` | Find similar intervals across activity history |
| `get_power_histogram` | Get power distribution histogram for an activity |
| `get_hr_histogram` | Get heart rate distribution histogram for an activity |
| `get_pace_histogram` | Get pace distribution histogram for an activity |
| `get_gap_histogram` | Get grade-adjusted pace histogram for an activity |

### Athlete (2 tools)

| Tool | Description |
|------|-------------|
| `get_athlete_profile` | Get athlete profile with fitness metrics and sport settings |
| `get_fitness_summary` | Get detailed CTL/ATL/TSB analysis with training recommendations |

### Wellness (3 tools)

| Tool | Description |
|------|-------------|
| `get_wellness` | Get recent wellness metrics with trends (HRV, sleep, mood, fatigue) |
| `get_wellness_for_date` | Get complete wellness data for a specific date |
| `update_wellness` | Update or create wellness data for a date |

### Events/Calendar (9 tools)

Note: Intervals.icu event IDs are numeric; the MCP tools accept either numbers or strings and forward them as required by the API.

| Tool | Description |
|------|-------------|
| `get_events` | Get planned events and workouts from calendar |
| `get_upcoming_workouts` | Get upcoming workouts (defaults to `WORKOUT`, supports `category=all`, and applies `limit` in compact/full modes) |
| `get_event` | Get details for a specific event |
| `create_event` | Create new calendar events (workouts, races, notes, goals) |
| `update_event` | Modify existing calendar events |
| `delete_event` | Remove events from calendar |
| `bulk_create_events` | Create multiple events in a single operation |
| `bulk_delete_events` | Delete multiple events in a single operation |
| `duplicate_event` | Duplicate an event to a new date |

### Performance/Curves (3 tools)

| Tool | Description |
|------|-------------|
| `get_power_curves` | Analyze power curves with FTP estimation and power zones |
| `get_hr_curves` | Analyze heart rate curves with HR zones |
| `get_pace_curves` | Analyze running/swimming pace curves with optional GAP |

### Workout Library (5 tools)

| Tool | Description |
|------|-------------|
| `get_workout_library` | Browse workout folders and training plans |
| `get_workouts_in_folder` | View all workouts in a specific folder |
| `create_folder` | Create new folder (training plan) |
| `update_folder` | Update existing folder details |
| `delete_folder` | Delete a folder (training plan) |

### Gear Management (6 tools)

| Tool | Description |
|------|-------------|
| `get_gear_list` | Get all gear items with usage and status |
| `create_gear` | Add new gear to tracking |
| `update_gear` | Update gear details, mileage, or status |
| `delete_gear` | Remove gear from tracking |
| `create_gear_reminder` | Create maintenance reminders for gear |
| `update_gear_reminder` | Update existing gear maintenance reminders |

### Sport Settings (5 tools)

| Tool | Description |
|------|-------------|
| `get_sport_settings` | Get sport-specific settings and thresholds |
| `update_sport_settings` | Update FTP, FTHR, pace threshold, or zone configuration |
| `apply_sport_settings` | Apply updated settings to historical activities |
| `create_sport_settings` | Create new sport-specific settings |
| `delete_sport_settings` | Delete sport-specific settings |

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

## MCP Prompts

Prompt templates for common queries (accessible via prompt suggestions in Claude/VSCode):

| Prompt | Description |
|--------|-------------|
| `analyze-recent-training` | Comprehensive training analysis over a specified period |
| `performance-analysis` | Detailed power/HR/pace curve analysis with zones |
| `activity-deep-dive` | Deep dive into a specific activity with streams, intervals, best efforts |
| `recovery-check` | Recovery assessment with wellness trends and training load |
| `training-plan-review` | Weekly training plan evaluation with workout library |
| `plan-training-week` | AI-assisted weekly training plan creation based on current fitness |
| `analyze-and-adapt-plan` | Compare recent training vs. plan and propose adaptive adjustments |

## Development

### Environment

See `.env.example` for example environment variables. Set `RUST_LOG` (info, warn, debug, trace) to control logging.

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
# Format code
cargo fmt

# Lint code
cargo clippy --all -- -D warnings

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

