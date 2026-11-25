# Dynamic OpenAPI Cutover Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/DYNAMIC_OPENAPI_CUTOVER.md`.
> The direct-cutover design below is not the v3.0.0 runtime contract. [ADR-0001](../../adr/0001-two-layer-architecture-with-dynamic-upstream-adapter.md) records the incremental adapter with typed fallback and opt-in mapped dispatch.

## Problem

Cutover from transition-era runtime to fully dynamic OpenAPI runtime needs explicit constraints and acceptance checks.

## Goal

Define a no-transition cutover path with clear scope, FR/NFR requirements, architecture target, and Definition of Done.

## Scope

### In Scope

- Immediate dynamic OpenAPI cutover requirements
- Business constraints and operational decisions
- Functional and non-functional requirements
- Acceptance criteria and verification checkpoints

### Out of Scope

- Long hybrid-transition compatibility mode
- Parallel legacy runtime maintenance

## Design Summary

- Source specifies direct cutover strategy and verification boundaries.
- FR/NFR sections frame both behavior and reliability expectations.
- Post-cutover clarification ensures consistency with real runtime behavior.

## Success Criteria

- Legacy transition dependencies are removed.
- Dynamic runtime satisfies documented FR/NFR requirements.
- Acceptance criteria are measurable and test-backed.
