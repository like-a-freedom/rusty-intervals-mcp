# OpenAPI Event Schema Fix Specification

> Status: Verified against current codebase and implementation
> Source: `docs/OPENAPI_EVENT_FIX.md`

## Problem

OpenAPI spec was missing `Event` schema and related event path, causing incomplete API representation and tooling inconsistencies.

## Goal

Document and lock the fix that adds `Event` schema and `GET /api/v1/athlete/{id}/events/{eventId}` path.

## Scope

### In Scope

- OpenAPI schema fix for event resource
- Path addition for single-event retrieval
- Compatibility expectations for generated/internal tooling

### Out of Scope

- Broader event API redesign
- Non-event schema refactoring

## Design Summary

- Source is narrowly scoped to a concrete OpenAPI correction.
- Fix improves spec completeness and downstream generation reliability.
- Change is intentionally surgical and low-risk.

## Success Criteria

- `Event` schema exists in OpenAPI spec.
- `GET /api/v1/athlete/{id}/events/{eventId}` is present and valid.
- Consumers relying on OpenAPI can access event resource definition consistently.
