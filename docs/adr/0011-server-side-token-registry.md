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
- The auth middleware resolves credentials from the registry by `jti` on each
  request. It rejects revoked records and athlete mismatches, and **fails closed**
  on a registry read failure or an unknown `jti` with no legacy credential.
- The UI lists the registry (filterable by athlete), not a session-scoped
  subset; the browser session only supplies a default filter.
- **Backward compatibility:** legacy tokens (credential embedded in claims, no
  or unknown `jti`) remain verifiable until they expire, via a documented
  claims-fallback path.

## Consequences

- Revocation applies to every newly issued token, not just UI-issued ones.
  Historical tokens with embedded credentials still use the compatibility path
  above when no registry record is available; that is not durable revocation.
- Newly issued bearer tokens no longer carry the upstream credential. The bearer
  token itself still grants access and must be kept secret.
- The `/ui` surface is a trusted operator panel (like `/metrics`); it is not
  multi-tenant authenticated. It lists token metadata only, never credentials.

## v3.0.0 implementation

- Standard HTTP startup requires the local SQLite store described in
  [ADR-0012](0012-single-process-sqlite-state.md); there is no in-memory fallback.
  Keep the database and `JWT_MASTER_KEY` across restarts. The old
  `MCP_TOKEN_REGISTRY_PATH` setting is rejected at startup, JSON registry records
  (including revocations) are not imported, and JWTs that depend on those
  records must be reissued. This is separate from legacy claims compatibility.
- Rust embedders can call `AppState::rotate_upstream_api_key` with a verified JWT
  and a nonblank replacement key. It updates only the encrypted credential of
  the matching, non-revoked `jti` and athlete. The embedder must validate the new
  key with Intervals.icu. The JWT, expiry, and mutation replay owner are unchanged.
  Subsequent requests see the replacement; already authenticated requests may
  still use the old key. No HTTP route, operator UI action, or MCP tool exposes
  this method.

See the [README token-management section](../../README.md#token-management-web-ui)
for operational usage.

## Alternatives considered

- **Keep stateless tokens.** Rejected: no revocability, no rotation, secret in
  the bearer artifact.
- **Store only a key fingerprint server-side**, keep the encrypted key in the
  token. Rejected: still ships secret material and still cannot rotate.
- **Shorten TTL and document the blast radius.** Rejected: reduces but does not
  remove the exposure, and adds UX friction without enabling revocation.
