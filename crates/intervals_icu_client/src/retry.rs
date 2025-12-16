use rand::{Rng, rng};
use std::time::Duration;

/// A simple retry policy with exponential backoff and jitter.
pub struct RetryPolicy {
    pub max_retries: u32,
    pub base_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            base_delay: Duration::from_millis(100),
        }
    }
}

impl RetryPolicy {
    pub async fn retry_async<F, Fut, T, E>(&self, mut f: F) -> Result<T, E>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, E>>,
    {
        let mut attempt = 0u32;
        loop {
            match f().await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    attempt += 1;
                    if attempt > self.max_retries {
                        return Err(e);
                    }
                    // exponential backoff with jitter
                    let max_delay = self.base_delay * (1u32 << attempt);
                    let mut rng = rng();
                    let jitter = rng.random_range(0..max_delay.as_millis() as u64);
                    let delay = Duration::from_millis(jitter.min(max_delay.as_millis() as u64));
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn retry_succeeds_after_retries() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let policy = RetryPolicy {
            max_retries: 3,
            base_delay: Duration::from_millis(1),
        };
        let calls = Arc::new(AtomicU32::new(0));
        let c = calls.clone();
        let result = policy
            .retry_async(move || {
                let c = c.clone();
                async move {
                    let prev = c.fetch_add(1, Ordering::SeqCst) + 1;
                    if prev < 3 { Err("fail") } else { Ok(42) }
                }
            })
            .await;
        assert_eq!(result.unwrap(), 42);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
}
