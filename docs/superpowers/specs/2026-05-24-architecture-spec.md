# Intervals.icu MCP Architecture Specification

> Status: Verified against current codebase and implementation
> Source: `docs/ARCHITECTURE.md`

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

## Current Implementation Alignment

- Transport modes are currently `stdio` and `http` via `MCP_TRANSPORT`; unknown values exit with error (`crates/intervals_icu_mcp/src/lib.rs`, `run`).
- Handler initialization is split by mode: single-user credentials from env in STDIO and multi-tenant JWT flow in HTTP (`initialize_handler_single_user`, `run_http_server`).
- Intent router exposes 8 high-level business intents to host tools (`tool_count`, `list_tools`).
- Dynamic OpenAPI runtime is present and preloaded internally, but dynamic tools are not exposed directly to the LLM host (`preload_dynamic_registry`, `dynamic` module, `list_tools`).

## Success Criteria

- New contributors can identify architecture layers and responsibilities quickly.
- Runtime contract is clear across transport modes.
- Internal extension points are documented without exposing unstable internals.
