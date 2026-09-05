//! Hotkey string parsing and runtime registration.
//!
//! The parser takes strings like `"ctrl+shift+a"` and produces a `HotkeySpec`
//! that downstream code can hand to `tauri-plugin-global-shortcut`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
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

impl HotkeySpec {
    #[allow(dead_code)] // only consumed by tests (key_name is not called from non-test code)
    pub fn key_name(&self) -> &str {
        &self.key
    }
}

/// Parse a hotkey string like `"ctrl+shift+a"` into a [`HotkeySpec`].
///
/// Tokens are split on `+`. The last token is the key (preserved as-typed
/// after `.to_lowercase()`); all preceding tokens are modifiers. Aliases:
/// `ctrl`/`control`, `alt`, `shift`, `cmd`/`super`/`win`/`meta` (all map to
/// `meta`). Unknown modifiers and empty input are rejected with an `Err`.
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
    if parts.len() < 2 {
        return Err(format!(
            "hotkey must contain at least one modifier and one key, got: {hotkey:?}"
        ));
    }
    let key = parts.last().unwrap().to_lowercase();
    // Reject when the key token is itself a modifier name — e.g. "ctrl+shift"
    // has 2 tokens but no actual key.
    match key.as_str() {
        "ctrl" | "control" | "alt" | "shift" | "cmd" | "super" | "win" | "meta" => {
            return Err(format!(
                "hotkey must contain a non-modifier key, got modifier-only: {hotkey:?}"
            ));
        }
        _ => {}
    }
    let mut mods = Modifiers::default();
    for token in &parts[..parts.len() - 1] {
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
    Ok(HotkeySpec { mods, key })
}

// ---- runtime registration ----

use tauri::AppHandle;
use tauri::Emitter;
use tauri::Manager;
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
/// **WS 4a1 Task 13b**: the closure captures `state.clone()` (cheap —
/// every field is `Arc`-backed) instead of a `SidecarHandle`. The hotkey
/// press now sends an `EngineCommand::Transcribe` straight into the
/// engine thread, bypassing the Python sidecar's `start_recording` RPC.
pub fn register(app: &AppHandle, state: &AppState, hotkey: &str) -> Result<(), String> {
    let spec = parse(hotkey)?;
    let shortcut = to_shortcut(&spec)?;
    let state_for_handler = state.clone();
    // WS 4a2b Task 7: capture the AppHandle so the handler can emit
    // recording-started/stopped + hotkey-error events directly. Prior to
    // this the handler had no way to talk to the frontend (the previous
    // code shipped with _app: AppHandle discarded).
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
/// `old == new` is a re-bind of the same combination, and there the old order
/// is the only one that works: the plugin refuses to register a shortcut it
/// already holds.
pub fn re_register(app: &AppHandle, state: &AppState, old: &str, new: &str) -> Result<(), String> {
    if old == new {
        let _ = unregister(app, old);
        return register(app, state, new);
    }
    register(app, state, new)?;
    if !old.is_empty() {
        let _ = unregister(app, old);
    }
    Ok(())
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
/// **WS 4a2 (Task 9)** introduced the push-to-talk toggle with cpal audio
/// capture; **WS 4a2b (Task 7)** wires that pipeline through
/// `state.recorder.start()`/`stop()` and emits frontend events from the
/// AppHandle so the React TrayApp sees the same `recording-started` /
/// `recording-stopped` / `hotkey-error` events as the Tauri command path.
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
    // Reading the small JSON file on each event is negligible (~50us) and
    // avoids the complexity of syncing a cached atomic into AppState.
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

/// Start recording: arm the cpal stream, allocate a session_id, store it
/// in the shared AtomicU64, and emit `recording-started`.
///
/// # Threading
///
/// This runs on the **main thread**: `global-hotkey` dispatches `Pressed`
/// inline from the WndProc of a message window it created during setup
/// (`global-hotkey-0.8.0/src/platform_impl/windows/mod.rs:146`). Opening a
/// WASAPI device here blocked the UI thread for as long as the audio stack
/// took to answer — sometimes forever. Everything past the foreground-HWND
/// snapshot therefore runs on the audio worker, fire-and-forget.
fn hotkey_do_start(app: &AppHandle, state: &AppState) {
    // The engine runs one job at a time, and a dictation with no
    // transcription route would end in an error nobody asked for after an
    // overlay that promised dictation. Both refusals — and their telemetry —
    // are shared with the `start_recording` command.
    //
    // Clearing `toggle_armed` matters because the toggle branch sets it
    // before calling us: leaving it armed would make the next press try to
    // stop a recording that never began.
    if let Some(refusal) = crate::refuse_dictation_start(app, state) {
        state.toggle_armed.store(false, Ordering::Release);
        log::info!("hotkey: start refused — {}", refusal.message);
        // `no_transcription_route_message` emits `hotkey-error` as it builds
        // the text; the busy refusal does not, and the hotkey has no other
        // way to reach the user.
        if matches!(refusal.reason, crate::telemetry::FailureReason::EngineBusy) {
            let _ = app.emit("hotkey-error", &refusal.message);
        }
        return;
    }
    // On Windows, snapshot the foreground HWND at the moment of press so
    // the paste pipeline can restore focus after the recording session
    // ends. Must stay on this thread: it reads the foreground window as it
    // is *now*, before anything else can steal focus. macOS does not need
    // this.
    #[cfg(windows)]
    {
        let _ = crate::windows_util::capture_target_hwnd();
    }
    // Allocate and publish the session id *synchronously*, before the
    // worker has opened the device. A push-to-talk tap shorter than the
    // device-open latency releases the key while the start job is still
    // queued; if the id were only published by that job, the release would
    // find `current_session_id == 0`, bail out, and leave the recorder
    // running with no way to stop it.
    let session_id = state.next_session_id();
    state.begin_session(session_id);
    state
        .current_session_id
        .store(session_id, Ordering::Release);
    let app = app.clone();
    let recorder = Arc::clone(&state.recorder);
    state.audio.submit(move || {
        // Re-acquired here rather than captured: `AppState` lives in
        // Tauri's managed state, which hands out borrows, not owned
        // handles.
        let state = app.state::<AppState>();
        let selected = crate::config::microphone_selection(
            crate::config::Config::load(&app)
                .ok()
                .and_then(|c| c.get("microphone")),
        );
        if let Err(e) = recorder.start_selected(selected.as_deref()) {
            log::error!("hotkey: recorder.start failed: {e}");
            // Retract the id, but only if it is still ours — a newer press
            // may already have published its own.
            let _ = state.current_session_id.compare_exchange(
                session_id,
                0,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            state.finish_session(session_id);
            let _ = app.emit("hotkey-error", &format!("start: {e}"));
            crate::record_recorder_start_failure(&app);
            return;
        }
        // Drive the overlay waveform: emit `audio-level` ~30 Hz until the
        // recorder stops. Same path the UI `start_recording` command uses.
        crate::spawn_level_emitter(&app, Arc::clone(&recorder));
        *crate::mutex_recover::lock(&state.app_fsm) = crate::state::AppFsm::Recording;
        crate::on_recording_started(&app);
        app.state::<crate::telemetry::Telemetry>()
            .begin_usage_session(crate::telemetry::SessionTrigger::Microphone);
        let _ = app.emit("recording-started", session_id);
    });
}

/// Stop recording: swap(0) the stored session_id, drop the cpal stream,
/// forward the captured audio as `EngineCommand::Transcribe`, and emit
/// `recording-stopped`.
///
/// # Threading
///
/// Reached from two threads: the poller `global-hotkey` spawns per press
/// (push-to-talk release) and the **main thread** (toggle mode's second
/// press is another `Pressed`). Dropping the cpal stream `join()`s its
/// audio thread, so that part goes to the audio worker. The session
/// bookkeeping above it stays inline — those are plain atomics, and
/// keeping them synchronous preserves the ordering the toggle handler
/// relies on.
fn hotkey_do_stop(app: &AppHandle, state: &AppState) {
    // Defensively reset the toggle arm in case this stop path was
    // reached outside the toggle handler (push-to-talk release, or
    // any future caller). The toggle handler already clears it before
    // calling hotkey_do_stop, so this is a harmless no-op there.
    state.toggle_armed.store(false, Ordering::Release);

    let session_id = state.current_session_id.swap(0, Ordering::AcqRel);
    if session_id == 0 {
        log::warn!("hotkey stop without active session_id");
        return;
    }
    let app = app.clone();
    let recorder = Arc::clone(&state.recorder);
    state.audio.submit(move || {
        let state = app.state::<AppState>();
        let config = crate::config::Config::load(&app).ok();
        let audio = match recorder.stop() {
            Ok(Some(a)) if !a.is_empty() => a,
            // `abandon_dictation` also returns the FSM to Idle: nothing
            // downstream would, because the engine is never handed a command
            // and the dispatcher never runs for this session.
            Ok(_) => {
                log::info!("hotkey: empty audio, skip transcription");
                if state.is_cancelled(session_id) {
                    state.finish_session(session_id);
                    let _ = app.emit("whisper-cancelled", session_id);
                    return;
                }
                let _ = app.emit("hotkey-error", "no audio captured");
                crate::abandon_dictation(
                    &app,
                    &state,
                    session_id,
                    crate::telemetry::FailureReason::NoAudio,
                );
                state.finish_session(session_id);
                return;
            }
            Err(e) => {
                log::error!("hotkey: recorder.stop failed: {e}");
                if state.is_cancelled(session_id) {
                    state.finish_session(session_id);
                    let _ = app.emit("whisper-cancelled", session_id);
                    return;
                }
                let _ = app.emit("hotkey-error", &format!("stop: {e}"));
                crate::abandon_dictation(
                    &app,
                    &state,
                    session_id,
                    crate::telemetry::FailureReason::RecorderStop,
                );
                state.finish_session(session_id);
                return;
            }
        };
        // The overlay can cancel while the audio worker is stopping.  Keep
        // that marker authoritative before any stop hook, debug dump, or
        // engine command is constructed.
        if state.is_cancelled(session_id) {
            state.finish_session(session_id);
            let _ = app.emit("whisper-cancelled", session_id);
            return;
        }
        crate::on_recording_stopped(&app, session_id, Some(&audio));
        let cancel_flag = Arc::new(AtomicBool::new(false));
        state.register_cancel_flag(session_id, cancel_flag.clone());
        let (reply_tx, _reply_rx) = tokio::sync::oneshot::channel();
        let cmd = match crate::build_dictation_command(
            &app,
            config.as_ref(),
            session_id,
            audio,
            cancel_flag,
            reply_tx,
        ) {
            Ok(cmd) => cmd,
            Err(error) => {
                state.finish_session(session_id);
                let _ = app.emit("hotkey-error", &error);
                return;
            }
        };
        // `try_send`, so a full engine queue cannot park the audio worker.
        if let Err(e) = state.engine_cmd_tx.try_send(cmd) {
            log::warn!(
                "hotkey stop: engine channel closed or full ({e}); \
                 session {session_id} dropped"
            );
            let _ = app.emit("hotkey-error", &format!("engine: {e}"));
            crate::record_engine_queue_failure(&app, config.as_ref());
            state.finish_session(session_id);
            return;
        }
        *crate::mutex_recover::lock(&state.app_fsm) = crate::state::AppFsm::Processing;
        let _ = app.emit("recording-stopped", session_id);
    });
}

// `_ordering` is read by the dead-store lint to silence the unused import
// warning when neither side of the cfg uses `Ordering`. Safe to keep.
#[allow(dead_code)]
fn _ordering_silencer() -> Ordering {
    Ordering::SeqCst
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captured_keys_convert_to_native_shortcuts() {
        for key in [
            "2", "f12", "escape", "delete", "left", "pageup", "slash", "numpad2", "a",
        ] {
            assert!(
                to_shortcut(&parse(&format!("ctrl+shift+{key}")).unwrap()).is_ok(),
                "{key}"
            );
        }
        assert!(to_shortcut(&parse("ctrl+unknown").unwrap()).is_err());
    }

    #[test]
    fn parse_simple_modifier_plus_letter() {
        let s = parse("ctrl+shift+a").unwrap();
        assert!(s.mods.ctrl);
        assert!(s.mods.shift);
        assert!(!s.mods.alt);
        assert!(!s.mods.meta);
        assert_eq!(s.key_name(), "a");
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
        assert_eq!(s.key_name(), "я");
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
        assert_eq!(a.key_name(), b.key_name());
    }

    #[test]
    fn parse_produces_spec_for_known_combinations() {
        for combo in ["ctrl+shift+space", "alt+q", "super+f1"] {
            assert!(parse(combo).is_ok(), "should parse {combo}");
        }
    }
}
