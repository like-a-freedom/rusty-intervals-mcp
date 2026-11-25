# Intent-Driven Skills Architecture Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/INTENT_DRIVEN_SKILLS.md`.
> For current layering and tools, see [ADR-0001](../../adr/0001-two-layer-architecture-with-dynamic-upstream-adapter.md) and the [README intent list](../../../README.md#intents). The design summary records original intent, not a separate skills runtime.

## Problem

The intent-driven skills architecture is comprehensive but large; implementation teams need a concise operational spec.

## Goal

Capture core architecture, principles, user scenarios, edge cases, and identifier-handling rules in a compact superpowers-compatible format.

## Scope

### In Scope

- High-level architecture and key principles
- Primary user scenarios and edge-case handling
- Business identifier processing requirements
- Requirements and constraints for skills-oriented intent runtime

### Out of Scope

- Detailed code-level implementation of each intent
- Tool-specific UIs outside MCP runtime scope

## Design Summary

- Source is SRS-scale with strong architectural and behavioral guidance.
- Core model: intents represent business outcomes; skills compose reusable capability paths.
- Emphasis on deterministic routing, safe boundaries, and explicit edge-case handling.

## v3.0.0 implementation notes

- The server currently registers nine intent handlers: `plan_training`, `analyze_training`, `modify_training`, `compare_periods`, `assess_recovery`, `manage_profile`, `manage_gear`, `analyze_race`, `track_progress` (`crates/intervals_icu_mcp/src/lib.rs`).
- Tool listing intentionally exposes only intent tools; dynamic OpenAPI operations remain internal-only (`list_tools`, `tool_count`).
- README intent table matches current handler set and mutating/non-mutating semantics (`README.md`, intents table).

## Success Criteria

- Architecture can be implemented/refined without ambiguity.
- Identifier and edge-case rules are testable.
- Spec supports stable evolution of skills-driven MCP behavior.
