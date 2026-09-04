//! Helpers shared by the crate's unit tests.
//!
//! Only compiled under `cfg(test)`: nothing here ships in the binary.

/// Tests that override the same environment variable serialize on this
/// lock, so they cannot observe each other's in-flight `set_var` calls
/// (cargo runs `--lib` tests in parallel threads by default). One lock
/// for all keys rather than one per key: the set is small, the tests are
/// fast, and a single lock cannot be forgotten when a third key appears.
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
