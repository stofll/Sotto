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
            "message": permission_message(std::env::current_exe().ok().as_deref()),
        }),
    );
}

/// The banner text for a missing Accessibility grant.
///
/// macOS grants Accessibility to the exact binary that asks for it, so
/// naming "the application" is only unambiguous for an installed bundle.
/// A build run straight from `target/` is a different TCC client than
/// `/Applications/Sotto.app`, and it is attributed to whatever launched
/// it — a terminal, typically, which is what must be granted access. A
/// message that omits this sends the reader to a Settings list where
/// Sotto is already switched on, which reads as "the permission is
/// granted and the app is broken".
///
/// Takes the path so the branch is testable; `None` falls back to the
/// bundle wording, the case that needs no extra explanation.
#[cfg(any(target_os = "macos", test))]
fn permission_message(exe: Option<&std::path::Path>) -> String {
    const BASE: &str = "Sotto нужен доступ к Специальным возможностям (Accessibility), \
                        чтобы автоматически вставлять распознанный текст в активное окно. \
                        Откройте «Системные настройки → Конфиденциальность → Специальные возможности» ";

    let unbundled = exe.filter(|path| !path.starts_with("/Applications"));
    match unbundled {
        None => format!("{BASE}и разрешите доступ для приложения."),
        Some(path) => format!(
            "{BASE}и разрешите доступ для «{}». Это сборка вне «Программ»: \
             система выдаёт доступ именно этому файлу, а если он запущен из терминала — \
             то терминалу. Включённый переключатель у другой копии Sotto здесь не поможет.",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn an_installed_bundle_is_named_simply_the_application() {
        let message = permission_message(Some(Path::new(
            "/Applications/Sotto.app/Contents/MacOS/Sotto",
        )));
        assert!(message.ends_with("разрешите доступ для приложения."));
    }

    #[test]
    fn a_build_outside_applications_names_the_binary_that_needs_the_grant() {
        // The case the old wording got wrong: Sotto.app is switched on in
        // Settings, but the running binary is a different TCC client.
        let path = "/Users/me/Project/Sotto/desktop/src-tauri/target/debug/Sotto";
        let message = permission_message(Some(Path::new(path)));
        assert!(message.contains(path));
        assert!(message.contains("терминалу"));
    }

    #[test]
    fn an_unknown_executable_falls_back_to_the_bundle_wording() {
        assert_eq!(
            permission_message(None),
            permission_message(Some(Path::new(
                "/Applications/Sotto.app/Contents/MacOS/Sotto"
            )))
        );
    }
}
