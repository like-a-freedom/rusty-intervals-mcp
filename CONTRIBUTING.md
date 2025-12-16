# Contributing

Thanks for helping improve Rusty-Intervals. This document explains how to run
local checks, tests, and how releases are produced.

## Local checks (pre-commit)

- Format: `cargo fmt`
- Lint: `cargo clippy --all --all-targets -- -D warnings`
- Tests: `cargo test`

We try to keep changes small, obvious and well-tested. Please add unit tests
for new functionality and run the full suite before opening PRs.

## Development

- Run a single crate tests: `cargo test -p intervals_icu_client` or
  `cargo test -p intervals_icu_mcp --test e2e_http` for integration tests.
- Build release binary: `cargo build -p intervals_icu_mcp --release`

## Releases & Binaries

Releases are published using GitHub Actions and produce pre-built binaries for
Linux (x86_64/aarch64), macOS (x86_64/aarch64) and Windows (x86_64).

When preparing a release manually:
1. Ensure all checks pass locally (format, clippy, tests).
2. Create a GitHub release (tag `vX.Y.Z`) — the workflow will produce and
   attach binary artifacts and checksums.

Note on container publishing

- The release workflow builds and pushes multi-architecture images to **GHCR** by
  default (using `GITHUB_TOKEN`). The image path is
  `ghcr.io/<your-org-or-user>/rusty-intervals-mcp` and tagged with the release
  tag and `latest` as applicable.

Security notes

- Do not store secrets in the repository. Use GitHub repository secrets for
  `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` (or a personal access token with
  the appropriate scope).
- GHCR publishing uses `GITHUB_TOKEN` and requires `packages: write` permission
  which the workflow already requests.

## Code style

- Keep changes focused and small.
- Use idiomatic Rust (prefer `?` operator, avoid `unwrap()` in non-tests).
- Add documentation comments for public types/functions.
- Run `cargo fmt` and `cargo clippy` before pushing.

---

If you have questions about project structure or design choices, prefer opening
an issue or discussion before a large design change.