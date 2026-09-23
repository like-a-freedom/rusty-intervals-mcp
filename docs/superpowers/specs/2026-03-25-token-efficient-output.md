# Token-Efficient Output Specification (Compact Markdown)

> Date: 2026-03-25
> Updated: 2026-03-25 (revised after peer review)
> Status: Draft
> Scope: Intent handler output formatting across all 8 intent tools

## Problem

Every intent handler currently emits `ContentBlock::Markdown` with verbose formatting tokens (`**`, `- `, `✅`, `⚠️`, excess `\n\n`) that inflate token count without adding structural value for the AI agent. The AI agent uses markdown structure (`#`, `##`) to navigate responses — removing it entirely would degrade comprehension. The goal is **compact markdown**: same structure, fewer tokens.

**Verified waste patterns (from code analysis of 83 `ContentBlock::markdown` call sites):**

| Pattern | Occurrences | Wasted tokens/call | Total waste |
|---------|------------|-------------------|-------------|
| `\n\n` (double newline) | ~120 | 1 token each | ~120 |
| `**bold**` key labels | ~65 | 2 tokens each (`**` × 2) | ~130 |
| `- ` bullet prefix | ~45 | 1 token each | ~45 |
| `✅`/`⚠️` emoji | ~20 | 1-2 tokens each | ~30 |
| `### ` subsection prefix | ~40 | 1 token each | ~40 |

**Estimated total waste per response:** 50-80 tokens across text blocks. With ~200-400 tokens per response in text blocks, this is **15-25% savings on text blocks**, or ~5-10% of total response (tables are unaffected).

### Example: Current Output

```
## Recovery Assessment (01 Jan - 07 Jan)

**Readiness for:** interval

**Red Flags:** None detected ✅

**Recommendation:** Ready for key workout

### Activity-Specific Readiness

- **GO** for interval work.
- HRV trending above baseline.
```

### Example: Compact Markdown Output

```
# Recovery 1-7 Jan
Readiness: interval
Flags: none
Recommendation: Ready for key workout
Activity readiness
  GO for interval work
  HRV trending above baseline
```

**Savings:** ~30% fewer tokens on this block. Structure preserved (`#` = section, indent = list).

## Scope

### In Scope
1. Establish baseline token measurements on 3-5 real intent responses (Task 0)
2. Apply compact markdown rules to all 8 intent handlers
3. Update `render/analysis.rs` shared rendering functions
4. Update `intents/utils.rs` data availability block
5. Keep `ContentBlock::Markdown` variant (no wire format change)
6. Add snapshot test for `analyze_training.rs` before changes (regression guard)

### Out of Scope
- `ContentBlock::Table` — already structured, no formatting overhead
- `ContentBlock::Text` variant — keep as-is for non-markdown text
- Dynamic OpenAPI tools — produce raw JSON, no markdown
- MCP prompt/resource content — consumed differently than tool output
- Removing the `Markdown` variant from the type system (unnecessary breaking change)

## Design Decisions

### DD-1: Keep `ContentBlock::Markdown` Variant

The `Markdown` variant signals to the AI agent that the text block has structural formatting. Removing it would lose this semantic signal and break backward compatibility for zero benefit — the savings come from making the markdown content compact, not from removing the type.

**Decision:** Keep `ContentBlock::Markdown` unchanged. All savings come from compacting the string content inside it.

### DD-2: Compact Markdown Rules

| Element | Current | Compact | Tokens saved |
|---------|---------|---------|-------------|
| Section header (H2) | `## Title` | `# Title` | 1 per header |
| Subsection (H3) | `### Title` | `Title` (plain line) | 2 per subsection |
| Key-value bold | `**Key:**` | `Key:` | 2 per KV pair |
| Bullet list | `- item` | `  item` (2-space indent) | 1 per item |
| Status good | `✅` | (removed) | 1-2 per status |
| Status warning | `⚠️` | (removed) | 1-2 per status |
| Double newline | `\n\n` | `\n` | 1 per break |
| Italic | `*note*` | `note` | 2 per italic |

**Rules:**
- `#` (H1) for top-level section headers (was `##`)
- Plain text (no `###`) for subsections — colon suffix optional: `Activity readiness:`
- `Key: value` for key-value pairs (drop `**...**` wrapping)
- 2-space indent for list items (drop `- ` prefix)
- Single `\n` between sections (drop `\n\n`)
- No emoji — plain text status in tables; if outside a table, just omit

### DD-3: Formatting Helpers in `utils.rs`

Add minimal helpers to `intents/utils.rs` (NOT a separate module — YAGNI):

```rust
/// Compact section header: "# Title"
pub fn compact_section(title: &str) -> String {
    format!("# {title}")
}

/// Compact subsection: "Title" (plain, no ### prefix)
pub fn compact_subsection(title: &str) -> String {
    title.to_string()
}

/// Compact key-value: "Key: value"
pub fn compact_kv(key: &str, value: impl Display) -> String {
    format!("{key}: {value}")
}

/// Compact list item: "  item" (2-space indent)
pub fn compact_item(text: &str) -> String {
    format!("  {text}")
}
```

These are thin wrappers that enforce the convention. If a handler needs a one-off format, inline `format!` is fine — helpers are not mandatory.

### DD-4: Emoji Removal Policy

- In `ContentBlock::Table` "Status" columns: use plain text (`good`, `warning`, `poor`, `n/a`)
- In `ContentBlock::Markdown`: remove all emoji entirely
- No `status_good()`/`status_warning()` string helpers needed — status lives in tables

### DD-5: Backward Compatibility

**No wire format change.** `ContentBlock::Markdown` stays. The `structuredContent` JSON schema is unchanged. The only difference is that the markdown string content is more compact.

### DD-6: Baseline Measurement (New)

Before making changes, measure token counts on 3-5 representative intent responses to establish a baseline. After changes, measure again to validate savings. This provides an empirical success criterion.

**Method:** Run each handler with a mock/fixed input, serialize the `IntentOutput` to JSON, count characters as a proxy for tokens (1 token ≈ 4 chars for English). Compare before/after.

## File Impact Map

| File | Change Type | Description |
|------|------------|-------------|
| `src/intents/utils.rs` | Modify | Add compact formatting helpers, update `data_availability_block` |
| `src/intents/handlers/render/analysis.rs` | Modify | Compact all shared rendering functions |
| `src/intents/handlers/analyze_training.rs` | Modify | Compact markdown in ~26 call sites |
| `src/intents/handlers/analyze_race.rs` | Modify | Compact markdown in ~10 call sites |
| `src/intents/handlers/assess_recovery.rs` | Modify | Compact markdown in ~4 call sites |
| `src/intents/handlers/compare_periods.rs` | Modify | Compact markdown in ~3 call sites |
| `src/intents/handlers/plan_training.rs` | Modify | Compact markdown in ~7 call sites |
| `src/intents/handlers/modify_training.rs` | Modify | Compact markdown in ~5 call sites |
| `src/intents/handlers/manage_gear.rs` | Modify | Compact markdown in ~4 call sites |
| `src/intents/handlers/manage_profile.rs` | Modify | Compact markdown in ~10 call sites |
| `docs/OUTPUT_SCHEMA.md` | Modify | Document compact markdown conventions |
| `docs/SPEC.md` | Modify | Update token efficiency section |

**NOT changed:** `types.rs`, `idempotency.rs`, `ContentBlock` enum, `standard_output_schema()`.

## Token Savings Estimate (Revised)

Based on verified call site analysis:

| Handler | Text block tokens (current) | Estimated compact | Savings |
|---------|---------------------------|-------------------|---------|
| analyze_training | ~350 | ~260 | ~25% |
| assess_recovery | ~250 | ~180 | ~28% |
| analyze_race | ~300 | ~220 | ~27% |
| compare_periods | ~180 | ~140 | ~22% |
| plan_training | ~400 | ~290 | ~28% |
| modify_training | ~200 | ~150 | ~25% |
| manage_gear | ~150 | ~110 | ~27% |
| manage_profile | ~200 | ~150 | ~25% |

**Text block savings:** ~25% average.
**Total response savings (text + tables):** ~8-12% (tables are unaffected but dominate some responses).

Note: These are estimates. Task 0 (baseline measurement) will provide actual numbers.
