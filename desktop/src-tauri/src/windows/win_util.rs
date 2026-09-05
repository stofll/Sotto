#![cfg(windows)]

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, GWL_STYLE,
    HWND_TOPMOST, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    SW_SHOWNOACTIVATE, WM_NCACTIVATE, WM_NCCALCSIZE, WM_NCDESTROY, WM_NCPAINT, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_POPUP,
};

const DWMWA_TRANSITIONS_FORCEDISABLED: u32 = 3;
const DWMWA_CLOAK_ATTR: u32 = 13;
const DWMWA_BORDER_COLOR_ATTR: u32 = 34;
const DWMWA_COLOR_NONE: u32 = 0xFFFFFFFE;

/// Make a borderless helper window (overlay, tray popup) unfocusable and
/// strip the caption Windows would otherwise paint on it.
///
/// # The caption
///
/// tao does *not* remove `WS_CAPTION` for `decorations(false)` windows — it
/// keeps the style and suppresses the frame by zeroing the non-client area
/// in `WM_NCCALCSIZE`, which is the usual way to keep shadows and snap
/// behaviour on a borderless window. That works only as long as DWM owns
/// the non-client rendering. We used to hand it back to USER32 here with
/// `DWMWA_NCRENDERING_POLICY = DWMNCRP_DISABLED`, and USER32's legacy path
/// ignores the zeroed non-client area: it painted a classic caption bar —
/// title text, close button and all — straight over the top of the client
/// area. Captured from the live overlay window, it read "Overlay" and had a
/// working X, which is exactly the "white system window" that kept
/// appearing over the recording pill.
///
/// So: no more `DWMNCRP_DISABLED`, and the entire `WS_OVERLAPPEDWINDOW` set
/// (including `WS_THICKFRAME`) comes off. The HWND becomes a real `WS_POPUP`,
/// which has no native non-client frame to resurrect. Neither is any loss
/// here — these windows are `shadow(false)`, non-resizable and non-focusable.
///
/// `DWMWA_TRANSITIONS_FORCEDISABLED` below is a different attribute and
/// stays: it only suppresses the open/close fade.
pub unsafe fn apply_noactivate_styles(hwnd: HWND) {
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
    SetWindowLongPtrW(
        hwnd,
        GWL_STYLE,
        (style & !(WS_OVERLAPPEDWINDOW as isize)) | WS_POPUP as isize,
    );
    let exstyle = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    SetWindowLongPtrW(
        hwnd,
        GWL_EXSTYLE,
        exstyle | WS_EX_NOACTIVATE as isize | WS_EX_TOOLWINDOW as isize,
    );
    SetWindowPos(
        hwnd,
        0,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED,
    );
    let no_border = DWMWA_COLOR_NONE;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_BORDER_COLOR_ATTR,
        &no_border as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&no_border) as u32,
    );
    // Disable DWM's open/close transition animation for this window.
    // Without this, ShowWindow/HideWindow trigger a brief fade where Windows
    // composites the redirection bitmap — visible as a flash of the default
    // system-window background on borderless transparent overlays.
    let force_disabled: i32 = 1;
    DwmSetWindowAttribute(
        hwnd,
        DWMWA_TRANSITIONS_FORCEDISABLED,
        &force_disabled as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&force_disabled) as u32,
    );
}

/// Change visibility without going through tao's `WindowFlags::apply_diff`.
///
/// Tauri's `window.show()` / `window.hide()` rebuild the native style set as
/// part of the visibility transition. For the overlay that briefly restores
/// `WS_CAPTION`, so Windows can composite a one-frame system title bar even
/// though `apply_noactivate_styles` removes it again immediately afterwards.
/// Direct ShowWindow calls preserve the already-sanitised style set.
pub unsafe fn show_window_noactivate(hwnd: HWND) {
    ShowWindow(hwnd, SW_SHOWNOACTIVATE);
}

pub unsafe fn hide_window(hwnd: HWND) {
    ShowWindow(hwnd, SW_HIDE);
}

/// Exclude a window from DWM presentation without tearing down its
/// composition surface. Visibility/style transitions can then happen behind
/// the cloak and cannot expose an intermediate native caption frame.
pub unsafe fn set_window_cloaked(hwnd: HWND, cloaked: bool) -> Result<(), String> {
    let value: i32 = i32::from(cloaked);
    let result = DwmSetWindowAttribute(
        hwnd,
        DWMWA_CLOAK_ATTR,
        &value as *const _ as *const std::ffi::c_void,
        std::mem::size_of_val(&value) as u32,
    );
    if result < 0 {
        Err(format!(
            "DwmSetWindowAttribute(DWMWA_CLOAK) failed: 0x{result:08X}"
        ))
    } else {
        Ok(())
    }
}

pub unsafe fn force_topmost_noactivate(hwnd: HWND) {
    SetWindowPos(
        hwnd,
        HWND_TOPMOST,
        0,
        0,
        0,
        0,
        SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
    );
}

pub fn extract_hwnd(window: &impl HasWindowHandle) -> Result<HWND, String> {
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    match handle.as_raw() {
        RawWindowHandle::Win32(h) => Ok(h.hwnd.get()),
        _ => Err("Not a Win32 window".into()),
    }
}

// ---------------------------------------------------------------------------
// Non-client guard (issue #24).
//
// Ten attempts in the experiment log all corrected the *state* of the window:
// styles, the drawing policy, cloak, the show/hide order. Each assumed that with
// the right state a frame with the system title bar cannot happen. The title bar
// still flashes — once per process, on the first cancel.
//
// The guard changes the rule, not the state. It does not matter who restored
// `WS_CAPTION` or when (tao on the first `apply_diff`, USER32 when rebuilding the
// frame, DWM when recreating the surface): the messages that draw the non-client
// area never reach painting.
//
//   * `WM_NCCALCSIZE` → 0: the client area equals the whole window. There is no
//     non-client area at all, nowhere to draw.
//   * `WM_NCPAINT` → 0: the frame is never repainted.
//   * `WM_NCACTIVATE` → TRUE: an activation change does not repaint the title bar.
//
// tao does the same for `decorations(false)` with its own `WM_NCCALCSIZE`, but it
// does so in its own window procedure — that is, after everything that manages to
// cut in earlier. The subclass sits ahead of it.
//
// What the guard does not cure: if the frame is drawn not by our window but by a
// stand-in USER32 window (hypothesis 2 in `overlay_diag`), that window has its own
// window procedure and our subclass has nothing to do with it. Telling those two
// cases apart is exactly what `enumerate_top_level` lives in `overlay_diag` for.
// ---------------------------------------------------------------------------

/// Subclass identifier. Arbitrary, but must match when removing.
const NC_GUARD_SUBCLASS_ID: usize = 0x0024_0001;

unsafe extern "system" fn nc_guard_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    match msg {
        // wparam == TRUE means "lparam holds NCCALCSIZE_PARAMS, the first
        // rectangle is the proposed client area". By returning 0 and leaving the
        // rectangle alone we say: the client occupies the whole window.
        WM_NCCALCSIZE => 0,
        WM_NCPAINT => 0,
        WM_NCACTIVATE => 1,
        // The subclass must be removed before the window ceases to exist.
        WM_NCDESTROY => {
            RemoveWindowSubclass(hwnd, Some(nc_guard_proc), NC_GUARD_SUBCLASS_ID);
            DefSubclassProc(hwnd, msg, wparam, lparam)
        }
        _ => DefSubclassProc(hwnd, msg, wparam, lparam),
    }
}

/// Install the non-client guard on a window. Calling it twice for the same
/// window is harmless: `SetWindowSubclass` with the same (procedure, id) pair
/// replaces the entry rather than adding a second one.
pub unsafe fn install_nc_guard(hwnd: HWND) -> Result<(), String> {
    if SetWindowSubclass(hwnd, Some(nc_guard_proc), NC_GUARD_SUBCLASS_ID, 0) == 0 {
        return Err("SetWindowSubclass failed".to_string());
    }
    Ok(())
}
