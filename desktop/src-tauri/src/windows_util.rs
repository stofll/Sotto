//! Windows-only helpers for paste: capture/restore foreground HWND, release
//! stuck modifier keys, verify paste via clipboard readback.
//!
//! All public items are `cfg(windows)`; the module is empty on macOS/Linux.
//! Hotkey handler and paste pipeline run in different threads (hotkey-handler
//! thread vs. main-thread via `run_on_main_thread`), so the captured HWND
//! lives in a process-wide `Mutex` — NOT a `thread_local`.

#![cfg(windows)]

use std::sync::Mutex;

use windows_sys::Win32::Foundation::{HGLOBAL, HWND};
use windows_sys::Win32::System::DataExchange::GetClipboardData;
use windows_sys::Win32::System::DataExchange::{CloseClipboard, OpenClipboard};
use windows_sys::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, MapVirtualKeyW, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MAPVK_VK_TO_VSC, VIRTUAL_KEY, VK_CONTROL, VK_LMENU,
    VK_LSHIFT, VK_LWIN, VK_MENU, VK_RETURN, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, BringWindowToTop, GetForegroundWindow, SetForegroundWindow,
};

/// Win32 clipboard format for Unicode text. Declared as a local constant
/// rather than imported from `Win32::System::Ole` to avoid pulling in the
/// full OLE/COM feature surface just for one `#define`. The numeric value
/// (13) has been stable since Windows 2000 and is part of the Win32 ABI.
const CF_UNICODETEXT: u32 = 13;

/// Process-wide storage for the captured target HWND. Hotkey handler and
/// paste pipeline run in different threads (hotkey-handler thread vs.
/// main-thread via `run_on_main_thread`), so a `thread_local` would not
/// work — capture writes from one thread, paste reads from another.
static CAPTURED: Mutex<Option<HWND>> = Mutex::new(None);

/// All 9 modifier VKs the production `release_stuck_modifiers` iterates
/// over. `keybd_event` takes `bVk: u8` but windows-sys 0.52 exports VK
/// constants as `VIRTUAL_KEY` (a `u16` alias), so we cast at the call
/// site (`vk as u8`).
const MODIFIER_VKS: [VIRTUAL_KEY; 9] = [
    VK_CONTROL, VK_SHIFT, VK_MENU, VK_LMENU, VK_RMENU, VK_LSHIFT, VK_RSHIFT, VK_LWIN, VK_RWIN,
];

/// Snapshot the foreground window's HWND into the process-wide `Mutex`.
/// Returns `None` if there is no foreground window (e.g. service session
/// or test environment with no focused UI).
///
/// Called from the hotkey handler at the moment of Press so the paste
/// pipeline can restore focus to the user's window after the recording
/// session ends. On macOS this is a no-op (no alternative focus-tracking
/// mechanism needed — `enigo` operates on the active app directly).
pub fn capture_target_hwnd() -> Option<HWND> {
    let hwnd = unsafe { GetForegroundWindow() };
    // `windows-sys 0.52` exports `HWND` as `isize`; null is `0`.
    // (Newer `windows-sys` versions wrap HWND in a pointer type with
    // `.is_null()`; the runtime check is identical either way.)
    if hwnd == 0 {
        return None;
    }
    *CAPTURED.lock().expect("CAPTURED mutex poisoned") = Some(hwnd);
    Some(hwnd)
}

/// Read-only accessor for the captured HWND. Returns `None` if either no
/// snapshot has been taken yet or `clear_captured_hwnd` was called.
pub fn get_captured_hwnd() -> Option<HWND> {
    *CAPTURED.lock().expect("CAPTURED mutex poisoned")
}

/// Centre point of the captured window, in physical screen coordinates.
///
/// Used to decide which monitor the recording overlay belongs on: the
/// overlay should appear where the user is looking, which is the window
/// they are about to dictate into — not necessarily the primary display.
///
/// `None` when nothing was captured (recording started from the in-app
/// button rather than the hotkey) or the window has since gone away, which
/// leaves the caller on its existing fallback.
pub fn captured_window_center() -> Option<(f64, f64)> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;

    let hwnd = get_captured_hwnd()?;
    let mut rect = RECT {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    // Zero means the window is gone — a stale HWND from a closed window is
    // exactly the case this guards.
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return None;
    }
    if rect.right <= rect.left || rect.bottom <= rect.top {
        return None;
    }
    Some((
        f64::from(rect.left) + f64::from(rect.right - rect.left) / 2.0,
        f64::from(rect.top) + f64::from(rect.bottom - rect.top) / 2.0,
    ))
}

/// Drop the captured HWND. Called on the cancel path so a cancelled
/// recording does not leak a stale HWND into the next session.
pub fn clear_captured_hwnd() {
    *CAPTURED.lock().expect("CAPTURED mutex poisoned") = None;
}

/// Bring `hwnd` to the foreground across the UIPI boundary.
///
/// Strategy (best-effort, applies when the Tauri host is *not* the
/// foreground process):
///   1. Synthesize an ALT press+release — this unlocks Windows' foreground
///      lock so external processes can call `SetForegroundWindow`.
///   2. `AllowSetForegroundWindow(ASFW_ANY)` — let the target process set
///      itself foreground.
///   3. `BringWindowToTop` + `SetForegroundWindow` — actually raise.
///
/// Returns `Err("null hwnd")` if `hwnd` is null. Always returns `Ok(())`
/// from the API calls regardless of their return value — partial success
/// is preferable to aborting the whole paste pipeline.
pub fn force_focus(hwnd: HWND) -> Result<(), String> {
    if hwnd == 0 {
        return Err("null hwnd".into());
    }
    unsafe {
        // ALT-key trick to unlock the foreground-window lock.
        keybd_event(VK_MENU as u8, 0, 0, 0);
        keybd_event(VK_MENU as u8, 0, KEYEVENTF_KEYUP, 0);

        // windows-sys 0.52: `AllowSetForegroundWindow(dwProcessId: u32)`.
        // `ASFW_ANY = -1i32 as u32 = 0xFFFF_FFFF_u32` lets any process
        // set the foreground — required when the Tauri host is not itself
        // the foreground process at the moment of the call.
        AllowSetForegroundWindow(0xFFFF_FFFF);
        BringWindowToTop(hwnd);
        SetForegroundWindow(hwnd);
    }
    Ok(())
}

/// `V`. Hardcoded rather than imported because windows-sys does not
/// export letter VKs (they are just their ASCII codepoints).
const VK_V: VIRTUAL_KEY = 0x56;

/// Build one keyboard `INPUT` record for `SendInput`.
fn key_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Synthesize Ctrl+V via `SendInput` using RAW virtual-key codes.
///
/// This is Strategy 1 of the Windows paste pipeline. It exists because
/// enigo cannot do this correctly under a non-Latin keyboard layout.
/// `enigo::Key::Unicode('v')` resolves the key through `VkKeyScanW`
/// against the FOREGROUND WINDOW's layout; with a Russian layout active
/// there is no `v` to find, so enigo logs "Unable to enter the key as a
/// virtual key / Falling back to entering it as text" and injects the
/// literal character with `KEYEVENTF_UNICODE` instead.
///
/// That fallback is doubly wrong. `queue_char` emits a complete
/// down+up pair per call, and `queue_key` routes BOTH the Press and the
/// Release call into it, so one logical Ctrl+V became two whole
/// character injections. Apps that treat Ctrl + a `v` character as the
/// paste accelerator (Telegram/Qt) pasted twice; apps that ignore
/// synthetic unicode for accelerators (terminals) pasted nothing. It
/// only ever worked with a Latin layout in front.
///
/// Virtual-key codes are layout-independent, so this path behaves the
/// same whichever layout the target window has. Scan codes are filled
/// in from `MapVirtualKeyW` — the VK→VSC mapping is positional and
/// identical across layouts — because some targets read the scan code
/// rather than the VK.
///
/// Returns `Err` when `SendInput` injects fewer events than requested,
/// which is how a UIPI block surfaces. That is a real signal the caller
/// can escalate on, unlike `keybd_event` (Strategy 2), which is `void`.
pub fn send_ctrl_v_sendinput() -> Result<(), String> {
    let ctrl_scan = unsafe { MapVirtualKeyW(VK_CONTROL as u32, MAPVK_VK_TO_VSC) } as u16;
    let v_scan = unsafe { MapVirtualKeyW(VK_V as u32, MAPVK_VK_TO_VSC) } as u16;

    let inputs = [
        key_input(VK_CONTROL, ctrl_scan, 0),
        key_input(VK_V, v_scan, 0),
        key_input(VK_V, v_scan, KEYEVENTF_KEYUP),
        key_input(VK_CONTROL, ctrl_scan, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(format!(
            "SendInput injected {sent}/{} events (UIPI block?)",
            inputs.len()
        ))
    }
}

/// Synthesize a single Enter press via `SendInput`.
///
/// Used by the auto-submit setting to send the dictated text on: a chat
/// window or a search box needs Enter after the paste, and reaching for the
/// keyboard is exactly what dictation is meant to avoid.
///
/// Same raw-VK approach as [`send_ctrl_v_sendinput`], for the same reason —
/// nothing here may depend on the foreground window's keyboard layout. Enter
/// is layout-invariant either way, but the failure mode (a partial
/// `SendInput` under UIPI) is worth reporting identically.
pub fn send_enter_sendinput() -> Result<(), String> {
    let scan = unsafe { MapVirtualKeyW(VK_RETURN as u32, MAPVK_VK_TO_VSC) } as u16;
    let inputs = [
        key_input(VK_RETURN, scan, 0),
        key_input(VK_RETURN, scan, KEYEVENTF_KEYUP),
    ];
    let sent = unsafe {
        SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        )
    };
    if sent as usize == inputs.len() {
        Ok(())
    } else {
        Err(format!(
            "SendInput injected {sent}/{} events (UIPI block?)",
            inputs.len()
        ))
    }
}

/// Synthesize a Ctrl+V keystroke via the legacy `keybd_event` API.
///
/// This is Strategy 2 of the Windows paste pipeline — a fallback for when
/// the enigo SendInput path is blocked (e.g. the focus window does not
/// process `WM_KEYDOWN`/`WM_CHAR` sequences the way enigo emits them,
/// or some accessibility software is sitting between us and the input
/// stack).
///
/// The legacy `keybd_event` API is still supported on Windows 10/11 for
/// compatibility and is the documented fallback for `SendInput` issues
/// in UIPI scenarios. Produces the same VK_CONTROL + VK_V sequence as
/// Strategy 1 but bypasses enigo's higher-level key translation.
///
/// `bScan` is set to 0 for both events — Windows resolves the scan code
/// from the VK; passing an explicit scan code would force a hardware-
/// specific code that might not match the user's layout.
pub fn send_ctrl_v_keybd_event() -> Result<(), String> {
    unsafe {
        keybd_event(VK_CONTROL as u8, 0, 0, 0); // Ctrl down
        keybd_event(0x56, 0, 0, 0); // 'V' down (VK_V = 0x56)
        keybd_event(0x56, 0, KEYEVENTF_KEYUP, 0); // 'V' up
        keybd_event(VK_CONTROL as u8, 0, KEYEVENTF_KEYUP, 0); // Ctrl up
    }
    Ok(())
}

/// Synthesize a key-up event for every modifier VK in `MODIFIER_VKS`.
///
/// Rationale: if the user is holding (say) Shift when a paste failure
/// cancels mid-flight, Shift stays "logically down" in the OS. Without
/// this release the next text input is silently shifted — a subtle but
/// catastrophic UX bug. Iterating every modifier VK guarantees none
/// remain stuck regardless of which side of the keyboard the user was
/// pressing.
pub fn release_stuck_modifiers() -> Result<(), String> {
    unsafe {
        for vk in MODIFIER_VKS.iter().copied() {
            keybd_event(vk as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }
    Ok(())
}

/// Synthesize a `WM_PASTE` message targeted at `hwnd`. This is Strategy 3
/// of the Windows paste pipeline — the final fallback.
///
/// `WM_PASTE` (`0x0302`) tells the target window to paste from the system
/// clipboard directly into its own focus. Unlike Strategies 1 and 2 (which
/// emit keystrokes that the focused window sees), this message is a
/// higher-level request that the focused window's WndProc typically
/// processes regardless of the host's input privilege level — a UIPI
/// block on keystrokes does not necessarily block `WM_PASTE`.
///
/// Returns `Err("null hwnd")` for a null `hwnd`. The return value of
/// `SendMessageW` (an `LRESULT = isize`) is ignored — paste either
/// happened or it didn't, and we verify that out-of-band via
/// `clipboard_contains` in the caller.
pub fn send_wm_paste(hwnd: HWND) -> Result<(), String> {
    if hwnd == 0 {
        return Err("null hwnd".into());
    }
    let result = unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{SendMessageW, WM_PASTE};
        SendMessageW(hwnd, WM_PASTE, 0, 0)
    };
    let _ = result; // success confirmed by clipboard verification in the caller
    Ok(())
}

/// Read the current clipboard text and compare it to `expected`.
///
/// This answers exactly one question: "is `expected` on the clipboard
/// right now?" It is NOT a paste-success test. Ctrl+V does not modify
/// the clipboard, so this returns `true` after a paste that landed AND
/// after a paste that was swallowed. Its only correct use is verifying
/// our OWN write (`copy_to_clipboard`) took effect before we send the
/// keystroke — see `wait_for_clipboard_write`.
///
/// Returns `false` on every error path — clipboard unavailable, format
/// mismatch, parse failure, lock contention. Note that a `false` from
/// lock contention says nothing about the clipboard's contents: another
/// process (commonly the paste target itself, or a clipboard manager)
/// simply had it open at that instant.
pub fn clipboard_contains(expected: &str) -> bool {
    unsafe {
        // windows-sys 0.52: `OpenClipboard(hwnd: HWND) -> BOOL`. The
        // documented failure sentinel is a zero return — `.is_err()` is
        // invalid here because `BOOL = i32` (not `Result`). Passing a
        // null HWND lets any process open the clipboard.
        if OpenClipboard(0) == 0 {
            return false;
        }
        let result = match GetClipboardData(CF_UNICODETEXT) {
            0 => false,
            handle => {
                // `GetClipboardData` returns `HANDLE (= isize)`; `GlobalLock`
                // expects `HGLOBAL (= *mut c_void)`. The underlying handle
                // is the same — explicit cast.
                let hg = handle as HGLOBAL;
                let ptr = GlobalLock(hg) as *const u16;
                if ptr.is_null() {
                    false
                } else {
                    let mut len = 0usize;
                    while *ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(ptr, len);
                    let s = String::from_utf16_lossy(slice);
                    // Balance the GlobalLock. The lock count is per-handle
                    // and persists after CloseClipboard, so skipping this
                    // leaks a lock on every single readback.
                    GlobalUnlock(hg);
                    s == expected
                }
            }
        };
        let _ = CloseClipboard();
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard: a refactor that drops any modifier VK from
    /// `MODIFIER_VKS` would leave that key "stuck" on the user's
    /// keyboard after every paste — catastrophic. This test asserts the
    /// production array still covers every documented modifier constant.
    #[test]
    fn modifier_vks_covers_all_modifiers() {
        for vk in [
            VK_CONTROL, VK_SHIFT, VK_MENU, VK_LMENU, VK_RMENU, VK_LSHIFT, VK_RSHIFT, VK_LWIN,
            VK_RWIN,
        ] {
            assert!(MODIFIER_VKS.contains(&vk), "missing modifier VK 0x{:X}", vk);
        }
    }

    /// `SendInput` silently injects nothing when `cbSize` does not match
    /// the real `INPUT` layout — no error, no events, just a paste that
    /// never happens. The same class of bug bit the legacy Python
    /// implementation (32 bytes passed for a 40-byte struct). Pin the
    /// size so a windows-sys bump that changes the layout fails here
    /// instead of in the field.
    #[test]
    fn input_struct_is_the_size_sendinput_expects() {
        assert_eq!(std::mem::size_of::<INPUT>(), 40, "x64 INPUT is 40 bytes");
    }

    /// The whole point of Strategy 1 is that it does NOT go through the
    /// keyboard layout: `VK_V` must stay a raw virtual-key code, never a
    /// character lookup. A layout-dependent variant broke pasting under
    /// the Russian layout (duplicate paste in Qt apps, no paste at all in
    /// terminals).
    #[test]
    fn ctrl_v_uses_raw_virtual_key_codes() {
        assert_eq!(VK_V, 0x56, "VK_V is the ASCII codepoint of 'V'");
        let input = key_input(VK_V, 0x2F, KEYEVENTF_KEYUP);
        assert_eq!(input.r#type, INPUT_KEYBOARD);
        // SAFETY: we just built this union as the keyboard variant.
        let ki = unsafe { input.Anonymous.ki };
        assert_eq!(ki.wVk, VK_V);
        assert_eq!(ki.wScan, 0x2F);
        assert_eq!(ki.dwFlags, KEYEVENTF_KEYUP);
    }

    /// Scan-code resolution must work regardless of the active layout —
    /// VK→VSC is positional. A zero here would mean we send a scan code
    /// of 0 to targets that read it instead of the VK.
    #[test]
    fn vk_to_scan_code_resolves_for_ctrl_and_v() {
        let ctrl_scan = unsafe { MapVirtualKeyW(VK_CONTROL as u32, MAPVK_VK_TO_VSC) };
        let v_scan = unsafe { MapVirtualKeyW(VK_V as u32, MAPVK_VK_TO_VSC) };
        assert_ne!(ctrl_scan, 0, "VK_CONTROL must map to a scan code");
        assert_ne!(v_scan, 0, "VK_V must map to a scan code");
    }

    /// Exercises the `CAPTURED` Mutex round-trip directly — we cannot
    /// call `capture_target_hwnd` without a real foreground window, but
    /// the read/write/clear contract is the same code path that the
    /// production caller hits.
    #[test]
    fn capture_then_get_returns_same_hwnd() {
        clear_captured_hwnd();
        assert_eq!(get_captured_hwnd(), None);

        let fake_hwnd = 0xDEAD_BEEF_usize as HWND;
        *CAPTURED.lock().unwrap() = Some(fake_hwnd);
        assert_eq!(get_captured_hwnd(), Some(fake_hwnd));

        clear_captured_hwnd();
        assert_eq!(get_captured_hwnd(), None);
    }

    /// Sanity check: VK constants are `VIRTUAL_KEY = u16` in
    /// windows-sys 0.52 — the production `keybd_event(vk as u8, ...)`
    /// cast would silently truncate or fail to compile if that ever
    /// changes. Assert the type at the test site so the refactor would
    /// fail here first.
    #[test]
    fn vk_alias_is_u16_in_windows_sys_0_52() {
        // `VIRTUAL_KEY` is a type alias for `u16`; this compile-time
        // assignment would fail at the binding if the underlying type
        // were ever widened to `u32`.
        let _typed: u16 = VK_SHIFT;
        // And the const itself is u16.
        let _also_typed: VIRTUAL_KEY = VK_SHIFT;
    }

    /// `clipboard_contains` is the success-signal after a paste attempt.
    /// It must not panic on an empty/closed clipboard — return `false`
    /// gracefully. Hard to assert "no panic" except by running it.
    #[test]
    fn clipboard_contains_does_not_panic_when_clipboard_unavailable() {
        // In a test environment the clipboard may or may not be openable;
        // either path must be non-panicking.
        let _ = clipboard_contains("anything");
    }
}
