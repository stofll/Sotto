//! History storage queries and retention. Independent of windows and native APIs.

use rusqlite::{
    functions::FunctionFlags, params_from_iter, types::Value as SqlValue, Connection,
    OptionalExtension,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Config key: how many days of history to keep. `0` means "no age limit".
const CONFIG_RETENTION_DAYS: &str = "history_retention_days";
/// Config key: hard cap on stored entries. `0` means "no count limit".
const CONFIG_MAX_ENTRIES: &str = "history_max_entries";

pub(super) const SECONDS_PER_DAY: i64 = 24 * 60 * 60;
/// A month of history. Long enough to answer "what did I dictate last
/// week?", short enough that the table stays small.
const DEFAULT_RETENTION_DAYS: i64 = 30;
/// Sized against the default retention: at ~20 dictations a day, a month is
/// roughly 600 entries, so this cap does not quietly override the age limit.
const DEFAULT_MAX_ENTRIES: i64 = 1000;
/// Ten years. Not a meaningful limit, just a guard against a hand-edited
/// config producing an absurd cutoff.
const MAX_RETENTION_DAYS: i64 = 3650;
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

    /// Whether a settings patch sets anything [`Self::from_config`] reads.
    pub fn is_changed_by(patch: &Value) -> bool {
        patch.get(CONFIG_RETENTION_DAYS).is_some() || patch.get(CONFIG_MAX_ENTRIES).is_some()
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
    /// Whether the dictation's saved recording is still on disk. The file
    /// name itself stays on this side of the bridge.
    #[serde(rename = "has_recording")]
    pub has_recording: bool,
    #[serde(skip)]
    pub recording_file: Option<String>,
}

const DEFAULT_PAGE_SIZE: u32 = 50;
const MAX_PAGE_SIZE: u32 = 100;

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoryStatus {
    #[default]
    All,
    Processed,
    Fallback,
    Skipped,
}

impl HistoryStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Processed => "processed",
            Self::Fallback => "fallback",
            Self::Skipped => "skipped",
        }
    }
}

/// Filters apply to the retained history before selecting a page. `ids` is
/// used by an explicit bulk-copy action; normal browsing never requests it.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HistoryQuery {
    pub page: u64,
    pub page_size: u32,
    pub query: String,
    pub status: HistoryStatus,
    /// An epoch cutoff computed by the UI, independent of the backend timezone.
    pub since: Option<f64>,
    pub focus_id: Option<u64>,
    pub ids: Option<Vec<u64>>,
    /// Selection / playback / open panels survive page changes, but not expiry.
    pub known_ids: Vec<u64>,
}

impl Default for HistoryQuery {
    fn default() -> Self {
        Self {
            page: 0,
            page_size: DEFAULT_PAGE_SIZE,
            query: String::new(),
            status: HistoryStatus::All,
            since: None,
            focus_id: None,
            ids: None,
            known_ids: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct HistoryPageResult {
    pub entries: Vec<HistoryEntry>,
    pub max_age_seconds: u64,
    pub max_entries: u32,
    pub total_count: u64,
    pub filtered_count: u64,
    pub page: u64,
    pub page_size: u32,
    /// Which requested ids still belong to the retained history, before filters.
    pub known_ids: Vec<u64>,
}

/// SQLite's built-in lower/LIKE only fold ASCII. Keep literal Unicode substring
/// search (including Russian) and the UI's LLM status precedence on the worker.
fn register_matcher(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.create_scalar_function(
        "sotto_history_matches",
        8,
        FunctionFlags::SQLITE_UTF8
            | FunctionFlags::SQLITE_DETERMINISTIC
            | FunctionFlags::SQLITE_INNOCUOUS,
        |ctx| {
            let query = ctx.get_raw(0).as_str()?;
            let status = ctx.get_raw(1).as_str()?;
            if status != "all" {
                let ai = ctx
                    .get_raw(7)
                    .as_str()
                    .ok()
                    .and_then(|raw| serde_json::from_str::<Value>(raw).ok());
                let flag = |name: &str| {
                    ai.as_ref()
                        .and_then(|value| value.get(name))
                        .and_then(Value::as_bool)
                        .unwrap_or(false)
                };
                let kind = if flag("attempted") && flag("used") {
                    "processed"
                } else if flag("attempted") && flag("fallback") {
                    "fallback"
                } else {
                    "skipped"
                };
                if status != kind {
                    return Ok(false);
                }
            }
            if query.is_empty() {
                return Ok(true);
            }
            let mut text = String::new();
            for index in 2..7 {
                if let Ok(field) = ctx.get_raw(index).as_str() {
                    if field.is_empty() {
                        continue;
                    }
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(field);
                }
            }
            Ok(text.to_lowercase().contains(query))
        },
    )
}

/// At most one page crosses the IPC boundary. Counts use the same retention
/// bounds and filters; only returned rows parse the large pipeline JSON fields.
pub fn list_history_page_from(
    conn: &Connection,
    policy: RetentionPolicy,
    query: &HistoryQuery,
) -> Result<HistoryPageResult, rusqlite::Error> {
    // A single history page performs multiple SQL statements (retention tail,
    // counts, focus position, entries, and selected IDs). On a separate WAL
    // reader, a writer may commit between them. Keep all values on the same
    // read snapshot without changing the caller's transaction state.
    let savepoint = conn.unchecked_transaction()?;
    let result = list_history_page_in_snapshot(&savepoint, policy, query)?;
    savepoint.commit()?;
    Ok(result)
}

fn list_history_page_in_snapshot(
    conn: &Connection,
    policy: RetentionPolicy,
    query: &HistoryQuery,
) -> Result<HistoryPageResult, rusqlite::Error> {
    let cutoff = if policy.max_age_seconds > 0 {
        unix_now()? - policy.max_age_seconds as f64
    } else {
        f64::MIN
    };
    // Resolve the retention cap on the timestamp index, before search/status
    // filters. Filtering first could resurrect rows outside the retained set.
    let tail: Option<(f64, i64)> = if policy.max_entries > 0 {
        conn.query_row(
            "SELECT timestamp, id FROM history WHERE timestamp > ?1 ORDER BY timestamp DESC, id DESC LIMIT 1 OFFSET ?2",
            rusqlite::params![cutoff, policy.max_entries - 1],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional()?
    } else {
        None
    };
    let (floor, floor_id) = tail.unwrap_or((cutoff, i64::MIN));
    let base = "timestamp > ?1 AND timestamp >= ?2 AND (timestamp > ?2 OR id >= ?3)";
    let base_params = vec![
        SqlValue::Real(cutoff),
        SqlValue::Real(floor),
        SqlValue::Integer(floor_id),
    ];
    let total_count: u64 = conn.query_row(
        &format!("SELECT COUNT(*) FROM history WHERE {base}"),
        params_from_iter(base_params.iter()),
        |row| row.get::<_, i64>(0).map(|count| count as u64),
    )?;
    let mut filters = base.to_owned();
    let mut params = base_params.clone();
    if let Some(since) = query.since.filter(|value| value.is_finite()) {
        params.push(SqlValue::Real(since));
        filters.push_str(&format!(" AND timestamp >= ?{}", params.len()));
    }
    if let Some(ids) = &query.ids {
        params.push(SqlValue::Text(serde_json::json!(ids).to_string()));
        filters.push_str(&format!(
            " AND id IN (SELECT value FROM json_each(?{}))",
            params.len()
        ));
    }
    let text = query.query.trim().to_lowercase();
    if !text.is_empty() || !matches!(query.status, HistoryStatus::All) {
        register_matcher(conn)?;
        params.push(SqlValue::Text(text));
        let text_parameter = params.len();
        params.push(SqlValue::Text(query.status.as_str().to_owned()));
        filters.push_str(&format!(" AND sotto_history_matches(?{text_parameter}, ?{}, text, formatted_text, raw_text, system_prompt, transcription_model, ai_processing_json)", params.len()));
    }
    let filtered_count: u64 = if params.len() == base_params.len() {
        total_count
    } else {
        conn.query_row(
            &format!("SELECT COUNT(*) FROM history WHERE {filters}"),
            params_from_iter(params.iter()),
            |row| row.get::<_, i64>(0).map(|count| count as u64),
        )?
    };
    let size = query.page_size.clamp(1, MAX_PAGE_SIZE);
    let mut page = query
        .page
        .min(filtered_count.saturating_sub(1) / u64::from(size));
    if let Some(id) = query.focus_id.and_then(|value| i64::try_from(value).ok()) {
        let mut focus_params = params.clone();
        focus_params.push(SqlValue::Integer(id));
        let position: Option<u64> = conn.query_row(
            &format!("SELECT position FROM (SELECT id, row_number() OVER (ORDER BY timestamp DESC, id DESC) - 1 AS position FROM history WHERE {filters}) WHERE id = ?{}", focus_params.len()),
            params_from_iter(focus_params.iter()), |row| row.get::<_, i64>(0).map(|position| position as u64),
        ).optional()?;
        if let Some(position) = position {
            page = position / u64::from(size);
        }
    }
    params.push(SqlValue::Integer(i64::from(size)));
    let size_parameter = params.len();
    params.push(SqlValue::Integer((page * u64::from(size)) as i64));
    let entries = conn.prepare(&format!(
        "SELECT {ENTRY_COLUMNS} FROM history WHERE {filters} ORDER BY timestamp DESC, id DESC LIMIT ?{size_parameter} OFFSET ?{}", params.len(),
    ))?.query_map(params_from_iter(params.iter()), entry_from_row)?.collect::<Result<Vec<_>, _>>()?;
    let known_ids = if query.known_ids.is_empty() {
        Vec::new()
    } else {
        let mut known_params = base_params;
        known_params.push(SqlValue::Text(
            serde_json::json!(&query.known_ids).to_string(),
        ));
        conn.prepare(&format!(
            "SELECT id FROM history WHERE {base} AND id IN (SELECT value FROM json_each(?4))"
        ))?
        .query_map(params_from_iter(known_params.iter()), |row| {
            row.get::<_, i64>(0).map(|id| id as u64)
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    Ok(HistoryPageResult {
        entries,
        total_count,
        filtered_count,
        page,
        page_size: size,
        known_ids,
        max_age_seconds: policy.max_age_seconds.max(0) as u64,
        max_entries: policy.max_entries.max(0) as u32,
    })
}

pub(super) fn unix_now() -> Result<f64, rusqlite::Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
                "system time: {e}"
            ))))
        })
}

pub(super) const ENTRY_COLUMNS: &str = "id, timestamp, text, raw_text, formatted_text, language, \
     inference_time_ms, ai_processing_json, processing_stats_json, system_prompt, \
     transcription_model, length, recording_file";

/// One row selected with [`ENTRY_COLUMNS`]. Recording availability is added
/// outside the database query, after releasing its connection.
pub(super) fn entry_from_row(r: &rusqlite::Row) -> Result<HistoryEntry, rusqlite::Error> {
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
        has_recording: false,
        recording_file: r.get(12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(include_str!("../migrations/v1.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../migrations/v3.sql"))
            .unwrap();
        conn.execute_batch(include_str!("../migrations/v11.sql"))
            .unwrap();
        conn
    }

    fn insert(conn: &Connection, id: i64, timestamp: f64, text: &str, ai: Option<&str>) {
        conn.execute(
            "INSERT INTO history (id, timestamp, text, length, ai_processing_json) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, timestamp, text, text.chars().count() as i64, ai],
        ).unwrap();
    }

    fn unlimited() -> RetentionPolicy {
        RetentionPolicy {
            max_age_seconds: 0,
            max_entries: 0,
        }
    }

    fn ids(result: &HistoryPageResult) -> Vec<u64> {
        result.entries.iter().map(|entry| entry.id).collect()
    }

    #[test]
    fn history_page_does_not_leave_a_transaction_open() {
        let conn = database();
        insert(&conn, 1, 1.0, "entry", None);
        let page = list_history_page_from(&conn, unlimited(), &HistoryQuery::default()).unwrap();
        assert_eq!(page.total_count, 1);
        assert!(conn.is_autocommit());
        insert(&conn, 2, 2.0, "another", None);
        assert_eq!(
            list_history_page_from(&conn, unlimited(), &HistoryQuery::default())
                .unwrap()
                .total_count,
            2
        );
        assert!(conn.is_autocommit());
    }

    #[test]
    fn pages_are_bounded_stably_ordered_and_clamp_after_deleting_the_last_page() {
        let conn = database();
        for id in 1..=125 {
            insert(&conn, id, (id / 2) as f64, "entry", None);
        }
        let first = list_history_page_from(&conn, unlimited(), &HistoryQuery::default()).unwrap();
        assert_eq!(first.total_count, 125);
        assert_eq!(first.filtered_count, 125);
        assert_eq!(ids(&first), (76..=125).rev().collect::<Vec<_>>());
        let query = HistoryQuery {
            page: 999,
            ..HistoryQuery::default()
        };
        let last = list_history_page_from(&conn, unlimited(), &query).unwrap();
        assert_eq!(last.page, 2);
        assert_eq!(ids(&last), (1..=25).rev().collect::<Vec<_>>());
        conn.execute("DELETE FROM history WHERE id <= 25", [])
            .unwrap();
        let last = list_history_page_from(&conn, unlimited(), &query).unwrap();
        assert_eq!(last.page, 1);
        assert_eq!(last.total_count, 100);
        assert_eq!(ids(&last), (26..=75).rev().collect::<Vec<_>>());
        let large = list_history_page_from(
            &conn,
            unlimited(),
            &HistoryQuery {
                page_size: u32::MAX,
                ..HistoryQuery::default()
            },
        )
        .unwrap();
        assert_eq!(large.page_size, 100);
        assert_eq!(large.entries.len(), 100);
    }

    #[test]
    fn substring_search_is_unicode_literal_and_covers_every_stored_text_stage() {
        let conn = database();
        for id in 1..=5 {
            insert(&conn, id, id as f64, "entry", None);
        }
        conn.execute("UPDATE history SET text = 'ПрИвЕт Мир' WHERE id = 1", [])
            .unwrap();
        conn.execute(
            "UPDATE history SET formatted_text = 'тест_100%' WHERE id = 2",
            [],
        )
        .unwrap();
        conn.execute("UPDATE history SET raw_text = 'СЕКРЕТ' WHERE id = 3", [])
            .unwrap();
        conn.execute(
            "UPDATE history SET system_prompt = 'Особый запрос' WHERE id = 4",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE history SET transcription_model = 'GigaAM-v3' WHERE id = 5",
            [],
        )
        .unwrap();
        for (query, id) in [
            ("  привет  ", 1),
            ("%", 2),
            ("_", 2),
            ("секрет", 3),
            ("entry\nтест_100%", 2),
            ("entry\nсекрет", 3),
            ("ОСОБЫЙ", 4),
            ("gigaam", 5),
        ] {
            let result = list_history_page_from(
                &conn,
                unlimited(),
                &HistoryQuery {
                    query: query.into(),
                    ..HistoryQuery::default()
                },
            )
            .unwrap();
            assert_eq!(result.total_count, 5);
            assert_eq!(result.filtered_count, 1, "{query}");
            assert_eq!(ids(&result), vec![id], "{query}");
        }
    }

    #[test]
    fn status_filters_match_the_ui_precedence_and_include_legacy_rows_without_llm() {
        let conn = database();
        let states = [
            None,
            Some("{}"),
            Some("invalid JSON"),
            Some(r#"{"attempted":true,"used":true,"fallback":true}"#),
            Some(r#"{"attempted":true,"fallback":true}"#),
            Some(r#"{"attempted":false,"used":true}"#),
        ];
        for (index, state) in states.iter().enumerate() {
            insert(&conn, index as i64 + 1, index as f64, "entry", *state);
        }
        for (status, expected) in [
            (HistoryStatus::Processed, vec![4]),
            (HistoryStatus::Fallback, vec![5]),
            (HistoryStatus::Skipped, vec![6, 3, 2, 1]),
        ] {
            let result = list_history_page_from(
                &conn,
                unlimited(),
                &HistoryQuery {
                    status,
                    ..HistoryQuery::default()
                },
            )
            .unwrap();
            assert_eq!(result.total_count, 6);
            assert_eq!(ids(&result), expected);
        }
    }

    #[test]
    fn retention_precedes_filters_and_known_ids_are_checked_before_search() {
        let conn = database();
        for id in 1..=5 {
            insert(
                &conn,
                id,
                id as f64,
                if id == 1 { "needle" } else { "other" },
                None,
            );
        }
        let result = list_history_page_from(
            &conn,
            RetentionPolicy {
                max_age_seconds: 0,
                max_entries: 2,
            },
            &HistoryQuery {
                query: "needle".into(),
                known_ids: vec![1, 4, 5, 999],
                ..HistoryQuery::default()
            },
        )
        .unwrap();
        assert_eq!(result.total_count, 2);
        assert_eq!(result.filtered_count, 0);
        assert!(result.entries.is_empty());
        assert_eq!(
            result
                .known_ids
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            [4, 5].into_iter().collect()
        );
    }

    #[test]
    fn epoch_date_filter_is_inclusive_and_independent_of_machine_timezone() {
        let conn = database();
        for (id, timestamp) in [(1, 4999.999), (2, 5000.0), (3, 5000.001)] {
            insert(&conn, id, timestamp, "entry", None);
        }
        let result = list_history_page_from(
            &conn,
            unlimited(),
            &HistoryQuery {
                since: Some(5000.0),
                ..HistoryQuery::default()
            },
        )
        .unwrap();
        assert_eq!(result.total_count, 3);
        assert_eq!(ids(&result), vec![3, 2]);
    }

    #[test]
    fn focus_finds_its_page_and_selected_copy_pages_return_only_actual_retained_rows() {
        let conn = database();
        for id in 1..=125 {
            insert(&conn, id, id as f64, "entry", None);
        }
        let focused = list_history_page_from(
            &conn,
            unlimited(),
            &HistoryQuery {
                focus_id: Some(17),
                ..HistoryQuery::default()
            },
        )
        .unwrap();
        assert_eq!(focused.page, 2);
        assert!(ids(&focused).contains(&17));
        let selected = list_history_page_from(
            &conn,
            unlimited(),
            &HistoryQuery {
                ids: Some(vec![1, 51, 101, 999]),
                page_size: 2,
                page: 1,
                ..HistoryQuery::default()
            },
        )
        .unwrap();
        assert_eq!(selected.filtered_count, 3);
        assert_eq!(ids(&selected), vec![1]);
        let empty = list_history_page_from(
            &conn,
            unlimited(),
            &HistoryQuery {
                ids: Some(vec![]),
                ..HistoryQuery::default()
            },
        )
        .unwrap();
        assert_eq!(empty.total_count, 125);
        assert_eq!(empty.filtered_count, 0);
        assert!(empty.entries.is_empty());
    }
}
