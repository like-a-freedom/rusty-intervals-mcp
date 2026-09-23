# Multi-Tenant HTTP with JWT Specification

> Status: Verified against current codebase and implementation
> Source: `docs/MULTI_TENANT_HTTP.md`

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

## Current Implementation Alignment

- HTTP mode requires `JWT_MASTER_KEY`; startup validates key format and initializes JWT manager (`run_http_server` in `crates/intervals_icu_mcp/src/lib.rs`).
- Multi-tenant mode creates per-request credentials from verified bearer tokens in middleware (`auth_middleware` in `crates/intervals_icu_mcp/src/auth.rs`).
- `/auth` endpoint validates upstream credentials before issuing token with configured TTL (`auth_endpoint` in `crates/intervals_icu_mcp/src/auth.rs`).
- HTTP router includes `/mcp` plus auth, health, and metrics routes with rate limit, body limit, and timeout layers (`run_http_server`).

## Success Criteria

- Tenant requests are authenticated and scoped correctly.
- Setup procedure is reproducible in local and deployed environments.
- Security assumptions are explicit and testable.
