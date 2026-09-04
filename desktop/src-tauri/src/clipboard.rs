//! Clipboard copy + paste pipeline.
//!
//! Public entry points:
//! - [`copy_to_clipboard`] / [`read_clipboard_text`] — thin shim over `tauri-plugin-clipboard-manager`.
//! - [`paste_text`] — top-level orchestrator: copy → (Windows: focus-restore +
//!   modifier-release) → 3-strategy paste fallback. Each strategy runs only
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
//! fallback that mitigates UIPI blocks. macOS uses Strategy 1 alone.
//! All paste operations must run on the main/UI thread (see
//! `sidecar::reader_loop`); enigo and the clipboard plugin are sensitive
//! to thread context.

#[cfg(not(windows))]
use enigo::{
    Direction::{Press, Release},
    Enigo, Key, Keyboard, Settings,
};
use tauri::{AppHandle, Runtime};
use tauri_plugin_clipboard_manager::ClipboardExt;

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

/// Hand the finished text to the user according to `options`.
///
/// Must run on the main/UI thread, like [`paste_text`].
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
/// `enigo::Key::Unicode('v')` is used for the V key — enigo handles the
/// physical-key mapping for the current keyboard layout under the hood.
///
/// Always runs on the main/UI thread (the caller schedules it via
/// `app.run_on_main_thread`).
///
/// `text` is ignored by the function body — by the time this is called,
/// `paste_text` has already written it to the clipboard. We keep the
/// parameter so the public signature matches the strategy-2/3 callers
/// uniformly and a future "type-then-Enter" variant is easy to add.
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
pub fn paste_strategy_1_enigo(text: &str) -> Result<(), String> {
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("enigo init failed: {e}"))?;

    // Platform-specific modifier: Ctrl on Windows/Linux, Meta (Cmd) on macOS.
    #[cfg(target_os = "macos")]
    let modifier = Key::Meta;
    #[cfg(not(target_os = "macos"))]
    let modifier = Key::Control;

    // Key-DOWN is the paste trigger. If either of these fails, the paste
    // did NOT happen — return Err so the caller escalates (no duplicate
    // risk, because nothing was pasted). Release the modifier best-effort
    // first so we don't leave it stuck.
    enigo.key(modifier, Press).map_err(|e| {
        let _ = enigo.key(modifier, Release);
        format!("enigo modifier press: {e}")
    })?;
    if let Err(e) = enigo.key(Key::Unicode('v'), Press) {
        let _ = enigo.key(Key::Unicode('v'), Release);
        let _ = enigo.key(modifier, Release);
        return Err(format!("enigo v press: {e}"));
    }

    // Paste has been delivered. Attempt the key-UPs best-effort; a failure
    // here must NOT propagate, or the caller would paste a second time.
    let _ = enigo.key(Key::Unicode('v'), Release);
    let _ = enigo.key(modifier, Release);
    let _ = text; // text already in clipboard via the caller
    Ok(())
}

/// Paste Strategy 2 — macOS-only osascript (Cmd+V via AppleScript).
///
/// Falls back to this when enigo fails. `osascript` uses the system
/// Accessibility API through AppleScript's `keystroke` command, which
/// can work even when direct CGEvent posting is blocked.
#[cfg(target_os = "macos")]
pub fn paste_strategy_2_osascript() -> Result<(), String> {
    use std::process::Command;
    let output = Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
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
/// On macOS: copy → enigo (Strategy 1) → osascript (Strategy 2).
/// On Linux: copy + Strategy 1 alone.
///
/// The caller is responsible for invoking this on the main thread (via
/// `app.run_on_main_thread`). `AppHandle` (not `&AppHandle`) so the inner
/// closures can `clone()` it cheaply when scheduling further main-thread
/// work (e.g. `copy_to_clipboard` from `sidecar.rs::reader_loop`).
pub fn paste_text(app: AppHandle, text: String) -> Result<(), String> {
    copy_to_clipboard(&app, &text)?;

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

        let _ = force_focus(h);
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

        // Strategy 2: keybd_event legacy fallback (added in Task 7).
        if crate::windows_util::send_ctrl_v_keybd_event().is_ok() {
            let _ = release_stuck_modifiers();
            clear_captured_hwnd();
            return Ok(());
        }

        // Strategy 3: WM_PASTE directly into captured HWND (added in Task 8).
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
        // Strategy 1: enigo SendInput (cross-platform).
        if let Ok(()) = paste_strategy_1_enigo(&text) {
            return Ok(());
        }

        // Strategy 2: macOS osascript fallback (more reliable on macOS 26+).
        #[cfg(target_os = "macos")]
        {
            if let Ok(()) = paste_strategy_2_osascript() {
                return Ok(());
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
}
