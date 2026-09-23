# Output Schema Documentation Specification

> Status: Verified against current codebase and implementation
> Source: `docs/OUTPUT_SCHEMA.md`

## Problem

Output schema behavior and MCP compliance details are documented but need a compact implementation contract.

## Goal

Define the standard output schema approach, IntentOutput mapping to MCP results, and migration/validation expectations.

## Scope

### In Scope

- Standard output schema conventions
- `IntentOutput` and MCP `CallToolResult` relationship
- Implementation location and usage pattern
- Validation and migration notes

### Out of Scope

- Intent business-logic redesign
- Non-MCP output format protocols

## Design Summary

- Source focuses on consistency, compliance, and predictable tool output consumption.
- Contract clarifies schema behavior for analytical intents and general tools.
- Migration notes provide backward-compatible transition guidance.

## Current Implementation Alignment

- Tool construction in `list_tools` still propagates `output_schema` when present using `with_raw_output_schema` (`crates/intervals_icu_mcp/src/lib.rs`).
- Intent tool exposure remains explicit and bounded to router definitions; schema-bearing tool metadata is preserved for host consumption.
- README continues to reflect token-efficiency and guidance-driven error/partial-state output behavior.

## Success Criteria

- Output contract is unambiguous for implementers and tool consumers.
- Validation/migration guidance supports safe rollout.
- Schema documentation matches current runtime behavior.
