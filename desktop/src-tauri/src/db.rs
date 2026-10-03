//! SQLite storage and schema migrations.
//!
//! `Connection` is Send but not Sync. A mutex serializes access to the shared
//! connection; blocking workers acquire their guards inside the worker closure.

use std::sync::Mutex;

use rusqlite::Connection;

/// Opens (or creates) `sotto.db` with WAL mode and applies schema migrations.
///
/// Returns `std::sync::Mutex<Connection>` for use with `tokio::task::spawn_blocking`:
/// `move || { let g = arc.lock().unwrap(); g.execute(...) }`.
pub fn open() -> Result<Mutex<Connection>, rusqlite::Error> {
    let dir = crate::user_data::data_dir();
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
/// v7: measured silence excluded from estimated dictation time.
/// v8: acknowledged version and cached release notes.
/// v9: drop the v6 observations; model cards no longer record anything.
/// v10: durable legacy-file import markers.
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
    if current < 7 {
        tx.execute_batch(include_str!("migrations/v7.sql"))?;
    }
    if current < 8 {
        tx.execute_batch(include_str!("migrations/v8.sql"))?;
    }
    if current < 9 {
        tx.execute_batch(include_str!("migrations/v9.sql"))?;
    }
    if current < 10 {
        tx.execute_batch(include_str!("migrations/v10.sql"))?;
    }
    if current < 11 {
        tx.execute_batch(include_str!("migrations/v11.sql"))?;
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
pub const SCHEMA_VERSION: i32 = 11;

const SCHEMA_V1: &str = include_str!("migrations/v1.sql");
const SCHEMA_V2: &str = include_str!("migrations/v2.sql");
const SCHEMA_V3: &str = include_str!("migrations/v3.sql");
const SCHEMA_V4: &str = include_str!("migrations/v4.sql");
const SCHEMA_V5: &str = include_str!("migrations/v5.sql");

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::EnvGuard;

    #[test]
    fn speech_timing_migration_preserves_existing_statistics() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA_V1).unwrap();
        conn.execute_batch(SCHEMA_V2).unwrap();
        conn.execute_batch(SCHEMA_V3).unwrap();
        conn.execute_batch(SCHEMA_V4).unwrap();
        conn.execute_batch(SCHEMA_V5).unwrap();
        conn.execute_batch(include_str!("migrations/v6.sql"))
            .unwrap();
        conn.execute_batch("PRAGMA user_version=6; INSERT INTO stats_daily(date, count, chars, audio_seconds) VALUES ('2026-09-22', 2, 100, 50);").unwrap();
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();
        let row = conn.query_row("SELECT count, chars, audio_seconds, excluded_silence_seconds, speech_timed_count FROM stats_daily", [], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, f64>(2)?, r.get::<_, f64>(3)?, r.get::<_, i64>(4)?))
        }).unwrap();
        assert_eq!(row, (2, 100, 50.0, 0.0, 0));
    }

    #[test]
    fn data_dir_respects_env_override() {
        let _g = EnvGuard::set("SOTTO_DATA_DIR", "/tmp/sotto-test-env");
        let path = crate::user_data::data_dir();
        assert_eq!(path, std::path::PathBuf::from("/tmp/sotto-test-env"));
    }

    #[test]
    fn open_creates_db_file() {
        let tmp = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set("SOTTO_DATA_DIR", tmp.path().to_str().unwrap());
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
    fn opening_preserves_current_history_and_leaves_legacy_json_untouched() {
        let tmp = tempfile::tempdir().unwrap();
        let _g = EnvGuard::set("SOTTO_DATA_DIR", tmp.path());
        let legacy_history = r#"[{"id": 99, "timestamp": 1, "text": "old"}]"#;
        let legacy_stats = r#"{"total_transcriptions": 99}"#;
        std::fs::write(tmp.path().join("history.json"), legacy_history).unwrap();
        std::fs::write(tmp.path().join("stats.json"), legacy_stats).unwrap();
        {
            let db = open().unwrap();
            db.lock()
                .unwrap()
                .execute(
                    "INSERT INTO history (id, timestamp, text, length) VALUES (1, 1, 'current', 7)",
                    [],
                )
                .unwrap();
        }

        let db = open().unwrap();
        let conn = db.lock().unwrap();
        let row: (i64, String) = conn
            .query_row("SELECT COUNT(*), text FROM history", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(row, (1, "current".into()));
        for (name, content) in [
            ("history.json", legacy_history),
            ("stats.json", legacy_stats),
        ] {
            assert_eq!(
                std::fs::read_to_string(tmp.path().join(name)).unwrap(),
                content
            );
            assert!(!tmp.path().join(format!("{name}.migrated")).exists());
        }
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
            include_str!("migrations/v7.sql"),
            include_str!("migrations/v8.sql"),
            include_str!("migrations/v9.sql"),
            include_str!("migrations/v10.sql"),
            include_str!("migrations/v11.sql"),
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
            run_migrations(&conn).unwrap();
            let text: String = conn
                .query_row("SELECT text FROM history WHERE id = 1", [], |r| r.get(0))
                .unwrap();
            assert_eq!(text, "preserved");
        }
    }

    #[test]
    fn upgrading_from_v8_drops_the_recorded_timings() {
        let conn = Connection::open_in_memory().unwrap();
        for sql in [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3, SCHEMA_V4, SCHEMA_V5] {
            conn.execute_batch(sql).unwrap();
        }
        for sql in [
            include_str!("migrations/v6.sql"),
            include_str!("migrations/v7.sql"),
            include_str!("migrations/v8.sql"),
        ] {
            conn.execute_batch(sql).unwrap();
        }
        conn.pragma_update(None, "user_version", 8).unwrap();
        conn.execute(
            "INSERT INTO model_performance (model_id, created, profile, payload) \
             VALUES ('tiny', 0, 'cpu', '{\"inference_ms\":1200.0}')",
            [],
        )
        .unwrap();

        run_migrations(&conn).unwrap();

        let tables: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name LIKE 'model_performance%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tables, 0);
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
