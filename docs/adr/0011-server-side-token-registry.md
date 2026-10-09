# Server-side token registry; JWTs carry identity, not secrets

## Context

HTTP multi-tenant mode issues bearer JWTs. Historically:

- the JWT custom claims embedded the AES-256-GCM-encrypted upstream API key;
- only tokens issued through the web UI carried a `jti` and were recorded in a
  UI-owned registry, so only they were revocable;
- tokens issued via `POST /auth` had `jti: None`, were not recorded, and were
  therefore stateless and permanently non-revocable until they expired;
- the UI token list was filtered by `session.athlete_id` — an ephemeral,
  in-memory browser-session value — so losing the session hid the operator's
  own tokens behind a dead-end ("no athlete in this session").

Two problems follow. The bearer artifact carries secret material (a leak of the
token plus the master key exposes the upstream credential, and the token cannot
be rotated without re-issuing), and the inventory of issued tokens depends on an
identity that is not authoritative for ownership.

## Decision

The **server-side token registry** is the single source of truth for issued
tokens, keyed by `jti`.

- Every issued token — via `POST /auth` and via the web UI alike — is assigned a
  `jti` and recorded in the registry. A record holds the athlete, issue/expiry
  timestamps, revocation state, and the athlete's upstream credential
  **encrypted at rest** with the master-key-derived AES-256-GCM key.
- The JWT carries only `athlete_id` + `jti` and the standard registered claims.
  The upstream credential never travels in the token.
- The auth middleware resolves credentials from the registry by `jti`, rejects
  revoked records unconditionally, and **fails closed** on an unknown `jti` that
  carries no legacy credential.
- The UI lists the registry (filterable by athlete), not a session-scoped
  subset; the browser session only supplies a default filter.
- **Backward compatibility:** legacy tokens (credential embedded in claims, no
  or unknown `jti`) remain verifiable until they expire, via a documented
  claims-fallback path.

## Consequences

- Revocation now works for *every* token, not just UI-issued ones. Because the
  credential lives in the registry rather than the token, an upstream key can
  also be rotated by updating its record — no token re-issue required.
- Bearer tokens no longer carry secret material.
- The registry must be persisted (`MCP_TOKEN_REGISTRY_PATH`) or newly issued
  tokens are rejected after a restart — the deliberate cost of a stateful,
  revocable token model. The server warns at startup when the registry is
  in-memory.
- The `/ui` surface is a trusted operator panel (like `/metrics`); it is not
  multi-tenant authenticated. It lists token metadata only, never credentials.

## Alternatives considered

- **Keep stateless tokens.** Rejected: no revocability, no rotation, secret in
  the bearer artifact.
- **Store only a key fingerprint server-side**, keep the encrypted key in the
  token. Rejected: still ships secret material and still cannot rotate.
- **Shorten TTL and document the blast radius.** Rejected: reduces but does not
  remove the exposure, and adds UX friction without enabling revocation.
