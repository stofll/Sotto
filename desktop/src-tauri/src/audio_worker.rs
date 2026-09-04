//! Serializes every cpal/WASAPI call onto one dedicated thread.
//!
//! Two verified hazards made this necessary, and both produced hard hangs
//! that Windows rendered as its unresponsive-window placeholder — a white
//! box with a default system title bar, drawn over whichever of our windows
//! happened to be on screen:
//!
//! 1. **cpal was running on the UI thread.** `global-hotkey` creates its
//!    `WM_HOTKEY` receiver window on whatever thread constructs the manager
//!    — the main thread, during plugin setup — and dispatches our handler
//!    inline from that window's WndProc
//!    (`global-hotkey-0.8.0/src/platform_impl/windows/mod.rs:146`). So
//!    every hotkey press opened a WASAPI device on the main thread, and
//!    toggle mode's second press dropped the stream there too. `Stream`'s
//!    `Drop` `join()`s cpal's audio thread, so a slow or wedged audio
//!    engine froze the entire app.
//!
//! 2. **`#[tauri::command(async)]` does not use a blocking pool.** On a
//!    *sync* fn the macro wraps the body in `async move` and hands it to
//!    `async_runtime::spawn`
//!    (`tauri-macros-2.6.1/src/command/wrapper.rs:388`); `"sync_threadpool"`
//!    in that file is a tracing label, not an execution mode. The same
//!    `join()` therefore parked a tokio *core* worker — the runtime that
//!    also serves every IPC response and the engine dispatcher.
//!
//! Jobs are plain closures, so each caller picks its own reply mechanism:
//! [`AudioWorker::submit`] for fire-and-forget (the hotkey path, which must
//! never block) and [`AudioWorker::call`] for async commands (awaits a
//! oneshot, so it parks no thread at all).
//!
//! Serializing has a second benefit: a cpal stream is now always created
//! and dropped on the same thread, which is what WASAPI wants. The previous
//! arrangement — created on the main thread by the hotkey, dropped on an
//! arbitrary tokio worker by `cancel_recording` — did not guarantee that.

use std::sync::{
    mpsc::{channel, Sender},
    Arc, Mutex,
};
use std::thread;

type Job = Box<dyn FnOnce() + Send + 'static>;

/// Cloning hands out another handle to the *same* thread — `AppState` is
/// `Clone`, and a second audio thread would defeat the point.
#[derive(Clone)]
pub struct AudioWorker {
    /// `std::sync::mpsc::Sender` is `Send` but not `Sync`, and `AppState`
    /// has to be `Sync`. The lock is only ever held across a send on an
    /// unbounded channel, which cannot block — nothing here can become the
    /// kind of contention this module exists to remove.
    tx: Arc<Mutex<Sender<Job>>>,
}

impl AudioWorker {
    pub fn spawn() -> Self {
        let (tx, rx) = channel::<Job>();
        thread::Builder::new()
            .name("audio-worker".into())
            .spawn(move || {
                while let Ok(job) = rx.recv() {
                    // cpal can panic on device disconnects and WASAPI
                    // exclusive-mode conflicts. Letting that kill the
                    // worker would silently disable recording for the rest
                    // of the session, so absorb it here.
                    if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job))
                    {
                        log::error!("audio worker job panicked: {}", crate::panic_msg(panic));
                    }
                }
                log::info!("audio worker thread exiting (channel closed)");
            })
            .expect("spawn audio worker thread");
        Self {
            tx: Arc::new(Mutex::new(tx)),
        }
    }

    /// Queue a job and return immediately.
    ///
    /// For callers that must not block under any circumstances — above all
    /// the hotkey handler, which Windows runs on the main thread.
    pub fn submit(&self, job: impl FnOnce() + Send + 'static) {
        let tx = crate::mutex_recover::lock(&self.tx);
        if tx.send(Box::new(job)).is_err() {
            log::error!("audio worker is gone, dropping job");
        }
    }

    /// Queue a job and await its result.
    ///
    /// Parks no thread: the caller's future suspends on a oneshot until the
    /// worker answers.
    pub async fn call<T, F>(&self, f: F) -> Result<T, String>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.submit(move || {
            // Receiver dropped means the caller's future was cancelled —
            // the work still ran, which is what we want for stop/teardown.
            let _ = tx.send(f());
        });
        rx.await
            .map_err(|_| "audio worker dropped the job".to_string())
    }
}

impl std::fmt::Debug for AudioWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AudioWorker")
    }
}
