# Deterministic Analytical Coach Logic Specification

> Status: Verified against current codebase and implementation
> Source: `docs/ANALYTICAL_COACH_REPORTING.md`

## Problem

Analytical coach logic was previously described in a long SRS-style document, but the implementation-facing boundaries and acceptance path were hard to consume quickly.

## Goal

Define a deterministic coach layer inside existing intents with explicit public/internal surfaces, stable principles, and testable behavior contracts.

## Scope

### In Scope

- Deterministic analytics behavior inside current intent handlers
- Clear architectural split (public API surface vs internal components)
- Methodological basis, constraints, and implementation principles
- Requirements and acceptance criteria aligned to existing repo structure

### Out of Scope

- Introducing new external MCP tools solely for coach logic
- Replacing intent-driven architecture with a separate agent runtime
- Non-deterministic/LLM-generated analytical decisions

## Design Summary

- Source sections include: goal, related docs, architecture, design rationale, and mandatory principles.
- Core decision: keep analytics deterministic and embedded into current intent processing flow.
- Key requirement: preserve maintainability through explicit module boundaries and stable interfaces.
- Validation focus: reproducible outputs and predictable behavior under identical inputs.

## Success Criteria

- Deterministic coach decisions are documented and implementable without ambiguity.
- Public vs internal interfaces are unambiguous for contributors.
- Acceptance criteria in source can be mapped directly to tests and runtime checks.
