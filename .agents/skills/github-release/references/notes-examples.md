# Release-note examples

The notes are the GitHub release body: English, short bulleted groups, written for someone deciding whether to upgrade. The examples below are drawn from several Rust repos' own release histories, but the register is the same everywhere: a reader who wants to know whether *this* release contains their problem.

## A full body

```markdown
No API, tool-surface or configuration changes — deployments upgrade with no operator action.

**Breaking changes**
- The compaction tool no longer accepts a `mode` argument; compaction is automatic. Callers passing `mode` get a validation error.

**Customer-facing changes**
- Configuration flags for the HTTP profile are now honoured by the stdio binary too; they were read and silently ignored before.

**Behaviour changes**
- An invalid value for the rollout stage now fails startup instead of being ignored and leaving the default in place.
- The backfill pass runs at most once every 60 seconds instead of on every one-second scheduler tick.
- `embedding.backfill_started` reports at `debug`, not `info`, when a pass finds nothing to do.

**Fixed issues**
- A tenant whose vectors sat at another provider's width was paid-for and then rejected on every tick; it is now declined before the provider call.
- A model dimension that did not match the configured width resolved at the wrong size, so semantic search silently degraded.

**Other**
- New `http.lifecycle.*` operations in the log when the decay and archival passes fail for a tenant.
```

## Item-level good vs bad

| Bad | Why | Good |
| --- | --- | --- |
| `Refactored the config layer so both profiles share one parser.` | Mechanism, no user-visible effect. | Delete it — internal-only changes get no bullet. |
| `Fixed a bug in the embedding code.` | Names neither the symptom nor the fix; a reader cannot tell if it is their bug. | `Semantic search returned nothing after enabling embeddings on an index built at 1536 dimensions; the index is now re-declared at the configured width.` |
| `Changed logging behaviour.` | Vague; which events, and what changed? | `embedding.backfill_started now logs at debug when there is nothing to backfill, so an idle deployment is silent at info.` |
| `Added lifecycle support.` | Does not say what it does or that it was previously broken. | `LIFECYCLE_ENABLED now runs decay and archival passes over HTTP; the flag was read but had no effect.` |
| `BREAKING: everything about config changed.` | If everything is breaking, say what a caller must do. | `MEMORY_CLAIM_CANDIDATE_PAGE_SIZE must be a positive integer; a non-numeric value is now a startup error.` |
| `Bumped rmcp to 3.x.` | True, but leaves the reader guessing whether anything they call changed. | `Upgraded rmcp to 3.x. The negotiated MCP protocol version is unchanged and existing clients keep working; the new MSRV is 1.99.` |

## Notes on voice

- **Tense.** Present for what the release now does ("the flag is honoured"), past for what was wrong ("it used to be ignored"). Do not narrate a journey.
- **Person.** No "we"/"I". The reader does not care who changed it.
- **Hedges.** Cut "may", "should mostly", "various". State the fact or leave it out.
- **Numbers.** Keep them when they are the point (`at most once every 60 seconds`, `1536`-dimension), because they are the detail an operator acts on.
- **Length.** If a group runs past ~5 bullets, the release is large — that is fine, but each bullet still fits one line in the code block.

## Matching the repo's own changelog

If the repo keeps a `CHANGELOG.md`, read its most recent section before writing the notes. Match its group headings where you can — a reader who has seen the changelog should not have to learn a second vocabulary from the release page. The two artifacts serve different audiences (changelog for archaeology, release body for the upgrade decision) but should not contradict each other.

If the previous release's notes are still visible (`gh release view <prev-tag>`), match their shape too. Consistency across releases is itself a signal that the release was done carefully.