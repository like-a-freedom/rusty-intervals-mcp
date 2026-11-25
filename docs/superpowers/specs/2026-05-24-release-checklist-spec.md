# Release Checklist Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/RELEASE_CHECKLIST.md`.
> Use the maintained [release checklist](../../../RELEASING.md#release-checklist) for actual release gates.

## Problem

Release checklist content is short and practical, but not normalized as a reusable specification artifact.

## Goal

Represent release checklist intent as a compact spec that can be referenced in delivery workflows.

## Scope

### In Scope

- Checklist-driven release validation gates
- Operational handoff/readiness expectations

### Out of Scope

- CI pipeline implementation details
- Post-release incident processes

## Design Summary

- Source is a concise checklist artifact.
- This spec formalizes checklist usage as a release quality contract.

## Success Criteria

- Release checklist remains discoverable in superpowers spec set.
- Teams can reference this artifact during pre-release validation.
