# Intent-Driven, Token-Efficient Intervals.icu MCP Server

A high-performance **Rust MCP server for Intervals.icu** designed around one idea: an LLM should interact with a **small, semantically rich coaching interface**, not a raw pile of endpoint wrappers.

[![.github/workflows/ci.yml](https://github.com/like-a-freedom/rusty-intervals/actions/workflows/ci.yml/badge.svg)](https://github.com/like-a-freedom/rusty-intervals/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/like-a-freedom/rusty-intervals?label=release)](https://github.com/like-a-freedom/rusty-intervals/releases)
[![codecov](https://codecov.io/gh/like-a-freedom/rusty-intervals-mcp/graph/badge.svg?token=I47UV16VY5)](https://codecov.io/gh/like-a-freedom/rusty-intervals-mcp)
![Rust](https://img.shields.io/badge/rust-1.92+-orange.svg)
![License](https://img.shields.io/badge/license-MIT-blue.svg)

> **Public contract:** 8 high-level intents + 1 resource  
> **Internal execution layer:** dynamic OpenAPI runtime that stays aligned with Intervals.icu  
> **Design goal:** respect the agent's context window and return decision-ready coaching context

## Table of Contents

- [Why this project exists](#why-this-project-exists)
- [What makes it different](#what-makes-it-different)
- [Public MCP surface](#public-mcp-surface)
- [Architecture at a glance](#architecture-at-a-glance)
- [Why it is token-efficient](#why-it-is-token-efficient)
- [Quick start](#quick-start)
- [VS Code / Copilot setup](#vs-code--copilot-setup)
- [Claude Desktop setup](#claude-desktop-setup)
- [Example asks](#example-asks)
- [Deterministic coaching analytics](#deterministic-coaching-analytics)
- [Runtime configuration](#runtime-configuration)
- [Development](#development)
- [Docker and remote deployment](#docker-and-remote-deployment)
- [Documentation map](#documentation-map)
- [License](#license)
- [Disclaimer](#disclaimer)

## Why this project exists

The original foundation of this project was strong: build tool behavior dynamically from the live Intervals.icu OpenAPI spec so the MCP server does not drift as the upstream API evolves.

That solved the **maintenance** problem.

It did **not** solve the **agent UX** problem.

Exposing one tool per API endpoint creates the exact failure mode modern MCP design tries to avoid:

- too many tools loaded into context
- too much low-level API detail exposed to the model
- too many multi-step orchestration burdens pushed onto the LLM
- more chances for bad tool selection, invalid arguments, and wasted tokens

This project now takes a different approach:

- keep the **dynamic OpenAPI layer** internally, where it belongs
- expose a **capability-level intent surface** to the LLM
- return **structured, guidance-driven outputs** instead of raw payloads
- compute important coaching metrics on the server, not in the model's head

In other words: **dynamic under the hood, curated at the boundary**.

## What makes it different

### 1. Intent-driven public interface

The LLM sees **8 high-level intents** such as `analyze_training` or `modify_training`, not dozens of endpoint-shaped tools.

### 2. Dynamic OpenAPI runtime retained internally

This is not a hand-maintained wrapper that goes stale. The server still loads the Intervals.icu OpenAPI spec dynamically and uses it as the execution layer behind the intent orchestration.

### 3. Token-efficiency by default

Responses are designed for LLMs:

- structured and compact
- pre-filtered
- pre-aggregated
- guidance-rich

### 4. Deterministic coaching analytics

Read-only coaching intents use a deterministic pipeline to compute metrics such as readiness, ACWR context, monotony, strain, recovery interpretation, and stream-derived execution signals.

### 5. Safer mutation flows

Mutating intents are designed for agents:

- business identifiers instead of opaque system-first flows
- `dry_run` previews for risky changes
- `idempotency_token` support for safe retries

### 6. Rust-first operational profile

- single binary
- fast startup
- strong type safety
- good fit for local MCP, containers, and remote HTTP deployments

## Public MCP surface

The public MCP contract is intentionally small and stable.

### Intents

| Intent | Purpose | Mutating | Example ask |
|---|---|---:|---|
| `plan_training` | Create training plans across any horizon | ✅ | “Build me a 12-week 50K plan” |
| `analyze_training` | Analyze a single workout or a training period | ❌ | “Analyze yesterday’s workout” |
| `modify_training` | Move, edit, create, or delete workouts and events | ✅ | “Move Saturday’s workout to Sunday” |
| `compare_periods` | Compare two blocks of training | ❌ | “Compare this month vs last month” |
| `assess_recovery` | Assess readiness, recovery, and red flags | ❌ | “Am I ready for intensity tomorrow?” |
| `manage_profile` | View or update thresholds, zones, and profile settings | ✅ | “Update my threshold values from a lab test” |
| `manage_gear` | List, add, or retire gear | ✅ | “How much mileage is on my shoes?” |
| `analyze_race` | Post-race analysis and follow-up guidance | ❌ | “How did my 50K go?” |

### Resource

| Resource | Purpose |
|---|---|
| `intervals-icu://athlete/profile` | Ongoing athlete context including profile and fitness-related information |

### Public contract rules

- **Names are outcome-oriented**, not endpoint-oriented.
- **Arguments are flattened** so agents do not have to invent nested structures.
- **Successful intent results use structured MCP output** via `structuredContent`.
- **Intent tool calls avoid duplicating the same payload into text `content`**, reducing token waste.
- **Error and partial states are guidance-driven**, so the model is told what to do next.

## Architecture at a glance

```text
LLM Host (VS Code / Claude / Cursor / other MCP client)
            |
            | calls one high-level intent
            v
  +-----------------------------+
  | Intent Layer                |
  | 8 public coaching intents   |
  +-----------------------------+
            |
            v
  +-----------------------------+
  | Intent Router               |
  | validation + idempotency    |
  | orchestration + rendering   |
  +-----------------------------+
            |
            v
  +-----------------------------+
  | Internal Execution Layer    |
  | dynamic OpenAPI runtime     |
  | Intervals client            |
  +-----------------------------+
            |
            v
        Intervals.icu API
```

### Layering philosophy

This README describes the project as a **capability-level MCP server**:

- the LLM interacts with **goals**
- the server handles **orchestration**
- the OpenAPI runtime remains the **internal product/component layer**

That separation is the core design decision behind the current architecture.

## Why it is token-efficient

This server is designed to reduce both **static tool metadata cost** and **dynamic response cost**.

### Smaller tool surface

Instead of flooding the model with endpoint-shaped tools, the server exposes only the intent surface that matters most in real coaching workflows.

### Compact outputs

Responses are shaped for actionability:

- summaries before detail
- decision-ready metrics before raw JSON
- markdown tables and structured content instead of schema dumps
- selective enrichment only when it changes the decision

### Server-side computation

The server computes important metrics and interpretations itself, including portions of:

- readiness context
- fatigue and load guidance
- stream-aware execution signals
- training period summaries

This keeps the model focused on reasoning with the result rather than reconstructing the result.

### Guidance-driven follow-up

Intent responses include `suggestions` and `next_actions` so the host model knows how to continue without trial-and-error tool calling.

## Quick start

### Prerequisites

- **Rust 1.94+** with Cargo, or
- **Docker**

### Get your Intervals.icu credentials

1. Open <https://intervals.icu/settings>
2. Scroll to the **Developer** section
3. Create an API key
4. Copy your API key
5. Note your athlete ID from your profile URL (format: `i123456`)

### Install and run the MCP server

The server supports both **STDIO** (for local MCP clients) and **HTTP** (for remote clients) transport modes via the `MCP_TRANSPORT` environment variable.

```sh
git clone https://github.com/like-a-freedom/rusty-intervals-mcp.git
cd rusty-intervals-mcp

cp .env.example .env
# edit .env and set:
# INTERVALS_ICU_API_KEY=your_api_key_here
# INTERVALS_ICU_ATHLETE_ID=i123456

cargo install --locked --path crates/intervals_icu_mcp
```

#### STDIO mode (default)

For local MCP clients like VS Code Copilot or Claude Desktop:

```sh
export INTERVALS_ICU_API_KEY=your_api_key_here
export INTERVALS_ICU_ATHLETE_ID=i123456
export MCP_TRANSPORT=stdio  # optional: stdio is the default
intervals_icu_mcp
```

#### HTTP mode

For remote MCP clients or when running as a service:

```sh
# Generate secrets for JWT authentication
export JWT_SECRET=$(openssl rand -hex 32)
export JWT_ENCRYPTION_KEY=$(openssl rand -hex 32)

export INTERVALS_ICU_API_KEY=your_api_key_here
export INTERVALS_ICU_ATHLETE_ID=i123456
export MCP_TRANSPORT=http
export MCP_HTTP_ADDRESS=127.0.0.1:3000  # optional: default is 127.0.0.1:3000
intervals_icu_mcp
```

The MCP endpoint is available at `http://<address>/mcp`.

Current HTTP security/runtime notes:
- `/auth` is rate-limited separately for brute-force protection.
- `/mcp` rate limiting is currently applied at the endpoint/peer-IP layer before request authentication, so it is **not** yet keyed by authenticated athlete identity.
- HTTP mode requires `JWT_SECRET` and `JWT_ENCRYPTION_KEY` in addition to the Intervals.icu credentials used during `/auth`. Generate them with:
  ```sh
  export JWT_SECRET=$(openssl rand -hex 32)
  export JWT_ENCRYPTION_KEY=$(openssl rand -hex 32)
  ```

## VS Code / Copilot setup

For local development, the simplest VS Code MCP configuration is:

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "cargo",
      "args": [
        "run",
        "--manifest-path",
        "/absolute/path/to/rusty_intervals_mcp/Cargo.toml",
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

After restarting VS Code, try asking:

```text
@intervals-icu Analyze yesterday's workout
@intervals-icu Build me a 12-week trail ultra plan
@intervals-icu How is my recovery this week?
```

## Claude Desktop setup

Add a local MCP entry pointing at the stdio binary:

```json
{
  "mcpServers": {
    "intervals-icu": {
      "command": "/ABSOLUTE/PATH/TO/intervals_icu_mcp",
      "env": {
        "INTERVALS_ICU_API_KEY": "your_api_key_here",
        "INTERVALS_ICU_ATHLETE_ID": "i123456"
      }
    }
  }
}
```

If you prefer not to hardcode credentials in the MCP client config, use an environment file and a launcher script or shell wrapper instead.

## Example asks

The best way to use this MCP server is to ask for **outcomes**, not API mechanics.

### Planning

- “Create a 10-week build toward a half marathon”
- “Plan next week around four available training days”
- “Build a recovery week after my race”

### Workout and period analysis

- “Analyze yesterday’s threshold workout”
- “Summarize my training for February”
- “Show interval insights from my Tuesday session”
- “What workouts are planned later this week?”

### Recovery and performance management

- “How is my recovery looking over the last 7 days?”
- “Am I ready for intensity tomorrow?”
- “Compare this month with last month”

### Calendar changes

- “Move Saturday’s long run to Sunday”
- “Create a 45-minute recovery run for Wednesday”
- “Preview deleting workouts next week before applying it”

### Profile and gear

- “Show my current running thresholds”
- “Update my profile using my new lab thresholds”
- “How much life is left in my shoes?”

## Deterministic coaching analytics

The read-only coaching layer is intentionally **deterministic**.

It follows this pipeline:

```text
Fetch → Audit → Compute → Interpret → Render
```

### What this means in practice

- the server gathers relevant activities, wellness, events, and profile context
- it checks data availability and degraded-data scenarios
- it computes metrics and summaries in Rust
- it interprets those metrics with explicit rules
- it renders compact, guidance-rich output for the host model

### Where it shows up most strongly

#### `analyze_training`

- single-workout deep dives
- interval-aware and stream-aware analysis modes
- planned workout and calendar event visibility in period windows
- explicit data-availability reporting

#### `assess_recovery`

- readiness framing
- personal-baseline-aware HRV interpretation
- recovery-first guidance and red-flag detection

#### `analyze_race`

- post-race execution review
- recovery-forward follow-up guidance
- comparison-to-plan behavior when a matching calendar event exists

### Why deterministic matters

- **repeatable** — same input, same output
- **testable** — rules can be unit and integration tested
- **explainable** — alerts have evidence
- **token-efficient** — the model receives interpretations, not just raw numbers

## Runtime configuration

See `.env.example` for the standard environment layout.

### Required environment variables

| Variable | Description |
|---|---|
| `INTERVALS_ICU_API_KEY` | Intervals.icu API key |
| `INTERVALS_ICU_ATHLETE_ID` | Athlete ID such as `i123456` |

### Common optional environment variables

| Variable | Default | Description |
|---|---|---|
| `INTERVALS_ICU_BASE_URL` | `https://intervals.icu` | Base URL for the upstream API |
| `INTERVALS_ICU_OPENAPI_SPEC` | unset | Explicit OpenAPI source (HTTP(S) URL or local file) |
| `INTERVALS_ICU_SPEC_REFRESH_SECS` | `300` | Refresh cadence for the cached OpenAPI runtime |
| `RUST_LOG` | unset | Standard Rust logging control |
| `MCP_TRANSPORT` | `stdio` | Transport mode: `stdio` or `http` |
| `MCP_HTTP_ADDRESS` | `127.0.0.1:3000` | Listen address for HTTP mode |
| `MAX_HTTP_BODY_SIZE` | `4194304` | Max request body size in bytes (HTTP mode) |

### OpenAPI runtime behavior

If `INTERVALS_ICU_OPENAPI_SPEC` is **unset**, the runtime:

1. fetches `${INTERVALS_ICU_BASE_URL}/api/v1/docs`
2. builds the internal registry dynamically
3. keeps a cached version in memory
4. falls back to `docs/intervals_icu_api.json` when remote loading is unavailable

If `INTERVALS_ICU_OPENAPI_SPEC` is **set explicitly**, that source becomes authoritative and failures are surfaced instead of silently switching to a different spec.

This gives the project the best of both worlds:

- **live compatibility** with the upstream API
- **stable local fallback** for development and testing

## Development

This repository is a Cargo workspace with two main crates:

| Path | Purpose |
|---|---|
| `crates/intervals_icu_client` | HTTP client, retries, observability, API compatibility helpers |
| `crates/intervals_icu_mcp` | MCP server, intent layer, dynamic runtime, resources, and tests |

### Recommended verification commands

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

### Useful development commands

```sh
# Client examples
cargo run -p intervals_icu_client --example basic_client
cargo run -p intervals_icu_client --example list_recent_activities

# MCP stdio mode (default)
cargo run -p intervals_icu_mcp

# MCP http mode
MCP_TRANSPORT=http cargo run -p intervals_icu_mcp
```

### Testing notes

The codebase includes:

- unit tests around intent handlers and runtime behavior
- mocked HTTP tests for OpenAPI and MCP behavior
- integration tests across the workspace
- ignored live contract checks for selected upstream compatibility cases

## Docker and remote deployment

### Build the container

```sh
docker build -t rusty-intervals:latest .
```

### Run locally with environment variables

#### STDIO mode (default)

```sh
docker run -i --rm \
  -e INTERVALS_ICU_API_KEY=your_key \
  -e INTERVALS_ICU_ATHLETE_ID=i123456 \
  -e MCP_TRANSPORT=stdio \
  rusty-intervals:latest
```

#### HTTP mode

```sh
docker run -i --rm \
  -p 3000:3000 \
  -e INTERVALS_ICU_API_KEY=your_key \
  -e INTERVALS_ICU_ATHLETE_ID=i123456 \
  -e MCP_TRANSPORT=http \
  -e MCP_HTTP_ADDRESS=0.0.0.0:3000 \
  rusty-intervals:latest
```

### Remote MCP clients

If your MCP host supports remote HTTP MCP servers, see `examples/mcp_remote.json` for a minimal example.

For production deployment:

- place the server behind TLS
- add an authentication layer at the proxy or gateway
- do not expose an unauthenticated plain HTTP MCP endpoint directly to the public internet

## License

MIT — see [`LICENSE`](LICENSE).

## Disclaimer

This project is not affiliated with, endorsed by, or sponsored by Intervals.icu. All product names, logos, and brands are the property of their respective owners.
