//! Microphone test.
//!
//! Owns a dedicated `MicrophoneTest` value behind `Arc<Mutex<_>>` so the
//! poller thread and the Tauri command body can both reach the
//! recorder + the saw-signal flag without moving the recorder.
//!
//! Events emitted:
//! - `microphone-test-started`  — fired once on successful start.
//! - `microphone-test-level`     — fired ~25 Hz while the test is
//!   running (the cpal callback updates the EMA inside `AudioRecorder`;
//!   we poll the `level()` getter).
//! - `microphone-test-audio`    — raw frames for echo monitoring; emitted
//!   only while monitoring is on, so a plain level check does not pay for
//!   serialising the whole capture over IPC.
//! - `microphone-test-stopped`  — fired once on stop.
//! - `app-error`     — fired when the OS rejects access
//!   (macOS TCC denial) or when 2 s of silence suggests the same.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json;
use tauri::{AppHandle, Emitter};

use crate::audio::{AudioConfig, AudioRecorder};
use crate::{panic_msg, AppState};

const SILENCE_WATCH_SECS: f64 = 2.0;
const LEVEL_POLL_HZ: u64 = 25;

#[derive(Debug, Clone, Serialize)]
pub struct MicrophoneTestInfo {
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LevelPayload {
    pub level: f32,
}

/// One frame of echo monitoring: the audio and the rate it was captured at.
#[derive(Debug, Clone, Serialize)]
struct MonitorPayload {
    sample_rate: u32,
    samples: Vec<f32>,
}

struct Inner {
    recorder: Option<AudioRecorder>,
    saw_signal: bool,
    /// When the test was started, so the silence-watch can compute
    /// the right 2 s deadline regardless of how long the poller has
    /// been running.
    started_at: Instant,
    poller: Option<std::thread::JoinHandle<()>>,
    silence_watch: Option<std::thread::JoinHandle<()>>,
    active: bool,
    /// Signalled by `stop()` so worker threads exit promptly instead
    /// of blocking `join()` for up to one loop interval (40ms).
    stop_signal: Arc<AtomicBool>,
    /// Echo monitoring: when off, the poller drops the captured frames
    /// instead of emitting them. The level meter runs either way — the
    /// two are separate user-facing modes, and hearing yourself is the
    /// one that needs headphones.
    monitor: Arc<AtomicBool>,
}

impl Inner {
    fn new() -> Self {
        Self {
            recorder: None,
            saw_signal: false,
            started_at: Instant::now(),
            poller: None,
            silence_watch: None,
            active: false,
            stop_signal: Arc::new(AtomicBool::new(false)),
            monitor: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[derive(Clone)]
pub struct MicrophoneTest {
    inner: Arc<Mutex<Inner>>,
    app: Arc<Mutex<Option<AppHandle>>>,
}

impl Default for MicrophoneTest {
    fn default() -> Self {
        Self::new()
    }
}

impl MicrophoneTest {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::new())),
            app: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(
        &self,
        app: &AppHandle,
        microphone: Option<String>,
        monitor: bool,
    ) -> Result<bool, String> {
        let mut guard = crate::mutex_recover::lock(&self.inner);
        if guard.recorder.is_some() {
            // Already capturing (the level check and echo share one stream):
            // only the monitoring flag can still differ.
            guard.monitor.store(monitor, Ordering::Release);
            return Ok(true);
        }
        let recorder = AudioRecorder::new(AudioConfig::default())
            .map_err(|error| Self::emit_error(app, &error))?;
        recorder
            .start_selected(microphone.as_deref())
            .map_err(|error| Self::emit_error(app, &error))?;
        let samples = recorder.attach_live_tap(8);
        guard.recorder = Some(recorder);
        guard.saw_signal = false;
        guard.active = true;
        guard.started_at = Instant::now();
        guard.stop_signal.store(false, Ordering::Release);
        guard.monitor.store(monitor, Ordering::Release);
        // The poller reads the same flag the `set_monitor` command writes, so
        // echo can be toggled mid-test without restarting the capture.
        let monitor_flag = Arc::clone(&guard.monitor);
        // CRITICAL: release the `inner` lock before `spawn_workers`, which
        // re-locks `inner` (to store the join handles) on THIS same thread.
        // std `Mutex` is not reentrant, so holding `guard` across the call
        // self-deadlocks the command thread forever — the mic test would
        // hang and freeze the app. Headless CI never exercises this path,
        // so the deadlock shipped unnoticed since the Rust port.
        drop(guard);
        *crate::mutex_recover::lock(&self.app) = Some(app.clone());
        let _ = app.emit("microphone-test-started", ());
        Self::spawn_workers(&self.inner, app, samples, monitor_flag);
        Ok(true)
    }

    pub fn stop(&self, app: &AppHandle) -> Result<bool, String> {
        // Idempotent: double-clicks or errors from the frontend should
        // not cause issues. If no recorder is active, return immediately.
        let mut guard = crate::mutex_recover::lock(&self.inner);
        if guard.recorder.is_none() && guard.poller.is_none() && guard.silence_watch.is_none() {
            return Ok(false);
        }

        // Signal workers to exit, then take handles and recorder.
        guard.stop_signal.store(true, Ordering::Release);
        let poller = guard.poller.take();
        let silence_watch = guard.silence_watch.take();
        let recorder = guard.recorder.take();
        guard.active = false;
        guard.monitor.store(false, Ordering::Release);
        drop(guard);

        // Join workers (they should exit within ~40ms after seeing stop_signal).
        if let Some(handle) = poller {
            let _ = handle.join();
        }
        if let Some(handle) = silence_watch {
            let _ = handle.join();
        }
        if let Some(recorder) = recorder {
            let _ = recorder.stop();
        }
        *crate::mutex_recover::lock(&self.app) = None;
        let _ = app.emit("microphone-test-stopped", ());
        Ok(false)
    }

    /// Turn echo monitoring on or off without touching the capture, so the
    /// echo button can be pressed while the level check is already running.
    pub fn set_monitor(&self, enabled: bool) {
        crate::mutex_recover::lock(&self.inner)
            .monitor
            .store(enabled, Ordering::Release);
    }

    pub fn info(&self) -> Result<MicrophoneTestInfo, String> {
        Ok(MicrophoneTestInfo {
            active: crate::mutex_recover::lock(&self.inner).active,
        })
    }

    fn emit_error(app: &AppHandle, error: &str) -> String {
        let _ = app.emit(
            "app-error",
            serde_json::json!({
                "kind": "audio",
                "message": error,
            }),
        );
        format!("microphone test start failed: {error}")
    }

    fn spawn_workers(
        inner: &Arc<Mutex<Inner>>,
        app: &AppHandle,
        samples: std::sync::mpsc::Receiver<Vec<f32>>,
        monitor: Arc<AtomicBool>,
    ) {
        let poller_inner = Arc::clone(inner);
        let poller_app = app.clone();
        let poller = std::thread::spawn(move || {
            // catch_unwind so a panic inside the poller loop (e.g. from
            // cpal or level computation) does not kill the app process.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let interval = Duration::from_millis(1000 / LEVEL_POLL_HZ.max(1));
                loop {
                    std::thread::sleep(interval);
                    let mut guard = crate::mutex_recover::lock(&poller_inner);
                    if guard.stop_signal.load(Ordering::Acquire) {
                        break;
                    }
                    let Some(recorder) = guard.recorder.as_ref() else {
                        break;
                    };
                    if !recorder.is_recording() {
                        break;
                    }
                    let raw = recorder.level();
                    // Read under the same guard as the level: once it is
                    // dropped, `stop` may take the recorder away.
                    let sample_rate = recorder.tap_sample_rate();
                    recorder.discard_buffer();
                    // Once we've seen a real signal, remember it so the
                    // silence-watch below doesn't raise a bogus permission
                    // warning. (Previously `saw_signal` was never set, so on
                    // macOS the watch always fired after 2 s.)
                    if raw > 0.003 {
                        guard.saw_signal = true;
                    }
                    drop(guard);
                    // Perceptual mapping so the VU meter actually moves — raw
                    // speech RMS (~0.005..0.05) is far below the meter's 0.08
                    // active threshold. Shared with the overlay waveform.
                    let level = crate::audio::display_level(raw);
                    let _ = poller_app.emit("microphone-test-level", LevelPayload { level });
                    // Drain the tap unconditionally — a full channel makes the
                    // audio callback drop frames — but only pay for the emit
                    // while the user is actually listening to themselves.
                    let audio: Vec<f32> = samples.try_iter().flatten().collect();
                    if monitor.load(Ordering::Acquire) && !audio.is_empty() {
                        // The rate travels with the samples. A device whose
                        // rate the capture resampler cannot divide (44.1 kHz,
                        // ratio ≈ 2.76) passes its own audio through
                        // untouched, and playing that back as 16 kHz stretches
                        // the voice almost threefold and drops it by an octave
                        // and a half — which sounds like a broken microphone,
                        // not like a rate mismatch.
                        let _ = poller_app.emit_to(
                            "main",
                            "microphone-test-audio",
                            MonitorPayload {
                                sample_rate,
                                samples: audio,
                            },
                        );
                    }
                }
            }));
        });
        let watch_inner = Arc::clone(inner);
        let watch_app = app.clone();
        let watch = std::thread::spawn(move || {
            // catch_unwind so a panic inside the silence-watch does not
            // crash the app.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let deadline = Instant::now() + Duration::from_secs_f64(SILENCE_WATCH_SECS);
                while Instant::now() < deadline {
                    if crate::mutex_recover::lock(&watch_inner)
                        .stop_signal
                        .load(Ordering::Acquire)
                    {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                let guard = crate::mutex_recover::lock(&watch_inner);
                if guard.stop_signal.load(Ordering::Acquire) {
                    return;
                }
                let active = if let Some(recorder) = guard.recorder.as_ref() {
                    recorder.is_recording() && !guard.saw_signal
                } else {
                    false
                };
                drop(guard);
                if active {
                    let _ = watch_app.emit(
                        "app-error",
                        serde_json::json!({
                            "kind": "audio",
                            "message": crate::ui_text::t("Звук не обнаружен. Скажите что-нибудь, проверьте подключение, выбранный микрофон и его громкость. Тишина сама по себе не означает запрет доступа."),
                        }),
                    );
                }
            }));
        });
        let mut guard = crate::mutex_recover::lock(inner);
        guard.poller = Some(poller);
        guard.silence_watch = Some(watch);
    }
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
#[tauri::command]
pub(crate) async fn start_microphone_test(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    microphone: Option<serde_json::Value>,
    monitor: Option<bool>,
) -> Result<MicrophoneTestInfo, String> {
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
pub(crate) async fn stop_microphone_test(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<MicrophoneTestInfo, String> {
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
pub(crate) async fn set_microphone_test_monitor(
    state: tauri::State<'_, AppState>,
    enabled: bool,
) -> Result<MicrophoneTestInfo, String> {
    let test = state.microphone_test.clone();
    state.audio.call(move || test.set_monitor(enabled)).await?;
    state.microphone_test.info()
}
