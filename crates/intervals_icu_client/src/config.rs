use crate::IntervalsError;
use secrecy::SecretString;

#[derive(Clone, Debug)]
pub struct Config {
    pub api_key: SecretString,
    pub athlete_id: String,
    pub base_url: String,
}

impl Config {
    pub fn from_env() -> Result<Self, IntervalsError> {
        Self::from_env_with(|k| std::env::var(k).ok())
    }

    /// Testable helper that reads configuration values using the provided
    /// function. This avoids mutating global environment in tests and keeps
    /// `from_env()` small and safe.
    pub fn from_env_with<F>(mut get: F) -> Result<Self, IntervalsError>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let api = get("INTERVALS_ICU_API_KEY")
            .ok_or_else(|| IntervalsError::Config("INTERVALS_ICU_API_KEY missing".into()))?;
        let athlete_id = get("INTERVALS_ICU_ATHLETE_ID")
            .ok_or_else(|| IntervalsError::Config("INTERVALS_ICU_ATHLETE_ID missing".into()))?;
        let base_url =
            get("INTERVALS_ICU_BASE_URL").unwrap_or_else(|| "https://intervals.icu".into());
        Ok(Self {
            api_key: SecretString::new(api.into()),
            athlete_id,
            base_url,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_env_missing_api_key() {
        let get = |k: &str| match k {
            "INTERVALS_ICU_API_KEY" => None,
            "INTERVALS_ICU_ATHLETE_ID" => Some("42".into()),
            "INTERVALS_ICU_BASE_URL" => Some("http://localhost".into()),
            _ => None,
        };
        let res = Config::from_env_with(get);
        assert!(res.is_err());
    }

    #[test]
    fn from_env_reads_values() {
        let get = |k: &str| match k {
            "INTERVALS_ICU_API_KEY" => Some("sekrit".into()),
            "INTERVALS_ICU_ATHLETE_ID" => Some("42".into()),
            "INTERVALS_ICU_BASE_URL" => Some("http://localhost".into()),
            _ => None,
        };
        let cfg = Config::from_env_with(get).expect("cfg");
        assert_eq!(cfg.athlete_id, "42");
        assert_eq!(cfg.base_url, "http://localhost");
    }
}
