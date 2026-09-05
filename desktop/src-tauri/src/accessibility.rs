//! macOS Accessibility permission check.
//!
//! On macOS, simulating keyboard input (e.g. Cmd+V paste via enigo) requires
//! the **Accessibility** permission. Without it, `CGEventPost` (used by enigo
//! under the hood) silently fails and the paste doesn't reach the target app.
//!
//! This module provides a simple check via `AXIsProcessTrusted()` and a
//! convenience function to emit an `app-error` event when permission is
//! missing, so the frontend can show a dismissable banner with a deep-link
//! into System Settings → Privacy & Security → Accessibility.

#[cfg(target_os = "macos")]
use tauri::{AppHandle, Emitter};

/// Returns `true` if the current process has Accessibility permission
/// (macOS TCC), or `false` if the app needs to be granted access.
///
/// On non-macOS platforms this always returns `true` (no-op).
#[cfg(target_os = "macos")]
pub fn is_accessibility_granted() -> bool {
    // AXIsProcessTrusted returns a bool directly — true if trusted (has
    // Accessibility access), false otherwise.
    //
    // Safety: AXIsProcessTrusted is a C function that takes no arguments
    // and returns a Boolean. It is safe to call from any thread and has
    // no side effects observable from Rust.
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Non-macOS stub: accessibility is always "granted" because there's no
/// equivalent permission model on Windows/Linux for enigo keystroke injection.
#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_granted() -> bool {
    true
}

/// Emit an `app-error` event for missing Accessibility permission.
///
/// The frontend (`MainWindow.tsx`) subscribes to `app-error` with
/// `kind === "permission"` and shows a dismissable banner with a
/// deep-link into System Settings.
///
/// macOS-only, like both call sites (`clipboard.rs`, `lib.rs`): other platforms
/// have no such permission, the calls are cut out by `cfg`, and without this
/// attribute the function would hang there dead — clippy failed the build on
/// Linux.
#[cfg(target_os = "macos")]
pub fn emit_accessibility_error(app: &AppHandle) {
    let _ = app.emit(
        "app-error",
        serde_json::json!({
            "kind": "permission",
            "permission": "accessibility",
            "hint": "Privacy → Accessibility",
            "message": "Sotto нужен доступ к Специальным возможностям (Accessibility), \
                         чтобы автоматически вставлять распознанный текст в активное окно. \
                         Откройте «Системные настройки → Конфиденциальность → Специальные возможности» \
                         и разрешите доступ для приложения.",
        }),
    );
}
