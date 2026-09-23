# ADR-0008 — Gate `test_support` Behind a Cargo Feature

| Field | Value |
|---|---|
| **Status** | Accepted |
| **Date** | 2026-09-22 |
| **Deciders** | Architecture review (2026-09-22 waves) |

## Context

`crates/intervals_icu_mcp/src/test_support.rs` (~3,898 LOC) was declared
`pub mod test_support;` with no gate. Everything interesting in it — the
`mock::MockIntervalsClient` adapter and the `content_text` assertion helper
(~1,900 LOC) — was part of the crate's production interface and compiled into
every release build (Dockerfile, `cargo build --release`). All 41 in-crate
unit-test uses already sit behind `#[cfg(test)]` internally; the only
integration-test consumer is
`tests/resources_domain_integration.rs` (one import). The workspace had no
`[features]` table anywhere.

### Considered options

1. **Feature gate (chosen).** `[features] test-support = []`; module becomes
   `#[cfg(any(test, feature = "test-support"))]`. Unit tests unaffected
   (`cfg(test)`); integration tests need the feature. CI PR test job gains
   `--all-features` so no coverage is lost.
2. **`required-features` on the one integration test target.** No CI edit,
   but the PR job would *silently skip* that test whenever the feature is
   off — hidden coverage loss. Rejected.
3. **`#[doc(hidden)]`, leave ungated.** Honest, but keeps the mock adapter
   in the release interface. Rejected: the point is interface shrinkage,
   not documentation cosmetics.
4. **Separate `testkit` crate.** Cleanest seam, but the mock is consumed by
   19 files *inside* the crate via `crate::test_support`; an external crate
   cannot serve `cfg(test)` unit tests without awkward path gymnastics.
   Rejected as disproportionate (KISS).

## Decision

**The test interface is gated, not public-by-default.**

- `Cargo.toml` gains `[features] test-support = []`.
- `lib.rs` declares
  `#[cfg(any(test, feature = "test-support"))] pub mod test_support;`.
- CI PR test job runs `cargo test --all --all-features --no-fail-fast`
  (aligned with the documented full-tests command).
- `AGENTS.md` CI-compatible test command gains `--all-features`.

Two adapters remain real across the one seam: the production
`IntervalsClient` impl and the mock — but only builds that ask for the
test adapter get it.

## Consequences

- **Positive:** release interface sheds ~1,900 LOC; Docker/release builds
  no longer compile the mock.
- **Positive:** the seam's purpose is legible from the build: test-only
  surface is explicitly opted into.
- **Negative:** first feature in the workspace sets precedent — future
  features should follow the same `cfg(any(test, feature))` pattern.
- **Negative:** plain `cargo test --all` (no `--all-features`) now fails
  to resolve `intervals_icu_mcp::test_support` in
  `tests/resources_domain_integration.rs`. Mitigated by CI and AGENTS.md
  both specifying `--all-features`.
