# API Coverage Diff Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/API_DIFF.md`.
> No maintained coverage diff is linked here. See the [README intent list](../../../README.md#intents) for the current public contract; this summary is not a current gap inventory.

## Problem

Coverage between Intervals.icu API surface and MCP implementation can drift over time, making planning and prioritization harder.

## Goal

Provide a structured and actionable diff of implemented vs missing API capabilities across tool groups.

## Scope

### In Scope

- Coverage inventory for activities, athlete/profile, wellness, events/calendar, gear, settings, and related tool families
- Explicit implemented vs missing capability mapping
- Gap visibility for roadmap and implementation planning

### Out of Scope

- Runtime behavior changes
- Wire-format/schema redesign
- Business-priority decisions beyond documented gaps

## Design Summary

- Source groups API coverage by domain and tool count.
- Implemented MCP tools and missing endpoints are separated for clear triage.
- Diff acts as backlog input, not as a replacement for integration tests.

## Success Criteria

- Every major Intervals API domain has an explicit implemented/missing status.
- Teams can derive prioritized tasks from the documented gaps.
- Coverage report stays compatible with intent-driven tool model.
