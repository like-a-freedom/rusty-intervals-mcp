# Releasing

This guide describes how to cut a release of `rusty-intervals-mcp` (the
`intervals_icu_mcp` crate). Releases are tag-triggered: pushing an annotated
`vX.Y.Z` tag runs the GitHub Actions release workflow, which builds
cross-platform binaries + checksums and publishes multi-arch GHCR images.

`intervals_icu_client` is versioned independently and is only bumped when it
changes (it currently sits at `2.17.0` and has no `rmcp` dependency).

## Version numbering

- **Major** (`v3.0.0`): breaking changes to the intent API or wire format.
- **Minor** (`v2.21.0`): new intents, features, or dependency/MRST migrations
  that are backward compatible for MCP clients. Example: the `rmcp` 2.x → 3.x
  migration landed as `2.21.0` (new MSRV declared, new major dependency, but
  no change to the negotiated protocol version).
- **Patch** (`v2.20.1`): bug fixes with no API change. Example: the `rmcp`
  1.8 → 2.1 migration landed as `2.19.1`.

## Release checklist

1. **Pre-flight.** Ensure the working tree is clean and you are on `master`
   with `origin/master` up to date (or you have explicitly chosen to release
   local-only work).

2. **Quality gates** (all must pass):
   ```sh
   cargo fmt --all -- --check
   cargo clippy --all-targets --all-features -- -D warnings
   cargo test --all-targets --all-features
   ```
   Fix root causes; never mute warnings or delete tests to go green.

3. **CHANGELOG.** Fold the top `## [Unreleased]` section into
   `## [X.Y.Z] - YYYY-MM-DD` (Keep a Changelog-style groups: `Added`,
   `Changed`, `Fixed`, `Breaking changes`). Each entry must state what
   changed and its user-facing impact. For a dependency migration, call out
   the new dependency version, any MSRV change, and whether the wire
   format / protocol version changed for existing clients.

4. **Version bump.** Update `version` in
   `crates/intervals_icu_mcp/Cargo.toml`. Do not touch
   `crates/intervals_icu_client/Cargo.toml` unless that crate actually
   changed in this release. `Cargo.lock` is gitignored in this repo, so no
   lockfile update is committed.

5. **Re-run the quality gates** after the bump (the version string is inert
   for compilation, but the gate is cheap insurance).

6. **Commit** with the `release:` prefix, mirroring the changelog scope:
   ```sh
   git commit -am "release: bump version to X.Y.Z"
   ```

7. **Tag and push** (the tag is what triggers CI):
   ```sh
   git tag -a vX.Y.Z -m "release vX.Y.Z"
   git push origin master
   git push origin vX.Y.Z
   ```

8. **Create the GitHub release** from the pushed tag (UI or `gh`). The CI
   workflow (`on: release`) attaches binary artifacts and checksums for
   Linux, macOS, and Windows, and publishes multi-arch images to GHCR
   (`ghcr.io/<org-or-user>/rusty-intervals-mcp`).

9. **Verify.** After the workflow completes, confirm the release assets are
   attached and the GHCR image with the `vX.Y.Z` tag exists.

## Notes

- `docs/` is gitignored (except `ARCHITECTURE.md` and `METRIC_METHODS.md`).
  ADRs and planning artifacts there are local-only unless explicitly
  force-added — do not expect them in release diffs.
- Never commit secrets. GHCR publishing uses `GITHUB_TOKEN`; any other
  publishing credentials must come from GitHub repository secrets.
- Keep release diffs minimal and task-scoped. Do not fold unrelated refactors
  or doc cleanup into a release commit.
