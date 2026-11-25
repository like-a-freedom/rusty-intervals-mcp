# Intervals.icu MCP Architecture Specification

> Status: Historical design summary.
> Original and maintained source: [architecture overview](../../ARCHITECTURE.md) (layering). See [ADR-0012](../../adr/0012-single-process-sqlite-state.md) for v3.0.0 persistence.

## Problem

Architecture content is broad and multilingual; contributors need a concise spec view that highlights critical runtime decisions and boundaries.

## Goal

Summarize transport modes, intent-driven runtime, dynamic OpenAPI internals, and deterministic coach layer as one coherent architecture contract.

## Scope

### In Scope

- Intent-driven architecture overview
- Transport modes: STDIO and streamable HTTP
- Dynamic OpenAPI runtime and internal component boundaries
- Deterministic coach layer and key runtime flows

### Out of Scope

- Endpoint-level low-level implementation details
- Historical migration narrative not affecting current behavior

## Design Summary

- Unified intent runtime with identical logical contract across transports.
- Two deployment modes (local STDIO and HTTP) share core execution pipeline.
- Dynamic OpenAPI layer supports internal API-driven extensibility.
- Deterministic coach functionality remains integrated with intent handlers.

## v3.0.0 implementation notes

- Transport modes are currently `stdio` and `http` via `MCP_TRANSPORT`; unknown values exit with error (`crates/intervals_icu_mcp/src/lib.rs`, `run`).
- Handler initialization is split by mode: single-user credentials from env in STDIO and multi-tenant JWT flow in HTTP (`initialize_handler_single_user`, `run_http_server`).
- The intent router exposes nine coaching tools, including `track_progress` (`tool_count`, `list_tools`).
- The dynamic OpenAPI runtime is private; only STDIO startup preloads it. Neither transport exposes upstream operation IDs as MCP tools.

## Success Criteria

- New contributors can identify architecture layers and responsibilities quickly.
- Runtime contract is clear across transport modes.
- Internal extension points are documented without exposing unstable internals.
