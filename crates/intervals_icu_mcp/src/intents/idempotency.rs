use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use super::types::{IntentError, IntentOutput};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdempotencyEntry {
    pub result: IntentOutput,
    pub request_fingerprint: String,
    pub created_at: DateTime<Local>,
    pub expires_at: DateTime<Local>,
}

impl IdempotencyEntry {
    pub fn new(result: IntentOutput, request_fingerprint: String, ttl: Duration) -> Self {
        let now = Local::now();
        let expires =
            now + chrono::Duration::from_std(ttl).unwrap_or_else(|_| chrono::Duration::days(1));
        Self {
            result,
            request_fingerprint,
            created_at: now,
            expires_at: expires,
        }
    }
    pub fn is_expired(&self) -> bool {
        Local::now() > self.expires_at
    }
}

#[derive(Debug, Clone, Default)]
pub struct IdempotencyStats {
    pub hits: u64,
    pub misses: u64,
    pub sets: u64,
    pub evictions: u64,
    pub expired_count: u64,
}

#[derive(Clone)]
pub struct IdempotencyMiddleware {
    cache: Arc<RwLock<CacheInner>>,
    default_ttl: Duration,
}

struct CacheInner {
    entries: HashMap<String, IdempotencyEntry>,
    stats: IdempotencyStats,
}

impl IdempotencyMiddleware {
    pub fn new() -> Self {
        Self::with_ttl(Duration::from_secs(86400))
    }
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            cache: Arc::new(RwLock::new(CacheInner {
                entries: HashMap::new(),
                stats: IdempotencyStats::default(),
            })),
            default_ttl: ttl,
        }
    }
    pub async fn execute_with_idempotency<F, Fut>(
        &self,
        token: &str,
        request_fingerprint: &str,
        action: F,
    ) -> Result<IntentOutput, IntentError>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<IntentOutput, IntentError>>,
    {
        if let Some(entry) = self.get(token, request_fingerprint).await? {
            let mut cache = self.cache.write().await;
            cache.stats.hits += 1;
            return Ok(entry);
        }
        let result = action().await?;
        self.set(token, request_fingerprint, &result).await;
        {
            let mut cache = self.cache.write().await;
            cache.stats.misses += 1;
            cache.stats.sets += 1;
        }
        Ok(result)
    }
    pub async fn get(
        &self,
        token: &str,
        request_fingerprint: &str,
    ) -> Result<Option<IntentOutput>, IntentError> {
        let mut cache = self.cache.write().await;
        if let Some(entry) = cache.entries.get(token) {
            if entry.is_expired() {
                cache.entries.remove(token);
                cache.stats.expired_count += 1;
                return Ok(None);
            }
            if entry.request_fingerprint != request_fingerprint {
                return Err(IntentError::IdempotencyConflict(format!(
                    "token '{}' was already used for a different request; generate a new idempotency token",
                    token
                )));
            }
            return Ok(Some(entry.result.clone()));
        }
        Ok(None)
    }
    pub async fn set(&self, token: &str, request_fingerprint: &str, result: &IntentOutput) {
        let mut cache = self.cache.write().await;
        cache.entries.insert(
            token.to_string(),
            IdempotencyEntry::new(
                result.clone(),
                request_fingerprint.to_string(),
                self.default_ttl,
            ),
        );
    }
    pub async fn get_stats(&self) -> IdempotencyStats {
        let cache = self.cache.read().await;
        cache.stats.clone()
    }
}

impl Default for IdempotencyMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

pub fn generate_idempotency_token(intent: &str, params: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(intent.as_bytes());
    for p in params {
        hasher.update(p.as_bytes());
    }
    format!("{}_{}", intent, hex::encode(&hasher.finalize()[..8]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn test_cache() {
        let m = IdempotencyMiddleware::new();
        let out = IntentOutput::markdown("test");
        m.set("t", "fingerprint-a", &out).await;
        assert!(m.get("t", "fingerprint-a").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_rejects_different_fingerprint_for_same_token() {
        let m = IdempotencyMiddleware::new();
        let out = IntentOutput::markdown("test");
        m.set("t", "fingerprint-a", &out).await;

        let err = m.get("t", "fingerprint-b").await.unwrap_err();
        assert!(matches!(err, IntentError::IdempotencyConflict(_)));
    }
    #[test]
    fn test_token_gen() {
        let t1 = generate_idempotency_token("test", &["a"]);
        let t2 = generate_idempotency_token("test", &["a"]);
        assert_eq!(t1, t2);
    }
}
