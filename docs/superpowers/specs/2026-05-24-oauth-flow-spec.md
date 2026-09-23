# OAuth Flow Realignment Specification

> Status: Strengthened and grounded on current implementation
> Source: `docs/OAUTH_FLOW.md`

## Problem

Current HTTP auth UX is custom and JWT-centric:

- user submits raw `api_key` + `athlete_id` to `POST /auth`
- server validates credentials upstream, issues internal JWT, and expects `Authorization: Bearer <jwt>` for `/mcp`
- token lifecycle UX is weak (expired token = unauthorized, no guided recovery)
- no browser-based UI exists for non-programmatic token management
- flow does not expose OAuth Resource Server metadata for standards-driven MCP clients

## Goal

Define a realistic migration target where HTTP auth remains production-safe today while becoming OAuth-ready and standards-aligned without breaking existing JWT clients.

## Scope

### In Scope

- Current-state baseline tied to actual code paths (`auth.rs`, `run_http_server`)
- Gap closure between custom JWT flow and OAuth Resource Server expectations
- Concrete endpoint and middleware evolution strategy
- Backward-compatible migration approach for existing `/auth` clients
- Browser-based token management web UI (create, view, revoke tokens)
- Security, observability, and test requirements for rollout

### Out of Scope

- Replacing STDIO mode credential model
- Rewriting intent router or business-intent handlers
- Hard dependency on unsupported upstream OAuth grants
- User account registration or password management

## Current Implementation Baseline

- HTTP mode is enabled via `MCP_TRANSPORT=http` and requires `JWT_MASTER_KEY`.
- `POST /auth` validates Intervals credentials via `get_athlete_profile`, then issues encrypted JWT claims (`athlete_id`, encrypted API key).
- `auth_middleware` verifies JWT on `/mcp`, extracts per-request credentials, and injects them into request extensions.
- `/mcp` is protected by auth middleware, request timeout, body-size limits, and rate limiting.
- `/health` and `/metrics` are available in HTTP mode; auth metrics are already emitted.

## Gap Analysis (Weak Points)

1. **Bootstrap friction**
	- raw API key must be handled by the user/client before any MCP call.

2. **Lifecycle ergonomics**
	- token expiry currently results in unauthorized response without protocol-level discovery flow.

3. **Standards discoverability gap**
	- no `/.well-known/oauth-protected-resource` endpoint is exposed.

4. **Mixed trust model is implicit**
	- custom JWT server role and future external OAuth trust boundaries are not documented as a single contract.

5. **Migration strategy is underspecified**
	- no explicit compatibility mode for legacy JWT bootstrap vs OAuth bootstrap.

## Design Summary

### Target Model

Adopt a multi-path auth model in HTTP mode:

- **Compatibility path (existing):** keep `POST /auth` + internal JWT for current programmatic clients.
- **Standards path (new):** add OAuth Resource Server metadata and OAuth callback exchange path that culminates in the same internal authenticated session model.
- **Web UI path (new):** add a browser-based token management interface at `/ui` for humans to create, view, and revoke tokens using `maud` + `maud-ui` server-rendered HTML.

### Why this model

- preserves working production behavior
- avoids flag-day migration
- keeps request-time auth enforcement centralized in existing middleware
- allows incremental adoption by clients that support OAuth discovery

### Core Principle

All authenticated `/mcp` requests continue to rely on a server-verified, local trust artifact (existing JWT/session token model), even when bootstrap originates from external OAuth authorization.

## Implementation Requirements

### Functional Requirements

- FR-1: keep current `POST /auth` flow operational for backward compatibility.
- FR-2: add OAuth protected resource metadata endpoint for client discovery.
- FR-3: add OAuth bootstrap endpoints for login redirect and callback code exchange.
- FR-4: callback path must map external OAuth token data to existing internal auth context used by middleware.
- FR-5: middleware must continue rejecting unauthenticated requests deterministically.
- FR-6: auth metrics must distinguish bootstrap mode (`jwt`, `oauth`, `ui`).
- FR-7: add `GET /ui` landing page with token creation form.
- FR-8: add `POST /ui/token` for credential submission and token issuance.
- FR-9: add `GET /ui/tokens` for token listing with active/revoked status.
- FR-10: add `POST /ui/revoke/:jti` for token revocation.
- FR-11: enforce CSRF protection on all UI form POSTs.
- FR-12: enforce rate limiting on all UI routes.

### Non-Functional Requirements

- NFR-1: no regression for current HTTP auth latency and error handling semantics.
- NFR-2: clear audit logs for auth bootstrap, failures, and token verification outcomes.
- NFR-3: secure secret handling for OAuth client credentials (env-only, never logged).
- NFR-4: graceful degradation when OAuth configuration is absent (JWT bootstrap still works).

## API Surface Adjustments

### Plan A — OAuth Authorization Flow

- Keep: `POST /auth`
- Add: `GET /.well-known/oauth-protected-resource`
- Add: `GET /auth/login` (redirect)
- Add: `GET /auth/callback` (authorization code exchange)
- Keep: `/mcp` auth middleware gate as single enforcement point

### Plan B — Auth Improvements + Web UI

- Keep: `POST /auth`
- Add: `GET /ui` (landing page with token form)
- Add: `POST /ui/token` (credential → JWT)
- Add: `GET /ui/tokens` (token list with status)
- Add: `POST /ui/revoke/:jti` (revoke token)
- Add: `GET /ui/static/css` (maud-ui stylesheet)
- Add: `GET /ui/callback` (OAuth callback for browser users, if OAuth configured)

## Security Considerations

- enforce `state` validation in login/callback flow
- use PKCE for public-client friendly code flow
- isolate OAuth client secret usage to server-side callback exchange
- continue encrypted storage/transport semantics for internal credential artifacts
- keep auth failure metrics and rate limits on auth endpoints
- CSRF token per session on all UI form POSTs
- HttpOnly + SameSite=Strict session cookies for UI
- rate limiting on UI routes (2 req/s, burst 5)
- in-memory token registry only; no persistent token storage

## Testing Strategy

### Plan A — OAuth Authorization Flow

- Unit: state/PKCE helpers, callback validation paths, metadata response contract
- Integration: auth route wiring, middleware acceptance/rejection for both bootstrap modes
- E2E HTTP: obtain auth via both paths, call `/mcp`, verify identical downstream behavior
- Negative: tampered state, invalid code exchange, missing bearer token, expired token

### Plan B — Auth Improvements + Web UI

- Unit: CSRF validation, session management, token registry operations
- Integration: UI route wiring, CSS serving, redirect behavior with/without session
- Negative: missing CSRF → redirect, invalid credentials → redirect, malformed JTI → no-op

## Success Criteria

### Plan A — OAuth Authorization Flow

- Existing JWT bootstrap clients continue working without changes.
- OAuth-capable clients can discover protected-resource metadata and complete auth bootstrap.
- `/mcp` authorization behavior is unchanged from handler perspective.
- Auth events are observable by metrics/logs for both bootstrap paths.

### Plan B — Auth Improvements + Web UI

- Web UI renders at `/ui` with token creation form.
- Users can create, view, and revoke tokens via the browser.
- CSRF and rate limiting protect all UI routes.
- Token revocation is tracked (in-memory), with documented limitation.
- All UI actions are recorded in metrics and structured logs.

### Shared

- Spec remains implementation-focused and free of schedule commitments.
