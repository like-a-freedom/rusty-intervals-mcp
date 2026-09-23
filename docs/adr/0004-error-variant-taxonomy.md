# ADR-0004 — Error Variant Taxonomy

| Field | Value |
|---|---|
| **Status** | Implemented |
| **Date** | 2026-07-27 |
| **Deciders** | Architecture audit |

## Context

`IntervalsError::Config(ConfigError::Other(...))` is abused at 13 sites as a catch-all for errors that are not configuration failures:
- `io::Error` from file operations (`download_file`, `sync_all`)
- Cancellation signals from `cancel_rx` watch channel
- JSON decode failures in event/response parsing
- Unimplemented method stubs on the `IntervalsClient` trait

Callers cannot distinguish "file could not be saved" from "URL is misconfigured" — both are `ConfigError::Other`. This violates the principle that error types should enable callers to make decisions about recovery.

**Current abuse sites** (all in `http_client.rs` unless noted):
| Line | Actual error | Wrapped as | Semantic category |
|------|-------------|------------|-------------------|
| 57 | URL construction | `ConfigError::Other` | Config (correct) |
| 302 | `tokio::fs::File::create` → `io::Error` | `ConfigError::Other` | **I/O** |
| 306 | `file.write_all` → `io::Error` | `ConfigError::Other` | **I/O** |
| 311 | `file.sync_all` → `io::Error` | `ConfigError::Other` | **I/O** |
| 496 | URL construction | `ConfigError::Other` | Config (correct) |
| 839,858,868,876,894 | Download I/O errors | `ConfigError::Other` | **I/O** |
| 847 | `cancel_rx` → cancellation | `ConfigError::Other` | **Cancelled** |
| 1008 | `serde_json` decode failure | `ConfigError::Other` | **Decode** (already `JsonDecode` variant, but this site bypasses it) |
| 1330 | URL construction | `ConfigError::Other` | Config (correct) |
| `lib.rs:152,232,306,314,319,324,333,342` | Unimplemented method | `ConfigError::Other` | **Unimplemented** |

## Decision

**Add three new `IntervalsError` variants** and remap all misclassified sites to their semantically correct variant.

### New variants

```rust
pub enum IntervalsError {
    // ... existing variants ...

    /// I/O error during file operations (download, save, sync).
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Operation was cancelled by an external signal.
    #[error("operation cancelled: {0}")]
    Cancelled { reason: String },

    /// Response body could not be decoded.
    #[error("decode error: {message}")]
    Decode { message: String, snippet: String },
}
```

### Remap table

| Current site | New variant | Rationale |
|-------------|-------------|-----------|
| `http_client.rs:302,306,311,839,858,868,876,894` | `IntervalsError::Io(io::Error)` | File operations are I/O, not config |
| `http_client.rs:847` | `IntervalsError::Cancelled { reason }` | Cancellation is control flow, not an error |
| `http_client.rs:1008` | `IntervalsError::Decode { message, snippet }` | Decode failure retains body snippet for diagnostics |
| `lib.rs:8 default methods` | `IntervalsError::Config(ConfigError::Unsupported { method })` | Unimplemented is a config/API surface concern, but use a typed `Unsupported` variant, not `Other` |
| `http_client.rs:57,496,1330` | Keep `ConfigError::Other` | These ARE genuinely config errors (URL construction/malformation) |

### `ConfigError` enhancement

Add `ConfigError::Unsupported { method: &'static str }` to replace the 8 `ConfigError::Other("... not implemented")` stubs with a typed, matchable variant.

### What stays in `ConfigError::Other`

Only errors where the caller genuinely cannot distinguish the cause programmatically — URL construction from raw user input, configuration file parse failures with opaque messages. These are truly "other" config errors.

## Consequences

- **Positive:** Callers can now write `match err { IntervalsError::Io(e) => cleanup_temp_file(), IntervalsError::Cancelled { .. } => return Ok(()), ... }` — actionable recovery logic.
- **Positive:** `io::Error` is a standard library type — no new dependency. `#[from]` on `IntervalsError::Io` means `?` propagation works naturally.
- **Positive:** `IntervalsError::Cancelled` is semantically distinct from errors — it signals "stop, don't retry, don't log as ERROR".
- **Negative:** Three new enum variants expand the match surface. Mitigated by the fact that existing `_ => {}` catch-alls absorb new variants at call sites that don't need to distinguish them.
- **Negative:** `Decode` partially overlaps with `JsonDecode(serde_json::Error)`. The distinction is intentional: `JsonDecode` is for structured JSON parse failures; `Decode` is for application-level decode failures where the JSON is valid but doesn't match the expected schema, and the body snippet is diagnostically useful.

## Verification

- `cargo test --all-targets --all-features` must pass.
- All 13 sites remapped to correct variant.
- `ConfigError::Unsupported` added; 8 default method stubs updated.
- `ConfigError::Other` usage limited to 3 sites (genuine config errors).
