use once_cell::sync::Lazy;
use std::collections::HashMap;
use tokio::sync::{Mutex, MutexGuard};

static ENV_MUTEX: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub(crate) const DYNAMIC_RUNTIME_ENV_VARS: &[&str] = &[
    "INTERVALS_ICU_BASE_URL",
    "INTERVALS_ICU_ATHLETE_ID",
    "INTERVALS_ICU_API_KEY",
    "INTERVALS_ICU_OPENAPI_SPEC",
    "INTERVALS_ICU_SPEC_REFRESH_SECS",
];

pub(crate) struct EnvVarGuard {
    _guard: MutexGuard<'static, ()>,
    saved: HashMap<&'static str, Option<String>>,
}

impl EnvVarGuard {
    pub(crate) fn acquire_blocking(keys: &'static [&'static str]) -> Self {
        let guard = ENV_MUTEX.blocking_lock();
        let saved = snapshot_env(keys);
        Self {
            _guard: guard,
            saved,
        }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => {
                    #[allow(unsafe_code)]
                    unsafe {
                        std::env::set_var(key, value);
                    }
                }
                None => {
                    #[allow(unsafe_code)]
                    unsafe {
                        std::env::remove_var(key);
                    }
                }
            }
        }
    }
}

fn snapshot_env(keys: &'static [&'static str]) -> HashMap<&'static str, Option<String>> {
    keys.iter()
        .copied()
        .map(|key| (key, std::env::var(key).ok()))
        .collect()
}
