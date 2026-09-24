# ADR-0010 — Runtime Public Base Path for Prefix Pass-Through

| Field | Value |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-09-24 |
| **Deciders** | Deployment design for shared domain `mcp.like-a-freedom.ru` |

## Context

Multiple MCP servers share one domain under path prefixes
(`https://mcp.like-a-freedom.ru/memory`, `…/intervals`, …). The memory
service was fixed with a relocatable, prefix-agnostic bundle plus serve-time
asset stamping. This repository has no SPA/build-time bundle — the web UI is
server-rendered (maud) — but it emitted absolute root URLs (`/ui/...` in
hrefs, form actions, redirects) and a cookie `Path=/ui`, so UI resources
escaped any prefix. The MCP transport itself had no prefix awareness; the
proxy compensated by rewriting `/intervals` → `/mcp` (root-only exposure).

Requirement: every resource (`/mcp`, `/auth`, `/ui*`, `/health`, `/metrics`)
available under the prefix, with one image that also still serves the root.

### Considered options

1. **Build-time prefix (rejected).** The memory_mcp failure mode: prefix must
   match in build and runtime; CI-built artifacts drift. Impossible here only
   by accident (no bundle), and undesirable by design.
2. **Strip-prefix proxy + relative URLs (rejected).** Requires every template
   to encode its depth (regression-prone), cannot express cookie `Path`
   relatively (RFC 6265), and breaks the shallow `/prefix` redirect without
   trailing slash. Also diverges from the memory service's pass-through
   operational model.
3. **Runtime env + `Router::nest` (chosen).** `MCP_PUBLIC_BASE_PATH` is read
   at startup, the app is mounted under the prefix, and `UiState` prefixes
   every emitted URL and the cookie `Path`. Empty value ⇒ byte-identical root
   behavior. One operational pattern for all services on the domain:
   **pass-through, no path rewriting**.

## Consequences

- Proxy contract: forward full paths; never strip the prefix.
- With a prefix set, host-root paths return 404 (root freed for other
  services); container healthchecks must use `{prefix}/health`.
- axum `nest("/x", …)` matches `/x` but not `/x/` — the `/x/` form is
  redirected by an outer fallback (`apply_public_base_path`).
- Switching between root and prefix deployments is a config change, not a
  rebuild.
- Metrics caveat: when a prefix is set, `MatchedPath`-derived `path` labels
  on `/metrics` include the prefix (e.g. `/intervals/health`); label values
  are stable per deployment but differ between root and prefixed configs.
