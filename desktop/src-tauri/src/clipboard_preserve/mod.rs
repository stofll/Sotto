//! «Не изменять буфер обмена» (`preserve_clipboard`): an automatic paste that
//! leaves the user's clipboard as it found it.
//!
//! The paste itself still goes through the clipboard: every target accepts
//! Ctrl/Cmd+V, and there is no portable way to type text into another
//! application. So the previous contents are captured, the text is put on the
//! clipboard for the keystroke, and the capture is put back afterwards.
//!
//! The restore waits [`RESTORE_DELAY`]: the keystroke is queued input, and the
//! target reads the clipboard only when it gets to it. It runs through
//! `run_delivery`, so it is ordered with later pastes, and it is skipped when
//! the clipboard changed after our write — the user copied something newer,
//! and putting older data over it would lose that. A paste that arrives while
//! a restore is still pending reuses that capture: by then the clipboard holds
//! the previous dictation, not the user's data.
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

/// The clipboard operations a preserving paste needs.
trait Store {
    type Snapshot;
    /// Identifies one write of ours, to tell later whether it is still there.
    type Stamp;
    fn capture(&self) -> Result<Self::Snapshot, String>;
    fn write_text(&self, text: &str) -> Result<Self::Stamp, String>;
    fn still_ours(&self, stamp: &Self::Stamp) -> bool;
    fn restore(&self, snapshot: &Self::Snapshot) -> Result<(), String>;
}

struct Pending<S, T> {
    generation: u64,
    snapshot: S,
    stamp: T,
}

#[derive(Debug, PartialEq, Eq)]
enum Restore {
    Restored,
    /// A later paste took the capture over; its own restore will run.
    Superseded,
    /// The user put something new on the clipboard; it stays.
    ChangedByUser,
    Failed(String),
}

/// The capture waiting for its restore, at most one at a time.
struct Preserver<S, T> {
    generation: u64,
    pending: Option<Pending<S, T>>,
}

impl<S, T> Preserver<S, T> {
    const fn new() -> Self {
        Self {
            generation: 0,
            pending: None,
        }
    }

    /// The user's contents to bring back after the next paste.
    fn original<B: Store<Snapshot = S, Stamp = T>>(&mut self, store: &B) -> Result<S, String> {
        match self.pending.take() {
            Some(pending) if store.still_ours(&pending.stamp) => Ok(pending.snapshot),
            _ => store.capture(),
        }
    }

    /// Hand `snapshot` to a restore; returns the generation it must match.
    fn park(&mut self, snapshot: S, stamp: T) -> u64 {
        self.generation += 1;
        self.pending = Some(Pending {
            generation: self.generation,
            snapshot,
            stamp,
        });
        self.generation
    }

    fn restore<B: Store<Snapshot = S, Stamp = T>>(
        &mut self,
        generation: u64,
        store: &B,
    ) -> Restore {
        let pending = match self.pending.take() {
            Some(pending) if pending.generation == generation => pending,
            other => {
                self.pending = other;
                return Restore::Superseded;
            }
        };
        if !store.still_ours(&pending.stamp) {
            return Restore::ChangedByUser;
        }
        match store.restore(&pending.snapshot) {
            Ok(()) => Restore::Restored,
            Err(error) => {
                // Keep it: the next paste then still brings the user's
                // contents back instead of capturing our text as theirs.
                self.pending = Some(pending);
                Restore::Failed(error)
            }
        }
    }
}

type NativePreserver =
    Preserver<<native::Clipboard as Store>::Snapshot, <native::Clipboard as Store>::Stamp>;

static STATE: std::sync::Mutex<NativePreserver> = std::sync::Mutex::new(Preserver::new());

/// Run `paste` with `text` on the clipboard, then put the user's contents
/// back. Runs inside `run_delivery`, like `clipboard::paste_text`.
///
/// Nothing is pasted when the contents cannot be captured: pasting anyway
/// would lose exactly what the setting promises to keep. The text is still
/// in the history.
pub fn paste_preserving(
    app: &AppHandle,
    text: &str,
    paste: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let store = native::Clipboard::new(app);
    let snapshot = crate::mutex_recover::lock(&STATE)
        .original(&store)
        .map_err(|error| format!("could not save the clipboard, nothing pasted: {error}"))?;
    let stamp = match store.write_text(text) {
        Ok(stamp) => stamp,
        Err(error) => {
            // A failed write may already have emptied the clipboard.
            if let Err(restore) = store.restore(&snapshot) {
                log::warn!("clipboard restore after a failed write failed: {restore}");
            }
            return Err(error);
        }
    };
    // Dropped on every way out of `paste`, a panic included.
    let _restore = ScheduledRestore {
        app: app.clone(),
        parked: Some((snapshot, stamp)),
    };
    paste()
}

type NativeSnapshot = <native::Clipboard as Store>::Snapshot;
type NativeStamp = <native::Clipboard as Store>::Stamp;

struct ScheduledRestore {
    app: AppHandle,
    parked: Option<(NativeSnapshot, NativeStamp)>,
}

impl Drop for ScheduledRestore {
    fn drop(&mut self) {
        let Some((snapshot, stamp)) = self.parked.take() else {
            return;
        };
        let generation = crate::mutex_recover::lock(&STATE).park(snapshot, stamp);
        let app = self.app.clone();
        let spawned = std::thread::Builder::new()
            .name("clipboard-restore".into())
            .spawn(move || {
                std::thread::sleep(RESTORE_DELAY);
                let job_app = app.clone();
                if let Err(error) =
                    crate::clipboard::run_delivery(&app, move || restore_now(&job_app, generation))
                {
                    log::warn!("clipboard restore not scheduled: {error}");
                }
            });
        if spawned.is_err() {
            // Early is better than never: the user's contents would be lost.
            restore_now(&self.app, generation);
        }
    }
}

fn restore_now(app: &AppHandle, generation: u64) {
    let store = native::Clipboard::new(app);
    match crate::mutex_recover::lock(&STATE).restore(generation, &store) {
        Restore::Restored | Restore::Superseded => {}
        Restore::ChangedByUser => {
            log::info!("clipboard changed after the paste; not restoring older contents")
        }
        Restore::Failed(error) => log::warn!("clipboard restore failed: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A clipboard holding one string, with a change counter like the
    /// Windows sequence number and the macOS change count.
    #[derive(Default)]
    struct FakeClipboard {
        contents: RefCell<Option<String>>,
        changes: RefCell<u32>,
        fail_capture: bool,
        fail_restore: RefCell<bool>,
    }

    impl FakeClipboard {
        fn holding(text: &str) -> Self {
            let clipboard = Self::default();
            clipboard.user_copies(text);
            clipboard
        }
        fn user_copies(&self, text: &str) {
            *self.contents.borrow_mut() = Some(text.to_string());
            *self.changes.borrow_mut() += 1;
        }
        fn contents(&self) -> Option<String> {
            self.contents.borrow().clone()
        }
    }

    impl Store for FakeClipboard {
        type Snapshot = Option<String>;
        type Stamp = u32;
        fn capture(&self) -> Result<Option<String>, String> {
            if self.fail_capture {
                return Err("busy".into());
            }
            Ok(self.contents())
        }
        fn write_text(&self, text: &str) -> Result<u32, String> {
            self.user_copies(text);
            Ok(*self.changes.borrow())
        }
        fn still_ours(&self, stamp: &u32) -> bool {
            *self.changes.borrow() == *stamp
        }
        fn restore(&self, snapshot: &Option<String>) -> Result<(), String> {
            if *self.fail_restore.borrow() {
                return Err("busy".into());
            }
            *self.contents.borrow_mut() = snapshot.clone();
            *self.changes.borrow_mut() += 1;
            Ok(())
        }
    }

    /// One preserving paste up to the moment its restore is due.
    fn paste(
        state: &mut Preserver<Option<String>, u32>,
        clipboard: &FakeClipboard,
        text: &str,
    ) -> u64 {
        let snapshot = state.original(clipboard).unwrap();
        let stamp = clipboard.write_text(text).unwrap();
        state.park(snapshot, stamp)
    }

    #[test]
    fn the_users_text_comes_back_after_the_paste() {
        let clipboard = FakeClipboard::holding("Привет, world! №1 — \"<&>\" 🙂");
        let mut state = Preserver::new();
        let generation = paste(&mut state, &clipboard, "диктовка");
        assert_eq!(clipboard.contents().as_deref(), Some("диктовка"));
        assert_eq!(state.restore(generation, &clipboard), Restore::Restored);
        assert_eq!(
            clipboard.contents().as_deref(),
            Some("Привет, world! №1 — \"<&>\" 🙂")
        );
    }

    #[test]
    fn an_empty_clipboard_stays_empty() {
        let clipboard = FakeClipboard::default();
        let mut state = Preserver::new();
        let generation = paste(&mut state, &clipboard, "диктовка");
        assert_eq!(state.restore(generation, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents(), None);
    }

    #[test]
    fn back_to_back_pastes_restore_the_users_contents_not_the_previous_dictation() {
        let clipboard = FakeClipboard::holding("мой текст");
        let mut state = Preserver::new();
        let first = paste(&mut state, &clipboard, "первая");
        // The second dictation lands before the first restore is due.
        let second = paste(&mut state, &clipboard, "вторая");
        assert_eq!(state.restore(first, &clipboard), Restore::Superseded);
        assert_eq!(clipboard.contents().as_deref(), Some("вторая"));
        assert_eq!(state.restore(second, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn newer_user_contents_are_not_overwritten_with_older_ones() {
        let clipboard = FakeClipboard::holding("старое");
        let mut state = Preserver::new();
        let generation = paste(&mut state, &clipboard, "диктовка");
        clipboard.user_copies("новое");
        assert_eq!(
            state.restore(generation, &clipboard),
            Restore::ChangedByUser
        );
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_paste_after_the_user_copied_captures_the_new_contents() {
        let clipboard = FakeClipboard::holding("старое");
        let mut state = Preserver::new();
        paste(&mut state, &clipboard, "первая");
        clipboard.user_copies("новое");
        // The first restore has not run yet: its capture is stale now.
        let second = paste(&mut state, &clipboard, "вторая");
        assert_eq!(state.restore(second, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("новое"));
    }

    #[test]
    fn a_failed_restore_is_retried_by_the_next_paste() {
        let clipboard = FakeClipboard::holding("мой текст");
        let mut state = Preserver::new();
        let first = paste(&mut state, &clipboard, "первая");
        *clipboard.fail_restore.borrow_mut() = true;
        assert!(matches!(
            state.restore(first, &clipboard),
            Restore::Failed(_)
        ));
        *clipboard.fail_restore.borrow_mut() = false;
        let second = paste(&mut state, &clipboard, "вторая");
        assert_eq!(state.restore(second, &clipboard), Restore::Restored);
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }

    #[test]
    fn a_capture_failure_stops_before_the_clipboard_is_touched() {
        let clipboard = FakeClipboard {
            fail_capture: true,
            ..FakeClipboard::holding("мой текст")
        };
        let mut state: Preserver<Option<String>, u32> = Preserver::new();
        assert!(state.original(&clipboard).is_err());
        assert_eq!(clipboard.contents().as_deref(), Some("мой текст"));
    }
}
