//! Crate root: application assembly, and the pieces more than one domain uses.
//!
//! A `#[tauri::command]` lives with its domain — `history::list_history`,
//! `config::save_config`, `model::set_model` — and `generate_handler!` in
//! [`run`] names them by path. What stays here is what has no single domain:
//!
//! - `run()` and `setup()`: window, tray, hotkey, engine and worker wiring.
//! - The app-level commands (`app_version`, `get_runtime_status`,
//!   `get_output_contract`) — they answer for the application, not for one
//!   of its parts.
//! - The dictation pipeline, from `on_recording_started` to
//!   `post_process_transcription`. It is the app's main flow rather than a
//!   module's, and its post-processing half is shared: `audio_file` runs the
//!   same formatting and LLM steps over an attached file, `history` re-runs
//!   them over a stored entry. Helpers with one consumer moved out with it;
//!   these did not, because moving a shared helper into one of its callers
//!   only inverts the dependency.
//! - Small shared utilities on the same footing: `run_db_op`, `panic_msg`,
//!   `load_model_into_engine`, `speech_language`, `apply_autostart`.
//!
//! `generate_handler!` stays in this file for a second reason: the frontend
//! test `bridge/command-surface.test.ts` reads it as raw text to check that
//! every invoked command is registered, and the platform-restricted ones are
//! guarded at the call site.

mod accessibility;
pub mod ai;
mod audio;
mod audio_file;
mod audio_resampler;
mod audio_worker;
#[cfg(any(target_os = "macos", test))]
mod autostart;
mod clipboard;
pub mod cloud_stt;
pub mod config;
mod db;
mod debug;
mod dictation;
mod dictionaries;
mod engine_events;
mod external_link;
mod feedback;
mod format_commands;
pub mod formatter;
mod hardware_profile;
mod history;
mod hotkey;
pub mod http_client;
pub mod mic_test;
pub mod model;
pub mod model_download;
pub mod model_performance;
pub mod mutex_recover;
mod output_volume;
mod overlay;
mod overlay_preferences;
mod portable;
mod release_notes;
pub mod secret_store;
pub mod sherpa;
mod sounds;
mod spelling;
pub mod state;
mod stats;
pub mod structured_log;
mod telemetry;
#[cfg(test)]
mod test_support;
mod text_protection;
mod tray;
mod ui_text;
mod updater;
mod user_data;
mod vad;
mod wav;
pub mod whisper;
mod window_state;
#[cfg(windows)]
mod windows_util;
#[cfg(windows)]
mod windows {
    pub mod overlay_diag;
    pub mod win_util;
}

use crate::state::{AppFsm, AppState};
use rusqlite::Connection;
use serde_json::Value;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// Run a blocking DB op on a worker thread and return the result as a
/// `Result<T, String>`.
///
/// Keep synchronous database work and mutex contention off the async runtime.
/// The connection stays locked inside the blocking task; its `MutexGuard`
/// never crosses an `.await` boundary.
///
/// Failure modes:
/// - `Ok(Err(e))` — DB op ran but returned a `rusqlite::Error`. We
///   convert via `e.to_string()` so the wire format stays a plain
///   `String` (no rusqlite leak across the IPC boundary).
/// - `Err(_)` — the worker died (channel closed). We surface a stable
///   `"worker died"` string so callers can pattern-match.
///
/// A poisoned connection mutex is not a failure mode: it is recovered
/// through [`mutex_recover`], so a panic under the lock cannot leave the
/// UI permanently unable to read stats or history.
pub(crate) async fn run_db_op<T, F>(
    db: Arc<std::sync::Mutex<Connection>>,
    f: F,
) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> Result<T, rusqlite::Error> + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::task::spawn_blocking(move || {
        // Same policy as the writers in `stats`/`history`/`telemetry`: one
        // panic under this lock must not make every later read fail for the
        // rest of the process. Poisoning says a previous holder died, not
        // that the database is corrupt — SQLite has its own transaction
        // guarantees for that.
        let result = {
            let conn = crate::mutex_recover::lock(&db);
            f(&conn).map_err(|e| e.to_string())
        };
        let _ = tx.send(result);
    });
    match rx.await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(e),
        Err(_) => Err("worker died".to_string()),
    }
}

#[cfg(test)]
mod db_op_tests {
    use super::run_db_op;
    use std::sync::{Arc, Mutex};

    /// Every history and stats *read* the UI makes goes through
    /// `run_db_op`. Before it used `mutex_recover` a single panic under the
    /// connection lock turned all of them into `"db lock poisoned"` for the
    /// rest of the process — writes kept landing, and the user simply could
    /// not see them again until a restart.
    #[tokio::test]
    async fn reads_survive_a_poisoned_connection_mutex() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&conn).unwrap();
        let db = Arc::new(Mutex::new(conn));

        let holder = Arc::clone(&db);
        let joined = std::thread::spawn(move || {
            let _guard = holder.lock().unwrap();
            panic!("simulated panic while holding the connection");
        })
        .join();
        assert!(joined.is_err(), "the holder thread should have panicked");
        assert!(db.is_poisoned(), "the connection mutex must be poisoned");

        let count: i64 = run_db_op(db, |conn| {
            conn.query_row("SELECT COUNT(*) FROM history", [], |row| row.get(0))
        })
        .await
        .expect("read after poisoning should succeed");
        assert_eq!(count, 0);
    }
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
/// - `state`: app FSM state string (idle/recording/processing)
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
        // A portable copy deliberately does not touch autostart (see
        // `apply_autostart_inner`), and the interface has to know: a checkbox
        // that saves its value and changes nothing is worse than no checkbox.
        "portable": crate::portable::data_dir().is_some(),
        // Build-target OS (`std::env::consts::OS`): the frontend's guard for any
        // command registered under `#[cfg(...)]` (see command-surface.test.ts).
        "os": std::env::consts::OS,
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
            tx.try_send(0)
                .expect("there is room, so the send cannot fail");
            sent += 1;
        }

        assert_eq!(sent, 64 - PREVIEW_QUEUE_RESERVE);
        assert_eq!(tx.capacity(), PREVIEW_QUEUE_RESERVE);
        assert!(
            tx.try_send(1).is_ok(),
            "room for the final transcription is left"
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

/// Send `SetModel` to the engine thread and await its reply.
///
/// Explicit loads read the current compute device from config. Idle restores
/// do the same in `restore_unloaded_model`, but queue without waiting so capture
/// can continue while the engine validates and loads the model.
pub(crate) async fn load_model_into_engine(
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
pub(crate) fn restore_unloaded_model(app: &AppHandle, state: &AppState) -> Option<String> {
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
            speech_timing: crate::vad::SpeechTiming::Ready(None),
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

/// The speech language substituted for `{{language}}` in the system prompt.
///
/// It sits at the top level of the config, next to the model and the device,
/// and NOT inside `ai_processing`. `AiConfig::from_ai_processing` used to read a
/// field of the same name from its own subtree — nobody ever wrote it there, so
/// the placeholder expanded to nothing and the model received "Output
/// language: .".
pub(crate) fn speech_language(config: Option<&crate::config::Config>) -> String {
    config
        .and_then(|cfg| cfg.get_string("language"))
        .unwrap_or_default()
}

/// Bring the main window forward on the tab the user left open.
pub(crate) fn show_main_window(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.unminimize().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// How long before the limit the overlay starts counting down: five minutes,
/// or a third of a shorter limit.
const RECORDING_WARNING_LEAD_SECONDS: f64 = 5.0 * 60.0;

#[derive(Debug, PartialEq)]
enum RecordingLimit {
    Within,
    Warn { remaining_seconds: u64 },
    Stop,
}

/// Where a recording stands against the configured limit
/// ([`crate::config::recording_limit_minutes`], `0` — none). At the limit it
/// stops and is transcribed like any other.
fn recording_limit(recorded_seconds: f64, warned: bool, limit_minutes: u64) -> RecordingLimit {
    if limit_minutes == 0 {
        return RecordingLimit::Within;
    }
    let limit = limit_minutes as f64 * 60.0;
    let warning = limit - RECORDING_WARNING_LEAD_SECONDS.min(limit / 3.0);
    if recorded_seconds >= limit {
        RecordingLimit::Stop
    } else if !warned && recorded_seconds >= warning {
        RecordingLimit::Warn {
            remaining_seconds: (limit - recorded_seconds).ceil() as u64,
        }
    } else {
        RecordingLimit::Within
    }
}

#[cfg(test)]
mod recording_limit_tests {
    use super::*;

    #[test]
    fn warns_once_then_stops_at_the_limit() {
        assert_eq!(recording_limit(599.0, false, 15), RecordingLimit::Within);
        assert_eq!(
            recording_limit(600.0, false, 15),
            RecordingLimit::Warn {
                remaining_seconds: 300
            }
        );
        assert_eq!(
            recording_limit(612.4, false, 15),
            RecordingLimit::Warn {
                remaining_seconds: 288
            }
        );
        assert_eq!(recording_limit(612.4, true, 15), RecordingLimit::Within);
        assert_eq!(recording_limit(900.0, true, 15), RecordingLimit::Stop);
        // A late first check past the limit stops rather than warns.
        assert_eq!(recording_limit(905.0, false, 15), RecordingLimit::Stop);
    }

    #[test]
    fn a_short_limit_warns_a_third_ahead_and_zero_means_none() {
        assert_eq!(recording_limit(199.0, false, 5), RecordingLimit::Within);
        assert_eq!(
            recording_limit(200.0, false, 5),
            RecordingLimit::Warn {
                remaining_seconds: 100
            }
        );
        assert_eq!(recording_limit(300.0, true, 5), RecordingLimit::Stop);
        assert_eq!(recording_limit(86_400.0, false, 0), RecordingLimit::Within);
    }
}

/// Emit `audio-level` events (~30 Hz) while a recording session is live so
/// the overlay waveform reacts to the user's voice. Reads the shared
/// recorder's EMA level and exits automatically when the recorder stops. A
/// static guard prevents overlapping pollers (only one session is ever
/// active at a time). Without this nothing emitted `audio-level`, so the
/// overlay waveform sat flat — see `OverlayApp.tsx` `listen("audio-level")`.
/// Once a second it also checks the recording against [`recording_limit`].
pub(crate) fn spawn_level_emitter(app: &AppHandle, recorder: Arc<crate::audio::AudioRecorder>) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static EMITTING: AtomicBool = AtomicBool::new(false);
    if EMITTING.swap(true, Ordering::AcqRel) {
        return; // a poller is already running for the current session
    }
    let app = app.clone();
    std::thread::spawn(move || {
        let mut tick: u32 = 0;
        let mut logged_first_frame = false;
        loop {
            let (mut warned, mut limited) = (false, false);
            let limit_minutes = crate::config::Config::load(&app)
                .map(|config| crate::config::recording_limit_minutes(config.as_value()))
                .unwrap_or(crate::config::DEFAULT_RECORDING_LIMIT_MINUTES);
            while recorder.is_recording() {
                if !logged_first_frame {
                    if let Some(first_frame_ms) = recorder.first_frame_ms() {
                        log::info!("capture timing: first_callback_ms={first_frame_ms}");
                        logged_first_frame = true;
                    }
                }
                let raw = recorder.level();
                let level = crate::audio::display_level(raw);
                let _ = app.emit("audio-level", serde_json::json!({ "level": level }));
                // Throttled (~1 Hz) diagnostic for a meter that looks dead
                // (raw + mapped). Debug level: at info it filled app.log with
                // a line for every second of every dictation.
                if tick.is_multiple_of(30) {
                    log::debug!("audio-level poll: raw={raw:.4} mapped={level:.4}");
                    if !limited {
                        // Read before measuring: a recording started after
                        // the read measures short, so the id can only name
                        // the recording being measured or an older one.
                        let session_id = app
                            .state::<AppState>()
                            .current_session_id
                            .load(Ordering::Acquire);
                        let recorded = recorder.recorded_seconds();
                        match recording_limit(recorded, warned, limit_minutes) {
                            RecordingLimit::Within => {}
                            RecordingLimit::Warn { remaining_seconds } => {
                                warned = true;
                                let _ = app.emit(
                                    "recording-limit",
                                    serde_json::json!({
                                        "session_id": session_id,
                                        "remaining_seconds": remaining_seconds,
                                    }),
                                );
                            }
                            RecordingLimit::Stop => {
                                limited = true;
                                log::info!(
                                    "recording limit reached after {recorded:.0}s, stopping"
                                );
                                let state = app.state::<AppState>();
                                let _ = dictation::stop_if_current(&app, &state, session_id);
                            }
                        }
                    }
                }
                tick = tick.wrapping_add(1);
                std::thread::sleep(std::time::Duration::from_millis(33));
            }
            if recorder.has_capture_error() {
                let state = app.state::<AppState>();
                let _ = dictation::stop(&app, &state);
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

/// Refuse a second owner without emitting a terminal event for the active job.
pub(crate) fn engine_busy_message() -> String {
    crate::ui_text::t("Завершите текущую запись.")
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
/// A pure builder: it emits nothing. Both entry points already report the
/// refusal themselves - `hotkey_do_start` emits `hotkey-error` and
/// `start_recording` returns the text to its caller - so emitting here too
/// delivered the same failure twice for every hotkey press.
///
/// Deliberately not `whisper-failed` either: that drives the overlay, and
/// the whole point of the refusal is that no overlay appears for a recording
/// that never began. The main window carries a standing banner for this
/// state, derived from the same three inputs.
pub(crate) fn no_transcription_route_message() -> String {
    crate::ui_text::t(
        "Модель распознавания не скачана — записывать нечем. Скачайте модель в настройках или включите облачную обработку.",
    )
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
            message: engine_busy_message(),
        }
    } else if !transcription_route_available(app, state) {
        DictationRefusal {
            reason: telemetry::FailureReason::NoTranscriptionRoute,
            message: no_transcription_route_message(),
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
    reply: tokio::sync::oneshot::Sender<Result<crate::whisper::InferenceResult, String>>,
) -> Result<crate::whisper::EngineCommand, String> {
    let (audio, speech_timing) =
        crate::vad::prepare_dictation(config.map(crate::config::Config::as_value), audio);
    let pipeline_mode = telemetry_pipeline_mode(config);
    if pipeline_mode != "cloud" {
        return Ok(crate::whisper::EngineCommand::Transcribe {
            session_id,
            audio,
            speech_timing,
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
        speech_timing,
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
    let session_id = state.dictation_id();
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

/// Helper: extract a panic message from `catch_unwind`'s `Box<dyn Any>`.
pub(crate) fn panic_msg(panic: Box<dyn std::any::Any + Send + 'static>) -> String {
    panic
        .downcast::<&str>()
        .map(|s| s.to_string())
        .or_else(|p| p.downcast::<String>().map(|s| *s))
        .unwrap_or_else(|_| "internal error".to_string())
}

/// Marker the OS autostart entry launches the app with, so startup can tell
/// "the user opened the app" from "the session started".
const AUTOSTART_ARG: &str = "--autostart";

/// Push the `auto_start` config value into the OS autostart entry.
///
/// Non-fatal by design: on Windows this writes to the Run key in the user
/// registry hive, which a policy or a cleanup tool can make unwritable. That
/// is worth a log line, not a failed startup or a failed settings save.
pub(crate) fn apply_autostart(app: &AppHandle) {
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

/// Delete the current user's data from its default locations, for the
/// uninstaller. Returns the process exit code: 0 when everything went.
#[cfg(windows)]
pub fn purge_user_data() -> i32 {
    i32::from(!crate::user_data::purge().is_empty())
}

/// Load the configured model on startup, so the overlay does not show
/// "Модель не загружена" on the first hotkey press.
fn spawn_model_autoload(app: AppHandle) {
    let engine = app.state::<AppState>().engine_cmd_tx.clone();
    tauri::async_runtime::spawn(async move {
        // Both outcomes are logged: "config unreadable" and "no model
        // configured" must not look like a healthy first run in the log.
        match crate::config::config_path(&app) {
            Ok(path) => log::info!("config: {} (exists: {})", path.display(), path.exists()),
            Err(error) => log::error!("config path unavailable: {error}"),
        }
        let config = match crate::config::Config::load(&app) {
            Ok(config) => config,
            Err(error) => {
                log::error!("config load failed, running with defaults: {error}");
                return;
            }
        };
        let Some(model) = config.get_string("model") else {
            log::info!("no model in config, skipping auto-load");
            return;
        };
        if !crate::model::is_downloaded(&model) {
            log::info!("config.model={model} not downloaded, skipping auto-load");
            return;
        }
        match load_model_into_engine(
            &app,
            &engine,
            &model,
            crate::whisper::ModelLoadReason::Requested,
        )
        .await
        {
            Ok(()) => log::info!("auto-loaded model {model} from saved config"),
            Err(e) => log::warn!("auto-load failed: {e}"),
        }
    });
}

/// The idle watchdog. It unloads nothing itself: the decision is made by the
/// engine thread, which alone knows what it is doing and for how long (see
/// `EngineCommand::UnloadIdle`). All that comes from here is a reason to check
/// plus the threshold from settings.
fn spawn_idle_watchdog(app: AppHandle) {
    let state = app.state::<AppState>().inner().clone();
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(IDLE_UNLOAD_TICK);
        // `interval` delivers its first tick immediately — we skip it: zero
        // seconds after startup there is nothing to be idle yet.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let Some(after) = idle_unload_after(&app) else {
                continue;
            };
            if crate::mutex_recover::lock(&state.engine_current_model).is_none() {
                continue;
            }
            // `try_send`: a busy queue means the engine is not idle, so there
            // is nothing to ask it about.
            let _ = state
                .engine_cmd_tx
                .try_send(crate::whisper::EngineCommand::UnloadIdle { after });
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Before the logger opens its file: the log directory moves with the
    // data. The outcome is logged once the logger exists.
    let data_migration = crate::user_data::migrate_legacy();
    // Installs before any other setup so every later log line reaches
    // `<data dir>/logs/app.log`, with API keys and bearer tokens redacted.
    let _ = crate::structured_log::install();
    if let Some(migration) = &data_migration {
        migration.log();
    }
    crate::user_data::log_renamed_env();

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
            // Set the locale before creating the native tray menu.
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
            // Detached: the engine exits on `EngineCommand::Shutdown` or with
            // the process.
            std::thread::spawn(move || {
                crate::whisper::engine_thread_main(
                    engine_cmd_rx,
                    engine_event_tx,
                    engine_app_handle,
                    engine_current_model_for_engine,
                );
            });

            // The recorder is lazy
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

            // Open the SQLite data layer (stats + history)
            // and seed it from legacy `stats.json` / `history.json` if those
            // exist. Each file is imported once, guarded by a DB marker, and
            // this runs synchronously in setup() so the dispatcher
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
                let config_dir = crate::user_data::data_dir();
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
            // Entries that aged out while the app was closed. After the block
            // above: the prune takes the connection itself, after the config
            // lock.
            crate::history::prune_to_settings(app.handle(), &db_arc);

            // Product telemetry is independent from stats/history and is
            // deliberately non-fatal. The installation ID is random and
            // stored in SQLite meta; no machine/account fingerprinting is
            // used. A missing build key makes the whole path a no-op.
            let startup_config = crate::config::Config::load(app.handle()).ok();
            if startup_config.as_ref().is_some_and(|config| {
                let formatting = text_formatting_config(config);
                formatting.enabled
                    && ((formatting.correct_spelling && speech_language(Some(config)) == "ru")
                        || !formatting.effective_custom_words().is_empty())
            }) {
                // Prepare the local lexicon off the UI thread before the first
                // dictation. Lazy also covers later changes to these settings.
                tauri::async_runtime::spawn_blocking(crate::spelling::prepare);
            }
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
                recorder,
                db_arc,
                microphone_test,
                engine_current_model,
            );
            app.manage(engine_state);
            spawn_model_autoload(app.handle().clone());
            spawn_idle_watchdog(app.handle().clone());
            crate::engine_events::spawn(app.handle().clone(), engine_event_rx);

            // Startup: register the saved hotkey from config.json.
            // The shortcut handler captures an AppState clone so it can
            // dispatch into the engine thread.
            let state: tauri::State<AppState> = app.state();
            match config::load_hotkey(app.handle()) {
                Ok(hotkey) => {
                    if let Err(e) = hotkey::register(app.handle(), &state, &hotkey) {
                        let _ = app.emit("hotkey-error", e);
                    }
                }
                Err(e) => log::warn!("could not load hotkey from config: {e}"),
            }
            // Wire the overlay to engine lifecycle events
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
            audio_file::pick_audio_file,
            audio_file::transcribe_audio_file,
            audio_file::cancel_audio_file,
            overlay::hide,
            overlay::current_state,
            overlay::overlay_ready,
            external_link::open_url,
            hotkey::validate_hotkey,
            ai::fetch_provider_models,
            model_download::cancel_model_download,
            crate::overlay::set_overlay_presentation,
            dictation::start_recording,
            dictation::stop_recording,
            dictation::cancel_recording,
            mic_test::start_microphone_test,
            mic_test::stop_microphone_test,
            mic_test::set_microphone_test_monitor,
            // Stats and history: `async fn`s whose DB work runs through
            // `spawn_blocking` via `run_db_op`.
            stats::get_stats,
            history::list_history,
            history::delete_history_entry,
            history::clear_history,
            // Manual LLM processing of an existing history entry: the run
            // and the write are separate so the result can be reviewed first.
            history::preview_history_ai_processing,
            history::apply_history_ai_processing,
            // Settings.
            config::get_config,
            config::save_config,
            // Boot-blocking commands called from MainWindow.load() via Promise.all.
            app_version,
            release_notes::get_whats_new,
            release_notes::dismiss_whats_new,
            updater::check_update,
            updater::install_update,
            audio::list_microphones,
            model::list_models,
            model_performance::model_assessments,
            get_runtime_status,
            // Model lifecycle.
            model_download::download_model,
            model::set_model,
            model::delete_model,
            // API-key storage (native secret store). The frontend's
            // API-keys / providers pages depend on these three.
            secret_store::save_api_key,
            secret_store::has_api_key,
            secret_store::delete_api_key,
            // Explicit-LLM commands: "Тест" and "Обработать текст".
            ai::test_ai_prompt,
            ai::process_text_ai,
            format_commands::preview_format,
            format_commands::preview_replacements,
            // macOS: check Accessibility permission on demand (frontend can
            // call this after the user grants access in System Settings).
            accessibility::check_accessibility,
            clipboard::test_paste,
            sounds::preview_sound_cue,
            output_volume::preview_output_duck,
            get_output_contract,
            debug::get_diagnostics,
            feedback::get_public_diagnostics,
            feedback::get_public_logs,
            feedback::save_public_logs,
            debug::open_diagnostics_folder,
            debug::logs_size,
            debug::clear_logs,
            dictionaries::dictionary_presets,
            dictionaries::parasite_sets,
            dictionaries::analyze_dictionary,
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

/// Result of the post-Whisper pipeline: local formatting + optional LLM.
///
/// `raw_text` is the untouched whisper output, `formatted_text` is the
/// pre-LLM text (after local formatting), and `final_text` is what actually
/// gets pasted. `ai_json` / `stats_json` are the serialized `ai_processing`
/// and `processing_stats` blobs the history UI renders.
pub(crate) struct ProcessedTranscription {
    pub(crate) raw_text: String,
    pub(crate) formatted_text: String,
    pub(crate) final_text: String,
    ai_json: Option<String>,
    /// Same thing as `ai_json`, unserialized. History stores the JSON; the
    /// stats aggregate needs the fields, and re-parsing our own JSON to get
    /// them back would be silly.
    pub(crate) ai_status: Option<crate::ai::step::AiStatus>,
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

/// Resolve the configured pipeline for a telemetry outcome.  Unknown or
/// malformed values are reported as `local` here, matching the safe runtime
/// default used by the recording path.
pub(crate) fn telemetry_pipeline_mode(config: Option<&crate::config::Config>) -> String {
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
pub(crate) fn telemetry_compute(
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
pub(crate) fn telemetry_formatting(config: &crate::config::Config) -> (bool, usize) {
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
pub(crate) fn ai_processing_json(status: Option<&crate::ai::step::AiStatus>) -> Option<String> {
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
pub(crate) async fn post_process_transcription(
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
    //    The shared formatter classifies intentional empty results after
    //    protecting code and replacement matches. Incidental empty cleanup
    //    falls back to raw text; hallucinations and explicit deletions do not.
    let formatted_text = match &config {
        Some(cfg) => {
            let value = cfg.as_value().clone();
            let text = raw_text.clone();
            tauri::async_runtime::spawn_blocking(move || {
                crate::formatter::format_transcription_with_config_value(&value, &text)
            })
            .await
            .unwrap_or_else(|_| raw_text.clone())
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
                crate::secret_store::load_key(&ai_cfg.api_key_ref)
                    .await
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
        "speech_seconds": inference.speech_seconds,
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

/// Borrow the `ai_processing` sub-object from a loaded config, or a
/// stable error string. Centralizes the lookup shared by the retry
/// and cloud-STT paths (both need the same object and the same
/// "missing ai_processing config" error message).
pub(crate) fn ai_processing_config(config: &crate::config::Config) -> Result<&Value, String> {
    config
        .as_value()
        .get("ai_processing")
        .ok_or_else(|| "missing ai_processing config".to_string())
}

/// Build a `CloudSttRequest` from the on-disk config + secret
/// store. Returns `Err(msg)` if any required field is missing —
/// the dispatcher surfaces the error in the existing toast.
pub(crate) fn build_cloud_stt_request(
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
            stt_service: None,
            inference_time_ms: 500,
            audio_seconds: 4.0,
            speech_seconds: None,
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
                "not classified as empty: {blank:?}"
            );
        }
    }

    #[test]
    fn real_text_goes_on_to_post_processing() {
        let outcome = classify_completion(false, Ok(inference("привет")));
        match outcome {
            Completion::Transcribed(result) => assert_eq!(result.text, "привет"),
            other => panic!("expected Transcribed, got {other:?}"),
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
            other => panic!("expected Failed, got {other:?}"),
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
