---
name: mcp-design
description: Design, review, and fix MCP servers and their tools — names, descriptions, argument schemas, response envelopes, guidance strings, write safety, latency budgets, and the conformance tests that keep them honest. Use this skill whenever you add an MCP tool, rename one, write or shorten a tool description, add a guidance or error message, review a server surface, or debug an agent that picks the wrong tool, loops, or gives up after an error — even when the request only says "make the tools better", "the agent misuses our API", or "add a tool for X".
metadata:
  version: "2.0"
  author: "mcp-design-skill"
  tags: ["mcp", "ai-agents", "tool-design", "intent-driven", "conformance"]
---

# MCP server design

An MCP server is a user interface for an agent. The agent has no docs, no tabs, no error dialogs, and it pays for every token of your interface on every turn. Design for the decisions the agent has to make, not for the API you happen to have.

## Doctrine

1. **Outcomes, not operations.** Expose what the agent must accomplish, not what the backend can do.
2. **Orchestration belongs in code.** Three calls to reach one answer is a design smell; merge them.
3. **Every string is a prompt.** Names, descriptions, argument docs, guidance, error text.
4. **A response is the next instruction.** Say what the result lets the agent do next.
5. **Never promise what the implementation cannot keep.** A field that is always null, a tool reference to something unregistered, or "there is no write tool" all become lies at the next release, and the agent plans on them.
6. **Curate.** Five tools that fit in one screen beat fifty that fit in none.

---

## Where each fact belongs

MCP clients load the interface in layers. Put a fact at the earliest layer that applies to every call, and never repeat it lower down.

| Layer | Cost | Holds |
|---|---|---|
| Server `instructions` | once per session | shared vocabulary (common filters, auth, response envelope), the cross-tool map: which tool for status, which for evidence |
| Tool name | every turn, per tool | which decision this tool answers |
| Tool description | every turn, per tool | when to use it, when not, argument names, return shape, error behavior |
| Input schema | every turn, per tool | types, bounds, enums, defaults, one line per argument |
| Response `guidance` | only when the tool runs | what this specific result permits next |

Two consequences:

- The description is not a second copy of the schema. Argument semantics live in the schema, where a client renders them next to the field. A description that restates every argument costs tokens and drifts out of sync.
- Server `instructions` are the only place for facts that would otherwise be repeated in ten descriptions. If you find the same sentence in three descriptions, it belongs one layer up.

---

## Step 1. Establish the capability layer

Identify the **business intent**, not the API surface.

- What are the 3–5 end-goals a user reaches through this server?
- Can each goal be stated as verb + object? (`diagnose_incident`, `explain_visibility`)
- Is the domain coherent? If the surface needs two personas, it is two servers.

Output: a named list of intents. These are the tool candidates.

## Step 2. Name the tools

The MCP specification asks tool names to be unique **within one server**, 1–128 characters from `[A-Za-z0-9_.-]`. It does not ask for uniqueness across servers, because namespacing is the client's job. Claude Code, for example, exposes a tool as `mcp__<server>__<tool>`.

So:

- **No product prefix in the tool name.** `get_briefing`, not `geo_get_briefing`. A prefix turns into `mcp__geo__geo_get_briefing`, spends the redundancy on every turn, and teaches the model a string it must reproduce on every call. The official `server-filesystem` server names its tools `read_text_file`, `list_allowed_directories`.
- **Name the decision, not the endpoint.** `search_observations` tells the agent which question it answers; `get_observations` reads like a row fetch and invites the wrong filters.
- **One closed verb list.** When several tools could be `get` or `list` or `search`, enumerate the allowed verbs in a constant and enforce it. Mixed synonyms inside one server cost the agent a reasoning step per choice.
- **Never ship a tool named just `get` or `list`.** The noun carries the meaning; a bare verb only works in a server nobody will ever aggregate.
- **A prefix is right in exactly one situation:** the server is consumed through a flat aggregator that cannot namespace, or the host has no namespacing and the agent sees dozens of servers. Naming style measurably affects tool-choice quality, so if you are in that situation, A/B it instead of arguing.

Renaming is a breaking change, not a refactor. It touches descriptions, guidance strings, server instructions, docs, snapshots, tests, and any persisted record that stores the tool name. See `references/session-findings.md` for the idempotency trap.

## Step 3. Write descriptions as prompts

Five sections, in this order. The headings are part of the contract: a conformance test can assert them, and an agent can scan them.

```
<one line: what this returns, in decision terms>

Use this tool when: <the condition, in the user's words>.
Do NOT use this tool when: <the nearby case> — <the registered tool that covers it>.

Arguments:
- `param` (type): <range, format, default, the trap>

Returns: <the fields that drive the next decision, and what a status means>
On error: <what the error means and what to do next, naming a tool>
```

Rules that survive review:

- **Budget the length.** Keep each description under a fixed ceiling and enforce it in a test. In this repo the ceiling is 1,800 characters and the longest tool sits near 1,465. The ceiling exists because the whole tool list is in context on every turn.
- **Reference only registered tools.** Prose that mentions a tool that does not exist sends the agent into a retry loop. Enforce with a test that extracts tool references from every description and checks them against `tools/list`. Allow a wildcard (`get_*`) for families if the test supports it.
- **Every refusal names the alternative.** "Do NOT use this for X" is a dead end unless it says where to go. This repo's test accepts a registered tool or the web UI.
- **Scope negative claims per tool.** "This tool does not write" is safe. "The server has no write tool" is a fact with an expiry date.
- **Say what the arguments mean to the agent, not what the column is called.** If a filter is one combined string value rather than a list, say so, because the model will send an array. If two fields look alike (`cohort` on a filter, `cohort_label` on a card), say which is which in one clause.
- **Name the vocabulary the response actually uses.** If output labels come from a fixed set, list them.

## Step 4. Design the arguments

- **Flat primitives only.** Strings, numbers, booleans, enums. A nested object forces the model to invent a shape.
- **Closed schemas.** `additionalProperties: false`. The repo's test asserts every input schema is flat and closed.
- **Every argument has its own description.** A documented name with an undocumented meaning is a guess the agent will make.
- **Bounds in the schema match the bounds in the code.** The repo's test compares the advertised maximums against the runtime limits; when they diverge, the agent passes a value the server rejects.
- **Enum values and output labels are the same strings.** If the database can produce "no value", normalize it at the edge to the documented label rather than returning an empty string.

## Step 5. Design the response

- **Envelope every result** with a status (`success`, `partial`, `not_found`, `error`), the payload, and a `guidance` string.
- **Guidance is derived from the returned resource, not from the tool.** One shared sentence across all views points at aliases and edits that are not in this response. A tracking read, a source-domain read and a prompt read each need their own next step.
- **Continuation fields appear only when they carry a value.** Omit `has_more` and `next_offset` when the page is complete; omit `evidence_next_offset` when there is no evidence. A present field is a promise, and a null inside it costs the agent a step to reject.
- **No dead fields.** If the storage cannot populate a field, drop it from the surface. This repo shipped `prompt_template` on runs for weeks while the row builder hardcoded `None`; the agent had no way to learn that. When a historical record genuinely lacks data, expose the id you do store and say why the name is absent.
- **Say what a status means for a decision.** `partial` should tell the agent which part is missing and whether a narrower call fixes it.
- **Numbers carry their sample base.** Any metric the agent might quote needs `sample_size`, `execution_count` and `as_of` next to it, plus an explicit signal when a delta is suppressed for insufficient samples. Otherwise the agent reports movement that is noise.

## Step 6. Design writes

- **One write tool per slice, with a mode argument or a `dry_run` flag.** Splitting preview from apply doubles the tool count and lets the two drift.
- **`dry_run` runs the same validation and collision checks as apply.** A preview that ignores collisions reports "will create" for a key that apply will reject. That is worse than no preview, because it moves the failure past the point where the agent could still choose. This repo made the rule explicit in the spec and pinned it with a test that asserts the preview returns the conflict apply would return.
- **Idempotency is an agent-supplied token**, with its format and its requirement ("required unless `dry_run` is true") stated in the schema. Fingerprint the normalized arguments, and decide deliberately whether the token is part of the fingerprint.
- **Do not derive the idempotency key from the public tool name.** Then a rename silently changes every future key and old rows stop matching. Store a stable operation name instead.
- **Replays must be visible.** A retried request returns the original result with a flag, not a second write.

## Step 7. Budget the latency

An MCP call has a deadline, and a deadline is a correctness requirement. This repo's is 30 seconds.

- **Design the query for the widest realistic filter, not the narrowest.** A correlated predicate that is fast for one market went quadratic for a combined slice and pushed a read past the deadline. The fix was an index on the columns the predicate actually binds, in the order it binds them.
- **Exercise composite filter values in tests.** "DE, UK, FR, RU" is one string value, not a list, and it multiplies the rows the planner must touch. A single-market test proves nothing about it.
- **Instrument the slow tool.** Log the duration and the filter that produced it, or you will hear about the timeout from a user.

## Step 8. Stay inside the tool budget

- 5–10 tools: a focused, single-domain server.
- 11–15: acceptable with clear grouping by persona or workflow.
- 16+: split the server or merge tools.

When over budget, in order: merge tools that always appear together in a workflow; move a domain to its own server; unify tools that differ by one parameter behind an enum.

## Step 9. Write the conformance gates

These are the tests that keep the surface honest. They are cheap, they fail loudly, and they stop the slow drift that no reviewer catches. The set that proved useful here:

| Gate | Catches |
|---|---|
| snapshot of the full descriptor surface | any silent change to a name, description or schema |
| names carry no product prefix | namespace duplication creeping back in |
| every description has the five sections | a description written in a hurry |
| every tool reference resolves to a registered tool | stale prose after a rename or a removal |
| every refusal names an alternative | dead-end error paths |
| no description denies the write surface | a claim that expires |
| description length budget | the list outgrowing the context window |
| flat and closed input schemas | arguments the agent has to guess |
| advertised bounds match runtime limits | rejections after a valid-looking call |
| output schemas validate real envelopes | a field the code stopped emitting |
| read-tool points at the registered write tool | a write path the agent never discovers |

Snapshot discipline: any description edit breaks the snapshot. Regenerate deliberately, then read the diff and confirm it contains only your change. A snapshot regenerated without reading is a rubber stamp.

## Step 10. Verify with live calls

Conformance tests check shape. They cannot see a wrong number, an empty label, a dead field, or a timeout. Before calling a tool done, call every tool in the server against a real database with a realistic filter, read the payload the way an agent would, and compare it against the raw table.

The defects this step found in one session are in `references/session-findings.md`. Read it before you declare a surface verified; each entry is a class of bug that schema checks pass.

---

## Anti-patterns

| Anti-pattern | Signal | Fix |
|---|---|---|
| Namespace duplication | `geo_get_briefing`, `mcp__geo__geo_get_briefing` | Drop the product prefix; namespacing is the client's job |
| CRUD mirror | `create_x`, `read_x`, `update_x`, `delete_x` | Merge into goal-oriented composites |
| Nested arguments | `params: dict`, `options: {...}` | Flatten to primitives, close the schema |
| Raw payload returns | The upstream JSON, unfiltered | Decision-ready fields plus `guidance` |
| Silent failure | `{"error": "not found"}` | Name the tool that finds the right id |
| Dead field | Documented, always null | Remove it, or expose what is actually stored |
| Guidance per tool, not per result | The same sentence for every view | Derive it from the returned resource |
| Preview that validates less than apply | `dry_run` reports a create, apply returns a conflict | Run the same checks on both paths |
| Empty string as "none" | `classification: ""` while the docs say `unclassified` | Normalize at the edge |
| Unmeasured read | Works on one market, times out on a combined one | Index for the predicate; test the wide slice |
| Description as documentation | Restates every argument and the API history | Keep the decision, push semantics into the schema |
| Promise without an owner | "The server has no write tool" | Scope the claim to one tool |

---

## Output format

### Server: `<server-name>`

**Domain:** one sentence. **Persona:** who calls it. **Layer:** capability / product / component.

#### Tool: `<tool_name>`

**Intent:** the decision it answers.

**Description (verbatim):** the five-section block.

**Arguments:** a flat JSON schema with a description per property.

**Returns:** one example envelope, including `guidance` and a status that is not `success`.

**Errors:** one example naming the recovery tool.

#### Intent coverage

| Intent | Tool | Calls | Verdict |
|---|---|---|---|
| ... | ... | 1 | single call / multi call / impossible |

---

## Checklist

- [ ] No tool name repeats the server name
- [ ] Verbs come from one closed list and match the code
- [ ] Every description carries the five sections and stays under the budget
- [ ] Every tool reference in prose resolves to a registered tool
- [ ] Every refusal names a registered tool or the UI
- [ ] Every negative claim is scoped to one tool
- [ ] Every argument is documented, typed, bounded, and part of a closed schema
- [ ] Enum values equal the labels in responses
- [ ] No advertised field is unpopulated
- [ ] Continuation fields appear only when they carry a value
- [ ] Guidance is derived from the returned resource
- [ ] `dry_run` runs the same validation as apply
- [ ] Writes require an idempotency token; the key does not embed the tool name
- [ ] The slowest tool is measured against the deadline with the widest realistic filter
- [ ] The snapshot is regenerated and its diff read
- [ ] Every tool has been called live against real data
- [ ] Total tool count is inside the budget and the domain fits one sentence
