# Relative date support across intent tools

**Date:** 2026-03-21
**Status:** Approved for implementation

## Goal

Support relative date inputs such as `today` consistently across intent-driven MCP tools that accept ordinary date fields, so agents do not have to pre-resolve current dates before calling the server.

## Problem

The current codebase has split behavior:

- shared validation in `src/intents/validator.rs` already allows relative date strings like `today`
- several handlers still parse dates directly with `NaiveDate::parse_from_str(..., "%Y-%m-%d")`
- as a result, requests can pass validation and still fail later inside handlers

This creates inconsistent user-visible behavior and breaks tool ergonomics.

## Chosen approach

Implement a shared date parser in `src/intents/utils.rs` and migrate handlers that consume ordinary day-level date inputs to use it.

This is preferred over patching each handler independently because it:

- centralizes date semantics
- keeps validation and execution aligned
- reduces future regressions when new tools add date fields

## Scope

### In scope

- shared support for relative day aliases:
  - `today`
  - `tomorrow`
  - `yesterday`
- migrating handlers that accept regular date inputs and currently parse them manually
- regression tests for both utility-level parsing and real intent execution

### Out of scope

- replacing specialized planning syntax such as `next_monday` or duration-like inputs such as `12weeks`
- broad natural-language date parsing
- changing API response formats beyond necessary schema/help text updates

## Design

### Shared parser

Add a utility function that resolves a date string into `NaiveDate` using the following rules:

1. if the value is `today`, return local current date
2. if the value is `tomorrow`, return local current date + 1 day
3. if the value is `yesterday`, return local current date - 1 day
4. otherwise, parse `YYYY-MM-DD`

The existing `parse_date(...)` helper should become the canonical parser for ordinary date fields.

### Handler migration

Update handlers that currently use `NaiveDate::parse_from_str(..., "%Y-%m-%d")` for ordinary date fields to use the shared parser.

Expected first targets:

- `analyze_training`
- `compare_periods`
- `modify_training` for day-level date fields where appropriate
- any similar intent code paths that accept ordinary dates and are not using special planning grammar

### Validation alignment

No special router-layer transformation is required if the shared parser semantics match validator semantics.

Validator behavior already accepts relative date strings; execution should now resolve them consistently instead of rejecting them later.

## Testing

Add:

- unit tests for shared parser relative aliases
- integration regression test proving `analyze_training` accepts `date: "today"`
- focused tests for any additional migrated handlers where relative dates are supported

## Risks and mitigations

### Risk: breaking specialized date grammars

Mitigation:
- only route ordinary day-level parsing through the shared parser
- leave specialized planning/date-range grammars untouched unless explicitly migrated with tests

### Risk: timezone confusion

Mitigation:
- use local current date consistently, matching existing server-side expectations for athlete-facing calendar dates

## Success criteria

- requests with `date: "today"` succeed for ordinary single-day intent tools
- validation and execution no longer disagree on accepted relative dates
- tests cover both parser utility behavior and at least one real tool regression path
