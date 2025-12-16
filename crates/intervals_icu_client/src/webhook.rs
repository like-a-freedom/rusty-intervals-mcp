use secrecy::SecretString;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::collections::HashSet;
use tokio::sync::RwLock;

type HmacSha256 = Hmac<Sha256>;

pub fn verify_hmac(secret: &SecretString, payload: &[u8], signature_header: &str) -> bool {
    // signature_header expected like: "sha256=..."
    let parts: Vec<&str> = signature_header.split('=').collect();
    if parts.len() != 2 { return false; }
    let sig = match hex::decode(parts[1]) {
        Ok(s) => s,
        Err(_) => return false,
    };
    let mut mac = HmacSha256::new_from_slice(secret.expose_secret().as_bytes()).expect("HMAC can take key of any size");
    mac.update(payload);
    mac.verify_slice(&sig).is_ok()
}

pub struct Deduper { seen: RwLock<HashSet<String>> }

impl Deduper {
    pub fn new() -> Self { Self { seen: RwLock::new(HashSet::new()) } }
    pub async fn is_duplicate(&self, uid: &str) -> bool {
        let mut lock = self.seen.write().await;
        if lock.contains(uid) { true } else { lock.insert(uid.to_string()); false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::SecretString;

    #[test]
    fn hmac_verification_works() {
        let secret = SecretString::new("sekret".into());
        let payload = b"hello";
        let mut mac = HmacSha256::new_from_slice(secret.expose_secret().as_bytes()).unwrap();
        mac.update(payload);
        let sig = mac.finalize().into_bytes();
        let header = format!("sha256={}", hex::encode(sig));
        assert!(verify_hmac(&secret, payload, &header));
    }

    #[tokio::test]
    async fn deduper_detects_duplicates() {
        let d = Deduper::new();
        assert!(!d.is_duplicate("x").await);
        assert!(d.is_duplicate("x").await);
    }
}
