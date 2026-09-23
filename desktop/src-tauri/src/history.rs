//! Dictation history and optional LLM reprocessing.
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
//! `ai_processing` / `processing_stats` containing the recorded pipeline metrics.

use std::sync::Mutex;

use rusqlite::Connection;
use serde::Serialize;
use serde_json::Value;
use tauri::AppHandle;

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
#[cfg(test)]
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
    let conn = crate::mutex_recover::lock(db);
    let timestamp = unix_now()?;
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

fn unix_now() -> Result<f64, rusqlite::Error> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs_f64())
        .map_err(|e| {
            rusqlite::Error::ToSqlConversionFailure(Box::new(std::io::Error::other(format!(
                "system time: {e}"
            ))))
        })
}

/// Physically delete the rows outside `policy`.
///
/// Runs after each new entry and at startup, so reading the history never
/// writes. Between those points a lowered setting already shows, because
/// the listing applies the same policy.
pub fn prune(conn: &Connection, policy: RetentionPolicy) -> Result<(), rusqlite::Error> {
    if policy.max_age_seconds > 0 {
        let cutoff = unix_now()? - policy.max_age_seconds as f64;
        conn.execute("DELETE FROM history WHERE timestamp <= ?1", [cutoff])?;
    }
    if policy.max_entries > 0 {
        conn.execute(
            "DELETE FROM history WHERE id NOT IN (SELECT id FROM history ORDER BY timestamp DESC LIMIT ?1)",
            [policy.max_entries],
        )?;
    }
    Ok(())
}

/// The entries `policy` keeps, newest first. Read-only; see [`prune`].
pub fn list_history_from(
    conn: &Connection,
    policy: RetentionPolicy,
) -> Result<HistoryListResult, rusqlite::Error> {
    let cutoff = if policy.max_age_seconds > 0 {
        unix_now()? - policy.max_age_seconds as f64
    } else {
        f64::MIN
    };
    // `-1` is SQLite's "no limit".
    let select_limit = if policy.max_entries > 0 {
        policy.max_entries
    } else {
        -1
    };
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, text, raw_text, formatted_text, language, inference_time_ms, \
         ai_processing_json, processing_stats_json, system_prompt, transcription_model, length \
         FROM history WHERE timestamp > ?1 ORDER BY timestamp DESC LIMIT ?2",
    )?;
    let entries = stmt
        .query_map(rusqlite::params![cutoff, select_limit], |r| {
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

#[tauri::command]
pub(crate) async fn list_history(
    app: AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<HistoryListResult, String> {
    // Read the retention policy before handing off to the blocking worker —
    // config access needs the AppHandle, which does not cross that boundary.
    let policy = crate::config::Config::load(&app)
        .map(|cfg| RetentionPolicy::from_config(cfg.as_value()))
        .unwrap_or_default();
    let db = state.db.clone();
    crate::run_db_op(db, move |conn| list_history_from(conn, policy)).await
}

#[tauri::command]
pub(crate) async fn delete_history_entry(
    state: tauri::State<'_, crate::state::AppState>,
    id: u64,
) -> Result<DeleteResult, String> {
    let db = state.db.clone();
    crate::run_db_op(db, move |conn| delete_from(conn, id)).await
}

#[tauri::command]
pub(crate) async fn clear_history(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<ClearResult, String> {
    let db = state.db.clone();
    crate::run_db_op(db, clear_from).await
}

/// Return shape for `apply_history_ai_processing`.
///
/// `{ updated: bool, entry?: HistoryEntry, reason?: string }`. The frontend
/// (`bridge/stats.ts`) pattern-matches on `updated + entry` to decide whether
/// to merge the updated row into local state, and shows `reason` when
/// `updated` is false. Whether the LLM produced anything is decided one step
/// earlier, by `preview_history_ai_processing`: nothing reaches the apply step
/// unless the user accepted a result.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct HistoryRetryAiResult {
    updated: bool,
    entry: Option<HistoryEntry>,
    reason: Option<String>,
}

/// Pipeline mode for processing a history entry by hand.
///
/// The mode decides whether a dictation is processed automatically, not whether
/// the user is allowed to process an entry by hand. Pressing «Обработать» in the
/// history is a direct instruction, and answering it with "local mode, LLM off"
/// means arguing with someone who has already said what they want. A provider
/// and a key are still required: without them the refusal is meaningful and
/// explainable.
fn manual_llm_mode(configured: &str) -> &str {
    if configured == "local" {
        "hybrid"
    } else {
        configured
    }
}

/// Run the LLM over an existing history entry without writing anything.
///
/// Phase 4 / PR-B — fully native Rust: reads the entry from the DB and calls
/// `crate::ai::ai_process_text_with_status` (the same orchestrator the
/// dispatcher uses for live transcriptions). Nothing is persisted here: the
/// history panel shows the result next to the current text, and only
/// `apply_history_ai_processing` writes it back.
///
/// `source_text` fallback: if `entry.formatted_text` is empty (migrated
/// legacy entries may not have a `formatted_text` field), fall back to
/// `entry.text` so we still have something to send to the LLM.
async fn run_history_entry_ai(
    state: &tauri::State<'_, crate::state::AppState>,
    app: &AppHandle,
    id: u64,
    profile_id: Option<String>,
    system_prompt: Option<String>,
) -> Result<
    (
        crate::ai::step::AiConfig,
        crate::ai::step::CallOutcome,
        String,
        String,
    ),
    String,
> {
    // 1. Read the entry from the DB.
    let db = state.db.clone();
    let entry = crate::run_db_op(db, move |conn| read_history_entry(conn, id))
        .await?
        .ok_or_else(|| format!("entry not found: {id}"))?;

    // 2. Source-text fallback (preserve existing behaviour).
    let source_text = if !entry.formatted_text.is_empty() {
        entry.formatted_text.clone()
    } else {
        entry.text.clone()
    };

    // 3. Load ai_processing config from disk. `from_ai_processing` mirrors
    //    the previous field-by-field extraction (no recording context, so
    //    the duration gate is skipped).
    let config = crate::config::Config::load(app)?;
    let ai = crate::ai_processing_config(&config)?;
    let mut ai_cfg = crate::ai::step::AiConfig::from_ai_processing(ai);
    ai_cfg.language = crate::speech_language(Some(&config));
    ai_cfg.pipeline_mode = manual_llm_mode(&ai_cfg.pipeline_mode).to_string();
    apply_ai_profile(&mut ai_cfg, ai, profile_id.as_deref())?;
    // Rust is handed finished prompt text here exactly as it is on the
    // dictation path — it knows nothing about presets. The caller resolves the
    // chosen profile's `prompt_preset` for us; nothing to resolve leaves the
    // profile's own prompt standing.
    if let Some(prompt) = system_prompt.filter(|value| !value.trim().is_empty()) {
        ai_cfg.system_prompt = prompt;
    }

    // 4. Look up the API key from the secret store.
    let api_key = if ai_cfg.api_key_ref.is_empty() {
        None
    } else {
        crate::secret_store::load_key(&ai_cfg.api_key_ref)
            .await
            .map_err(|e| format!("secret_store get_key({}): {e}", ai_cfg.api_key_ref))?
    };

    // 5. Call the Rust AI orchestrator (no Python subprocess).
    let outcome =
        crate::ai::ai_process_text_with_status(&source_text, &ai_cfg, api_key.as_deref()).await;

    // 6. Build both JSON columns in the shapes the live dispatcher writes
    //    (`ai_processing_json` = serialized AiStatus, `processing_stats_json`
    //    = timings). They are carried to the apply step as they are rather
    //    than rebuilt there: rebuilding means asking the model again, and the
    //    second answer is not the one the user accepted.
    let ai_json = crate::ai_processing_json(Some(&outcome.status))
        .ok_or_else(|| "serialize ai_processing".to_string())?;
    let stats_json = stats_with_llm_timing(
        entry.processing_stats.as_ref(),
        outcome.status.elapsed_seconds,
    );
    Ok((ai_cfg, outcome, ai_json, stats_json))
}

/// Overlay one saved AI profile onto the flat active `ai_processing` fields.
///
/// The flat fields describe the profile used for dictation; the history panel
/// lets a single entry be re-run through any other saved profile, and without
/// this the request would still go to the dictation one. `None` keeps the flat
/// fields and only fills in the profile identity that the entry badge shows —
/// `from_ai_processing` leaves it empty because the live path sets it itself.
fn apply_ai_profile(
    cfg: &mut crate::ai::step::AiConfig,
    ai: &Value,
    profile_id: Option<&str>,
) -> Result<(), String> {
    let str_field = |value: &Value, key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let Some(profile_id) = profile_id.filter(|id| !id.is_empty()) else {
        cfg.profile_id = str_field(ai, "profile_id");
        cfg.profile_name = str_field(ai, "profile_name");
        return Ok(());
    };
    let profile = ai
        .get("profiles")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|profile| profile.get("id").and_then(Value::as_str) == Some(profile_id))
        .ok_or_else(|| format!("unknown ai profile: {profile_id}"))?;

    cfg.provider = str_field(profile, "provider");
    cfg.model = str_field(profile, "model");
    cfg.api_key_ref = str_field(profile, "api_key_ref");
    cfg.profile_id = profile_id.to_string();
    cfg.profile_name = str_field(profile, "name");
    // Empty means "inherit" — but only from a config describing the same
    // provider. A base URL is the address of one provider's API: carrying LM
    // Studio's port over to an Anthropic profile does not leave the field
    // unset, it points the request at the wrong server, and
    // `AnthropicProvider::new` takes any `Some` in preference to its own
    // endpoint.
    let base_url = str_field(profile, "base_url");
    if !base_url.is_empty() {
        cfg.base_url = Some(base_url);
    } else if cfg.provider != str_field(ai, "provider") {
        cfg.base_url = None;
    }
    // The prompt is the one field a profile may legitimately leave empty while
    // still meaning something specific: empty says «use my `prompt_preset`»,
    // and the preset texts live in the frontend (`effectiveSystemPrompt`), so
    // the resolved text arrives alongside `profile_id` — see
    // `run_history_entry_ai`. What is inherited here is only the last resort.
    let system_prompt = str_field(profile, "system_prompt");
    if !system_prompt.is_empty() {
        cfg.system_prompt = system_prompt;
    }
    if let Some(timeout) = profile
        .get("llm_timeout_seconds")
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
    {
        cfg.llm_timeout_seconds = timeout;
    }
    Ok(())
}

/// A dry run of LLM processing over a history entry.
///
/// `ai_json` / `stats_json` are the two columns the write would need. They
/// travel back through `apply_history_ai_processing` unchanged so that what
/// ends up stored describes the run the user actually saw and accepted.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct HistoryAiPreview {
    ok: bool,
    text: String,
    reason: Option<String>,
    provider: String,
    model: String,
    profile_name: String,
    elapsed_seconds: f64,
    ai_json: String,
    stats_json: String,
}

/// Re-run the LLM over a history entry and return the result without storing
/// it. `profile_id` picks one of the saved AI profiles; `None` uses the one
/// configured for dictation. `system_prompt` is that profile's prompt already
/// resolved against its preset — the presets are the frontend's, and a profile
/// that never edited its own carries an empty `system_prompt` field.
#[tauri::command]
pub(crate) async fn preview_history_ai_processing(
    state: tauri::State<'_, crate::state::AppState>,
    app: AppHandle,
    id: u64,
    profile_id: Option<String>,
    system_prompt: Option<String>,
) -> Result<HistoryAiPreview, String> {
    use tauri::Manager;
    app.state::<crate::telemetry::Telemetry>()
        .begin_usage_session(crate::telemetry::SessionTrigger::Llm);
    let (ai_cfg, outcome, ai_json, stats_json) =
        run_history_entry_ai(&state, &app, id, profile_id, system_prompt).await?;
    Ok(HistoryAiPreview {
        ok: outcome.status.used,
        text: outcome.text,
        reason: retry_failure_reason(&outcome.status),
        provider: ai_cfg.provider,
        model: ai_cfg.model,
        profile_name: ai_cfg.profile_name,
        elapsed_seconds: outcome.status.elapsed_seconds,
        ai_json,
        stats_json,
    })
}

/// Store a previewed LLM result on its history entry.
///
/// The text goes in alongside the status of the run that produced it: a row
/// showing the new text under the old «fallback» badge would describe an
/// attempt that never happened.
#[tauri::command]
pub(crate) async fn apply_history_ai_processing(
    state: tauri::State<'_, crate::state::AppState>,
    id: u64,
    text: String,
    ai_json: String,
    stats_json: String,
) -> Result<HistoryRetryAiResult, String> {
    if text.trim().is_empty() {
        return Err("refusing to store an empty LLM result".to_string());
    }
    let db = state.db.clone();
    crate::run_db_op(db, move |conn| {
        update_entry_ai(conn, id, Some(text.as_str()), &ai_json, &stats_json)
    })
    .await?;

    let db = state.db.clone();
    let entry = crate::run_db_op(db, move |conn| read_history_entry(conn, id)).await?;
    Ok(HistoryRetryAiResult {
        updated: entry.is_some(),
        entry,
        reason: None,
    })
}

/// Replace the LLM leg of a row's `processing_stats` with a fresh
/// measurement, leaving every other timing intact.
///
/// A retry re-runs only the LLM: `audio_seconds`, `whisper_seconds` and
/// anything else on the row was measured when the recording happened and
/// is still true, so overwriting the whole object would throw away
/// numbers nothing can recompute. `total_seconds` is rebased off the old
/// LLM figure rather than summed from the individual legs, because not
/// all of them are enumerated here.
fn stats_with_llm_timing(existing: Option<&Value>, llm_seconds: f64) -> String {
    let mut stats = existing
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let num = |key: &str| stats.get(key).and_then(Value::as_f64).unwrap_or(0.0);
    let without_llm = (num("total_seconds") - num("llm_seconds")).max(0.0);
    stats.insert("llm_seconds".into(), serde_json::json!(llm_seconds));
    stats.insert(
        "total_seconds".into(),
        serde_json::json!(without_llm + llm_seconds),
    );
    Value::Object(stats).to_string()
}

/// Why a retry produced no new text, as the machine-readable code the
/// frontend already knows how to label.
///
/// Deliberately NOT a human sentence: `HistoryPage` owns the whole
/// vocabulary for these codes (`aiFallbackLabel` and the skip cases next
/// to it), and duplicating it here would give the same failure two
/// different wordings depending on which screen you looked at.
///
/// `skipped_reason` is always populated on failure — the provider error
/// paths map `error_type` into it via `skipped_reason_for` — so the
/// fallback only covers a status that failed without saying why.
fn retry_failure_reason(status: &crate::ai::step::AiStatus) -> Option<String> {
    if status.used {
        return None;
    }
    Some(status.skipped_reason.clone())
        .filter(|r| !r.trim().is_empty())
        .or_else(|| Some("unknown".to_string()))
}

/// Read a single history row by id (used by the manual LLM processing path
/// to fetch the source text and to re-fetch the row after the write).
fn read_history_entry(conn: &Connection, id: u64) -> Result<Option<HistoryEntry>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, text, raw_text, formatted_text, language, inference_time_ms, \
         ai_processing_json, processing_stats_json, system_prompt, transcription_model, length \
         FROM history WHERE id = ?1",
    )?;
    let mut rows = stmt.query([id as i64])?;
    if let Some(row) = rows.next()? {
        Ok(Some(HistoryEntry {
            id: row.get::<_, i64>(0)? as u64,
            timestamp: row.get(1)?,
            text: row.get(2)?,
            raw_text: row.get(3)?,
            formatted_text: row.get(4)?,
            language: row.get(5)?,
            inference_time_ms: row.get::<_, Option<i64>>(6)?.map(|v| v as u64),
            ai_processing: row
                .get::<_, Option<String>>(7)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            processing_stats: row
                .get::<_, Option<String>>(8)?
                .and_then(|s| serde_json::from_str(&s).ok()),
            system_prompt: row.get(9)?,
            transcription_model: row.get(10)?,
            length: row.get::<_, i64>(11)? as u32,
        }))
    } else {
        Ok(None)
    }
}

/// Write back the result of re-running the LLM over an existing history row.
///
/// `ai_json` and `ps_json` are always written: they describe the pass that
/// just ran, and that is true whether it succeeded or not. This matches
/// the live dispatcher, which records a failing `AiStatus` just as readily
/// as a successful one — and it is what lets the history UI explain a
/// failed retry, since it renders `provider_error` / `skipped_reason`
/// straight off this column.
///
/// `text` is `Some` only when there is new text to store — the apply step
/// passes the result the user accepted, and `None` leaves the transcript
/// alone while still recording what the run did. `length` moves with the
/// text: the column is what the list shows, and a stale count is a visible
/// lie.
fn update_entry_ai(
    conn: &Connection,
    id: u64,
    text: Option<&str>,
    ai_json: &str,
    ps_json: &str,
) -> Result<(), rusqlite::Error> {
    match text {
        Some(text) => conn.execute(
            "UPDATE history SET text = ?1, length = ?2, ai_processing_json = ?3, \
             processing_stats_json = ?4 WHERE id = ?5",
            rusqlite::params![
                text,
                text.chars().count() as i64,
                ai_json,
                ps_json,
                id as i64
            ],
        )?,
        None => conn.execute(
            "UPDATE history SET ai_processing_json = ?1, processing_stats_json = ?2 \
             WHERE id = ?3",
            rusqlite::params![ai_json, ps_json, id as i64],
        )?,
    };
    Ok(())
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
        // Listing only filters: a looser policy still sees every row.
        let all = list_history_from(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
        assert_eq!(all.entries.len(), 5);
        // Pruning is physical: afterwards the looser policy cannot bring
        // the deleted rows back.
        prune(&db.lock().unwrap(), policy).unwrap();
        let all = list_history_from(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
        assert_eq!(all.entries.len(), 2);
        assert_eq!(all.entries[1].text, "entry 3");
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
        prune(&db.lock().unwrap(), policy).unwrap();
        let count: i64 = db
            .lock()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
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
    fn entries_older_than_max_age_are_hidden_then_pruned() {
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
        assert_eq!(list.entries.len(), 1, "stale entry should be hidden");
        assert_eq!(list.entries[0].id, id);
        let stored = |db: &Mutex<Connection>| -> i64 {
            db.lock()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM history WHERE id = ?1",
                    rusqlite::params![stale_id],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(stored(&db), 1, "listing must not delete");
        prune(&db.lock().unwrap(), RetentionPolicy::default()).unwrap();
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

    /// The same poisoning guard as `stats::writes_survive_a_poisoned_
    /// connection_mutex`, for the other writer on the shared connection: a
    /// panic under the lock must not make every later transcription fail to
    /// reach the history.
    #[test]
    fn appends_survive_a_poisoned_connection_mutex() {
        let db = std::sync::Arc::new(fresh_db());
        let holder = std::sync::Arc::clone(&db);
        let joined = std::thread::spawn(move || {
            let _conn = holder.lock().unwrap();
            panic!("simulated panic while holding the history connection");
        })
        .join();
        assert!(joined.is_err(), "the holder thread should have panicked");
        assert!(db.is_poisoned(), "the connection mutex must be poisoned");

        let id = append(&db, "after the panic", Some(1), None, 100, 2.0)
            .expect("append after poisoning should succeed");
        let list = list_history_from(&crate::mutex_recover::lock(&db), RetentionPolicy::default())
            .unwrap();
        assert_eq!(list.entries.len(), 1);
        assert_eq!(list.entries[0].id, id);
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

#[cfg(test)]
mod manual_llm_mode_tests {
    use super::manual_llm_mode;

    /// This case is the reason the function exists: in "local" mode the
    /// «Обработать» button in the history must still work.
    #[test]
    fn local_mode_still_allows_a_manual_run() {
        assert_eq!(manual_llm_mode("local"), "hybrid");
    }

    /// The other modes are left alone: the LLM is already permitted in them, and
    /// substituting "hybrid" for "cloud" would change the report of what
    /// actually happened.
    #[test]
    fn the_other_modes_are_passed_through_untouched() {
        assert_eq!(manual_llm_mode("hybrid"), "hybrid");
        assert_eq!(manual_llm_mode("cloud"), "cloud");
    }
}

#[cfg(test)]
mod retry_ai_tests {
    use super::*;
    use crate::ai::step::AiStatus;

    fn status(used: bool, skipped_reason: &str) -> AiStatus {
        let mut status = AiStatus {
            telemetry_service: None,
            mode: "hybrid".to_string(),
            provider: "compatible".to_string(),
            model: "some-model".to_string(),
            profile_id: String::new(),
            profile_name: String::new(),
            api_key_ref: "key_x".to_string(),
            audio_duration_seconds: None,
            min_duration_seconds: 0.0,
            enabled: true,
            attempted: used,
            used,
            fallback: false,
            skipped_reason: skipped_reason.to_string(),
            timeout_seconds: 12,
            attempt_timeout_seconds: 4,
            attempts: u32::from(used),
            elapsed_seconds: 2.5,
            usage: None,
            error_type: None,
            provider_error: None,
            http_status: None,
            response_snippet: None,
            output_length: None,
            provider_attempts: Vec::new(),
        };
        status.output_length = used.then_some(10);
        status
    }

    fn conn_with_row(processing_stats: Option<&str>) -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO history (id, timestamp, text, raw_text, formatted_text, length, \
             ai_processing_json, processing_stats_json) \
             VALUES (1, 0.0, 'старый текст', 'raw', 'formatted', 12, '{\"used\":true}', ?1)",
            rusqlite::params![processing_stats],
        )
        .unwrap();
        conn
    }

    /// The bug this whole path was fixed for: the retry used to write
    /// `{"text": …}` here while the live dispatcher wrote a serialized
    /// `AiStatus`, so a retried row rendered as never processed.
    #[test]
    fn ai_processing_json_is_the_serialized_status() {
        let json = crate::ai_processing_json(Some(&status(true, ""))).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["used"], serde_json::json!(true));
        assert_eq!(parsed["attempted"], serde_json::json!(true));
        assert_eq!(parsed["provider"], serde_json::json!("compatible"));
        assert!(parsed.get("text").is_none());
    }

    #[test]
    fn ai_processing_json_is_none_without_a_status() {
        assert!(crate::ai_processing_json(None).is_none());
    }

    #[test]
    fn stats_keep_recording_time_measurements_and_rebase_the_total() {
        let existing = serde_json::json!({
            "audio_seconds": 19.2,
            "whisper_seconds": 0.5,
            "llm_seconds": 3.0,
            "total_seconds": 3.5,
        });
        let merged: Value =
            serde_json::from_str(&stats_with_llm_timing(Some(&existing), 8.0)).unwrap();
        // Measured once, at recording time — a retry cannot re-measure them.
        assert_eq!(merged["audio_seconds"], serde_json::json!(19.2));
        assert_eq!(merged["whisper_seconds"], serde_json::json!(0.5));
        // Only the LLM leg is replaced, and the total follows it.
        assert_eq!(merged["llm_seconds"], serde_json::json!(8.0));
        assert_eq!(merged["total_seconds"], serde_json::json!(8.5));
    }

    #[test]
    fn stats_survive_a_row_that_has_none() {
        let merged: Value = serde_json::from_str(&stats_with_llm_timing(None, 8.0)).unwrap();
        assert_eq!(merged["llm_seconds"], serde_json::json!(8.0));
        assert_eq!(merged["total_seconds"], serde_json::json!(8.0));
    }

    /// A malformed `total_seconds` must not produce a negative one.
    #[test]
    fn stats_never_rebase_below_zero() {
        let existing = serde_json::json!({ "llm_seconds": 9.0, "total_seconds": 1.0 });
        let merged: Value =
            serde_json::from_str(&stats_with_llm_timing(Some(&existing), 2.0)).unwrap();
        assert_eq!(merged["total_seconds"], serde_json::json!(2.0));
    }

    #[test]
    fn a_successful_pass_has_no_failure_reason() {
        assert!(retry_failure_reason(&status(true, "")).is_none());
    }

    #[test]
    fn a_skipped_pass_reports_its_code() {
        assert_eq!(
            retry_failure_reason(&status(false, "missing_api_key")),
            Some("missing_api_key".to_string())
        );
    }

    /// Silence is the one thing the caller must never get back: the
    /// frontend shows an error whenever `updated` is false, and an empty
    /// reason there is what made the button look like a no-op.
    #[test]
    fn a_failure_without_a_code_still_reports_something() {
        assert_eq!(
            retry_failure_reason(&status(false, "   ")),
            Some("unknown".to_string())
        );
    }

    /// The config a panel run starts from: flat active fields plus two saved
    /// profiles, one of which overrides nothing but the model.
    fn ai_processing_with_profiles() -> Value {
        serde_json::json!({
            "provider": "compatible",
            "model": "qwen3-27b",
            "api_key_ref": "slot-voice",
            "base_url": "http://localhost:1234/v1",
            "system_prompt": "почини пунктуацию",
            "llm_timeout_seconds": 12,
            "profile_id": "voice",
            "profile_name": "Диктовка",
            "profiles": [
                {
                    "id": "voice",
                    "name": "Диктовка",
                    "provider": "compatible",
                    "model": "qwen3-27b",
                    "api_key_ref": "slot-voice",
                },
                {
                    "id": "precise",
                    "name": "Точный",
                    "provider": "anthropic",
                    "model": "claude-opus-5",
                    "api_key_ref": "slot-precise",
                    "system_prompt": "перепиши аккуратно",
                    "llm_timeout_seconds": 60,
                },
            ],
        })
    }

    fn ai_config_from(ai: &Value) -> crate::ai::step::AiConfig {
        crate::ai::step::AiConfig::from_ai_processing(ai)
    }

    #[test]
    fn no_profile_keeps_the_dictation_target_and_names_it() {
        let ai = ai_processing_with_profiles();
        let mut cfg = ai_config_from(&ai);
        apply_ai_profile(&mut cfg, &ai, None).unwrap();
        assert_eq!(cfg.provider, "compatible");
        assert_eq!(cfg.model, "qwen3-27b");
        // `from_ai_processing` leaves the identity empty; without this the
        // entry badge would lose the profile name on every manual run.
        assert_eq!(cfg.profile_id, "voice");
        assert_eq!(cfg.profile_name, "Диктовка");
    }

    #[test]
    fn a_chosen_profile_redirects_the_request() {
        let ai = ai_processing_with_profiles();
        let mut cfg = ai_config_from(&ai);
        apply_ai_profile(&mut cfg, &ai, Some("precise")).unwrap();
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.model, "claude-opus-5");
        assert_eq!(cfg.api_key_ref, "slot-precise");
        assert_eq!(cfg.profile_name, "Точный");
        assert_eq!(cfg.system_prompt, "перепиши аккуратно");
        assert_eq!(cfg.llm_timeout_seconds, 60);
    }

    /// Switching provider drops an inherited base URL. The flat one is LM
    /// Studio's port; leaving it in place would send the Anthropic request to
    /// `http://localhost:1234/v1/messages` instead of the provider's endpoint.
    #[test]
    fn a_chosen_profile_does_not_inherit_another_providers_base_url() {
        let ai = ai_processing_with_profiles();
        let mut cfg = ai_config_from(&ai);
        apply_ai_profile(&mut cfg, &ai, Some("precise")).unwrap();
        assert_eq!(cfg.base_url, None);
    }

    /// A profile that never overrode the prompt or the base URL inherits
    /// them; blanking them out would send a bare request to nowhere.
    #[test]
    fn a_profile_without_overrides_inherits_prompt_and_base_url() {
        let ai = ai_processing_with_profiles();
        let mut cfg = ai_config_from(&ai);
        apply_ai_profile(&mut cfg, &ai, Some("voice")).unwrap();
        assert_eq!(cfg.system_prompt, "почини пунктуацию");
        assert_eq!(cfg.base_url.as_deref(), Some("http://localhost:1234/v1"));
        assert_eq!(cfg.llm_timeout_seconds, 12);
    }

    /// A stale id from a deleted profile must fail loudly rather than
    /// silently sending the text to the dictation profile.
    #[test]
    fn an_unknown_profile_is_an_error() {
        let ai = ai_processing_with_profiles();
        let mut cfg = ai_config_from(&ai);
        assert!(apply_ai_profile(&mut cfg, &ai, Some("deleted")).is_err());
    }

    #[test]
    fn a_successful_pass_replaces_the_text_and_its_length() {
        let conn = conn_with_row(None);
        update_entry_ai(
            &conn,
            1,
            Some("новый текст подлиннее"),
            "{\"used\":true}",
            "{}",
        )
        .unwrap();
        let (text, length): (String, i64) = conn
            .query_row("SELECT text, length FROM history WHERE id = 1", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(text, "новый текст подлиннее");
        // Characters, not bytes — the text is Cyrillic.
        assert_eq!(length, 21);
    }

    /// A failed pass still records what happened, but must not touch the
    /// text: the last good result is what the user keeps.
    #[test]
    fn a_failed_pass_records_the_status_without_touching_the_text() {
        let conn = conn_with_row(None);
        update_entry_ai(&conn, 1, None, "{\"used\":false}", "{\"llm_seconds\":1.0}").unwrap();
        let (text, length, ai, ps): (String, i64, String, String) = conn
            .query_row(
                "SELECT text, length, ai_processing_json, processing_stats_json \
                 FROM history WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .unwrap();
        assert_eq!(text, "старый текст");
        assert_eq!(length, 12);
        assert_eq!(ai, "{\"used\":false}");
        assert_eq!(ps, "{\"llm_seconds\":1.0}");
    }
}
