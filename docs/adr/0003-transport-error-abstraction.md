# ADR-0003 — Transport Error Abstraction

| Field | Value |
|---|---|
| **Status** | Implemented |
| **Date** | 2026-07-27 |
| **Deciders** | Architecture audit |

## Context

`IntervalsError::Http(#[from] reqwest::Error)` (`error.rs:105`) leaks the `reqwest` crate into the public error surface. Every consumer must compile-link `reqwest::Error` to match on `IntervalsError`. The `#[from]` derive means any `reqwest::Error` auto-wraps without explicit mapping, coupling the domain error type to a specific HTTP transport.

**Current state:**
```rust
#[derive(Debug, Error)]
pub enum IntervalsError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),  // ← transport impl detail in public API
    // ...
}
```

**Problem scope:**
- `cargo build -p intervals_icu_mcp` transitively drags `reqwest` into symbol tables purely for error matching.
- Any future transport swap (hyper, ureq, curl) would break the public API.
- Test assertions that `matches!(err, IntervalsError::Http(_))` are implicitly coupled to reqwest's existence.
- The `#[from]` blanket-implemented auto-wrapping hides the exact site of the transport-to-domain error conversion.

**Non-goal:** This ADR does NOT propose replacing `reqwest` — it only proposes wrapping its error type behind a domain-owned struct.

## Decision

**Replace `IntervalsError::Http(#[from] reqwest::Error)` with `IntervalsError::Transport(TransportError)`** where `TransportError` is a domain-owned struct.

### New type

```rust
/// Transport-level error independent of the HTTP client library.
///
/// Constructed from the implementation-specific error at the `http_client` boundary.
#[derive(Debug, Clone)]
pub struct TransportError {
    /// Human-readable message for logging/diagnostics.
    pub message: String,
    /// True if the error was caused by a connection timeout.
    pub is_timeout: bool,
    /// True if the error was caused by a connection-refused or DNS failure.
    pub is_connect: bool,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for TransportError {}

impl IntervalsError {
    pub fn is_timeout(&self) -> bool {
        matches!(self, IntervalsError::Transport(t) if t.is_timeout)
    }
}
```

### Mapping from `reqwest::Error` (single conversion site)

```rust
// In http_client.rs only — the sole boundary that knows about reqwest
impl From<reqwest::Error> for TransportError {
    fn from(e: reqwest::Error) -> Self {
        TransportError {
            message: e.to_string(),
            is_timeout: e.is_timeout(),
            is_connect: e.is_connect(),
        }
    }
}

impl From<TransportError> for IntervalsError {
    fn from(e: TransportError) -> Self {
        IntervalsError::Transport(e)
    }
}
```

**Removed:**
- `#[from] reqwest::Error` on `IntervalsError::Http`
- All `impl From<reqwest::Error> for IntervalsError` (derived by `#[from]` macro)

**Added:**
- `IntervalsError::Transport(TransportError)` (non-`#[from]` — manual conversion at boundary)
- `impl From<reqwest::Error> for TransportError` in `http_client.rs` (single, documented conversion site)

### Migration

All existing sites that match `IntervalsError::Http(e)` → replace with `IntervalsError::Transport(e)`. The `e.to_string()` output is unchanged — the `Display` impl of `TransportError` delegates to the same message string.

Existing `?` propagation through `IntervalsError` continues to work: call sites use `map_err(TransportError::from).map_err(IntervalsError::from)` or a helper `fn transport_err(e: reqwest::Error) -> IntervalsError`.

## Consequences

- **Positive:** `reqwest` is no longer a transitive public dependency of the error type. Consumers match on `IntervalsError::Transport` without knowing the HTTP library.
- **Positive:** Single conversion site (`http_client.rs`) — all `reqwest::Error` → `IntervalsError` mapping is centralized and auditable.
- **Positive:** `is_timeout()` and `is_connect()` become first-class `IntervalsError` methods, enabling circuit-breaker and retry logic without matching on `reqwest::Error`.
- **Negative:** Minor boilerplate at the single conversion site (`map_err(TransportError::from)?` instead of `?`). Acceptable — one trade-off for transport independence.
- **Neutral:** Existing `IntervalsError::Http` match arms in MCP crate (if any) need to become `IntervalsError::Transport`. Verified: engine layer does NOT match on `IntervalsError::Http` directly, so migration is compile-safe.

## Verification

- `cargo test --all-targets --all-features` must pass.
- `cargo clippy --all-targets --all-features -- -D warnings` must pass.
- No `reqwest::Error` symbol in public API surface of `intervals_icu_client` (verified by `cargo doc --no-deps -p intervals_icu_client` — `IntervalsError` docs must not reference `reqwest::Error`).
