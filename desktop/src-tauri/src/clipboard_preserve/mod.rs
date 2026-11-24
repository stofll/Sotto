//! «Не изменять буфер обмена» (`preserve_clipboard`): an automatic paste that
//! leaves the user's clipboard as it found it.
//!
//! The paste itself still goes through the clipboard: every target accepts
//! Ctrl/Cmd+V, and there is no portable way to type text into another
//! application. So the previous contents are captured, the text is put on the
//! clipboard for the keystroke, and the capture is put back afterwards.
//!
//! Every step runs inside one clipboard [`Session`]: the check that the
//! clipboard still holds what Sotto put there and the write it guards happen
//! together, so a copy made by another application in between is never
//! overwritten with older contents.
//!
//! The restore waits [`RESTORE_DELAY`]: the keystroke is queued input, and the
//! target reads the clipboard only when it gets to it. It runs through
//! `run_delivery`, so it is ordered with later pastes. A paste that arrives
//! while a restore is still pending reuses that capture: by then the
//! clipboard holds the previous dictation, not the user's data.
//!
//! Failures never pass for success. A capture that cannot read the contents
//! stops the paste before the clipboard is touched; a restore that does not
//! complete keeps the capture, notes the state its own attempt left behind,
//! and is retried.
//!
//! What a capture can hold differs per platform; `docs/privacy.md` lists it.

use std::time::Duration;

use tauri::AppHandle;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(any(not(any(windows, target_os = "macos")), test))]
mod portable;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "macos")]
use macos as native;
#[cfg(not(any(windows, target_os = "macos")))]
use portable as native;
#[cfg(windows)]
use windows as native;

/// How long the dictated text stays on the clipboard after the keystroke.
/// Long enough for a busy Electron or browser window to handle the queued
/// paste; shorter risks the target reading the restored contents instead.
const RESTORE_DELAY: Duration = Duration::from_millis(500);

/// A restore that keeps failing is given up after this many attempts. The
/// capture stays pending, so the next preserving paste still brings the
/// user's contents back instead of capturing the dictated text as theirs.
const MAX_RESTORE_ATTEMPTS: u32 = 3;

/// The clipboard, held for a sequence of operations that must not be split
/// by another application's write.
trait Session {
    /// Everything a restore needs to put back.
    type Snapshot;
    /// Identifies the clipboard's current contents. Read in the session that
    /// wrote them, it compares equal later only while nobody else has
    /// written since; every write, ours included, produces a new one.
    type Stamp: PartialEq;
    fn stamp(&self) -> Self::Stamp;
    /// The current contents. Fails, without changing anything, when a
    /// supported part of them cannot be read: an empty clipboard and formats
    /// a platform deliberately leaves out are not failures.
    fn capture(&self) -> Result<Self::Snapshot, String>;
    /// Replace the contents with `text`, unless they changed since
    /// `expected`. Returns the stamp of what was written.
    fn replace_with_text(
        &mut self,
        text: &str,
        expected: &Self::Stamp,
    ) -> Result<Self::Stamp, WriteError<Self::Stamp>>;
    /// Replace the contents with `snapshot`, unless they changed since
    /// `expected`. Fails when any part of it could not be written; the
    /// clipboard may then hold part of it.
    fn replace_with(
        &mut self,
        snapshot: &Self::Snapshot,
        expected: &Self::Stamp,
    ) -> Result<Self::Stamp, WriteError<Self::Stamp>>;
}

/// Why a write did not go through.
///
/// `expected` is checked immediately before the clipboard is cleared, after
/// everything to be written has been prepared. A backend that holds the
/// clipboard for the whole session (Windows) cannot see a change and ignores
/// it; one that cannot hold it (macOS) narrows the window to that moment.
#[derive(Debug)]
enum WriteError<T> {
    /// Someone else wrote since `expected`; nothing was changed. Only the
    /// macOS backend produces it: Windows holds the clipboard instead, and
    /// the portable backend cannot see a change.
    #[cfg_attr(all(not(target_os = "macos"), not(test)), allow(dead_code))]
    Changed,
    Failed {
        error: String,
        /// The state the failed write left behind, which Sotto may still
        /// write over; `None` when the clipboard was not changed.
        left: Option<T>,
    },
}

trait Store {
    type Session: Session;
    /// Fails when another application holds the clipboard for too long.
    fn open(&self) -> Result<Self::Session, String>;
}

type SnapshotOf<B> = <<B as Store>::Session as Session>::Snapshot;
type StampOf<B> = <<B as Store>::Session as Session>::Stamp;

struct Pending<S, T> {
    generation: u64,
    snapshot: S,
    /// The clipboard as Sotto left it. A restore may write only over this.
    stamp: T,
}

#[derive(Debug, PartialEq, Eq)]
enum Restore {
    Restored,
    /// A later paste took the capture over; its own restore will run.
    Superseded,
    /// Someone put something new on the clipboard; it stays.
    ChangedByUser,
    /// The capture is still pending; the restore can be tried again.
    Failed(String),
}

/// Why a preserving paste did not go ahead.
#[derive(Debug)]
struct BeginError {
    error: String,
    /// Set when the clipboard was left changed and could not be put back in
    /// the same session: the restore of this generation is due.
    retry: Option<u64>,
}

/// The capture waiting for its restore, at most one at a time.
struct Preserver<S, T> {
    generation: u64,
    pending: Option<Pending<S, T>>,
}

impl<S, T: PartialEq> Preserver<S, T> {
    const fn new() -> Self {
        Self {
            generation: 0,
            pending: None,
        }
    }

    fn park(&mut self, snapshot: S, stamp: T) -> u64 {
        self.generation += 1;
        self.pending = Some(Pending {
            generation: self.generation,
            snapshot,
            stamp,
        });
        self.generation
    }

    /// Put `text` on the clipboard, keeping what it replaced. Returns the
    /// generation whose restore is then due.
    fn begin<B>(&mut self, store: &B, text: &str) -> Result<u64, BeginError>
    where
        B: Store,
        B::Session: Session<Snapshot = S, Stamp = T>,
    {
        let mut session = store
            .open()
            .map_err(|error| BeginError { error, retry: None })?;
        let current = session.stamp();
        // Still the previous dictation: its capture is the user's data. It
        // stays pending, with its own restore due, until this write has
        // actually replaced the clipboard.
        let reuse = self
            .pending
            .as_ref()
            .is_some_and(|pending| pending.stamp == current);
        let fresh = if reuse {
            None
        } else {
            match session.capture() {
                Ok(snapshot) => Some(snapshot),
                Err(error) => return Err(BeginError { error, retry: None }),
            }
        };
        match session.replace_with_text(text, &current) {
            Ok(stamp) => {
                let snapshot = self.original(fresh);
                Ok(self.park(snapshot, stamp))
            }
            // Someone copied after the capture: that copy is the user's data
            // now, and nothing was written over it.
            Err(WriteError::Changed) => Err(BeginError {
                error: "the clipboard changed while the paste was prepared".into(),
                retry: None,
            }),
            Err(WriteError::Failed { error, left: None }) => Err(BeginError { error, retry: None }),
            // The write changed the clipboard. Put the contents back while it
            // is still held; only if that fails too is the capture left for a
            // later restore.
            Err(WriteError::Failed {
                error,
                left: Some(left),
            }) => {
                let snapshot = self.original(fresh);
                match session.replace_with(&snapshot, &left) {
                    Ok(_) | Err(WriteError::Changed) => Err(BeginError { error, retry: None }),
                    Err(WriteError::Failed {
                        error: restore,
                        left: after,
                    }) => Err(BeginError {
                        error: format!("{error}; restoring the clipboard failed: {restore}"),
                        retry: Some(self.park(snapshot, after.unwrap_or(left))),
                    }),
                }
            }
        }
    }

    /// The user's contents a write has just replaced: the fresh capture, or
    /// the pending one it reused, which a new restore now takes over.
    fn original(&mut self, fresh: Option<S>) -> S {
        match fresh {
            Some(snapshot) => snapshot,
            None => {
                self.pending
                    .take()
                    .expect("a reused capture is still pending")
                    .snapshot
            }
        }
    }

    fn restore<B>(&mut self, generation: u64, store: &B) -> Restore
    where
        B: Store,
        B::Session: Session<Snapshot = S, Stamp = T>,
    {
        let pending = match self.pending.take() {
            Some(pending) if pending.generation == generation => pending,
            other => {
                self.pending = other;
                return Restore::Superseded;
            }
        };
        let mut session = match store.open() {
            Ok(session) => session,
            Err(error) => {
                self.pending = Some(pending);
                return Restore::Failed(error);
            }
        };
        if session.stamp() != pending.stamp {
            return Restore::ChangedByUser;
        }
        match session.replace_with(&pending.snapshot, &pending.stamp) {
            Ok(_) => Restore::Restored,
            Err(WriteError::Changed) => Restore::ChangedByUser,
            Err(WriteError::Failed { error, left }) => {
                // A failed attempt that changed the clipboard left a state
                // that is still ours to write over on the next try.
                self.pending = Some(match left {
                    Some(stamp) => Pending { stamp, ..pending },
                    None => pending,
                });
                Restore::Failed(error)
            }
        }
    }
}

type NativePreserver = Preserver<SnapshotOf<native::Clipboard>, StampOf<native::Clipboard>>;

static STATE: std::sync::Mutex<NativePreserver> = std::sync::Mutex::new(Preserver::new());

/// Run `paste` with `text` on the clipboard, then put the user's contents
/// back. Runs inside `run_delivery`, like `clipboard::paste_text`.
///
/// Nothing is pasted when the contents cannot be captured: pasting anyway
/// would lose exactly what the setting promises to keep.
///
/// `restore_after_failure`: whether the contents come back when `paste`
/// fails. Off when the text has no other copy: it is then left on the
/// clipboard to paste by hand. The capture stays pending, so the next
/// preserving paste over that text still puts the user's contents back.
pub fn paste_preserving(
    app: &AppHandle,
    text: &str,
    restore_after_failure: bool,
    paste: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let store = native::Clipboard::new(app);
    let begun = crate::mutex_recover::lock(&STATE).begin(&store, text);
    let generation = match begun {
        Ok(generation) => generation,
        Err(BeginError { error, retry }) => {
            if let Some(generation) = retry {
                schedule_restore(app, generation, 1);
            }
            return Err(format!(
                "could not prepare the clipboard, nothing pasted: {error}"
            ));
        }
    };
    // Dropped on every way out of `paste`, a panic included.
    let mut restore = ScheduledRestore {
        app: app.clone(),
        generation,
        armed: true,
    };
    let result = paste();
    if result.is_err() && !restore_after_failure {
        restore.armed = false;
    }
    result
}

struct ScheduledRestore {
    app: AppHandle,
    generation: u64,
    armed: bool,
}

impl Drop for ScheduledRestore {
    fn drop(&mut self) {
        if self.armed {
            schedule_restore(&self.app, self.generation, 1);
        }
    }
}

fn schedule_restore(app: &AppHandle, generation: u64, attempt: u32) {
    let thread_app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("clipboard-restore".into())
        .spawn(move || {
            std::thread::sleep(RESTORE_DELAY);
            let job_app = thread_app.clone();
            if let Err(error) = crate::clipboard::run_delivery(&thread_app, move || {
                restore_now(&job_app, generation, attempt)
            }) {
                log::warn!("clipboard restore not scheduled: {error}");
            }
        });
    if spawned.is_err() {
        // Early is better than never: the user's contents would be lost. This
        // runs on the delivery thread like the scheduled attempt would: it is
        // reached from a delivery job, the first time from the paste and
        // later from the failed attempt. No further retry is scheduled.
        restore_now(app, generation, MAX_RESTORE_ATTEMPTS);
    }
}

fn restore_now(app: &AppHandle, generation: u64, attempt: u32) {
    let store = native::Clipboard::new(app);
    let outcome = crate::mutex_recover::lock(&STATE).restore(generation, &store);
    match outcome {
        Restore::Restored | Restore::Superseded => {}
        Restore::ChangedByUser => {
            log::info!("clipboard changed after the paste; not restoring older contents")
        }
        Restore::Failed(error) if attempt < MAX_RESTORE_ATTEMPTS => {
            log::warn!("clipboard restore attempt {attempt} failed, retrying: {error}");
            schedule_restore(app, generation, attempt + 1);
        }
        Restore::Failed(error) => log::error!(
            "clipboard restore failed after {attempt} attempts; the next preserving paste retries it: {error}"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A clipboard holding one string, with a change counter like the
    /// Windows sequence number and the macOS change count.
    #[derive(Default)]
    struct State {
        contents: Option<String>,
        changes: u32,
        /// Another application copying this while we wait for access.
        copied_while_opening: Option<String>,
        /// Another application copying this right after the capture, before
        /// the clipboard is cleared (no lock on macOS).
        copied_after_capture: Option<String>,
        /// Another application copying this while a restore prepares its
        /// items, after the ownership check.
        copied_before_restore: Option<String>,
        busy: bool,
        fail_capture: bool,
        /// A write that fails before touching the clipboard, as running out
        /// of memory while preparing the blocks does on Windows.
        fail_prepare: bool,
        fail_write: bool,
        /// Restores that fail after writing part of the snapshot.
        failing_restores: u32,
    }

    impl State {
        fn set(&mut self, contents: Option<String>) {
            self.contents = contents;
            self.changes += 1;
        }
    }

    #[derive(Clone, Default)]
    struct Fake(Rc<RefCell<State>>);

    impl Fake {
        fn holding(text: &str) -> Self {
            let fake = Self::default();
            fake.user_copies(text);
            fake
        }
        fn user_copies(&self, text: &str) {
            self.0.borrow_mut().set(Some(text.to_string()));
        }
        fn contents(&self) -> Option<String> {
            self.0.borrow().contents.clone()
        }
        fn with(&self, change: impl FnOnce(&mut State)) {
            change(&mut self.0.borrow_mut());
        }
    }

    impl Store for Fake {
        type Session = Fake;
        fn open(&self) -> Result<Fake, String> {
            let mut state = self.0.borrow_mut();
            if state.busy {
                return Err("busy".into());
            }
            if let Some(text) = state.copied_while_opening.take() {
                state.set(Some(text));
            }
            Ok(self.clone())
        }
    }

    impl Session for Fake {
        type Snapshot = Option<String>;
        type Stamp = u32;
        fn stamp(&self) -> u32 {
            self.0.borrow().changes
        }
        fn capture(&self) -> Result<Option<String>, String> {
            let mut state = self.0.borrow_mut();
            if state.fail_capture {
                return Err("unreadable format".into());
            }
            let contents = state.contents.clone();
            if let Some(text) = state.copied_after_capture.take() {
                state.set(Some(text));
            }
            Ok(contents)
        }
        fn replace_with_text(
            &mut self,
            text: &str,
            expected: &u32,
        ) -> Result<u32, WriteError<u32>> {
            let mut state = self.0.borrow_mut();
            if state.changes != *expected {
                return Err(WriteError::Changed);
            }
            if state.fail_prepare {
                return Err(WriteError::Failed {
                    error: "out of memory".into(),
                    left: None,
                });
            }
            // Emptied first, as the native backends do.
            state.set(None);
            if state.fail_write {
                return Err(WriteError::Failed {
                    error: "write failed".into(),
                    left: Some(state.changes),
                });
            }
            state.set(Some(text.to_string()));
            Ok(state.changes)
        }
        fn replace_with(
            &mut self,
            snapshot: &Option<String>,
            expected: &u32,
        ) -> Result<u32, WriteError<u32>> {
            let mut state = self.0.borrow_mut();
            if let Some(text) = state.copied_before_restore.take() {
                state.set(Some(text));
            }
            if state.changes != *expected {
                return Err(WriteError::Changed);
            }
            if state.failing_restores > 0 {
                state.failing_restores -= 1;
                state.set(Some("частично".into()));
                return Err(WriteError::Failed {
                    error: "format not restored".into(),
                    left: Some(state.changes),
                });
            }
            state.set(snapshot.clone());
            Ok(state.changes)
        }
    }

    type FakePreserver = Preserver<Option<String>, u32>;

    #[test]
    fn the_users_text_comes_back_after_the_paste() {
        let original = "Привет, world! №1 — \"<&>\" 🙂";
        let clipboard = Fake::holding(original);
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        assert_eq!(clipboard.contents().as_deref(), Some("диктовка"));
        assert_eq!(state.restore(generation, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some(original));
    }

    #[test]
    fn an_empty_clipboard_stays_empty() {
        let clipboard = Fake::default();
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        assert_eq!(state.restore(generation, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents(), None);
    }

    #[test]
    fn back_to_back_pastes_restore_the_users_contents_not_the_previous_dictation() {
        let clipboard = Fake::holding("мой текст");
        let mut state = FakePreserver::new();
        let first = state.begin(&clipboard, "первая").unwrap();
        // The second dictation lands before the first restore is due.
        let second = state.begin(&clipboard, "вторая").unwrap();
        assert_eq!(state.restore(first, &clipboard), Restore::Superseded);
        assert_eq!(clipboard.contents().as_deref(), Some("вторая"));
        assert_eq!(state.restore(second, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn newer_user_contents_are_not_overwritten_with_older_ones() {
        let clipboard = Fake::holding("старое");
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        clipboard.user_copies("новое");
        assert_eq!(
            state.restore(generation, &clipboard),
            Restore::ChangedByUser
        );
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_copy_made_while_waiting_for_the_clipboard_is_not_overwritten() {
        // The ownership check must follow the wait for access, not precede it.
        let clipboard = Fake::holding("старое");
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        clipboard.with(|s| s.copied_while_opening = Some("новое".into()));
        assert_eq!(
            state.restore(generation, &clipboard),
            Restore::ChangedByUser
        );
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_copy_made_while_waiting_to_paste_is_captured_not_mistaken_for_ours() {
        let clipboard = Fake::holding("старое");
        let mut state = FakePreserver::new();
        state.begin(&clipboard, "первая").unwrap();
        // The first restore has not run, and someone copies as the second
        // paste waits for the clipboard: that copy is now the user's data.
        clipboard.with(|s| s.copied_while_opening = Some("новое".into()));
        let second = state.begin(&clipboard, "вторая").unwrap();
        assert_eq!(state.restore(second, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_second_paste_that_fails_untouched_leaves_the_first_restore_due() {
        let clipboard = Fake::holding("мой текст");
        let mut state = FakePreserver::new();
        let first = state.begin(&clipboard, "первая").unwrap();
        clipboard.with(|s| s.fail_prepare = true);
        let error = state.begin(&clipboard, "вторая").unwrap_err();
        assert!(error.retry.is_none());
        assert_eq!(clipboard.contents().as_deref(), Some("первая"));
        // The first dictation's restore still owns the user's contents.
        assert_eq!(state.restore(first, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn a_copy_made_between_capture_and_write_is_kept_and_nothing_is_pasted() {
        // macOS cannot hold the pasteboard: the count is checked again
        // right before it is cleared.
        let clipboard = Fake::holding("старое");
        clipboard.with(|s| s.copied_after_capture = Some("новое".into()));
        let mut state = FakePreserver::new();
        let error = state.begin(&clipboard, "диктовка").unwrap_err();
        assert!(error.retry.is_none());
        assert!(state.pending.is_none());
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_copy_made_while_a_restore_prepares_is_not_overwritten() {
        let clipboard = Fake::holding("старое");
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        clipboard.with(|s| s.copied_before_restore = Some("новое".into()));
        assert_eq!(
            state.restore(generation, &clipboard),
            Restore::ChangedByUser
        );
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_copy_made_while_a_retry_prepares_is_not_overwritten() {
        // The retry writes only over the state its failed attempt reported
        // leaving, and checks it again right before clearing.
        let clipboard = Fake::holding("мой текст");
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        clipboard.with(|s| s.failing_restores = 1);
        assert!(matches!(
            state.restore(generation, &clipboard),
            Restore::Failed(_)
        ));
        clipboard.with(|s| s.copied_before_restore = Some("новое".into()));
        assert_eq!(
            state.restore(generation, &clipboard),
            Restore::ChangedByUser
        );
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_capture_failure_stops_before_the_clipboard_is_touched() {
        let clipboard = Fake::holding("мой текст");
        clipboard.with(|s| s.fail_capture = true);
        let mut state = FakePreserver::new();
        let changes = clipboard.0.borrow().changes;
        let error = state.begin(&clipboard, "диктовка").unwrap_err();
        assert!(error.retry.is_none());
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
        assert_eq!(clipboard.0.borrow().changes, changes);
    }

    #[test]
    fn a_busy_clipboard_stops_the_paste() {
        let clipboard = Fake::holding("мой текст");
        clipboard.with(|s| s.busy = true);
        let mut state = FakePreserver::new();
        assert!(state.begin(&clipboard, "диктовка").is_err());
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn a_failed_write_puts_the_contents_back_at_once() {
        let clipboard = Fake::holding("мой текст");
        clipboard.with(|s| s.fail_write = true);
        let mut state = FakePreserver::new();
        let error = state.begin(&clipboard, "диктовка").unwrap_err();
        assert!(error.retry.is_none());
        assert!(state.pending.is_none());
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn a_failed_write_that_cannot_be_undone_leaves_a_restore_due() {
        let clipboard = Fake::holding("мой текст");
        clipboard.with(|s| {
            s.fail_write = true;
            s.failing_restores = 1;
        });
        let mut state = FakePreserver::new();
        let generation = state
            .begin(&clipboard, "диктовка")
            .unwrap_err()
            .retry
            .unwrap();
        assert_eq!(state.restore(generation, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn a_partial_restore_is_retried_over_the_state_it_left() {
        let clipboard = Fake::holding("мой текст");
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        clipboard.with(|s| s.failing_restores = 1);
        assert!(matches!(
            state.restore(generation, &clipboard),
            Restore::Failed(_)
        ));
        // The failed attempt changed the clipboard itself; that must not be
        // mistaken for a newer copy by the user.
        assert_eq!(clipboard.contents().as_deref(), Some("частично"));
        assert_eq!(state.restore(generation, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn a_partial_restore_is_not_retried_over_a_newer_copy() {
        let clipboard = Fake::holding("мой текст");
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        clipboard.with(|s| s.failing_restores = 1);
        assert!(matches!(
            state.restore(generation, &clipboard),
            Restore::Failed(_)
        ));
        clipboard.user_copies("новое");
        assert_eq!(
            state.restore(generation, &clipboard),
            Restore::ChangedByUser
        );
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_restore_blocked_by_a_busy_clipboard_keeps_its_capture() {
        let clipboard = Fake::holding("мой текст");
        let mut state = FakePreserver::new();
        let generation = state.begin(&clipboard, "диктовка").unwrap();
        clipboard.with(|s| s.busy = true);
        assert!(matches!(
            state.restore(generation, &clipboard),
            Restore::Failed(_)
        ));
        clipboard.with(|s| s.busy = false);
        assert_eq!(state.restore(generation, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn a_restore_given_up_on_is_finished_by_the_next_paste() {
        let clipboard = Fake::holding("мой текст");
        let mut state = FakePreserver::new();
        let first = state.begin(&clipboard, "первая").unwrap();
        clipboard.with(|s| s.failing_restores = 1);
        assert!(matches!(
            state.restore(first, &clipboard),
            Restore::Failed(_)
        ));
        // No retry ran; the next dictation still keeps the user's contents.
        let second = state.begin(&clipboard, "вторая").unwrap();
        assert_eq!(state.restore(second, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }
}
