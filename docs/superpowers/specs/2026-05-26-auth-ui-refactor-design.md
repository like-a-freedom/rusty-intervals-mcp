# `auth_ui.rs` maintainability refactor design

## Context

`crates/intervals_icu_mcp/src/auth_ui.rs` currently combines several concerns in one module:

- HTTP handlers and redirects
- session and CSRF state management
- token registry persistence and revocation
- input normalization and TTL derivation
- Maud view rendering

The file is functional and has basic route-level coverage in `crates/intervals_icu_mcp/tests/auth_ui_test.rs`, but most behavior is only testable through HTTP integration tests. Internal rules are duplicated across handlers and are not isolated behind small, intention-revealing units.

## Goals

- Improve maintainability with smaller, focused units
- Improve testability of non-HTTP logic with inline unit tests
- Preserve existing routes, HTML flow, and externally visible behavior
- Apply KISS/DRY/YAGNI: refactor only where duplication or mixed responsibility creates friction

## Non-goals

- No route or wire-format changes
- No redesign of the web UI
- No broad auth subsystem rewrite
- No speculative repository/service abstraction intended for future reuse only

## Options considered

### Option A — Small helper functions only

Extract a handful of free functions for duplicated logic.

**Pros**
- Minimal diff
- Low risk

**Cons**
- Responsibility boundaries stay blurry
- State-related behavior remains scattered
- Test seams improve only slightly

### Option B — Small internal helper types inside `auth_ui.rs` (recommended)

Keep one module, but extract focused internal types and methods for session access, token registry access, normalized form data, and token TTL presentation. Keep route handlers as orchestration only.

**Pros**
- Best balance of clarity and diff size
- Improves naming and testability without over-abstracting
- Avoids file explosion for a modest feature area

**Cons**
- File remains a single module, though better organized

### Option C — Split into multiple files/modules plus trait-driven services

Create submodules for handlers, views, persistence, and auth validation.

**Pros**
- Stronger separation
- Highest theoretical testability

**Cons**
- Too much ceremony for current scope
- Higher churn and more review overhead
- Risks violating YAGNI for a compact UI surface

## Recommended design

Adopt **Option B**.

### Proposed internal boundaries

1. **Session helpers**
   - Create/update session from headers
   - Validate CSRF tokens
   - Remember last athlete ID for token listing
   - Return current CSRF token

2. **Token registry helpers**
   - Record newly issued token metadata
   - Revoke token by JTI
   - Return tokens for current athlete
   - Persist registry to disk

3. **Pure request/value helpers**
   - Normalize alternate form fields (`athlete_id`/`email`, `api_key`/`password`)
   - Clamp TTL and produce derived values (`seconds`, display label)
   - Keep formatting helpers pure

4. **HTTP handlers**
   - Reduce each handler to orchestration:
     - establish session
     - validate input / CSRF
     - call auth/JWT logic
     - delegate state updates
     - render response

### Testing strategy

Add inline unit tests in `auth_ui.rs` for:

- form normalization
- TTL clamping / labeling
- token sorting
- session lookup / CSRF validation behavior
- token registry filtering and revocation behavior

Keep existing integration tests in `crates/intervals_icu_mcp/tests/auth_ui_test.rs` for route-level smoke coverage.

## Scope check

This is a single implementation-sized refactor. It is intentionally internal and should not require changes outside `auth_ui.rs` except for closely related documentation or tests if necessary.
