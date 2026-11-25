# Integration Tests Guide Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/INTEGRATION_TESTS.md`.
> Use the [contribution guide](../../../CONTRIBUTING.md) and [README testing notes](../../../README.md#testing-notes) for current commands and prerequisites.

## Problem

Integration test setup includes credentials and environment assumptions that can create onboarding friction and inconsistent execution.

## Goal

Define repeatable integration-test setup and execution flow with clear prerequisites, environment requirements, and run modes.

## Scope

### In Scope

- Prerequisites and credential requirements
- Required/optional environment variables
- Commands/patterns for full and targeted integration test runs

### Out of Scope

- Unit-test strategy
- Production secret management policy changes

## Design Summary

- Source is operational and command-centric.
- Spec emphasizes reproducibility and explicit env configuration.
- Different test run scopes (all vs specific) are documented as first-class flows.

## Success Criteria

- Contributors can run integration tests consistently after setup.
- Env requirements are explicit and unambiguous.
- Test invocation paths are easy to discover and reuse.
