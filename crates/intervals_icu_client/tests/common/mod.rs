//! Shared test helpers for the integration test suite.
//!
//! The helpers here reduce boilerplate around three repeating concerns:
//!
//! 1. **Constructing a real `ReqwestIntervalsClient`** pointed at a
//!    `wiremock` mock server. Every test in `tests/` spins up a
//!    `MockServer` and then constructs the client against it. The
//!    `setup_client` helper folds both steps together.
//!
//! 2. **Spinning up a `MockServer` ready to accept mocks.** `setup_mock`
//!    returns the server so callers can register mocks with
//!    `Mock::given(...).respond_with(...).mount(&server)`.
//!
//! 3. **Asserting on `NaiveDate`-based period windows.** Several tests
//!    work with periods like `(2025-04-01, 2025-06-30)` and need to verify
//!    that the request body or query string encodes the expected window.
//!    `assert_period_window` is the single point of truth for that check.
//!
//! These helpers are intentionally minimal — they do not own any state of
//! their own, and they never panic with custom messages. If a future test
//! needs additional setup, prefer extending an existing helper over
//! introducing a new one.

#![allow(dead_code)] // Some helpers are only used by a subset of integration tests.

use chrono::NaiveDate;
use intervals_icu_client::http_client::ReqwestIntervalsClient;
use secrecy::SecretString;
use wiremock::MockServer;

/// Construct a `ReqwestIntervalsClient` pointed at the given mock server.
///
/// The athlete id is the conventional `"ath"` and the API key is a
/// non-secret placeholder string. Tests do not exercise auth, only
/// request/response shape, so a fixed value is sufficient.
pub fn setup_client(server: &MockServer) -> ReqwestIntervalsClient {
    ReqwestIntervalsClient::new(
        &server.uri(),
        "ath",
        SecretString::new("tok".to_string().into_boxed_str()),
    )
    .expect("client construction must succeed for a valid base URL")
}

/// Start a `MockServer` and return it ready for `Mock::given(...).mount(&server)`.
///
/// This is a thin wrapper around `MockServer::start()` that exists so tests
/// can write a single line and signal intent. It is currently equivalent to
/// the upstream call; the wrapper exists as a forward-compat seam in case
/// common setup logic (logger init, panic hook, etc.) needs to be added.
pub async fn setup_mock() -> MockServer {
    MockServer::start().await
}

/// Assert that a `(start, end)` pair represents a valid period window.
///
/// A window is valid when `start <= end` and both endpoints parse to
/// `NaiveDate`. Returns the parsed pair on success so callers can chain
/// further checks without re-parsing.
pub fn assert_period_window(start: &str, end: &str) -> (NaiveDate, NaiveDate) {
    let start_date = NaiveDate::parse_from_str(start, "%Y-%m-%d")
        .unwrap_or_else(|_| panic!("start date must be YYYY-MM-DD, got {start:?}"));
    let end_date = NaiveDate::parse_from_str(end, "%Y-%m-%d")
        .unwrap_or_else(|_| panic!("end date must be YYYY-MM-DD, got {end:?}"));
    assert!(
        start_date <= end_date,
        "period window must satisfy start <= end, got {start_date} > {end_date}",
    );
    (start_date, end_date)
}

/// Build a `SecretString` API key suitable for `ReqwestIntervalsClient::new`.
///
/// Public so tests that need to construct a client by hand (e.g., to inject
/// a custom circuit breaker) can still produce a syntactically-valid key
/// without re-typing the boxed-string dance.
pub fn fake_api_key() -> SecretString {
    SecretString::new("test-token".to_string().into_boxed_str())
}
