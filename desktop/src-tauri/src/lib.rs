mod accessibility;
pub mod ai;
mod audio;
mod audio_file;
mod audio_worker;
#[cfg(any(target_os = "macos", test))]
mod autostart;
mod clipboard;
pub mod cloud_stt;
pub mod config;
mod db;
mod debug;
mod format_commands;
pub mod formatter;
mod history;
mod hotkey;
pub mod mic_test;
pub mod model;
pub mod model_download;
pub mod mutex_recover;
mod output_volume;
mod overlay;
mod portable;
pub mod secret_store;
pub mod sherpa;
mod sounds;
pub mod state;
mod stats;
pub mod structured_log;
mod telemetry;
#[cfg(test)]
mod test_support;
mod tray;
mod ui_text;
mod updater;
mod vad;
mod wav;
pub mod whisper;
mod window_state;
#[cfg(windows)]
mod windows_util;
#[cfg(windows)]
mod windows {
    pub mod overlay_diag;
    pub mod tray_popup;
    pub mod win_util;
}

use crate::state::{AppFsm, AppState};
use rusqlite::Connection;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Run a blocking DB op on a worker thread and return the result as a
/// `Result<T, String>`.
///
/// **WS 4b (Task 9)**: every Tauri command in this module that touches
/// the DB goes through this helper. Rationale:
///
/// 1. `rusqlite::Connection` is `!Send`, so we cannot hold the lock
///    across `.await` from the async Tauri command body. The std
///    `MutexGuard` is `Send` (when `T: Send`), but mixing it with
///    `.await` still risks blocking the runtime if the lock is
///    contended.
/// 2. `spawn_blocking` hands the work to the blocking-task pool and
///    frees the async runtime to drive other commands while DB I/O
///    runs.
/// 3. The closure is `FnOnce + Send + 'static` and the result is
///    `T: Send + 'static` so it crosses the thread boundary cleanly.
///
/// Failure modes:
/// - `Ok(Err(e))` — DB op ran but returned a `rusqlite::Error`. We
///   convert via `e.to_string()` so the wire format stays a plain
///   `String` (no rusqlite leak across the IPC boundary).
/// - `Err(_)` — the worker died (channel closed). We surface a stable
///   `"worker died"` string so callers can pattern-match.
async fn run_db_op<T, F>(db: Arc<std::sync::Mutex<Connection>>, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, rusqlite::Error> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        let result = (|| -> Result<T, String> {
            let conn = db.lock().map_err(|e| format!("db lock poisoned: {e}"))?;
            f(&conn).map_err(|e| e.to_string())
        })();
        let _ = tx.send(result);
    });
    match rx.await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("worker died".to_string()),
    }
}

#[tauri::command]
async fn get_stats(state: tauri::State<'_, AppState>) -> Result<stats::StatsResult, String> {
    let db = state.db.clone();
    run_db_op(db, stats::get_stats_from).await
}

#[tauri::command]
async fn list_history(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<history::HistoryListResult, String> {
    // Read the retention policy before handing off to the blocking worker —
    // config access needs the AppHandle, which does not cross that boundary.
    let policy = crate::config::Config::load(&app)
        .map(|cfg| history::RetentionPolicy::from_config(cfg.as_value()))
        .unwrap_or_default();
    let db = state.db.clone();
    run_db_op(db, move |conn| history::list_history_from(conn, policy)).await
}

#[tauri::command]
async fn delete_history_entry(
    state: tauri::State<'_, AppState>,
    id: u64,
) -> Result<history::DeleteResult, String> {
    let db = state.db.clone();
    run_db_op(db, move |conn| history::delete_from(conn, id)).await
}

#[tauri::command]
async fn update_history_entry_text(
    state: tauri::State<'_, AppState>,
    id: u64,
    text: String,
) -> Result<history::UpdateTextResult, String> {
    let db = state.db.clone();
    run_db_op(db, move |conn| history::update_text_from(conn, id, &text)).await
}

#[tauri::command]
async fn clear_history(state: tauri::State<'_, AppState>) -> Result<history::ClearResult, String> {
    let db = state.db.clone();
    run_db_op(db, history::clear_from).await
}

/// Return shape for `retry_history_ai_processing`.
///
/// Mirrors the Python legacy `retry_history_ai_processing` shape:
/// `{ updated: bool, entry?: HistoryEntry, reason?: string }`. The
/// frontend (`bridge/stats.ts`) pattern-matches on `updated + entry` to
/// decide whether to merge the updated row into local state, and shows
/// `reason` when `updated` is false.
///
/// `updated` means "the LLM produced new text", NOT "the row exists". It
/// used to mean the latter, which made every skipped or failed retry look
/// like a success: the button finished, nothing changed, and no error was
/// ever shown.
#[derive(Debug, Clone, serde::Serialize)]
struct HistoryRetryAiResult {
    updated: bool,
    entry: Option<history::HistoryEntry>,
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

/// Re-run AI processing on an existing history entry.
///
/// Phase 4 / PR-B — fully native Rust: reads the entry from the DB, calls
/// `crate::ai::ai_process_text_with_status` (the same orchestrator the
/// dispatcher uses for live transcriptions), then writes the resulting
/// `ai_processing` / `processing_stats` JSON back to the same row. No
/// Python subprocess involved.
///
/// `source_text` fallback: if `entry.formatted_text` is empty (migrated
/// legacy entries may not have a `formatted_text` field), fall back to
/// `entry.text` so we still have something to send to the LLM.
#[tauri::command]
async fn retry_history_ai_processing(
    state: tauri::State<'_, AppState>,
    app: AppHandle,
    id: u64,
) -> Result<HistoryRetryAiResult, String> {
    app.state::<telemetry::Telemetry>()
        .begin_usage_session(telemetry::SessionTrigger::Llm);
    // 1. Read the entry from the DB.
    let db = state.db.clone();
    let entry = run_db_op(db, move |conn| read_history_entry(conn, id))
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
    let config = crate::config::Config::load(&app)?;
    let mut ai_cfg = crate::ai::step::AiConfig::from_ai_processing(ai_processing_config(&config)?);
    ai_cfg.language = speech_language(Some(&config));
    ai_cfg.pipeline_mode = manual_llm_mode(&ai_cfg.pipeline_mode).to_string();

    // 4. Look up the API key from the secret store.
    let api_key = if ai_cfg.api_key_ref.is_empty() {
        None
    } else {
        crate::secret_store::get_key(&ai_cfg.api_key_ref)
            .map_err(|e| format!("secret_store get_key({}): {e}", ai_cfg.api_key_ref))?
    };

    // 5. Call the Rust AI orchestrator (no Python subprocess).
    let outcome =
        crate::ai::ai_process_text_with_status(&source_text, &ai_cfg, api_key.as_deref()).await;

    // 6. Build both JSON columns in the shapes the live dispatcher writes
    //    (`ai_processing_json` = serialized AiStatus, `processing_stats_json`
    //    = timings).
    let ai_str = ai_processing_json(Some(&outcome.status))
        .ok_or_else(|| "serialize ai_processing".to_string())?;
    let ps_str = stats_with_llm_timing(
        entry.processing_stats.as_ref(),
        outcome.status.elapsed_seconds,
    );

    // 7. Write back to DB. The new text goes in alongside the status —
    //    without it the row keeps showing the pre-retry text and the
    //    button looks like it did nothing.
    let db = state.db.clone();
    let new_text = outcome.status.used.then(|| outcome.text.clone());
    run_db_op(db, move |conn| {
        update_entry_ai(conn, id, new_text.as_deref(), &ai_str, &ps_str)
    })
    .await?;

    // 8. Re-read the row so the returned `entry` reflects the update.
    let db = state.db.clone();
    let entry = run_db_op(db, move |conn| read_history_entry(conn, id)).await?;
    Ok(HistoryRetryAiResult {
        updated: outcome.status.used && entry.is_some(),
        entry,
        reason: retry_failure_reason(&outcome.status),
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

/// Read a single history row by id (used by `retry_history_ai_processing`
/// to fetch + re-fetch the row around the Python LLM round-trip).
fn read_history_entry(
    conn: &Connection,
    id: u64,
) -> Result<Option<history::HistoryEntry>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, timestamp, text, raw_text, formatted_text, language, inference_time_ms, \
         ai_processing_json, processing_stats_json, system_prompt, transcription_model, length \
         FROM history WHERE id = ?1",
    )?;
    let mut rows = stmt.query([id as i64])?;
    if let Some(row) = rows.next()? {
        Ok(Some(history::HistoryEntry {
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

/// Update only the AI-related JSON columns on a history row. Length /
/// text / raw_text / formatted_text are NOT touched — the LLM step
/// enriches metadata, not the transcript itself.
/// Write back the result of re-running the LLM over an existing history row.
///
/// `ai_json` and `ps_json` are always written: they describe the pass that
/// just ran, and that is true whether it succeeded or not. This matches
/// the live dispatcher, which records a failing `AiStatus` just as readily
/// as a successful one — and it is what lets the history UI explain a
/// failed retry, since it renders `provider_error` / `skipped_reason`
/// straight off this column.
///
/// `text` is `Some` only when the pass produced new text
/// (`AiStatus::used`); a failed pass leaves the last good text in place.
/// `length` moves with it for the same reason `history::update_text` keeps
/// them together — the column is what the list shows, and a stale count is
/// a visible lie.
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

/// Return the full on-disk config as a JSON value.
///
/// Phase 4 / PR-B: native replacement for the Python sidecar's
/// `get_config` RPC. The frontend calls this via `rustInvoke` to
/// load settings without a Python subprocess round-trip.
#[tauri::command]
fn get_config(app: AppHandle) -> Result<Value, String> {
    let cfg = crate::config::Config::load(&app)?;
    Ok(cfg.as_value().clone())
}

/// Save a JSON Merge Patch to the on-disk config.
///
/// Phase 4 / PR-B: native replacement for the Python sidecar's
/// `save_config` RPC. The frontend sends a partial config object
/// (`patch`) which is merged per RFC 7396: null removes keys,
/// scalars/arrays replace atomically, objects recurse.
/// Changing `device` (CPU / GPU) additionally triggers a model reload:
/// `use_gpu` is a *context* parameter in whisper.cpp, so it only takes
/// effect when the context is created. Without the reload the setting
/// would appear to save and change nothing until the next restart.
///
/// The reload runs detached so the settings UI is not blocked for the
/// second-plus a large model takes to load; the frontend already reacts to
/// the `model-loading` / `model-ready` events the engine emits.
#[tauri::command]
fn save_config(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    patch: Value,
) -> Result<Value, String> {
    let current_config = crate::config::Config::load(&app)?;
    let device_before = crate::config::resolve_device(current_config.as_value());
    let mut candidate_config = current_config.clone();
    candidate_config.apply_merge_patch(&patch)?;
    // The configured-model half of the GigaAM language rule lives in
    // `config::validate`, which every writer goes through. This half cannot:
    // it asks what the engine has loaded right now, which no `Value` knows.
    if patch.get("language").is_some() || patch.get("model").is_some() {
        let language = candidate_config
            .get_string("language")
            .unwrap_or_else(|| "ru".to_string());
        let loaded_model = crate::mutex_recover::lock(&state.engine_current_model).clone();
        if let Some(model) = loaded_model.as_deref() {
            if !crate::model::model_supports_language(model, &language) {
                let languages = crate::model::model_languages(model).unwrap_or_default();
                return Err(crate::model::language_unsupported_message(languages));
            }
        }
    }
    let saved = crate::config::save_with_merge_patch(&app, patch.clone())?;
    let device_after = crate::config::resolve_device(&saved);
    apply_runtime_config(&app, &saved, &patch);

    if device_before != device_after {
        // Nothing to reload if no model is loaded — whatever loads next
        // reads the new setting through `load_model_into_engine`.
        let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
        if let Some(model) = loaded {
            log::info!("device changed {device_before} → {device_after}, reloading {model}");
            let reload_app = app.clone();
            let reload_tx = state.engine_cmd_tx.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(error) = load_model_into_engine(
                    &reload_app,
                    &reload_tx,
                    &model,
                    crate::whisper::ModelLoadReason::Requested,
                )
                .await
                {
                    log::warn!("reload after device change failed: {error}");
                }
            });
        }
    }

    Ok(saved)
}

/// Everything that has to happen inside the running app once a setting has
/// been written.
///
/// One place, rather than a `touches_X -> do_Y` chain growing in the middle of
/// the save command: a new live setting adds a branch here next to its
/// neighbours instead of another `if` three screens into an unrelated
/// function. Nothing here can fail the save — the value is already on disk,
/// so a subsystem that refuses to pick it up is logged, not propagated.
fn apply_runtime_config(app: &AppHandle, saved: &Value, patch: &Value) {
    // Waiting for a restart here would keep capturing events after the user
    // opted out, which is the one thing the switch must not do.
    if patch.get(crate::telemetry::enabled_config_key()).is_some() {
        app.state::<crate::telemetry::Telemetry>()
            .set_enabled(crate::telemetry::enabled_from_value(saved));
    }
    if patch
        .get(crate::telemetry::session_timeout_config_key())
        .is_some()
    {
        app.state::<crate::telemetry::Telemetry>()
            .set_session_timeout_minutes(crate::telemetry::session_timeout_minutes_from_value(
                saved,
            ));
    }
    if patch.get("auto_start").is_some() {
        apply_autostart(app);
    }
    if patch.get(crate::ui_text::CONFIG_KEY).is_some() {
        crate::ui_text::set_from_config(saved);
        // The tray menu is built once at startup, so it will not notice a
        // language change on its own — we rebuild it.
        if let Err(error) = crate::tray::build_tray(app) {
            log::warn!("не пересобрали трей после смены языка: {error}");
        }
    }
    // Unconditional: cheap, and the point of turning up logging is usually to
    // catch the thing that is happening right now.
    crate::structured_log::set_level(crate::debug::log_level_from_config(saved));
    #[cfg(windows)]
    crate::windows::overlay_diag::configure(saved);
}

/// Play one audio cue so the user can hear what they are enabling.
///
/// Takes the volume as an argument instead of reading it back from config:
/// the settings UI previews the value under the slider *before* it is
/// saved, and a preview that lags the control by one change is useless.
#[tauri::command]
fn preview_sound_cue(cue: String, volume: f64) -> Result<(), String> {
    let cue = match cue.as_str() {
        "start" => crate::sounds::Cue::Start,
        "stop" => crate::sounds::Cue::Stop,
        "done" => crate::sounds::Cue::Done,
        "error" => crate::sounds::Cue::Error,
        other => return Err(format!("unknown cue: {other}")),
    };
    crate::sounds::play_at_volume(cue, volume as f32);
    Ok(())
}

/// Temporarily lower the Windows multimedia output so the setting can be
/// verified without starting a recording. Unlike normal best-effort ducking,
/// this command returns the Core Audio error to the settings UI.
#[tauri::command]
async fn preview_output_duck(level: f64) -> Result<(), String> {
    tokio::task::spawn_blocking(move || {
        crate::output_volume::preview(level as f32, std::time::Duration::from_millis(1200))
    })
    .await
    .map_err(|error| format!("output volume preview task failed: {error}"))?
}

/// The rules appended to every system prompt, verbatim.
///
/// The settings page shows this read-only under the prompt editor. Without it
/// the character counter under the textarea understates what the model is
/// given by the length of this block, and a user debugging the LLM's
/// behaviour is reading only half of its instructions.
#[tauri::command]
fn get_output_contract() -> String {
    crate::ai::step::output_contract().to_string()
}

/// Return the app version string (from Cargo.toml).
/// Boot-blocking — called from `MainWindow.load()` via `Promise.all`.
#[tauri::command]
fn app_version(app: AppHandle) -> Result<serde_json::Value, String> {
    let version = app.package_info().version.to_string();
    Ok(serde_json::json!({ "version": version }))
}

/// Ask the update server. An `available: false` answer is not an error.
#[tauri::command]
async fn check_update(app: AppHandle) -> Result<updater::UpdateInfo, String> {
    updater::check(&app).await
}

/// Download and install the update. Progress arrives as
/// `update-download-progress` events; on success the application restarts and
/// the command never returns control to the frontend.
#[tauri::command]
async fn install_update(app: AppHandle) -> Result<(), String> {
    updater::install(&app).await
}

/// Enumerate available microphones with index+id+name+label.
/// Boot-blocking — called from `MainWindow.load()` via `Promise.all`.
///
/// Maps each device position to `index`/`id` (same value), and
/// copies the device `name` into both `name` and `label` fields.
/// The frontend consumes this via:
///   `microphones.map((mic) => ({
///      label: mic.name || mic.label || String(mic.id ?? mic.index),
///      value: mic.id ?? mic.index ?? null
///   }))`
/// Device enumeration is a WASAPI call, so it runs on the audio worker
/// like every other one. It used to run inline on the main thread, which
/// meant the settings page could freeze the UI on a sick audio stack.
#[tauri::command]
async fn list_microphones(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<serde_json::Value>, String> {
    let devices = state
        .audio
        .call(crate::audio::AudioRecorder::list_devices)
        .await
        .unwrap_or_default();
    // The id is built from the whole list at once, not per device: whether a
    // name identifies anything depends on the other names next to it.
    let ids = crate::audio::device_ids(&devices);
    Ok(devices
        .into_iter()
        .zip(ids)
        .enumerate()
        .map(|(i, (dev, id))| {
            // `name` stays null when cpal could not read one. A placeholder
            // belongs to the interface, which can say it in the user's own
            // language; here it would only be an English string pretending to
            // be a device name.
            serde_json::json!({
                "id": id,
                "index": i,
                "name": dev.name,
                "label": dev.name,
            })
        })
        .collect())
}

/// List available models with download/selection status.
/// Boot-blocking — called from `MainWindow.load()` via `Promise.all`.
///
/// Delegates to `crate::model::list_models(selected)` which returns
/// `Vec<ModelInfo>` with `id`, `label`, `size`, `ram`, `recommended`,
/// `downloaded`, and `selected` fields.
#[tauri::command]
fn list_models(app: AppHandle, state: tauri::State<'_, AppState>) -> Vec<crate::model::ModelInfo> {
    let selected = crate::config::Config::load(&app)
        .ok()
        .and_then(|c| c.get_string("model"))
        .unwrap_or_else(|| "turbo".to_string());
    let current = crate::mutex_recover::lock(&state.engine_current_model).clone();
    crate::model::list_models(&selected, current.as_deref())
}

/// Return a snapshot of the current runtime status.
/// Boot-blocking — called from `MainWindow.load()` via `Promise.all`.
///
/// Fields:
/// - `model_loaded`: whether the engine has a loaded model
/// - `model`: currently selected model id from config
/// - `loaded_model`: model id actually owned by the engine thread (null while
///   loading failed or no model is loaded)
/// - `active_model` / `active_engine` / `active_device`: the effective STT
///   route for a new recording. In cloud mode this is the configured remote
///   model and `cloud-stt`; the local engine fields remain available for
///   diagnostics but are not the model that will transcribe the recording.
/// - `device`: compute device actually used by the loaded engine ("cpu" /
///   "gpu"), or null when no engine is loaded
/// - `model_loads_on_demand`: the model is not in memory, but it is selected
///   and downloaded — so it will be brought back at the next dictation. Tells a
///   model unloaded on idle apart from a missing one: there is something to
///   transcribe with, the memory is simply free right now
/// - `recording`: whether the audio recorder is active
/// - `state`: app FSM state string (idle/recording/processing/done/error)
/// - `last_error`: null (error tracking is not wired yet)
#[tauri::command]
fn get_runtime_status(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let config = crate::config::Config::load(&app).ok();
    let model = config.as_ref().and_then(|c| c.get_string("model"));
    // Normalised, not the raw string: the UI shows this verbatim and the
    // stored value may still be the legacy `"cuda"`.
    let device = config
        .as_ref()
        .map(|c| crate::config::resolve_device(c.as_value()));

    let fsm = *crate::mutex_recover::lock(&state.app_fsm);
    let state_str = match fsm {
        AppFsm::Idle => "idle",
        AppFsm::Recording => "recording",
        AppFsm::Processing => "processing",
        AppFsm::Done => "done",
        AppFsm::Error => "error",
    };

    let loaded_model = crate::mutex_recover::lock(&state.engine_current_model).clone();
    let loaded_engine = loaded_model
        .as_deref()
        .and_then(|id| crate::model::model_engine(id).ok());
    let actual_device = match loaded_engine {
        Some(crate::model::ModelEngine::Whisper) => device,
        // Every sherpa family runs on ONNX Runtime's CPU provider.
        Some(_) => Some("cpu"),
        None => None,
    };

    let ai = config
        .as_ref()
        .and_then(|c| c.as_value().get("ai_processing"));
    let pipeline_mode = ai
        .and_then(|value| value.get("pipeline_mode"))
        .and_then(Value::as_str)
        .unwrap_or("local");
    let active_runtime = effective_active_transcription(
        pipeline_mode,
        ai,
        loaded_model.as_deref(),
        loaded_engine,
        actual_device,
    );

    // A model unloaded on idle and one never downloaded are the same "nothing
    // is loaded" to the engine, yet opposite news to a person. What tells them
    // apart is whether the file is on disk.
    let loads_on_demand = loaded_model.is_none()
        && pipeline_mode != "cloud"
        && model.as_deref().is_some_and(crate::model::is_downloaded);

    Ok(serde_json::json!({
        "model_loaded": loaded_engine.is_some(),
        "model_loads_on_demand": loads_on_demand,
        "model": model,
        "loaded_model": loaded_model,
        "device": actual_device,
        "engine": loaded_engine.map(|engine| engine.wire_name()),
        "active_model": active_runtime.model,
        "active_engine": active_runtime.engine,
        "active_device": active_runtime.device,
        "cpu_only": loaded_engine.is_some_and(|engine| engine.is_sherpa()),
        "recording": state.recorder.is_recording(),
        "state": state_str,
        "last_error": null,
    }))
}

#[derive(Debug, PartialEq, Eq)]
struct ActiveTranscriptionRuntime {
    model: Option<String>,
    engine: Option<String>,
    device: Option<String>,
}

/// Resolve the STT route that will handle the next recording. The local
/// engine slot is intentionally not used as the source of truth for cloud
/// mode: it may still contain the last local model selected before the user
/// switched pipelines.
fn effective_active_transcription(
    pipeline_mode: &str,
    ai: Option<&Value>,
    loaded_model: Option<&str>,
    loaded_engine: Option<crate::model::ModelEngine>,
    actual_device: Option<&str>,
) -> ActiveTranscriptionRuntime {
    if pipeline_mode == "cloud" {
        return ActiveTranscriptionRuntime {
            model: ai
                .and_then(|value| value.get("stt_model").or_else(|| value.get("model")))
                .and_then(Value::as_str)
                .filter(|model| !model.trim().is_empty())
                .map(str::to_string),
            engine: Some("cloud-stt".to_string()),
            device: Some("cloud".to_string()),
        };
    }

    ActiveTranscriptionRuntime {
        model: loaded_model.map(str::to_string),
        engine: loaded_engine.map(|engine| engine.wire_name().to_string()),
        device: actual_device.map(str::to_string),
    }
}

#[cfg(test)]
mod preview_queue_tests {
    use super::{preview_has_room, PREVIEW_QUEUE_RESERVE};

    #[test]
    fn live_preview_never_takes_the_seats_the_recording_needs() {
        // The engine has one queue for every command, and a streaming model
        // that fell behind managed to fill it with preview chunks. After that a
        // `try_send` carrying the finished recording was rejected and the whole
        // dictation was lost — the draft crowded out the result.
        let (tx, _rx) = tokio::sync::mpsc::channel::<u8>(64);
        let mut sent = 0;
        while preview_has_room(tx.capacity()) {
            tx.try_send(0).expect("место есть, отправка обязана пройти");
            sent += 1;
        }

        assert_eq!(sent, 64 - PREVIEW_QUEUE_RESERVE);
        assert_eq!(tx.capacity(), PREVIEW_QUEUE_RESERVE);
        assert!(
            tx.try_send(1).is_ok(),
            "место под финальную расшифровку осталось"
        );
    }
}

#[cfg(test)]
mod runtime_status_tests {
    use super::{effective_active_transcription, model, Value};
    use serde_json::json;

    #[test]
    fn cloud_active_route_wins_over_stale_local_engine() {
        let ai = json!({
            "pipeline_mode": "cloud",
            "model": "llm-model",
            "stt_model": "whisper-1"
        });

        let active = effective_active_transcription(
            "cloud",
            Some(&ai),
            Some("gigaam-v3"),
            Some(model::ModelEngine::SherpaNemoCtc),
            Some("cpu"),
        );

        assert_eq!(active.model.as_deref(), Some("whisper-1"));
        assert_eq!(active.engine.as_deref(), Some("cloud-stt"));
        assert_eq!(active.device.as_deref(), Some("cloud"));
    }

    #[test]
    fn local_active_route_mirrors_loaded_engine() {
        let ai: Value = json!({ "pipeline_mode": "local" });
        let active = effective_active_transcription(
            "local",
            Some(&ai),
            Some("gigaam-v3"),
            Some(model::ModelEngine::SherpaNemoCtc),
            Some("cpu"),
        );

        assert_eq!(active.model.as_deref(), Some("gigaam-v3"));
        assert_eq!(active.engine.as_deref(), Some("sherpa-onnx"));
        assert_eq!(active.device.as_deref(), Some("cpu"));
    }
}

/// Download a model by id ("tiny", "base", "small", "medium", "large-v3", "turbo").
///
/// Streams the GGML file from Hugging Face, verifies SHA-256, and renames
/// onto the final path. Emits `model-download-progress` events during
/// download with payload `{ model, downloaded, total }`.
///
/// A cancelled download is `Ok(None)`, not an error: the user pressed «отменить»
/// and got exactly what they asked for. What was not downloaded is erased along
/// the way — we have no resume, and a leftover chunk would be nothing but
/// occupied space.
#[tauri::command]
async fn download_model(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
) -> Result<Option<crate::model_download::DownloadOutcomeInfo>, String> {
    // Cancellation is registered before the first byte: otherwise «отменить»
    // pressed within the first second would find nothing to cancel. The same
    // registration rejects a second download of the same model: both would write
    // one `*.part` and race each other checking its checksum.
    let Some(download) = state.try_claim_download(&model) else {
        return Err(crate::ui_text::t("Эта модель уже скачивается."));
    };
    let cancel = download.flag();
    if crate::model::model_engine(&model)?.is_sherpa() {
        let entry = crate::model::bundle_manifest_entry(&model)?;
        let dir = crate::model::models_dir()?;
        let final_dir = dir.join(entry.directory_name);
        // `is_downloaded` intentionally performs only cheap size checks
        // because it runs while refreshing the Settings list. A final bundle
        // path must therefore always be verified here: a missing artifact or
        // wrong-size file makes `is_downloaded` false but must not strand the
        // downloader behind an existing destination.
        let model_id = entry.public_id.to_string();
        let already_ready =
            tokio::task::spawn_blocking(move || crate::model::recover_bundle_if_needed(&model_id))
                .await
                .map_err(|error| format!("bundle verification task failed: {error}"))??;
        if already_ready {
            let bytes = entry
                .artifacts
                .iter()
                .map(|artifact| artifact.expected_bytes)
                .sum();
            return Ok(Some(crate::model_download::DownloadOutcomeInfo {
                model_id: entry.public_id.to_string(),
                path: final_dir.to_string_lossy().into_owned(),
                bytes,
            }));
        }
        let spec = crate::model_download::BundleDownloadSpec {
            model_id: entry.public_id.to_string(),
            directory_name: entry.directory_name.to_string(),
            artifacts: entry
                .artifacts
                .iter()
                .map(|artifact| crate::model_download::DownloadSpec {
                    model_id: entry.public_id.to_string(),
                    file_name: artifact.file_name.to_string(),
                    url: artifact.download_url.to_string(),
                    expected_bytes: artifact.expected_bytes,
                    sha256: artifact.sha256.to_string(),
                })
                .collect(),
        };
        let client = reqwest::Client::new();
        let app_for_progress = app.clone();
        let model_for_progress = entry.public_id.to_string();
        let progress_cb = move |p: crate::model_download::DownloadProgress| {
            let _ = app_for_progress.emit(
                "model-download-progress",
                serde_json::json!({
                    "model": model_for_progress,
                    "downloaded": p.downloaded,
                    "total": p.total,
                }),
            );
        };
        let outcome = match crate::model_download::download_bundle_to_dir(
            &client,
            &spec,
            &dir,
            &cancel,
            Some(&progress_cb),
            None,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(crate::model_download::ModelDownloadError::Cancelled) => {
                crate::model_download::discard_bundle_partial(&dir, &spec);
                return Ok(None);
            }
            Err(error) => return Err(format!("download: {error}")),
        };
        return Ok(Some(crate::model_download::DownloadOutcomeInfo {
            model_id: entry.public_id.to_string(),
            path: outcome.path.to_string_lossy().into_owned(),
            bytes: outcome.bytes,
        }));
    }
    let entry =
        crate::model::manifest_entry(&model).map_err(|e| format!("unknown model {model}: {e}"))?;
    let spec = crate::model_download::DownloadSpec {
        model_id: entry.public_id.to_string(),
        file_name: entry.file_name.to_string(),
        url: entry.download_url.to_string(),
        expected_bytes: entry.expected_bytes,
        sha256: entry.sha256.to_string(),
    };
    let dir = crate::model::models_dir().map_err(|e| e.to_string())?;
    let client = reqwest::Client::new();

    // Wire progress events so the frontend can show a download bar.
    let app_for_progress = app.clone();
    let mid_for_progress = model.clone();
    let progress_cb = move |p: crate::model_download::DownloadProgress| {
        let _ = app_for_progress.emit(
            "model-download-progress",
            serde_json::json!({
                "model": mid_for_progress,
                "downloaded": p.downloaded,
                "total": p.total,
            }),
        );
    };

    let outcome = match crate::model_download::download_spec_to_dir(
        &client,
        &spec,
        &dir,
        &cancel,
        Some(&progress_cb),
        None,
    )
    .await
    {
        Ok(outcome) => outcome,
        Err(crate::model_download::ModelDownloadError::Cancelled) => {
            crate::model_download::discard_partial(&dir, &spec);
            return Ok(None);
        }
        Err(error) => return Err(format!("download: {error}")),
    };

    Ok(Some(crate::model_download::DownloadOutcomeInfo::for_model(
        &model, &outcome,
    )))
}

/// Stop a download of a model that is in progress.
///
/// `false` — nobody is downloading this model right now: the button was pressed
/// after the download had already finished. That is not an error but a race, and
/// it is cured by staying silent.
#[tauri::command]
fn cancel_model_download(state: tauri::State<'_, AppState>, model: String) -> bool {
    state.cancel_download(&model)
}

/// Load a downloaded model into the whisper engine.
///
/// Sends `EngineCommand::SetModel` to the engine thread. The engine
/// emits `model-loading` then `model-ready` or `model-load-failed`.
/// Returns `Err` if the model is not downloaded or the engine channel
/// is closed.
#[tauri::command]
async fn set_model(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
) -> Result<(), String> {
    load_model_into_engine(
        &app,
        &state.engine_cmd_tx,
        &model,
        crate::whisper::ModelLoadReason::Requested,
    )
    .await
}

/// Send `SetModel` to the engine thread and await its reply.
///
/// Explicit loads read the current compute device from config. Idle restores
/// do the same in `restore_unloaded_model`, but queue without waiting so capture
/// can continue while the engine validates and loads the model.
async fn load_model_into_engine(
    app: &AppHandle,
    engine_cmd_tx: &tokio::sync::mpsc::Sender<crate::whisper::EngineCommand>,
    model: &str,
    reason: crate::whisper::ModelLoadReason,
) -> Result<(), String> {
    let engine = crate::model::model_engine(model)?;
    if !crate::model::is_downloaded(model) {
        return Err(format!("model {model} not downloaded"));
    }
    if engine.is_sherpa() {
        // Mandatory closed-registry validation before crossing the Sherpa C
        // boundary. A malformed ONNX graph can abort the process via a C++
        // exception rather than return a Rust error.
        crate::model::verify_bundle_files(model)?;
    }
    let use_gpu = crate::config::Config::load(app)
        .map(|c| crate::config::device_uses_gpu(c.as_value()))
        .unwrap_or(true);
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
    let spec = crate::model::model_load_spec(model, use_gpu)?;
    engine_cmd_tx
        .send(crate::whisper::EngineCommand::SetModel {
            name: model.to_string(),
            spec,
            reason,
            reply: reply_tx,
        })
        .await
        .map_err(|e| format!("engine channel closed: {e}"))?;
    reply_rx
        .await
        .map_err(|e| format!("engine reply dropped: {e}"))?
}

/// Queue a restore before any preview or transcription commands can follow.
/// Verification and loading run on the engine thread, in queue order; capture
/// only does cheap metadata checks. Returns the model that will handle audio,
/// including one still being restored, so preview can attach immediately.
fn restore_unloaded_model(app: &AppHandle, state: &AppState) -> Option<String> {
    let config = crate::config::Config::load(app).ok()?;
    let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
    let model = recording_model(loaded.as_deref(), config.as_value())?;
    if loaded.is_some() {
        return Some(model);
    }
    if !crate::model::is_downloaded(&model) {
        return None;
    }
    let result =
        crate::model::model_load_spec(&model, crate::config::device_uses_gpu(config.as_value()))
            .and_then(|spec| queue_model_restore(&state.engine_cmd_tx, &model, spec));
    match result {
        Ok(()) => Some(model),
        Err(error) => {
            log::warn!("не поставили восстановление модели {model} в очередь: {error}");
            None
        }
    }
}

fn recording_model(loaded: Option<&str>, config: &Value) -> Option<String> {
    if config
        .pointer("/ai_processing/pipeline_mode")
        .and_then(Value::as_str)
        == Some("cloud")
    {
        return None;
    }
    loaded
        .or_else(|| config.get("model").and_then(Value::as_str))
        .map(str::to_string)
}

fn queue_model_restore(
    tx: &tokio::sync::mpsc::Sender<crate::whisper::EngineCommand>,
    model: &str,
    spec: crate::model::ModelLoadSpec,
) -> Result<(), String> {
    let (reply, _rx) = tokio::sync::oneshot::channel();
    tx.try_send(crate::whisper::EngineCommand::SetModel {
        name: model.to_string(),
        spec,
        reason: crate::whisper::ModelLoadReason::Restore,
        reply,
    })
    .map_err(|error| format!("engine: {error}"))
}

#[cfg(test)]
mod model_restore_tests {
    use super::{queue_model_restore, recording_model};
    use crate::model::ModelLoadSpec;
    use crate::whisper::{EngineCommand, ModelLoadReason};
    use serde_json::json;

    fn unloaded_spec() -> ModelLoadSpec {
        ModelLoadSpec::Whisper {
            path: "missing-test-model.bin".into(),
            use_gpu: false,
        }
    }

    #[test]
    fn a_short_recording_queues_behind_restore_even_before_the_engine_runs() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        // No model files or running engine are needed to start capture. Even
        // if validation/loading has not begun when recording stops, restoration
        // already owns the first queue slot.
        queue_model_restore(&tx, "tiny", unloaded_spec()).unwrap();
        tx.try_send(EngineCommand::PreviewReset { session_id: 1 })
            .unwrap();
        tx.try_send(EngineCommand::PreviewChunk {
            session_id: 1,
            samples: vec![0.0; 160],
        })
        .unwrap();
        let (reply, _reply_rx) = tokio::sync::oneshot::channel();
        tx.try_send(EngineCommand::Transcribe {
            session_id: 1,
            audio: std::sync::Arc::new(vec![0.0; 160]),
            cancel_flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            language: None,
            initial_prompt: None,
            reply,
        })
        .unwrap();
        assert!(matches!(
            rx.try_recv().unwrap(),
            EngineCommand::SetModel {
                reason: ModelLoadReason::Restore,
                ..
            }
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            EngineCommand::PreviewReset { .. }
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            EngineCommand::PreviewChunk { .. }
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            EngineCommand::Transcribe { .. }
        ));
    }

    #[test]
    fn failed_enqueue_does_not_report_a_pending_restore() {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.try_send(EngineCommand::PreviewReset { session_id: 1 })
            .unwrap();
        assert!(queue_model_restore(&tx, "tiny", unloaded_spec()).is_err());
        drop(rx);
        assert!(queue_model_restore(&tx, "tiny", unloaded_spec()).is_err());
    }

    #[test]
    fn capture_uses_selected_model_before_restore_finishes() {
        let config = json!({"model": "zipformer-ru-streaming"});
        assert_eq!(
            recording_model(None, &config).as_deref(),
            Some("zipformer-ru-streaming")
        );
        assert_eq!(
            recording_model(Some("tiny"), &config).as_deref(),
            Some("tiny")
        );
        assert_eq!(recording_model(None, &json!({})), None);
        let cloud =
            json!({"model": "zipformer-ru-streaming", "ai_processing": {"pipeline_mode": "cloud"}});
        assert_eq!(recording_model(None, &cloud), None);
        assert_eq!(
            recording_model(Some("zipformer-ru-streaming"), &cloud),
            None
        );
    }

    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn an_unloaded_streaming_model_still_arms_preview() {
        let config = json!({"model": "zipformer-ru-streaming"});
        let model = recording_model(None, &config).unwrap();
        assert!(crate::model::model_engine(&model).unwrap().is_streaming());
    }
}

/// How often to ask the engine whether it is time to give the memory back.
///
/// Half a minute is the precision with which the chosen threshold is honoured,
/// and the price of asking: the check costs one config read and one command in
/// the queue.
const IDLE_UNLOAD_TICK: std::time::Duration = std::time::Duration::from_secs(30);

/// After how much idling to unload the model. `None` — do not unload: either
/// the settings say so, or the config could not be read and nothing should be
/// touched.
fn idle_unload_after(app: &AppHandle) -> Option<std::time::Duration> {
    let config = crate::config::Config::load(app).ok()?;
    let minutes = crate::config::model_unload_after_minutes(config.as_value());
    (minutes > 0).then(|| std::time::Duration::from_secs(minutes * 60))
}

/// Delete a cached model file from disk.
///
/// The model the engine is currently holding may be deleted too. Refusing it
/// left downloading something else as the only way out of a full disk, which
/// is backwards — the files are the user's. The engine unloads the model
/// first and the reply is awaited, so its memory is freed and, on Windows,
/// the file handles are released before the removal is attempted. After this
/// the app has no model until one is downloaded again; the confirmation
/// dialog says so before calling this.
///
/// Returns `true` if the file existed and was deleted, `false` if it
/// was already absent.
#[tauri::command]
async fn delete_model(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
) -> Result<bool, String> {
    // A user's own file is unknown to the catalog and normalisation fails on it.
    // Its identifier is the file name, and `delete_cached_model` checks it by the
    // same rule as a download does: there is no way out of the models directory
    // from here.
    let normalized = crate::model::normalize_model_id(&model)
        .map(str::to_string)
        .unwrap_or_else(|_| model.clone());
    let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
    if loaded.as_deref() == Some(normalized.as_str()) {
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel::<()>();
        state
            .engine_cmd_tx
            .send(crate::whisper::EngineCommand::UnloadModel { reply: reply_tx })
            .await
            .map_err(|e| format!("engine channel closed: {e}"))?;
        // Commands are served in order on one thread, so this reply also
        // means any transcription that was already queued has finished.
        reply_rx
            .await
            .map_err(|e| format!("engine reply dropped: {e}"))?;
        let _ = app.emit("model-unloaded", normalized.clone());
    }
    crate::model::delete_cached_model(&model).map_err(|e| e.to_string())
}

/// Return both configured and actually loaded model state. The two values can
/// differ briefly during startup or after a failed switch; `engine` always
/// describes the engine thread rather than merely echoing config.
#[tauri::command]
fn get_model_status(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let selected = crate::config::Config::load(&app)
        .ok()
        .and_then(|c| c.get_string("model"));
    let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
    let engine = loaded
        .as_deref()
        .and_then(|id| crate::model::model_engine(id).ok())
        .map(|engine| engine.wire_name());
    Ok(serde_json::json!({
        "selected": selected,
        "loaded": loaded,
        "model_loaded": engine.is_some(),
        "engine": engine,
    }))
}

/// Save an API key into the platform secret store.
///
/// The frontend (API-keys / providers pages) invokes this with the
/// slot ref (`key_id`) as the storage username, matching how the AI
/// pipeline later resolves keys via `secret_store::get_key(api_key_ref)`.
/// `provider` is passed for context but not needed for storage.
///
/// `rename_all = "snake_case"` because the frontend sends snake_case
/// argument names (`key_id`) — Tauri's default is camelCase.
///
/// Returns `{ saved, label, masked }`. Labels are not portably stored in
/// the OS credential store, so the caller-supplied `label` is echoed back
/// for the in-memory UI state (it does not survive a restart).
#[tauri::command(rename_all = "snake_case")]
fn save_api_key(
    key_id: String,
    key: String,
    label: Option<String>,
) -> Result<serde_json::Value, String> {
    crate::secret_store::save_key(&key_id, &key)?;
    let masked = crate::secret_store::get_key_meta(&key_id)?
        .map(|meta| meta.masked)
        .unwrap_or_default();
    Ok(serde_json::json!({
        "saved": true,
        "label": label.unwrap_or_default(),
        "masked": masked,
    }))
}

/// Report whether a key exists for the given slot ref, with its mask.
/// Called at boot for every known slot and after edits.
#[tauri::command(rename_all = "snake_case")]
fn has_api_key(key_id: String) -> Result<serde_json::Value, String> {
    match crate::secret_store::get_key_meta(&key_id)? {
        Some(meta) => Ok(serde_json::json!({
            "available": meta.available,
            "label": meta.label,
            "masked": meta.masked,
        })),
        None => Ok(serde_json::json!({ "available": false, "label": "", "masked": "" })),
    }
}

/// Delete a stored API key. Returns `{ deleted }` (false if there was
/// no key in that slot — not an error).
#[tauri::command(rename_all = "snake_case")]
fn delete_api_key(key_id: String) -> Result<serde_json::Value, String> {
    let deleted = crate::secret_store::delete_key(&key_id)?;
    Ok(serde_json::json!({ "deleted": deleted }))
}

/// The speech language substituted for `{{language}}` in the system prompt.
///
/// It sits at the top level of the config, next to the model and the device,
/// and NOT inside `ai_processing`. `AiConfig::from_ai_processing` used to read a
/// field of the same name from its own subtree — nobody ever wrote it there, so
/// the placeholder expanded to nothing and the model received "Output
/// language: .".
fn speech_language(config: Option<&crate::config::Config>) -> String {
    config
        .and_then(|cfg| cfg.get_string("language"))
        .unwrap_or_default()
}

/// Shared core for the two explicit-LLM commands (`test_ai_prompt`,
/// `process_text_ai`). Both run the SAME orchestrator the live
/// dispatcher and history-retry use, but force `pipeline_mode = "hybrid"`
/// and `llm_min_duration_seconds = 0` so the LLM step always runs — the
/// user pressed a button explicitly, there is no recording-duration gate.
///
/// Returns the `AiRunResult` shape the frontend expects:
/// `{ available, output, message?, fallback, provider_error?, skipped_reason?, ai_processing }`.
// The provider/model/key/url/prompt/profile fields mirror the Tauri command
// IPC signatures below; collapsing them into a struct would change the
// frontend invoke contract, so keep the flat arg list.
#[allow(clippy::too_many_arguments)]
async fn run_ai_prompt(
    provider: Option<String>,
    model: Option<String>,
    api_key_ref: Option<String>,
    base_url: Option<String>,
    system_prompt: Option<String>,
    profile_id: Option<String>,
    profile_name: Option<String>,
    language: String,
    text: &str,
) -> Result<serde_json::Value, String> {
    let provider = provider.unwrap_or_default();
    if provider.trim().is_empty() {
        return Ok(
            serde_json::json!({ "available": false, "message": crate::ui_text::t("Не выбран провайдер.") }),
        );
    }
    let api_key_ref = api_key_ref.unwrap_or_default();
    let cfg = crate::ai::step::AiConfig {
        pipeline_mode: "hybrid".to_string(),
        provider,
        model: model.unwrap_or_default(),
        profile_id: profile_id.unwrap_or_default(),
        profile_name: profile_name.unwrap_or_default(),
        api_key_ref: api_key_ref.clone(),
        system_prompt: system_prompt.unwrap_or_default(),
        language,
        base_url: base_url.filter(|value| !value.trim().is_empty()),
        audio_duration_seconds: None,
        llm_min_duration_seconds: 0.0,
        llm_timeout_seconds: 30,
    };
    let api_key = if api_key_ref.is_empty() {
        None
    } else {
        crate::secret_store::get_key(&api_key_ref)
            .map_err(|e| format!("secret_store get_key({api_key_ref}): {e}"))?
    };
    let outcome = crate::ai::ai_process_text_with_status(text, &cfg, api_key.as_deref()).await;
    let status = &outcome.status;
    let message = if status.used {
        serde_json::Value::Null
    } else if let Some(err) = &status.provider_error {
        serde_json::json!(err)
    } else if !status.skipped_reason.is_empty() {
        serde_json::json!(status.skipped_reason)
    } else {
        serde_json::json!(crate::ui_text::t("LLM не вернула результат."))
    };
    Ok(serde_json::json!({
        "available": status.used,
        "output": outcome.text,
        "fallback": status.fallback,
        "provider_error": status.provider_error,
        "skipped_reason": if status.skipped_reason.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::json!(status.skipped_reason)
        },
        "message": message,
        // The provider's response body. It is assembled in `send_request` and
        // reaches `AiStatus`, but this response used to be built by hand and the
        // field never made it in — that is, the only source of truth about "the
        // response has the wrong shape" was lost at the final step, already
        // outside the HTTP layer.
        "http_status": status.http_status,
        "response_snippet": status.response_snippet,
        "ai_processing": {
            "attempted": status.attempted,
            "used": status.used,
            "skipped_reason": status.skipped_reason,
        },
    }))
}

/// Run one LLM request through the active profile — used by the "Тест"
/// buttons in the LLM and Providers pages. Falls back to a fixed sample
/// sentence when the caller supplies no text.
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)] // IPC command signature; see run_ai_prompt
async fn test_ai_prompt(
    app: AppHandle,
    provider: Option<String>,
    model: Option<String>,
    api_key_ref: Option<String>,
    base_url: Option<String>,
    system_prompt: Option<String>,
    profile_id: Option<String>,
    profile_name: Option<String>,
    text: Option<String>,
) -> Result<serde_json::Value, String> {
    app.state::<telemetry::Telemetry>()
        .begin_usage_session(telemetry::SessionTrigger::Llm);
    let sample = text
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            "ну в общем нужно сегодня встретиться с командой и обсудить следующие шаги".to_string()
        });
    run_ai_prompt(
        provider,
        model,
        api_key_ref,
        base_url,
        system_prompt,
        profile_id,
        profile_name,
        speech_language(crate::config::Config::load(&app).ok().as_ref()),
        &sample,
    )
    .await
}

/// Process arbitrary user-supplied text through the active profile's LLM
/// — the "Обработать текст" panel in the LLM page.
#[tauri::command(rename_all = "snake_case")]
#[allow(clippy::too_many_arguments)] // IPC command signature; see run_ai_prompt
async fn process_text_ai(
    app: AppHandle,
    text: String,
    provider: Option<String>,
    model: Option<String>,
    api_key_ref: Option<String>,
    base_url: Option<String>,
    system_prompt: Option<String>,
    profile_id: Option<String>,
    profile_name: Option<String>,
) -> Result<serde_json::Value, String> {
    app.state::<telemetry::Telemetry>()
        .begin_usage_session(telemetry::SessionTrigger::Llm);
    if text.trim().is_empty() {
        return Ok(
            serde_json::json!({ "available": false, "message": crate::ui_text::t("Вставьте текст для обработки.") }),
        );
    }
    run_ai_prompt(
        provider,
        model,
        api_key_ref,
        base_url,
        system_prompt,
        profile_id,
        profile_name,
        speech_language(crate::config::Config::load(&app).ok().as_ref()),
        &text,
    )
    .await
}

/// Open the system file picker and return the chosen audio file's path.
///
/// The dialog lives in Rust rather than in the webview so the extension
/// filter and the picker permission stay on this side: the frontend can
/// ask for a file, but it cannot ask for an arbitrary one.
///
/// `Ok(None)` means the user closed the dialog — a normal outcome, not an
/// error, and the panel must not show anything for it.
#[tauri::command]
async fn pick_audio_file(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    // `blocking_pick_file` on a blocking worker: the docs are explicit that
    // it must not run on the main thread, and a Tauri command's async task
    // is not a safe place to park a modal either.
    let picked = tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter(
                crate::ui_text::t("Аудио"),
                &["wav", "mp3", "m4a", "mp4", "ogg", "oga", "opus", "flac"],
            )
            .blocking_pick_file()
    })
    .await
    .map_err(|e| {
        log::error!("pick_audio_file: dialog task failed: {e}");
        crate::ui_text::t("Не удалось открыть диалог выбора файла.")
    })?;

    Ok(picked.map(|file| file.to_string()))
}

/// What the "Прикрепить аудио" panel gets back from a file transcription.
///
/// Deliberately not `InferenceResult`: the panel shows the *processed*
/// text, and it needs the intermediate stages to render the "Whisper без
/// обработки" disclosure the same way history does.
#[derive(Debug, serde::Serialize)]
pub struct TranscribeFileResult {
    /// The text to show and copy — formatted, and LLM-cleaned when the
    /// configuration calls for it.
    text: String,
    /// Straight from the engine, before any formatting.
    raw_text: String,
    /// After local formatting, before the LLM.
    formatted_text: String,
    /// `None` when the LLM never ran (disabled, or the mode is local-only).
    /// That is a normal outcome for a file, not a failure — the panel shows
    /// "Распознано" for it, not an error.
    ai_status: Option<crate::ai::step::AiStatus>,
    audio_seconds: f64,
    inference_time_ms: u64,
    language: Option<String>,
}

/// Why a file transcription stopped, in the shape the terminal handler needs.
///
/// The point of the type is that the body below can go back to using `?`.
/// Before it existed, every early return had to remember its own telemetry
/// call — eleven of them — and a forgotten one loses the operation from the
/// failure rate silently, without so much as a warning.
struct FileFailure {
    stage: telemetry::FailureStage,
    reason: telemetry::FailureReason,
    message: String,
}

impl FileFailure {
    fn new(
        stage: telemetry::FailureStage,
        reason: telemetry::FailureReason,
        message: String,
    ) -> Self {
        Self {
            stage,
            reason,
            message,
        }
    }
}

/// Errors that arrive as a bare message from somewhere further down land in
/// the generic bucket rather than claiming a stage they cannot know.
impl From<String> for FileFailure {
    fn from(message: String) -> Self {
        Self::new(
            telemetry::FailureStage::Stt,
            telemetry::FailureReason::EngineError,
            message,
        )
    }
}

/// What a completed run carries out of the inner function: the engine's
/// result and the post-processed text, both of which the terminal event and
/// the panel's payload are built from.
struct FileRun {
    inference: crate::whisper::InferenceResult,
    processed: ProcessedTranscription,
}

/// Transcribe an audio file the user attached, without touching the
/// focused window, the history, or the statistics.
///
/// This runs the same engine and the same post-processing as dictation;
/// the only difference is where the samples come from and where the text
/// goes. Everything unusual about it is defensive, and each guard below
/// exists because the engine is a single shared resource that the
/// dictation path assumes it owns.
///
/// The body lives in [`transcribe_file_inner`]; this wrapper is the single
/// place a terminal telemetry event is emitted, on either outcome.
#[tauri::command]
async fn transcribe_audio_file(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<TranscribeFileResult, String> {
    let telemetry = app.state::<telemetry::Telemetry>().clone();
    telemetry.begin_usage_session(telemetry::SessionTrigger::File);

    // Read once, up front, because every terminal event needs it. The guards
    // that fire before the engine is even claimed used to report `local`
    // whatever the user had configured, which quietly mislabelled every
    // engine-busy failure in cloud mode.
    let config = crate::config::Config::load(&app).ok();
    let pipeline_mode = telemetry_pipeline_mode(config.as_ref());

    match transcribe_file_inner(&app, &state, &path, config.as_ref(), &pipeline_mode).await {
        Ok(run) => {
            record_file_run(&telemetry, &pipeline_mode, config.as_ref(), &run);
            let FileRun {
                inference,
                processed,
            } = run;
            Ok(TranscribeFileResult {
                text: processed.final_text,
                raw_text: processed.raw_text,
                formatted_text: processed.formatted_text,
                ai_status: processed.ai_status,
                audio_seconds: inference.audio_seconds,
                inference_time_ms: inference.inference_time_ms,
                language: inference.language,
            })
        }
        Err(failure) => {
            // A deliberate cancellation is not a reliability failure, and
            // counting it as one would make the failure rate meaningless.
            if matches!(failure.reason, telemetry::FailureReason::UserCancelled) {
                telemetry.record_cancelled(telemetry::Source::File, &pipeline_mode);
            } else {
                telemetry.record_failed(
                    telemetry::Source::File,
                    &pipeline_mode,
                    failure.stage,
                    failure.reason,
                );
            }
            Err(failure.message)
        }
    }
}

/// Emit the completed event for a run that produced text.
///
/// An empty `final_text` is not an error the caller sees — the panel still
/// gets its (empty) result — but it means the post-processor threw away the
/// whole transcript, which is a failure worth counting.
fn record_file_run(
    telemetry: &telemetry::Telemetry,
    pipeline_mode: &str,
    config: Option<&crate::config::Config>,
    run: &FileRun,
) {
    if run.processed.final_text.trim().is_empty() {
        telemetry.record_failed(
            telemetry::Source::File,
            pipeline_mode,
            telemetry::FailureStage::PostProcess,
            telemetry::FailureReason::EmptyAfterProcessing,
        );
        return;
    }
    let (formatting_enabled, replacement_rules) =
        config.map(telemetry_formatting).unwrap_or((false, 0));
    telemetry.record_completed(telemetry::Outcome {
        source: telemetry::Source::File,
        pipeline_mode,
        recording_mode: telemetry::RecordingMode::NotApplicable,
        stt_model: run.inference.model_id.as_deref(),
        audio_seconds: run.inference.audio_seconds,
        stt_millis: run.inference.inference_time_ms,
        chars: run.processed.final_text.chars().count(),
        ai_status: run.processed.ai_status.as_ref(),
        compute: config.map(|config| telemetry_compute(config, run.inference.model_id.as_deref())),
        formatting_enabled,
        replacement_rules,
        paste_result: telemetry::PasteResult::NotApplicable,
    });
}

async fn transcribe_file_inner(
    app: &AppHandle,
    state: &AppState,
    path: &str,
    config: Option<&crate::config::Config>,
    pipeline_mode: &str,
) -> Result<FileRun, FileFailure> {
    // 1. Claim the engine. The guard releases on every exit path below,
    //    including the `?`s — a hand-written release would not.
    let engine_claim = state.claim_engine().ok_or_else(|| {
        FileFailure::new(
            telemetry::FailureStage::Start,
            telemetry::FailureReason::EngineBusy,
            crate::ui_text::t("Идёт транскрипция файла — дождитесь её окончания."),
        )
    })?;

    // 2. A dictation in flight owns the engine too, just through a
    //    different mechanism (it was queued before we claimed).
    if !matches!(*crate::mutex_recover::lock(&state.app_fsm), AppFsm::Idle) {
        return Err(FileFailure::new(
            telemetry::FailureStage::Start,
            telemetry::FailureReason::EngineBusy,
            crate::ui_text::t("Завершите текущую запись."),
        ));
    }

    // 3. The sherpa recognizers have no VAD of their own. They are fine on a
    //    dictation-length utterance and degrade badly across an hour-long
    //    recording, so refuse rather than hand back mush the user would blame
    //    on the file. Only checked for the local path — cloud STT does not
    //    touch the loaded model at all.
    if pipeline_mode != "cloud" {
        // Cloned out of the guard rather than read through it: `model_engine`
        // is unrelated code, and holding an engine lock across it is how the
        // next deadlock gets written.
        let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
        let is_sherpa = loaded.as_deref().is_some_and(|model| {
            crate::model::model_engine(model).is_ok_and(|engine| engine.is_sherpa())
        });
        if is_sherpa {
            return Err(FileFailure::new(
                telemetry::FailureStage::Stt,
                telemetry::FailureReason::EngineError,
                crate::ui_text::t(
                    "Эта модель не умеет расшифровывать файлы — выберите модель Whisper в «Настройки → Модели».",
                ),
            ));
        }
    }

    // 4. Decode off the async runtime: symphonia and the resampler are
    //    CPU-bound, and an hour of MP3 would stall every other task.
    let decode_path = std::path::PathBuf::from(path);
    let decode_failed = |message: String| {
        FileFailure::new(
            telemetry::FailureStage::Decode,
            telemetry::FailureReason::Decode,
            message,
        )
    };
    let decoded =
        tokio::task::spawn_blocking(move || crate::audio_file::decode_to_pcm16k_mono(&decode_path))
            .await
            .map_err(|e| {
                log::error!("transcribe_audio_file: decode task panicked: {e}");
                decode_failed(crate::ui_text::t(
                    "Не удалось прочитать звук из файла — возможно, он повреждён.",
                ))
            })?
            .map_err(decode_failed)?;

    log::info!(
        "file transcription: {:.1}s of audio decoded from {path}",
        decoded.audio_seconds
    );

    let session_id = state.next_session_id();
    let audio = Arc::new(decoded.samples);
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // 5. Claimed before the command is queued; the guard releases on every
    //    exit path below.
    let session_guard = state.claim_file_session(session_id, Arc::clone(&cancel_flag));
    // The frontend needs the id to be able to cancel; the result carries it
    // too late to be useful.
    let _ = app.emit(
        "file-transcription-started",
        serde_json::json!({ "session_id": session_id }),
    );

    // The same queue orders restoration before file transcription as well.
    restore_unloaded_model(app, state);

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    // Same branch as `stop_recording`: a cloud-configured user has no local
    // model loaded, and sending `Transcribe` would fail with «модель не
    // загружена» for a reason that has nothing to do with their setup.
    let command = if pipeline_mode == "cloud" {
        // Built before the move: the request borrows the samples that the
        // command is about to take ownership of.
        let request = build_cloud_stt_request(app, &audio).map_err(|error| {
            FileFailure::new(
                telemetry::FailureStage::Queue,
                telemetry::FailureReason::CloudConfiguration,
                error,
            )
        })?;
        crate::whisper::EngineCommand::TranscribeCloud {
            session_id,
            audio,
            cancel_flag,
            request,
            reply: reply_tx,
        }
    } else {
        crate::whisper::EngineCommand::Transcribe {
            session_id,
            audio,
            cancel_flag,
            language: config.and_then(|cfg| cfg.get_string("language")),
            initial_prompt: config.and_then(custom_words_prompt),
            reply: reply_tx,
        }
    };

    // `send`, not `try_send`: the queue is ours by the claim above, and a
    // full-channel error here would be a lie about what went wrong.
    state.engine_cmd_tx.send(command).await.map_err(|e| {
        log::error!("file transcription {session_id}: engine channel closed: {e}");
        FileFailure::new(
            telemetry::FailureStage::Queue,
            telemetry::FailureReason::EngineQueue,
            format!("engine: {e}"),
        )
    })?;

    let inference = reply_rx.await.map_err(|e| {
        log::error!("file transcription {session_id}: engine dropped the reply: {e}");
        FileFailure::new(
            telemetry::FailureStage::Stt,
            telemetry::FailureReason::EngineError,
            crate::ui_text::t("Движок не ответил. Попробуйте ещё раз."),
        )
    })?;
    // Nothing else will clear the registrations — the dispatcher skipped this
    // session — and the LLM pass below can take the better part of a minute.
    // Observe cancellation before dropping the guard (which removes the
    // skip-set entry); otherwise a cancel that wins during STT is lost.
    let was_cancelled = state.is_cancelled(session_id);
    if was_cancelled {
        state.drop_cancellation(session_id);
    }
    drop(session_guard);

    if was_cancelled {
        return Err(FileFailure::new(
            telemetry::FailureStage::Stt,
            telemetry::FailureReason::UserCancelled,
            crate::ui_text::t("Транскрипция отменена."),
        ));
    }
    if inference.text.trim().is_empty() {
        return Err(FileFailure::new(
            telemetry::FailureStage::Stt,
            telemetry::FailureReason::EmptyTranscript,
            crate::ui_text::t("В файле не распознана речь."),
        ));
    }

    // The engine is genuinely free from here on — what remains is local
    // formatting and, in hybrid mode, an LLM round-trip that can take the
    // better part of a minute. Holding the claim across it would refuse the
    // user's dictation for no reason at all.
    drop(engine_claim);
    let processed = post_process_transcription(app, &inference).await;

    Ok(FileRun {
        inference,
        processed,
    })
}

/// Cancel an in-flight file transcription.
///
/// Reuses the dictation cancel machinery: `cancel_session` flips the
/// registered flag, which the engine checks before `state.full()` and
/// between segments.
///
/// Ignores an id that is not the in-flight file session. A stale cancel —
/// a click that lands just after the transcription returned, a frontend
/// that kept the id too long — would otherwise mark that id cancelled
/// forever, and since ids restart from zero after every dictation, the next
/// dictation to be handed that id would be silently dropped: no paste, no
/// history, no error. The skip-set is the authority on "is this still the
/// file session", because the command clears it the moment it is done.
#[tauri::command(rename_all = "snake_case")]
async fn cancel_audio_file(
    state: tauri::State<'_, AppState>,
    session_id: u64,
) -> Result<(), String> {
    if !state.is_dispatch_skipped(session_id) {
        log::info!("cancel_audio_file: session {session_id} is no longer in flight, ignoring");
        return Ok(());
    }
    state.cancel_session(session_id);
    Ok(())
}

#[tauri::command]
fn focus_main_window(app: AppHandle, tab: String) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        let _ = window.emit("navigate-tab", tab);
    }
    Ok(())
}

/// Open an arbitrary URL/scheme in the system handler. Used by the
/// permission banners to deep-link into macOS Privacy & Security panes
/// (e.g. `x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone`).
/// On Windows/Linux we fall back to `start`/`xdg-open`.
#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("open failed: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("xdg-open failed: {e}"))?;
    }
    #[cfg(windows)]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| format!("start failed: {e}"))?;
    }
    Ok(())
}

/// Validate a hotkey string at the UI layer — used by `SettingsPage` to
/// show an inline error as the user types. Pure parser call; no side
/// effects, no sidecar round-trip, hence `sync`. Returns `Err(msg)` for
/// any parse failure (unknown modifier, empty, modifier-only, etc.) and
/// `Ok(())` for a valid string.
#[tauri::command]
fn validate_hotkey(hotkey: String) -> Result<(), String> {
    crate::hotkey::parse(&hotkey).map(|_| ())
}

/// Persist a new hotkey: re-register the global shortcut (releasing the
/// previous binding atomically) and write the new value to
/// `config.json` directly.
///
/// Calls `hotkey::re_register` to swap the global shortcut binding,
/// then writes the new hotkey string into `config.json` via the
/// `Config` API. No IPC round-trip — everything runs in-process.
#[tauri::command]
async fn set_hotkey(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    hotkey: String,
    old_hotkey: Option<String>,
) -> Result<(), String> {
    // WS 4a1 Task 13b: `re_register` no longer takes a `SidecarHandle` —
    // it now operates directly on the shared `AppState` (which the hotkey
    // handler closure also captures, so the round-trip is consistent).
    let old = old_hotkey.unwrap_or_default();
    crate::hotkey::re_register(&app, &state, &old, &hotkey).inspect_err(|e| {
        let _ = app.emit("hotkey-error", e.clone());
    })?;
    // Direct write: load the existing config, patch `hotkey`, save.
    // Single-writer per Tauri command invocation; no locking needed.
    let mut config = crate::config::Config::load(&app).map_err(|e| format!("config load: {e}"))?;
    config
        .set("hotkey", serde_json::json!(hotkey))
        .map_err(|e| format!("config set: {e}"))?;
    config.save(&app).map_err(|e| format!("config save: {e}"))
}

/// Ask a provider which models it currently serves.
///
/// The key is looked up from the secret store here rather than passed in:
/// the frontend knows the ref, never the value, and this endpoint should
/// not become the one place that changes that.
///
/// Errors come back as plain strings for display next to the model field.
/// A failed list is not a failed configuration — the user can still type a
/// model id — so the caller must not treat it as fatal.
#[tauri::command(rename_all = "snake_case")]
async fn fetch_provider_models(
    provider: String,
    base_url: Option<String>,
    api_key_ref: Option<String>,
) -> Result<Vec<String>, String> {
    let api_key = match api_key_ref.filter(|value| !value.trim().is_empty()) {
        Some(reference) => crate::secret_store::get_key(&reference)
            .map_err(|e| format!("secret_store get_key({reference}): {e}"))?
            .unwrap_or_default(),
        None => String::new(),
    };
    crate::ai::models::fetch_models(&provider, base_url.as_deref(), &api_key).await
}

/// Start a recording session.
///
/// **WS 4a2 (Task 9)**: arms the cpal stream via `AudioRecorder::start()`
/// and emits a `recording-started` event carrying the new `session_id`
/// so the frontend can route a future `cancel_recording(session_id)` call
/// to the same session. Audio capture happens asynchronously on cpal's
/// real-time thread; the engine command is NOT sent here — it's sent in
/// `stop_recording` once the user releases the hotkey.
///
/// Emit `audio-level` events (~30 Hz) while a recording session is live so
/// the overlay waveform reacts to the user's voice. Reads the shared
/// recorder's EMA level and exits automatically when the recorder stops. A
/// static guard prevents overlapping pollers (only one session is ever
/// active at a time). Without this nothing emitted `audio-level`, so the
/// overlay waveform sat flat — see `OverlayApp.tsx` `listen("audio-level")`.
pub(crate) fn spawn_level_emitter(app: &AppHandle, recorder: Arc<crate::audio::AudioRecorder>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static EMITTING: AtomicBool = AtomicBool::new(false);
    if EMITTING.swap(true, Ordering::AcqRel) {
        return; // a poller is already running for the current session
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut tick: u32 = 0;
        loop {
            while recorder.is_recording() {
                let raw = recorder.level();
                let level = crate::audio::display_level(raw);
                let _ = app.emit("audio-level", serde_json::json!({ "level": level }));
                // Throttled (~1 Hz) diagnostic so app.log reveals the real level
                // if the meter ever looks dead again (log both raw + mapped).
                if tick.is_multiple_of(30) {
                    log::info!("audio-level poll: raw={raw:.4} mapped={level:.4}");
                }
                tick = tick.wrapping_add(1);
                std::thread::sleep(std::time::Duration::from_millis(33));
            }
            // Recording stopped — release ownership.
            EMITTING.store(false, Ordering::Release);
            // A start that raced with the `store` above would have found
            // EMITTING still true and skipped spawning its own poller, leaving
            // a live session with a flat waveform. If recording is active
            // again, reclaim ownership (swap false→true) and keep polling so
            // that session still gets `audio-level` events. If someone else
            // already re-acquired it (swap returns true), exit.
            if recorder.is_recording() && !EMITTING.swap(true, Ordering::AcqRel) {
                continue;
            }
            break;
        }
    });
}

/// The refusal shown when a recording is attempted while a file
/// transcription owns the engine.
///
/// Emits `whisper-failed` as well as returning the text: the hotkey is the
/// path people actually use, and it has no return value the user ever sees
/// — the overlay is the only surface that reaches them.
pub(crate) fn engine_busy_message(app: &AppHandle) -> String {
    let message = crate::ui_text::t("Идёт транскрипция файла — дождитесь её окончания.");
    let _ = app.emit(
        "whisper-failed",
        serde_json::json!({ "message": message.clone() }),
    );
    message
}

/// Whether anything could transcribe a recording started right now.
///
/// Cloud mode routes to a remote model and needs no local weights. Every
/// other mode needs a local one — loaded, or at least downloaded, so a press
/// during the startup auto-load is not refused for a model that is on its
/// way up. With neither, a recording only produces audio nobody can read:
/// the overlay opens on a dictation that cannot finish, which reads as a bug
/// rather than as the missing download it is.
pub(crate) fn transcription_route_available(app: &AppHandle, state: &AppState) -> bool {
    // A config that will not load is not evidence of a missing model, and
    // refusing to record over it would be the worse failure of the two.
    let Ok(config) = crate::config::Config::load(app) else {
        return true;
    };
    let pipeline_mode = config
        .as_value()
        .get("ai_processing")
        .and_then(|value| value.get("pipeline_mode"))
        .and_then(Value::as_str)
        .unwrap_or("local")
        .to_string();
    let loaded = crate::mutex_recover::lock(&state.engine_current_model).clone();
    let selected_downloaded = config
        .get_string("model")
        .is_some_and(|model| crate::model::is_downloaded(&model));
    has_transcription_route(&pipeline_mode, loaded.as_deref(), selected_downloaded)
}

/// The decision behind [`transcription_route_available`], split out from the
/// config and engine reads so it can be tested without a running app.
fn has_transcription_route(
    pipeline_mode: &str,
    loaded_model: Option<&str>,
    selected_downloaded: bool,
) -> bool {
    pipeline_mode == "cloud" || loaded_model.is_some() || selected_downloaded
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
mod transcription_route_tests {
    use super::has_transcription_route;

    #[test]
    fn cloud_mode_needs_no_local_weights() {
        assert!(has_transcription_route("cloud", None, false));
    }

    #[test]
    fn a_downloaded_model_counts_before_it_is_loaded() {
        // The startup auto-load has not finished yet; the Transcribe queues
        // behind its SetModel and still gets served.
        assert!(has_transcription_route("local", None, true));
    }

    #[test]
    fn a_loaded_model_counts_after_its_file_is_gone() {
        // Deleting the active model unloads it first, so this pairing only
        // arises from a file removed behind the app's back.
        assert!(has_transcription_route("hybrid", Some("turbo"), false));
    }

    #[test]
    fn local_mode_without_a_model_has_no_route() {
        assert!(!has_transcription_route("local", None, false));
        assert!(!has_transcription_route("hybrid", None, false));
    }
}

/// Message for a start refused by [`transcription_route_available`].
///
/// Deliberately does not emit `whisper-failed`: that drives the overlay, and
/// the whole point of the refusal is that no overlay appears for a recording
/// that never began. The main window carries a standing banner for this
/// state, derived from the same three inputs.
pub(crate) fn no_transcription_route_message(app: &AppHandle) -> String {
    let message = crate::ui_text::t(
        "Модель распознавания не скачана — записывать нечем. Скачайте модель в настройках или включите облачную обработку.",
    );
    let _ = app.emit("hotkey-error", &message);
    message
}

/// Why a dictation was refused before it began, and what to tell the user.
pub(crate) struct DictationRefusal {
    pub reason: telemetry::FailureReason,
    pub message: String,
}

/// The two refusals that guard the start of a dictation.
///
/// Shared by the `start_recording` command and the global hotkey, which are
/// otherwise independent implementations of the same sequence. Telemetry is
/// recorded here rather than by each caller: the place that knows which
/// refusal happened is the only one that can label it, and keeping that
/// knowledge in two files is how the two paths drift apart.
pub(crate) fn refuse_dictation_start(
    app: &AppHandle,
    state: &AppState,
) -> Option<DictationRefusal> {
    let refusal = if state.is_engine_busy() {
        DictationRefusal {
            reason: telemetry::FailureReason::EngineBusy,
            message: engine_busy_message(app),
        }
    } else if !transcription_route_available(app, state) {
        DictationRefusal {
            reason: telemetry::FailureReason::NoTranscriptionRoute,
            message: no_transcription_route_message(app),
        }
    } else {
        return None;
    };
    app.state::<telemetry::Telemetry>().record_failed(
        telemetry::Source::Microphone,
        &telemetry_pipeline_mode_of(app),
        telemetry::FailureStage::Start,
        refusal.reason,
    );
    Some(refusal)
}

/// The recorder refused to arm — permission denied, device gone.
///
/// Not [`abandon_dictation`]: nothing was recording, so there is no FSM to
/// reset and no stop cue to play. Shared only so the two callers cannot label
/// the same failure differently.
pub(crate) fn record_recorder_start_failure(app: &AppHandle) {
    app.state::<telemetry::Telemetry>().record_failed(
        telemetry::Source::Microphone,
        &telemetry_pipeline_mode_of(app),
        telemetry::FailureStage::Capture,
        telemetry::FailureReason::RecorderStart,
    );
}

/// The engine queue would not take the command — full, or closed.
pub(crate) fn record_engine_queue_failure(app: &AppHandle, config: Option<&crate::config::Config>) {
    app.state::<telemetry::Telemetry>().record_failed(
        telemetry::Source::Microphone,
        &telemetry_pipeline_mode(config),
        telemetry::FailureStage::Queue,
        telemetry::FailureReason::EngineQueue,
    );
}

/// A stop that produced nothing usable — silence, or a recorder error.
///
/// Returns the FSM to `Idle`, runs the stop hooks and records the failure.
/// Skipping the FSM reset is what left the hotkey path stuck in `Recording`
/// until the next successful dictation.
pub(crate) fn abandon_dictation(
    app: &AppHandle,
    state: &AppState,
    session_id: u64,
    reason: telemetry::FailureReason,
) {
    crate::state::set_app_fsm(&state.app_fsm, AppFsm::Idle);
    on_recording_stopped(app, session_id, None);
    app.state::<telemetry::Telemetry>().record_failed(
        telemetry::Source::Microphone,
        &telemetry_pipeline_mode_of(app),
        telemetry::FailureStage::Capture,
        reason,
    );
}

/// Trim the captured audio and wrap it in the command the engine expects.
///
/// The cloud branch used to exist in three copies — the command, the hotkey
/// and file transcription — and the hotkey's was missing for long enough that
/// a cloud-configured user's hotkey silently ran the local model.
///
/// Call after `on_recording_stopped`, so a debug dump keeps the untrimmed
/// audio: that is exactly what you want when the complaint is that the trim
/// ate a word.
pub(crate) fn build_dictation_command(
    app: &AppHandle,
    config: Option<&crate::config::Config>,
    session_id: u64,
    audio: Arc<Vec<f32>>,
    cancel_flag: Arc<AtomicBool>,
    reply: tokio::sync::oneshot::Sender<crate::whisper::InferenceResult>,
) -> Result<crate::whisper::EngineCommand, String> {
    let audio = match config {
        Some(cfg) => crate::vad::trim_for_transcription(cfg.as_value(), audio),
        None => audio,
    };
    let pipeline_mode = telemetry_pipeline_mode(config);
    if pipeline_mode != "cloud" {
        return Ok(crate::whisper::EngineCommand::Transcribe {
            session_id,
            audio,
            cancel_flag,
            // Configured whisper language (e.g. "ru"); None auto-detects.
            // Without it the engine falls back to whisper.cpp's "en" default
            // and mis-decodes non-English dictation.
            language: config.and_then(|cfg| cfg.get_string("language")),
            initial_prompt: config.and_then(custom_words_prompt),
            reply,
        });
    }
    // A cloud-configured user has no local model loaded, so sending
    // `Transcribe` would fail with «модель не загружена» for a reason that
    // has nothing to do with their setup.
    let request = build_cloud_stt_request(app, &audio).map_err(|error| {
        log::error!("session {session_id}: cloud STT setup failed: {error}");
        app.state::<telemetry::Telemetry>().record_failed(
            telemetry::Source::Microphone,
            &pipeline_mode,
            telemetry::FailureStage::Queue,
            telemetry::FailureReason::CloudConfiguration,
        );
        error
    })?;
    Ok(crate::whisper::EngineCommand::TranscribeCloud {
        session_id,
        audio,
        cancel_flag,
        request,
        reply,
    })
}

/// Everything that should happen when capture begins, beyond starting the
/// recorder itself.
///
/// Exists because there are two ways to start a recording — the global
/// hotkey (`hotkey::hotkey_do_start`) and the `start_recording` command —
/// and the hotkey is the one people actually use. Anything hung off only
/// the command silently does not exist in practice.
pub(crate) fn on_recording_started(app: &AppHandle) {
    crate::sounds::play(app, crate::sounds::Cue::Start);
    // After the start cue, so the cue itself is still audible.
    if let Ok(cfg) = crate::config::Config::load(app) {
        crate::output_volume::duck(cfg.as_value());
    }
    let state = app.state::<AppState>();
    let model = restore_unloaded_model(app, &state);
    let session_id = state.current_session_id.load(Ordering::Acquire);
    let armed = start_live_preview(&state, session_id, model.as_deref());
    // The overlay sized for live text looks different, and it must know that
    // from the start of the recording rather than from the first recognised
    // word: otherwise the window changes shape mid-phrase.
    let _ = app.emit(
        "live-preview-armed",
        serde_json::json!({ "session_id": session_id, "armed": armed }),
    );
}

/// The mirror of [`on_recording_started`]: everything that should happen
/// when capture ends, whichever path ended it.
///
/// `audio` is `None` when nothing usable was captured — silence, or a
/// recorder error. From the user's side that is the same outcome as a
/// failure (no text appears), so it gets the same cue.
pub(crate) fn on_recording_stopped(app: &AppHandle, session_id: u64, audio: Option<&[f32]>) {
    // Unconditional and first: a recording that ends without this leaves
    // the machine quiet with no indication why.
    crate::output_volume::restore();
    // The tap is detached here rather than in the stop commands: there are
    // several ways out of a recording, and this is the only one they all take.
    app.state::<AppState>().recorder.detach_live_tap();

    let Some(samples) = audio else {
        crate::sounds::play(app, crate::sounds::Cue::Error);
        return;
    };

    let Ok(cfg) = crate::config::Config::load(app) else {
        return;
    };
    // A cancel may arrive while the audio worker is finalizing, and a
    // cancelled capture must not be written out as a successful debug
    // recording. A plain read, not a lock held across the write: both callers
    // check the same marker immediately before calling in, and holding the
    // cancellation mutex across file I/O would park every concurrent
    // `request_cancel`/`begin_commit` for the length of a WAV write.
    if app.state::<AppState>().is_cancelled(session_id) {
        return;
    }
    if let Some(path) = crate::debug::save_recording(cfg.as_value(), session_id, samples) {
        log::info!(
            "session {session_id}: recording saved to {}",
            path.display()
        );
    }
    // Toggle mode only — see `sounds::Cue::Stop`.
    if cfg.get_string("recording_mode").as_deref() == Some("toggle") {
        crate::sounds::play(app, crate::sounds::Cue::Stop);
    }
}

/// Returns the `session_id` so any sync caller can immediately use it
/// (e.g. for cancel). The frontend currently discards the return value
/// (it reads session_id from the `recording-started` event payload).
#[tauri::command]
async fn start_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<u64, String> {
    let telemetry = app.state::<telemetry::Telemetry>().clone();
    if let Some(refusal) = refuse_dictation_start(&app, &state) {
        return Err(refusal.message);
    }
    // Arm the cpal stream on the audio worker. Errors here surface
    // permission denials / missing-device conditions loudly to the UI.
    let recorder = Arc::clone(&state.recorder);
    let selected =
        crate::config::microphone_selection(crate::config::Config::load(&app)?.get("microphone"));
    let started = match state
        .audio
        .call(move || recorder.start_selected(selected.as_deref()))
        .await
    {
        Ok(inner) => inner,
        Err(error) => Err(error),
    };
    if let Err(error) = started {
        record_recorder_start_failure(&app);
        return Err(error);
    }
    spawn_level_emitter(&app, Arc::clone(&state.recorder));
    let session_id = state.next_session_id();
    state.begin_session(session_id);
    telemetry.begin_usage_session(telemetry::SessionTrigger::Microphone);
    // WS 4a2b Task 3: store the freshly-allocated session_id in the
    // shared AtomicU64 so the matching `stop_recording` (Tauri command OR
    // hotkey Released branch) can swap(0) and pair them correctly. Without
    // this, stop_recording allocates a NEW id and the dispatcher can't
    // correlate the cancelled-check / dispatch.
    state
        .current_session_id
        .store(session_id, Ordering::Release);
    crate::state::set_app_fsm(&state.app_fsm, AppFsm::Recording);
    // After `recorder.start` succeeded, so the cue means "the microphone is
    // live" and not merely "the hotkey registered".
    on_recording_started(&app);
    // Emit session_id in event payload so the frontend can use it for
    // cancel_recording later. This is a payload-shape change from WS 4a1
    // (was `()`); existing TS frontend discards the payload so it's
    // runtime-compatible.
    let _ = app.emit("recording-started", session_id);
    Ok(session_id)
}

/// Tap audio into the loaded or queued model if it supports live text.
///
/// Called from [`on_recording_started`] rather than from the `start_recording`
/// command: dictation is launched by a hotkey, which has its own start path, and
/// a preview hung on the command did not exist in real life.
///
/// Forwarding is done by a separate thread rather than the engine thread: that
/// one is busy with commands and cannot wait on two channels at once. Overflowing
/// the command queue loses a preview chunk — which is acceptable, since the full
/// recording accumulates separately anyway and goes to transcription whole. The
/// preview never occupies the last places in the engine's queue.
///
/// There is one queue for every command. A hypothesis is a draft and losing it
/// costs one skipped frame; the final transcription is the entire recording, and
/// losing it costs everything that was said. A streaming model that fell behind
/// managed to fill the queue with preview chunks, after which a `try_send`
/// carrying the recording was rejected and the dictation was lost.
const PREVIEW_QUEUE_RESERVE: usize = 16;

/// Whether the queue has room for one more preview chunk.
///
/// `capacity` is the free slots, not the size of the queue.
fn preview_has_room(capacity: usize) -> bool {
    capacity > PREVIEW_QUEUE_RESERVE
}

fn start_live_preview(state: &AppState, session_id: u64, model: Option<&str>) -> bool {
    let streams = model
        .and_then(|model| crate::model::model_engine(model).ok())
        .is_some_and(|engine| engine.is_streaming());
    if !streams {
        log::debug!(
            "session {session_id}: live preview off, model {:?} is not streaming",
            model.unwrap_or("<none>")
        );
        return false;
    }
    log::info!(
        "session {session_id}: live preview on, model {:?}",
        model.unwrap_or("<none>")
    );
    // A queue of roughly one second of audio: the cpal callback hands over one
    // chunk per call.
    let rx = state.recorder.attach_live_tap(48);
    let engine_tx = state.engine_cmd_tx.clone();
    let _ = engine_tx.try_send(crate::whisper::EngineCommand::PreviewReset { session_id });
    std::thread::spawn(move || {
        // The channel breaks when the recording stops and the tap is detached —
        // that is exactly the exit condition.
        while let Ok(samples) = rx.recv() {
            // Room for real commands is preserved before sending: the queue is
            // shared, and a slot taken here is a slot the recording will not
            // have.
            if !preview_has_room(engine_tx.capacity()) {
                log::debug!("session {session_id}: preview chunk dropped, queue reserved");
                continue;
            }
            if engine_tx
                .try_send(crate::whisper::EngineCommand::PreviewChunk {
                    session_id,
                    samples,
                })
                .is_err()
            {
                log::debug!("session {session_id}: preview chunk dropped, engine busy");
            }
        }
        log::debug!("session {session_id}: live preview tap closed");
    });
    true
}

/// Stop the active recording session and send the captured audio to the
/// whisper engine.
///
/// **WS 4a2 (Task 9)**: drops the cpal stream (canonical drop-and-drain),
/// pulls the captured buffer, wraps it in an `Arc`, and forwards it as
/// `EngineCommand::Transcribe` to the engine thread via
/// `state.engine_cmd_tx`. Returns the new `session_id` so the frontend
/// can track / cancel the in-flight transcription (idempotent if no
/// recording was active — returns 0).
#[tauri::command]
async fn stop_recording(app: AppHandle, state: tauri::State<'_, AppState>) -> Result<u64, String> {
    let telemetry = app.state::<telemetry::Telemetry>().clone();
    // WS 4a2b Task 4: reuse the session_id stored by start_recording
    // (or hotkey Pressed in Task 7) via swap(0). If the swap returns 0
    // there is no active session — idempotent return 0 to the UI.
    let session_id = state.current_session_id.swap(0, Ordering::AcqRel);
    // Reset toggle arm if this was a toggle-mode session. Must happen
    // after the swap but BEFORE any early return so the flag is always
    // cleared when recording stops, regardless of the path.
    state.toggle_armed.store(false, Ordering::Release);
    if session_id == 0 {
        return Ok(0);
    }
    // Dropping the cpal stream joins its audio thread; that has to happen
    // on the audio worker, never on a thread anyone else is waiting for.
    let recorder = Arc::clone(&state.recorder);
    let stopped = match state.audio.call(move || recorder.stop()).await {
        Ok(value) => value,
        Err(error) => {
            if state.is_cancelled(session_id) {
                state.finish_session(session_id);
                let _ = app.emit("whisper-cancelled", session_id);
                return Ok(session_id);
            }
            state.finish_session(session_id);
            return Err(error);
        }
    };
    let audio = match stopped {
        Ok(Some(a)) if !a.is_empty() => a,
        Ok(_) => {
            // Recorder returned None (empty buffer) or an empty Vec —
            // skip transcription, return to Idle so the UI doesn't stay
            // stuck in Processing.
            log::info!("session {session_id}: empty audio, skip transcription");
            if state.is_cancelled(session_id) {
                state.finish_session(session_id);
                let _ = app.emit("whisper-cancelled", session_id);
                return Ok(session_id);
            }
            abandon_dictation(&app, &state, session_id, telemetry::FailureReason::NoAudio);
            state.finish_session(session_id);
            return Ok(session_id);
        }
        Err(e) => {
            log::error!("session {session_id}: recorder.stop failed: {e}");
            if state.is_cancelled(session_id) {
                state.finish_session(session_id);
                let _ = app.emit("whisper-cancelled", session_id);
                // Cancellation won the race with recorder finalization; this
                // is a successful terminal cancel, not a stop error for the
                // tray/overlay caller to surface.
                return Ok(session_id);
            }
            abandon_dictation(
                &app,
                &state,
                session_id,
                telemetry::FailureReason::RecorderStop,
            );
            state.finish_session(session_id);
            return Err(e);
        }
    };
    // Cancellation is set synchronously by the overlay command. Check it
    // before stop hooks/debug persistence and before constructing any engine
    // command; a click racing this worker must never become transcription.
    if state.is_cancelled(session_id) {
        state.finish_session(session_id);
        let _ = app.emit("whisper-cancelled", session_id);
        return Ok(session_id);
    }
    on_recording_stopped(&app, session_id, Some(&audio));
    let cancel_flag = Arc::new(AtomicBool::new(false));
    // Phase 4 / Batch 6 / P0: register the cancel flag so
    // `cancel_recording(session_id)` can flip it from outside the
    // engine thread. The engine checks the flag before
    // `state.full(...)` and short-circuits a cancel that lands
    // during the (otherwise uninterruptible) `.full()` C call.
    state.register_cancel_flag(session_id, cancel_flag.clone());
    let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();

    let loaded_config = crate::config::Config::load(&app).ok();
    let pipeline_mode = telemetry_pipeline_mode(loaded_config.as_ref());
    let command = match build_dictation_command(
        &app,
        loaded_config.as_ref(),
        session_id,
        audio,
        cancel_flag,
        reply_tx,
    ) {
        Ok(command) => command,
        Err(error) => {
            state.finish_session(session_id);
            crate::state::set_app_fsm(&state.app_fsm, AppFsm::Idle);
            return Err(error);
        }
    };
    let send_result = state.engine_cmd_tx.try_send(command);

    if let Err(e) = send_result {
        log::error!("session {session_id}: engine channel full/closed: {e}");
        crate::state::set_app_fsm(&state.app_fsm, AppFsm::Idle);
        state.finish_session(session_id);
        telemetry.record_failed(
            crate::telemetry::Source::Microphone,
            &pipeline_mode,
            telemetry::FailureStage::Queue,
            telemetry::FailureReason::EngineQueue,
        );
        return Err(format!("engine: {e}"));
    }
    crate::state::set_app_fsm(&state.app_fsm, AppFsm::Processing);
    let _ = app.emit("recording-stopped", session_id);
    Ok(session_id)
}

/// Cancel an in-flight recording / transcription session.
///
/// Marks the session as cancelled in `AppState` (the dispatcher checks
/// this BEFORE pasting — see `setup()` in this file). If a recording is
/// still active (user pressed hotkey then cancelled before releasing),
/// the cpal stream is dropped so the audio buffer is discarded.
///
/// **WS 4a2 (Task 9)**: the frontend MUST capture the session_id from
/// `recording-started` and pass it here. If it loses the id (refresh,
/// etc.), passing a wrong id is a no-op (session_id not in the cancelled
/// set — no harm).
#[tauri::command]
async fn cancel_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: u64,
) -> Result<bool, String> {
    let telemetry = app.state::<telemetry::Telemetry>().clone();
    // Set the cancellation marker BEFORE touching the recorder.  The stop
    // command may already be waiting on the audio worker; setting it only
    // after that await lets its stale completion queue Whisper anyway.
    let cancellation_requested = state.request_cancel(session_id);
    if !cancellation_requested {
        log::info!("cancel_recording: session {session_id} is no longer cancellable");
        return Ok(false);
    }
    // Claim the live session with the SAME atomic swap both stop paths use,
    // and do it before any await. `stop_recording` and `hotkey_do_stop` swap
    // this slot to 0 synchronously before they touch the recorder, so
    // whoever the swap hands `session_id` to is the one path that will run
    // this session's terminal cleanup: the loser sees 0 and bails out.
    // Loading the id instead — and re-using that answer after the await —
    // let both paths believe they owned the session. The cancel then wiped
    // the cancellation marker while the stop worker was still queued, and
    // the stop worker read a stopped recorder as "no audio captured": an
    // error cue and a false Capture/NoAudio failure after a clean cancel.
    let owns_live_recorder = state.claim_live_session(session_id);
    if owns_live_recorder && state.recorder.is_recording() {
        state.recorder.detach_live_tap();
        // Drop the live stream; discard the audio buffer. Errors here
        // are not fatal — the cancellation already took effect on the
        // dispatcher side. Goes through the audio worker: this is the
        // exact call whose `join()` used to wedge whatever thread ran it.
        let recorder = Arc::clone(&state.recorder);
        let _ = state.audio.call(move || recorder.stop()).await;
        // Cancel is a third way out of Recording, so it needs the volume
        // put back too. No cue: the user cancelled, they know.
        crate::output_volume::restore();
    }
    if owns_live_recorder {
        // No engine completion will arrive for a recording cancelled before
        // stop, so this command owns the terminal cleanup in that case.
        state.finish_session(session_id);
    }
    crate::state::set_app_fsm(&state.app_fsm, AppFsm::Idle);
    // Reset toggle_armed if this was a toggle-mode session so the
    // next hotkey press doesn't try to stop a non-existent recording.
    state.toggle_armed.store(false, Ordering::Release);
    if owns_live_recorder {
        // Nothing was ever queued, so the dispatcher will not emit
        // `whisper-cancelled` for this session — and that event is what the
        // UI uses to leave the "recording" state
        // (`desktop/src/bridge/recording.ts:67`). Without this, cancelling
        // mid-recording left the tray and the shell reading "Идёт запись"
        // forever.
        let _ = app.emit("whisper-cancelled", session_id);
        if session_id != 0 {
            telemetry.record_cancelled(
                crate::telemetry::Source::Microphone,
                &telemetry_pipeline_mode_of(&app),
            );
        }
    }
    Ok(true)
}

/// Start a microphone self-test session.
///
/// Creates a dedicated `AudioRecorder`, starts capturing audio, and
/// emits `microphone-test-started` / `microphone-test-level` events
/// at ~25 Hz so the frontend can render a VU meter. Returns the test
/// state info (`active: true` on success).
///
/// `monitor` turns on echo — the captured frames are also emitted as
/// `microphone-test-audio` for the frontend to play back. It is off by
/// default: the level check is a separate mode and must not send the
/// user's voice to the speakers on its own.
/// Helper: extract a panic message from `catch_unwind`'s `Box<dyn Any>`.
pub(crate) fn panic_msg(panic: Box<dyn std::any::Any + Send + 'static>) -> String {
    panic
        .downcast::<&str>()
        .map(|s| s.to_string())
        .or_else(|p| p.downcast::<String>().map(|s| *s))
        .unwrap_or_else(|_| "internal error".to_string())
}

#[tauri::command]
async fn start_microphone_test(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    microphone: Option<serde_json::Value>,
    monitor: Option<bool>,
) -> Result<crate::mic_test::MicrophoneTestInfo, String> {
    // catch_unwind prevents a panic inside cpal/audio from crashing
    // the app. The microphone test path can fail silently or hard-crash
    // on WASAPI exclusive-mode issues, device disconnects, etc.
    let test = state.microphone_test.clone();
    let app_for_worker = app.clone();
    let start_result = state
        .audio
        .call(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                test.start(
                    &app_for_worker,
                    crate::config::microphone_selection(microphone),
                    monitor.unwrap_or(false),
                )
            }))
        })
        .await?;
    match start_result {
        Ok(Ok(_)) => state.microphone_test.info(),
        Ok(Err(e)) => {
            let _ = app.emit("microphone-test-failed", serde_json::json!({"message": e}));
            Err(e)
        }
        Err(panic) => {
            let msg = panic_msg(panic);
            log::error!("microphone_test.start panicked: {msg}");
            let _ = app.emit(
                "microphone-test-failed",
                serde_json::json!({"message": msg}),
            );
            Err(msg)
        }
    }
}

/// Stop an active microphone self-test session.
///
/// Joins the poller/silence-watch threads, drops the dedicated
/// `AudioRecorder`, and emits `microphone-test-stopped`. Returns
/// the final test state info (`active: false`).
#[tauri::command]
async fn stop_microphone_test(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<crate::mic_test::MicrophoneTestInfo, String> {
    let test = state.microphone_test.clone();
    let app_for_worker = app.clone();
    let stop_result = state
        .audio
        .call(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test.stop(&app_for_worker)))
        })
        .await?;
    match stop_result {
        Ok(Ok(_)) => state.microphone_test.info(),
        Ok(Err(e)) => Err(e),
        Err(panic) => {
            let msg = panic_msg(panic);
            log::error!("microphone_test.stop panicked: {msg}");
            Err(msg)
        }
    }
}

/// Toggle echo monitoring on a running microphone test.
///
/// Separate from start/stop so switching echo on or off does not restart
/// the capture stream — the level meter keeps running across the toggle.
///
/// Dispatched through the audio worker like its two neighbours, even though it
/// only flips an atomic: the `inner` mutex it takes is the same one `start`
/// holds across `AudioRecorder::start_selected`, and opening a WASAPI device can
/// take hundreds of milliseconds. Nothing but a `busy` flag on the frontend
/// keeps the two commands apart today, and that invariant lives in another
/// language on the far side of an IPC boundary.
#[tauri::command]
async fn set_microphone_test_monitor(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<crate::mic_test::MicrophoneTestInfo, String> {
    let test = state.microphone_test.clone();
    state.audio.call(move || test.set_monitor(enabled)).await?;
    state.microphone_test.info()
}

/// Test the paste pipeline from the frontend.
/// Copies test text to clipboard and attempts to paste it via the
/// standard pipeline (enigo → osascript).
#[tauri::command]
async fn test_paste(app: AppHandle) -> Result<String, String> {
    let test_text = "Тест вставки Sotto — ".to_owned()
        + &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis().to_string())
            .unwrap_or_default();

    match crate::clipboard::paste_text(app, test_text.clone()) {
        Ok(()) => Ok(format!("Paste OK. Text на буфере: {test_text}")),
        Err(e) => Err(format!("Paste FAILED: {e}")),
    }
}

/// Copy-pasteable summary of the setup for a bug report.
/// Ready-made term sets for the dictionary.
///
/// We hand over the id and the words; the set's name is displayed by the
/// frontend because the name is translatable while the word list is not.
#[tauri::command]
fn dictionary_presets() -> Vec<(String, Vec<String>)> {
    crate::formatter::DICTIONARY_PRESETS
        .iter()
        .map(|set| {
            (
                set.id.to_string(),
                set.words.iter().map(|w| w.to_string()).collect(),
            )
        })
        .collect()
}

#[tauri::command]
fn get_diagnostics(app: AppHandle) -> Result<String, String> {
    let config = crate::config::Config::load(&app)?;
    Ok(crate::debug::diagnostics_report(&app, config.as_value()))
}

/// Reveal the diagnostics folder (logs + saved recordings) in the file
/// manager. `save_recordings` puts the WAVs in a subfolder of the same
/// place, so one button covers both.
#[tauri::command]
fn open_diagnostics_folder() -> Result<(), String> {
    crate::debug::open_in_file_manager(&crate::debug::diagnostics_dir())
}

/// Bytes the logs occupy: the active file plus its rotated archives.
#[tauri::command]
fn logs_size() -> u64 {
    crate::structured_log::logs_total_bytes()
}

/// Empty the logs, returning the resulting size so the caller does not
/// have to ask a second time.
///
/// Waits for the writer thread to finish, which is why it reports a size
/// that is actually true rather than one read mid-truncate. The wait is a
/// truncate and a few `remove_file` calls behind whatever is queued.
#[tauri::command]
fn clear_logs() -> u64 {
    crate::structured_log::clear();
    crate::structured_log::logs_total_bytes()
}

/// Marker the OS autostart entry launches the app with, so startup can tell
/// "the user opened the app" from "the session started".
const AUTOSTART_ARG: &str = "--autostart";

/// Push the `auto_start` config value into the OS autostart entry.
///
/// Non-fatal by design: on Windows this writes to the Run key in the user
/// registry hive, which a policy or a cleanup tool can make unwritable. That
/// is worth a log line, not a failed startup or a failed settings save.
fn apply_autostart(app: &AppHandle) {
    apply_autostart_inner(app, false)
}

/// Refresh the executable path after an update, even if an entry exists.
/// Keep the old entry until its replacement is ready.
fn refresh_autostart(app: &AppHandle) {
    apply_autostart_inner(app, true)
}

fn apply_autostart_inner(app: &AppHandle, rewrite_when_unchanged: bool) {
    // A portable copy must not erase or redirect the installed copy's entry.
    if crate::portable::data_dir().is_some() {
        return;
    }
    use tauri_plugin_autostart::ManagerExt;

    let wanted = crate::config::Config::load(app)
        .ok()
        .and_then(|c| c.get("auto_start"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let manager = app.autolaunch();
    let current = match manager.is_enabled() {
        Ok(value) => value,
        Err(error) => {
            log::warn!("autostart state unreadable: {error}");
            return;
        }
    };
    if current == wanted && !(rewrite_when_unchanged && wanted) {
        return;
    }
    let result: Result<(), String> = if wanted {
        // Windows enable() overwrites the Run value without deleting it.
        // The macOS plugin truncates the plist, so write it atomically instead.
        #[cfg(target_os = "macos")]
        {
            (|| {
                let home = dirs::home_dir().ok_or("home directory unavailable")?;
                let name = &app.package_info().name;
                let path = home
                    .join("Library/LaunchAgents")
                    .join(format!("{name}.plist"));
                let exe = std::env::current_exe().map_err(|e| e.to_string())?;
                crate::autostart::write_launch_agent(&path, name, &exe, AUTOSTART_ARG)
                    .map_err(|e| e.to_string())
            })()
        }
        #[cfg(not(target_os = "macos"))]
        {
            manager.enable().map_err(|e| e.to_string())
        }
    } else {
        manager.disable().map_err(|e| e.to_string())
    };
    match result {
        Ok(()) => log::info!("autostart set to {wanted}"),
        Err(error) => log::warn!("autostart could not be set to {wanted}: {error}"),
    }
}

/// Returns whether the app has macOS Accessibility permission.
/// The frontend calls this after the user follows the deep-link to
/// System Settings to see if the permission was granted.
#[tauri::command]
fn check_accessibility() -> bool {
    crate::accessibility::is_accessibility_granted()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Phase 4 / Batch 6 / P1: structured file logging. Installs
    // early (before any other setup) so every subsequent log line
    // ends up in `~/.speech_to_text/logs/app.log` with API keys
    // and bearer tokens redacted.
    let _ = crate::structured_log::install();

    tauri::Builder::default()
        // Must be registered first — the plugin decides whether this process
        // gets to become the app at all, before any other plugin sets up
        // state a second instance would duplicate. Two instances would both
        // try to claim the global hotkey and one would silently lose.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Launching the app again is how a user asks for the window when
            // they have forgotten it lives in the tray.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_ARG]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Only ever driven from Rust (`pick_audio_file`), so the JS API
        // stays out of `capabilities/default.json` — the webview cannot
        // open a file dialog, and the one place that can is a command that
        // decides for itself which extensions are offered.
        .plugin(tauri_plugin_dialog::init())
        .on_window_event(crate::window_state::handle)
        .setup(|app| {
            crate::window_state::restore(app.handle());
            // Before the tray: its single menu item is built immediately.
            if let Ok(cfg) = crate::config::Config::load(app.handle()) {
                crate::ui_text::set_from_config(cfg.as_value());
            }
            tray::build_tray(app.handle())?;

            // Started by the OS at login: the user did not ask to look at
            // the app, only to have the hotkey available. Everything else
            // (tray, engine, hotkey) is set up exactly as usual.
            if std::env::args().any(|arg| arg == AUTOSTART_ARG) {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            // Reconcile the OS autostart entry with config. Config is the
            // source of truth: the entry can go missing (profile migration,
            // a cleanup tool) and the setting would otherwise keep claiming
            // it is on. At startup the entry is rewritten even when it looks
            // right — see `refresh_autostart` for why a correct-looking entry
            // can still point at the wrong file.
            refresh_autostart(app.handle());

            // Normalise the compute-device settings before anything reads
            // them. Non-fatal: `resolve_device` copes with the legacy value
            // either way, this just stops the on-disk config from keeping a
            // spelling the UI no longer offers.
            // Logging installs before config is reachable (it has to catch
            // startup failures), so the configured level is applied here,
            // as early as an AppHandle exists.
            if let Ok(cfg) = crate::config::Config::load(app.handle()) {
                crate::structured_log::set_level(crate::debug::log_level_from_config(
                    cfg.as_value(),
                ));
                // Issue #24 instrumentation. Read here rather than at each
                // call site: the overlay worker has no AppHandle of its own,
                // and re-reading config on every transition would itself add
                // latency to the thing being measured.
                #[cfg(windows)]
                crate::windows::overlay_diag::configure(cfg.as_value());
            }

            match crate::config::migrate_legacy_device(app.handle()) {
                Ok(true) => log::info!("migrated legacy device/compute_type settings"),
                Ok(false) => {}
                Err(error) => log::warn!("device settings migration skipped: {error}"),
            }

            // macOS: check Accessibility permission on startup. If missing,
            // emit an `app-error` event so the frontend shows a banner with
            // a deep-link into System Settings → Privacy → Accessibility.
            #[cfg(target_os = "macos")]
            if !crate::accessibility::is_accessibility_granted() {
                crate::accessibility::emit_accessibility_error(app.handle());
            }

            // Shared state: tracks the model currently loaded into the
            // whisper engine. Created here so we can hand a clone to the
            // engine thread (which writes to it) and keep one for AppState
            // (which Tauri commands read).
            let engine_current_model: std::sync::Arc<std::sync::Mutex<Option<String>>> =
                std::sync::Arc::new(std::sync::Mutex::new(None));
            let engine_current_model_for_engine = std::sync::Arc::clone(&engine_current_model);

            // Spawn whisper engine thread.
            let (engine_cmd_tx, engine_cmd_rx) =
                tokio::sync::mpsc::channel::<crate::whisper::EngineCommand>(64);
            let (engine_event_tx, engine_event_rx) =
                tokio::sync::mpsc::channel::<crate::whisper::EngineEvent>(64);
            let engine_app_handle = app.handle().clone();
            let engine_handle = std::thread::spawn(move || {
                crate::whisper::engine_thread_main(
                    engine_cmd_rx,
                    engine_event_tx,
                    engine_app_handle,
                    engine_current_model_for_engine,
                );
            });

            // Init AudioRecorder (WS 4a2 Task 8). The recorder is lazy
            // about device acquisition — `new()` only probes the default
            // input device to pre-size the buffer; the actual cpal Stream
            // is built on first `start()` call. Failure here is fatal
            // (we want loud feedback on a broken mic permission setup).
            let recorder = std::sync::Arc::new(
                crate::audio::AudioRecorder::new(crate::audio::AudioConfig::default())
                    .map_err(|e| format!("AudioRecorder::new: {e}"))?,
            );

            // Microphone test harness: shares the same AudioRecorder so
            // the test can start/stop audio capture and poll levels.
            let microphone_test = crate::mic_test::MicrophoneTest::new();

            // WS 4b Task 7: open the SQLite data layer (stats + history)
            // and seed it from legacy `stats.json` / `history.json` if those
            // exist. Migration is idempotent (INSERT OR IGNORE / INSERT OR
            // REPLACE) and runs synchronously in setup() so the dispatcher
            // can rely on a fully-migrated DB by the time the first
            // transcription completes.
            //
            // Migration failures are NON-FATAL (warn + continue) — a broken
            // JSON file shouldn't prevent app startup. The DB connection
            // itself IS fatal (we can't run without it).
            let db = crate::db::open().map_err(|e| format!("db open: {e}"))?;
            let db_arc = std::sync::Arc::new(db);
            {
                let conn = crate::mutex_recover::lock(&db_arc);
                let config_dir = crate::db::db_path();
                if let Err(e) = crate::db::migrate_from_json(&conn, &config_dir) {
                    log::warn!("migration from JSON failed (non-fatal): {e}");
                }
                // Repairs counters rolled back by re-importing stats.json.
                // Does nothing on a healthy database.
                match crate::stats::reconcile_totals_with_daily(&conn) {
                    Ok(0) => {}
                    Ok(count) => {
                        log::info!("stats: {count} lifetime totals repaired from stats_daily")
                    }
                    Err(e) => log::warn!("stats reconcile failed (non-fatal): {e}"),
                }
            }

            // Product telemetry is independent from stats/history and is
            // deliberately non-fatal. The installation ID is random and
            // stored in SQLite meta; no machine/account fingerprinting is
            // used. A missing build key makes the whole path a no-op.
            let startup_config = crate::config::Config::load(app.handle()).ok();
            let telemetry =
                crate::telemetry::Telemetry::new(db_arc.clone(), startup_config.as_ref());
            app.manage(telemetry.clone());
            let autostart = std::env::args().any(|arg| arg == AUTOSTART_ARG);
            let ui_language = startup_config
                .as_ref()
                .and_then(|config| config.get_string("ui_language"))
                .unwrap_or_default();
            telemetry.record_app_started(autostart, &ui_language);

            let engine_state = crate::state::AppState::new(
                engine_cmd_tx,
                engine_handle,
                recorder,
                db_arc,
                microphone_test,
                engine_current_model,
            );
            // Capture the cancellable-sessions handle before moving
            // engine_state into Tauri's managed state. The dispatcher task
            // is `'static` (lives forever) so it needs an owned clone.
            // WS 4a2b Task 6: also capture app_fsm so the dispatcher can
            // reset FSM → Idle after every InferenceCompleted arm without
            // taking a tauri::State borrow (the task is owned by the
            // tokio runtime, not the Tauri command context).
            //
            // WS 4b Task 8: also capture `db` so the dispatcher can spawn
            // a blocking write of stats+history on every successful
            // transcription. The clone is `Arc<Mutex<Connection>>` —
            // `Arc::clone` is cheap and the `Mutex` serializes with the
            // Tauri commands that also use `state.db`.
            let dispatch_skipped = engine_state.dispatch_skipped_arc();
            let dispatch_app_fsm = engine_state.app_fsm.clone();
            let dispatch_db = engine_state.db.clone();
            let dispatch_state = engine_state.clone();
            let dispatch_telemetry = app.state::<crate::telemetry::Telemetry>().inner().clone();
            app.manage(engine_state);

            // Auto-load the configured model on startup. If the user has
            // a previously-downloaded model in config, send SetModel to
            // the engine thread so the overlay doesn't show "Модель не
            // загружена" on the first hotkey press.
            let auto_load_app = app.handle().clone();
            let auto_load_tx = app.state::<AppState>().engine_cmd_tx.clone();
            tauri::async_runtime::spawn(async move {
                // Both outcomes are logged. `.ok().and_then(...)` used to
                // collapse "config unreadable" and "no model configured"
                // into the same silent return, which is indistinguishable
                // from a healthy first run in the log.
                match crate::config::config_path(&auto_load_app) {
                    Ok(path) => {
                        log::info!("config: {} (exists: {})", path.display(), path.exists())
                    }
                    Err(error) => log::error!("config path unavailable: {error}"),
                }
                let config = match crate::config::Config::load(&auto_load_app) {
                    Ok(config) => config,
                    Err(error) => {
                        log::error!("config load failed, running with defaults: {error}");
                        return;
                    }
                };
                let model = match config.get_string("model") {
                    Some(model) => model,
                    None => {
                        log::info!("no model in config, skipping auto-load");
                        return;
                    }
                };
                if !crate::model::is_downloaded(&model) {
                    log::info!("config.model={model} not downloaded, skipping auto-load");
                    return;
                }
                match load_model_into_engine(
                    &auto_load_app,
                    &auto_load_tx,
                    &model,
                    crate::whisper::ModelLoadReason::Requested,
                )
                .await
                {
                    Ok(()) => log::info!("auto-loaded model {model} from saved config"),
                    Err(e) => log::warn!("auto-load failed: {e}"),
                }
            });

            // The idle watchdog. It unloads nothing itself: the decision is
            // made by the engine thread, which alone knows what it is doing and
            // for how long (see `EngineCommand::UnloadIdle`). All that comes
            // from here is a reason to check plus the threshold from settings.
            let idle_app = app.handle().clone();
            let idle_state = app.state::<AppState>().inner().clone();
            tauri::async_runtime::spawn(async move {
                let mut ticker = tokio::time::interval(IDLE_UNLOAD_TICK);
                // `interval` delivers its first tick immediately — we skip it:
                // zero seconds after startup there is nothing to be idle yet.
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    let Some(after) = idle_unload_after(&idle_app) else {
                        continue;
                    };
                    if crate::mutex_recover::lock(&idle_state.engine_current_model).is_none() {
                        continue;
                    }
                    // `try_send`: a busy queue means the engine is not idle, so
                    // there is nothing to ask it about.
                    let _ = idle_state
                        .engine_cmd_tx
                        .try_send(crate::whisper::EngineCommand::UnloadIdle { after });
                }
            });

            // Engine event dispatcher: EngineEvent → Tauri events + paste.
            // The engine_event_rx is a single-consumer; moving it into the
            // dispatcher task consumes it here. The task runs in Tauri's
            // tokio runtime so we can `.await` on the receiver.
            //
            // CRITICAL: the dispatcher checks cancellation BEFORE pasting.
            // Without this, a cancelled session's text would still be pasted
            // (G2 fix from plan review). Cancelled sessions are dropped from
            // the set on the dispatcher path so future invocations start
            // from a clean slate.
            let app_for_dispatch = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use crate::whisper::EngineEvent;
                let mut event_rx = engine_event_rx;
                while let Some(event) = event_rx.recv().await {
                    match event {
                        EngineEvent::ModelLoading { name } => {
                            let _ = app_for_dispatch.emit("whisper-loading", name);
                        }
                        EngineEvent::ModelReady { name } => {
                            let _ = app_for_dispatch.emit("whisper-ready", name);
                        }
                        EngineEvent::ModelUnloaded { name } => {
                            // The same event as an unload before deleting a
                            // model: the lists and the status refresh the same
                            // way, and they have no need to know exactly how the
                            // memory was freed.
                            let _ = app_for_dispatch.emit("model-unloaded", name);
                        }
                        EngineEvent::ModelRestored { name } => {
                            let _ = app_for_dispatch.emit("model-restored", name);
                        }
                        EngineEvent::ModelLoadFailed { name, error } => {
                            // Same contract fix as `whisper-failed`: the
                            // frontend reads `message`, not `error`.
                            let _ = app_for_dispatch.emit(
                                "whisper-load-failed",
                                serde_json::json!({
                                    "name": name,
                                    "message": error,
                                }),
                            );
                        }
                        EngineEvent::PreviewText { session_id, text } => {
                            // The previous dictation's hypothesis must not be
                            // appended to the current one's overlay: while the
                            // event travelled the channel, the recording could
                            // have changed.
                            let current = app_for_dispatch
                                .state::<AppState>()
                                .current_session_id
                                .load(Ordering::Acquire);
                            if current != session_id {
                                continue;
                            }
                            let _ = app_for_dispatch.emit(
                                "transcription-delta",
                                serde_json::json!({
                                    "session_id": session_id,
                                    "text": text,
                                }),
                            );
                        }
                        EngineEvent::InferenceStarted { session_id } => {
                            // `contains`, not `remove`: the entry has to
                            // survive until `InferenceCompleted`, which is
                            // the event that would otherwise paste a file
                            // transcription into the focused window. Emitting
                            // `whisper-started` here would raise the overlay
                            // (overlay.rs) with nothing left to lower it.
                            if crate::mutex_recover::lock(&dispatch_skipped).contains(&session_id) {
                                continue;
                            }
                            let _ = app_for_dispatch.emit("whisper-started", session_id);
                        }
                        EngineEvent::InferenceCompleted { session_id, result } => {
                            // The session belongs to a caller that awaits the
                            // engine's `oneshot` reply itself (file
                            // transcription). None of the dictation delivery
                            // below applies to it: no paste, no history, no
                            // stats, no FSM transition. Removed here — one
                            // completion per session, so the entry is gone
                            // for good and cannot swallow a later dictation
                            // that reuses the id.
                            if crate::mutex_recover::lock(&dispatch_skipped).remove(&session_id) {
                                log::info!("session {session_id} dispatch skipped (file job)");
                                continue;
                            }
                            // The cancelled entry is REMOVED here, in the same
                            // critical section that reads it, so the same
                            // session_id can never trip the check twice. Which
                            // branch this becomes is decided by
                            // `classify_completion` — see its doc comment for
                            // why that decision does not live inline.
                            // Keep a cancellation marker alive until the
                            // entire post-processing/delivery pipeline has
                            // settled. Removing it here made a click during
                            // the LLM await look like a fresh success.
                            let cancelled = dispatch_state.is_cancelled(session_id);
                            match classify_completion(cancelled, result) {
                                Completion::Cancelled => {
                                    log::info!("session {session_id} cancelled, skipping paste");
                                    dispatch_state.finish_session(session_id);
                                    crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                    let _ = app_for_dispatch.emit("whisper-cancelled", session_id);
                                    dispatch_telemetry.record_cancelled(
                                        crate::telemetry::Source::Microphone,
                                        &telemetry_pipeline_mode_of(&app_for_dispatch),
                                    );
                                }
                                Completion::Empty => {
                                    log::info!("session {session_id} empty transcription");
                                    dispatch_state.finish_session(session_id);
                                    let _ = app_for_dispatch.emit("whisper-empty", session_id);
                                    crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                    crate::sounds::play(
                                        &app_for_dispatch,
                                        crate::sounds::Cue::Error,
                                    );
                                    dispatch_telemetry.record_failed(
                                        crate::telemetry::Source::Microphone,
                                        &telemetry_pipeline_mode_of(&app_for_dispatch),
                                        telemetry::FailureStage::Stt,
                                        telemetry::FailureReason::EmptyTranscript,
                                    );
                                }
                                Completion::Failed(message) => {
                                    dispatch_state.finish_session(session_id);
                                    crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                    crate::sounds::play(
                                        &app_for_dispatch,
                                        crate::sounds::Cue::Error,
                                    );
                                    // The frontend ErrorPayload contract
                                    // (`desktop/src/overlay/OverlayApp.tsx`)
                                    // is `{ message?: string }`. Emit the
                                    // failure reason under that key so the
                                    // overlay shows the real cause instead
                                    // of its hardcoded fallback.
                                    let _ = app_for_dispatch.emit(
                                        "whisper-failed",
                                        serde_json::json!({
                                            "session_id": session_id,
                                            "message": message,
                                        }),
                                    );
                                    dispatch_telemetry.record_failed(
                                        crate::telemetry::Source::Microphone,
                                        &telemetry_pipeline_mode_of(&app_for_dispatch),
                                        telemetry::FailureStage::Stt,
                                        telemetry::FailureReason::EngineError,
                                    );
                                }
                                Completion::Transcribed(inference) => {
                                    // The engine completion and the overlay
                                    // cancel are independent async events. Do
                                    // not even enter formatting/LLM when the
                                    // cancel won the race.
                                    if dispatch_state.is_cancelled(session_id) {
                                        dispatch_state.finish_session(session_id);
                                        crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                        let _ =
                                            app_for_dispatch.emit("whisper-cancelled", session_id);
                                        dispatch_telemetry.record_cancelled(
                                            crate::telemetry::Source::Microphone,
                                            &telemetry_pipeline_mode_of(&app_for_dispatch),
                                        );
                                        continue;
                                    }
                                    let _ = app_for_dispatch.emit("whisper-done", &inference);

                                    // Post-Whisper pipeline: local formatting +
                                    // optional LLM cleanup. This produces the
                                    // final text to paste AND the raw/formatted
                                    // stages + AI status the history diff needs.
                                    // Awaited inline (the dispatcher is async);
                                    // in hybrid mode the paste is intentionally
                                    // delayed by the LLM round-trip.
                                    let processed =
                                        post_process_transcription(&app_for_dispatch, &inference)
                                            .await;
                                    if dispatch_state.is_cancelled(session_id) {
                                        dispatch_state.finish_session(session_id);
                                        crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                        let _ =
                                            app_for_dispatch.emit("whisper-cancelled", session_id);
                                        dispatch_telemetry.record_cancelled(
                                            crate::telemetry::Source::Microphone,
                                            &telemetry_pipeline_mode_of(&app_for_dispatch),
                                        );
                                        continue;
                                    }

                                    // WS 4b Task 8: stats + history write via
                                    // `spawn_blocking`. The Connection is
                                    // `!Send` so we can't hold the lock across
                                    // `.await` from the async dispatcher
                                    // thread; instead we hand the work off to a
                                    // blocking worker and `await` the result.
                                    // DB write failures are NON-FATAL — a
                                    // broken DB must NOT prevent paste.
                                    let db = dispatch_db.clone();
                                    let lang = inference.language.clone();
                                    let transcription_model = inference.model_id.clone();
                                    let inf_ms = inference.inference_time_ms;
                                    let audio_secs = inference.audio_seconds;
                                    let sess_id = inference.session_id;
                                    // `processed` is not used after this point, so
                                    // move its fields out instead of cloning all
                                    // six. `final_text` alone is needed twice (the
                                    // DB write below and the paste closure), so it
                                    // is the only field that still needs a clone.
                                    let ProcessedTranscription {
                                        raw_text,
                                        formatted_text,
                                        final_text,
                                        ai_json,
                                        ai_status,
                                        stats_json,
                                        system_prompt,
                                    } = processed;
                                    // One config read for every telemetry
                                    // field below: this sits between "STT
                                    // finished" and "text pasted", the part of
                                    // the run the user is waiting through.
                                    let telemetry_config =
                                        crate::config::Config::load(&app_for_dispatch).ok();
                                    let telemetry_pipeline =
                                        telemetry_pipeline_mode(telemetry_config.as_ref());

                                    // The post-processor emptied the text —
                                    // the whole transcription was a Whisper
                                    // silence hallucination. Treat it exactly
                                    // like an empty transcription: no paste,
                                    // no history entry, no stats. `whisper-done`
                                    // already fired above, so `whisper-empty`
                                    // is what corrects the overlay from "Текст
                                    // готов" back to idle.
                                    if !is_deliverable(&final_text) {
                                        log::info!(
                                            "session {session_id} produced only hallucinations, \
                                             skipping paste"
                                        );
                                        let _ = app_for_dispatch.emit("whisper-empty", session_id);
                                        dispatch_state.finish_session(session_id);
                                        crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                        crate::sounds::play(
                                            &app_for_dispatch,
                                            crate::sounds::Cue::Error,
                                        );
                                        dispatch_telemetry.record_failed(
                                            crate::telemetry::Source::Microphone,
                                            &telemetry_pipeline,
                                            telemetry::FailureStage::PostProcess,
                                            telemetry::FailureReason::EmptyAfterProcessing,
                                        );
                                        continue;
                                    }

                                    // Atomically claim final delivery before
                                    // any stats/history write. A cancel that
                                    // arrives first keeps this session out of
                                    // all successful side effects; a cancel
                                    // after this point is too late to turn a
                                    // committed session into a false success.
                                    if !dispatch_state.begin_commit(session_id) {
                                        let was_cancelled = dispatch_state.is_cancelled(session_id);
                                        dispatch_state.finish_session(session_id);
                                        crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                        if was_cancelled {
                                            let _ = app_for_dispatch
                                                .emit("whisper-cancelled", session_id);
                                            dispatch_telemetry.record_cancelled(
                                                crate::telemetry::Source::Microphone,
                                                &telemetry_pipeline_mode_of(&app_for_dispatch),
                                            );
                                        }
                                        continue;
                                    }

                                    let paste_text = final_text.clone();
                                    // Measured on the text that is actually
                                    // going in. `whisper-done` fires before the
                                    // LLM runs, so anything counted there is the
                                    // pre-cleanup draft.
                                    let pasted_length = paste_text.chars().count();
                                    // Same reason: whether the LLM fell back to
                                    // the local text is only known now, so the
                                    // overlay's warning has to ride along with
                                    // the paste rather than with `whisper-done`.
                                    let paste_ai = ai_status.as_ref().map(|status| {
                                        serde_json::json!({
                                            "fallback": status.fallback,
                                            "skipped_reason": status.skipped_reason,
                                        })
                                    });
                                    let telemetry_for_paste = dispatch_telemetry.clone();
                                    let dictation_telemetry = DictationTelemetry::capture(
                                        telemetry_config.as_ref(),
                                        &telemetry_pipeline,
                                        transcription_model.clone(),
                                        audio_secs,
                                        inf_ms,
                                        ai_status.clone(),
                                    );
                                    let (stats_tx, stats_rx) =
                                        tokio::sync::oneshot::channel::<Result<(), String>>();
                                    tokio::task::spawn_blocking(move || {
                                        // LLM outcome first: it is the one
                                        // aggregate that survives history
                                        // pruning, so it must not be skipped
                                        // when a later write fails.
                                        if let Some(status) = &ai_status {
                                            if let Err(e) =
                                                crate::stats::record_ai_outcome(&db, status)
                                            {
                                                log::warn!(
                                                    "llm stats write failed (non-fatal): {e}"
                                                );
                                            }
                                        }
                                        let result = crate::stats::record_transcription(
                                            &db,
                                            &final_text,
                                            lang.as_deref(),
                                            inf_ms,
                                            audio_secs,
                                            crate::stats::TIME_SAVED_CPM_FALLBACK,
                                        )
                                        .and_then(|_| {
                                            crate::history::append_entry(
                                                &db,
                                                &crate::history::NewEntry {
                                                    text: &final_text,
                                                    raw_text: &raw_text,
                                                    formatted_text: &formatted_text,
                                                    session_id: Some(sess_id),
                                                    language: lang.as_deref(),
                                                    inference_time_ms: inf_ms,
                                                    ai_processing_json: ai_json.as_deref(),
                                                    processing_stats_json: Some(&stats_json),
                                                    system_prompt: system_prompt.as_deref(),
                                                    transcription_model: transcription_model
                                                        .as_deref(),
                                                },
                                            )
                                            .map(|_| ())
                                        })
                                        .map_err(|e| e.to_string());
                                        let _ = stats_tx.send(result);
                                    });
                                    match stats_rx.await {
                                        Ok(Ok(())) => {}
                                        Ok(Err(e)) => log::warn!(
                                            "stats/history write failed (non-fatal): {e}"
                                        ),
                                        Err(_) => log::warn!(
                                            "stats/history worker channel closed (non-fatal)"
                                        ),
                                    }

                                    // Tell the frontend a history entry
                                    // was just appended so HistoryPage can
                                    // re-fetch. Payload is the session_id
                                    // (Tauri auto-serializes the inner JSON
                                    // object as the event body).
                                    let _ = app_for_dispatch.emit(
                                        "history-updated",
                                        serde_json::json!({ "session_id": session_id }),
                                    );

                                    crate::state::set_app_fsm(&dispatch_app_fsm, AppFsm::Idle);
                                    // Move only AppHandle clones into the
                                    // main-thread closure; `app2` would be
                                    // moved twice otherwise. Paste the FINAL
                                    // (formatted + LLM-cleaned) text.
                                    let app2 = app_for_dispatch.clone();
                                    let app3 = app2.clone();
                                    // auto-paste / trailing space / auto-submit.
                                    // Read here rather than inside the delivery
                                    // call so the main thread does not touch the
                                    // disk.
                                    let delivery = crate::config::Config::load(&app_for_dispatch)
                                        .map(|cfg| {
                                            crate::clipboard::DeliveryOptions::from_config(
                                                cfg.as_value(),
                                            )
                                        })
                                        .unwrap_or_default();
                                    let telemetry_paste_result = if delivery.auto_paste {
                                        telemetry::PasteResult::Success
                                    } else {
                                        telemetry::PasteResult::ClipboardOnly
                                    };
                                    let dispatch_state_for_paste = dispatch_state.clone();
                                    let paste_result = app2.run_on_main_thread(move || {
                                        // Cue and `paste-done` after the paste
                                        // result is known — "done" has to mean
                                        // the text actually went in, not that we
                                        // tried. The overlay waits for this
                                        // event before claiming anything was
                                        // inserted, which is what keeps it quiet
                                        // while a slow LLM is still working.
                                        match crate::clipboard::deliver(
                                            app3.clone(),
                                            paste_text,
                                            delivery,
                                        ) {
                                            Ok(()) => {
                                                crate::sounds::play(
                                                    &app3,
                                                    crate::sounds::Cue::Done,
                                                );
                                                let _ = app3.emit(
                                                    "paste-done",
                                                    serde_json::json!({
                                                        "session_id": session_id,
                                                        "length": pasted_length,
                                                        "ai_processing": paste_ai,
                                                    }),
                                                );
                                                dictation_telemetry.record(
                                                    &telemetry_for_paste,
                                                    pasted_length,
                                                    telemetry_paste_result,
                                                );
                                            }
                                            Err(e) => {
                                                log::error!("paste failed: {e}");
                                                crate::sounds::play(
                                                    &app3,
                                                    crate::sounds::Cue::Error,
                                                );
                                                let message = format!(
                                                    "{} {e}",
                                                    crate::ui_text::t(
                                                        "Не удалось вставить текст в активное окно."
                                                    )
                                                );
                                                let _ = app3.emit(
                                                    "app-error",
                                                    serde_json::json!({
                                                        "kind": "paste",
                                                        "message": message,
                                                    }),
                                                );
                                                // The overlay is waiting in
                                                // "распознано" for a paste that
                                                // is never coming. Without this
                                                // it sits there until the
                                                // stuck-overlay timeout — three
                                                // minutes of pretending to work.
                                                let _ = app3.emit(
                                                    "paste-failed",
                                                    serde_json::json!({
                                                        "session_id": session_id,
                                                        "message": message,
                                                    }),
                                                );
                                                dictation_telemetry.record(
                                                    &telemetry_for_paste,
                                                    pasted_length,
                                                    telemetry::PasteResult::Failed,
                                                );
                                            }
                                        }
                                        dispatch_state_for_paste.finish_session(session_id);
                                    });
                                    if paste_result.is_err() {
                                        // If the main-thread handoff itself
                                        // failed, no completion will arrive to
                                        // clean the session registration.
                                        dispatch_state.finish_session(session_id);
                                    }
                                }
                            }
                        }
                    }
                }
                log::info!("whisper event dispatcher exiting");
            });

            // Startup: register the saved hotkey from config.json.
            // WS 4a1 Task 13b: fetch the AppState we just `manage()`-ed
            // and hand it to `register`. The handler closure captures a
            // clone so the global-shortcut handler can dispatch into the
            // engine thread without going through the sidecar.
            let state: tauri::State<AppState> = app.state();
            match config::load_hotkey(app.handle()) {
                Ok(hotkey) => {
                    if let Err(e) = hotkey::register(app.handle(), &state, &hotkey) {
                        let _ = app.emit("hotkey-error", e);
                    }
                }
                Err(e) => log::warn!("could not load hotkey from config: {e}"),
            }
            // WS 4a1 Task 15: wire the overlay to engine lifecycle events
            // (whisper-started/done/failed/cancelled/loading/load-failed +
            // recording-started from the start_recording command). The
            // listener closure captures `app.handle()` and lives for the
            // lifetime of the app — there is no explicit unlisten.
            // Must come before the listeners: they only enqueue work, and ops
            // posted before the worker exists are dropped.
            crate::overlay::start_worker(app.handle().clone());
            crate::overlay::subscribe_engine_events(app.handle());
            #[cfg(windows)]
            {
                // Create WebView2 up front to avoid cold-start latency, but keep
                // its HWND genuinely hidden until the first recording. A shown
                // transparent pre-warm window still participates in hit-testing
                // through WebView2 child HWNDs and can expose a native frame when
                // the user clicks its otherwise invisible screen rectangle.
                if let Err(e) = crate::overlay::ensure_window(app.handle()) {
                    eprintln!("[overlay] startup creation failed: {e}");
                }
            }
            #[cfg(not(windows))]
            {
                // Create the (hidden) overlay window up front so the first
                // hotkey press doesn't pay the WKWebView cold-start: without
                // this, show_state() shows a still-empty transparent window
                // and the pill "pops in" once React hydrates. No off-screen
                // paint warm-up is needed — WKWebView has no gray redirection
                // bitmap to flush, unlike WebView2.
                if let Err(e) = crate::overlay::ensure_window(app.handle()) {
                    eprintln!("[overlay] pre-warm failed: {e}");
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_audio_file,
            transcribe_audio_file,
            cancel_audio_file,
            overlay::show_state,
            overlay::hide,
            overlay::current_state,
            overlay::overlay_ready,
            #[cfg(windows)]
            windows::tray_popup::show_tray_popup,
            #[cfg(windows)]
            windows::tray_popup::hide_tray_popup,
            focus_main_window,
            open_url,
            validate_hotkey,
            set_hotkey,
            fetch_provider_models,
            cancel_model_download,
            crate::overlay::set_overlay_streaming,
            start_recording,
            stop_recording,
            cancel_recording,
            start_microphone_test,
            stop_microphone_test,
            set_microphone_test_monitor,
            // WS 4b Task 9: stats + history Tauri commands. Frontend calls
            // these via `rustInvoke` from `desktop/src/bridge/stats.ts`.
            // All 5 are `async fn` so the DB op runs through `spawn_blocking`
            // via the `run_db_op` helper above.
            get_stats,
            list_history,
            delete_history_entry,
            update_history_entry_text,
            clear_history,
            // Phase 4 / PR-B: re-run AI processing on an existing history
            // entry (pure Rust — no Python subprocess).
            retry_history_ai_processing,
            // Phase 4 / PR-B: native Tauri commands (replaced Python sidecar).
            get_config,
            save_config,
            // PR-A: boot-blocking commands called from MainWindow.load() via Promise.all.
            app_version,
            check_update,
            install_update,
            list_microphones,
            list_models,
            get_runtime_status,
            // PR-B0: model lifecycle commands.
            download_model,
            set_model,
            delete_model,
            get_model_status,
            // API-key storage (native secret store). The frontend's
            // API-keys / providers pages depend on these three.
            save_api_key,
            has_api_key,
            delete_api_key,
            // Explicit-LLM commands: "Тест" and "Обработать текст".
            test_ai_prompt,
            process_text_ai,
            format_commands::preview_format,
            format_commands::preview_replacements,
            // macOS: check Accessibility permission on demand (frontend can
            // call this after the user grants access in System Settings).
            check_accessibility,
            test_paste,
            preview_sound_cue,
            preview_output_duck,
            get_output_contract,
            get_diagnostics,
            open_diagnostics_folder,
            logs_size,
            clear_logs,
            dictionary_presets,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        // The app's one lifecycle seam. Closing the usage session here is
        // best-effort: it only queues the event, so a shutdown that outruns
        // the outbox worker loses it rather than delaying the exit.
        .run(|app_handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                app_handle
                    .state::<crate::telemetry::Telemetry>()
                    .finish_usage_session();
            }
        });
}

/// Borrow the `ai_processing` sub-object from a loaded config, or a
/// stable error string. Centralizes the lookup shared by the retry
/// and cloud-STT paths (both need the same object and the same
/// "missing ai_processing config" error message).
/// Result of the post-Whisper pipeline: local formatting + optional LLM.
///
/// `raw_text` is the untouched whisper output, `formatted_text` is the
/// pre-LLM text (after local formatting), and `final_text` is what actually
/// gets pasted. `ai_json` / `stats_json` are the serialized `ai_processing`
/// and `processing_stats` blobs the history UI renders.
struct ProcessedTranscription {
    raw_text: String,
    formatted_text: String,
    final_text: String,
    ai_json: Option<String>,
    /// Same thing as `ai_json`, unserialized. History stores the JSON; the
    /// stats aggregate needs the fields, and re-parsing our own JSON to get
    /// them back would be silly.
    ai_status: Option<crate::ai::step::AiStatus>,
    stats_json: String,
    system_prompt: Option<String>,
}

/// How a recognition session ended — before post-processing starts.
///
/// Split out of the dispatcher because every branch here is a promise to the
/// user: which one is chosen decides what the overlay shows, which sound plays,
/// and whether the text is inserted at all. Inside the dispatcher this logic is
/// out of reach for a test — it sits behind an `AppHandle`, a tokio task and a
/// channel — and both bugs we had to learn about from users were right here.
///
/// `PartialEq` is deliberately absent: there is no reason to compare a whole
/// `InferenceResult` in a test, and printing it via `Debug` would drag the entire
/// transcription into the failure message. Tests match the variant with
/// `matches!`.
#[derive(Debug)]
pub(crate) enum Completion {
    /// The user cancelled while the engine was working. Nothing is inserted and
    /// nothing is recorded.
    Cancelled,
    /// The engine failed. The message goes to the overlay as is.
    Failed(String),
    /// The engine returned nothing — silence or too short a recording. It
    /// differs from `Failed` only in wording: both branches end the cycle
    /// without text.
    Empty,
    /// There is text; post-processing comes next.
    Transcribed(crate::whisper::InferenceResult),
}

pub(crate) fn classify_completion(
    cancelled: bool,
    result: Result<crate::whisper::InferenceResult, String>,
) -> Completion {
    // Cancellation outranks everything else, errors included: the user has
    // already said they do not need this session, and reporting a failure for it
    // means explaining the consequences of a decision they made themselves.
    if cancelled {
        return Completion::Cancelled;
    }
    match result {
        Err(error) => Completion::Failed(error),
        // `trim` rather than `is_empty`: on silence whisper returns a space or a
        // newline, and treating such an answer as "non-empty" led to inserting
        // nothing with a cheerful «Текст готов».
        Ok(inference) if inference.text.trim().is_empty() => Completion::Empty,
        Ok(inference) => Completion::Transcribed(inference),
    }
}

/// Whether what remains after post-processing is worth inserting.
///
/// Empty here does not mean "the engine heard nothing" — it means the formatter
/// removed everything it heard. That happens when the whole transcription turned
/// out to be a hallucination on silence («Субтитры сделал…»). Inserting the
/// fallback in that case means inserting exactly the artifact just cleaned out.
pub(crate) fn is_deliverable(final_text: &str) -> bool {
    !final_text.trim().is_empty()
}

/// Whether the LLM cleanup pass runs for a live dictation, given the
/// `ai_processing` block of the config.
///
/// Every entry point — both hotkey and the UI's start/stop buttons —
/// asks this one question, and the answer comes from the configured mode
/// alone. It briefly did not: the dictation shortcut carried its own
/// "local only" intent that outranked `pipeline_mode`, so a config that
/// clearly said `hybrid` still pasted unprocessed Whisper output, and the
/// LLM could only be reached from the history retry button. One source of
/// truth is what keeps the setting and the behaviour from disagreeing.
///
/// A missing block or a missing mode reads as `local` (matching the
/// dispatch side and the frontend) so a partial config never silently
/// enables the LLM. `cloud` swaps whisper for a cloud STT endpoint and
/// adds no LLM pass of its own.
fn llm_should_run(ai: Option<&Value>) -> bool {
    ai.and_then(|ai| ai.get("pipeline_mode"))
        .and_then(Value::as_str)
        .unwrap_or("local")
        == "hybrid"
}

/// The dictation facts telemetry needs, captured before the paste closure
/// takes ownership of the text. Delivery is the only thing still unknown at
/// that point, so the closure supplies it and calls `record` exactly once.
struct DictationTelemetry {
    pipeline_mode: String,
    stt_model: Option<String>,
    audio_seconds: f64,
    stt_millis: u64,
    ai_status: Option<crate::ai::step::AiStatus>,
    recording_mode: crate::telemetry::RecordingMode,
    compute: Option<crate::telemetry::Compute>,
    formatting_enabled: bool,
    replacement_rules: usize,
}

impl DictationTelemetry {
    fn capture(
        config: Option<&crate::config::Config>,
        pipeline_mode: &str,
        stt_model: Option<String>,
        audio_seconds: f64,
        stt_millis: u64,
        ai_status: Option<crate::ai::step::AiStatus>,
    ) -> Self {
        let (formatting_enabled, replacement_rules) =
            config.map(telemetry_formatting).unwrap_or((false, 0));
        Self {
            pipeline_mode: pipeline_mode.to_string(),
            recording_mode: config
                .map(telemetry_recording_mode)
                .unwrap_or(crate::telemetry::RecordingMode::NotApplicable),
            compute: config.map(|config| telemetry_compute(config, stt_model.as_deref())),
            stt_model,
            audio_seconds,
            stt_millis,
            ai_status,
            formatting_enabled,
            replacement_rules,
        }
    }

    fn record(
        &self,
        telemetry: &crate::telemetry::Telemetry,
        chars: usize,
        paste_result: crate::telemetry::PasteResult,
    ) {
        telemetry.record_completed(crate::telemetry::Outcome {
            source: crate::telemetry::Source::Microphone,
            pipeline_mode: &self.pipeline_mode,
            recording_mode: self.recording_mode,
            stt_model: self.stt_model.as_deref(),
            audio_seconds: self.audio_seconds,
            stt_millis: self.stt_millis,
            chars,
            ai_status: self.ai_status.as_ref(),
            compute: self.compute,
            formatting_enabled: self.formatting_enabled,
            replacement_rules: self.replacement_rules,
            paste_result,
        });
    }
}

/// Resolve the configured pipeline for a telemetry outcome.  Unknown or
/// malformed values are reported as `local` here, matching the safe runtime
/// default used by the recording path.
fn telemetry_pipeline_mode(config: Option<&crate::config::Config>) -> String {
    config
        .and_then(|config| {
            config
                .as_value()
                .get("ai_processing")
                .and_then(|ai| ai.get("pipeline_mode"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| "local".to_string())
}

/// The failure paths have no config in hand and only need this one field, so
/// they pay a single read — the success path loads once and passes it down.
fn telemetry_pipeline_mode_of(app: &AppHandle) -> String {
    telemetry_pipeline_mode(crate::config::Config::load(app).ok().as_ref())
}

fn telemetry_recording_mode(config: &crate::config::Config) -> crate::telemetry::RecordingMode {
    match config.get_string("recording_mode").as_deref() {
        Some("toggle") => crate::telemetry::RecordingMode::Toggle,
        Some("push_to_talk") => crate::telemetry::RecordingMode::PushToTalk,
        _ => crate::telemetry::RecordingMode::NotApplicable,
    }
}

/// Cloud mode has no local device, so [`crate::telemetry::compute_wire`]
/// overrides this; the ONNX bundles run through sherpa, which is CPU-only
/// whatever the device setting says.
fn telemetry_compute(
    config: &crate::config::Config,
    model: Option<&str>,
) -> crate::telemetry::Compute {
    let sherpa = model
        .and_then(|id| crate::model::model_engine(id).ok())
        .is_some_and(|engine| engine.is_sherpa());
    if !sherpa && crate::config::device_uses_gpu(config.as_value()) {
        crate::telemetry::Compute::Gpu
    } else {
        crate::telemetry::Compute::Cpu
    }
}

/// Derive low-cardinality formatter metadata without ever serializing any
/// user vocabulary or replacement text. Counting through
/// `normalize_replacement_rules` also covers the legacy `replacements` dict,
/// which a hand-rolled array read reports as zero.
fn telemetry_formatting(config: &crate::config::Config) -> (bool, usize) {
    let enabled_rules = crate::formatter::normalize_replacement_rules(Some(config.as_value()))
        .iter()
        .filter(|rule| rule.enabled)
        .count();
    (text_formatting_config(config).enabled, enabled_rules)
}

/// Serialize an LLM pass into the `ai_processing_json` column.
///
/// Both writers of that column — the live dispatcher and the history
/// "Повторить LLM" path — go through here. They used to build it
/// separately and had already drifted: the retry wrote `{"text": …}`
/// while the frontend reads `attempted` / `used` / `fallback` /
/// `provider_error` straight off the serialized [`AiStatus`]. A retried
/// entry therefore rendered as never processed, with no error to explain
/// it. One constructor is what keeps that from happening again.
fn ai_processing_json(status: Option<&crate::ai::step::AiStatus>) -> Option<String> {
    status.and_then(|s| serde_json::to_string(s).ok())
}

/// Run the post-Whisper pipeline for a completed live transcription:
///   1. local formatting (config-gated `Formatter`),
///   2. the LLM cleanup step (`ai_process_text_with_status`), which
///      internally decides whether to run based on `pipeline_mode`, the
///      key, and the min-duration gate.
///
/// This is the piece that makes the history diff / before-after blocks work:
/// it records all three text stages plus the AI status. It NEVER fails hard —
/// on any config or provider error it falls back to the best text available
/// so the paste still happens.
async fn post_process_transcription(
    app: &AppHandle,
    inference: &crate::whisper::InferenceResult,
) -> ProcessedTranscription {
    let raw_text = inference.text.trim().to_string();
    let whisper_seconds = inference.inference_time_ms as f64 / 1000.0;
    let config = crate::config::Config::load(app).ok();

    // 1. Local formatting (fillers, capitalization, replacements, …). Gated
    //    by `text_formatting.enabled`; when disabled the formatter just
    //    trims, so `formatted_text == raw_text` and the "Whisper без
    //    обработки" block stays hidden.
    //
    //    An empty result normally means some cleanup step over-reached
    //    (e.g. the parasite remover ate a "ну да"), so we fall back to the
    //    raw text rather than pasting nothing. The ONE case where empty is
    //    the correct answer is a pure Whisper silence hallucination
    //    ("Субтитры сделал DimaTorzok", "Thank you.", "you"): there the
    //    fallback would paste exactly the artifact we just removed, so we
    //    keep the empty string and let the caller skip the paste.
    let formatted_text = match &config {
        Some(cfg) => {
            let formatting = text_formatting_config(cfg);
            if formatting.enabled
                && formatting.remove_hallucinations
                && crate::formatter::is_pure_hallucination(&raw_text)
            {
                log::info!(
                    "session {}: whole transcription is a Whisper hallucination, dropping it",
                    inference.session_id
                );
                String::new()
            } else {
                let out = crate::formatter::format_with_config_value(cfg.as_value(), &raw_text);
                if out.trim().is_empty() {
                    raw_text.clone()
                } else {
                    out
                }
            }
        }
        None => raw_text.clone(),
    };

    // 2. LLM cleanup step — hybrid mode only, see `llm_should_run`.
    let ai_value = config
        .as_ref()
        .and_then(|c| ai_processing_config(c).ok().cloned());
    let run_llm = llm_should_run(ai_value.as_ref());
    let (final_text, ai_status) = match &ai_value {
        // Nothing survived the hallucination filter — there is no text to
        // clean up, and sending an empty prompt would only burn a request
        // and invite the model to invent a reply.
        _ if formatted_text.is_empty() => (String::new(), None),
        Some(ai_val) if run_llm => {
            let mut ai_cfg = crate::ai::step::AiConfig::from_ai_processing(ai_val);
            ai_cfg.language = speech_language(config.as_ref());
            ai_cfg.audio_duration_seconds = Some(inference.audio_seconds);
            let api_key = if ai_cfg.api_key_ref.is_empty() {
                None
            } else {
                crate::secret_store::get_key(&ai_cfg.api_key_ref)
                    .ok()
                    .flatten()
            };
            let outcome = crate::ai::ai_process_text_with_status(
                &formatted_text,
                &ai_cfg,
                api_key.as_deref(),
            )
            .await;
            (outcome.text, Some(outcome.status))
        }
        _ => (formatted_text.clone(), None),
    };

    let llm_seconds = ai_status.as_ref().map(|s| s.elapsed_seconds).unwrap_or(0.0);
    let stats_json = serde_json::json!({
        "audio_seconds": inference.audio_seconds,
        "whisper_seconds": whisper_seconds,
        "llm_seconds": llm_seconds,
        "total_seconds": whisper_seconds + llm_seconds,
    })
    .to_string();
    let ai_json = ai_processing_json(ai_status.as_ref());
    let system_prompt = ai_value
        .as_ref()
        .and_then(|ai| ai.get("system_prompt").and_then(serde_json::Value::as_str))
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string);

    ProcessedTranscription {
        raw_text,
        formatted_text,
        final_text,
        ai_json,
        ai_status,
        stats_json,
        system_prompt,
    }
}

/// Read the `text_formatting` block out of a loaded config, falling back
/// to `TextFormattingConfig::default()` (everything on) when the key is
/// absent or malformed — same resolution `format_with_config_value` does
/// internally, so the two can't disagree about which steps are active.
/// The custom vocabulary as one prompt line for the whisper decoder.
///
/// Comma-separated, which is the form whisper.cpp's own examples use and
/// what Handy feeds it too — the prompt is conditioning context, not a
/// sentence, so the separator only has to keep the terms apart.
///
/// `None` when the list is empty, so a user who never opened the setting
/// pays nothing: an empty prompt still costs decoder tokens.
pub(crate) fn custom_words_prompt(config: &crate::config::Config) -> Option<String> {
    // The effective dictionary, not just your own words: an enabled set must
    // hint the decoder exactly the same way, otherwise it would repair terms
    // after recognition without preventing them from being broken.
    let words = text_formatting_config(config).effective_custom_words();
    let joined = words
        .iter()
        .map(|word| word.trim())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    (!joined.is_empty()).then_some(joined)
}

fn text_formatting_config(
    config: &crate::config::Config,
) -> crate::formatter::TextFormattingConfig {
    config
        .as_value()
        .get("text_formatting")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

fn ai_processing_config(config: &crate::config::Config) -> Result<&Value, String> {
    config
        .as_value()
        .get("ai_processing")
        .ok_or_else(|| "missing ai_processing config".to_string())
}

/// Build a `CloudSttRequest` from the on-disk config + secret
/// store. Returns `Err(msg)` if any required field is missing —
/// the dispatcher surfaces the error in the existing toast.
fn build_cloud_stt_request(
    app: &AppHandle,
    audio: &Arc<Vec<f32>>,
) -> Result<crate::cloud_stt::CloudSttRequest, String> {
    let config = crate::config::Config::load(app)?;
    let ai = ai_processing_config(&config)?;
    let base_url = ai
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cloud STT requires 'base_url' in ai_processing".to_string())?
        .to_string();
    let model = ai
        .get("stt_model")
        .or_else(|| ai.get("model"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cloud STT requires 'stt_model' (or 'model') in ai_processing".to_string())?
        .to_string();
    let api_key_ref = ai
        .get("api_key_ref")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "cloud STT requires 'api_key_ref' in ai_processing".to_string())?;
    let api_key = crate::secret_store::get_key(api_key_ref)
        .map_err(|error| format!("cloud STT api_key for '{api_key_ref}': {error}"))?
        .ok_or_else(|| {
            format!("cloud STT api_key for '{api_key_ref}' is empty (save it in API Keys)")
        })?;
    let timeout_seconds = ai
        .get("cloud_stt_timeout_seconds")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(45);
    let language = ai
        .get("language")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);

    Ok(crate::cloud_stt::CloudSttRequest {
        provider: crate::cloud_stt::CloudSttProvider::Compatible,
        base_url,
        api_key,
        model,
        language,
        audio: Arc::clone(audio),
        timeout_seconds,
    })
}

#[cfg(test)]
mod retry_ai_tests {
    use super::*;
    use crate::ai::step::AiStatus;

    fn status(used: bool, skipped_reason: &str) -> AiStatus {
        let mut status = AiStatus {
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
        let json = ai_processing_json(Some(&status(true, ""))).unwrap();
        let parsed: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["used"], serde_json::json!(true));
        assert_eq!(parsed["attempted"], serde_json::json!(true));
        assert_eq!(parsed["provider"], serde_json::json!("compatible"));
        assert!(parsed.get("text").is_none());
    }

    #[test]
    fn ai_processing_json_is_none_without_a_status() {
        assert!(ai_processing_json(None).is_none());
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

#[cfg(test)]
mod llm_gate_tests {
    use super::*;
    use serde_json::json;

    /// A live dictation with `pipeline_mode: "hybrid"` must go to the LLM —
    /// this is exactly the case that broke when the main hotkey carried its own
    /// "local only": the setting was in place while raw Whisper landed in the
    /// clipboard.
    #[test]
    fn hybrid_config_runs_the_llm() {
        assert!(llm_should_run(Some(&json!({"pipeline_mode": "hybrid"}))));
    }

    /// `local` is whisper and nothing else; `cloud` swaps whisper for cloud STT
    /// but adds no LLM pass of its own.
    #[test]
    fn local_and_cloud_do_not() {
        assert!(!llm_should_run(Some(&json!({"pipeline_mode": "local"}))));
        assert!(!llm_should_run(Some(&json!({"pipeline_mode": "cloud"}))));
    }

    /// An incomplete config must not silently enable the network: neither a
    /// missing mode nor a missing `ai_processing` block equals `hybrid`.
    #[test]
    fn a_partial_config_never_enables_the_llm() {
        assert!(!llm_should_run(Some(&json!({}))));
        assert!(!llm_should_run(Some(&json!({"pipeline_mode": ""}))));
        assert!(!llm_should_run(Some(&json!({"pipeline_mode": 3}))));
        assert!(!llm_should_run(None));
    }
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    use crate::whisper::InferenceResult;

    fn inference(text: &str) -> InferenceResult {
        InferenceResult {
            session_id: 1,
            text: text.to_string(),
            language: Some("ru".to_string()),
            model_id: Some("turbo".to_string()),
            inference_time_ms: 500,
            audio_seconds: 4.0,
        }
    }

    /// The dispatcher's first promise: a cancelled session is not inserted. This
    /// is the very branch for which the cancellation check comes before
    /// everything else.
    #[test]
    fn a_cancelled_session_is_cancelled_whatever_the_engine_returned() {
        assert!(matches!(
            classify_completion(true, Ok(inference("готовый текст"))),
            Completion::Cancelled
        ));
        assert!(matches!(
            classify_completion(true, Err("движок упал".to_string())),
            Completion::Cancelled
        ));
        assert!(matches!(
            classify_completion(true, Ok(inference(""))),
            Completion::Cancelled
        ));
    }

    /// On silence Whisper returns not an empty string but a space or a newline.
    /// Without `trim` such an answer counted as text, and the user got «Текст
    /// готов» for nothing at all.
    #[test]
    fn whitespace_is_not_text() {
        for blank in ["", " ", "\n", "\t\n  "] {
            assert!(
                matches!(
                    classify_completion(false, Ok(inference(blank))),
                    Completion::Empty
                ),
                "не распознано как пустое: {blank:?}"
            );
        }
    }

    #[test]
    fn real_text_goes_on_to_post_processing() {
        let outcome = classify_completion(false, Ok(inference("привет")));
        match outcome {
            Completion::Transcribed(result) => assert_eq!(result.text, "привет"),
            other => panic!("ожидался Transcribed, получено {other:?}"),
        }
    }

    /// The engine's message travels to the overlay verbatim: the overlay has its
    /// own substitution for the empty case, and the cause of a failure must not
    /// be replaced.
    #[test]
    fn the_engine_message_survives_verbatim() {
        let outcome = classify_completion(false, Err("GigaAM v3 не умеет английский".to_string()));
        match outcome {
            Completion::Failed(message) => assert_eq!(message, "GigaAM v3 не умеет английский"),
            other => panic!("ожидался Failed, получено {other:?}"),
        }
    }

    /// An error does not turn into an empty result even when it carries no text:
    /// the two are worded differently in the overlay.
    #[test]
    fn a_failure_is_not_an_empty_transcription() {
        assert!(matches!(
            classify_completion(false, Err(String::new())),
            Completion::Failed(_)
        ));
    }

    /// The formatter may have removed everything it heard: the whole
    /// transcription turned out to be a hallucination on silence. There is
    /// nothing to insert.
    #[test]
    fn text_emptied_by_the_formatter_is_not_delivered() {
        assert!(!is_deliverable(""));
        assert!(!is_deliverable("   "));
        assert!(!is_deliverable("\n"));
    }

    #[test]
    fn surviving_text_is_delivered() {
        assert!(is_deliverable("привет"));
        // A single meaningful character is still text.
        assert!(is_deliverable("!"));
    }
}
