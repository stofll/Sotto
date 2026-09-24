//! Shared hotkey/IPC capture lifecycle. Reservations and worker submission are
//! ordered together; native device operations never run on the caller's thread.

use crate::{
    config::Config,
    state::{AppFsm, AppState},
    telemetry,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::oneshot;

pub type Reply = oneshot::Receiver<Result<u64, String>>;

pub fn finish(state: &AppState, session_id: u64) {
    if state.owns_dictation(session_id) {
        crate::clipboard::clear_target();
    }
    state.finish_session(session_id);
}

fn failed(app: &AppHandle, state: &AppState, session_id: u64, message: &str) {
    crate::on_recording_stopped(app, session_id, None);
    let _ = app.emit(
        "whisper-failed",
        serde_json::json!({
            "session_id": session_id, "message": message,
        }),
    );
    finish(state, session_id);
}

fn cancelled(app: &AppHandle, state: &AppState, session_id: u64) {
    state.recorder.detach_live_tap();
    crate::output_volume::restore();
    let _ = app.emit("whisper-cancelled", session_id);
    app.state::<telemetry::Telemetry>().record_cancelled(
        telemetry::Source::Microphone,
        &crate::telemetry_pipeline_mode_of(app),
    );
    finish(state, session_id);
}

fn native_call<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(crate::panic_msg)?
}

pub fn start(app: &AppHandle, state: &AppState, from_hotkey: bool) -> Result<Reply, String> {
    let requested = Instant::now();
    if let Some(refusal) = crate::refuse_dictation_start(app, state) {
        return Err(refusal.message);
    }
    let config = Config::load(app)?;
    let selected = crate::config::microphone_selection(config.get("microphone"));
    let _commands = crate::mutex_recover::lock(&state.capture_commands);
    let session_id = state
        .try_begin_dictation()
        .ok_or_else(crate::engine_busy_message)?;
    crate::clipboard::clear_target();
    if from_hotkey {
        crate::clipboard::capture_target();
    }
    state.toggle_armed.store(
        config.get_string("recording_mode").as_deref() != Some("push_to_talk"),
        Ordering::Release,
    );
    let (tx, rx) = oneshot::channel();
    let worker_app = app.clone();
    let worker_state = state.clone();
    if let Err(error) = state.audio.submit(move || {
        let app = worker_app;
        let state = worker_state;
        if state.is_cancelled(session_id) {
            // The queued cancel owns terminal cleanup, including a start that
            // was cancelled before the device had even opened.
            let _ = tx.send(Ok(session_id));
            return;
        }
        let queued_ms = requested.elapsed().as_millis();
        let result = native_call(|| state.recorder.start_selected(selected.as_deref()));
        match result {
            Ok(()) => {
                log::info!(
                    "capture timing: session={session_id} queue_ms={queued_ms} stream_ready_ms={}",
                    requested.elapsed().as_millis()
                );
                crate::spawn_level_emitter(&app, Arc::clone(&state.recorder));
                crate::state::set_app_fsm(&state.app_fsm, AppFsm::Recording);
                crate::on_recording_started(&app);
                app.state::<telemetry::Telemetry>()
                    .begin_usage_session(telemetry::SessionTrigger::Microphone);
                let _ = app.emit("recording-started", session_id);
                let _ = tx.send(Ok(session_id));
            }
            Err(error) => {
                // A panic may leave a partially opened stream. Teardown stays
                // on this same worker, before the reservation is released.
                let _ = native_call(|| state.recorder.stop());
                crate::record_recorder_start_failure(&app);
                failed(&app, &state, session_id, &error);
                let _ = tx.send(Err(error));
            }
        }
    }) {
        failed(app, state, session_id, &error);
        return Err(error);
    }
    Ok(rx)
}

pub fn stop(app: &AppHandle, state: &AppState) -> Result<Reply, String> {
    let _commands = crate::mutex_recover::lock(&state.capture_commands);
    let session_id = state.current_session_id.swap(0, Ordering::AcqRel);
    state.toggle_armed.store(false, Ordering::Release);
    submit_stop(app, state, session_id)
}

/// Stop `session_id` only while it is still the live recording, so a decision
/// made about one recording cannot stop the one started after it.
pub fn stop_if_current(
    app: &AppHandle,
    state: &AppState,
    session_id: u64,
) -> Result<Reply, String> {
    let _commands = crate::mutex_recover::lock(&state.capture_commands);
    let claimed = state.claim_live_session(session_id);
    if claimed {
        state.toggle_armed.store(false, Ordering::Release);
    }
    submit_stop(app, state, if claimed { session_id } else { 0 })
}

/// Queue the stop of a session already taken from `current_session_id`; 0
/// replies at once. The caller holds `capture_commands`.
fn submit_stop(app: &AppHandle, state: &AppState, session_id: u64) -> Result<Reply, String> {
    let (tx, rx) = oneshot::channel();
    if session_id == 0 {
        let _ = tx.send(Ok(0));
        return Ok(rx);
    }
    let worker_app = app.clone();
    let worker_state = state.clone();
    if let Err(error) = state.audio.submit(move || {
        let result = stop_on_worker(&worker_app, &worker_state, session_id);
        let _ = tx.send(result.map(|()| session_id));
    }) {
        failed(app, state, session_id, &error);
        return Err(error);
    }
    Ok(rx)
}

fn stop_on_worker(app: &AppHandle, state: &AppState, session_id: u64) -> Result<(), String> {
    // A failed start already emitted its terminal event. Its queued stop must
    // not stop a later recording or produce a second failure.
    if !state.is_session_active(session_id) {
        return Ok(());
    }
    let stopped = native_call(|| state.recorder.stop());
    if state.is_cancelled(session_id) {
        cancelled(app, state, session_id);
        return Ok(());
    }
    let audio = match stopped {
        Ok(Some(audio)) if !audio.is_empty() => audio,
        Ok(_) => {
            crate::abandon_dictation(app, state, session_id, telemetry::FailureReason::NoAudio);
            let _ = app.emit("whisper-empty", session_id);
            finish(state, session_id);
            return Ok(());
        }
        Err(error) => {
            app.state::<telemetry::Telemetry>().record_failed(
                telemetry::Source::Microphone,
                &crate::telemetry_pipeline_mode_of(app),
                telemetry::FailureStage::Capture,
                telemetry::FailureReason::RecorderStop,
            );
            failed(app, state, session_id, &error);
            return Err(error);
        }
    };
    crate::on_recording_stopped(app, session_id, Some(&audio));
    let flag = Arc::new(AtomicBool::new(false));
    state.register_cancel_flag(session_id, flag.clone());
    let (reply, _rx) = oneshot::channel();
    let config = Config::load(app).ok();
    let command = match crate::build_dictation_command(
        app,
        config.as_ref(),
        session_id,
        audio,
        flag,
        reply,
    ) {
        Ok(command) => command,
        Err(error) => {
            failed(app, state, session_id, &error);
            return Err(error);
        }
    };
    crate::state::set_app_fsm(&state.app_fsm, AppFsm::Processing);
    let _ = app.emit("recording-stopped", session_id);
    if let Err(error) = state.engine_cmd_tx.try_send(command) {
        crate::record_engine_queue_failure(app, config.as_ref());
        let message = format!("engine: {error}");
        failed(app, state, session_id, &message);
        return Err(message);
    }
    Ok(())
}

pub async fn cancel(app: &AppHandle, state: &AppState, session_id: u64) -> Result<bool, String> {
    let reply = {
        let _commands = crate::mutex_recover::lock(&state.capture_commands);
        if !state.request_cancel(session_id) {
            return Ok(false);
        }
        if !state.claim_live_session(session_id) {
            // Stop/engine/LLM owns completion and observes the marker.
            return Ok(true);
        }
        let (tx, rx) = oneshot::channel();
        let worker_app = app.clone();
        let worker_state = state.clone();
        if let Err(error) = state.audio.submit(move || {
            let _ = native_call(|| worker_state.recorder.stop());
            if worker_state.is_session_active(session_id) {
                cancelled(&worker_app, &worker_state, session_id);
            }
            let _ = tx.send(());
        }) {
            failed(app, state, session_id, &error);
            return Err(error);
        }
        rx
    };
    reply
        .await
        .map_err(|_| "audio worker dropped cancellation".to_string())?;
    Ok(true)
}

/// Returns the `session_id` so any sync caller can immediately use it
/// (e.g. for cancel). The frontend currently discards the return value
/// (it reads session_id from the `recording-started` event payload).
#[tauri::command]
pub(crate) async fn start_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u64, String> {
    start(&app, &state, false)?
        .await
        .map_err(|_| "audio worker dropped start".to_string())?
}

/// Stop the active recording session and send the captured audio to the
/// whisper engine.
///
/// Returns the stopped `session_id`, or 0 when no recording was active.
#[tauri::command]
pub(crate) async fn stop_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<u64, String> {
    stop(&app, &state)?
        .await
        .map_err(|_| "audio worker dropped stop".to_string())?
}

/// Cancel an in-flight recording / transcription session.
///
/// Marks the session as cancelled in `AppState` (the dispatcher checks
/// this BEFORE pasting — see `setup()` in `lib.rs`). If a recording is
/// still active (user pressed hotkey then cancelled before releasing),
/// the cpal stream is dropped so the audio buffer is discarded.
///
/// The frontend must capture the session_id from `recording-started` and
/// pass it here. If it loses the id (refresh, etc.), passing a wrong id is
/// a no-op: the session is not cancellable, and the call returns `false`.
#[tauri::command]
pub(crate) async fn cancel_recording(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    session_id: u64,
) -> Result<bool, String> {
    cancel(&app, &state, session_id).await
}
