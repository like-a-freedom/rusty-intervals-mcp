# Streamable HTTP Migration Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/MIGRATION_TO_STREAMABLE_HTTP.md`.
> The v2.1 migration is retained as history. For current STDIO/HTTP setup, use the [README quick start](../../../README.md#quick-start) and [HTTP reliability](../../HTTP_RELIABILITY.md).

## Problem

Migration to Streamable HTTP v2.1 changes binary/runtime assumptions and can cause deployment drift if not standardized.

## Goal

Define a concise migration contract for STDIO users, HTTP users, and Docker users with clear deltas from previous versions.

## Scope

### In Scope

- v2.1 behavior changes and removed components
- Unified binary and transport-mode configuration
- Migration steps by runtime profile (STDIO, HTTP, Docker)

### Out of Scope

- Reintroduction of removed legacy components
- Non-v2.1 compatibility guarantees

## Design Summary

- Source explicitly contrasts old/new runtime behavior.
- Migration path is segmented by user/deployment mode.
- Unified binary and transport switching are key operational pivots.

## Success Criteria

- Existing users migrate without transport confusion.
- Deprecated components are fully removed from operational guidance.
- v2.1 runtime behavior is consistently configured across environments.
