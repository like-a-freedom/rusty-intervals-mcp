---
name: github-release
description: Cut a release of this project end to end — mirror CI locally, bump the version, update the changelog, commit, create the annotated vX.Y.Z tag, push the branch and tag, and publish the GitHub Release with brief English bullet notes. Use this skill whenever the user says "release", "cut/publish a release", "bump the version", "tag a version", "ship vX.Y.Z", "make the release/notes", or asks for GitHub release notes for this repo — even if they name only one step, because the steps only make sense as one sequence and a half-done release (tag without release, release without a bump) is a state the repo should never be left in.
metadata:
  version: "1.0"
  tags: ["release", "github", "gh", "semver", "changelog"]
---

# Release this project

One sequence, in order: **preflight → decide the version → bump + changelog → commit → annotated tag → push branch + tag → publish the GitHub Release with brief English bullet notes.** Each step assumes the previous one landed; skipping ahead (e.g. a GitHub Release for a tag that does not exist, or a tag whose manifest version differs) leaves the repo inconsistent, and the release workflow keys off the release event, not the tag push.

## Read the repo's own release docs first

**Before anything else, look for the repo's own instructions and let them win.** Every project encodes its own release conventions, and this skill is the generic fallback for repos that have none written down:

```bash
ls RELEASING.md RELEASE.md CONTRIBUTING.md docs/RELEASING.md 2>/dev/null
grep -rn "release" AGENTS.md CLAUDE.md 2>/dev/null | head
```

If a `RELEASING.md` (or equivalent) exists, it is authoritative — follow its checklist, version-owner path, and commit-message style, and use this skill only to fill gaps. Where this skill and the repo's docs disagree, the repo's docs win.

## Discover the conventions before you rely on them

Do not assume any of the following. Each is repo-specific, and guessing wrong is how a release ships half-formed. Read them off the repo:

| Fact | How to find it |
| --- | --- |
| Which manifest owns the version | `grep -rl '^version' --include=Cargo.toml .`, then check which is the published crate |
| Whether a lockfile is committed | `git ls-files --error-unmatch Cargo.lock 2>&1` — many repos gitignore it |
| The release commit prefix | `git log --oneline --grep='^release' \| head` |
| The release-tag format | `git tag --sort=-v:refname \| head` then `git tag -l --format='%(contents)' <tag>` |
| The release commit identity | `git log -1 --format='%an <%ae>' --grep='^release'` |
| The current branch | `git rev-parse --abbrev-ref HEAD` |
| The CI gate | read `.github/workflows/*.yml` — **the workflow's commands are the gate** |
| Whether a changelog is kept | `ls CHANGELOG.md` and its section style |

## Why the shape is what it is

- **The version has exactly one owner.** In a Cargo workspace that is usually the published crate's own manifest (`crates/<name>/Cargo.toml`), *not* the workspace root — the root may have no `version` at all. Bumping the wrong file is the most common silent mistake here.
- **A tag alone ships nothing.** The release workflow fires on the *release* event in most repos, not the tag push. Tag without a release and no artifacts are ever built.
- **The notes are the release body, and they are for a reader deciding whether to upgrade.** Brief bullets beat prose: what changed for a user, what breaks, what behaves differently, what was fixed.
- **CI is the gate, not a formality.** If the local preflight and CI disagree, the release fails after the tag is already public. Run CI's own commands first.

## GitHub goes through `gh`, git goes through `git`

Every GitHub-facing step uses the `gh` CLI. It already holds the credentials, resolves the repository's slug, and speaks the API natively — so no step hardcodes an org/repo name or re-implements auth, and none reaches for `curl https://api.github.com/…`. `git` stays for the three operations it owns — `commit`, `tag`, `push` — which `gh` cannot perform. Everything else is a `gh` command: `gh repo view --json nameWithOwner -q .nameWithOwner` for the slug, `gh release …` for the release, `gh api …` for anything the porcelain does not cover.

## Preconditions — check these before touching anything

1. On the release branch, and the tree is clean apart from the version bump you are about to make. If `git status --short` shows unrelated edits, stop and ask — releasing uncommitted work is a mistake.
2. `gh auth status` succeeds.
3. The local preflight passes (below). Do not tag a revision whose gate is red.

## Step 0 — mirror CI locally

**Read the CI workflow and run what it runs**, so a green local run predicts a green release. Read `.github/workflows/*.yml` and execute its build/test steps verbatim rather than guessing a conventional command. In a Cargo repo that usually looks like:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all --all-features --no-fail-fast
```

Note the flags CI actually uses (`--all-features` vs an explicit list, `--locked` or not) and match them — a local run with different feature flags is not evidence.

Some CI jobs are not locally runnable at all: coverage upload, cross-platform binary matrices, Docker image builds, and publishing. You cannot mirror those, and you should not pretend to. Do the two things instead:

- Run everything that *is* local — fmt, clippy, tests, any `cargo build` or `xtask` checks the workflow invokes.
- **Say which CI jobs you did not run**, and that they gate the release anyway. "clippy was green" when CI also cross-compiles for Windows and publishes images is not evidence; "I ran the local subset; CI covers the matrix and publishing" is.

## Step 1 — decide the version

Read the current version from the manifest you identified:

```bash
grep -m1 '^version' <owner-manifest>/Cargo.toml
```

Choose the next version from the **public surface**, following semver, and say why in one sentence:

- **major** — a breaking change to a wire-visible contract (tool schemas, HTTP API shapes, on-disk schema) or a documented behaviour users depend on.
- **minor** — new capability that is additive; the previous release's clients keep working.
- **patch** — fixes only, no new surface.

When it is a genuine judgement call (technically breaking but affects nobody in practice), state the call and its reason rather than asking. A common pattern: minor for anything that changes what the server accepts or produces, patch for pure fixes.

**In a workspace, check whether sibling crates share the version.** If they are versioned independently, bump only the crate that changed — bumping every member is unreviewable churn and breaks the release diff.

## Step 2 — collect and categorise the changes

```bash
git describe --tags --abbrev=0            # previous release tag
git log --oneline <prev-tag>..HEAD
git diff <prev-tag>..HEAD --stat
```

Read the commits and the diff — not just the subjects — and sort each change into the note categories below. A commit subject can undersell or mislabel; the diff is the source of truth for what a user will see.

## Step 3 — bump the version, and the lockfile only if the repo tracks one

Edit the owning manifest so `version = "X.Y.Z"` names the new version. Then:

```bash
git ls-files --error-unmatch Cargo.lock 2>&1
```

- **Lockfile is tracked** → move it and confirm the bump is exactly one `version = "..."` line under that package's name:
  ```bash
  cargo update -p <package-name>
  git diff -- Cargo.lock
  ```
  If it moved more than that single line, something else re-resolved — revert the extra churn (`git checkout Cargo.lock`, re-run) rather than shipping unrelated moves.
- **Lockfile is gitignored or absent** (check `.gitignore`) → **do not** stage one and do not add an ignore-bypassing force-add. A correct bump is exactly one line in one manifest. This is the common case and the one most easily broken by muscle memory from a repo that does commit its lockfile.

## Step 4 — update the changelog, if the repo keeps one

If `CHANGELOG.md` exists, add the new section **before** committing the bump, using the file's own heading and group style (Keep a Changelog uses `## [X.Y.Z] - YYYY-MM-DD` with `Added` / `Changed` / `Fixed` / `Breaking changes`). Entries state what changed and its user-facing impact. For a dependency migration, call out the new version, any MSRV change, and whether the wire format or protocol version changed for existing clients.

The changelog entry and the release notes answer the same question for two audiences — keep them consistent rather than writing the release notes twice from scratch.

## Step 5 — commit

Stage only the release files, and commit under the identity and prefix you discovered:

```bash
git add <owner-manifest>/Cargo.toml [Cargo.lock] [CHANGELOG.md]
git commit -m "<discovered release prefix> X.Y.Z"
```

If the repo's release commits land under a project-specific GitHub identity rather than the local `git config`, pass it per-command:

```bash
git -c user.name="<name>" -c user.email="<email>" commit -m "..."
```

Match the existing release history: if its commits are subject-only, write a subject only; if they carry a body, follow suit. Never add co-author or bot-attribution trailers.

If the release itself reverses or changes a standing decision, the reason belongs in the tag message and the release notes — those are what a reader sees.

## Step 6 — annotated tag

Follow the tag format you discovered (`git tag -l --format='%(contents)' <latest-tag>`). The usual shape is `vX.Y.Z — <short title>`, a blank line, then a short paragraph:

```bash
git tag -a vX.Y.Z -m "vX.Y.Z — <short title>

<one short paragraph: what this release is and why it matters>"
```

Annotated (`-a`), not lightweight: an annotated tag carries the message and is what existing releases use (`git cat-file -t v2.26.0` → `tag`). If the repo's history mixes both forms, follow the recent majority.

## Step 7 — push branch and tag

```bash
git push origin <branch>
git push origin vX.Y.Z
```

(`git push origin <branch> --follow-tags` does both once the tag is on `HEAD`.) Then **verify the remote tag actually points at the release commit**, reading it the native way through `gh`:

```bash
slug=$(gh repo view --json nameWithOwner -q .nameWithOwner)
gh api "repos/$slug/git/ref/tags/vX.Y.Z" --jq '.object.type, .object.sha'
```

An annotated tag reports `type = tag` and the sha of the *tag object*, not the commit — dereference it once more, and the result must equal `git rev-parse HEAD`:

```bash
gh api "repos/$slug/git/tags/$(gh api "repos/$slug/git/ref/tags/vX.Y.Z" --jq .object.sha)" --jq .object.sha
```

(`git ls-remote origin 'refs/tags/vX.Y.Z^{}'` is the same lookup in a single call.)

If it does not match, the tag was created on the wrong commit; delete it locally and remotely (`git push origin :refs/tags/vX.Y.Z`), re-tag, and re-push before publishing.

## Step 8 — publish the GitHub Release with brief English notes

Write the notes to a file (format below), then publish:

```bash
gh release create vX.Y.Z \
  --title "vX.Y.Z" \
  --notes-file <path-to-notes.md> \
  --verify-tag
```

`--verify-tag` refuses to create a release for a tag that is not already on the remote — a guard against publishing a release whose tag push failed. If the release workflow triggers on a *different* event than `published` (some repos use `created`), `--verify-tag` plus a check of the workflow's `on:` block covers it either way.

## Step 9 — verify

```bash
gh release view vX.Y.Z
gh run list --limit 3
```

Confirm the tag, title and body are what you intended, then watch the release workflow to completion — the whole point of the release is the artifacts it attaches. If the notes have a mistake, fix them with `gh release edit vX.Y.Z --notes-file <path>`; the release body is editable, the tag is not.

## Release notes format

**Language: English, always** — even when the working conversation is Russian. The release is read by GitHub visitors, not by us.

**Shape: short themed groups of bullets.** The categories below, **omitting any group with nothing in it** (an empty heading is noise):

```markdown
<optional single framing line: the blast radius in one sentence>

**Breaking changes**
- …

**Customer-facing changes**
- …

**Behaviour changes**
- …

**Deprecations**
- …

**Fixed issues**
- …

**Other**
- …
```

Rules that make the notes useful:

- **Brevity is the point.** One line per item; no preamble, no "in this release we…". A reader scans for the item that names their problem.
- **Lead with the blast radius when it is non-trivial.** A single line such as "No API, tool-surface or configuration changes — upgrade with no operator action" tells the reader whether they need to read further. Omit it when the answer is obvious from the bullets.
- **Symptom before mechanism.** For a fix, name what the user saw (and the status/error if it was externally visible), then what changed.
- **A behaviour change and a breaking change are different claims.** A behaviour change is what now happens differently; a breaking change is something a caller must react to. Put each where it belongs; do not file behaviour under Fixed.
- **Be honest about what is unsupported.** A case that is deliberately still not handled is a note, not an omission.
- **Only what ships.** Internal refactors that change nothing a user can observe get no bullet ("Other" is for the valuable ones — an operator-visible metric, a new log field, a documented limit).

The notes equal the release body, so emit them in the reply inside a fenced ```markdown block as well — the clickable record and the text the user can copy should be the same bytes.

See `references/notes-examples.md` for concrete good/bad examples.

## Pitfalls

- **Don't publish the GitHub Release before the tag is pushed.** `--verify-tag` exists to stop this; without the tag on the remote the release has nothing to point at and the workflow fails.
- **Don't `--generate-notes`.** GitHub's generated changelog adds a "## What's Changed" commit/PR list that buries the brief bullets. Write the notes; for a compare link, build it from the slug `gh` gives you: `**Full Changelog**: https://github.com/$(gh repo view --json nameWithOwner -q .nameWithOwner)/compare/<prev>...vX.Y.Z`.
- **Don't tag the branch if the release commit is not `HEAD`.** Tag exactly the release commit (`git rev-parse HEAD` after committing).
- **Don't bump a manifest that doesn't own the version.** In a workspace with independently versioned crates, bumping the root or an unchanged sibling is unreviewable churn.
- **Don't force-add a gitignored `Cargo.lock`.** Check whether the repo tracks it before deciding to stage one.
- **Don't reuse another repo's release commit prefix or tag style.** Read them off `git log` — `chore: release X.Y.Z` and `release: bump version to X.Y.Z` are both common, and mixing them breaks history grep.
- **Don't leave the tree dirty.** After committing, `git status --short` should show nothing but any intentionally-ignored local files.