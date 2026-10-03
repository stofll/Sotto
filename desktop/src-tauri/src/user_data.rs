//! Current user data locations and cleanup requested by the uninstaller.
//!
//! The database, logs and recordings live in the platform's local data
//! directory under the bundle identifier, unless explicitly overridden.

#[cfg(windows)]
use std::fs;
#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;

/// Must equal `identifier` in `tauri.conf.json`, which names the directories
/// Tauri itself resolves; a test keeps the two in step.
pub const IDENTIFIER: &str = "com.sotto.app";
#[cfg(windows)]
const LEGACY_DIR_NAME: &str = ".speech_to_text";
const DATA_DIR_ENV: &str = "SOTTO_DATA_DIR";

/// Directory overrides renamed after 0.1.3, as (current, pre-rename) names.
/// A shell that still sets an old name keeps pointing a development run at
/// its own directory instead of silently falling back to live user data.
const RENAMED_ENV: [(&str, &str); 3] = [
    (DATA_DIR_ENV, "SOTTO_CONFIG_DIR"),
    ("SOTTO_LOG_DIR", "SPEECH_TO_TEXT_LOG_DIR"),
    ("SOTTO_MODELS_DIR", "SPEECH_TO_TEXT_MODELS_DIR"),
];

/// The value of a `SOTTO_*_DIR` override, or of its pre-rename name when the
/// current one is not set.
pub fn env_override(name: &str) -> Option<String> {
    override_from(name, |key| std::env::var(key).ok())
}

fn override_from(name: &str, get: impl Fn(&str) -> Option<String>) -> Option<String> {
    get(name).or_else(|| {
        RENAMED_ENV
            .iter()
            .find(|(current, _)| *current == name)
            .and_then(|(_, old)| get(old))
    })
}

/// Warn about every pre-rename override still set, naming its replacement.
/// Called once the logger exists.
pub fn log_renamed_env() {
    for (current, old) in RENAMED_ENV {
        if std::env::var_os(old).is_none() {
            continue;
        }
        if std::env::var_os(current).is_some() {
            log::warn!("{old} is ignored because {current} is set; remove {old}");
        } else {
            log::warn!("{old} is deprecated; rename it to {current}");
        }
    }
}

/// The directory holding `sotto.db`, `logs/` and diagnostic recordings.
///
/// Priority: the portable folder, then `SOTTO_DATA_DIR` (used as given, even
/// when empty), then the default location.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = crate::portable::data_dir() {
        return dir;
    }
    if let Some(dir) = env_override(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    default_dir()
}

fn default_dir() -> PathBuf {
    dirs::data_local_dir()
        .expect("local data directory must be available")
        .join(IDENTIFIER)
}

#[cfg(windows)]
fn legacy_dir() -> PathBuf {
    dirs::home_dir()
        .expect("user home directory must be available")
        .join(LEGACY_DIR_NAME)
}

/// Delete everything Sotto stored for the current user in its default
/// locations: the data and configuration directories, the pre-rename data
/// directory, downloaded models and API keys. Run by the Windows uninstaller
/// when the user asks it to delete the application data.
///
/// `SOTTO_*` overrides and the portable folder are deliberately ignored: they
/// can point at directories Sotto does not own. Returns the failures, if any.
#[cfg(windows)]
pub fn purge() -> Vec<String> {
    let mut failures: Vec<String> = purge_targets(
        &dirs::data_local_dir().expect("local data directory must be available"),
        &dirs::config_dir().expect("config directory must be available"),
        &legacy_dir(),
    )
    .into_iter()
    .chain(crate::model::default_models_dirs())
    .filter_map(|path| remove_tree(&path).err())
    .collect();
    for parent in crate::model::default_models_dirs()
        .iter()
        .filter_map(|models| models.parent())
    {
        // Only when empty: the default install directory is also the model
        // cache's parent, and the uninstaller removes its own files after us.
        let _ = fs::remove_dir(parent);
    }
    failures.extend(crate::secret_store::purge());
    failures
}

#[cfg(any(windows, test))]
fn purge_targets(data_local: &Path, config: &Path, legacy: &Path) -> Vec<PathBuf> {
    let mut targets = vec![
        data_local.join(IDENTIFIER),
        config.join(IDENTIFIER),
        legacy.to_path_buf(),
    ];
    targets.dedup();
    targets
}

#[cfg(windows)]
fn remove_tree(path: &Path) -> Result<(), String> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_matches_the_tauri_configuration() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
    }

    #[test]
    fn a_pre_rename_override_applies_only_while_the_current_one_is_unset() {
        let lookup = |name: &str, set: &[(&str, &str)]| {
            override_from(name, |key| {
                set.iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.to_string())
            })
        };
        let old = [
            ("SOTTO_CONFIG_DIR", "old"),
            ("SPEECH_TO_TEXT_LOG_DIR", "old"),
        ];
        assert_eq!(lookup(DATA_DIR_ENV, &old).as_deref(), Some("old"));
        assert_eq!(lookup("SOTTO_LOG_DIR", &old).as_deref(), Some("old"));
        assert_eq!(lookup("SOTTO_MODELS_DIR", &old), None);

        let both = [
            ("SOTTO_MODELS_DIR", "new"),
            ("SPEECH_TO_TEXT_MODELS_DIR", "old"),
        ];
        assert_eq!(lookup("SOTTO_MODELS_DIR", &both).as_deref(), Some("new"));
    }

    #[test]
    fn purge_covers_data_configuration_and_the_legacy_directory_only() {
        let root = Path::new("/users/me");
        let targets = purge_targets(
            &root.join("AppData/Local"),
            &root.join("AppData/Roaming"),
            &root.join(".speech_to_text"),
        );
        assert_eq!(
            targets,
            [
                root.join("AppData/Local/com.sotto.app"),
                root.join("AppData/Roaming/com.sotto.app"),
                root.join(".speech_to_text"),
            ]
        );
        // macOS resolves both directories to Application Support.
        let support = root.join("Library/Application Support");
        assert_eq!(
            purge_targets(&support, &support, &root.join(".speech_to_text")).len(),
            2
        );
    }
}
