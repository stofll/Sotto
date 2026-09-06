//! History persistence (WS 4b).
//!
//! Replaces Python `transcription_history.py`. Used by the frontend as a
//! fallback when auto-paste misses (focus changed, clipboard race, etc.) and
//! as a record of what was dictated.
//!
//! How much is kept is a user setting — see [`RetentionPolicy`]. It used to
//! be hardcoded at the Python defaults of 24 hours / 50 entries, which is a
//! fallback buffer rather than a history: anything from yesterday was
//! already gone.
//!
//! Schema: see `migrations/v1.sql` and `migrations/v3.sql` — `history` table
//! with `id INTEGER PRIMARY KEY` (ms timestamp + collision suffix),
//! `timestamp REAL`, exact `transcription_model`, and JSON-blob columns for
//! `ai_processing` / `processing_stats` (deferred to WS 4c for real LLM
//! metrics).

use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;

/// Config key: how many days of history to keep. `0` means "no age limit".
const CONFIG_RETENTION_DAYS: &str = "history_retention_days";
/// Config key: hard cap on stored entries. `0` means "no count limit".
const CONFIG_MAX_ENTRIES: &str = "history_max_entries";

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
/// A month of history. Long enough to answer "what did I dictate last
/// week?", short enough that the table stays small.
const DEFAULT_RETENTION_DAYS: i64 = 30;
/// Sized against the default retention: at ~20 dictations a day, a month is
/// roughly 600 entries, so this cap does not quietly override the age limit.
const DEFAULT_MAX_ENTRIES: i64 = 1000;
/// Ten years. Not a meaningful limit, just a guard against a hand-edited
/// config producing an absurd cutoff.
const MAX_RETENTION_DAYS: i64 = 3650;
/// Safety bound for `INSERT OR IGNORE` collision retries. 1000 ms = 1 second
/// of accumulated clock skew; in practice the loop exits after 1-3 iterations
/// because ms-collision requires two appends in the same millisecond.
const APPEND_COLLISION_MAX_ITER: i64 = 1000;

/// How much history to keep. Both limits apply; whichever bites first wins.
///
/// `0` disables a limit rather than meaning "keep nothing" — that reading
/// would turn a plausible hand-edit into silent data loss.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub max_age_seconds: i64,
    pub max_entries: i64,
}

impl Default for RetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_seconds: DEFAULT_RETENTION_DAYS * SECONDS_PER_DAY,
            max_entries: DEFAULT_MAX_ENTRIES,
        }
    }
}

impl RetentionPolicy {
    /// Read the policy out of a loaded config value. Missing or nonsensical
    /// values fall back to the default rather than failing: history settings
    /// are not worth breaking the History page over.
    pub fn from_config(config: &Value) -> Self {
        let default = Self::default();
        let days = config
            .get(CONFIG_RETENTION_DAYS)
            .and_then(Value::as_i64)
            .filter(|days| (0..=MAX_RETENTION_DAYS).contains(days))
            .unwrap_or(DEFAULT_RETENTION_DAYS);
        let max_entries = config
            .get(CONFIG_MAX_ENTRIES)
            .and_then(Value::as_i64)
            .filter(|entries| *entries >= 0)
            .unwrap_or(default.max_entries);
        Self {
            max_age_seconds: days * SECONDS_PER_DAY,
            max_entries,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub id: u64,
    pub timestamp: f64,
    pub text: String,
    #[serde(rename = "raw_text")]
    pub raw_text: String,
    #[serde(rename = "formatted_text")]
    pub formatted_text: String,
    /// Exact primary transcription model used by the engine thread. This is
    /// intentionally separate from `ai_processing`, which describes the
    /// optional post-processing provider/model.
    pub transcription_model: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "inference_time_ms")]
    pub inference_time_ms: Option<u64>,
    #[serde(rename = "ai_processing")]
    pub ai_processing: Option<serde_json::Value>,
    #[serde(rename = "processing_stats")]
    pub processing_stats: Option<serde_json::Value>,
    #[serde(rename = "system_prompt")]
    pub system_prompt: Option<String>,
    pub length: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryListResult {
    pub entries: Vec<HistoryEntry>,
    #[serde(rename = "max_age_seconds")]
    pub max_age_seconds: u64,
    #[serde(rename = "max_entries")]
    pub max_entries: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeleteResult {
    pub deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClearResult {
    pub deleted: u64,
}

/// A fully-populated history row awaiting insertion. Borrows its string
/// fields so the caller (the dispatcher) doesn't have to clone them.
///
/// `text` is the FINAL text (what was pasted); `raw_text` is the untouched
/// whisper output; `formatted_text` is the pre-LLM text (after local
/// formatting). The history UI diffs `formatted_text` against `text` to show
/// the LLM's edits and lists `raw_text` when it differs from `formatted_text`.
#[derive(Debug, Default)]
pub struct NewEntry<'a> {
    pub text: &'a str,
    pub raw_text: &'a str,
    pub formatted_text: &'a str,
    pub session_id: Option<u64>,
    pub language: Option<&'a str>,
    pub inference_time_ms: u64,
    pub ai_processing_json: Option<&'a str>,
    pub processing_stats_json: Option<&'a str>,
    pub system_prompt: Option<&'a str>,
    pub transcription_model: Option<&'a str>,
}

/// Append a new entry. Returns the assigned `id`.
///
/// Convenience wrapper over [`append_entry`] for callers that only have the
/// transcript text + timing (e.g. tests, and any path with no formatting/LLM
/// context). Builds the `processing_stats` JSON with audio + whisper timing
/// and leaves raw/formatted/ai fields empty.
#[allow(dead_code)] // convenience wrapper; exercised by tests, live path uses append_entry
pub fn append(
    db: &Mutex<Connection>,
    text: &str,
    session_id: Option<u64>,
    language: Option<&str>,
    inference_time_ms: u64,
    audio_seconds: f64,
) -> Result<u64, rusqlite::Error> {
    let whisper_seconds = inference_time_ms as f64 / 1000.0;
    let processing_stats = serde_json::json!({
        "audio_seconds": audio_seconds,
        "whisper_seconds": whisper_seconds,
        "total_seconds": whisper_seconds,
    })
    .to_string();
    append_entry(
        db,
        &NewEntry {
            text,
            raw_text: "",
            formatted_text: "",
            session_id,
            language,
            inference_time_ms,
            ai_processing_json: None,
            processing_stats_json: Some(&processing_stats),
            system_prompt: None,
            transcription_model: None,
        },
    )
}

/// Append a fully-populated entry (all text stages + AI/processing JSON).
///
/// Collision handling (IMPORTANT-5): two transcriptions in the same
/// millisecond → the second `INSERT OR IGNORE` returns `Ok(0)` (NOT an
/// Err — IGNORE swallows the conflict). We detect this via
/// `affected_rows == 0` and retry with `id + 1` until success, up to
/// `APPEND_COLLISION_MAX_ITER` iterations as a safety bound.
pub fn append_entry(db: &Mutex<Connection>, entry: &NewEntry) -> Result<u64, rusqlite::Error> {
    let conn = db.lock().unwrap();
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
                "system time: {e}"
            ))))
        })?
        .as_secs_f64();
    let length = entry.text.chars().count() as i64;

    // Base id: ms-since-epoch. On collision, increment by 1.
    let id = (timestamp * 1000.0) as i64;
    insert_entry_with_collision_retry(&conn, id, timestamp, entry, length)
}

/// Insert a row, retrying with `id + 1` on primary-key collision.
///
/// Collision handling (IMPORTANT-5): two transcriptions in the same
/// millisecond → the second `INSERT OR IGNORE` returns `Ok(0)` (NOT an
/// Err — IGNORE swallows the conflict). We detect this via
/// `affected_rows == 0` and retry with `id + 1` until success, up to
/// `APPEND_COLLISION_MAX_ITER` iterations as a safety bound.
fn insert_entry_with_collision_retry(
    conn: &Connection,
    mut id: i64,
    timestamp: f64,
    entry: &NewEntry,
    length: i64,
) -> Result<u64, rusqlite::Error> {
    #[allow(clippy::explicit_counter_loop)]
    // intentional: bumping id inside the loop on PK collision
    for _ in 0..APPEND_COLLISION_MAX_ITER {
        let affected = conn.execute(
            "INSERT OR IGNORE INTO history (id, timestamp, text, raw_text, formatted_text, \
             length, language, session_id, inference_time_ms, ai_processing_json, \
             processing_stats_json, system_prompt, transcription_model) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id,
                timestamp,
                entry.text,
                entry.raw_text,
                entry.formatted_text,
                length,
                entry.language,
                entry.session_id.map(|v| v as i64),
                entry.inference_time_ms as i64,
                entry.ai_processing_json,
                entry.processing_stats_json,
                entry.system_prompt,
                entry.transcription_model,
            ],
        )?;
        if affected > 0 {
            return Ok(id as u64);
        }
        // Collision: INSERT OR IGNORE silently skipped. Bump id and retry.
        id += 1;
    }
    Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
        std::io::Error::other(format!(
            "history::append exhausted {APPEND_COLLISION_MAX_ITER} collision retries"
        )),
    )))
}

/// Prune to `policy`, then list what is left. Caller holds &Connection.
///
/// Rows outside the policy are physically deleted (NOT just filtered out of
/// the response), so the table stays bounded. This is the only place pruning
/// happens, which means a lowered retention setting takes effect the next
/// time the History page is opened rather than immediately.
pub fn list_history_from(
    conn: &Connection,
    policy: RetentionPolicy,
) -> Result<HistoryListResult, rusqlite::Error> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
                "system time: {e}"
            ))))
        })?
        .as_secs_f64();

    // 1. DELETE rows past the age limit.
    if policy.max_age_seconds > 0 {
        let cutoff = now - policy.max_age_seconds as f64;
        conn.execute("DELETE FROM history WHERE timestamp <= ?1", [cutoff])?;
    }
    // 2. Cap to the newest `max_entries`.
    if policy.max_entries > 0 {
        conn.execute(
            "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY timestamp DESC LIMIT ?1)",
            [policy.max_entries],
        )?;
    }

    // 3. SELECT remaining. `-1` is SQLite's "no limit".
    let select_limit = if policy.max_entries > 0 {
        policy.max_entries
    } else {
        -1
    };
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, text, raw_text, formatted_text, language, inference_time_ms, \
         ai_processing_json, processing_stats_json, system_prompt, transcription_model, length \
         FROM history ORDER BY timestamp DESC LIMIT ?1",
    )?;
    let entries = stmt
        .query_map([select_limit], |r| {
            Ok(HistoryEntry {
                id: r.get::<_, i64>(0)? as u64,
                timestamp: r.get(1)?,
                text: r.get(2)?,
                raw_text: r.get(3)?,
                formatted_text: r.get(4)?,
                language: r.get(5)?,
                inference_time_ms: r.get::<_, Option<i64>>(6)?.map(|v| v as u64),
                ai_processing: r
                    .get::<_, Option<String>>(7)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                processing_stats: r
                    .get::<_, Option<String>>(8)?
                    .and_then(|s| serde_json::from_str(&s).ok()),
                system_prompt: r.get(9)?,
                transcription_model: r.get(10)?,
                length: r.get::<_, i64>(11)? as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(HistoryListResult {
        entries,
        max_age_seconds: policy.max_age_seconds.max(0) as u64,
        max_entries: policy.max_entries.max(0) as u32,
    })
}

pub fn delete_from(conn: &Connection, id: u64) -> Result<DeleteResult, rusqlite::Error> {
    let affected = conn.execute("DELETE FROM history WHERE id = ?1", [id as i64])?;
    Ok(DeleteResult {
        deleted: affected > 0,
    })
}

pub fn clear_from(conn: &Connection) -> Result<ClearResult, rusqlite::Error> {
    let affected = conn.execute("DELETE FROM history", [])?;
    Ok(ClearResult {
        deleted: affected as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_db() -> std::sync::Mutex<rusqlite::Connection> {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        std::sync::Mutex::new(conn)
    }

    // ------------------------------------------------------------------
    // Retention policy
    // ------------------------------------------------------------------

    #[test]
    fn default_retention_is_a_month() {
        let policy = RetentionPolicy::from_config(&serde_json::json!({}));
        assert_eq!(policy.max_age_seconds, 30 * 24 * 60 * 60);
        assert_eq!(policy.max_entries, 1000);
    }

    #[test]
    fn retention_reads_config() {
        let policy = RetentionPolicy::from_config(&serde_json::json!({
            "history_retention_days": 7,
            "history_max_entries": 200,
        }));
        assert_eq!(policy.max_age_seconds, 7 * 24 * 60 * 60);
        assert_eq!(policy.max_entries, 200);
    }

    #[test]
    fn zero_means_unlimited_not_empty() {
        // The dangerous misreading: "0 days" deleting everything on the next
        // History page visit.
        let policy = RetentionPolicy::from_config(&serde_json::json!({
            "history_retention_days": 0,
            "history_max_entries": 0,
        }));
        assert_eq!(policy.max_age_seconds, 0);
        assert_eq!(policy.max_entries, 0);

        let db = fresh_db();
        append(&db, "keep me", Some(1), None, 10, 1.0).unwrap();
        let list = list_history_from(&db.lock().unwrap(), policy).unwrap();
        assert_eq!(list.entries.len(), 1);
    }

    #[test]
    fn nonsense_retention_falls_back_to_default() {
        let default = RetentionPolicy::default();
        for bad in [
            serde_json::json!({ "history_retention_days": -1 }),
            serde_json::json!({ "history_retention_days": 100_000 }),
            serde_json::json!({ "history_retention_days": "месяц" }),
            serde_json::json!({ "history_max_entries": -5 }),
        ] {
            assert_eq!(RetentionPolicy::from_config(&bad), default, "{bad}");
        }
    }

    #[test]
    fn entry_cap_prunes_oldest() {
        let db = fresh_db();
        for i in 0..5 {
            append(&db, &format!("entry {i}"), Some(i), None, 10, 1.0).unwrap();
        }
        let policy = RetentionPolicy {
            max_age_seconds: 0,
            max_entries: 2,
        };
        let list = list_history_from(&db.lock().unwrap(), policy).unwrap();
        assert_eq!(list.entries.len(), 2);
        assert_eq!(list.entries[0].text, "entry 4");
        // Pruning is physical: a later listing with a looser policy must not
        // bring the deleted rows back.
        let all = list_history_from(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
        assert_eq!(all.entries.len(), 2);
    }

    #[test]
    fn age_cutoff_keeps_recent_and_drops_stale() {
        let db = fresh_db();
        let id = append(&db, "recent", Some(1), None, 10, 1.0).unwrap();
        {
            // Backdate a second row past a one-day cutoff.
            let conn = db.lock().unwrap();
            let stale_ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs_f64()
                - 2.0 * SECONDS_PER_DAY as f64;
            conn.execute(
                "INSERT INTO history (id, timestamp, text, raw_text, formatted_text, length) \
                 VALUES (?1, ?2, 'stale', '', '', 5)",
                rusqlite::params![1i64, stale_ts],
            )
            .unwrap();
        }
        let policy = RetentionPolicy {
            max_age_seconds: SECONDS_PER_DAY,
            max_entries: 0,
        };
        let list = list_history_from(&db.lock().unwrap(), policy).unwrap();
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].id, id);
    }

    #[test]
    fn list_result_reports_the_policy_in_force() {
        // The History page renders these as "хранится N дней / M записей".
        let db = fresh_db();
        let policy = RetentionPolicy {
            max_age_seconds: 3 * SECONDS_PER_DAY,
            max_entries: 42,
        };
        let list = list_history_from(&db.lock().unwrap(), policy).unwrap();
        assert_eq!(list.max_age_seconds, 3 * 24 * 60 * 60);
        assert_eq!(list.max_entries, 42);
    }

    #[test]
    fn append_then_list_returns_entry() {
        let db = fresh_db();
        let id = append(&db, "hello", Some(1), Some("en"), 250, 4.0).unwrap();
        let list = list_history_from(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].id, id);
        assert_eq!(list.entries[0].text, "hello");
        assert_eq!(list.entries[0].language.as_deref(), Some("en"));
        assert_eq!(list.entries[0].length, 5);
        // processing_stats carries the audio + whisper timing shown in the UI.
        let stats = list.entries[0].processing_stats.as_ref().unwrap();
        assert_eq!(stats["audio_seconds"], serde_json::json!(4.0));
        assert_eq!(stats["whisper_seconds"], serde_json::json!(0.25));
    }

    #[test]
    fn append_entry_persists_all_text_stages_and_ai_metadata() {
        // The live dictation path stores raw whisper, the pre-LLM text, and
        // the final LLM text so the history UI can diff them and show the
        // before/after blocks. Pin that all four columns round-trip.
        let db = fresh_db();
        let ai_json = r#"{"attempted":true,"used":true,"provider":"anthropic"}"#;
        let stats_json = r#"{"audio_seconds":4.0,"whisper_seconds":0.25,"llm_seconds":0.5}"#;
        let id = append_entry(
            &db,
            &NewEntry {
                text: "Привет, как дела?",
                raw_text: "привет как дела",
                formatted_text: "Привет как дела",
                session_id: Some(7),
                language: Some("ru"),
                inference_time_ms: 250,
                ai_processing_json: Some(ai_json),
                processing_stats_json: Some(stats_json),
                system_prompt: Some("Ты редактор диктовки."),
                transcription_model: Some("gigaam-v3"),
            },
        )
        .unwrap();
        let list = list_history_from(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
        let entry = list.entries.iter().find(|e| e.id == id).unwrap();
        assert_eq!(entry.text, "Привет, как дела?");
        assert_eq!(entry.raw_text, "привет как дела");
        assert_eq!(entry.formatted_text, "Привет как дела");
        assert_eq!(entry.transcription_model.as_deref(), Some("gigaam-v3"));
        assert_eq!(
            entry.system_prompt.as_deref(),
            Some("Ты редактор диктовки.")
        );
        assert_eq!(entry.inference_time_ms, Some(250));
        assert_eq!(
            entry.ai_processing.as_ref().unwrap()["used"],
            serde_json::json!(true)
        );
        assert_eq!(
            entry.processing_stats.as_ref().unwrap()["llm_seconds"],
            serde_json::json!(0.5)
        );
    }

    #[test]
    fn list_prunes_entries_older_than_max_age() {
        let db = fresh_db();
        let stale_id = 1_000_000_i64; // year 1970
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO history (id, timestamp, text, length) VALUES (?1, 0.0, 'old', 3)",
                rusqlite::params![stale_id],
            )
            .unwrap();
        let id = append(&db, "fresh", Some(1), None, 100, 2.0).unwrap();
        let list = list_history_from(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
        assert_eq!(list.entries.len(), 1, "stale entry should be pruned");
        assert_eq!(list.entries[0].id, id);
        // Stale entry physically deleted.
        let count: i64 = db
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM history WHERE id = ?1",
                rusqlite::params![stale_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0, "stale entry must be physically deleted");
    }

    #[test]
    fn delete_history_entry_returns_deleted_true() {
        let db = fresh_db();
        let id = append(&db, "test", Some(1), None, 100, 2.0).unwrap();
        let result = delete_from(&db.lock().unwrap(), id).unwrap();
        assert!(result.deleted);
        // Second delete returns false.
        let result2 = delete_from(&db.lock().unwrap(), id).unwrap();
        assert!(!result2.deleted);
    }

    #[test]
    fn append_collision_in_same_ms_yields_unique_ids() {
        // IMPORTANT-5: two appends in the same millisecond must NOT collide.
        // We force the collision by pre-seeding two rows with the same id
        // we expect (timestamp*1000) to fall back to.
        let db = fresh_db();
        // Force a collision: seed an entry at id 1_700_000_000_000 (approx
        // 2023-11-14). Then monkey-patch SystemTime... actually, we can't
        // easily. So we directly test the collision logic by inserting two
        // rows with deliberately-equal timestamps and verifying both end up
        // with unique ids.
        let base_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let base_id = (base_ts * 1000.0) as i64;
        // Pre-seed with the EXACT id our first append will compute.
        db.lock()
            .unwrap()
            .execute(
                "INSERT INTO history (id, timestamp, text, length) VALUES (?1, ?2, 'pre', 3)",
                rusqlite::params![base_id, base_ts],
            )
            .unwrap();
        // Now append — should detect collision and bump id by 1.
        let new_id = append(&db, "after", None, None, 0, 0.0).unwrap();
        assert_ne!(new_id as i64, base_id, "must skip past the pre-seeded id");
        assert_eq!(new_id as i64, base_id + 1);
        let count: i64 = db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2, "both rows should be present");
    }

    #[test]
    fn clear_history_removes_all_entries() {
        let db = fresh_db();
        append(&db, "a", None, None, 0, 0.0).unwrap();
        append(&db, "b", None, None, 0, 0.0).unwrap();
        let result = clear_from(&db.lock().unwrap()).unwrap();
        assert_eq!(result.deleted, 2);
        let list = list_history_from(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
        assert_eq!(list.entries.len(), 0);
    }

    // ------------------------------------------------------------------
    // Collision retry and update semantics
    // ------------------------------------------------------------------

    #[test]
    fn collision_retry_bumps_id_forward_not_backward() {
        // Deterministic version of `append_collision_in_same_ms_yields_unique_ids`:
        // seed the exact id we pass in, then verify the retry loop bumps the
        // id FORWARD by one and never re-inserts over the pre-seeded row.
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, length) VALUES (?1, 0.0, 'pre', 3)",
            rusqlite::params![1_700_000_000_000i64],
        )
        .unwrap();
        let id = insert_entry_with_collision_retry(
            &conn,
            1_700_000_000_000,
            0.0,
            &NewEntry {
                text: "after",
                ..Default::default()
            },
            5,
        )
        .unwrap();
        assert_eq!(
            id, 1_700_000_000_001u64,
            "collision must bump the id forward by exactly one, never backward"
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM history WHERE id = ?1",
                rusqlite::params![1_700_000_000_000i64],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "pre-seeded row must still exist");
    }

    #[test]
    fn max_entries_zero_means_unlimited() {
        let db = fresh_db();
        for i in 0..5 {
            append(&db, &format!("entry {i}"), Some(i), None, 10, 1.0).unwrap();
        }
        let unlimited = RetentionPolicy {
            max_age_seconds: 0,
            max_entries: 0,
        };
        let list = list_history_from(&db.lock().unwrap(), unlimited).unwrap();
        assert_eq!(list.entries.len(), 5, "max_entries = 0 must mean unlimited");
        let capped = RetentionPolicy {
            max_age_seconds: 0,
            max_entries: 2,
        };
        let capped_list = list_history_from(&db.lock().unwrap(), capped).unwrap();
        assert_eq!(
            capped_list.entries.len(),
            2,
            "max_entries = 2 must cap the listing to the two newest"
        );
    }
}
