# Multi-Tenant HTTP with JWT Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/MULTI_TENANT_HTTP.md`.
> For running HTTP mode, use the [README quick start](../../../README.md#quick-start), [ADR-0011](../../adr/0011-server-side-token-registry.md) and [ADR-0012](../../adr/0012-single-process-sqlite-state.md).

## Problem

Multi-tenant HTTP mode introduces auth and isolation requirements that need a compact, implementation-facing spec.

## Goal

Document secure JWT-based tenant authentication flow, setup steps, and security model for multi-tenant MCP HTTP operation.

## Scope

### In Scope

- Multi-tenant HTTP architecture
- JWT auth flow and token usage
- Secret generation and environment configuration
- Security model and request authorization behavior

### Out of Scope

- OAuth-style interactive user login
- Tenant billing/business logic

## Design Summary

- Source provides practical setup workflow from secret generation to authenticated requests.
- JWT bearer token is the core auth primitive for tenant isolation.
- Security section frames boundaries and expected protections.

## v3.0.0 implementation notes

- HTTP mode requires `JWT_MASTER_KEY`; startup validates key format and initializes JWT manager (`run_http_server` in `crates/intervals_icu_mcp/src/lib.rs`).
- HTTP startup also requires `MCP_STATE_DB_PATH` and `MCP_STATE_ENCRYPTION_KEY`; it has no volatile registry fallback ([ADR-0012](../../adr/0012-single-process-sqlite-state.md)).
- The JWT middleware resolves credentials on each request from the SQLite registry by `jti`, with a legacy-claims fallback ([ADR-0011](../../adr/0011-server-side-token-registry.md)). OAuth discovery and exchange are not implemented.
- `POST /auth` and the operator UI validate upstream credentials and record each new JWT in the registry. The JWT carries identity, not the upstream API key.
- HTTP router includes `/mcp` plus auth, health, and metrics routes with rate limit, body limit, and timeout layers (`run_http_server`).

## Success Criteria

- Tenant requests are authenticated and scoped correctly.
- Setup procedure is reproducible in local and deployed environments.
- Security assumptions are explicit and testable.
