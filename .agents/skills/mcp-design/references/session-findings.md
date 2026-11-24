# Session findings: what conformance tests missed

Evidence from designing and auditing one production MCP server (10 tools, SQLite/Rust).
Every entry passed the schema and snapshot gates. Each was found by calling the tool with real
data, reading the payload as an agent would, and comparing it with the database.

Use this file when a surface looks verified and you want to know what that claim still leaves open.

---

## 1. A read that is fast on one filter and quadratic on another

**Symptom.** `explain_visibility` normally answered in under a second. With
`market = "DE, UK, FR, RU"` it hit the 30-second MCP deadline.

**Cause.** The query carried a correlated predicate `EXISTS (... gct.market = ?)`. SQLite chose the
`(market, language)` index, so for each observation it scanned every tag sharing that market. One
market is cheap; four multiplied the scan by the number of rows.

**Fix.** An index on `(market, observation_id)`, bound in the order the predicate binds it. The
probe became a lookup.

**Rule.** A compact filter value is not a small query. Test the widest composite value the contract
allows, not the one that fits in a demo. Treat the deadline as a correctness requirement, and log
the duration with the filter that produced it.

---

## 2. An empty string where the documented vocabulary had a word

**Symptom.** The source ranking returned `classification: ""` for citations with no classification.
The tool description, the filter enum and the guidance all said `unclassified`.

**Cause.** The column is nullable, and the row mapper passed `NULL` through to an empty string. The
filter accepted `unclassified`; the output never produced it.

**Fix.** Normalize `NULL` and `""` to `unclassified` at the edge, so the filter vocabulary and the
output labels are one set.

**Rule.** If an enum appears in both a filter and a result, one test should assert that the two lists
are equal. An agent that filters on a value it has never seen in a response will conclude the filter
is broken.

---

## 3. A field that was never populated

**Symptom.** `list_runs` and `get_run` documented `prompt_template`. Every response had it `null`.
The storage held only the template id.

**Cause.** The field was added for a name the frozen lineage deliberately does not store. The row
builder hardcoded `None`. The description was written from the intended shape rather than the actual
one.

**Fix.** Remove the field and expose `prompt_template_id`. A current template name would also
relabel history, which is worse than a missing convenience.

**Rule.** A documented field is a promise. Before shipping, populate it from real rows and read the
JSON. If the storage cannot fill it, drop it. If the absence is meaningful (frozen lineage), say why
in the description instead of returning null.

---

## 4. An evaluation that appended a new copy of every result

**Symptom.** The opportunity list grew on every run. Cards recomputed the same numbers but kept
their old ids, so each run inserted a fresh copy while the older copies kept their original figures.

**Cause.** The engine minted an id per evaluation, and the save path upserted by id. The identity of
a card is its natural key, not its id.

**Fix.** Upsert on `(rule_name, cohort_label)`, add a unique index, and collapse existing duplicates
in a migration before the index is created.

**Rule.** Ask of every write path: "if this runs twice on the same input, what does the second run
do?" If the answer is "inserts again", the identity is wrong. Test the repeat, not just the first
call.

---

## 5. A shared guidance sentence across resources that differ

**Symptom.** Tracking reads, source-domain reads and prompt reads all ended with the same advice:
inspect aliases, then edit them. For a source-domain read or an intent-profile read, neither the
aliases nor the edit target was in the response. Opportunity detail promised an evidence
continuation even when the page was complete.

**Cause.** Guidance was a constant on the tool rather than a function of the returned resource.

**Fix.** Each view guides from its own resource, and a continuation hint appears only when
`evidence_next_offset` exists.

**Rule.** Guidance is part of the payload, not part of the schema. If one sentence covers every
mode of a tool, it is too generic to help. Write one per returned resource and test that the mode
selects the right one.

---

## 6. A preview that validated less than apply

**Symptom.** `manage_tracking` with `dry_run: true` reported "will create" for a key that already
existed. Applying the same call then failed with a conflict.

**Cause.** The design said the preview performs "no lookup", so collision checks ran only on the
applying path.

**Fix.** Run the same validation and collision checks on both paths, and pin it with a test that
asserts the preview returns the conflict apply returns.

**Rule.** A preview exists to let the agent choose before it is committed. A preview that skips a
check the apply performs moves the failure past the point of decision, which is worse than having no
preview at all.

---

## 7. A rename that quietly changed persisted identity

**Symptom.** Tool names carried the product prefix (`geo_manage_tracking`). Dropping it was the
right call for the client-facing surface, and the rename was mechanical: descriptions, guidance,
server instructions, docs, snapshot, tests. The catch was that the idempotency table stored the tool
name, so rows written under the old name no longer matched keys written under the new one.

**Fix.** The rename shipped with the string updated everywhere. The durable fix is to key
idempotency on a stable operation name rather than the public tool name.

**Rule.** Before renaming a tool, list everything that stores the name: idempotency records, audit
rows, metric labels, alerts, saved queries. A rename is a breaking change with a data component; say
so in the release notes.

---

## 8. Applied migrations are immutable, even in a comment

**Symptom.** The prefix rename touched a comment inside an already-applied migration that mentioned
the old tool name.

**Cause and outcome.** `rusqlite_migration` versions by `PRAGMA user_version` and does not checksum
migration bodies, so a comment edit changes nothing at runtime. With a checksum-based runner
(`refinery` with hashes enabled, Flyway, Atlas) the same edit would fail on a live database.

**Rule.** Treat applied migrations as immutable. If a comment is wrong, fix it in a new commit only
after confirming the runner does not verify content. Prefer leaving the stale comment to risking a
deploy.

---

## 9. Descriptions that promised things the surface did not have

**Symptom.** Two classes of stale prose. Descriptions referred to tools that had been renamed, and
the design spec named tools that did not exist yet. One description said the server had no write
tool, which was true until the write tool shipped.

**Fix.** A test that extracts every tool reference from every description and checks it against
`tools/list` (with wildcard support for families), plus a test that no description makes a global
claim about the write surface.

**Rule.** Prose in a tool description is code. Gate it the same way. The cheapest gate is the one
that fails when a name stops existing.

---

## 10. Lookalike arguments with no disambiguation

**Symptom.** A filter argument `cohort` and a card field `cohort_label` were both in play, and a
composite market value was one string (`"DE, UK, FR, RU"`), not a list. The first live calls sent an
array and filtered the wrong field.

**Fix.** One clause per argument saying what it is not. For a composite string: "one value, markets
comma-separated, not an array".

**Rule.** If two fields in the interface share a stem, or one has a type the model would guess wrong,
spend the characters. Ambiguity is paid on every call, not once, so it belongs in the argument
description where the model sees it while constructing the request.
