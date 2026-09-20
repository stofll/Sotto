//! SQLite storage, schema migrations and import of legacy JSON data.
//!
//! `Connection` is Send but not Sync. A mutex serializes access to the shared
//! connection; blocking workers acquire their guards inside the worker closure.

use std::path::PathBuf;
use std::sync::Mutex;

use rusqlite::Connection;

/// Directory containing `sotto.db` and legacy history/statistics files.
///
/// Priority:
/// 1. Portable data directory, when enabled on Windows
/// 2. Env `SOTTO_CONFIG_DIR` (if set, even to empty)
/// 3. `~/.speech_to_text` (via `dirs` crate)
///
/// NOT `app.path().app_config_dir()` — that resolves to a different path on macOS
/// (`~/Library/Application Support/<bundle>/`). Keeping the legacy directory
/// preserves access to existing history and statistics.
pub fn db_path() -> PathBuf {
    if let Some(dir) = crate::portable::data_dir() {
        return dir;
    }
    if let Ok(dir) = std::env::var("SOTTO_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::home_dir()
        .expect("user home directory must be available")
        .join(".speech_to_text")
}

/// Opens (or creates) `sotto.db` with WAL mode and applies schema migrations.
///
/// Returns `std::sync::Mutex<Connection>` for use with `tokio::task::spawn_blocking`:
/// `move || { let g = arc.lock().unwrap(); g.execute(...) }`.
pub fn open() -> Result<Mutex<Connection>, rusqlite::Error> {
    let dir = db_path();
    std::fs::create_dir_all(&dir).map_err(|e| {
        rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
            "create_dir_all({:?}): {}",
            dir, e
        ))))
    })?;
    let conn = Connection::open(dir.join("sotto.db"))?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
    run_migrations(&conn)?;
    Ok(Mutex::new(conn))
}

/// Apply schema migrations based on `PRAGMA user_version`.
///
/// v1: initial schema (3 tables: stats_totals, stats_daily, history).
/// v2: `llm_fallback_reasons` — per-day breakdown of why the LLM step failed.
/// v3: exact primary transcription model on each history row.
/// v4: repair history rows whose two JSON columns the old retry path swapped.
/// v5: telemetry installation metadata and durable event outbox.
/// v6: bounded local model performance observations.
pub fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    // Schema changes and their version must survive (or roll back) together.
    let tx = rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
    let current: i32 = tx.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }
    if current < 1 {
        tx.execute_batch(SCHEMA_V1)?;
    }
    if current < 2 {
        tx.execute_batch(SCHEMA_V2)?;
    }
    if current < 3 {
        // Older builds could commit ALTER TABLE before updating user_version.
        let has_model: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('history') WHERE name = 'transcription_model')",
            [],
            |row| row.get(0),
        )?;
        if !has_model {
            tx.execute_batch(SCHEMA_V3)?;
        }
    }
    if current < 4 {
        tx.execute_batch(SCHEMA_V4)?;
    }
    if current < 5 {
        tx.execute_batch(SCHEMA_V5)?;
    }
    if current < 6 {
        tx.execute_batch(include_str!("migrations/v6.sql"))?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    log::info!("db: migrated schema from v{current} to v{SCHEMA_VERSION}");
    Ok(())
}

/// The `PRAGMA user_version` a fully migrated database ends on.
///
/// Tests assert against this rather than a literal, so adding a migration
/// does not break every test that only cares about "ended up current".
pub const SCHEMA_VERSION: i32 = 6;

const SCHEMA_V1: &str = include_str!("migrations/v1.sql");
const SCHEMA_V2: &str = include_str!("migrations/v2.sql");
const SCHEMA_V3: &str = include_str!("migrations/v3.sql");
const SCHEMA_V4: &str = include_str!("migrations/v4.sql");
const SCHEMA_V5: &str = include_str!("migrations/v5.sql");

/// Migrate `stats.json` + `history.json` into the DB, once.
///
/// A consumed file is renamed to `*.migrated` — this is not cosmetic cleanup.
/// The stats import uses `INSERT OR REPLACE`, so leaving the file in place made
/// every single app start overwrite `stats_totals` with the Python-era
/// snapshot: lifetime counters silently rolled back to their migration-day
/// values on each launch while `stats_daily` kept accumulating. The two then
/// disagreed on the statistics page, which is exactly how this was found.
///
/// Only processes `history.json` entries younger than 24h (MAX_AGE_SECONDS) —
/// stale entries are dropped (mirror Python `transcription_history._prune`).
///
/// `config_dir` is the directory holding the legacy `.json` files
/// (typically `db_path()`). Returns Err with a human-readable message on
/// read, parse or database failure. Each file is imported atomically before
/// it is retired; a failed rename is logged separately. Callers treat import
/// failures as non-fatal warnings so a broken JSON file cannot prevent startup.
pub fn migrate_from_json(conn: &Connection, config_dir: &std::path::Path) -> Result<(), String> {
    // 1. stats.json → stats_totals + stats_daily.
    let stats_path = config_dir.join("stats.json");
    if stats_path.exists() {
        let raw =
            std::fs::read_to_string(&stats_path).map_err(|e| format!("read stats.json: {e}"))?;
        let stats: serde_json::Value =
            serde_json::from_str(&raw).map_err(|e| format!("parse stats.json: {e}"))?;
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("begin stats import: {e}"))?;
        migrate_stats_json(&tx, &stats)?;
        tx.commit()
            .map_err(|e| format!("commit stats import: {e}"))?;
        log::info!("migrate_from_json: stats.json seeded");
        retire_legacy_file(&stats_path);
    }

    // 2. history.json → history (only fresh entries).
    let history_path = config_dir.join("history.json");
    if history_path.exists() {
        let (inserted, skipped_stale) = import_history_json(conn, &history_path)?;
        log::info!(
            "migrate_from_json: history.json — {inserted} fresh entries seeded, {skipped_stale} stale skipped"
        );
        retire_legacy_file(&history_path);
    }
    Ok(())
}

/// Import `history.json` into `history`, keeping only fresh rows.
///
/// Returns `(inserted, skipped_stale)` so the caller can log what happened
/// and tests can assert the counters without scraping a log line.
fn import_history_json(
    conn: &Connection,
    path: &std::path::Path,
) -> Result<(usize, usize), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("read history.json: {e}"))?;
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&raw).map_err(|e| format!("parse history.json: {e}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| format!("system time: {e}"))?
        .as_secs_f64();
    let cutoff = now - 86_400.0; // MAX_AGE_SECONDS = 24h
    let mut inserted = 0usize;
    let mut skipped_stale = 0usize;
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("begin history import: {e}"))?;
    for entry in entries {
        let ts = entry
            .get("timestamp")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        if is_stale(ts, cutoff) {
            skipped_stale += 1;
            continue;
        }
        // id from JSON, or derive from timestamp*1000 (ms-since-epoch).
        let id = entry
            .get("id")
            .and_then(|v| v.as_i64())
            .unwrap_or((ts * 1000.0) as i64);
        let text = entry
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if text.is_empty() {
            continue;
        }
        let raw_text = entry
            .get("raw_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let formatted_text = entry
            .get("formatted_text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let language = entry
            .get("language")
            .and_then(|v| v.as_str())
            .map(String::from);
        let session_id = entry.get("session_id").and_then(|v| v.as_i64());
        let ai_processing = entry.get("ai_processing").map(|v| v.to_string());
        let processing_stats = entry.get("processing_stats").map(|v| v.to_string());
        let system_prompt = entry
            .get("system_prompt")
            .and_then(|v| v.as_str())
            .map(String::from);
        let transcription_model = entry
            .get("transcription_model")
            .or_else(|| entry.get("model_id"))
            .and_then(|v| v.as_str())
            .filter(|value| !value.trim().is_empty())
            .map(String::from);
        let length = text.chars().count() as i64;
        inserted += tx.execute(
            "INSERT INTO history (id, timestamp, text, raw_text, formatted_text, \
             language, session_id, ai_processing_json, processing_stats_json, system_prompt, transcription_model, length) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12) \
             ON CONFLICT(id) DO NOTHING",
            rusqlite::params![
                id,
                ts,
                text,
                raw_text,
                formatted_text,
                language,
                session_id,
                ai_processing,
                processing_stats,
                system_prompt,
                transcription_model,
                length
            ],
        ).map_err(|e| format!("insert history[{id}]: {e}"))?;
    }
    tx.commit()
        .map_err(|e| format!("commit history import: {e}"))?;
    Ok((inserted, skipped_stale))
}

/// A history entry is stale when its timestamp is strictly older than the
/// cutoff: an entry born at exactly the cutoff is kept, not dropped.
/// Extracted so the boundary is testable without mocking the clock.
fn is_stale(ts: f64, cutoff: f64) -> bool {
    ts < cutoff
}

/// Mark a legacy JSON file as consumed by renaming it to `*.migrated`.
///
/// Renamed rather than deleted: the import is one-way, and a user who needs to
/// look at the original should still be able to. A failure here is logged and
/// tolerated — a stale file is a bug, not a reason to refuse to start — but it
/// is logged at warn level because the import will then run again.
fn retire_legacy_file(path: &std::path::Path) {
    let retired = path.with_extension(match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => format!("{ext}.migrated"),
        None => "migrated".to_string(),
    });
    match std::fs::rename(path, &retired) {
        Ok(()) => log::info!("migrate_from_json: {} retired", path.display()),
        Err(error) => log::warn!(
            "migrate_from_json: could not retire {} ({error}); it will be imported again on the next start",
            path.display()
        ),
    }
}

fn migrate_stats_json(conn: &Connection, stats: &serde_json::Value) -> Result<(), String> {
    let obj = stats
        .as_object()
        .ok_or_else(|| "stats.json: root value is not an object".to_string())?;

    // Iterate all top-level numeric fields as totals. Skip daily_history
    // (handled separately below) and any non-numeric metadata fields.
    for (key, value) in obj {
        if key == "daily_history" {
            continue;
        }
        if let Some(n) = value.as_f64() {
            conn.execute(
                "INSERT OR REPLACE INTO stats_totals (key, value) VALUES (?1, ?2)",
                rusqlite::params![key, n],
            )
            .map_err(|e| format!("insert stats_totals[{key}]: {e}"))?;
        }
    }

    // Daily history — 16-column INSERT OR REPLACE per row.
    if let Some(daily) = obj.get("daily_history").and_then(|v| v.as_array()) {
        for entry in daily {
            let date = match entry.get("date").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            conn.execute(
                "INSERT OR REPLACE INTO stats_daily \
                 (date, count, chars, time_saved_seconds, audio_seconds, processing_seconds, \
                  whisper_seconds, format_seconds, llm_seconds, llm_attempts, llm_used, \
                  llm_fallbacks, llm_input_tokens, llm_output_tokens, llm_tokens, \
                  replacement_applications) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                rusqlite::params![
                    date,
                    entry.get("count").and_then(|v| v.as_i64()).unwrap_or(0),
                    entry.get("chars").and_then(|v| v.as_i64()).unwrap_or(0),
                    entry
                        .get("time_saved_seconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    entry
                        .get("audio_seconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    entry
                        .get("processing_seconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    entry
                        .get("whisper_seconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    entry
                        .get("format_seconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    entry
                        .get("llm_seconds")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(0.0),
                    entry
                        .get("llm_attempts")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    entry.get("llm_used").and_then(|v| v.as_i64()).unwrap_or(0),
                    entry
                        .get("llm_fallbacks")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    entry
                        .get("llm_input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    entry
                        .get("llm_output_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    entry
                        .get("llm_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                    entry
                        .get("replacement_applications")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0),
                ],
            )
            .map_err(|e| format!("insert stats_daily[{date}]: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::EnvGuard;

    /// Wall-clock seconds, the same unit the import path stores in
    /// `timestamp` — tests seed entries relative to it.
    fn now_secs() -> f64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64()
    }

    #[test]
    fn db_path_uses_home_dir_by_default() {
        let _g = EnvGuard::remove("SOTTO_CONFIG_DIR");
        let path = db_path();
        assert!(
            path.to_string_lossy().contains(".speech_to_text"),
            "expected default path to contain .speech_to_text, got {:?}",
            path,
        );
    }

    #[test]
    fn db_path_respects_env_override() {
        let _g = EnvGuard::set("SOTTO_CONFIG_DIR", "/tmp/sotto-test-env");
        let path = db_path();
        assert_eq!(path, std::path::PathBuf::from("/tmp/sotto-test-env"));
    }

    #[test]
    fn open_creates_db_file() {
        let tmp = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set("SOTTO_CONFIG_DIR", tmp.path().to_str().unwrap());
        let conn_mutex = open().expect("open should succeed");
        let conn = conn_mutex.lock().unwrap();
        // Verify table exists after migration
        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='history'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "history table should exist after open()");
    }

    #[test]
    fn migrate_from_json_seeds_stats_and_history() {
        let tmp = tempfile::tempdir().unwrap();

        // Seed stats.json
        let stats_json = serde_json::json!({
            "total_transcriptions": 42,
            "total_characters": 1234,
            "total_time_saved_seconds": 100.5,
            "daily_history": [
                {"date": "2026-07-01", "count": 5, "chars": 100, "time_saved_seconds": 12.0}
            ]
        });
        std::fs::write(tmp.path().join("stats.json"), stats_json.to_string()).unwrap();

        // Seed history.json with one fresh + one stale entry.
        let now = now_secs();
        let fresh_ts = now - 60.0; // 1 min ago
        let stale_ts = now - 200_000.0; // > 24h ago
        let history_json = serde_json::json!([
            {"id": 9001, "timestamp": fresh_ts, "text": "fresh entry", "length": 11, "model_id": "large-v3-turbo"},
            {"id": 9002, "timestamp": stale_ts, "text": "stale entry", "length": 11}
        ]);
        std::fs::write(tmp.path().join("history.json"), history_json.to_string()).unwrap();

        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();

        // Verify totals seeded.
        let total: f64 = conn
            .query_row(
                "SELECT value FROM stats_totals WHERE key = 'total_transcriptions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 42.0);

        // Verify daily seeded.
        let count: i64 = conn
            .query_row(
                "SELECT count FROM stats_daily WHERE date = '2026-07-01'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 5);

        // Verify only fresh history entry inserted.
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1, "stale history entry should be skipped");
        let text: String = conn
            .query_row("SELECT text FROM history WHERE id = 9001", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "fresh entry");
        let model: String = conn
            .query_row(
                "SELECT transcription_model FROM history WHERE id = 9001",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, "large-v3-turbo");
    }

    #[test]
    fn v3_migration_keeps_legacy_rows_without_a_model_as_null() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, length) VALUES (1, 0.0, 'old', 3)",
            [],
        )
        .unwrap();
        let model: Option<String> = conn
            .query_row(
                "SELECT transcription_model FROM history WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, None);
    }

    #[test]
    fn v3_upgrade_preserves_existing_v2_history_rows() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.execute("PRAGMA user_version = 2", []).unwrap();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, length) VALUES (42, 123.0, 'before v3', 9)",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let row: (String, Option<String>) = conn
            .query_row(
                "SELECT text, transcription_model FROM history WHERE id = 42",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(row, ("before v3".to_string(), None));
    }

    #[test]
    fn upgrades_from_every_schema_version() {
        let migrations = [
            SCHEMA_V1,
            SCHEMA_V2,
            SCHEMA_V3,
            SCHEMA_V4,
            SCHEMA_V5,
            include_str!("migrations/v6.sql"),
        ];
        for version in 0..=SCHEMA_VERSION {
            let conn = Connection::open_in_memory().unwrap();
            for sql in migrations.iter().take(version as usize) {
                conn.execute_batch(sql).unwrap();
            }
            conn.pragma_update(None, "user_version", version).unwrap();
            run_migrations(&conn).unwrap();
            let actual: i32 = conn
                .query_row("PRAGMA user_version", [], |r| r.get(0))
                .unwrap();
            assert_eq!(actual, SCHEMA_VERSION, "upgrade from v{version}");
            // Exercise the final schema, including the non-idempotent ALTER.
            conn.execute(
                "INSERT INTO history (id, timestamp, text, length, transcription_model) \
                 VALUES (1, 0, 'preserved', 9, 'test-model')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO model_performance (model_id, created, profile, payload) \
                 VALUES ('test-model', 0, 'cpu', '{}')",
                [],
            )
            .unwrap();
            run_migrations(&conn).unwrap();
            let text: String = conn
                .query_row("SELECT text FROM history WHERE id = 1", [], |r| r.get(0))
                .unwrap();
            assert_eq!(text, "preserved");
        }
    }

    #[test]
    fn resumes_v3_whose_column_was_committed_without_its_version() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(SCHEMA_V3).unwrap();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, length, transcription_model) \
             VALUES (1, 0, 'kept', 4, 'legacy-model')",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        let model: String = conn
            .query_row(
                "SELECT transcription_model FROM history WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(model, "legacy-model");
    }

    #[test]
    fn failed_upgrade_rolls_back_schema_data_and_version_and_can_retry() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.pragma_update(None, "user_version", 2).unwrap();
        conn.execute_batch(
            "INSERT INTO history (id, timestamp, text, length, ai_processing_json, processing_stats_json) \
             VALUES (1, 0, 'before', 6, '{\"text\":\"after\"}', '{\"attempted\":true}'); \
             CREATE TABLE telemetry_outbox (incompatible_column TEXT);",
        ).unwrap();

        // v5 cannot create its index; v3 ALTER and v4 data repair ran before it.
        assert!(run_migrations(&conn).is_err());
        assert!(conn.is_autocommit());
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 2);
        let has_model: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('history') WHERE name = 'transcription_model')",
            [], |r| r.get(0),
        ).unwrap();
        assert!(!has_model, "ALTER TABLE must roll back with the version");
        let text: String = conn
            .query_row("SELECT text FROM history WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "before", "data repairs must roll back too");

        conn.execute_batch("DROP TABLE telemetry_outbox").unwrap();
        run_migrations(&conn).unwrap();
        let text: String = conn
            .query_row("SELECT text FROM history WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "after");
    }

    #[test]
    fn malformed_history_is_reported_and_preserved_for_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.json");
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        for malformed in ["[{", "{}", "null"] {
            std::fs::write(&path, malformed).unwrap();
            let error = migrate_from_json(&conn, tmp.path()).unwrap_err();
            assert!(error.contains("parse history.json"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), malformed);
            assert!(!tmp.path().join("history.json.migrated").exists());
        }
        std::fs::write(&path, "[]").unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();
        assert!(!path.exists());
        assert!(tmp.path().join("history.json.migrated").exists());
    }

    #[test]
    fn history_sql_failure_rolls_back_the_file_and_preserves_it_for_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("history.json");
        std::fs::write(
            &path,
            serde_json::json!([
                {"id": 1, "timestamp": now_secs(), "text": "first"},
                {"id": 2, "timestamp": now_secs(), "text": "second"}
            ])
            .to_string(),
        )
        .unwrap();
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_second BEFORE INSERT ON history WHEN NEW.id = 2 \
             BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;",
        )
        .unwrap();

        let error = migrate_from_json(&conn, tmp.path()).unwrap_err();
        assert!(error.contains("insert history[2]"));
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 0,
            "first row must not survive the failed file import"
        );
        assert!(path.exists());
        assert!(!tmp.path().join("history.json.migrated").exists());

        conn.execute_batch("DROP TRIGGER reject_second").unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        assert!(!path.exists());
        migrate_from_json(&conn, tmp.path()).unwrap();
        let count: usize = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn stats_sql_failure_rolls_back_totals_and_preserves_source() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("stats.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "total_transcriptions": 7,
                "daily_history": [{"date": "2026-07-01", "count": 3}]
            })
            .to_string(),
        )
        .unwrap();
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn.execute_batch(
            "INSERT INTO stats_totals VALUES ('total_transcriptions', 10); \
             CREATE TRIGGER reject_daily BEFORE INSERT ON stats_daily \
             BEGIN SELECT RAISE(ABORT, 'synthetic failure'); END;",
        )
        .unwrap();
        assert!(migrate_from_json(&conn, tmp.path()).is_err());
        let count: f64 = conn
            .query_row(
                "SELECT value FROM stats_totals WHERE key = 'total_transcriptions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 10.0);
        assert!(path.exists());
        assert!(!tmp.path().join("stats.json.migrated").exists());
    }

    #[test]
    fn run_migrations_skips_steps_at_or_below_current_version() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute("PRAGMA user_version = 1", []).unwrap();
        run_migrations(&conn).unwrap();
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        // v3 must have applied (adds the column v1 lacks).
        let cols: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('history') WHERE name = 'transcription_model'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cols, 1);
    }

    #[test]
    fn run_migrations_from_v3_must_not_reapply_v3() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.execute_batch(SCHEMA_V3).unwrap();
        conn.execute("PRAGMA user_version = 3", []).unwrap();
        // A normal v3 database must upgrade without adding the column again.
        run_migrations(&conn).unwrap();
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn is_stale_is_strict_at_the_cutoff() {
        assert!(
            !is_stale(100.0, 100.0),
            "an entry born exactly at the cutoff is kept (strict <)"
        );
        assert!(is_stale(99.999, 100.0), "older than cutoff is stale");
        assert!(!is_stale(100.001, 100.0), "younger than cutoff is kept");
    }

    #[test]
    fn migrate_from_json_derives_id_from_timestamp_ms() {
        let tmp = tempfile::tempdir().unwrap();
        let now = now_secs();
        let ts = now.floor(); // whole second, freshly recent
        std::fs::write(
            tmp.path().join("history.json"),
            serde_json::json!([{ "timestamp": ts, "text": "derived" }]).to_string(),
        )
        .unwrap();

        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();

        // Without an explicit `id`, the id is `timestamp * 1000` exactly —
        // asserting the exact value catches both `+` and `/` substitutions.
        let id: i64 = conn
            .query_row("SELECT id FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(id, (ts * 1000.0) as i64);
    }

    #[test]
    fn import_history_json_counts_fresh_and_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let now = now_secs();
        let path = tmp.path().join("history.json");
        std::fs::write(
            &path,
            serde_json::json!([
                { "timestamp": now - 60.0, "text": "a" },
                { "timestamp": now - 120.0, "text": "b" },
                { "timestamp": now - 200_000.0, "text": "stale" }
            ])
            .to_string(),
        )
        .unwrap();

        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        let (inserted, skipped_stale) = import_history_json(&conn, &path).unwrap();
        assert_eq!(inserted, 2, "two fresh entries must be counted as inserted");
        assert_eq!(
            skipped_stale, 1,
            "one stale entry must be counted as skipped"
        );
        let (inserted, skipped_stale) = import_history_json(&conn, &path).unwrap();
        assert_eq!(inserted, 0, "existing IDs are not newly inserted rows");
        assert_eq!(skipped_stale, 1);
    }

    #[test]
    fn migrate_from_json_is_idempotent() {
        // Running migrate_from_json twice should not duplicate rows.
        let tmp = tempfile::tempdir().unwrap();
        let stats_json = serde_json::json!({
            "total_transcriptions": 7,
            "daily_history": [{"date": "2026-07-01", "count": 3}]
        });
        std::fs::write(tmp.path().join("stats.json"), stats_json.to_string()).unwrap();
        let now = now_secs();
        let history_json = serde_json::json!([
            {"id": 8001, "timestamp": now - 30.0, "text": "hello", "length": 5}
        ]);
        std::fs::write(tmp.path().join("history.json"), history_json.to_string()).unwrap();

        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();

        let n_total: f64 = conn
            .query_row(
                "SELECT value FROM stats_totals WHERE key = 'total_transcriptions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n_total, 7.0, "totals should remain at seeded value");
        let n_history: i64 = conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n_history, 1, "history should not duplicate on rerun");
    }

    #[test]
    fn migrate_from_json_retires_the_files_it_consumed() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("stats.json"),
            serde_json::json!({"total_transcriptions": 7}).to_string(),
        )
        .unwrap();
        std::fs::write(tmp.path().join("history.json"), "[]").unwrap();

        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();

        assert!(!tmp.path().join("stats.json").exists());
        assert!(tmp.path().join("stats.json.migrated").exists());
        assert!(!tmp.path().join("history.json").exists());
        assert!(tmp.path().join("history.json.migrated").exists());
    }

    /// The bug this guards: the stats import uses INSERT OR REPLACE, so a
    /// stats.json left in place rolled the lifetime counters back to their
    /// migration-day values on every single app start.
    #[test]
    fn a_second_start_does_not_roll_lifetime_totals_back() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("stats.json"),
            serde_json::json!({"total_transcriptions": 997}).to_string(),
        )
        .unwrap();

        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        migrate_from_json(&conn, tmp.path()).unwrap();

        // Five transcriptions recorded while the app was running.
        conn.execute(
            "UPDATE stats_totals SET value = value + 5 WHERE key = 'total_transcriptions'",
            [],
        )
        .unwrap();

        // Restart.
        migrate_from_json(&conn, tmp.path()).unwrap();

        let total: f64 = conn
            .query_row(
                "SELECT value FROM stats_totals WHERE key = 'total_transcriptions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 1002.0, "restart must not undo what was recorded");
    }
}

#[cfg(test)]
mod migration_v4_tests {
    use super::*;

    /// Reproduces a row as the pre-fix retry path left it: the cleaned text
    /// stranded in `ai_processing_json`, the status in the timings column,
    /// and the `text` column still holding the pre-LLM version.
    fn broken_row(conn: &Connection) {
        conn.execute(
            "INSERT INTO history (id, timestamp, text, raw_text, formatted_text, length, \
             ai_processing_json, processing_stats_json) \
             VALUES (1, 0.0, 'до обработки', 'raw', 'до обработки', 12, \
             json_object('text', 'После обработки.'), \
             json_object('attempted', json('true'), 'used', json('true'), \
                         'elapsed_seconds', 4.5, 'provider', 'compatible'))",
            [],
        )
        .unwrap();
    }

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn repairs_a_row_the_old_retry_path_swapped() {
        let conn = fresh();
        broken_row(&conn);
        conn.execute_batch(SCHEMA_V4).unwrap();

        let (text, length, ai, ps): (String, i64, String, String) = conn
            .query_row(
                "SELECT text, length, ai_processing_json, processing_stats_json \
                 FROM history WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();

        // The text the provider returned finally reaches the column the UI shows.
        assert_eq!(text, "После обработки.");
        assert_eq!(length, 16);
        // The status moves to the column the frontend actually reads.
        let ai: serde_json::Value = serde_json::from_str(&ai).unwrap();
        assert_eq!(ai["used"], serde_json::json!(true));
        assert_eq!(ai["provider"], serde_json::json!("compatible"));
        // Timings are rebuilt from what the status knew about itself.
        let ps: serde_json::Value = serde_json::from_str(&ps).unwrap();
        assert_eq!(ps["llm_seconds"], serde_json::json!(4.5));
        assert_eq!(ps["total_seconds"], serde_json::json!(4.5));
    }

    /// Running it twice must be a no-op — after the first pass the row no
    /// longer matches the discriminator, and a second swap would move the
    /// status back out of the column it belongs in.
    #[test]
    fn is_idempotent() {
        let conn = fresh();
        broken_row(&conn);
        conn.execute_batch(SCHEMA_V4).unwrap();
        conn.execute_batch(SCHEMA_V4).unwrap();
        let text: String = conn
            .query_row("SELECT text FROM history WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "После обработки.");
    }

    /// A row written by the live dispatcher has the status in
    /// `ai_processing_json` already and must be left strictly alone.
    #[test]
    fn leaves_healthy_rows_untouched() {
        let conn = fresh();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, raw_text, formatted_text, length, \
             ai_processing_json, processing_stats_json) \
             VALUES (2, 0.0, 'живой текст', 'raw', 'formatted', 11, \
             json_object('attempted', json('true'), 'used', json('true')), \
             json_object('audio_seconds', 19.2, 'whisper_seconds', 0.5, \
                         'llm_seconds', 3.0, 'total_seconds', 3.5))",
            [],
        )
        .unwrap();
        conn.execute_batch(SCHEMA_V4).unwrap();

        let (text, ps): (String, String) = conn
            .query_row(
                "SELECT text, processing_stats_json FROM history WHERE id = 2",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(text, "живой текст");
        let ps: serde_json::Value = serde_json::from_str(&ps).unwrap();
        assert_eq!(ps["audio_seconds"], serde_json::json!(19.2));
    }

    /// A pin on the second discriminator from the WHERE clause.
    ///
    /// There are two conditions — `$.text` in ai_processing_json and
    /// `$.attempted` in processing_stats_json — and a mutation run showed the
    /// first was covered by nothing: remove it from the SQL and every test is
    /// still green, because in all the rows under test the second condition
    /// fails as well. Here the row is chosen so that **only** the `$.text` check
    /// saves it. Both checks on a destructive UPDATE are worth keeping, but each
    /// must be responsible for something.
    #[test]
    fn a_row_without_the_stranded_text_is_not_repaired() {
        let conn = fresh();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, raw_text, formatted_text, length,              ai_processing_json, processing_stats_json)              VALUES (4, 0.0, 'живой текст', 'raw', 'formatted', 11,              json_object('used', json('true')),              json_object('attempted', json('true'), 'elapsed_seconds', 2.0))",
            [],
        )
        .unwrap();
        conn.execute_batch(SCHEMA_V4).unwrap();

        let (text, ai): (String, String) = conn
            .query_row(
                "SELECT text, ai_processing_json FROM history WHERE id = 4",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(text, "живой текст");
        // The columns did not swap places.
        let ai: serde_json::Value = serde_json::from_str(&ai).unwrap();
        assert_eq!(ai["used"], serde_json::json!(true));
    }

    /// Rows that predate the AI columns entirely (both NULL) must not trip
    /// the json_valid() guards.
    #[test]
    fn tolerates_rows_without_ai_columns() {
        let conn = fresh();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, length) VALUES (3, 0.0, 'старьё', 6)",
            [],
        )
        .unwrap();
        conn.execute_batch(SCHEMA_V4).unwrap();
        let text: String = conn
            .query_row("SELECT text FROM history WHERE id = 3", [], |r| r.get(0))
            .unwrap();
        assert_eq!(text, "старьё");
    }
}
