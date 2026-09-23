//! Clipboard copy + paste pipeline.
//!
//! Public entry points:
//! - [`copy_to_clipboard`] / [`read_clipboard_text`] — thin shim over `tauri-plugin-clipboard-manager`.
//! - [`paste_text`] — top-level orchestrator: copy → (Windows: focus-restore +
//!   modifier-release; macOS: wait for the hotkey modifiers to come up) →
//!   3-strategy paste fallback. Each strategy runs only
//!   if the previous one reported an error — every extra attempt is a
//!   duplicate paste if the earlier one actually worked.
//! - `paste_strategy_1_enigo` — the enigo path, non-Windows ONLY.
//!
//! Windows deliberately does not use enigo for the paste keystroke:
//! `Key::Unicode('v')` is resolved through the foreground window's
//! keyboard layout and breaks under a non-Latin one. Windows goes through
//! `windows_util::send_ctrl_v_sendinput` (raw virtual-key codes) instead —
//! that function's doc comment has the full story.
//!
//! The paste pipeline is cross-platform with a Windows-only 3-strategy
//! fallback that mitigates UIPI blocks. macOS falls back to AppleScript.
//! Deliveries go through [`run_delivery`], which picks the thread each
//! platform needs.

#[cfg(any(not(windows), test))]
use enigo::{
    Direction::{Press, Release},
    Key, Keyboard,
};
#[cfg(not(windows))]
use enigo::{Enigo, Settings};

use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[cfg(any(target_os = "macos", test))]
const MAC_V_KEYCODE: u16 = 0x09;

pub fn clear_target() {
    #[cfg(windows)]
    crate::windows_util::clear_captured_hwnd();
}

pub fn capture_target() {
    #[cfg(windows)]
    {
        let _ = crate::windows_util::capture_target_hwnd();
    }
}

/// What happens to the final text once the pipeline has produced it.
///
/// Kept separate from [`paste_text`], which stays a pure "get these
/// characters into the focused window" primitive with its own fallback
/// chain. This struct is the policy layer on top.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeliveryOptions {
    /// Paste into the focused window. When off, the text is only copied —
    /// the clipboard is the delivery mechanism and the user pastes it
    /// themselves.
    pub auto_paste: bool,
    /// Append a space, so dictating several times in a row does not glue
    /// the results together.
    pub trailing_space: bool,
    /// Press Enter after the paste: sends the message in a chat, runs the
    /// query in a search box. Off by default — a stray Enter sends
    /// something half-finished, which is not an error you can take back.
    pub auto_submit: bool,
}

impl Default for DeliveryOptions {
    fn default() -> Self {
        Self {
            auto_paste: true,
            trailing_space: false,
            auto_submit: false,
        }
    }
}

impl DeliveryOptions {
    pub fn from_config(config: &serde_json::Value) -> Self {
        let flag = |key: &str, default: bool| {
            config
                .get(key)
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(default)
        };
        Self {
            auto_paste: flag("auto_paste", true),
            trailing_space: flag("paste_trailing_space", false),
            auto_submit: flag("paste_auto_submit", false),
        }
    }
}

/// Apply `options` to `text`: the text the user actually receives.
///
/// Separate from [`deliver`] so the transformation is testable without a
/// live `AppHandle`.
pub fn apply_delivery_text(text: &str, options: DeliveryOptions) -> String {
    if options.trailing_space && !text.is_empty() {
        format!("{text} ")
    } else {
        text.to_string()
    }
}

/// Run a delivery job on the thread the platform needs, one at a time.
///
/// On Windows the clipboard, focus restore and `SendInput` work from any
/// thread, and a paste waits up to a quarter of a second on the clipboard
/// and the target window; on the main thread every window of the app froze
/// for that long. macOS keeps the main thread, where enigo's keyboard
/// lookups have to run.
pub fn run_delivery<R: Runtime>(
    app: &AppHandle<R>,
    job: impl FnOnce() + Send + 'static,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        let _ = app;
        spawn_serialized(job)
    }
    #[cfg(not(windows))]
    {
        app.run_on_main_thread(job)
            .map_err(|error| error.to_string())
    }
}

/// Start `job` on its own thread once every earlier job has finished.
///
/// The main thread used to serialize deliveries; two pastes racing for the
/// clipboard and the focus would interleave their text.
#[cfg(windows)]
fn spawn_serialized(job: impl FnOnce() + Send + 'static) -> Result<(), String> {
    static DELIVERY: std::sync::Mutex<()> = std::sync::Mutex::new(());
    std::thread::Builder::new()
        .name("delivery".into())
        .spawn(move || {
            let _serial = crate::mutex_recover::lock(&DELIVERY);
            job();
        })
        .map(|_| ())
        .map_err(|error| format!("spawn delivery thread: {error}"))
}

/// Hand the finished text to the user according to `options`.
///
/// Runs inside [`run_delivery`], like [`paste_text`].
pub fn deliver(app: AppHandle, text: String, options: DeliveryOptions) -> Result<(), String> {
    let text = apply_delivery_text(&text, options);
    if !options.auto_paste {
        let result = copy_to_clipboard(&app, &text);
        // A hotkey-started session leaves its target HWND behind until the
        // delivery step consumes it. Copy-only mode consumes the session too:
        // keeping that HWND would let a later tray/button recording paste into
        // the window captured by an older session.
        #[cfg(windows)]
        crate::windows_util::clear_captured_hwnd();
        return result;
    }
    paste_text(app, text)?;
    if options.auto_submit {
        // Failure to submit is not a failure to deliver — the text is in
        // the window either way, and reporting an error here would make the
        // caller treat a successful dictation as a failed one.
        if let Err(error) = send_submit_key() {
            log::warn!("auto-submit failed: {error}");
        }
    }
    Ok(())
}

/// Press Enter in the focused window. No-op off Windows: the paste
/// pipeline's fallbacks are Windows-only too, and a submit that fires on
/// some platforms and not others is worse than one that never does.
fn send_submit_key() -> Result<(), String> {
    #[cfg(windows)]
    {
        // Give the target a moment to consume the paste keystroke before
        // the Enter lands on top of it. Same order of magnitude as the
        // focus-restore sleep in `paste_text`.
        std::thread::sleep(std::time::Duration::from_millis(30));
        crate::windows_util::send_enter_sendinput()
    }
    #[cfg(not(windows))]
    {
        Err("auto-submit is only implemented on Windows".to_string())
    }
}

/// Copy `text` into the system clipboard via `tauri-plugin-clipboard-manager`.
///
/// This is the only public writer of the clipboard from Rust: every paste
/// strategy first copies the text and then triggers a paste keystroke (so a
/// paste that loses the keystroke still leaves the text available for the
/// user to retry manually).
pub fn copy_to_clipboard<R: Runtime>(app: &AppHandle<R>, text: &str) -> Result<(), String> {
    app.clipboard()
        .write_text(text.to_string())
        .map_err(|e| format!("clipboard write failed: {e}"))
}

/// Paste Strategy 1 — enigo SendInput. NON-WINDOWS ONLY; Windows uses
/// `windows_util::send_ctrl_v_sendinput` because enigo's key lookup goes
/// through the active keyboard layout and misfires under a non-Latin one.
///
/// On macOS the modifier is `Key::Meta` (Cmd); elsewhere it is `Key::Control`.
/// macOS uses physical keycode 9 so the shortcut also works in Cyrillic layouts.
///
/// Runs on the main thread, scheduled by [`run_delivery`].
///
/// Success/failure is decided by the paste TRIGGER — the modifier+V
/// key-DOWN. Once those land, the target application has received the
/// paste, so we report `Ok` even if the subsequent key-UP calls error.
/// This matters because the caller (`paste_text`) escalates to a second
/// paste strategy on `Err`: if we failed the whole call just because a
/// key-release glitched AFTER the paste already registered, that second
/// strategy would paste the text a SECOND time (the intermittent
/// "text inserted twice" bug). The releases are still attempted
/// best-effort, and `release_stuck_modifiers()` in the caller cleans up
/// any key left down.
#[cfg(not(windows))]
pub fn paste_strategy_1_enigo() -> Result<(), String> {
    let settings = Settings {
        independent_of_keyboard_state: true,
        ..Settings::default()
    };
    let mut enigo = Enigo::new(&settings).map_err(|e| format!("enigo init failed: {e}"))?;

    // Platform-specific modifier: Ctrl on Windows/Linux, Meta (Cmd) on macOS.
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    #[cfg(target_os = "macos")]
    let physical_v = Some(MAC_V_KEYCODE);
    #[cfg(not(target_os = "macos"))]
    let physical_v = None;
    send_paste_shortcut(&mut enigo, modifier, physical_v)
}

/// enigo resolves Unicode through the current macOS layout and returns
/// keycode 0 (A) when it cannot find Latin 'v'. That reports success for Cmd+A,
/// preventing fallback. A shortcut uses the physical V key instead.
///
/// Still true in 0.6.1, which is worth recording so the bump does not read as
/// having made this removable: `get_layoutdependent_keycode` initialises the
/// keycode to 0 and returns it unchanged when no keycode in 0..128 produces
/// the string it was asked for.
#[cfg(any(not(windows), test))]
fn send_paste_shortcut(
    enigo: &mut impl Keyboard,
    modifier: Key,
    physical_v: Option<u16>,
) -> Result<(), String> {
    let v = |keyboard: &mut _, direction| match physical_v {
        Some(code) => Keyboard::raw(keyboard, code, direction),
        None => Keyboard::key(keyboard, Key::Unicode('v'), direction),
    };

    // Key-DOWN is the paste trigger. If either of these fails, the paste
    // did NOT happen — return Err so the caller escalates (no duplicate
    // risk, because nothing was pasted). Release the modifier best-effort
    // first so we don't leave it stuck.
    enigo.key(modifier, Press).map_err(|e| {
        let _ = enigo.key(modifier, Release);
        format!("enigo modifier press: {e}")
    })?;
    if let Err(e) = v(enigo, Press) {
        let _ = v(enigo, Release);
        let _ = enigo.key(modifier, Release);
        return Err(format!("enigo v press: {e}"));
    }

    // Paste has been delivered. Attempt the key-UPs best-effort; a failure
    // here must NOT propagate, or the caller would paste a second time.
    let _ = v(enigo, Release);
    let _ = enigo.key(modifier, Release);
    Ok(())
}

/// Physical modifiers that corrupt a synthetic Cmd+V.
///
/// `CGEventFlags` bits. Caps Lock and the numeric-pad flag are excluded
/// deliberately: neither changes what Cmd+V means, and Caps Lock is a
/// latch the user may simply leave on forever.
#[cfg(any(target_os = "macos", test))]
const BLOCKING_MODIFIER_FLAGS: u64 = 0x0002_0000   // Shift
    | 0x0004_0000                                  // Control
    | 0x0008_0000                                  // Option
    | 0x0010_0000                                  // Command
    | 0x0080_0000; // Fn

#[cfg(any(target_os = "macos", test))]
fn has_blocking_modifiers(flags: u64) -> bool {
    flags & BLOCKING_MODIFIER_FLAGS != 0
}

/// Currently held physical modifiers, as the window server sees them.
///
/// `kCGEventSourceStateCombinedSessionState` (0) is the real keyboard
/// state — the state merged into an event injected at the HID tap, which
/// is what makes a held Option turn our Cmd+V into Cmd+Option+V.
///
/// Safety: `CGEventSourceFlagsState` is a C function taking a
/// `CGEventSourceStateID` (an `int32_t` enum) and returning
/// `CGEventFlags` (a `uint64_t`). It reads global state, is callable from
/// any thread, and has no side effects observable from Rust.
#[cfg(target_os = "macos")]
fn current_modifier_flags() -> u64 {
    const COMBINED_SESSION_STATE: i32 = 0;
    extern "C" {
        fn CGEventSourceFlagsState(state: i32) -> u64;
    }
    unsafe { CGEventSourceFlagsState(COMBINED_SESSION_STATE) }
}

/// Poll `flags` until no blocking modifier is held, giving up after
/// `attempts` sleeps. Returns the last flags observed and how many times
/// it slept, so the caller can report an actual wait.
///
/// Split from the polling constants and the FFI so the loop is testable
/// without a keyboard.
#[cfg(any(target_os = "macos", test))]
fn wait_for_modifier_release(
    mut flags: impl FnMut() -> u64,
    attempts: u32,
    mut sleep: impl FnMut(),
) -> (u64, u32) {
    let mut current = flags();
    for waited in 0..attempts {
        if !has_blocking_modifiers(current) {
            return (current, waited);
        }
        sleep();
        current = flags();
    }
    (current, attempts)
}

/// Wait out the hotkey that ended the dictation before pasting.
///
/// The paste fires immediately after the keystroke that stopped
/// recording, and that keystroke needs a modifier — `hotkey.rs` rejects a
/// combination without one. A Cmd+V posted while Option is still down
/// reaches the target as Cmd+Option+V, which pastes in no application:
/// the text silently never appears, while every layer below reports
/// success. Windows fixes the same problem by forcing the keys up with
/// `release_stuck_modifiers`; macOS has no such call, so we wait for the
/// user to let go.
///
/// Timing out is not a reason to abort. The paste may still land, and
/// refusing to send it guarantees that it does not.
#[cfg(target_os = "macos")]
fn settle_modifiers() {
    // 25 × 10ms. Letting go of a key takes well under that, and this runs
    // on the main thread, so the ceiling stays inside the same budget the
    // Windows branch already spends on `wait_for_clipboard_write`.
    const ATTEMPTS: u32 = 25;
    const POLL: std::time::Duration = std::time::Duration::from_millis(10);

    let (flags, waited) = wait_for_modifier_release(current_modifier_flags, ATTEMPTS, || {
        std::thread::sleep(POLL)
    });
    if has_blocking_modifiers(flags) {
        log::warn!(
            "modifiers still held after {}ms (flags {flags:#x}), pasting anyway",
            ATTEMPTS as u128 * POLL.as_millis()
        );
    } else if waited > 0 {
        log::info!(
            "waited {}ms for the hotkey modifiers to be released before pasting",
            waited as u128 * POLL.as_millis()
        );
    }
}

/// Paste Strategy 2 — macOS-only osascript (Cmd+V via AppleScript).
///
/// Falls back to this when enigo fails. `osascript` uses the system
/// Accessibility API through AppleScript's `key code` command, which
/// can work even when direct CGEvent posting is blocked.
#[cfg(target_os = "macos")]
pub fn paste_strategy_2_osascript() -> Result<(), String> {
    use std::process::Command;
    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to key code 9 using command down"#,
        ])
        .output()
        .map_err(|e| format!("osascript failed: {e}"))?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("osascript paste failed: {stderr}"))
    }
}

/// Top-level paste entry point. Dispatches to the platform-appropriate
/// pipeline. Always called from the main/UI thread.
///
/// On Windows: copy → write-readback → focus-restore + modifier-release →
/// Strategies 1→2→3, escalating ONLY when a strategy reports an error.
/// There is no way to confirm from outside the target application that a
/// paste landed, so a strategy that reports success is taken at its word;
/// guessing wrong in the other direction pastes the text twice.
/// On macOS: copy → modifier settle → enigo (Strategy 1) → osascript
/// (Strategy 2).
/// On Linux: copy + Strategy 1 alone.
///
/// The caller runs this inside [`run_delivery`].
pub fn paste_text(app: AppHandle, text: String) -> Result<(), String> {
    copy_to_clipboard(&app, &text)?;

    // CGEventPost can report success while macOS silently rejects the keys.
    // Check after copying so denied access still leaves a usable transcript.
    #[cfg(target_os = "macos")]
    if !crate::accessibility::is_accessibility_granted() {
        crate::accessibility::emit_accessibility_error(&app);
        return Err(crate::ui_text::t(
            "Текст скопирован. Нажмите ⌘V для вставки. Для автоматической вставки разрешите Sotto доступ в настройках универсального доступа macOS.",
        ));
    }

    #[cfg(windows)]
    {
        use crate::windows_util::{
            clear_captured_hwnd, force_focus, get_captured_hwnd, release_stuck_modifiers,
            send_ctrl_v_sendinput,
        };
        let hwnd = get_captured_hwnd();

        // Even without a captured hwnd (e.g. recording started via the
        // in-app button rather than a global hotkey press), we still try
        // Strategy 1 — the modifier+keystroke will reach whichever window
        // happens to be focused.
        let Some(h) = hwnd else {
            return send_ctrl_v_sendinput();
        };

        // Confirm our OWN clipboard write landed before sending any
        // keystroke — otherwise a slow write races the paste and the
        // target receives whatever the user copied previously. Not being
        // able to confirm it is not a reason to abort: the read can fail
        // purely because another process holds the clipboard open.
        if !wait_for_clipboard_write(&text, 200) {
            log::warn!("clipboard write unconfirmed after 200ms, pasting anyway");
        }

        if let Err(error) = force_focus(h) {
            clear_captured_hwnd();
            return Err(error);
        }
        let _ = release_stuck_modifiers();
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Strategy 1: raw-VK SendInput. NOT enigo — see
        // `send_ctrl_v_sendinput` for why enigo cannot do this under a
        // non-Latin keyboard layout.
        //
        // Escalation is driven by the strategy's OWN error, never by a
        // clipboard readback. `SendInput` reports failure when UIPI blocks
        // the injection, which is the case the fallbacks exist for; if it
        // reports success the keystroke is in the input queue and sending
        // a second one would paste the text twice.
        if let Err(error) = send_ctrl_v_sendinput() {
            log::warn!("paste strategy 1 (SendInput) failed: {error}");
        } else {
            let _ = release_stuck_modifiers();
            clear_captured_hwnd();
            return Ok(());
        }

        // Strategy 2: keybd_event legacy fallback.
        if crate::windows_util::send_ctrl_v_keybd_event().is_ok() {
            let _ = release_stuck_modifiers();
            clear_captured_hwnd();
            return Ok(());
        }

        // Strategy 3: WM_PASTE directly into the captured HWND.
        if crate::windows_util::send_wm_paste(h).is_ok() {
            let _ = release_stuck_modifiers();
            clear_captured_hwnd();
            return Ok(());
        }

        let _ = release_stuck_modifiers();
        clear_captured_hwnd();
        Err("all three paste strategies failed".into())
    }

    #[cfg(not(windows))]
    {
        // The hotkey that stopped the dictation is still down. Send the
        // shortcut only once its modifiers are up — see `settle_modifiers`.
        #[cfg(target_os = "macos")]
        settle_modifiers();

        // Strategy 1: enigo SendInput (cross-platform).
        match paste_strategy_1_enigo() {
            Ok(()) => return Ok(()),
            Err(error) => log::warn!("paste strategy 1 (enigo) failed: {error}"),
        }

        // Strategy 2: macOS osascript fallback.
        #[cfg(target_os = "macos")]
        {
            match paste_strategy_2_osascript() {
                Ok(()) => return Ok(()),
                Err(error) => log::warn!("paste strategy 2 (osascript) failed: {error}"),
            }
        }

        // All strategies failed. Check if it's an Accessibility issue.
        #[cfg(target_os = "macos")]
        if !crate::accessibility::is_accessibility_granted() {
            crate::accessibility::emit_accessibility_error(&app);
        }

        Err("all paste strategies failed".into())
    }
}

/// Poll for up to `timeout_ms` ms until the clipboard holds `expected`,
/// i.e. until our own `copy_to_clipboard` write is observable. Returns
/// whether it was confirmed.
///
/// This ran AFTER each paste keystroke until it was found to be the cause
/// of the duplicate-paste bug. The reasoning was that a readback matching
/// `expected` proved the keystroke had been accepted — but Ctrl+V does
/// not modify the clipboard, so the readback matches whether or not the
/// paste landed. The only way it could return `false` was
/// `OpenClipboard` failing, which is exactly what happens while the paste
/// target has the clipboard open to read the data we just pasted. A
/// successful paste therefore reported failure, the caller escalated, and
/// the next strategy pasted the same text a second time.
///
/// Verifying a write we performed ourselves is the one thing a readback
/// can legitimately prove, so that is all it is used for now. A `false`
/// return is not fatal: the write may well have succeeded with the reads
/// merely blocked, so the caller proceeds regardless.
///
/// Polling is intentionally short (≤200ms in production) to keep the
/// paste pipeline latency below human perception.
#[cfg(windows)]
fn wait_for_clipboard_write(expected: &str, timeout_ms: u64) -> bool {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < deadline {
        if crate::windows_util::clipboard_contains(expected) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// Test the paste pipeline from the frontend.
/// Copies test text to the clipboard and pastes it through `paste_text`,
/// the same path a dictation takes.
#[tauri::command]
pub(crate) async fn test_paste(app: AppHandle) -> Result<String, String> {
    let test_text = "Тест вставки Sotto — ".to_owned()
        + &std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis().to_string())
            .unwrap_or_default();

    let (reply, result) = tokio::sync::oneshot::channel();
    let paste_app = app.clone();
    let paste_text = test_text.clone();
    run_delivery(&app, move || {
        let _ = reply.send(crate::clipboard::paste_text(paste_app, paste_text));
    })?;
    match result
        .await
        .map_err(|_| "paste test worker dropped reply".to_string())?
    {
        Ok(()) => Ok(format!("Paste OK. Text на буфере: {test_text}")),
        Err(e) => Err(format!("Paste FAILED: {e}")),
    }
}

#[cfg(test)]
mod delivery_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_paste_without_space_or_submit() {
        let options = DeliveryOptions::from_config(&json!({}));
        assert_eq!(options, DeliveryOptions::default());
        assert!(options.auto_paste);
        assert!(!options.trailing_space);
        assert!(!options.auto_submit);
    }

    #[test]
    fn reads_every_flag_from_config() {
        let options = DeliveryOptions::from_config(&json!({
            "auto_paste": false,
            "paste_trailing_space": true,
            "paste_auto_submit": true,
        }));
        assert!(!options.auto_paste);
        assert!(options.trailing_space);
        assert!(options.auto_submit);
    }

    #[test]
    fn trailing_space_is_appended_once() {
        let options = DeliveryOptions {
            trailing_space: true,
            ..DeliveryOptions::default()
        };
        assert_eq!(apply_delivery_text("привет", options), "привет ");
    }

    #[test]
    fn trailing_space_skips_empty_text() {
        // A lone space is not a dictation result.
        let options = DeliveryOptions {
            trailing_space: true,
            ..DeliveryOptions::default()
        };
        assert_eq!(apply_delivery_text("", options), "");
    }

    #[test]
    fn text_is_untouched_without_the_option() {
        assert_eq!(
            apply_delivery_text("привет", DeliveryOptions::default()),
            "привет"
        );
    }
    #[derive(Default)]
    struct TestKeyboard {
        command: bool,
        pasted: usize,
        fail_press: bool,
        fail_release: bool,
    }
    impl Keyboard for TestKeyboard {
        fn fast_text(&mut self, _: &str) -> enigo::InputResult<Option<()>> {
            panic!("paste must use a shortcut");
        }
        fn key(&mut self, key: Key, direction: enigo::Direction) -> enigo::InputResult<()> {
            assert_eq!(
                key,
                Key::Meta,
                "Latin key lookup is invalid in a Cyrillic layout"
            );
            self.command = direction == Press;
            Ok(())
        }
        fn raw(&mut self, code: u16, direction: enigo::Direction) -> enigo::InputResult<()> {
            assert_eq!(code, MAC_V_KEYCODE);
            if direction == Press {
                if self.fail_press {
                    return Err(enigo::InputError::Simulate("press failed"));
                }
                assert!(self.command);
                self.pasted += 1;
            } else if self.fail_release {
                return Err(enigo::InputError::Simulate("release failed"));
            }
            Ok(())
        }
    }

    #[test]
    fn mac_paste_uses_one_physical_shortcut_and_releases_command() {
        let mut keyboard = TestKeyboard::default();
        send_paste_shortcut(&mut keyboard, Key::Meta, Some(MAC_V_KEYCODE)).unwrap();
        assert_eq!(keyboard.pasted, 1);
        assert!(!keyboard.command);
    }

    #[test]
    fn failed_key_release_after_paste_must_not_trigger_a_second_paste() {
        let mut keyboard = TestKeyboard {
            fail_release: true,
            ..Default::default()
        };
        assert!(send_paste_shortcut(&mut keyboard, Key::Meta, Some(MAC_V_KEYCODE)).is_ok());
        assert_eq!(keyboard.pasted, 1);
        assert!(!keyboard.command);
    }

    #[test]
    fn a_held_option_blocks_the_paste_shortcut() {
        // The hotkey needs a modifier, so one is always down when the
        // dictation ends. Cmd+Option+V pastes in no application.
        assert!(has_blocking_modifiers(0x0008_0000));
    }

    #[test]
    fn caps_lock_and_numeric_pad_do_not_block_the_paste() {
        // A latch the user may leave on forever, and a flag that merely
        // marks which key was pressed. Neither changes what Cmd+V means.
        assert!(!has_blocking_modifiers(0x0001_0000 | 0x0020_0000));
    }

    #[test]
    fn paste_waits_until_the_hotkey_modifiers_come_up() {
        // Option held for the first two polls, released on the third.
        let readings = std::cell::Cell::new(0);
        let flags = || {
            let n = readings.get();
            readings.set(n + 1);
            if n < 2 {
                0x0008_0000
            } else {
                0
            }
        };
        let mut slept = 0;
        let (final_flags, waited) = wait_for_modifier_release(flags, 40, || slept += 1);
        assert_eq!(final_flags, 0);
        assert_eq!(waited, 2);
        assert_eq!(slept, 2);
    }

    #[test]
    fn paste_is_not_abandoned_when_a_modifier_stays_down() {
        // Giving up on the paste guarantees the text never arrives; sending
        // it anyway at least leaves the chance that it does.
        let mut slept = 0;
        let (final_flags, waited) = wait_for_modifier_release(|| 0x0010_0000, 3, || slept += 1);
        assert!(has_blocking_modifiers(final_flags));
        assert_eq!(waited, 3);
        assert_eq!(slept, 3);
    }

    #[test]
    fn paste_does_not_wait_when_no_modifier_is_held() {
        let mut slept = 0;
        let (final_flags, waited) = wait_for_modifier_release(|| 0, 40, || slept += 1);
        assert_eq!((final_flags, waited), (0, 0));
        assert_eq!(slept, 0);
    }

    #[test]
    fn failed_paste_press_releases_command_and_allows_fallback() {
        let mut keyboard = TestKeyboard {
            fail_press: true,
            ..Default::default()
        };
        assert!(send_paste_shortcut(&mut keyboard, Key::Meta, Some(MAC_V_KEYCODE)).is_err());
        assert_eq!(keyboard.pasted, 0);
        assert!(!keyboard.command);
    }
}

#[cfg(all(test, windows))]
mod delivery_thread_tests {
    use super::spawn_serialized;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn deliveries_run_one_at_a_time_off_the_calling_thread() {
        let caller = std::thread::current().id();
        let running = Arc::new(AtomicUsize::new(0));
        let (done, finished) = mpsc::channel();
        for _ in 0..3 {
            let (running, done) = (running.clone(), done.clone());
            spawn_serialized(move || {
                let overlapping = running.fetch_add(1, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(30));
                running.fetch_sub(1, Ordering::SeqCst);
                done.send((overlapping, std::thread::current().id()))
                    .unwrap();
            })
            .unwrap();
        }
        for _ in 0..3 {
            let (overlapping, thread) = finished.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(overlapping, 0, "another delivery was still running");
            assert_ne!(thread, caller);
        }
    }
}
