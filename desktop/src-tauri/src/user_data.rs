//! Where Sotto keeps the current user's data, the one-time move out of the
//! directory used before the rename, and the removal the uninstaller asks for.
//!
//! The history database, logs and diagnostic recordings live in the platform's
//! local data directory under the bundle identifier — what Tauri resolves as
//! `app_local_data_dir`. Builds up to 0.1.3 kept them in `~/.speech_to_text`;
//! [`migrate_legacy`] moves them on the first launch of a newer build;
//! stofll/Sotto#40 tracks removing the move once no such installs remain.

use std::fs;
use std::path::{Path, PathBuf};

/// Must equal `identifier` in `tauri.conf.json`, which names the directories
/// Tauri itself resolves; a test keeps the two in step.
pub const IDENTIFIER: &str = "com.sotto.app";
const LEGACY_DIR_NAME: &str = ".speech_to_text";
const DATA_DIR_ENV: &str = "SOTTO_DATA_DIR";
const DATABASE: &str = "sotto.db";

/// The directory holding `sotto.db`, `logs/` and diagnostic recordings.
///
/// Priority: the portable folder, then `SOTTO_DATA_DIR` (used as given, even
/// when empty), then the default location. The legacy directory is returned
/// only while its database has not been moved yet — a move postponed because
/// another process still had it open.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = crate::portable::data_dir() {
        return dir;
    }
    if let Ok(dir) = std::env::var(DATA_DIR_ENV) {
        return PathBuf::from(dir);
    }
    resolve(&default_dir(), &legacy_dir())
}

fn default_dir() -> PathBuf {
    dirs::data_local_dir()
        .expect("local data directory must be available")
        .join(IDENTIFIER)
}

fn legacy_dir() -> PathBuf {
    dirs::home_dir()
        .expect("user home directory must be available")
        .join(LEGACY_DIR_NAME)
}

fn resolve(default: &Path, legacy: &Path) -> PathBuf {
    if legacy.join(DATABASE).is_file() && !default.join(DATABASE).exists() {
        legacy.to_path_buf()
    } else {
        default.to_path_buf()
    }
}

/// What [`migrate_legacy`] did, kept until the logger can record it: the move
/// runs before the logger opens its file, because the log directory moves too.
pub struct Migration {
    from: PathBuf,
    to: PathBuf,
    outcome: Outcome,
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Moved,
    /// The database moved; these entries could not and stay behind.
    Incomplete(Vec<String>),
    /// Nothing moved. The legacy directory stays in use until the next launch.
    Postponed(String),
}

impl Migration {
    pub fn log(&self) {
        let (from, to) = (self.from.display(), self.to.display());
        match &self.outcome {
            Outcome::Moved => log::info!("user data moved from {from} to {to}"),
            Outcome::Incomplete(left) => log::warn!(
                "user data moved from {from} to {to}; left behind: {}",
                left.join("; ")
            ),
            Outcome::Postponed(reason) => {
                log::warn!("user data stays in {from} until the next launch: {reason}")
            }
        }
    }
}

/// Move the pre-rename data directory into the default location.
///
/// Returns `None` when there is nothing to move, and for a portable copy or an
/// explicit `SOTTO_DATA_DIR`, which never read the legacy directory. Safe to
/// run on every launch: once the legacy directory is gone this is one `stat`.
pub fn migrate_legacy() -> Option<Migration> {
    if crate::portable::data_dir().is_some() || std::env::var_os(DATA_DIR_ENV).is_some() {
        return None;
    }
    let from = legacy_dir();
    if !from.is_dir() {
        return None;
    }
    let to = default_dir();
    let outcome = migrate_between(&from, &to);
    Some(Migration { from, to, outcome })
}

fn migrate_between(legacy: &Path, target: &Path) -> Outcome {
    if let Err(error) = fs::create_dir_all(target) {
        return Outcome::Postponed(format!("create {}: {error}", target.display()));
    }
    // The database goes first and alone: it is the one thing that must never
    // end up split between two directories. Everything else can be retried.
    let database = legacy.join(DATABASE);
    if database.is_file() && !target.join(DATABASE).exists() {
        if let Err(reason) = release_database(&database) {
            return Outcome::Postponed(reason);
        }
        if let Err(error) = fs::rename(&database, target.join(DATABASE)) {
            return Outcome::Postponed(format!("move {DATABASE}: {error}"));
        }
    }
    let mut left = Vec::new();
    move_entries(legacy, target, &mut left);
    if left.is_empty() {
        if let Err(error) = fs::remove_dir(legacy) {
            left.push(format!("{}: {error}", legacy.display()));
        }
    }
    if left.is_empty() {
        Outcome::Moved
    } else {
        Outcome::Incomplete(left)
    }
}

/// Fold the write-ahead log into the database file and confirm that no other
/// process holds the database, so a single rename moves all of its data.
///
/// SQLite deletes the `-wal` file when its last connection closes. If the file
/// survives our close, someone else — a second copy of the app — still has the
/// database open, and moving it now would split its writes between two paths.
fn release_database(database: &Path) -> Result<(), String> {
    let connection = rusqlite::Connection::open(database)
        .map_err(|error| format!("open {}: {error}", database.display()))?;
    let busy: i64 = connection
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
        .map_err(|error| format!("checkpoint {}: {error}", database.display()))?;
    connection
        .close()
        .map_err(|(_, error)| format!("close {}: {error}", database.display()))?;
    let wal = database.with_file_name(format!("{DATABASE}-wal"));
    if busy != 0 || wal.exists() {
        return Err(format!(
            "{} is in use by another process",
            database.display()
        ));
    }
    Ok(())
}

/// Move every entry of `from` into `to`, merging directories that exist on
/// both sides. A file that exists on both sides is never overwritten: it stays
/// behind and is reported, which keeps a second database or anything else
/// unexpected rather than guessing which copy matters.
fn move_entries(from: &Path, to: &Path, left: &mut Vec<String>) {
    let entries = match fs::read_dir(from) {
        Ok(entries) => entries,
        Err(error) => {
            left.push(format!("{}: {error}", from.display()));
            return;
        }
    };
    for entry in entries.flatten() {
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if !destination.exists() {
            if let Err(error) = fs::rename(&source, &destination) {
                left.push(format!("{}: {error}", source.display()));
            }
        } else if source.is_dir() && destination.is_dir() {
            move_entries(&source, &destination, left);
            let _ = fs::remove_dir(&source);
        } else {
            left.push(format!(
                "{}: already exists in {}",
                source.display(),
                to.display()
            ));
        }
    }
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

    fn write(path: &Path, content: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn database_with_history(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let connection = rusqlite::Connection::open(path).unwrap();
        crate::db::run_migrations(&connection).unwrap();
        connection
            .execute_batch("PRAGMA journal_mode=WAL;")
            .unwrap();
        connection
            .execute(
                "INSERT INTO history (timestamp, text, length) VALUES (1.0, ?1, 1)",
                [text],
            )
            .unwrap();
    }

    fn history_texts(path: &Path) -> Vec<String> {
        let connection = rusqlite::Connection::open(path).unwrap();
        let mut statement = connection.prepare("SELECT text FROM history").unwrap();
        let rows = statement.query_map([], |row| row.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    }

    #[test]
    fn identifier_matches_the_tauri_configuration() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(config["identifier"], IDENTIFIER);
    }

    #[test]
    fn the_legacy_directory_is_used_only_until_its_database_moves() {
        let root = tempfile::tempdir().unwrap();
        let default = root.path().join("default");
        let legacy = root.path().join("legacy");
        assert_eq!(resolve(&default, &legacy), default);

        write(&legacy.join(DATABASE), "");
        assert_eq!(resolve(&default, &legacy), legacy);

        write(&default.join(DATABASE), "");
        assert_eq!(resolve(&default, &legacy), default);
    }

    #[test]
    fn migration_moves_the_database_logs_and_recordings_and_removes_the_old_directory() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join(".speech_to_text");
        let target = root.path().join("com.sotto.app");
        database_with_history(&legacy.join(DATABASE), "первая диктовка");
        write(&legacy.join("logs/app.log"), "log\n");
        write(&legacy.join("logs/recordings/1-1.wav"), "RIFF");
        // The configuration directory can be the same as the target (macOS).
        write(&target.join("config.json"), "{}");

        assert_eq!(migrate_between(&legacy, &target), Outcome::Moved);

        assert!(!legacy.exists());
        assert_eq!(history_texts(&target.join(DATABASE)), ["первая диктовка"]);
        assert_eq!(
            fs::read_to_string(target.join("logs/app.log")).unwrap(),
            "log\n"
        );
        assert!(target.join("logs/recordings/1-1.wav").is_file());
        assert_eq!(
            fs::read_to_string(target.join("config.json")).unwrap(),
            "{}"
        );
        assert_eq!(resolve(&target, &legacy), target);
    }

    #[test]
    fn a_database_held_open_elsewhere_is_left_where_it_is() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        let target = root.path().join("target");
        let database = legacy.join(DATABASE);
        database_with_history(&database, "в работе");
        write(&legacy.join("logs/app.log"), "log\n");
        let other_process = rusqlite::Connection::open(&database).unwrap();
        other_process
            .query_row("SELECT COUNT(*) FROM history", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();

        let outcome = migrate_between(&legacy, &target);

        assert!(matches!(outcome, Outcome::Postponed(_)), "{outcome:?}");
        assert!(database.is_file());
        assert!(legacy.join("logs/app.log").is_file());
        assert!(!target.join(DATABASE).exists());
        assert_eq!(resolve(&target, &legacy), legacy);
        drop(other_process);

        // The next launch finishes the move.
        assert_eq!(migrate_between(&legacy, &target), Outcome::Moved);
        assert_eq!(history_texts(&target.join(DATABASE)), ["в работе"]);
    }

    #[test]
    fn existing_files_are_never_overwritten_and_the_rest_still_moves() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        let target = root.path().join("target");
        database_with_history(&legacy.join(DATABASE), "старая");
        database_with_history(&target.join(DATABASE), "новая");
        write(&legacy.join("config.json"), "legacy");
        write(&target.join("config.json"), "current");
        write(&legacy.join("logs/app.log.1"), "archive");

        let outcome = migrate_between(&legacy, &target);

        let Outcome::Incomplete(left) = outcome else {
            panic!("expected leftovers, got {outcome:?}");
        };
        assert_eq!(left.len(), 2, "{left:?}");
        assert_eq!(history_texts(&legacy.join(DATABASE)), ["старая"]);
        assert_eq!(history_texts(&target.join(DATABASE)), ["новая"]);
        assert_eq!(
            fs::read_to_string(target.join("config.json")).unwrap(),
            "current"
        );
        assert_eq!(
            fs::read_to_string(legacy.join("config.json")).unwrap(),
            "legacy"
        );
        assert!(target.join("logs/app.log.1").is_file());
        assert_eq!(resolve(&target, &legacy), target);
    }

    #[test]
    fn a_repeated_run_after_an_incomplete_move_finishes_it() {
        let root = tempfile::tempdir().unwrap();
        let legacy = root.path().join("legacy");
        let target = root.path().join("target");
        database_with_history(&target.join(DATABASE), "уже перенесена");
        write(&legacy.join("history.json"), "[]");

        assert_eq!(migrate_between(&legacy, &target), Outcome::Moved);
        assert!(target.join("history.json").is_file());
        assert!(!legacy.exists());
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
