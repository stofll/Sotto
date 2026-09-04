//! Recording state machine + Whisper engine state machine + AppState container.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

/// Application-level state machine (visible to UI via events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppFsm {
    Idle,
    Recording,
    Processing,
    Done,
    Error,
}

/// Whisper engine state (internal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineFsm {
    Unloaded,
    Loading,
    Ready,
    Inferring,
}

/// Set the application FSM state, recovering from a poisoned lock.
///
/// Free function (not an `AppState` method) so both the Tauri command
/// handlers — which hold `&state.app_fsm` — and the engine-event
/// dispatcher — which holds a bare `Arc<Mutex<AppFsm>>` clone rather
/// than an `AppState` — share one call shape. Centralizes the
/// `*mutex_recover::lock(..) = ..` idiom that was repeated ~11× in
/// `lib.rs`, so the poison-recovery policy lives in exactly one place.
pub fn set_app_fsm(fsm: &Mutex<AppFsm>, next: AppFsm) {
    *crate::mutex_recover::lock(fsm) = next;
}

/// RAII claim on the engine, handed out by `AppState::claim_engine`.
/// Dropping it frees the engine for the next job.
pub struct EngineBusyGuard {
    flag: Arc<AtomicBool>,
}

impl Drop for EngineBusyGuard {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

/// AppState — passed to Tauri commands via `tauri::State<AppState>`.
///
/// Holds channel sender (NOT receiver — receiver is single-consumer, owned
/// by setup()-scope then moved into the dispatcher task), session-id
/// tracking, and the engine thread JoinHandle for graceful shutdown.
///
/// `AppState` is `Clone` (WS 4a1 Task 13b) so the hotkey handler can
/// capture it into a `'static + Send` closure. Every non-Clone field is
/// wrapped in `Arc`, so the clone is just a handful of refcount bumps.
/// All clones share the same underlying state — `next_session_id` and
/// `cancel_session` calls are coordinated across every clone.
#[derive(Clone)]
pub struct AppState {
    pub app_fsm: Arc<Mutex<AppFsm>>,
    pub engine_fsm: Arc<Mutex<EngineFsm>>,
    pub engine_cmd_tx: tokio::sync::mpsc::Sender<crate::whisper::EngineCommand>,
    pub current_session_id: Arc<AtomicU64>,
    /// Dictation/file sessions that have started and have not reached their
    /// terminal delivery path yet.  `current_session_id` alone is not enough:
    /// `stop_recording` clears it before the audio worker finishes, which used
    /// to leave a cancellation click with no session it could claim.
    pub active_sessions: Arc<Mutex<HashSet<u64>>>,
    /// Sessions that have atomically entered final delivery (stats/history +
    /// paste).  Cancellation cannot overtake a session after this point; the
    /// claim is the linearization point between a user cancel and success.
    pub committing_sessions: Arc<Mutex<HashSet<u64>>>,
    pub cancelled_sessions: Arc<Mutex<HashSet<u64>>>,
    /// Phase 4 / Batch 6 / P0: session → cancel-flag registry.
    /// When a session's `Transcribe` command is queued, the engine
    /// registers its `Arc<AtomicBool>` here. `cancel_recording`
    /// flips the flag on lookup; the engine thread checks the
    /// flag before `state.full(...)` and short-circuits a cancel
    /// that lands during the (otherwise uninterruptible) `.full()`
    /// C call. The dispatcher clears the registry entry when an
    /// `InferenceCompleted` arrives so the working set stays
    /// bounded at zero (one in-flight session in practice).
    pub cancel_flags: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    /// Sessions the engine-event dispatcher must ignore entirely.
    ///
    /// File transcription (`transcribe_audio_file`) reuses the one whisper
    /// engine, so its result arrives on the same event channel as a
    /// dictation's. The dispatcher's job for a dictation is to paste into
    /// the focused window and write history — both catastrophic for a file
    /// the user is reading in our own window. Registering the session here
    /// before the command is queued makes the dispatcher drop *both* of the
    /// session's events on the floor; the command reads its result from the
    /// `oneshot` reply instead.
    ///
    /// Why both events and not just the completion: `InferenceStarted`
    /// raises the overlay (`overlay.rs`), and nothing else lowers it — the
    /// event that would (`InferenceCompleted`) is exactly the one being
    /// skipped. So the started-branch checks with `contains` and the
    /// completed-branch removes; a `remove` in the started-branch would let
    /// the completion through and paste the file into a foreign window.
    pub dispatch_skipped: Arc<Mutex<HashSet<u64>>>,
    /// True while a non-dictation job owns the engine (file transcription).
    ///
    /// Separate from `AppFsm` on purpose: the FSM describes a dictation's
    /// lifecycle and is returned to `Idle` by the dispatcher, which a file
    /// session deliberately bypasses. Recording commands refuse while this
    /// is set — the engine runs one job at a time, and a dictation queued
    /// behind an hour-long file would look frozen rather than rejected.
    pub engine_busy: Arc<AtomicBool>,
    pub engine_thread: Arc<Mutex<Option<JoinHandle<()>>>>,
    pub pending_recording: Arc<Mutex<bool>>,
    pub model_warming: Arc<Mutex<bool>>,
    /// WS 4a2 — cpal audio capture. Lives in AppState symmetric to the
    /// whisper engine. Held by `Arc` so Tauri commands (`start_recording`,
    /// `stop_recording`, `get_audio_level`) can borrow it cheaply, and so
    /// the engine-dispatcher task could in future subscribe to audio-level
    /// events without paying a clone cost. `AudioRecorder: Send + Sync`
    /// because every internal field is `Mutex<_>` / `Atomic_` /
    /// `Arc<Mutex<_>>` / `Arc<Atomic_>` — the only non-`Sync` inner type
    /// is `cpal::Stream`, which we hold inside `Mutex<Option<Stream>>`
    /// (Mutex requires `T: Send` for Sync, and `cpal::Stream: Send`).
    pub recorder: Arc<crate::audio::AudioRecorder>,

    /// The one thread allowed to touch cpal. Every `recorder.start()` /
    /// `recorder.stop()` and every microphone-test device call goes through
    /// here — see `crate::audio_worker` for why calling them inline froze
    /// the app.
    pub audio: crate::audio_worker::AudioWorker,

    /// WS 4b: rusqlite data layer (stats + history). Wrapped in
    /// `std::sync::Mutex<Connection>` (NOT `tokio::sync::Mutex` — the std
    /// variant's guard is `Send` when `T: Send`, which is what
    /// `tokio::task::spawn_blocking` requires; `tokio::sync::Mutex` is
    /// not `Send`-safe across await points). Held by `Arc` so the
    /// dispatcher can spawn a blocking write task and keep the original
    /// `AppState` in Tauri-managed state.
    pub db: Arc<Mutex<rusqlite::Connection>>,

    /// Microphone test state (Phase 4 / Batch 4 / PR 4.1). Wired to the
    /// shared `AudioRecorder` so the test can start/stop audio capture
    /// and poll levels during a self-test session.
    pub microphone_test: crate::mic_test::MicrophoneTest,

    /// Toggle-mode arm flag: set to true when the first toggle hotkey
    /// press starts recording, and cleared to false when the second
    /// toggle press stops recording. Provides a race-free alternative
    /// to querying `recorder.is_recording()` for toggle-mode decision
    /// logic, which can race with rapid hotkey events on Windows.
    pub toggle_armed: Arc<AtomicBool>,

    /// Physical key-held debounce flag. Windows global shortcuts (both the
    /// `RegisterHotKey` path and the low-level keyboard hook) fire *repeated*
    /// `Pressed` events while the combo is held down (OS key auto-repeat).
    /// Without debouncing, toggle mode flips start→stop within ~30-50 ms of a
    /// single physical press and the captured audio is too short to
    /// transcribe. `key_held` is set on the leading `Pressed` edge and
    /// cleared on `Released`, so the handler acts on one physical press only.
    pub key_held: Arc<AtomicBool>,

    /// Tracks which model (if any) is currently loaded into the whisper
    /// engine. Updated by the engine thread (via `Arc`) whenever
    /// `SetModel` succeeds or `UnloadModel` is called. Read by
    /// `list_models` and `get_runtime_status` so the UI can show an
    /// accurate indicator separate from `downloaded` (file on disk).
    pub engine_current_model: Arc<Mutex<Option<String>>>,

    /// Флаги отмены скачиваний, по идентификатору модели.
    ///
    /// Отдельно от `cancel_flags`: те живут сессиями распознавания и
    /// нумеруются, а скачивание идентифицируется моделью — отменить надо
    /// именно ту, что сейчас качается, и знать про неё пользователь может
    /// только по имени.
    pub download_cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl AppState {
    pub fn new(
        cmd_tx: tokio::sync::mpsc::Sender<crate::whisper::EngineCommand>,
        engine_thread_handle: JoinHandle<()>,
        recorder: Arc<crate::audio::AudioRecorder>,
        db: Arc<Mutex<rusqlite::Connection>>,
        microphone_test: crate::mic_test::MicrophoneTest,
        engine_current_model: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            app_fsm: Arc::new(Mutex::new(AppFsm::Idle)),
            engine_fsm: Arc::new(Mutex::new(EngineFsm::Unloaded)),
            engine_cmd_tx: cmd_tx,
            // JoinHandle stored for graceful shutdown (Task 10).
            engine_thread: Arc::new(Mutex::new(Some(engine_thread_handle))),
            current_session_id: Arc::new(AtomicU64::new(0)),
            active_sessions: Arc::new(Mutex::new(HashSet::new())),
            committing_sessions: Arc::new(Mutex::new(HashSet::new())),
            cancelled_sessions: Arc::new(Mutex::new(HashSet::new())),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            dispatch_skipped: Arc::new(Mutex::new(HashSet::new())),
            engine_busy: Arc::new(AtomicBool::new(false)),
            pending_recording: Arc::new(Mutex::new(false)),
            model_warming: Arc::new(Mutex::new(false)),
            toggle_armed: Arc::new(AtomicBool::new(false)),
            key_held: Arc::new(AtomicBool::new(false)),
            recorder,
            audio: crate::audio_worker::AudioWorker::spawn(),
            db,
            microphone_test,
            engine_current_model,
            download_cancels: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Занять модель под скачивание и получить страж, который освободит её
    /// на любом выходе — включая `?` и панику.
    ///
    /// `None` — эту модель уже качают. Проверка и запись под одним замком:
    /// две загрузки одной модели писали бы один и тот же `*.part`,
    /// наперегонки проверяли бы его сумму и переименовывали бы результат, а
    /// отмена доставалась бы только последней.
    ///
    /// Флаг регистрируется до первого байта: иначе отмена, нажатая в первую
    /// секунду, не найдёт что отменять и молча ничего не сделает.
    pub fn try_claim_download(&self, model_id: &str) -> Option<DownloadGuard> {
        let mut registry = crate::mutex_recover::lock(&self.download_cancels);
        if registry.contains_key(model_id) {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        registry.insert(model_id.to_string(), Arc::clone(&flag));
        Some(DownloadGuard {
            state: self.clone(),
            model_id: model_id.to_string(),
            flag,
        })
    }

    /// Попросить скачивание остановиться. `false` — такого скачивания нет.
    pub fn cancel_download(&self, model_id: &str) -> bool {
        let flag = crate::mutex_recover::lock(&self.download_cancels)
            .get(model_id)
            .cloned();
        match flag {
            Some(flag) => {
                flag.store(true, Ordering::Release);
                true
            }
            None => false,
        }
    }

    /// Cheap clone of the cancelled-sessions set for moving into an async
    /// dispatcher task. Returns an `Arc<Mutex<HashSet<u64>>>` which can
    /// check / drop / insert from any thread.
    pub fn cancelled_sessions_arc(&self) -> Arc<Mutex<HashSet<u64>>> {
        Arc::clone(&self.cancelled_sessions)
    }

    /// Cheap clone of the dispatch-skip set, for the same reason as
    /// `cancelled_sessions_arc`: the dispatcher task owns a bare `Arc`,
    /// not an `AppState`.
    pub fn dispatch_skipped_arc(&self) -> Arc<Mutex<HashSet<u64>>> {
        Arc::clone(&self.dispatch_skipped)
    }

    /// Register a session the dispatcher must ignore. Call this BEFORE
    /// queueing the engine command: the engine can finish and emit before
    /// a later insert lands, and a completion that slips past the check is
    /// pasted into whatever window happens to be focused.
    pub fn skip_dispatch(&self, session_id: u64) {
        crate::mutex_recover::lock(&self.dispatch_skipped).insert(session_id);
    }

    /// Undo `skip_dispatch` when the command fails before the engine ever
    /// runs (send error, dropped reply, early return).
    ///
    /// Not housekeeping — a leak here is a silent data-loss bug. Session
    /// ids restart from zero after every dictation (`stop_recording` swaps
    /// `current_session_id` back to 0), so a stale id WILL be handed out
    /// again, and the dictation that gets it is dropped by the dispatcher
    /// with no paste, no history entry, and no error anywhere.
    pub fn unskip_dispatch(&self, session_id: u64) {
        crate::mutex_recover::lock(&self.dispatch_skipped).remove(&session_id);
    }

    pub fn is_dispatch_skipped(&self, session_id: u64) -> bool {
        crate::mutex_recover::lock(&self.dispatch_skipped).contains(&session_id)
    }

    /// Take exclusive ownership of the engine for a non-dictation job.
    /// Returns `None` when another such job already holds it.
    ///
    /// The returned guard releases on drop, so every exit path of the
    /// caller — `?`, early return, panic — puts the engine back. Releasing
    /// by hand does not survive the `?` operator, which is how this kind of
    /// flag ends up stuck at `true` until the app restarts.
    pub fn claim_engine(&self) -> Option<EngineBusyGuard> {
        self.engine_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| EngineBusyGuard {
                flag: Arc::clone(&self.engine_busy),
            })
    }

    pub fn is_engine_busy(&self) -> bool {
        self.engine_busy.load(Ordering::Acquire)
    }

    /// Cheap clone of the engine command sender for moving into an async
    /// task. Used by the dispatcher when forwarding hotkey presses that
    /// arrive from a different runtime context.
    pub fn engine_cmd_tx_clone(&self) -> tokio::sync::mpsc::Sender<crate::whisper::EngineCommand> {
        self.engine_cmd_tx.clone()
    }

    pub fn next_session_id(&self) -> u64 {
        self.current_session_id.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Take the live-recording slot for `session_id`, returning whether this
    /// caller is the one that found it still there.
    ///
    /// Both stop paths and `cancel_recording` race for the same slot, and the
    /// winner is the path that owns the session's terminal cleanup — the
    /// loser must not stop the recorder, clear the cancellation marker, or
    /// emit a terminal event, because the winner will. Reading the slot and
    /// swapping it later is not the same thing: the read can go stale across
    /// an `.await`, and then both paths believe they own the session.
    ///
    /// A value that is neither ours nor 0 belongs to a `start_recording` that
    /// published its id while we were here; it is put straight back so the
    /// new session is not orphaned.
    pub fn claim_live_session(&self, session_id: u64) -> bool {
        let prev = self.current_session_id.swap(0, Ordering::AcqRel);
        if prev != session_id && prev != 0 {
            self.current_session_id.store(prev, Ordering::Release);
        }
        prev == session_id
    }

    /// Mark a newly allocated session as live before any async capture/queue
    /// work begins.  This is what lets a cancel click win while `stop_recording`
    /// is still waiting for the audio worker.
    pub fn begin_session(&self, session_id: u64) {
        if session_id != 0 {
            crate::mutex_recover::lock(&self.active_sessions).insert(session_id);
        }
    }

    pub fn is_session_active(&self, session_id: u64) -> bool {
        crate::mutex_recover::lock(&self.active_sessions).contains(&session_id)
    }

    /// Claim the final delivery phase.  Cancellation and this claim use the
    /// same lock order, so exactly one of them wins when the overlay click and
    /// an async completion arrive together.
    pub fn begin_commit(&self, session_id: u64) -> bool {
        let active = crate::mutex_recover::lock(&self.active_sessions);
        if !active.contains(&session_id) {
            return false;
        }
        let mut committing = crate::mutex_recover::lock(&self.committing_sessions);
        if committing.contains(&session_id) {
            return false;
        }
        if crate::mutex_recover::lock(&self.cancelled_sessions).contains(&session_id) {
            return false;
        }
        committing.insert(session_id);
        true
    }

    /// Remove every registration for a terminal session.  This is deliberately
    /// separate from `drop_cancellation`: the dispatcher must keep the cancel
    /// marker alive while formatting/LLM work is in flight.
    pub fn finish_session(&self, session_id: u64) {
        crate::mutex_recover::lock(&self.active_sessions).remove(&session_id);
        crate::mutex_recover::lock(&self.committing_sessions).remove(&session_id);
        self.drop_cancellation(session_id);
        self.clear_cancel_flag(session_id);
    }

    /// Request cancellation for a live session before stopping/finalizing it.
    /// Returns false for stale ids and for a session whose final delivery has
    /// already claimed the commit point.
    pub fn request_cancel(&self, session_id: u64) -> bool {
        let active = crate::mutex_recover::lock(&self.active_sessions);
        if !active.contains(&session_id) {
            return false;
        }
        let committing = crate::mutex_recover::lock(&self.committing_sessions);
        if committing.contains(&session_id) {
            return false;
        }
        let mut cancelled = crate::mutex_recover::lock(&self.cancelled_sessions);
        cancelled.insert(session_id);
        drop(cancelled);
        drop(committing);
        drop(active);
        self.flip_cancel_flag(session_id);
        true
    }

    pub fn cancel_session(&self, session_id: u64) {
        crate::mutex_recover::lock(&self.cancelled_sessions).insert(session_id);
        self.flip_cancel_flag(session_id);
    }

    fn flip_cancel_flag(&self, session_id: u64) {
        // Phase 4 / Batch 6 / P0: also flip the registered cancel
        // flag so the engine thread sees the cancel even if it is
        // currently inside the (non-interruptible) `state.full()`
        // C call. The flag is checked between segments and (with
        // the pre-full guard) at the top of the Transcribe arm.
        let flag = crate::mutex_recover::lock(&self.cancel_flags)
            .get(&session_id)
            .cloned();
        if let Some(flag) = flag {
            flag.store(true, Ordering::Release);
        }
    }

    /// Register a cancel flag for an in-flight session. Called by
    /// `stop_recording` just before it queues the `Transcribe`
    /// command.
    /// Register a file-transcription session and hand back the guard that
    /// unregisters it.
    ///
    /// A file session is invisible to the dispatcher by construction, so
    /// unlike a dictation it has to clean up after itself — on every exit
    /// path, including the `?`s. A leaked skip-set entry is not untidiness:
    /// ids restart from zero after every dictation, so a stale one silently
    /// swallows a later dictation with no paste and no error.
    pub fn claim_file_session(
        &self,
        session_id: u64,
        cancel_flag: Arc<AtomicBool>,
    ) -> FileSessionGuard {
        self.begin_session(session_id);
        // Both registered BEFORE the caller queues the command: the engine
        // can complete before a later insert lands, and a completion that
        // slips past the dispatcher's check is pasted into whatever window
        // happens to be focused.
        self.skip_dispatch(session_id);
        self.register_cancel_flag(session_id, cancel_flag);
        FileSessionGuard {
            state: self.clone(),
            session_id,
        }
    }

    pub fn register_cancel_flag(&self, session_id: u64, flag: Arc<AtomicBool>) {
        if self.is_cancelled(session_id) {
            flag.store(true, Ordering::Release);
        }
        crate::mutex_recover::lock(&self.cancel_flags).insert(session_id, flag);
    }

    /// Clear the cancel flag entry after the engine has finished
    /// (cancelled or completed). Called by the dispatcher on
    /// `InferenceCompleted`.
    pub fn clear_cancel_flag(&self, session_id: u64) {
        crate::mutex_recover::lock(&self.cancel_flags).remove(&session_id);
    }

    pub fn is_cancelled(&self, session_id: u64) -> bool {
        crate::mutex_recover::lock(&self.cancelled_sessions).contains(&session_id)
    }

    pub fn drop_cancellation(&self, session_id: u64) {
        crate::mutex_recover::lock(&self.cancelled_sessions).remove(&session_id);
    }
}

/// Занятая под скачивание модель, выданная [`AppState::try_claim_download`].
///
/// Пока страж жив, вторая загрузка этой же модели не начнётся.
pub struct DownloadGuard {
    state: AppState,
    model_id: String,
    flag: Arc<AtomicBool>,
}

impl DownloadGuard {
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.flag)
    }

    pub fn is_cancelled(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
}

impl Drop for DownloadGuard {
    fn drop(&mut self) {
        // Записи под этим именем всегда одна: `try_claim_download` не даёт
        // завести вторую, пока жив этот страж.
        crate::mutex_recover::lock(&self.state.download_cancels).remove(&self.model_id);
    }
}

/// Releases the registrations made by [`AppState::claim_file_session`].
pub struct FileSessionGuard {
    state: AppState,
    session_id: u64,
}

impl Drop for FileSessionGuard {
    fn drop(&mut self) {
        self.state.unskip_dispatch(self.session_id);
        self.state.clear_cancel_flag(self.session_id);
        crate::mutex_recover::lock(&self.state.active_sessions).remove(&self.session_id);
        crate::mutex_recover::lock(&self.state.committing_sessions).remove(&self.session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a minimal `Arc<AudioRecorder>` for unit tests that
    /// only need an AppState with the recorder slot populated (and never
    /// touch the audio path). Using `AudioConfig::default()` against a
    /// likely-missing headless device is fine — the lazy `default_input_*`
    /// probes inside `AudioRecorder::new` already fall back to a
    /// conservative buffer size without panicking.
    fn test_recorder() -> Arc<crate::audio::AudioRecorder> {
        Arc::new(
            crate::audio::AudioRecorder::new(crate::audio::AudioConfig::default())
                .expect("AudioRecorder::new should succeed even without a real device"),
        )
    }

    /// Helper: build a minimal `MicrophoneTest` for unit tests that
    /// only need the `microphone_test` slot populated. Uses the same
    /// `test_recorder()` to construct the inner test harness.
    fn test_microphone_test() -> crate::mic_test::MicrophoneTest {
        crate::mic_test::MicrophoneTest::new()
    }

    /// Helper: in-memory rusqlite Connection with the v1 schema applied.
    /// Used by every AppState test that needs to satisfy the `db` argument
    /// added in WS 4b. Schema is applied so callers can immediately use
    /// `stats_*` / `history_*` helpers against it.
    fn test_db() -> Arc<Mutex<rusqlite::Connection>> {
        let conn = rusqlite::Connection::open_in_memory()
            .expect("in-memory Connection::open_in_memory should succeed");
        crate::db::run_migrations(&conn).expect("run_migrations v1 should succeed");
        Arc::new(Mutex::new(conn))
    }

    #[test]
    fn session_ids_are_unique() {
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        let id1 = state.next_session_id();
        let id2 = state.next_session_id();
        assert_ne!(id1, id2);
        assert!(id1 < id2);
    }

    #[test]
    fn cancel_session_round_trip() {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel::<crate::whisper::EngineCommand>(1);
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            cmd_tx,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        let id = state.next_session_id();
        assert!(!state.is_cancelled(id));
        state.cancel_session(id);
        assert!(state.is_cancelled(id));
        state.drop_cancellation(id);
        assert!(!state.is_cancelled(id));
    }

    #[test]
    fn fsm_can_transition_idle_to_recording() {
        // Verifies the public AppFsm field is writable: Tauri commands
        // mutate `*state.app_fsm.lock().unwrap()` to drive UI state.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel::<crate::whisper::EngineCommand>(1);
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            cmd_tx,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        assert_eq!(*state.app_fsm.lock().unwrap(), AppFsm::Idle);
        *state.app_fsm.lock().unwrap() = AppFsm::Recording;
        assert_eq!(*state.app_fsm.lock().unwrap(), AppFsm::Recording);
    }

    #[test]
    fn fsm_clone_keeps_fsm_observable_across_clones() {
        // The Tauri command receives `tauri::State<'_, AppState>` (a
        // borrowed view), but the dispatcher holds a clone. Writes via
        // the original must be visible to the clone.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel::<crate::whisper::EngineCommand>(1);
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            cmd_tx,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        let clone = state.clone();
        *state.app_fsm.lock().unwrap() = AppFsm::Recording;
        assert_eq!(*clone.app_fsm.lock().unwrap(), AppFsm::Recording);
    }

    #[test]
    fn join_handle_stored_then_taken() {
        // Verifies the Task 10 plumbing: engine_thread field starts as
        // Some(handle) after construction, and can be taken via .take() for
        // graceful shutdown. We use a thread that returns immediately so the
        // JoinHandle resolves successfully when joined.
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        let taken = state.engine_thread.lock().unwrap().take();
        assert!(
            taken.is_some(),
            "engine_thread should be Some right after AppState::new"
        );
        let taken_again = state.engine_thread.lock().unwrap().take();
        assert!(
            taken_again.is_none(),
            "engine_thread should be None after take()"
        );
        taken
            .unwrap()
            .join()
            .expect("spawned thread should exit cleanly");
    }

    #[test]
    fn app_state_clone_shares_session_counter() {
        // WS 4a1 Task 13b: the hotkey closure captures an AppState clone.
        // session_id allocations MUST be coordinated across clones — a
        // hotkey-pressed session (allocated by the cloned state) and a
        // start_recording Tauri command session (allocated by the original
        // state) must not collide on the same id.
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        let clone = state.clone();

        let id_a = state.next_session_id();
        let id_b = clone.next_session_id();
        assert_ne!(id_a, id_b, "clones must share the atomic counter");
        assert_eq!(id_a, 1);
        assert_eq!(id_b, 2);
    }

    #[test]
    fn app_state_clone_shares_cancelled_sessions() {
        // Mirrored cancel_session on the clone must be visible to the
        // original (and vice versa) — cancellation is what the dispatcher
        // checks, and it's populated from one clone and read from another.
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        let clone = state.clone();
        let id = state.next_session_id();
        clone.cancel_session(id);
        assert!(state.is_cancelled(id));
        assert!(clone.is_cancelled(id));
    }

    #[test]
    fn cancellation_before_flag_registration_is_seen_by_the_engine() {
        let state = test_state();
        let id = state.next_session_id();
        state.begin_session(id);

        assert!(state.request_cancel(id));
        let flag = Arc::new(AtomicBool::new(false));
        state.register_cancel_flag(id, Arc::clone(&flag));

        assert!(flag.load(Ordering::Acquire));
        assert!(state.is_cancelled(id));
        state.finish_session(id);
        assert!(!state.is_cancelled(id));
    }

    #[test]
    fn cancellation_marker_survives_stop_finalization_until_terminal_cleanup() {
        let state = test_state();
        let id = state.next_session_id();
        state.begin_session(id);

        assert!(state.request_cancel(id));
        // This is the stop worker's terminal branch: it must still observe
        // the marker even though the cancel command ran before recorder.stop
        // completed, then own cleanup of the session registration.
        assert!(state.is_cancelled(id));
        state.finish_session(id);

        assert!(!state.is_session_active(id));
        assert!(!state.is_cancelled(id));
    }

    #[test]
    fn only_one_path_claims_the_live_session() {
        // `cancel_recording` and both stop paths race for the same slot.
        // Two winners means two owners of the terminal cleanup, and the one
        // that runs second reads a session the first already tore down.
        let state = test_state();
        let id = state.next_session_id();
        state.current_session_id.store(id, Ordering::Release);

        assert!(state.claim_live_session(id));
        assert!(!state.claim_live_session(id));
        assert_eq!(state.current_session_id.load(Ordering::Acquire), 0);
    }

    #[test]
    fn claiming_a_stale_session_puts_a_newer_one_back() {
        // A `start_recording` that published its id while the cancel was in
        // flight must survive: losing it strands the new recording with no
        // stop path able to find it.
        let state = test_state();
        let stale = state.next_session_id();
        let fresh = state.next_session_id();
        state.current_session_id.store(fresh, Ordering::Release);

        assert!(!state.claim_live_session(stale));
        assert_eq!(state.current_session_id.load(Ordering::Acquire), fresh);
    }

    #[test]
    fn commit_claim_wins_over_a_late_cancel() {
        let state = test_state();
        let id = state.next_session_id();
        state.begin_session(id);

        assert!(state.begin_commit(id));
        assert!(!state.request_cancel(id));
        assert!(!state.is_cancelled(id));

        state.finish_session(id);
        assert!(!state.is_session_active(id));
    }

    #[test]
    fn app_state_clone_shares_recorder() {
        // The recorder must be the SAME instance across clones (cloning the
        // Arc, not the recorder). Verifies that `AppState::new`'s third
        // arg propagates correctly through `derive(Clone)`.
        let handle = std::thread::spawn(|| {});
        let recorder = test_recorder();
        let recorder_ptr = Arc::as_ptr(&recorder);
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
            handle,
            recorder,
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );
        let clone = state.clone();
        assert_eq!(
            Arc::as_ptr(&state.recorder),
            Arc::as_ptr(&clone.recorder),
            "clones must share the same AudioRecorder Arc"
        );
        assert_eq!(
            Arc::as_ptr(&clone.recorder),
            recorder_ptr,
            "clone's recorder must be the same Arc we constructed"
        );
    }

    #[test]
    fn cancel_recording_swap_and_restore_preserves_concurrent_session() {
        // WS 4a2b Task 5: cancel_recording does a swap-and-restore dance
        // on current_session_id to avoid a TOCTOU race with a concurrent
        // start_recording that bumps the counter between our read and our
        // write. This test simulates that race:
        //
        //   1. session A (42) is in flight
        //   2. concurrently, a new session B (43) starts (counter bumped)
        //   3. cancel_recording(42) runs, swap(0) returns 43 (not 42)
        //   4. we restore 43 so B is not orphaned
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::channel::<crate::whisper::EngineCommand>(1);
        let handle = std::thread::spawn(|| {});
        let state = AppState::new(
            cmd_tx,
            handle,
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        );

        // Step 1: session A allocated.
        let id_a = state.next_session_id();
        assert_eq!(id_a, 1);
        state.current_session_id.store(id_a, Ordering::Release);
        // Step 2: race — concurrent start_recording bumps the counter to 2
        // and overwrites current_session_id with the new id.
        let id_b = state.next_session_id();
        assert_eq!(id_b, 2);
        state.current_session_id.store(id_b, Ordering::Release);

        // Step 3: cancel_recording(id_a) does swap(0) — returns the
        // LATEST value (id_b), not id_a.
        let prev = state.current_session_id.swap(0, Ordering::AcqRel);
        assert_eq!(
            prev, id_b,
            "swap returns the latest written value, not id_a"
        );
        // Step 4: cancel_recording detects the mismatch and restores id_b
        // so the next stop_recording / hotkey release can pair with it.
        if prev != id_a && prev != 0 {
            state.current_session_id.store(prev, Ordering::Release);
        }
        assert_eq!(
            state.current_session_id.load(Ordering::Acquire),
            id_b,
            "concurrent session id_b must be restored, not orphaned"
        );

        // Sanity: a clean cancel-recording for the current session does
        // NOT restore (prev == session_id, so the restore branch is skipped).
        let prev = state.current_session_id.swap(0, Ordering::AcqRel);
        assert_eq!(prev, id_b);
        if prev != id_b && prev != 0 {
            state.current_session_id.store(prev, Ordering::Release);
        }
        assert_eq!(
            state.current_session_id.load(Ordering::Acquire),
            0,
            "matching cancel leaves counter at 0"
        );
    }

    fn test_state() -> AppState {
        AppState::new(
            tokio::sync::mpsc::channel(1).0,
            std::thread::spawn(|| {}),
            test_recorder(),
            test_db(),
            test_microphone_test(),
            Arc::new(Mutex::new(None)),
        )
    }

    #[test]
    fn cancelling_a_download_reaches_the_flag_the_downloader_is_watching() {
        let state = test_state();
        let download = state.try_claim_download("parakeet-tdt-v3").unwrap();

        assert!(
            state.cancel_download("parakeet-tdt-v3"),
            "загрузка зарегистрирована, отмена обязана её найти"
        );
        assert!(
            download.is_cancelled(),
            "у скачивателя на руках тот же флаг — иначе он не остановится"
        );
    }

    #[test]
    fn a_finished_download_leaves_nothing_to_cancel() {
        let state = test_state();
        drop(state.try_claim_download("gigaam-v3").unwrap());

        assert!(
            !state.cancel_download("gigaam-v3"),
            "кнопка, нажатая после конца загрузки, — гонка, а не отмена"
        );
        assert!(
            !state.cancel_download("никогда-не-качалась"),
            "чужой идентификатор тоже ничего не отменяет"
        );
    }

    #[test]
    fn the_same_model_is_never_downloaded_twice_at_once() {
        // Две загрузки одной модели писали бы один и тот же `*.part`,
        // наперегонки проверяли бы его сумму и переименовывали бы
        // результат, а отмена доставалась бы только последней.
        let state = test_state();
        let first = state.try_claim_download("turbo").unwrap();

        assert!(
            state.try_claim_download("turbo").is_none(),
            "вторая заявка на ту же модель отклоняется"
        );
        // Другая модель при этом не заперта: качать их одновременно можно.
        assert!(state.try_claim_download("tiny").is_some());

        drop(first);
        assert!(
            state.try_claim_download("turbo").is_some(),
            "после окончания первой загрузки модель снова свободна"
        );
    }

    #[test]
    fn dispatch_skip_survives_the_started_check_and_clears_on_the_completed_one() {
        // The dispatcher sees two events per session. `InferenceStarted`
        // must be able to ask without consuming the entry, or
        // `InferenceCompleted` walks straight into the paste-and-record
        // path it was registered to avoid.
        let state = test_state();
        state.skip_dispatch(7);

        assert!(state.is_dispatch_skipped(7), "the started-branch check");
        assert!(
            state.is_dispatch_skipped(7),
            "asking twice must not consume the entry — the started-branch \
             fires once per session but nothing guarantees the ordering"
        );

        assert!(
            crate::mutex_recover::lock(&state.dispatch_skipped).remove(&7),
            "the completed-branch must find the entry still there"
        );
        assert!(
            !crate::mutex_recover::lock(&state.dispatch_skipped).remove(&7),
            "and must not find it a second time: session ids are reused, so a \
             leftover entry silently swallows a later dictation"
        );
    }

    #[test]
    fn unskip_dispatch_releases_an_id_the_engine_never_got() {
        // The failure this guards: the command inserts the id, the send to
        // the engine fails, the entry stays. `stop_recording` resets the id
        // counter to 0, so that id is handed out again — to a dictation the
        // dispatcher then drops with no paste and no error.
        let state = test_state();
        state.skip_dispatch(1);
        state.unskip_dispatch(1);

        assert!(
            !state.is_dispatch_skipped(1),
            "a rolled-back registration must leave nothing behind"
        );
    }

    #[test]
    fn skipping_one_session_leaves_others_dispatchable() {
        let state = test_state();
        state.skip_dispatch(3);

        assert!(state.is_dispatch_skipped(3));
        assert!(
            !state.is_dispatch_skipped(4),
            "a file session must not suppress an unrelated dictation"
        );
    }

    #[test]
    fn claim_engine_is_exclusive_and_released_on_drop() {
        let state = test_state();

        let guard = state.claim_engine().expect("first claim must succeed");
        assert!(
            state.is_engine_busy(),
            "the claim must be visible to callers"
        );
        assert!(
            state.claim_engine().is_none(),
            "a second job must be refused while the first holds the engine"
        );

        drop(guard);
        assert!(
            !state.is_engine_busy(),
            "dropping the guard must free the engine — this is what makes \
             every early return and `?` in the command safe"
        );
        assert!(
            state.claim_engine().is_some(),
            "and the next job must be able to claim it"
        );
    }

    #[test]
    fn claim_engine_is_shared_across_clones() {
        // The hotkey handler holds a clone, and it is the caller that has to
        // see the flag a file transcription set on the original.
        let state = test_state();
        let clone = state.clone();

        let _guard = state.claim_engine().expect("first claim must succeed");

        assert!(clone.is_engine_busy(), "clones must observe the same flag");
        assert!(
            clone.claim_engine().is_none(),
            "a clone must not be able to claim an engine that is already busy"
        );
    }
    #[test]
    fn file_session_registrations_clear_on_drop() {
        // The skip-set entry is the dangerous one: ids restart from zero
        // after every dictation, so one left behind eats a later dictation's
        // result with no paste and no error.
        let state = test_state();
        let flag = Arc::new(AtomicBool::new(false));

        let guard = state.claim_file_session(9, Arc::clone(&flag));
        assert!(state.is_dispatch_skipped(9), "the dispatcher must skip it");
        state.cancel_session(9);
        assert!(
            flag.load(Ordering::Acquire),
            "the cancel flag must be reachable while the session is claimed"
        );

        drop(guard);

        assert!(
            !state.is_dispatch_skipped(9),
            "a leaked skip entry silently swallows a later dictation"
        );
        // Reset and cancel again: if the flag were still registered under
        // this id it would flip a second time, which is how a cancel meant
        // for a new session reaches the previous session's flag.
        flag.store(false, Ordering::Release);
        state.cancel_session(9);
        assert!(
            !flag.load(Ordering::Acquire),
            "the old flag must no longer be registered under this id"
        );
    }
}
