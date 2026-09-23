# Observability and Prometheus Metrics Specification

> Status: Verified against current codebase and implementation
> Source: `docs/OBSERVABILITY_SRS.md`

## Problem

Observability requirements span multiple metric domains and need concise alignment across implementation, operations, and testing.

## Goal

Define observability architecture, metric emission points, endpoint contracts, and requirement scope for Prometheus-based monitoring.

## Scope

### In Scope

- Metrics architecture and dependencies
- Emission points and endpoint behavior
- Upstream API metrics, protocol metrics, and auth metrics
- Functional scope and required monitoring coverage

### Out of Scope

- Vendor-specific dashboard styling
- Non-Prometheus observability backends

## Design Summary

- Source provides SRS-level requirements for metric collection and exposure.
- Metrics are grouped by concern: upstream calls, MCP protocol behavior, authentication.
- Endpoint and emission placement are explicit for reliable instrumentation.

## Current Implementation Alignment

- Metrics recorder is initialized in HTTP mode startup (`run_http_server` calling `metrics::init_prometheus_recorder`).
- Metrics subsystem is HTTP-only by design; STDIO mode returns no-op/error for recorder initialization (`crates/intervals_icu_mcp/src/metrics.rs`).
- Router merges dedicated metrics endpoint via `metrics::create_metrics_router()` (`crates/intervals_icu_mcp/src/lib.rs`).
- Current metrics include tool calls/duration, token issuance/verification, HTTP transport counters, and active-athlete tracking (`crates/intervals_icu_mcp/src/metrics.rs`, `README.md` observability section).

## Success Criteria

- Required metric families are implemented and exposed.
- Observability contract supports troubleshooting and SLO tracking.
- Monitoring behavior is validated through tests or runtime checks.
