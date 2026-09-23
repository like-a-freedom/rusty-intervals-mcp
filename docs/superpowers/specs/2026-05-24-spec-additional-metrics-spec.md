# Additional Metrics Specification (Montis Reference)

> Status: Verified against current codebase and implementation
> Source: `docs/SPEC_ADDITIONAL_METRICS.md`

## Problem

Additional metric definitions are valuable but spread across a longer document and need concise implementation framing.

## Goal

Normalize definitions and integration guidance for extra metrics (including fatigue/stress-related measures) into superpowers spec format.

## Scope

### In Scope

- Metric definitions and implementation notes
- Integration strategy for existing system
- Configuration/environment concerns such as idempotency cache path context

### Out of Scope

- Full recalibration of existing metric system
- Non-documented experimental metrics

## Design Summary

- Source outlines multiple additional metrics with definition/implementation/integration triplets.
- Focus is incremental extension, not full analytical model replacement.
- Config considerations are included to keep rollout operationally safe.

## Success Criteria

- Additional metrics are documented in implementation-ready form.
- Integration points are explicit and bounded.
- Config dependencies are visible to maintainers.
