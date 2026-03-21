use getrandom::fill as fill_random;

fn random_hex(byte_len: usize) -> String {
    let mut bytes = vec![0u8; byte_len];
    fill_random(&mut bytes).expect("failed to generate secure random bytes");
    hex::encode(bytes)
}

fn main() {
    let jwt_secret = random_hex(32);
    let jwt_encryption_key = random_hex(32);

    println!("export JWT_SECRET=\"{}\"", jwt_secret);
    println!("export JWT_ENCRYPTION_KEY=\"{}\"", jwt_encryption_key);
}

#[cfg(test)]
mod tests {
    use super::random_hex;

    #[test]
    fn random_hex_returns_expected_length() {
        let value = random_hex(32);
        assert_eq!(value.len(), 64);
        assert!(value.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn random_hex_is_not_constant() {
        let first = random_hex(32);
        let second = random_hex(32);
        assert_ne!(first, second);
    }
}
