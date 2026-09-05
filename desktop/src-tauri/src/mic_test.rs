//! Microphone test (Phase 4 / Batch 4 / PR 4.1).
//!
//! 1:1 port of `sidecar.py::handle_start_microphone_test` /
//! `handle_stop_microphone_test`. The Rust implementation owns a
//! dedicated `MicrophoneTest` value behind `Arc<Mutex<_>>` so the
//! poller thread and the Tauri command body can both reach the
//! recorder + the saw-signal flag without moving the recorder.
//!
//! Events emitted:
//! - `microphone-test-started`  — fired once on successful start.
//! - `microphone-test-level`     — fired ~25 Hz while the test is
//!   running (the cpal callback updates the EMA inside `AudioRecorder`;
//!   we poll the `level()` getter).
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

    pub fn start(&self, app: &AppHandle, microphone: Option<String>) -> Result<bool, String> {
        let mut guard = crate::mutex_recover::lock(&self.inner);
        if guard.recorder.is_some() {
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
        // CRITICAL: release the `inner` lock before `spawn_workers`, which
        // re-locks `inner` (to store the join handles) on THIS same thread.
        // std `Mutex` is not reentrant, so holding `guard` across the call
        // self-deadlocks the command thread forever — the mic test would
        // hang and freeze the app. Headless CI never exercises this path,
        // so the deadlock shipped unnoticed since the Rust port.
        drop(guard);
        *crate::mutex_recover::lock(&self.app) = Some(app.clone());
        let _ = app.emit("microphone-test-started", ());
        Self::spawn_workers(&self.inner, app, samples);
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

    pub fn info(&self) -> Result<MicrophoneTestInfo, String> {
        Ok(MicrophoneTestInfo {
            active: crate::mutex_recover::lock(&self.inner).active,
        })
    }

    pub fn is_active(&self) -> bool {
        crate::mutex_recover::lock(&self.inner).recorder.is_some()
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
                    let audio: Vec<f32> = samples.try_iter().flatten().collect();
                    if !audio.is_empty() {
                        let _ = poller_app.emit_to("main", "microphone-test-audio", audio);
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
                            "message": "Звук не обнаружен. Скажите что-нибудь, проверьте подключение, выбранный микрофон и его громкость. Тишина сама по себе не означает запрет доступа.",
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
