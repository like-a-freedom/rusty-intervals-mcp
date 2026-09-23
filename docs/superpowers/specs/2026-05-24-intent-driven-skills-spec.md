# Intent-Driven Skills Architecture Specification

> Status: Verified against current codebase and implementation
> Source: `docs/INTENT_DRIVEN_SKILLS.md`

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

## Current Implementation Alignment

- The server currently creates and wires 8 intent handlers: `plan_training`, `analyze_training`, `modify_training`, `compare_periods`, `assess_recovery`, `manage_profile`, `manage_gear`, `analyze_race` (`crates/intervals_icu_mcp/src/lib.rs`).
- Tool listing intentionally exposes only intent tools; dynamic OpenAPI operations remain internal-only (`list_tools`, `tool_count`).
- README intent table matches current handler set and mutating/non-mutating semantics (`README.md`, intents table).

## Success Criteria

- Architecture can be implemented/refined without ambiguity.
- Identifier and edge-case rules are testable.
- Spec supports stable evolution of skills-driven MCP behavior.
