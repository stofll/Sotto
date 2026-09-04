//! Stats persistence (WS 4b).
//!
//! Replaces Python `stats.py`. Single source of truth for daily + totals
//! in `~/.speech_to_text/sotto.db` (mirror Python `stats.py` schema 1:1).
//!
//! Writes are serialized via `std::sync::Mutex<Connection>` (NOT
//! `tokio::sync::Mutex` — `MutexGuard` from std is `Send` when `T: Send`,
//! which is what `spawn_blocking` requires; the Tokio variant is not).

use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;

/// Fallback typing speed (chars per minute) used when no config is
/// available. Matches Python `config.py` `typing_speed_cpm` default.
///
/// `pub` so the dispatcher in `lib.rs::setup` can use the same constant
/// when calling `record_transcription` (WS 4b doesn't yet plumb the live
/// config value — that's deferred to WS 4c).
pub const TIME_SAVED_CPM_FALLBACK: f64 = 240.0;

/// Days of daily_history retained in DB (mirror Python `HISTORY_DAYS = 365`).
const DAILY_RETENTION_DAYS: i64 = 365;

#[derive(Debug, Clone, Serialize, Default)]
pub struct StatsResult {
    #[serde(rename = "total_transcriptions")]
    pub total_transcriptions: u64,
    pub total_characters: u64,
    pub total_time_saved_seconds: f64,
    pub total_audio_seconds: f64,
    pub total_processing_seconds: f64,
    pub total_whisper_seconds: f64,
    pub total_format_seconds: f64,
    pub total_llm_seconds: f64,
    pub total_llm_attempts: u64,
    pub total_llm_used: u64,
    pub total_llm_fallbacks: u64,
    pub total_llm_input_tokens: u64,
    pub total_llm_output_tokens: u64,
    pub total_llm_tokens: u64,
    pub total_replacement_applications: u64,
    pub daily_history: Vec<DailyEntry>,
    /// Why the LLM step fell back, most frequent first. Empty when it never
    /// has. See [`record_ai_outcome`].
    pub llm_fallback_reasons: Vec<LlmFallbackReason>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LlmFallbackReason {
    /// `provider_timeout`, `rate_limit`, `auth_error`, … — see
    /// `ai::step::SKIPPED_REASON_BY_ERROR_TYPE`.
    pub error_type: String,
    /// `0` when the provider never answered (timeout, connection refused).
    pub http_status: u16,
    pub count: u64,
    /// Most recent day this reason occurred, `YYYY-MM-DD`.
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DailyEntry {
    pub date: String,
    pub count: u64,
    pub chars: u64,
    #[serde(rename = "time_saved_seconds")]
    pub time_saved_seconds: f64,
    pub audio_seconds: f64,
    pub processing_seconds: f64,
    pub whisper_seconds: f64,
    pub format_seconds: f64,
    pub llm_seconds: f64,
    pub llm_attempts: u64,
    pub llm_used: u64,
    pub llm_fallbacks: u64,
    pub llm_input_tokens: u64,
    pub llm_output_tokens: u64,
    pub llm_tokens: u64,
    pub replacement_applications: u64,
}

/// Increment stats for a successful transcription.
///
/// `cpm` — typing speed in chars/minute. WS 4c will pass the live config
/// value here; until then we fall back to `TIME_SAVED_CPM_FALLBACK` so
/// `time_saved` math matches Python `stats.py:119` exactly:
///
/// ```text
///     time_saved_seconds = chars * 60 / cpm
/// ```
///
/// NOTE: do NOT use `chars/5 / (cpm/60)` — that would be off by 5x.
pub fn record_transcription(
    db: &Mutex<Connection>,
    text: &str,
    language: Option<&str>,
    inference_time_ms: u64,
    audio_seconds: f64,
    cpm: f64,
) -> Result<(), rusqlite::Error> {
    let conn = db.lock().unwrap();
    let chars = text.chars().count() as i64;
    let inference_seconds = inference_time_ms as f64 / 1000.0;
    // Guard against NaN/negative durations sneaking into the aggregate.
    let audio_seconds = if audio_seconds.is_finite() && audio_seconds > 0.0 {
        audio_seconds
    } else {
        0.0
    };

    // Mirror Python `stats.py:119`: time_saved = char_count / typing_speed_cpm * 60.
    // Caller-supplied cpm wins; we fall back to the constant if zero/invalid.
    let effective_cpm = if cpm > 0.0 {
        cpm
    } else {
        TIME_SAVED_CPM_FALLBACK
    };
    let time_saved = chars as f64 * 60.0 / effective_cpm;

    let today = chrono_today();

    let tx = conn.unchecked_transaction()?;

    upsert_total(&tx, "total_transcriptions", 1.0)?;
    upsert_total(&tx, "total_characters", chars as f64)?;
    upsert_total(&tx, "total_time_saved_seconds", time_saved)?;
    upsert_total(&tx, "total_processing_seconds", inference_seconds)?;
    upsert_total(&tx, "total_whisper_seconds", inference_seconds)?;
    upsert_total(&tx, "total_audio_seconds", audio_seconds)?;

    // Today's daily row — INSERT, then UPDATE if it already existed.
    // Using `INSERT OR IGNORE` then `UPDATE ... WHERE date=?1` keeps the
    // SQL portable (avoids UPSERT / `ON CONFLICT` syntax variance).
    tx.execute(
        "INSERT OR IGNORE INTO stats_daily (date, count) VALUES (?1, 0)",
        rusqlite::params![today],
    )?;
    tx.execute(
        "UPDATE stats_daily SET \
         count = count + 1, \
         chars = chars + ?1, \
         time_saved_seconds = time_saved_seconds + ?2, \
         processing_seconds = processing_seconds + ?3, \
         whisper_seconds = whisper_seconds + ?3, \
         audio_seconds = audio_seconds + ?4 \
         WHERE date = ?5",
        rusqlite::params![chars, time_saved, inference_seconds, audio_seconds, today],
    )?;

    // Retention: delete rows older than DAILY_RETENTION_DAYS days.
    // Use 'YYYY-MM-DD' string compare — `date()` SQL function gives today.
    tx.execute(
        "DELETE FROM stats_daily WHERE date < date('now', ?1)",
        rusqlite::params![format!("-{} days", DAILY_RETENTION_DAYS)],
    )?;

    tx.commit()?;
    let _ = language; // unused at WS 4b; WS 4c will persist language stats
    Ok(())
}

/// Record what the LLM step did with one transcription.
///
/// Until this existed the `llm_*` columns were written by exactly one thing:
/// the one-off import from the Python version. Everything the Rust pipeline
/// did was invisible in the aggregate, so "the LLM falls back 7% of the
/// time" was a statement about a program that no longer runs.
///
/// Failures are also broken down by reason in `llm_fallback_reasons`, which
/// is the part that answers *whose* problem it is: `provider_timeout` and
/// `rate_limit` say the provider, `auth_error` and `bad_response` say us.
pub fn record_ai_outcome(
    db: &Mutex<Connection>,
    status: &crate::ai::step::AiStatus,
) -> Result<(), rusqlite::Error> {
    if !status.attempted {
        return Ok(());
    }
    let conn = db.lock().unwrap();
    let today = chrono_today();
    let used = i64::from(status.used);
    let fallback = i64::from(status.fallback);
    let (input_tokens, output_tokens, total_tokens) = match &status.usage {
        Some(usage) => (
            usage.input_tokens as i64,
            usage.output_tokens as i64,
            usage.total_tokens as i64,
        ),
        None => (0, 0, 0),
    };

    let tx = conn.unchecked_transaction()?;

    upsert_total(&tx, "total_llm_attempts", 1.0)?;
    upsert_total(&tx, "total_llm_used", used as f64)?;
    upsert_total(&tx, "total_llm_fallbacks", fallback as f64)?;
    upsert_total(&tx, "total_llm_seconds", status.elapsed_seconds)?;
    upsert_total(&tx, "total_llm_input_tokens", input_tokens as f64)?;
    upsert_total(&tx, "total_llm_output_tokens", output_tokens as f64)?;
    upsert_total(&tx, "total_llm_tokens", total_tokens as f64)?;

    tx.execute(
        "INSERT OR IGNORE INTO stats_daily (date, count) VALUES (?1, 0)",
        rusqlite::params![today],
    )?;
    tx.execute(
        "UPDATE stats_daily SET \
         llm_attempts = llm_attempts + 1, \
         llm_used = llm_used + ?1, \
         llm_fallbacks = llm_fallbacks + ?2, \
         llm_seconds = llm_seconds + ?3, \
         llm_input_tokens = llm_input_tokens + ?4, \
         llm_output_tokens = llm_output_tokens + ?5, \
         llm_tokens = llm_tokens + ?6 \
         WHERE date = ?7",
        rusqlite::params![
            used,
            fallback,
            status.elapsed_seconds,
            input_tokens,
            output_tokens,
            total_tokens,
            today
        ],
    )?;

    if status.fallback {
        // `skipped_reason` is the classified verdict, `error_type` the raw
        // provider category; prefer whichever is present so an unclassified
        // failure still lands somewhere other than "unknown".
        let reason = first_non_empty(&[
            status.skipped_reason.as_str(),
            status.error_type.as_deref().unwrap_or_default(),
        ])
        .unwrap_or("unknown");
        let http_status = status.http_status.unwrap_or(0) as i64;
        // No provider message column: a provider message can echo the prompt,
        // and with it the transcription, so only the classified reason and the
        // status code are persisted. The legacy `last_error` column stays in
        // the schema on its NOT NULL DEFAULT so the migration history is left
        // alone; nothing writes or reads it any more.
        tx.execute(
            "INSERT INTO llm_fallback_reasons (date, error_type, http_status, count) \
             VALUES (?1, ?2, ?3, 1) \
             ON CONFLICT(date, error_type, http_status) DO UPDATE SET \
             count = count + 1",
            rusqlite::params![today, reason, http_status],
        )?;
        tx.execute(
            "DELETE FROM llm_fallback_reasons WHERE date < date('now', ?1)",
            rusqlite::params![format!("-{} days", DAILY_RETENTION_DAYS)],
        )?;
    }

    tx.commit()?;
    Ok(())
}

fn first_non_empty<'a>(candidates: &[&'a str]) -> Option<&'a str> {
    candidates.iter().copied().find(|s| !s.is_empty())
}

fn upsert_total(conn: &Connection, key: &str, delta: f64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO stats_totals (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = value + ?2",
        rusqlite::params![key, delta],
    )?;
    Ok(())
}

/// Lifetime counter ← the daily column it must never fall below.
const TOTAL_TO_DAILY_COLUMN: &[(&str, &str)] = &[
    ("total_transcriptions", "count"),
    ("total_characters", "chars"),
    ("total_time_saved_seconds", "time_saved_seconds"),
    ("total_audio_seconds", "audio_seconds"),
    ("total_processing_seconds", "processing_seconds"),
    ("total_whisper_seconds", "whisper_seconds"),
    ("total_format_seconds", "format_seconds"),
    ("total_llm_seconds", "llm_seconds"),
    ("total_llm_attempts", "llm_attempts"),
    ("total_llm_used", "llm_used"),
    ("total_llm_fallbacks", "llm_fallbacks"),
    ("total_llm_input_tokens", "llm_input_tokens"),
    ("total_llm_output_tokens", "llm_output_tokens"),
    ("total_llm_tokens", "llm_tokens"),
    ("total_replacement_applications", "replacement_applications"),
];

/// Repair lifetime counters that the repeated `stats.json` import rolled back.
///
/// A lifetime total can legitimately EXCEED the sum of retained daily rows —
/// `stats_daily` keeps a year, the counters keep everything. It can never be
/// smaller: every write bumps both in one transaction. Where it is smaller, the
/// counter was overwritten by the legacy import (see `db::migrate_from_json`),
/// and the daily sum is the better of the two numbers we have.
///
/// Only ever raises a counter, so a user with more than a year of history keeps
/// their older total intact. Runs once at startup; a no-op on healthy data.
pub fn reconcile_totals_with_daily(conn: &Connection) -> Result<usize, rusqlite::Error> {
    let mut repaired = 0usize;
    for (total_key, daily_column) in TOTAL_TO_DAILY_COLUMN {
        let daily_sum: f64 = conn.query_row(
            &format!("SELECT COALESCE(SUM({daily_column}), 0) FROM stats_daily"),
            [],
            |row| row.get(0),
        )?;
        let stored: f64 = conn
            .query_row(
                "SELECT value FROM stats_totals WHERE key = ?1",
                rusqlite::params![total_key],
                |row| row.get(0),
            )
            .unwrap_or(0.0);
        if daily_sum > stored {
            conn.execute(
                "INSERT OR REPLACE INTO stats_totals (key, value) VALUES (?1, ?2)",
                rusqlite::params![total_key, daily_sum],
            )?;
            log::info!("stats: {total_key} raised {stored} → {daily_sum} from stats_daily");
            repaired += 1;
        }
    }
    Ok(repaired)
}

/// Query all stats — pivot KV → flat struct.
///
/// Caller holds `&Connection` (lock already acquired).
pub fn get_stats_from(conn: &Connection) -> Result<StatsResult, rusqlite::Error> {
    let mut result = StatsResult::default();

    // Pivot KV into flat struct.
    let mut stmt = conn.prepare("SELECT key, value FROM stats_totals")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?)))?;
    for row in rows {
        let (key, value) = row?;
        assign_total(&mut result, &key, value);
    }

    // Daily history — 16 columns, newest first, capped at retention window.
    let mut stmt = conn.prepare(
        "SELECT date, count, chars, time_saved_seconds, audio_seconds, processing_seconds, \
         whisper_seconds, format_seconds, llm_seconds, llm_attempts, llm_used, llm_fallbacks, \
         llm_input_tokens, llm_output_tokens, llm_tokens, replacement_applications \
         FROM stats_daily ORDER BY date DESC LIMIT ?1",
    )?;
    let daily_rows = stmt.query_map([DAILY_RETENTION_DAYS], |r| {
        Ok(DailyEntry {
            date: r.get(0)?,
            count: r.get::<_, i64>(1)? as u64,
            chars: r.get::<_, i64>(2)? as u64,
            time_saved_seconds: r.get(3)?,
            audio_seconds: r.get(4)?,
            processing_seconds: r.get(5)?,
            whisper_seconds: r.get(6)?,
            format_seconds: r.get(7)?,
            llm_seconds: r.get(8)?,
            llm_attempts: r.get::<_, i64>(9)? as u64,
            llm_used: r.get::<_, i64>(10)? as u64,
            llm_fallbacks: r.get::<_, i64>(11)? as u64,
            llm_input_tokens: r.get::<_, i64>(12)? as u64,
            llm_output_tokens: r.get::<_, i64>(13)? as u64,
            llm_tokens: r.get::<_, i64>(14)? as u64,
            replacement_applications: r.get::<_, i64>(15)? as u64,
        })
    })?;
    for row in daily_rows {
        result.daily_history.push(row?);
    }

    // Fallback reasons, summed across days: the question is "what keeps
    // failing", not "what failed last Tuesday". `last_seen` keeps the
    // per-day detail that matters — whether it is still happening.
    let mut stmt = conn.prepare(
        "SELECT error_type, http_status, SUM(count), MAX(date) \
         FROM llm_fallback_reasons \
         GROUP BY error_type, http_status ORDER BY SUM(count) DESC",
    )?;
    let reason_rows = stmt.query_map([], |r| {
        Ok(LlmFallbackReason {
            error_type: r.get(0)?,
            http_status: r.get::<_, i64>(1)? as u16,
            count: r.get::<_, i64>(2)? as u64,
            last_seen: r.get(3)?,
        })
    })?;
    for row in reason_rows {
        result.llm_fallback_reasons.push(row?);
    }
    Ok(result)
}

fn assign_total(result: &mut StatsResult, key: &str, value: f64) {
    match key {
        "total_transcriptions" => result.total_transcriptions = value as u64,
        "total_characters" => result.total_characters = value as u64,
        "total_time_saved_seconds" => result.total_time_saved_seconds = value,
        "total_audio_seconds" => result.total_audio_seconds = value,
        "total_processing_seconds" => result.total_processing_seconds = value,
        "total_whisper_seconds" => result.total_whisper_seconds = value,
        "total_format_seconds" => result.total_format_seconds = value,
        "total_llm_seconds" => result.total_llm_seconds = value,
        "total_llm_attempts" => result.total_llm_attempts = value as u64,
        "total_llm_used" => result.total_llm_used = value as u64,
        "total_llm_fallbacks" => result.total_llm_fallbacks = value as u64,
        "total_llm_input_tokens" => result.total_llm_input_tokens = value as u64,
        "total_llm_output_tokens" => result.total_llm_output_tokens = value as u64,
        "total_llm_tokens" => result.total_llm_tokens = value as u64,
        "total_replacement_applications" => result.total_replacement_applications = value as u64,
        _ => log::warn!("unknown stats_totals key: {key}"),
    }
}

/// Returns today's date in `YYYY-MM-DD` using the LOCAL timezone.
///
/// Mirror of Python `date.today()` from the `datetime` stdlib module —
/// which uses local time, not UTC. Critical for stats correctness: a
/// user recording at 23:00 UTC+5 would otherwise see their entry
/// attributed to the wrong day once UTC rolls over.
///
/// Implementation: `libc::localtime_r` on unix, days-since-epoch UTC +
/// civil_from_days algorithm on Windows (no `localtime_r` there).
pub fn chrono_today() -> String {
    #[cfg(unix)]
    {
        let now = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        // localtime_r takes a pointer to time_t and a pointer to tm;
        // returns null on failure (returns the same pointer on success).
        let result = unsafe { libc::localtime_r(&now, &mut tm) };
        if result.is_null() {
            // Fall back to UTC if localtime_r fails (e.g. invalid time).
            log::warn!("localtime_r failed; falling back to UTC");
            return utc_today_str();
        }
        format!(
            "{:04}-{:02}-{:02}",
            tm.tm_year + 1900,
            tm.tm_mon + 1,
            tm.tm_mday
        )
    }
    #[cfg(not(unix))]
    {
        // Windows fallback: UTC. Acceptable because Windows users are
        // a minority of the audience and we never want to crash startup.
        utc_today_str()
    }
}

fn utc_today_str() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (y, m, d) = days_to_ymd(days);
    format!("{:04}-{:02}-{:02}", y, m, d)
}

/// Convert days-since-1970-01-01 → (year, month, day) using Howard
/// Hinnant's `civil_from_days` algorithm. Public for testing.
pub fn days_to_ymd(days_since_epoch: i64) -> (i32, u32, u32) {
    // Shift epoch from 1970-01-01 to 0000-03-01 (Hinnant's civil_from_days).
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    (y as i32, m as u32, d as u32)
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
    // LLM outcome recording
    // ------------------------------------------------------------------

    fn ai_status(attempted: bool, used: bool, fallback: bool) -> crate::ai::step::AiStatus {
        crate::ai::step::AiStatus {
            mode: "hybrid".into(),
            provider: "cerebras".into(),
            model: "test".into(),
            profile_id: String::new(),
            profile_name: String::new(),
            api_key_ref: String::new(),
            audio_duration_seconds: Some(10.0),
            min_duration_seconds: 0.0,
            enabled: true,
            attempted,
            used,
            fallback,
            skipped_reason: String::new(),
            timeout_seconds: 12,
            attempt_timeout_seconds: 4,
            attempts: 1,
            elapsed_seconds: 1.5,
            usage: None,
            error_type: None,
            provider_error: None,
            http_status: None,
            response_snippet: None,
            output_length: None,
            provider_attempts: Vec::new(),
        }
    }

    #[test]
    fn ai_outcome_counts_attempts_and_uses() {
        let db = fresh_db();
        record_ai_outcome(&db, &ai_status(true, true, false)).unwrap();
        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        assert_eq!(stats.total_llm_attempts, 1);
        assert_eq!(stats.total_llm_used, 1);
        assert_eq!(stats.total_llm_fallbacks, 0);
        assert!((stats.total_llm_seconds - 1.5).abs() < 1e-6);
        assert_eq!(stats.daily_history[0].llm_attempts, 1);
        assert_eq!(stats.daily_history[0].llm_used, 1);
        assert!(stats.llm_fallback_reasons.is_empty());
    }

    #[test]
    fn skipped_llm_records_nothing() {
        // The min-duration gate: not an attempt, so it must not dilute the
        // fallback rate.
        let db = fresh_db();
        record_ai_outcome(&db, &ai_status(false, false, false)).unwrap();
        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        assert_eq!(stats.total_llm_attempts, 0);
        assert!(stats.daily_history.is_empty());
    }

    #[test]
    fn fallback_is_recorded_with_its_reason() {
        let db = fresh_db();
        let mut status = ai_status(true, false, true);
        status.skipped_reason = "provider_timeout".into();
        status.http_status = Some(504);
        status.provider_error = Some("upstream timed out".into());
        record_ai_outcome(&db, &status).unwrap();
        record_ai_outcome(&db, &status).unwrap();

        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        assert_eq!(stats.total_llm_fallbacks, 2);
        assert_eq!(stats.llm_fallback_reasons.len(), 1);
        let reason = &stats.llm_fallback_reasons[0];
        assert_eq!(reason.error_type, "provider_timeout");
        assert_eq!(reason.http_status, 504);
        assert_eq!(reason.count, 2);
        assert_eq!(reason.last_seen, chrono_today());
    }

    #[test]
    fn reasons_are_ranked_by_frequency() {
        let db = fresh_db();
        let mut rare = ai_status(true, false, true);
        rare.skipped_reason = "provider_auth_error".into();
        rare.http_status = Some(401);
        let mut common = ai_status(true, false, true);
        common.skipped_reason = "rate_limit".into();
        common.http_status = Some(429);

        record_ai_outcome(&db, &rare).unwrap();
        for _ in 0..3 {
            record_ai_outcome(&db, &common).unwrap();
        }

        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        let reasons: Vec<(&str, u64)> = stats
            .llm_fallback_reasons
            .iter()
            .map(|r| (r.error_type.as_str(), r.count))
            .collect();
        assert_eq!(reasons, [("rate_limit", 3), ("provider_auth_error", 1)]);
    }

    #[test]
    fn unclassified_failure_falls_back_to_error_type_then_unknown() {
        let db = fresh_db();
        // No skipped_reason, but the provider category is known.
        let mut typed = ai_status(true, false, true);
        typed.error_type = Some("bad_response".into());
        record_ai_outcome(&db, &typed).unwrap();
        // Neither is set — must still be counted, not dropped.
        record_ai_outcome(&db, &ai_status(true, false, true)).unwrap();

        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        let mut kinds: Vec<&str> = stats
            .llm_fallback_reasons
            .iter()
            .map(|r| r.error_type.as_str())
            .collect();
        kinds.sort_unstable();
        assert_eq!(kinds, ["bad_response", "unknown"]);
    }

    #[test]
    fn timeout_without_a_response_uses_status_zero() {
        // NULL would make the upsert key distinct every time and turn the
        // aggregate into an append-only log.
        let db = fresh_db();
        let mut status = ai_status(true, false, true);
        status.skipped_reason = "provider_connection_error".into();
        record_ai_outcome(&db, &status).unwrap();
        record_ai_outcome(&db, &status).unwrap();

        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        assert_eq!(stats.llm_fallback_reasons.len(), 1);
        assert_eq!(stats.llm_fallback_reasons[0].http_status, 0);
        assert_eq!(stats.llm_fallback_reasons[0].count, 2);
    }

    #[test]
    fn provider_errors_never_reach_the_database() {
        // Asserted against the row, not against a struct field: a provider
        // message can echo the prompt and with it the transcription, so the
        // guarantee has to hold for every column of this table — including
        // the legacy `last_error` one that is now left on its default.
        let db = fresh_db();
        let mut status = ai_status(true, false, true);
        status.skipped_reason = "provider_bad_response".into();
        status.provider_error = Some("ECHOED-TRANSCRIPT-MARKER".into());
        record_ai_outcome(&db, &status).unwrap();

        let conn = db.lock().unwrap();
        let dumped: String = conn
            .query_row(
                "SELECT group_concat(date || '|' || error_type || '|' || last_error, '~') \
                 FROM llm_fallback_reasons",
                [],
                |r| r.get::<_, Option<String>>(0),
            )
            .unwrap()
            .unwrap_or_default();
        assert!(!dumped.is_empty(), "no fallback row was written at all");
        assert!(
            !dumped.contains("ECHOED-TRANSCRIPT-MARKER"),
            "provider message reached the stats table: {dumped}"
        );
    }

    #[test]
    fn tokens_are_summed_when_the_provider_reports_them() {
        let db = fresh_db();
        let mut status = ai_status(true, true, false);
        status.usage = Some(crate::ai::providers::UsageInfo {
            input_tokens: 120,
            output_tokens: 30,
            total_tokens: 150,
        });
        record_ai_outcome(&db, &status).unwrap();
        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        assert_eq!(stats.total_llm_input_tokens, 120);
        assert_eq!(stats.total_llm_output_tokens, 30);
        assert_eq!(stats.total_llm_tokens, 150);
        assert_eq!(stats.daily_history[0].llm_tokens, 150);
    }

    #[test]
    fn reconcile_raises_a_total_that_was_rolled_back() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO stats_daily (date, count, chars) VALUES ('2026-08-01', 1027, 296950)",
            [],
        )
        .unwrap();
        // The value a stale stats.json kept restoring.
        conn.execute(
            "INSERT INTO stats_totals (key, value) VALUES ('total_transcriptions', 997)",
            [],
        )
        .unwrap();

        assert!(reconcile_totals_with_daily(&conn).unwrap() > 0);

        let total: f64 = conn
            .query_row(
                "SELECT value FROM stats_totals WHERE key = 'total_transcriptions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 1027.0);
    }

    /// `stats_daily` holds a year, the counters hold everything — a total above
    /// the daily sum is normal on an old install and must survive untouched.
    #[test]
    fn reconcile_never_lowers_a_total() {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO stats_daily (date, count) VALUES ('2026-08-01', 10)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO stats_totals (key, value) VALUES ('total_transcriptions', 5000)",
            [],
        )
        .unwrap();

        assert_eq!(reconcile_totals_with_daily(&conn).unwrap(), 0);

        let total: f64 = conn
            .query_row(
                "SELECT value FROM stats_totals WHERE key = 'total_transcriptions'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(total, 5000.0);
    }

    #[test]
    fn record_transcription_increments_totals_and_daily() {
        let db = fresh_db();
        record_transcription(&db, "hello world", Some("en"), 250, 5.0, 240.0).unwrap();
        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        assert_eq!(stats.total_transcriptions, 1);
        assert_eq!(stats.total_characters, 11);
        assert!(
            (stats.total_audio_seconds - 5.0).abs() < 1e-6,
            "total_audio_seconds should record the clip duration (got {})",
            stats.total_audio_seconds
        );
        assert!((stats.daily_history[0].audio_seconds - 5.0).abs() < 1e-6);
        assert!(
            stats.total_time_saved_seconds > 0.0,
            "time_saved should be positive (got {})",
            stats.total_time_saved_seconds
        );
        assert_eq!(stats.daily_history.len(), 1);
        assert_eq!(stats.daily_history[0].count, 1);
    }

    #[test]
    fn record_transcription_uses_cpm_parameter() {
        // time_saved = chars * 60 / cpm. For 11 chars at cpm=60:
        //   11 * 60 / 60 = 11.0 seconds.
        // This validates the cpm parameter is actually plumbed through
        // (NOT hardcoded to the fallback constant).
        let db = fresh_db();
        record_transcription(&db, "hello world", None, 100, 0.0, 60.0).unwrap();
        let stats = get_stats_from(&db.lock().unwrap()).unwrap();
        assert!(
            (stats.total_time_saved_seconds - 11.0).abs() < 1e-6,
            "expected ~11.0s, got {}",
            stats.total_time_saved_seconds
        );
    }

    #[test]
    fn record_transcription_retention_keeps_365_days() {
        let db = fresh_db();
        // Insert 400 daily rows.
        for i in 0..400 {
            let date = format!("2024-01-{:03}", (i % 365) + 1);
            let _ = db.lock().unwrap().execute(
                "INSERT OR REPLACE INTO stats_daily (date, count) VALUES (?1, 1)",
                rusqlite::params![date],
            );
        }
        record_transcription(&db, "test", None, 100, 1.0, 240.0).unwrap();
        let count: i64 = db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM stats_daily", [], |r| r.get(0))
            .unwrap();
        assert!(
            count <= 366,
            "retention should keep ≤ 365 days + today (got {count})"
        );
    }

    #[test]
    fn chrono_today_matches_system_local_date() {
        // chrono_today() must return YYYY-MM-DD format and be a sane date
        // (year >= 2025, month 1-12, day 1-31). We can't easily verify
        // timezone correctness without timezone control, but the format
        // is verifiable on every platform.
        let s = chrono_today();
        assert_eq!(s.len(), 10);
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        let year: i32 = s[..4].parse().unwrap();
        let month: u32 = s[5..7].parse().unwrap();
        let day: u32 = s[8..10].parse().unwrap();
        assert!((2025..=2100).contains(&year), "year out of range: {year}");
        assert!((1..=12).contains(&month), "month out of range: {month}");
        assert!((1..=31).contains(&day), "day out of range: {day}");
    }

    #[test]
    fn days_to_ymd_known_dates() {
        // days=0 → 1970-01-01 (unix epoch)
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        // days=20000 → 2024-10-04 (sanity check, leap year safe)
        assert_eq!(days_to_ymd(20_000), (2024, 10, 4));
        // days=20512 → 2026-02-28 (end of Feb 2026, non-leap year)
        assert_eq!(days_to_ymd(20_512), (2026, 2, 28));
        // days=1 → 1970-01-02
        assert_eq!(days_to_ymd(1), (1970, 1, 2));
        // Leap day: 2024-02-29 = days=19782
        // (1970-01-01 to 2024-01-01 = 19723 days; 2024-02-29 = +59 days = 19782)
        assert_eq!(days_to_ymd(19_782), (2024, 2, 29));
    }
}
