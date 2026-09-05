//! Helpers shared by the crate's unit tests.
//!
//! Only compiled under `cfg(test)`: nothing here ships in the binary.

/// Tests that override the same environment variable serialize on this
/// lock, so they cannot observe each other's in-flight `set_var` calls
/// (cargo runs `--lib` tests in parallel threads by default). One lock
/// for all keys rather than one per key: the set is small, the tests are
/// fast, and a single lock cannot be forgotten when a third key appears.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Holds the same lock [`EnvGuard`] serializes on, without touching any
/// variable: for tests that only read what the environment-derived paths
/// (`models_dir()`, `resolve_model_path`) answer. Without it such a test can
/// read the variable once before and once after a parallel `EnvGuard` test
/// flips it, and see two different worlds.
/// The transducer load-spec test — the only taker — is compiled where the
/// sherpa transducer is, so the helper is too; elsewhere it would be dead
/// code under `-D warnings`.
#[cfg(any(windows, target_os = "macos"))]
pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

/// Sets or removes an environment variable for the duration of a test and
/// restores the previous value on drop (removing the variable again if it
/// was unset before).
pub struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
    /// Held for the lifetime of the guard — released on drop.
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    pub fn set(key: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var(key).ok();
        std::env::set_var(key, value);
        Self {
            key,
            prev,
            _lock: lock,
        }
    }

    pub fn remove(key: &'static str) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var(key).ok();
        std::env::remove_var(key);
        Self {
            key,
            prev,
            _lock: lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match &self.prev {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}
