//! Circuit breaker for upstream Intervals.icu API.
//!
//! Prevents cascading failures by rejecting requests when the upstream
//! is consistently failing. Three states:
//! - `Closed`: normal operation, requests pass through
//! - `Open`: upstream failing, requests rejected immediately
//! - `HalfOpen`: probe state, one request allowed to test recovery

use metrics::gauge;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed = 0,
    Open = 1,
    HalfOpen = 2,
}

struct CircuitBreakerInner {
    state: CircuitState,
    failure_count: u32,
    last_failure_at: Option<Instant>,
}

pub struct CircuitBreaker {
    inner: Mutex<CircuitBreakerInner>,
    failure_threshold: u32,
    reset_timeout: Duration,
}

impl std::fmt::Debug for CircuitBreaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.lock().unwrap();
        f.debug_struct("CircuitBreaker")
            .field("state", &inner.state)
            .field("failure_count", &inner.failure_count)
            .field("failure_threshold", &self.failure_threshold)
            .field("reset_timeout", &self.reset_timeout)
            .finish_non_exhaustive()
    }
}

impl CircuitBreaker {
    #[must_use]
    pub fn new(failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            inner: Mutex::new(CircuitBreakerInner {
                state: CircuitState::Closed,
                failure_count: 0,
                last_failure_at: None,
            }),
            failure_threshold,
            reset_timeout,
        }
    }

    /// Returns the current circuit state.
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned.
    pub fn state(&self) -> CircuitState {
        let mut inner = self.inner.lock().unwrap();
        if inner.state == CircuitState::Open
            && let Some(last) = inner.last_failure_at
            && last.elapsed() >= self.reset_timeout
        {
            inner.state = CircuitState::HalfOpen;
            return CircuitState::HalfOpen;
        }
        inner.state
    }

    fn record_state(state: CircuitState) {
        let value = match state {
            CircuitState::Closed => 0.0,
            CircuitState::Open => 1.0,
            CircuitState::HalfOpen => 2.0,
        };
        gauge!("intervals_icu_client_circuit_state").set(value);
    }

    /// Records a successful request.
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned.
    pub fn record_success(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.failure_count = 0;
        inner.state = CircuitState::Closed;
        Self::record_state(CircuitState::Closed);
    }

    /// Records a failed request.
    ///
    /// # Panics
    /// Panics if the internal mutex is poisoned.
    pub fn record_failure(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.failure_count += 1;
        inner.last_failure_at = Some(Instant::now());

        if inner.failure_count >= self.failure_threshold {
            inner.state = CircuitState::Open;
            Self::record_state(CircuitState::Open);
        }
    }

    pub fn allow_request(&self) -> bool {
        match self.state() {
            CircuitState::Open => false,
            CircuitState::Closed | CircuitState::HalfOpen => true,
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(5, Duration::from_secs(30))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn circuit_starts_closed() {
        let cb = CircuitBreaker::default();
        assert_eq!(cb.state(), CircuitState::Closed);
        assert!(cb.allow_request());
    }

    #[test]
    fn circuit_opens_after_threshold_failures() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Closed);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
        assert!(!cb.allow_request());
    }

    #[test]
    fn circuit_resets_on_success() {
        let cb = CircuitBreaker::new(3, Duration::from_secs(60));
        cb.record_failure();
        cb.record_failure();
        cb.record_success();
        assert_eq!(cb.state(), CircuitState::Closed);
    }

    #[test]
    fn circuit_half_open_after_timeout() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(1));
        cb.record_failure();
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);

        thread::sleep(Duration::from_millis(10));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        assert!(cb.allow_request());
    }

    #[test]
    fn circuit_reopens_on_failure_in_half_open() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(1));
        cb.record_failure();
        cb.record_failure();
        thread::sleep(Duration::from_millis(10));
        assert_eq!(cb.state(), CircuitState::HalfOpen);
        cb.record_failure();
        assert_eq!(cb.state(), CircuitState::Open);
    }
}
