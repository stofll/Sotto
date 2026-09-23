//! Recording state machine and the AppState container.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Application-level state machine (visible to UI via events).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppFsm {
    Idle,
    Recording,
    Processing,
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
/// by setup()-scope then moved into the dispatcher task) and session-id
/// tracking.
///
/// `AppState` is `Clone` (WS 4a1 Task 13b) so the hotkey handler can
/// capture it into a `'static + Send` closure. Every non-Clone field is
/// wrapped in `Arc`, so the clone is just a handful of refcount bumps.
/// All clones share the same underlying state — `next_session_id` and
/// `cancel_session` calls are coordinated across every clone.
#[derive(Clone)]
pub struct AppState {
    pub app_fsm: Arc<Mutex<AppFsm>>,
    pub engine_cmd_tx: tokio::sync::mpsc::Sender<crate::whisper::EngineCommand>,
    pub current_session_id: Arc<AtomicU64>,
    session_counter: Arc<AtomicU64>,
    dictation_session_id: Arc<AtomicU64>,
    pub(crate) capture_commands: Arc<Mutex<()>>,
    cancel_notify: Arc<tokio::sync::Notify>,
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
    /// The dispatcher also checks dictation ownership so a file guard can
    /// retire its marker before a queued engine event is consumed.
    pub dispatch_skipped: Arc<Mutex<HashSet<u64>>>,
    /// True while file transcription or a dictation through delivery owns the engine.
    ///
    /// Separate from `AppFsm` on purpose: the FSM describes a dictation's
    /// lifecycle and is returned to `Idle` by the dispatcher, which a file
    /// session deliberately bypasses. Recording commands refuse while this
    /// is set — the engine runs one job at a time, and a dictation queued
    /// behind an hour-long file would look frozen rather than rejected.
    pub engine_busy: Arc<AtomicBool>,
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

    /// Shared SQLite connection for statistics and history. Blocking jobs
    /// receive an `Arc` clone and acquire/release the standard mutex guard
    /// inside the job; that guard is not `Send` and never crosses an await.
    pub db: Arc<Mutex<rusqlite::Connection>>,

    /// Microphone self-test with its own `AudioRecorder`, separate from
    /// dictation capture. Device operations use the shared audio worker.
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

    /// Download cancellation flags, keyed by model identifier.
    ///
    /// Separate from `cancel_flags`: those live per recognition session and are
    /// numbered, while a download is identified by its model — what must be
    /// cancelled is the one being downloaded right now, and the only way the
    /// user knows it is by name.
    pub download_cancels: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
}

impl AppState {
    pub fn new(
        cmd_tx: tokio::sync::mpsc::Sender<crate::whisper::EngineCommand>,
        recorder: Arc<crate::audio::AudioRecorder>,
        db: Arc<Mutex<rusqlite::Connection>>,
        microphone_test: crate::mic_test::MicrophoneTest,
        engine_current_model: Arc<Mutex<Option<String>>>,
    ) -> Self {
        Self {
            app_fsm: Arc::new(Mutex::new(AppFsm::Idle)),
            engine_cmd_tx: cmd_tx,
            current_session_id: Arc::new(AtomicU64::new(0)),
            session_counter: Arc::new(AtomicU64::new(0)),
            dictation_session_id: Arc::new(AtomicU64::new(0)),
            capture_commands: Arc::new(Mutex::new(())),
            cancel_notify: Arc::new(tokio::sync::Notify::new()),
            active_sessions: Arc::new(Mutex::new(HashSet::new())),
            committing_sessions: Arc::new(Mutex::new(HashSet::new())),
            cancelled_sessions: Arc::new(Mutex::new(HashSet::new())),
            cancel_flags: Arc::new(Mutex::new(HashMap::new())),
            dispatch_skipped: Arc::new(Mutex::new(HashSet::new())),
            engine_busy: Arc::new(AtomicBool::new(false)),
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

    /// Claim a model for downloading and receive a guard that releases it on any
    /// exit — including `?` and a panic.
    ///
    /// `None` — this model is already being downloaded. The check and the write
    /// happen under one lock: two downloads of the same model would write the
    /// same `*.part`, race each other checking its checksum and renaming the
    /// result, while a cancel would reach only the last one.
    ///
    /// The flag is registered before the first byte: otherwise a cancel pressed
    /// within the first second finds nothing to cancel and silently does
    /// nothing.
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

    /// Ask a download to stop. `false` — there is no such download.
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

    /// Register a session the dispatcher must ignore. Call this BEFORE
    /// queueing the engine command: the engine can finish and emit before
    /// a later insert lands, and a completion that slips past the check is
    /// pasted into whatever window happens to be focused.
    pub fn skip_dispatch(&self, session_id: u64) {
        crate::mutex_recover::lock(&self.dispatch_skipped).insert(session_id);
    }

    /// Retire a file dispatch marker on early exit or after its reply.
    /// The dispatcher also checks dictation ownership if completion arrives later.
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

    pub fn next_session_id(&self) -> u64 {
        self.session_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Reserve capture and delivery together, before queueing any native work.
    /// File jobs use the same flag, so neither entry point can overtake the other.
    pub fn try_begin_dictation(&self) -> Option<u64> {
        self.engine_busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;
        let id = self.next_session_id();
        self.begin_session(id);
        self.dictation_session_id.store(id, Ordering::Release);
        self.current_session_id.store(id, Ordering::Release);
        Some(id)
    }

    pub fn owns_dictation(&self, session_id: u64) -> bool {
        session_id != 0 && self.dictation_session_id.load(Ordering::Acquire) == session_id
    }

    pub fn dictation_id(&self) -> u64 {
        self.dictation_session_id.load(Ordering::Acquire)
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
    /// Compare-and-exchange leaves a newer session's slot untouched.
    pub fn claim_live_session(&self, session_id: u64) -> bool {
        session_id != 0
            && self
                .current_session_id
                .compare_exchange(session_id, 0, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
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
        let mut active = crate::mutex_recover::lock(&self.active_sessions);
        active.remove(&session_id);
        crate::mutex_recover::lock(&self.committing_sessions).remove(&session_id);
        self.drop_cancellation(session_id);
        self.clear_cancel_flag(session_id);
        if self.owns_dictation(session_id) {
            self.claim_live_session(session_id);
            self.toggle_armed.store(false, Ordering::Release);
            set_app_fsm(&self.app_fsm, AppFsm::Idle);
            self.dictation_session_id.store(0, Ordering::Release);
            // Publish availability last: an old completion must never reset a
            // newer recording's state or release a file job's engine claim.
            self.engine_busy.store(false, Ordering::Release);
        }
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
        self.cancel_notify.notify_waiters();
        true
    }

    pub async fn wait_cancelled(&self, session_id: u64) {
        loop {
            let notified = self.cancel_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.is_cancelled(session_id) {
                return;
            }
            notified.await;
        }
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

    /// Register a file session through STT and post-processing.
    ///
    /// IDs increase monotonically within this `AppState`. The guard bounds
    /// active-session, cancel-flag and dispatch-skip registrations on early
    /// exits and through optional post-processing, independently of engine ownership.
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

    /// Register the engine's cancel flag before queueing transcription.
    /// Preserve cancellation requested before the flag was available.
    pub fn register_cancel_flag(&self, session_id: u64, flag: Arc<AtomicBool>) {
        let cancelled = crate::mutex_recover::lock(&self.cancelled_sessions);
        if cancelled.contains(&session_id) {
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

/// A model claimed for download, handed out by [`AppState::try_claim_download`].
///
/// While the guard is alive, a second download of the same model cannot start.
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
        // There is always exactly one entry under this name:
        // `try_claim_download` will not create a second while this guard lives.
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
        self.state.finish_session(self.session_id);
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
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
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
        let state = AppState::new(
            cmd_tx,
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
        let state = AppState::new(
            cmd_tx,
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
        let state = AppState::new(
            cmd_tx,
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
    fn app_state_clone_shares_session_counter() {
        // WS 4a1 Task 13b: the hotkey closure captures an AppState clone.
        // session_id allocations MUST be coordinated across clones — a
        // hotkey-pressed session (allocated by the cloned state) and a
        // start_recording Tauri command session (allocated by the original
        // state) must not collide on the same id.
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
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
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
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
        let recorder = test_recorder();
        let recorder_ptr = Arc::as_ptr(&recorder);
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
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
    fn a_stale_cancel_never_takes_the_current_capture_slot() {
        let state = test_state();
        let old = state.next_session_id();
        let current = state.next_session_id();
        state.current_session_id.store(current, Ordering::Release);
        assert!(!state.claim_live_session(old));
        assert_eq!(state.current_session_id.load(Ordering::Acquire), current);
        assert!(state.claim_live_session(current));
        assert!(!state.claim_live_session(current));
        assert!(!state.claim_live_session(0));
    }

    #[test]
    fn capture_stop_does_not_release_delivery_or_reuse_ids() {
        let state = test_state();
        let first = state.try_begin_dictation().unwrap();
        assert!(state.claim_live_session(first));
        assert!(state.try_begin_dictation().is_none());
        assert!(state.claim_engine().is_none());
        assert!(state.begin_commit(first));
        assert!(!state.request_cancel(first));
        state.finish_session(first);
        let second = state.try_begin_dictation().unwrap();
        assert!(second > first);
        state.finish_session(first);
        assert!(state.is_session_active(second));
        assert!(state.is_engine_busy());
        assert!(!state.request_cancel(first));
        assert!(!state.claim_live_session(first));
        state.finish_session(second);
        assert!(state.claim_engine().is_some());
    }

    #[test]
    fn a_file_and_a_capture_cannot_claim_the_engine_together() {
        let state = test_state();
        let file = state.claim_engine().unwrap();
        assert!(state.try_begin_dictation().is_none());
        drop(file);
        let capture = state.try_begin_dictation().unwrap();
        assert!(state.claim_engine().is_none());
        state.finish_session(capture);
    }

    #[test]
    fn simultaneous_starts_accept_exactly_one_session() {
        let state = test_state();
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let state = state.clone();
            let barrier = barrier.clone();
            threads.push(std::thread::spawn(move || {
                barrier.wait();
                state.try_begin_dictation()
            }));
        }
        barrier.wait();
        let ids: Vec<_> = threads
            .into_iter()
            .filter_map(|t| t.join().unwrap())
            .collect();
        assert_eq!(ids.len(), 1);
        state.finish_session(ids[0]);
        assert!(state.try_begin_dictation().is_some());
    }

    #[tokio::test]
    async fn cancellation_wakes_post_processing_and_prevents_commit() {
        let state = test_state();
        let id = state.try_begin_dictation().unwrap();
        assert!(state.claim_live_session(id));
        let cancel = state.wait_cancelled(id);
        assert!(state.request_cancel(id));
        tokio::select! {
            _ = cancel => {},
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => panic!("missed cancellation"),
        }
        assert!(!state.begin_commit(id));
        state.finish_session(id);
        assert!(state.try_begin_dictation().is_some());
    }

    fn test_state() -> AppState {
        AppState::new(
            tokio::sync::mpsc::channel(1).0,
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
            "the download is registered, so a cancel must find it"
        );
        assert!(
            download.is_cancelled(),
            "the downloader holds the same flag; without it, it would not stop"
        );
    }

    #[test]
    fn a_finished_download_leaves_nothing_to_cancel() {
        let state = test_state();
        drop(state.try_claim_download("gigaam-v3").unwrap());

        assert!(
            !state.cancel_download("gigaam-v3"),
            "a button pressed after the download ended is a race, not a cancel"
        );
        assert!(
            !state.cancel_download("никогда-не-качалась"),
            "an unknown id cancels nothing either"
        );
    }

    #[test]
    fn the_same_model_is_never_downloaded_twice_at_once() {
        // Two downloads of one model would write the same `*.part`, race each
        // other checking its checksum and renaming the result, while a cancel
        // would reach only the last one.
        let state = test_state();
        let first = state.try_claim_download("turbo").unwrap();

        assert!(
            state.try_claim_download("turbo").is_none(),
            "a second claim on the same model is refused"
        );
        // A different model is not locked out: downloading them at once is fine.
        assert!(state.try_claim_download("tiny").is_some());

        drop(first);
        assert!(
            state.try_claim_download("turbo").is_some(),
            "the model is free again once the first download ends"
        );
    }

    #[tokio::test]
    async fn file_post_processing_stays_cancellable_after_engine_release() {
        let state = test_state();
        let engine = state.claim_engine().unwrap();
        let id = state.next_session_id();
        let guard = state.claim_file_session(id, Arc::new(AtomicBool::new(false)));
        drop(engine);
        let next_engine = state
            .claim_engine()
            .expect("post-processing must not hold the engine");
        assert!(state.is_dispatch_skipped(id));
        assert!(state.request_cancel(id));
        tokio::time::timeout(std::time::Duration::from_secs(1), state.wait_cancelled(id))
            .await
            .unwrap();
        assert!(!state.begin_commit(id));
        drop(guard);
        assert!(!state.is_dispatch_skipped(id));
        assert!(!state.is_cancelled(id));
        assert!(!state.is_session_active(id));
        assert!(!state.request_cancel(id));
        assert!(
            state.is_engine_busy(),
            "file cleanup must not release another job's claim"
        );
        drop(next_engine);
    }

    #[test]
    fn unskip_dispatch_releases_an_id_the_engine_never_got() {
        // Failed queue submission must not leak a dispatch marker.
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
        // Every per-session registration must be released on early returns as
        // well as success; session ids themselves are never reused.
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
            "the file registration must retire with its guard"
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
