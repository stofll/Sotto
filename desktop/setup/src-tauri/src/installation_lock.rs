use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

// Lock installation work, not the UI: each setup must retain its own payload and arguments.
pub struct InstallationLock {
    _file: File,
}

impl InstallationLock {
    pub fn acquire() -> Result<Self, &'static str> {
        Self::at_path(&std::env::temp_dir().join("sotto-setup-install.lock"))
    }

    fn at_path(path: &Path) -> Result<Self, &'static str> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|_| "prepare_failed")?;
        // Keep the file on disk: unlinking it would let another process lock a different file.
        // Dropping its handle (including on process exit) releases the OS lock.
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(TryLockError::WouldBlock) => Err("installation_running"),
            Err(TryLockError::Error(_)) => Err("prepare_failed"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[test]
    fn child_probe() {
        let Some(path) = std::env::var_os("SOTTO_TEST_INSTALL_LOCK") else {
            return;
        };
        let lock = InstallationLock::at_path(Path::new(&path));
        if std::env::var("SOTTO_TEST_LOCK_EXPECT").unwrap() == "busy" {
            assert!(matches!(lock, Err("installation_running")));
        } else {
            assert!(lock.is_ok());
        }
        // Exit without running destructors to check OS cleanup as well as normal Drop.
        std::process::exit(0);
    }

    fn probe(path: &Path, expected: &str) {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "installation_lock::tests::child_probe",
                "--nocapture",
            ])
            .env("SOTTO_TEST_INSTALL_LOCK", path)
            .env("SOTTO_TEST_LOCK_EXPECT", expected)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "child failed: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn serializes_processes_and_allows_retry_after_exit() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("install.lock");
        let lock = InstallationLock::at_path(&path).unwrap();
        probe(&path, "busy");
        drop(lock);
        probe(&path, "available");
        assert!(InstallationLock::at_path(&path).is_ok());
    }

    #[test]
    fn inaccessible_lock_stops_preparation() {
        let directory = tempfile::tempdir().unwrap();
        let result = InstallationLock::at_path(&directory.path().join("missing/install.lock"));
        assert!(matches!(result, Err("prepare_failed")));
    }
}
