//! Hotkey string parsing and runtime registration.
//!
//! The parser takes strings like `"ctrl+shift+a"` and produces a `HotkeySpec`
//! that downstream code can hand to `tauri-plugin-global-shortcut`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub mods: Modifiers,
    pub key: String,
}

/// Parse a hotkey string like `"ctrl+shift+a"` into a [`HotkeySpec`].
///
/// Tokens are split on `+`. The last token is the key (preserved as-typed
/// after `.to_lowercase()`); all preceding tokens are modifiers. Aliases:
/// `ctrl`/`control`, `alt`, `shift`, `cmd`/`super`/`win`/`meta` (all map to
/// `meta`). Unknown modifiers and empty input are rejected with an `Err`.
/// A key without modifiers is accepted only when [`is_standalone_key`].
///
/// The parser is layout-agnostic: any non-empty key token (including
/// non-Latin characters like Cyrillic letters) is accepted; downstream
/// `tauri-plugin-global-shortcut` is responsible for physical-key mapping.
pub fn parse(hotkey: &str) -> Result<HotkeySpec, String> {
    let trimmed = hotkey.trim();
    if trimmed.is_empty() {
        return Err("empty hotkey".into());
    }
    let parts: Vec<&str> = trimmed.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last().expect("split yields at least one token");
    let key = key.to_lowercase();
    // Reject when the key token is itself a modifier name — e.g. "ctrl+shift"
    // has no actual key.
    match key.as_str() {
        "ctrl" | "control" | "alt" | "shift" | "cmd" | "super" | "win" | "meta" => {
            return Err(format!(
                "hotkey must contain a non-modifier key, got modifier-only: {hotkey:?}"
            ));
        }
        _ => {}
    }
    let mut mods = Modifiers::default();
    for token in modifiers {
        match token.to_lowercase().as_str() {
            "ctrl" | "control" => mods.ctrl = true,
            "alt" => mods.alt = true,
            "shift" => mods.shift = true,
            "cmd" | "super" | "win" | "meta" => mods.meta = true,
            other => return Err(format!("unknown modifier: {other}")),
        }
    }
    if key.is_empty() {
        return Err("empty key token".into());
    }
    if modifiers.is_empty() && !is_standalone_key(&key) {
        return Err(format!(
            "a hotkey without modifiers must be F1-F24 or a numpad key, got: {hotkey:?}"
        ));
    }
    Ok(HotkeySpec { mods, key })
}

/// Keys that may be bound without a modifier. A global registration takes
/// the key away from every other application, so letters, digits, editing
/// and navigation keys would break ordinary typing. `numpadenter` is left
/// out because Windows registers it as the main Enter key.
fn is_standalone_key(key: &str) -> bool {
    let function_key = key
        .strip_prefix('f')
        .filter(|n| !n.starts_with('0'))
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n));
    let numpad_operator = matches!(
        key,
        "numpadadd" | "numpadsubtract" | "numpadmultiply" | "numpaddivide"
    );
    function_key || numpad_operator || is_num_lock_key(key)
}

/// Numpad keys that Windows reports as navigation keys (Insert, End, …)
/// unless Num Lock is on.
fn is_num_lock_key(key: &str) -> bool {
    key.strip_prefix("numpad").is_some_and(|rest| {
        rest == "decimal" || (rest.len() == 1 && rest.as_bytes()[0].is_ascii_digit())
    })
}

// ---- runtime registration ----

use tauri::AppHandle;
use tauri::Emitter;
use tauri_plugin_global_shortcut::Modifiers as PluginMods;
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::state::AppState;

/// Convert our parsed spec into the plugin's `Shortcut` type.
fn to_shortcut(spec: &HotkeySpec) -> Result<Shortcut, String> {
    let mut mods = PluginMods::empty();
    if spec.mods.ctrl {
        mods |= PluginMods::CONTROL;
    }
    if spec.mods.alt {
        mods |= PluginMods::ALT;
    }
    if spec.mods.shift {
        mods |= PluginMods::SHIFT;
    }
    if spec.mods.meta {
        // docs.rs for global-hotkey: `Shortcut::new` accepts only
        // ALT/SHIFT/CONTROL/SUPER. There is no META flag, so cmd/super/win
        // aliases must be mapped to SUPER here.
        mods |= PluginMods::SUPER;
    }
    let code = match spec.key.as_str() {
        // Letters
        "a" => Code::KeyA,
        "b" => Code::KeyB,
        "c" => Code::KeyC,
        "d" => Code::KeyD,
        "e" => Code::KeyE,
        "f" => Code::KeyF,
        "g" => Code::KeyG,
        "h" => Code::KeyH,
        "i" => Code::KeyI,
        "j" => Code::KeyJ,
        "k" => Code::KeyK,
        "l" => Code::KeyL,
        "m" => Code::KeyM,
        "n" => Code::KeyN,
        "o" => Code::KeyO,
        "p" => Code::KeyP,
        "q" => Code::KeyQ,
        "r" => Code::KeyR,
        "s" => Code::KeyS,
        "t" => Code::KeyT,
        "u" => Code::KeyU,
        "v" => Code::KeyV,
        "w" => Code::KeyW,
        "x" => Code::KeyX,
        "y" => Code::KeyY,
        "z" => Code::KeyZ,
        "0" => Code::Digit0,
        "1" => Code::Digit1,
        "2" => Code::Digit2,
        "3" => Code::Digit3,
        "4" => Code::Digit4,
        "5" => Code::Digit5,
        "6" => Code::Digit6,
        "7" => Code::Digit7,
        "8" => Code::Digit8,
        "9" => Code::Digit9,
        "f1" => Code::F1,
        "f2" => Code::F2,
        "f3" => Code::F3,
        "f4" => Code::F4,
        "f5" => Code::F5,
        "f6" => Code::F6,
        "f7" => Code::F7,
        "f8" => Code::F8,
        "f9" => Code::F9,
        "f10" => Code::F10,
        "f11" => Code::F11,
        "f12" => Code::F12,
        "f13" => Code::F13,
        "f14" => Code::F14,
        "f15" => Code::F15,
        "f16" => Code::F16,
        "f17" => Code::F17,
        "f18" => Code::F18,
        "f19" => Code::F19,
        "f20" => Code::F20,
        "f21" => Code::F21,
        "f22" => Code::F22,
        "f23" => Code::F23,
        "f24" => Code::F24,
        "delete" => Code::Delete,
        "insert" => Code::Insert,
        "left" => Code::ArrowLeft,
        "right" => Code::ArrowRight,
        "up" => Code::ArrowUp,
        "down" => Code::ArrowDown,
        "home" => Code::Home,
        "end" => Code::End,
        "pageup" => Code::PageUp,
        "pagedown" => Code::PageDown,
        "minus" => Code::Minus,
        "equal" => Code::Equal,
        "bracketleft" => Code::BracketLeft,
        "bracketright" => Code::BracketRight,
        "backslash" => Code::Backslash,
        "semicolon" => Code::Semicolon,
        "quote" => Code::Quote,
        "backquote" => Code::Backquote,
        "comma" => Code::Comma,
        "period" => Code::Period,
        "slash" => Code::Slash,
        "numpad0" => Code::Numpad0,
        "numpad1" => Code::Numpad1,
        "numpad2" => Code::Numpad2,
        "numpad3" => Code::Numpad3,
        "numpad4" => Code::Numpad4,
        "numpad5" => Code::Numpad5,
        "numpad6" => Code::Numpad6,
        "numpad7" => Code::Numpad7,
        "numpad8" => Code::Numpad8,
        "numpad9" => Code::Numpad9,
        "numpadadd" => Code::NumpadAdd,
        "numpadsubtract" => Code::NumpadSubtract,
        "numpadmultiply" => Code::NumpadMultiply,
        "numpaddivide" => Code::NumpadDivide,
        "numpaddecimal" => Code::NumpadDecimal,
        "numpadenter" => Code::NumpadEnter,
        // Special keys
        "space" => Code::Space,
        "enter" => Code::Enter,
        "tab" => Code::Tab,
        "esc" | "escape" => Code::Escape,
        "backspace" => Code::Backspace,
        other => return Err(format!("unsupported key token: {other:?}")),
    };
    Ok(Shortcut::new(Some(mods), code))
}

/// Install a global shortcut and route Pressed/Released events to the
/// whisper engine via the shared `AppState`.
///
/// The closure captures `state.clone()` (cheap — every field is
/// `Arc`-backed), so a press reaches the engine thread directly.
pub fn register(app: &AppHandle, state: &AppState, hotkey: &str) -> Result<(), String> {
    let spec = parse(hotkey)?;
    let shortcut = to_shortcut(&spec)?;
    let state_for_handler = state.clone();
    // The handler needs its own AppHandle to emit recording-started/stopped
    // and hotkey-error events; the plugin's `_app` argument is not one.
    let app_for_handler = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _sc, event| {
            handle_shortcut_event(app_for_handler.clone(), state_for_handler.clone(), event);
        })
        .map_err(|e| format!("register failed: {e}"))
}

pub fn unregister(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let spec = parse(hotkey)?;
    let shortcut = to_shortcut(&spec)?;
    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|e| format!("unregister failed: {e}"))
}

/// Swap a global shortcut binding: register `new`, then release `old`.
///
/// The order matters and it is not the obvious one. Releasing first is what
/// you would write, and it loses the working shortcut whenever the new
/// combination cannot be taken — because the OS reserves it, or because the
/// app's *other* dictation shortcut already holds it. The user is then left
/// with nothing bound and an error message, and the only way back is to set
/// it again or restart. Registering first means a rejected combination
/// costs an error and nothing else.
///
/// Re-selecting a shortcut already owned by this application is a no-op.
pub fn re_register(app: &AppHandle, state: &AppState, old: &str, new: &str) -> Result<(), String> {
    let new_shortcut = to_shortcut(&parse(new)?)?;
    // Modifier order, case and aliases can spell the same native shortcut.
    // An invalid/unregistered old setting must not prevent repairing it.
    let old_shortcut = parse(old).and_then(|spec| to_shortcut(&spec)).ok();
    if old_shortcut == Some(new_shortcut) && app.global_shortcut().is_registered(new_shortcut) {
        return Ok(());
    }
    register(app, state, new)?;
    if old_shortcut
        .filter(|shortcut| *shortcut != new_shortcut)
        .is_some_and(|shortcut| app.global_shortcut().is_registered(shortcut))
    {
        if let Err(error) = unregister(app, old) {
            return match unregister(app, new) {
                Ok(()) => Err(error),
                Err(rollback) => Err(format!("{error}; release new shortcut: {rollback}")),
            };
        }
    }
    Ok(())
}

/// Undo one binding change while the configuration writer lock is still held.
pub(crate) type BindingRollback = Box<dyn FnOnce() -> Result<(), String>>;

pub(crate) fn re_register_with_rollback(
    app: &AppHandle,
    state: &AppState,
    old: &str,
    new: &str,
) -> Result<BindingRollback, String> {
    let registered = |text| {
        parse(text)
            .and_then(|spec| to_shortcut(&spec))
            .is_ok_and(|shortcut| app.global_shortcut().is_registered(shortcut))
    };
    let had_old = registered(old);
    let had_new = registered(new);
    re_register(app, state, old, new)?;
    let app = app.clone();
    let state = state.clone();
    let old = old.to_string();
    let new = new.to_string();
    Ok(Box::new(move || {
        if had_old {
            re_register(&app, &state, &new, &old)
        } else if !had_new {
            // A missing/invalid old setting had no native binding to restore.
            // Remove only the new binding acquired by this transaction.
            unregister(&app, &new)
        } else {
            Ok(())
        }
    }))
}

/// Handle a global-shortcut event.
///
/// Behaviour depends on the `recording_mode` config key:
/// - `"toggle"` (default): `Pressed` toggles recording on/off; `Released` is ignored.
/// - `"push_to_talk"`: `Pressed` starts recording, `Released` stops.
///
/// The default MUST match the frontend, which treats a missing
/// `recording_mode` as `"toggle"` (`SettingsPage.tsx`: `?? "toggle"` and
/// `value === "push_to_talk" ? … : "toggle"`). Because the UI shows toggle
/// as pre-selected on a fresh/partial config, the user never explicitly
/// saves the key — so if the backend defaulted to push-to-talk instead, the
/// app would silently run push-to-talk while the UI claimed toggle (a single
/// tap then captured only a few ms of audio → empty transcription).
///
/// Debounce window for OS key auto-repeat. A second `Pressed` that lands
/// within this window of the previous one (while the key is still believed
/// held) is treated as auto-repeat and ignored. The time bound is also what
/// makes a *dropped* `Released` self-healing: `key_held` alone would stay set
/// forever and brick the toggle, but after this long the next press is honoured
/// as a fresh leading edge regardless of the stuck flag.
const AUTO_REPEAT_DEBOUNCE_MS: u64 = 500;

/// Monotonic millisecond clock for the auto-repeat debounce. Lazily anchored to
/// the first press so the value can live in a plain `AtomicU64`.
fn monotonic_now_ms() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// Timestamp (via [`monotonic_now_ms`]) of the last `Pressed` we saw, used
/// together with `key_held` to distinguish auto-repeat from a real re-press.
static LAST_PRESS_MS: AtomicU64 = AtomicU64::new(0);

fn handle_shortcut_event(app: AppHandle, state: AppState, event: ShortcutEvent) {
    // Read recording_mode from config to determine toggle vs push-to-talk.
    // Read the current setting on each event so mode changes take effect
    // without a separate cache synchronization path in AppState.
    // Toggle is the default: only an explicit `"push_to_talk"` selects
    // push-to-talk. Absent key, unreadable config, or any other value all
    // fall through to toggle so the runtime behaviour matches the UI.
    let is_toggle = crate::config::Config::load(&app)
        .ok()
        .and_then(|c| c.get_string("recording_mode"))
        .map(|m| m != "push_to_talk")
        .unwrap_or(true);

    match event.state() {
        ShortcutState::Pressed => {
            // Debounce OS key auto-repeat. Windows fires repeated `Pressed`
            // events while the combo is physically held; we only act on the
            // leading edge (up→down transition). Without this guard, toggle
            // mode flips start→stop within tens of milliseconds and the
            // recording is too short to transcribe (whisper-empty).
            //
            // We treat a press as auto-repeat only when the key is STILL
            // believed held (`key_held` was already true) AND the previous
            // press was recent. Relying on `key_held` alone was fragile: it is
            // cleared only on `Released`, so a single dropped `Released` event
            // (focus steal, out-of-order modifier release, hook glitch) would
            // wedge the flag true forever and brick the hotkey until restart.
            // The time bound self-heals that: a genuine re-press after
            // `AUTO_REPEAT_DEBOUNCE_MS` is honoured even if `key_held` is stuck.
            let now = monotonic_now_ms();
            let was_held = state.key_held.swap(true, Ordering::AcqRel);
            let last = LAST_PRESS_MS.swap(now, Ordering::AcqRel);
            if was_held && now.saturating_sub(last) < AUTO_REPEAT_DEBOUNCE_MS {
                return;
            }
            if is_toggle {
                // Toggle mode: use toggle_armed atomic instead of
                // recorder.is_recording() to avoid TOCTOU races with
                // rapid hotkey events on Windows.
                if state.toggle_armed.load(Ordering::Acquire) {
                    // Second toggle press → stop.
                    state.toggle_armed.store(false, Ordering::Release);
                    hotkey_do_stop(&app, &state);
                } else {
                    // First toggle press → start.
                    state.toggle_armed.store(true, Ordering::Release);
                    hotkey_do_start(&app, &state);
                }
            } else {
                // Push-to-talk: press starts recording.
                hotkey_do_start(&app, &state);
            }
        }
        ShortcutState::Released => {
            // Physical key up: clear the debounce flag so the next real
            // press is recognised as a leading edge.
            state.key_held.store(false, Ordering::Release);
            if !is_toggle {
                // Push-to-talk: release stops recording.
                hotkey_do_stop(&app, &state);
            }
            // Toggle mode: release is ignored (the second press stops).
        }
    }
}

/// Submit through the same lifecycle as IPC, without waiting on the UI thread.
fn hotkey_do_start(app: &AppHandle, state: &AppState) {
    if let Err(message) = crate::dictation::start(app, state, true) {
        if !state.recorder.is_recording() {
            state.toggle_armed.store(false, Ordering::Release);
        }
        let _ = app.emit("hotkey-error", message);
    }
}

fn hotkey_do_stop(app: &AppHandle, state: &AppState) {
    if let Err(message) = crate::dictation::stop(app, state) {
        let _ = app.emit("hotkey-error", message);
    }
}

/// Validate a hotkey string at the UI layer — used by `SettingsPage` to
/// show an inline error as the user types. Pure parser call; no side
/// effects, hence `sync`. Returns `Err(msg)` for
/// any parse failure (unknown modifier, empty, modifier-only, etc.) and
/// `Ok(())` for a valid string.
#[tauri::command]
pub(crate) fn validate_hotkey(hotkey: String) -> Result<(), String> {
    let spec = parse(&hotkey)?;
    // Windows turns Shift+numpad digit into a navigation key: with Num Lock
    // on it also releases Shift first, with it off the digit never arrives.
    // Only new choices are refused, so an already saved binding can still be
    // parsed, released and replaced.
    if cfg!(windows) && spec.mods.shift && is_num_lock_key(&spec.key) {
        return Err(format!(
            "Windows never delivers Shift with a numpad digit, got: {hotkey:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    #[ignore = "temporarily registers Ctrl+Alt+Shift+F23/F24 and bare F24; requires a Windows desktop session"]
    fn native_hotkey_rebinding_preserves_ownership() {
        use std::sync::{Arc, Mutex};

        const FIRST: &str = "ctrl+alt+shift+f23";
        const SECOND: &str = "ctrl+alt+shift+f24";
        const BARE: &str = "f24";
        let first = to_shortcut(&parse(FIRST).unwrap()).unwrap();
        let second = to_shortcut(&parse(SECOND).unwrap()).unwrap();
        let bare = to_shortcut(&parse(BARE).unwrap()).unwrap();
        // No application setup, windows, running event loop or injected keys. The
        // recorder constructors allocate state but never open a native stream.
        let mut context = tauri::generate_context!();
        context.config_mut().identifier = "com.sotto.hotkey-test".into();
        context.config_mut().app.windows.clear();
        let app = tauri::Builder::default()
            .any_thread()
            .plugin(tauri_plugin_global_shortcut::Builder::new().build())
            .build(context)
            .expect("create isolated native shortcut app");
        let handle = app.handle();
        struct OwnedShortcuts<'a> {
            app: &'a AppHandle,
            keys: [Shortcut; 3],
        }
        impl Drop for OwnedShortcuts<'_> {
            fn drop(&mut self) {
                for key in self.keys {
                    // This registry only contains this test app's shortcuts.
                    // Never unregister a key merely because another process
                    // owns it, and clean up even when an assertion panics.
                    if self.app.global_shortcut().is_registered(key) {
                        if let Err(error) = self.app.global_shortcut().unregister(key) {
                            eprintln!("native shortcut test cleanup failed: {error}");
                        }
                    }
                }
            }
        }
        let cleanup = OwnedShortcuts {
            app: handle,
            keys: [first, second, bare],
        };
        let db = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::run_migrations(&db).unwrap();
        let state = AppState::new(
            tokio::sync::mpsc::channel(1).0,
            Arc::new(
                crate::audio::AudioRecorder::new(crate::audio::AudioConfig::default()).unwrap(),
            ),
            Arc::new(Mutex::new(db)),
            crate::mic_test::MicrophoneTest::new(),
            Arc::new(Mutex::new(None)),
        );
        // Defensively refuse capture even if a physical shortcut event were
        // delivered by native code; this test exercises registration only.
        state.engine_busy.store(true, Ordering::Release);
        assert!(!handle.global_shortcut().is_registered(first));
        assert!(!handle.global_shortcut().is_registered(second));

        re_register(handle, &state, FIRST, FIRST)
            .expect("register absent old==new; the test key must be available");
        assert!(handle.global_shortcut().is_registered(first));
        re_register(handle, &state, FIRST, " ALT + CONTROL + SHIFT + F23 ")
            .expect("aliases of the same native shortcut must be a no-op");
        assert!(handle.global_shortcut().is_registered(first));

        re_register(handle, &state, FIRST, SECOND).unwrap();
        assert!(!handle.global_shortcut().is_registered(first));
        assert!(handle.global_shortcut().is_registered(second));
        // Re-acquiring the released key goes through RegisterHotKey again;
        // checking only the plugin's registry would miss a native leak.
        handle.global_shortcut().register(first).unwrap();
        handle.global_shortcut().unregister(first).unwrap();
        assert!(re_register(handle, &state, SECOND, "ctrl+unsupported-key").is_err());
        assert!(handle.global_shortcut().is_registered(second));

        for invalid_old in ["invalid", "ctrl+unsupported-key"] {
            unregister(handle, SECOND).unwrap();
            re_register(handle, &state, invalid_old, SECOND)
                .expect("repairing an invalid saved shortcut must keep the new registration");
            assert!(handle.global_shortcut().is_registered(second));
        }
        // A real persistence failure must restore absence as well as a binding.
        let blocked_destination = tempfile::tempdir().unwrap();
        unregister(handle, SECOND).unwrap();
        for old in ["invalid", "ctrl+unsupported-key", FIRST] {
            let undo = re_register_with_rollback(handle, &state, old, FIRST).unwrap();
            assert!(handle.global_shortcut().is_registered(first));
            assert!(std::fs::write(blocked_destination.path(), b"config").is_err());
            undo().unwrap();
            assert!(!handle.global_shortcut().is_registered(first));
            assert!(!handle.global_shortcut().is_registered(second));
        }
        register(handle, &state, SECOND).unwrap();
        let undo = re_register_with_rollback(handle, &state, SECOND, FIRST).unwrap();
        assert!(std::fs::write(blocked_destination.path(), b"config").is_err());
        undo().unwrap();
        assert!(!handle.global_shortcut().is_registered(first));
        assert!(handle.global_shortcut().is_registered(second));
        let undo =
            re_register_with_rollback(handle, &state, SECOND, "ALT+CONTROL+SHIFT+F24").unwrap();
        undo().unwrap();
        assert!(
            handle.global_shortcut().is_registered(second),
            "no-op rollback preserves existing ownership"
        );
        // A modifier-less binding goes through RegisterHotKey without flags.
        re_register(handle, &state, SECOND, BARE).unwrap();
        assert!(handle.global_shortcut().is_registered(bare));
        assert!(!handle.global_shortcut().is_registered(second));
        re_register(handle, &state, BARE, SECOND).unwrap();
        assert!(!handle.global_shortcut().is_registered(bare));
        assert!(handle.global_shortcut().is_registered(second));
        drop(cleanup);
        assert!(!handle.global_shortcut().is_registered(first));
        assert!(!handle.global_shortcut().is_registered(second));
        assert!(!handle.global_shortcut().is_registered(bare));
        assert!(!state.recorder.is_recording());
    }

    /// Captured key tokens that may be bound on their own.
    fn standalone_key_tokens() -> Vec<String> {
        let mut keys: Vec<String> = (1..=24).map(|n| format!("f{n}")).collect();
        keys.extend((0..=9).map(|n| format!("numpad{n}")));
        keys.extend(
            [
                "numpadadd",
                "numpadsubtract",
                "numpadmultiply",
                "numpaddivide",
                "numpaddecimal",
            ]
            .map(String::from),
        );
        keys
    }

    /// Captured key tokens that are used for typing or navigation.
    fn modifier_required_key_tokens() -> Vec<String> {
        let mut keys: Vec<String> = ('a'..='z').chain('0'..='9').map(String::from).collect();
        keys.extend(
            [
                "numpadenter",
                "space",
                "escape",
                "enter",
                "tab",
                "backspace",
                "delete",
                "insert",
                "left",
                "right",
                "up",
                "down",
                "home",
                "end",
                "pageup",
                "pagedown",
                "minus",
                "equal",
                "bracketleft",
                "bracketright",
                "backslash",
                "semicolon",
                "quote",
                "backquote",
                "comma",
                "period",
                "slash",
            ]
            .map(String::from),
        );
        keys
    }

    /// Every key token the settings capture (`desktop/src/hotkey.ts`) can produce.
    fn captured_key_tokens() -> Vec<String> {
        let mut keys = standalone_key_tokens();
        keys.extend(modifier_required_key_tokens());
        keys
    }

    /// The modifier subset selected by the low four bits of `mask`.
    fn modifier_set(mask: u8) -> (Vec<&'static str>, PluginMods) {
        let mut names = Vec::new();
        let mut mods = PluginMods::empty();
        for (bit, (name, flag)) in [
            ("ctrl", PluginMods::CONTROL),
            ("alt", PluginMods::ALT),
            ("shift", PluginMods::SHIFT),
            ("cmd", PluginMods::SUPER),
        ]
        .into_iter()
        .enumerate()
        {
            if mask & (1 << bit) != 0 {
                names.push(name);
                mods |= flag;
            }
        }
        (names, mods)
    }

    fn shortcut(hotkey: &str) -> Result<Shortcut, String> {
        parse(hotkey).and_then(|spec| to_shortcut(&spec))
    }

    #[test]
    fn every_captured_key_registers_with_every_modifier_set() {
        let keys = captured_key_tokens();
        let mut codes = std::collections::HashSet::new();
        for key in &keys {
            for mask in 1..16 {
                let (names, mods) = modifier_set(mask);
                let combo = format!("{}+{key}", names.join("+"));
                let native = shortcut(&combo).unwrap_or_else(|e| panic!("{combo}: {e}"));
                assert_eq!(native.mods, mods, "{combo}");
                codes.insert(native.key);
            }
        }
        assert_eq!(
            codes.len(),
            keys.len(),
            "every key token needs its own native key"
        );
    }

    #[test]
    fn function_and_numpad_keys_register_without_modifiers() {
        for key in standalone_key_tokens() {
            let native = shortcut(&key).unwrap_or_else(|e| panic!("{key}: {e}"));
            assert_eq!(native.mods, PluginMods::empty(), "{key}");
            assert_eq!(
                native,
                shortcut(&format!(" {} ", key.to_uppercase())).unwrap()
            );
        }
    }

    #[test]
    fn typing_and_navigation_keys_require_a_modifier() {
        let typing = modifier_required_key_tokens()
            .into_iter()
            .chain(["я", "f0", "f01", "f25", "fn", "numpad10"].map(String::from));
        for key in typing {
            let error = parse(&key).expect_err(&key);
            assert!(error.contains("without modifiers"), "{key}: {error}");
            // The same key stays available behind any single modifier.
            assert!(parse(&format!("alt+{key}")).is_ok(), "alt+{key}");
        }
    }

    #[test]
    fn shift_with_a_num_lock_numpad_key_is_refused_on_windows() {
        for key in ["numpad0", "numpad5", "numpad9", "numpaddecimal"] {
            for combo in [format!("shift+{key}"), format!("ctrl+Shift+{key}")] {
                assert_eq!(
                    validate_hotkey(combo.clone()).is_err(),
                    cfg!(windows),
                    "{combo}"
                );
                assert!(
                    shortcut(&combo).is_ok(),
                    "a saved {combo} can still be released"
                );
            }
            assert!(validate_hotkey(key.to_string()).is_ok(), "{key}");
            assert!(validate_hotkey(format!("ctrl+alt+{key}")).is_ok(), "{key}");
        }
        for combo in [
            "shift+numpadadd",
            "shift+numpaddivide",
            "shift+numpadenter",
            "shift+f5",
        ] {
            assert!(validate_hotkey(combo.into()).is_ok(), "{combo}");
        }
    }

    #[test]
    fn modifier_aliases_case_order_and_spacing_spell_the_same_shortcut() {
        let expected = shortcut("ctrl+alt+shift+cmd+numpad0").unwrap();
        for spelling in [
            "control+alt+shift+super+numpad0",
            "win+shift+alt+ctrl+numpad0",
            "META + SHIFT + ALT + CONTROL + NUMPAD0",
            "  cmd+ctrl+shift+alt+Numpad0  ",
            "ctrl+ctrl+alt+shift+cmd+numpad0",
        ] {
            assert_eq!(shortcut(spelling).unwrap(), expected, "{spelling}");
        }
        assert_eq!(
            shortcut("ctrl+esc").unwrap(),
            shortcut("ctrl+escape").unwrap()
        );
    }

    #[test]
    fn malformed_hotkeys_are_rejected() {
        for hotkey in [
            "",
            "   ",
            "+",
            "ctrl+",
            "+numpad0",
            "ctrl++a",
            "ctrl",
            "numpad0+ctrl",
            "shift+cmd",
            "hyper+f1",
            "fn+f1",
            "f1+a",
            "ctrl-a",
        ] {
            assert!(parse(hotkey).is_err(), "{hotkey:?} must be rejected");
        }
        // Accepted by the layout-agnostic parser, but no native key exists.
        for hotkey in ["ctrl+unknown", "ctrl+я"] {
            assert!(shortcut(hotkey).is_err(), "{hotkey:?} must not register");
        }
    }

    #[test]
    fn parse_simple_modifier_plus_letter() {
        let s = parse("ctrl+shift+a").unwrap();
        assert!(s.mods.ctrl);
        assert!(s.mods.shift);
        assert!(!s.mods.alt);
        assert!(!s.mods.meta);
        assert_eq!(s.key, "a");
    }

    #[test]
    fn parse_super_aliases() {
        for alias in ["cmd", "super", "win"] {
            let s = parse(&format!("{alias}+space")).unwrap();
            assert!(s.mods.meta, "alias {alias} should map to meta");
        }
    }

    #[test]
    fn parse_modifier_only_is_rejected() {
        assert!(parse("ctrl").is_err());
        assert!(parse("ctrl+shift").is_err());
    }

    #[test]
    fn parse_cyrillic_letter_is_accepted() {
        // Cyrillic letter — must be accepted as a physical key token, NOT rejected
        // for being non-Latin. The downstream plugin handles physical-key mapping.
        let s = parse("ctrl+я").unwrap();
        assert!(s.mods.ctrl);
        assert_eq!(s.key, "я");
    }

    #[test]
    fn parse_unknown_modifier_is_rejected() {
        assert!(parse("hyper+space").is_err());
    }

    #[test]
    fn parse_empty_is_rejected() {
        assert!(parse("").is_err());
    }

    #[test]
    fn parse_order_independent() {
        let a = parse("ctrl+shift+a").unwrap();
        let b = parse("shift+ctrl+a").unwrap();
        assert_eq!(a.mods.ctrl, b.mods.ctrl);
        assert_eq!(a.mods.shift, b.mods.shift);
        assert_eq!(a.key, b.key);
    }

    #[test]
    fn parse_produces_spec_for_known_combinations() {
        for combo in ["ctrl+shift+space", "alt+q", "super+f1"] {
            assert!(parse(combo).is_ok(), "should parse {combo}");
        }
    }
}
