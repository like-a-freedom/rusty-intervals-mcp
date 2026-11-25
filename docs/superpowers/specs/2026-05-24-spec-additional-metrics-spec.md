# Additional Metrics Specification (Montis Reference)

> Status: Historical design summary.
> Original source (no longer tracked): `docs/SPEC_ADDITIONAL_METRICS.md`.
> The original proposals do not establish which metrics shipped; see [metric methods](../../METRIC_METHODS.md) and [deterministic coaching analytics](../../../README.md#deterministic-coaching-analytics). The old cache-path discussion is not current configuration; see [ADR-0012](../../adr/0012-single-process-sqlite-state.md).

## Problem

Additional metric definitions are valuable but spread across a longer document and need concise implementation framing.

## Goal

Normalize definitions and integration guidance for extra metrics (including fatigue/stress-related measures) into superpowers spec format.

## Scope

### In Scope

- Metric definitions and implementation notes
- Integration strategy for existing system
- Historical configuration concerns, including the old idempotency cache path

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
